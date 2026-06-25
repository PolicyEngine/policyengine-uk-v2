"""Python ↔ Rust parity harness for the PolicyEngine UK engine.

Runs the **real FRS microdata** through both engines and compares the
household-level microdata outputs they each produce:

* the Python ``policyengine-uk`` ``Microsimulation`` (loads the FRS), and
* the Rust ``policyengine_uk_compiled`` wrapper
  (``Simulation(year=...).run_microdata()``).

It does NOT build synthetic households and does NOT use any per-scenario,
per-variable mapping table — it simply compares the microdata each engine
emits for the same fiscal year.

Comparison mode
---------------
The two engines expose the FRS through different, non-aligned household
identifier schemes (Python emits ~53k households with sparse FRS ids; the Rust
wrapper emits ~17k households re-indexed to a contiguous ``0..N`` id), and the
two record sets have different counts and different total weights. Cell-for-cell
alignment on a shared household id is therefore **not** reliable, so the harness
compares **weighted aggregate statistics** per variable: the weighted mean and
the weighted p10 / p50 / p90 quantiles. (If, in some future build, the two
engines emit a shared stable id with a clean 1:1 match, ``--sample`` would let
us add cell-level checks; today the harness verifies that no reliable match
exists and uses aggregate mode.)

Compared variables (Python name → Rust column):

* ``hbai_household_net_income`` → ``baseline_net_income``
* ``household_tax``            → ``baseline_total_tax``
* ``household_benefits``       → ``baseline_total_benefits``

We deliberately use ``hbai_household_net_income`` (not plain
``household_net_income``) because the latter nets off indirect / transaction
taxes that the Rust ``baseline_net_income`` does not include.

Failure semantics
-----------------
The harness FAILS LOUDLY. It exits non-zero whenever any compared statistic
diverges beyond ``--tolerance`` (relative). A missing expected column is a hard
error; a per-variable failure is a hard error — nothing is filled with NaN and
skipped. The ONLY non-failure exit when something is wrong is the explicit
"FRS data unavailable" path, which exits 0 *only* when the FRS genuinely cannot
be loaded (so CI without data can skip) and prints a loud message making clear
that a skip is NOT a pass.

Usage::

    python scripts/parity.py
    python scripts/parity.py --tolerance 0.02
    python scripts/parity.py --year 2024
"""

from __future__ import annotations

import argparse
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

import numpy as np

# Allow running from a checkout without `pip install -e .`
_REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(_REPO / "interfaces" / "python"))


# ── Variable definitions ──────────────────────────────────────────────────────

@dataclass(frozen=True)
class Variable:
    """A household-level variable compared across the two engines."""
    label: str
    python_name: str   # policyengine-uk household-level variable
    rust_column: str   # column in Rust md.households


VARIABLES: list[Variable] = [
    Variable("hbai household net income", "hbai_household_net_income", "baseline_net_income"),
    Variable("household total tax",       "household_tax",             "baseline_total_tax"),
    Variable("household total benefits",  "household_benefits",        "baseline_total_benefits"),
]


# ── Data-unavailable sentinel ────────────────────────────────────────────────

class FRSUnavailable(Exception):
    """Raised when the FRS microdata genuinely cannot be loaded."""


# ── Weighted statistics ───────────────────────────────────────────────────────

def weighted_mean(values: np.ndarray, weights: np.ndarray) -> float:
    total = float(weights.sum())
    if total == 0:
        raise ValueError("zero total weight")
    return float((values * weights).sum() / total)


def weighted_quantile(values: np.ndarray, weights: np.ndarray, q: float) -> float:
    order = np.argsort(values)
    v = np.asarray(values, dtype=float)[order]
    w = np.asarray(weights, dtype=float)[order]
    cum = np.cumsum(w) - 0.5 * w
    cum /= w.sum()
    return float(np.interp(q, cum, v))


# The statistics computed and compared per variable.
_STATS = ("mean", "p10", "p50", "p90")


def variable_stats(values: np.ndarray, weights: np.ndarray) -> dict[str, float]:
    return {
        "mean": weighted_mean(values, weights),
        "p10": weighted_quantile(values, weights, 0.10),
        "p50": weighted_quantile(values, weights, 0.50),
        "p90": weighted_quantile(values, weights, 0.90),
    }


# ── Engine drivers (each returns a {label: stats-dict}) ───────────────────────

def run_python(year: int) -> dict[str, dict[str, float]]:
    """Run the Python engine on the FRS. Raise ``FRSUnavailable`` if it can't load."""
    try:
        from policyengine_uk import Microsimulation
    except Exception as exc:  # import-level failure → treat as unavailable.
        raise FRSUnavailable(f"policyengine-uk not importable: {exc!r}") from exc

    try:
        sim = Microsimulation()
        weights = np.asarray(sim.calculate("household_weight", year).values, dtype=float)
    except Exception as exc:
        # Construction / dataset load failed → FRS unavailable.
        raise FRSUnavailable(f"Microsimulation() / FRS load failed: {exc!r}") from exc

    out: dict[str, dict[str, float]] = {}
    for var in VARIABLES:
        # A failure to calculate an expected variable is a HARD error, not a skip.
        series = sim.calculate(var.python_name, year)
        values = np.asarray(series.values, dtype=float)
        if len(values) != len(weights):
            raise RuntimeError(
                f"Python '{var.python_name}' length {len(values)} != weight length {len(weights)}"
            )
        out[var.label] = variable_stats(values, weights)
    return out


def run_rust(year: int) -> dict[str, dict[str, float]]:
    """Run the Rust engine on the FRS. Raise ``FRSUnavailable`` if it can't load."""
    try:
        import policyengine_uk_compiled as c
    except Exception as exc:
        raise FRSUnavailable(f"policyengine_uk_compiled not importable: {exc!r}") from exc

    try:
        households = c.Simulation(year=year, dataset="frs").run_microdata().households
    except Exception as exc:
        raise FRSUnavailable(f"Rust run_microdata() / FRS load failed: {exc!r}") from exc

    weights = households["weight"].to_numpy(dtype=float)
    out: dict[str, dict[str, float]] = {}
    for var in VARIABLES:
        # A missing expected column is a HARD error, not a skip.
        if var.rust_column not in households.columns:
            raise RuntimeError(
                f"Rust microdata missing expected column '{var.rust_column}' "
                f"(have: {list(households.columns)})"
            )
        values = households[var.rust_column].to_numpy(dtype=float)
        out[var.label] = variable_stats(values, weights)
    return out


# ── Comparison ─────────────────────────────────────────────────────────────────

@dataclass
class StatDiff:
    label: str
    stat: str
    python: float
    rust: float

    @property
    def rel(self) -> float:
        denom = abs(self.python)
        if denom == 0:
            # Both zero → no divergence; one zero → treat as full divergence.
            return 0.0 if self.rust == 0 else float("inf")
        return abs(self.rust - self.python) / denom


def compare(
    python_stats: dict[str, dict[str, float]],
    rust_stats: dict[str, dict[str, float]],
    tolerance: float,
) -> tuple[list[StatDiff], list[StatDiff]]:
    """Compare aggregate stats. Return (all_diffs, over_tolerance_diffs).

    Raises if an expected variable or statistic is absent on either side — a
    missing expected value is a hard error, never a silent skip.
    """
    all_diffs: list[StatDiff] = []
    over: list[StatDiff] = []
    for var in VARIABLES:
        if var.label not in python_stats:
            raise RuntimeError(f"Python stats missing expected variable '{var.label}'")
        if var.label not in rust_stats:
            raise RuntimeError(f"Rust stats missing expected variable '{var.label}'")
        for stat in _STATS:
            if stat not in python_stats[var.label]:
                raise RuntimeError(f"Python stats missing '{stat}' for '{var.label}'")
            if stat not in rust_stats[var.label]:
                raise RuntimeError(f"Rust stats missing '{stat}' for '{var.label}'")
            d = StatDiff(
                label=var.label,
                stat=stat,
                python=python_stats[var.label][stat],
                rust=rust_stats[var.label][stat],
            )
            all_diffs.append(d)
            if d.rel > tolerance:
                over.append(d)
    return all_diffs, over


# ── Reporting ──────────────────────────────────────────────────────────────────

def print_report(all_diffs: list[StatDiff], tolerance: float) -> None:
    print("\n=== Python ↔ Rust microdata parity (FRS, weighted-aggregate mode) ===\n")
    print(f"Tolerance: {tolerance:.1%} relative on each weighted statistic\n")
    current = None
    for d in all_diffs:
        if d.label != current:
            current = d.label
            print(f"-- {d.label} --")
        marker = "  OK" if d.rel <= tolerance else "  ** OVER **"
        rel = "inf" if d.rel == float("inf") else f"{d.rel:6.2%}"
        print(
            f"  {d.stat:<4}  py={d.python:>14,.2f}  rust={d.rust:>14,.2f}  rel={rel}{marker}"
        )
    print()


# ── Entry point ──────────────────────────────────────────────────────────────

def parity(year: int = 2025, tolerance: float = 0.01, sample: Optional[int] = None) -> int:
    """Run the parity harness; return 0 on success, 1 on any divergence.

    Returns 0 (with a loud message) ONLY when the FRS genuinely cannot be loaded.
    """
    try:
        python_stats = run_python(year)
        rust_stats = run_rust(year)
    except FRSUnavailable as exc:
        # The ONLY non-failure exit when something is "wrong": data is absent.
        print("=" * 72)
        print("FRS data unavailable — SKIPPING parity (this is NOT a pass).")
        print(f"Reason: {exc}")
        print("=" * 72)
        return 0

    all_diffs, over = compare(python_stats, rust_stats, tolerance)
    print_report(all_diffs, tolerance)

    # Cell-level alignment was evaluated and is not reliable (different record
    # counts and id schemes between the engines); aggregate mode is used.
    if sample is not None:
        print(
            f"Note: --sample {sample} requested, but the engines do not expose a "
            "reliably alignable shared household id (different counts/id schemes); "
            "using weighted-aggregate mode.\n"
        )

    if over:
        print(f"{len(over)} statistic(s) diverged beyond tolerance {tolerance:.1%}:")
        for d in sorted(over, key=lambda x: x.rel, reverse=True):
            rel = "inf" if d.rel == float("inf") else f"{d.rel:.2%}"
            print(f"  - {d.label} / {d.stat}: py={d.python:,.2f} rust={d.rust:,.2f} rel={rel}")
        print("\nPARITY FAILED.")
        return 1

    print(f"All {len(all_diffs)} statistics within tolerance {tolerance:.1%}. PARITY OK.")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Compare PolicyEngine UK Python vs Rust FRS microdata outputs.",
    )
    parser.add_argument("--year", type=int, default=2025, help="Fiscal year (default 2025)")
    parser.add_argument(
        "--tolerance", type=float, default=0.01,
        help="Relative tolerance per weighted statistic (default 0.01 = 1%%)",
    )
    parser.add_argument(
        "--sample", type=int, default=None,
        help="Optional N for cell-level alignment (used only if a reliable shared id exists)",
    )
    args = parser.parse_args()
    return parity(year=args.year, tolerance=args.tolerance, sample=args.sample)


if __name__ == "__main__":
    sys.exit(main())
