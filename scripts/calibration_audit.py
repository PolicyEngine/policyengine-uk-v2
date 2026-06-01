"""Revenue / spending calibration audit: engine totals vs OBR targets.

Runs a full FRS simulation for the requested fiscal year, extracts per-program
totals from ``SimulationResult.program_breakdown``, and prints a table
comparing each total against an OBR target loaded from
``scripts/obr_<year>_<year+1>.yaml``. Flags any program whose ratio drifts
beyond a tolerance (default 15%).

This is the regenerator for the calibration table tracked in
https://github.com/PolicyEngine/policyengine-uk-rust/issues/13. The data inputs
(FRS + WAS + LCFS imputations, uprating to the target year) are whatever the
underlying ``Simulation`` is configured to load — see CLAUDE.md in the
``policyengine_uk_compiled`` package for dataset selection.

Usage::

    python scripts/calibration_audit.py
    python scripts/calibration_audit.py --year 2024
    python scripts/calibration_audit.py --tolerance 0.10 --no-fail
    python scripts/calibration_audit.py --targets path/to/custom_targets.yaml
"""

from __future__ import annotations

import argparse
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

import yaml

# Allow running from a checkout without `pip install -e .`
_REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(_REPO / "interfaces" / "python"))

from policyengine_uk_compiled import Simulation


# Programs grouped roughly as the issue body presents them: direct taxes,
# indirect / wealth taxes, then benefits. Order is purely cosmetic — the YAML
# file is the source of truth for which programs are audited.
_DISPLAY_ORDER = [
    "income_tax",
    "employee_ni",
    "employer_ni",
    "vat",
    "fuel_duty",
    "alcohol_duty",
    "tobacco_duty",
    "capital_gains_tax",
    "stamp_duty",
    "council_tax",
    "hicbc",
    "state_pension",
    "child_benefit",
    "universal_credit",
    "pension_credit",
    "housing_benefit",
    "carers_allowance",
]


@dataclass
class Row:
    program: str
    engine_b: float          # engine total in £ billions
    obr_b: Optional[float]   # OBR target in £ billions, or None if no target
    ratio: Optional[float]   # engine / obr, or None if obr is None / 0


def _default_targets_path(year: int) -> Path:
    return _REPO / "scripts" / f"obr_{year}_{(year + 1) % 100:02d}.yaml"


def _load_targets(path: Path) -> dict[str, float]:
    if not path.exists():
        raise FileNotFoundError(
            f"OBR targets file not found: {path}\n"
            f"Pass --targets to point at a different YAML, or create one mirroring "
            f"scripts/obr_2025_26.yaml.",
        )
    with path.open() as f:
        data = yaml.safe_load(f) or {}
    if not isinstance(data, dict):
        raise ValueError(f"Expected a mapping in {path}, got {type(data).__name__}")
    return {k: float(v) for k, v in data.items()}


def _build_rows(
    program_breakdown,
    targets: dict[str, float],
) -> list[Row]:
    """Convert ProgramBreakdown + targets to display rows in £ billions."""
    rows: list[Row] = []
    seen: set[str] = set()
    for program in _DISPLAY_ORDER:
        if program not in targets and not hasattr(program_breakdown, program):
            continue
        seen.add(program)
        rows.append(_make_row(program, program_breakdown, targets))

    # Surface any extra programs in the targets YAML that aren't in
    # _DISPLAY_ORDER (lets the YAML drive new comparisons without code edits).
    for program in targets:
        if program in seen:
            continue
        rows.append(_make_row(program, program_breakdown, targets))

    return rows


def _make_row(program: str, program_breakdown, targets: dict[str, float]) -> Row:
    raw = getattr(program_breakdown, program, None)
    engine_b = float(raw) / 1e9 if raw is not None else float("nan")
    obr_b = targets.get(program)
    ratio: Optional[float]
    if obr_b is None or obr_b == 0 or engine_b != engine_b:
        ratio = None
    else:
        ratio = engine_b / obr_b
    return Row(program=program, engine_b=engine_b, obr_b=obr_b, ratio=ratio)


def _fmt_b(x: float) -> str:
    if x != x:  # NaN
        return "    n/a"
    return f"£{x:>5.1f}b"


def _fmt_ratio(r: Optional[float], tolerance: float) -> str:
    if r is None:
        return "   —  "
    marker = "  " if abs(1 - r) <= tolerance else " *"
    return f"{r:>5.2f}{marker}"


def print_report(rows: list[Row], tolerance: float, year: int) -> None:
    print(f"\n=== Calibration audit (engine vs OBR, fiscal year {year}/{(year + 1) % 100:02d}) ===\n")
    print(f"  {'program':<22}{'engine':>10}{'OBR':>10}{'ratio':>10}")
    print(f"  {'-' * 20:<22}{'-' * 8:>10}{'-' * 8:>10}{'-' * 8:>10}")
    for r in rows:
        obr = "    n/a" if r.obr_b is None else f"£{r.obr_b:>5.1f}b"
        print(f"  {r.program:<22}{_fmt_b(r.engine_b):>10}{obr:>10}{_fmt_ratio(r.ratio, tolerance):>10}")
    flagged = [r for r in rows if r.ratio is not None and abs(1 - r.ratio) > tolerance]
    print()
    if flagged:
        print(f"{len(flagged)}/{len(rows)} programs drift more than {tolerance:.0%} from OBR:")
        for r in flagged:
            assert r.ratio is not None
            print(f"  - {r.program:<22} ratio={r.ratio:.2f}")
    else:
        print(f"All {len(rows)} programs within {tolerance:.0%} of OBR.")


def audit(
    year: int = 2025,
    tolerance: float = 0.15,
    targets_path: Optional[Path] = None,
    fail_on_drift: bool = True,
) -> int:
    targets_path = targets_path or _default_targets_path(year)
    targets = _load_targets(targets_path)

    sim = Simulation(year=year)
    result = sim.run()
    rows = _build_rows(result.program_breakdown, targets)
    print_report(rows, tolerance, year)

    over = [r for r in rows if r.ratio is not None and abs(1 - r.ratio) > tolerance]
    if over and fail_on_drift:
        return 1
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--year", type=int, default=2025, help="Fiscal year (default 2025)")
    parser.add_argument(
        "--tolerance", type=float, default=0.15,
        help="Per-program max |1 - engine/OBR| before flagging (default 0.15)",
    )
    parser.add_argument(
        "--targets", type=Path, default=None,
        help="Path to OBR targets YAML (default: scripts/obr_<year>_<year+1>.yaml)",
    )
    parser.add_argument(
        "--no-fail", action="store_true",
        help="Always exit 0 even when programs drift beyond tolerance",
    )
    args = parser.parse_args()
    return audit(
        year=args.year,
        tolerance=args.tolerance,
        targets_path=args.targets,
        fail_on_drift=not args.no_fail,
    )


if __name__ == "__main__":
    sys.exit(main())
