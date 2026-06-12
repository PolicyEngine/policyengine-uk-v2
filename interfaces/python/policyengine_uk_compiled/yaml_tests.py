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
   converted to input DataFrames by the harness's internal
   ``_situation_to_dataframes`` helper.

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

import pandas as pd

from policyengine_uk_compiled.engine import (
    BENUNIT_DEFAULTS,
    HOUSEHOLD_DEFAULTS,
    PERSON_DEFAULTS,
    Simulation,
)


# Region values accepted in situation dicts → canonical form the Rust engine
# expects. Accepts the upper-snake names used by the PolicyEngine web app and
# the title-case forms used by `PERSON_DEFAULTS`/`HOUSEHOLD_DEFAULTS` and the
# `parse_region` function in `src/data/clean.rs`.
_REGION_CANONICAL = {
    "NORTH_EAST": "North East",       "North East": "North East",
    "NORTH_WEST": "North West",       "North West": "North West",
    "YORKSHIRE": "Yorkshire",         "Yorkshire": "Yorkshire",
    "EAST_MIDLANDS": "East Midlands", "East Midlands": "East Midlands",
    "WEST_MIDLANDS": "West Midlands", "West Midlands": "West Midlands",
    "EAST_OF_ENGLAND": "East of England", "East of England": "East of England",
    "LONDON": "London",               "London": "London",
    "SOUTH_EAST": "South East",       "South East": "South East",
    "SOUTH_WEST": "South West",       "South West": "South West",
    "WALES": "Wales",                 "Wales": "Wales",
    "SCOTLAND": "Scotland",           "Scotland": "Scotland",
    "NORTHERN_IRELAND": "Northern Ireland", "Northern Ireland": "Northern Ireland",
}


def _resolve_period_value(value, year: int):
    """Pick a value out of a period-keyed dict, or return the scalar unchanged.

    Picks an exact match on ``year`` first, then any period whose first four
    characters match (covers ``"2025-01"`` style entries), then the most-recent
    period that is not later than ``year``, then the earliest period.
    """
    if not isinstance(value, dict):
        return value
    year_str = str(year)
    if year_str in value:
        return value[year_str]
    for k, v in value.items():
        if str(k)[:4] == year_str:
            return v
    # Numeric-period fallback
    candidates = []
    for k, v in value.items():
        try:
            candidates.append((int(str(k)[:4]), v))
        except (ValueError, TypeError):
            continue
    if not candidates:
        # Single non-period entry (e.g. {"ETERNITY": x}) → use it
        return next(iter(value.values()))
    candidates.sort()
    earlier_or_equal = [v for y, v in candidates if y <= year]
    return earlier_or_equal[-1] if earlier_or_equal else candidates[0][1]


def _situation_to_dataframes(situation: dict, year: int):
    """Convert a PolicyEngine situation-JSON dict into the three input DataFrames.

    Private helper inlined into the YAML harness so it stays self-contained.
    Supports the ``people`` / ``benunits`` / ``households`` dict shape, with
    period-keyed or scalar field values.
    """
    people = situation.get("people") or {}
    benunits = situation.get("benunits") or {}
    households = situation.get("households") or {}

    if not people:
        raise ValueError("situation must contain at least one entry under 'people'")
    if not households:
        raise ValueError("situation must contain at least one entry under 'households'")
    if not benunits:
        # Fold all people into a single implicit benunit so callers don't
        # have to supply one for trivial cases.
        benunits = {"_default": {"members": list(people.keys())}}

    person_id_map = {pid: i for i, pid in enumerate(people.keys())}
    benunit_id_map = {bid: i for i, bid in enumerate(benunits.keys())}
    household_id_map = {hid: i for i, hid in enumerate(households.keys())}

    # Build reverse lookups: person → benunit, person → household
    person_to_benunit: dict[str, str] = {}
    for bid, fields in benunits.items():
        for member in (fields.get("members") or []):
            person_to_benunit[member] = bid
    person_to_household: dict[str, str] = {}
    for hid, fields in households.items():
        for member in (fields.get("members") or []):
            person_to_household[member] = hid

    person_rows = []
    for pid, fields in people.items():
        if pid not in person_to_benunit:
            raise ValueError(f"person {pid!r} is not a member of any benunit")
        if pid not in person_to_household:
            raise ValueError(f"person {pid!r} is not a member of any household")
        row = dict(PERSON_DEFAULTS)
        row["person_id"] = person_id_map[pid]
        row["benunit_id"] = benunit_id_map[person_to_benunit[pid]]
        row["household_id"] = household_id_map[person_to_household[pid]]
        for var, val in (fields or {}).items():
            if var == "members":
                continue
            resolved = _resolve_period_value(val, year)
            if var == "gender" and isinstance(resolved, str):
                resolved = resolved.lower()
            row[var] = resolved
        person_rows.append(row)

    # Mark the first member of each benunit as benunit head, and the first
    # member of each household as household head, unless the situation
    # already specified these flags.
    seen_bu_head: set[int] = set()
    seen_hh_head: set[int] = set()
    explicit_bu_head: set[str] = set()
    explicit_hh_head: set[str] = set()
    for pid, fields in people.items():
        if "is_benunit_head" in (fields or {}):
            explicit_bu_head.add(pid)
        if "is_household_head" in (fields or {}):
            explicit_hh_head.add(pid)
    for pid, row in zip(people.keys(), person_rows):
        bu = row["benunit_id"]
        hh = row["household_id"]
        if pid in explicit_bu_head:
            seen_bu_head.add(bu)
        else:
            row["is_benunit_head"] = bu not in seen_bu_head
            if bu not in seen_bu_head:
                seen_bu_head.add(bu)
        if pid in explicit_hh_head:
            seen_hh_head.add(hh)
        else:
            row["is_household_head"] = hh not in seen_hh_head
            if hh not in seen_hh_head:
                seen_hh_head.add(hh)

    benunit_rows = []
    for bid, fields in benunits.items():
        members = fields.get("members") or []
        member_int_ids = [person_id_map[m] for m in members if m in person_id_map]
        # Single household owns this benunit — pick from the first member.
        if member_int_ids:
            owner_household = next(
                household_id_map[person_to_household[m]]
                for m in members
                if m in person_to_household
            )
        else:
            owner_household = 0
        row = dict(BENUNIT_DEFAULTS)
        row["benunit_id"] = benunit_id_map[bid]
        row["household_id"] = owner_household
        row["person_ids"] = ";".join(str(i) for i in member_int_ids)
        for var, val in (fields or {}).items():
            if var == "members":
                continue
            row[var] = _resolve_period_value(val, year)
        benunit_rows.append(row)

    household_rows = []
    for hid, fields in households.items():
        members = fields.get("members") or []
        member_int_ids = [person_id_map[m] for m in members if m in person_id_map]
        member_benunits = sorted({
            benunit_id_map[person_to_benunit[m]]
            for m in members
            if m in person_to_benunit
        })
        row = dict(HOUSEHOLD_DEFAULTS)
        row["household_id"] = household_id_map[hid]
        row["person_ids"] = ";".join(str(i) for i in member_int_ids)
        row["benunit_ids"] = ";".join(str(i) for i in member_benunits)
        for var, val in (fields or {}).items():
            if var == "members":
                continue
            resolved = _resolve_period_value(val, year)
            if var == "region" and isinstance(resolved, str):
                resolved = _REGION_CANONICAL.get(resolved, resolved)
            row[var] = resolved
        household_rows.append(row)

    # Propagate `is_in_scotland` from each person's household region unless
    # the situation already set it explicitly.
    region_by_household = {h["household_id"]: h.get("region") for h in household_rows}
    explicit_in_scotland = {
        pid for pid, fields in people.items()
        if "is_in_scotland" in (fields or {})
    }
    for pid, row in zip(people.keys(), person_rows):
        if pid in explicit_in_scotland:
            continue
        row["is_in_scotland"] = region_by_household.get(row["household_id"]) == "Scotland"

    return (
        pd.DataFrame(person_rows),
        pd.DataFrame(benunit_rows),
        pd.DataFrame(household_rows),
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
    # Hypothetical households default to full benefit take-up; set false to
    # suppress modelled means-tested benefits (e.g. to isolate passthroughs).
    full_take_up: bool = True

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
            full_take_up=bool(raw.get("full_take_up", True)),
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
    ``on_uc``).
    """
    benunit_keys = {"is_lone_parent", "rent_monthly", "on_uc"}
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
    sim = Simulation(
        year=case.period, persons=persons, benunits=benunits,
        households=households, full_take_up=case.full_take_up,
    )
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
