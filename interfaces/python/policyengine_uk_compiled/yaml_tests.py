"""YAML-based policy test runner for the Rust engine.

Mirrors the format used by `policyengine_uk/tests/policy/`, so YAML test
cases can be ported across one at a time.

A YAML file contains a list of cases::

    - name: A descriptive name
      period: 2025                # year
      absolute_error_margin: 1    # optional, default 1.0
      relative_error_margin: 0.01 # optional
      input:
        employment_income: 50000  # flat single-person shorthand
      output:
        baseline_income_tax: 7486

The ``input`` section can be either:

1. **Flat** (single-person shorthand) — variable names map to the wrapper's
   input columns and a one-person household is auto-built. Adding a
   ``region`` key sets the household's region.

2. **Full situation** — a dict with ``people``, ``benunits``, ``households``,
   matching :func:`Simulation.from_situation`.

Output names match the columns Rust microdata exposes (e.g.
``baseline_income_tax`` on persons, ``baseline_universal_credit`` on
benunits, ``baseline_net_income`` on households). Each output is summed
over all rows of the table it lives in, so single-person scenarios just
get the per-person value.
"""

from __future__ import annotations

import argparse
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional

from policyengine_uk_compiled.engine import (
    Simulation,
    _situation_to_dataframes,
)


# Map output variable names → which microdata table they live in.
# Order matters: persons checked first, then benunits, then households.
_OUTPUT_TABLES = ("persons", "benunits", "households")


@dataclass
class YamlTestCase:
    name: str
    period: int
    input: dict
    output: dict
    absolute_error_margin: float = 1.0
    relative_error_margin: Optional[float] = None
    file: Optional[str] = None

    @classmethod
    def from_dict(cls, raw: dict, file: Optional[str] = None) -> "YamlTestCase":
        if "name" not in raw:
            raise ValueError(f"YAML test case missing 'name' (file={file})")
        if "period" not in raw:
            raise ValueError(f"{raw.get('name')!r} missing 'period'")
        if "input" not in raw or "output" not in raw:
            raise ValueError(f"{raw.get('name')!r} needs both 'input' and 'output'")
        return cls(
            name=raw["name"],
            period=int(raw["period"]),
            input=dict(raw["input"]),
            output=dict(raw["output"]),
            absolute_error_margin=float(raw.get("absolute_error_margin", 1.0)),
            relative_error_margin=(
                float(raw["relative_error_margin"])
                if "relative_error_margin" in raw else None
            ),
            file=file,
        )


@dataclass
class YamlTestResult:
    case: YamlTestCase
    passed: bool
    actual: dict
    failures: list[str]


def _is_full_situation(input_dict: dict) -> bool:
    """A situation has any of the three entity-keyed top-level keys."""
    return any(k in input_dict for k in ("people", "benunits", "households"))


def _flat_input_to_situation(flat: dict) -> dict:
    """Wrap a flat single-person input dict in a full situation.

    Treats every key as a person-level field except ``region`` (household-level)
    and a few benunit-only flags (``is_lone_parent``, ``rent_monthly``,
    ``would_claim_*``, ``on_uc``, ``on_legacy``).
    """
    benunit_keys = {
        "is_lone_parent", "rent_monthly", "on_uc", "on_legacy",
        "would_claim_uc", "would_claim_cb", "would_claim_hb",
        "would_claim_pc", "would_claim_ctc", "would_claim_wtc",
        "would_claim_is", "would_claim_esa", "would_claim_jsa",
    }
    household_keys = {"region", "rent_annual", "council_tax_annual", "weight"}

    person_fields: dict = {}
    benunit_fields: dict = {}
    household_fields: dict = {"members": ["you"]}
    for key, val in flat.items():
        if key in household_keys:
            household_fields[key] = val
        elif key in benunit_keys:
            benunit_fields[key] = val
        else:
            person_fields[key] = val

    benunit_fields["members"] = ["you"]

    return {
        "people":     {"you": person_fields},
        "benunits":   {"b": benunit_fields},
        "households": {"h": household_fields},
    }


def _run_case(case: YamlTestCase) -> YamlTestResult:
    """Execute one YAML test case against the Rust engine."""
    if _is_full_situation(case.input):
        situation = case.input
    else:
        situation = _flat_input_to_situation(case.input)

    # Pre-build the DataFrames (validates the situation early).
    persons, benunits, households = _situation_to_dataframes(situation, case.period)
    sim = Simulation(year=case.period, persons=persons, benunits=benunits, households=households)
    micro = sim.run_microdata()
    tables = {"persons": micro.persons, "benunits": micro.benunits, "households": micro.households}

    actual: dict[str, float] = {}
    failures: list[str] = []
    for var, expected in case.output.items():
        # Find which table holds the column.
        for table_name in _OUTPUT_TABLES:
            df = tables[table_name]
            if var in df.columns:
                got = float(df[var].sum())
                actual[var] = got
                break
        else:
            failures.append(
                f"{var!r}: column not found in any output table "
                f"(persons/benunits/households)"
            )
            continue

        if not _within_tolerance(got, expected, case):
            failures.append(
                f"{var!r}: expected {expected}, got {got:.4f} "
                f"(diff={got - float(expected):+.4f}, "
                f"tol=abs:{case.absolute_error_margin}"
                + (f", rel:{case.relative_error_margin}" if case.relative_error_margin else "")
                + ")"
            )

    return YamlTestResult(case=case, passed=not failures, actual=actual, failures=failures)


def _within_tolerance(got: float, expected: Any, case: YamlTestCase) -> bool:
    # Booleans are int subclasses in Python; compare exactly so a 1-£ tolerance
    # doesn't make False ≈ True.
    if isinstance(expected, bool) or isinstance(got, bool):
        return bool(got) == bool(expected)
    try:
        e = float(expected)
    except (TypeError, ValueError):
        return got == expected  # non-numeric string — exact match
    abs_ok = abs(got - e) <= case.absolute_error_margin
    if case.relative_error_margin is None or e == 0:
        return abs_ok
    rel_ok = abs(got - e) / abs(e) <= case.relative_error_margin
    return abs_ok or rel_ok


def load_yaml_file(path: Path) -> list[YamlTestCase]:
    """Load all cases from a single YAML file."""
    import yaml
    with open(path) as f:
        raw = yaml.safe_load(f) or []
    if not isinstance(raw, list):
        raise ValueError(f"{path}: top level must be a list of test cases")
    return [YamlTestCase.from_dict(c, file=str(path)) for c in raw]


def discover_cases(root: Path) -> list[YamlTestCase]:
    """Recursively load every YAML test case under ``root``."""
    cases: list[YamlTestCase] = []
    for path in sorted(root.rglob("*.yaml")):
        cases.extend(load_yaml_file(path))
    return cases


def run_cases(cases: list[YamlTestCase]) -> list[YamlTestResult]:
    """Run every case and return per-case results."""
    return [_run_case(c) for c in cases]


def _print_results(results: list[YamlTestResult]) -> None:
    n_pass = sum(1 for r in results if r.passed)
    n_fail = len(results) - n_pass
    for r in results:
        status = "PASS" if r.passed else "FAIL"
        loc = f" [{Path(r.case.file).name}]" if r.case.file else ""
        print(f"  {status}  {r.case.name}{loc}")
        for f in r.failures:
            print(f"        ✗ {f}")
    print(f"\n{n_pass} passed, {n_fail} failed of {len(results)}")


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        description="Run YAML policy tests against the Rust engine."
    )
    parser.add_argument(
        "path", nargs="?", default="tests/policy",
        help="YAML file or directory of YAML files (default: tests/policy)",
    )
    args = parser.parse_args(argv)

    target = Path(args.path)
    if not target.exists():
        print(f"error: {target} does not exist", file=sys.stderr)
        return 2
    cases = (
        load_yaml_file(target) if target.is_file() else discover_cases(target)
    )
    if not cases:
        print(f"warning: no YAML test cases found under {target}")
        return 0

    results = run_cases(cases)
    _print_results(results)
    return 0 if all(r.passed for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
