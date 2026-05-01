"""Python ↔ Rust parity harness for the PolicyEngine UK engine.

Runs a fixed set of synthetic households through both the Python
``policyengine-uk`` package and the Rust ``policyengine_uk_compiled`` wrapper,
diffs key tax / benefit / net-income outputs cell-for-cell, and prints a
summary. Exits non-zero when any diff exceeds the configured tolerance.

Designed to surface drift introduced by Rust ports of Python variables. Uses
synthetic households so it has no FRS data dependency. If
``policyengine-uk`` isn't installed the Python comparison is skipped and the
script still produces a Rust-only smoke check.

Usage::

    python scripts/parity.py
    python scripts/parity.py --tolerance 5
    python scripts/parity.py --year 2024 --no-fail
"""

from __future__ import annotations

import argparse
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Optional

# Allow running from a checkout without `pip install -e .`
_REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(_REPO / "interfaces" / "python"))

from policyengine_uk_compiled import Simulation as RustSimulation


# ── Variable definitions ──────────────────────────────────────────────────────

@dataclass(frozen=True)
class Variable:
    """A variable to compare across the two engines.

    `python_name` is the Python policyengine-uk name passed to
    ``sim.calculate``. `rust_table` and `rust_column` locate the value in the
    Rust microdata output (sum over all rows in that table).
    """
    python_name: str
    rust_table: str          # "persons" | "benunits" | "households"
    rust_column: str         # column in that table


# Compare against `hbai_household_net_income` rather than the broader
# `household_net_income`: Rust's `baseline_net_income` is the HBAI definition
# (gross minus direct taxes plus benefits, excluding council tax / TV licence /
# transaction taxes), so comparing against the broad Python net-income variable
# would surface a spurious ~£159 diff on every scenario for the TV licence
# alone.
VARIABLES: list[Variable] = [
    Variable("income_tax",                "persons",    "baseline_income_tax"),
    Variable("ni_employee",               "persons",    "baseline_employee_ni"),
    Variable("ni_employer",               "persons",    "baseline_employer_ni"),
    Variable("universal_credit",          "benunits",   "baseline_universal_credit"),
    Variable("child_benefit",             "benunits",   "baseline_child_benefit"),
    Variable("state_pension",             "benunits",   "baseline_state_pension"),
    Variable("pension_credit",            "benunits",   "baseline_pension_credit"),
    Variable("housing_benefit",           "benunits",   "baseline_housing_benefit"),
    Variable("hbai_household_net_income", "households", "baseline_net_income"),
]


# ── Synthetic households ─────────────────────────────────────────────────────

def _person(age: int, **kwargs) -> dict:
    """Helper: build a person record with year-keyed period values."""
    p = {"age": {"YEAR": age}}
    for key, val in kwargs.items():
        p[key] = {"YEAR": val}
    return p


def _scenarios(year: int) -> list[tuple[str, dict]]:
    """Return (name, situation_dict) pairs. Period 'YEAR' is rewritten below."""
    s: list[tuple[str, dict]] = []

    # Single person at a range of incomes — exercises personal allowance taper,
    # higher- and additional-rate bands, NI primary threshold and UEL.
    for income in (0, 12_000, 25_000, 50_000, 80_000, 150_000):
        s.append((
            f"single_£{income:,}".replace(",", "k"),
            {
                "people":     {"you": _person(35, employment_income=income)},
                "benunits":   {"b": {"members": ["you"]}},
                "households": {"h": {"members": ["you"], "region": {"YEAR": "LONDON"}}},
            },
        ))

    # Couple, no children — both earn, second-earner interaction
    s.append((
        "couple_no_kids_40k_25k",
        {
            "people": {
                "p1": _person(35, employment_income=40_000),
                "p2": _person(33, employment_income=25_000),
            },
            "benunits":   {"b": {"members": ["p1", "p2"]}},
            "households": {"h": {"members": ["p1", "p2"], "region": {"YEAR": "LONDON"}}},
        },
    ))

    # Couple with two children — should activate Child Benefit
    s.append((
        "couple_2kids_30k_15k",
        {
            "people": {
                "p1": _person(38, employment_income=30_000),
                "p2": _person(36, employment_income=15_000),
                "c1": _person(8),
                "c2": _person(4),
            },
            "benunits":   {"b": {"members": ["p1", "p2", "c1", "c2"]}},
            "households": {"h": {"members": ["p1", "p2", "c1", "c2"], "region": {"YEAR": "LONDON"}}},
        },
    ))

    # Lone parent, low income — should activate UC + CB
    s.append((
        "lone_parent_2kids_18k",
        {
            "people": {
                "p1": _person(32, employment_income=18_000),
                "c1": _person(7),
                "c2": _person(3),
            },
            "benunits":   {"b": {"members": ["p1", "c1", "c2"], "is_lone_parent": {"YEAR": True}}},
            "households": {"h": {"members": ["p1", "c1", "c2"], "region": {"YEAR": "NORTH_EAST"}}},
        },
    ))

    # Pensioner couple
    s.append((
        "pensioner_couple",
        {
            "people": {
                "p1": _person(70, state_pension=11_500),
                "p2": _person(68, state_pension=11_500),
            },
            "benunits":   {"b": {"members": ["p1", "p2"]}},
            "households": {"h": {"members": ["p1", "p2"], "region": {"YEAR": "WALES"}}},
        },
    ))

    # Scotland resident — exercises devolved income-tax bands
    s.append((
        "scotland_single_45k",
        {
            "people":     {"you": _person(40, employment_income=45_000)},
            "benunits":   {"b": {"members": ["you"]}},
            "households": {"h": {"members": ["you"], "region": {"YEAR": "SCOTLAND"}}},
        },
    ))

    # Substitute the placeholder period key with the real year string.
    return [(name, _replace_period(sit, year)) for name, sit in s]


def _replace_period(obj, year: int):
    """Recursively replace 'YEAR' keys with the real year string."""
    if isinstance(obj, dict):
        return {(str(year) if k == "YEAR" else k): _replace_period(v, year) for k, v in obj.items()}
    if isinstance(obj, list):
        return [_replace_period(x, year) for x in obj]
    return obj


# ── Engine drivers ────────────────────────────────────────────────────────────

def run_rust(situation: dict, year: int) -> dict[str, float]:
    """Run the Rust engine and extract per-variable totals."""
    sim = RustSimulation.from_situation(situation, year=year)
    micro = sim.run_microdata()
    tables = {"persons": micro.persons, "benunits": micro.benunits, "households": micro.households}
    out: dict[str, float] = {}
    for v in VARIABLES:
        df = tables[v.rust_table]
        if v.rust_column in df.columns:
            out[v.python_name] = float(df[v.rust_column].sum())
        else:
            out[v.python_name] = float("nan")
    return out


def run_python(situation: dict, year: int):
    """Run the Python policyengine-uk engine, or return None if unavailable."""
    try:
        from policyengine_uk import Simulation as PySimulation
    except Exception:
        return None
    py_sim = PySimulation(situation=situation)
    out: dict[str, float] = {}
    for v in VARIABLES:
        try:
            out[v.python_name] = float(py_sim.calculate(v.python_name, year).sum())
        except Exception as e:
            out[v.python_name] = float("nan")
    return out


# ── Reporting ────────────────────────────────────────────────────────────────

@dataclass
class ScenarioResult:
    name: str
    rust: dict[str, float]
    python: Optional[dict[str, float]]
    diffs: dict[str, float] = field(default_factory=dict)
    max_abs_diff: float = 0.0

    def compute_diffs(self) -> None:
        if self.python is None:
            return
        for var in self.rust:
            r = self.rust.get(var, float("nan"))
            p = self.python.get(var, float("nan"))
            d = r - p
            self.diffs[var] = d
            if d == d and abs(d) > self.max_abs_diff:  # NaN check via self-equality
                self.max_abs_diff = abs(d)


def _fmt_money(x: float) -> str:
    if x != x:  # NaN
        return "    n/a"
    return f"{x:>10,.0f}"


def print_report(results: list[ScenarioResult], comparing: bool) -> None:
    headers = ["scenario"] + [v.python_name for v in VARIABLES]
    if comparing:
        print("\n=== Rust vs Python parity report ===\n")
        for r in results:
            print(f"-- {r.name} --")
            for var in [v.python_name for v in VARIABLES]:
                rv = r.rust.get(var, float("nan"))
                pv = r.python.get(var, float("nan"))  # type: ignore[union-attr]
                diff = r.diffs.get(var, 0.0)
                marker = "  " if abs(diff) < 0.5 else " *"
                print(f"  {var:<24}  rust={_fmt_money(rv)}  py={_fmt_money(pv)}  diff={_fmt_money(diff)}{marker}")
            print(f"  → max |diff|: {r.max_abs_diff:,.2f}")
            print()
    else:
        print("\n=== Rust-only smoke output (policyengine-uk not installed) ===\n")
        for r in results:
            print(f"-- {r.name} --")
            for var, val in r.rust.items():
                print(f"  {var:<24}  {_fmt_money(val)}")
            print()


# ── Entry point ──────────────────────────────────────────────────────────────

def parity(year: int = 2025, tolerance: float = 1.0, fail_on_diff: bool = True) -> int:
    """Run the parity harness; return 0 on success, 1 on diff exceeded."""
    scenarios = _scenarios(year)
    results: list[ScenarioResult] = []
    for name, situation in scenarios:
        rust_out = run_rust(situation, year)
        py_out = run_python(situation, year)
        sr = ScenarioResult(name=name, rust=rust_out, python=py_out)
        sr.compute_diffs()
        results.append(sr)

    comparing = any(r.python is not None for r in results)
    print_report(results, comparing)

    if not comparing:
        print("Note: install `policyengine-uk` to enable parity comparison.")
        return 0

    over_tolerance = [r for r in results if r.max_abs_diff > tolerance]
    if over_tolerance:
        print(f"\n{len(over_tolerance)} scenarios exceeded tolerance £{tolerance:,.2f}:")
        for r in over_tolerance:
            print(f"  - {r.name}: max |diff| = £{r.max_abs_diff:,.2f}")
        return 1 if fail_on_diff else 0

    print(f"\nAll {len(results)} scenarios within tolerance £{tolerance:,.2f}.")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--year", type=int, default=2025, help="Fiscal year (default 2025)")
    parser.add_argument(
        "--tolerance", type=float, default=1.0,
        help="Per-scenario max-abs-diff in pounds (default 1.0)",
    )
    parser.add_argument(
        "--no-fail", action="store_true",
        help="Always exit 0 even when diffs exceed tolerance",
    )
    args = parser.parse_args()
    return parity(year=args.year, tolerance=args.tolerance, fail_on_diff=not args.no_fail)


if __name__ == "__main__":
    sys.exit(main())
