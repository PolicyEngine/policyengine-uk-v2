"""Interface to the compiled PolicyEngine UK Rust binary."""

from __future__ import annotations

import io
import json
import subprocess
from pathlib import Path
from typing import Optional, Union

try:
    import pandas as pd
    HAS_PANDAS = True
except ImportError:
    HAS_PANDAS = False

from policyengine_uk_compiled.models import MicrodataResult, Parameters, SimulationResult, HbaiIncomes, PovertyHeadcounts
from policyengine_uk_compiled.structural import StructuralReform, aggregate_microdata

# The binary and parameters/ dir are bundled inside the package at build time.
_PKG_DIR = Path(__file__).resolve().parent
_BUNDLED_BINARY = _PKG_DIR / "bin" / "policyengine-uk-rust"

# Default column schemas with sensible defaults for hypothetical households.
PERSON_DEFAULTS = {
    "person_id": 0, "benunit_id": 0, "household_id": 0,
    "age": 30, "gender": "male",
    "is_benunit_head": True, "is_household_head": True,
    "employment_income": 0.0, "self_employment_income": 0.0,
    "private_pension_income": 0.0, "state_pension": 0.0,
    "savings_interest": 0.0, "dividend_income": 0.0,
    "property_income": 0.0, "maintenance_income": 0.0,
    "miscellaneous_income": 0.0, "other_income": 0.0,
    "is_in_scotland": False, "hours_worked_annual": 0.0,
}

BENUNIT_DEFAULTS = {
    "benunit_id": 0, "household_id": 0, "person_ids": "0",
    "migration_seed": 0.0, "on_uc": False, "on_legacy": False,
    "rent_monthly": 0.0, "is_lone_parent": False,
    "would_claim_uc": True, "would_claim_cb": True,
    "would_claim_hb": True, "would_claim_pc": True,
    "would_claim_ctc": True, "would_claim_wtc": True,
    "would_claim_is": True, "would_claim_esa": True,
    "would_claim_jsa": True,
}

HOUSEHOLD_DEFAULTS = {
    "household_id": 0, "benunit_ids": "0", "person_ids": "0",
    "weight": 1.0, "region": "London",
    "rent_annual": 0.0, "council_tax_annual": 0.0,
}


def _find_binary() -> str:
    """Locate the policyengine-uk-rust binary."""
    if _BUNDLED_BINARY.is_file():
        return str(_BUNDLED_BINARY)
    # Walk up from package dir to find the repo root containing target/
    candidate = _PKG_DIR.parent
    for _ in range(5):
        for subdir in ("target/release", "target/debug"):
            p = candidate / subdir / "policyengine-uk-rust"
            if p.is_file():
                return str(p)
        candidate = candidate.parent
    raise FileNotFoundError(
        "Cannot find policyengine-uk-rust binary. "
        "Install the package (`pip install policyengine-uk-compiled`) "
        "or build from source (`cargo build --release`)."
    )


def _find_cwd(binary_path: str) -> str:
    """Find the working directory that contains parameters/."""
    if (_PKG_DIR / "parameters").is_dir():
        return str(_PKG_DIR)
    binary = Path(binary_path).resolve()
    for ancestor in (binary.parent, binary.parent.parent, binary.parent.parent.parent):
        if (ancestor / "parameters").is_dir():
            return str(ancestor)
    raise FileNotFoundError("Cannot find parameters/ directory.")


def _df_to_csv(df) -> str:
    """Convert a DataFrame to CSV string."""
    return df.to_csv(index=False)


def _build_stdin_payload(persons_csv: str, benunits_csv: str, households_csv: str) -> str:
    """Build the concatenated CSV protocol payload."""
    return (
        "===PERSONS===\n" + persons_csv +
        "===BENUNITS===\n" + benunits_csv +
        "===HOUSEHOLDS===\n" + households_csv
    )


def _parse_stdin_payload(payload: str):
    """Parse a stdin protocol payload back into three DataFrames."""
    import io
    import pandas as pd
    sections: dict[str, str] = {}
    current_name = None
    current_lines: list[str] = []
    for line in payload.split("\n"):
        if line.startswith("===") and line.endswith("==="):
            if current_name is not None:
                sections[current_name] = "\n".join(current_lines)
            current_name = line.strip("=").lower()
            current_lines = []
        else:
            current_lines.append(line)
    if current_name is not None:
        sections[current_name] = "\n".join(current_lines)
    return (
        pd.read_csv(io.StringIO(sections.get("persons", ""))),
        pd.read_csv(io.StringIO(sections.get("benunits", ""))),
        pd.read_csv(io.StringIO(sections.get("households", ""))),
    )


# Region values accepted in situation dicts → canonical form the Rust engine expects.
# Accepts the upper-snake names used by the PolicyEngine web app and the
# title-case forms used by `PERSON_DEFAULTS`/`HOUSEHOLD_DEFAULTS` and the
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

    See ``Simulation.from_situation`` for the supported dict shape.
    """
    if not HAS_PANDAS:
        raise ImportError("pandas is required for from_situation")

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


def _parse_microdata_stdout(raw: str) -> MicrodataResult:
    """Parse the concatenated CSV protocol output into a MicrodataResult."""
    sections = {}
    current_name = None
    current_lines = []
    for line in raw.split("\n"):
        if line.startswith("===") and line.endswith("==="):
            if current_name is not None:
                sections[current_name] = "\n".join(current_lines)
            current_name = line.strip("=").lower()
            current_lines = []
        else:
            current_lines.append(line)
    if current_name is not None:
        sections[current_name] = "\n".join(current_lines)
    dfs = {
        name: pd.read_csv(io.StringIO(csv_text))
        for name, csv_text in sections.items()
        if csv_text.strip()
    }
    return MicrodataResult(
        persons=dfs.get("persons", pd.DataFrame()),
        benunits=dfs.get("benunits", pd.DataFrame()),
        households=dfs.get("households", pd.DataFrame()),
    )


def _aggregate_persons_only(records: list[dict], year: int) -> SimulationResult:
    """Aggregate person-level records (from --persons-only) into a SimulationResult.

    Persons-only datasets (e.g. SPI) only have income tax and NI.
    Household/benefit fields are zeroed.
    """
    from policyengine_uk_compiled.models import (
        BudgetaryImpact, IncomeBreakdown, ProgramBreakdown, Caseloads,
        DecileImpact, WinnersLosers,
    )

    total_baseline_tax = 0.0
    total_reform_tax = 0.0
    total_baseline_ni = 0.0
    total_reform_ni = 0.0
    total_baseline_employer_ni = 0.0
    total_reform_employer_ni = 0.0
    total_employment = 0.0
    total_self_employment = 0.0
    total_pension = 0.0
    total_savings = 0.0
    total_dividend = 0.0

    weighted_records = []
    for r in records:
        w = r["weight"]
        b_tax = r["baseline_income_tax"] * w
        r_tax = r["reform_income_tax"] * w
        b_ni = r["baseline_employee_ni"] * w
        r_ni = r["reform_employee_ni"] * w
        b_eni = r["baseline_employer_ni"] * w
        r_eni = r["reform_employer_ni"] * w

        total_baseline_tax += b_tax
        total_reform_tax += r_tax
        total_baseline_ni += b_ni
        total_reform_ni += r_ni
        total_baseline_employer_ni += b_eni
        total_reform_employer_ni += r_eni
        total_employment += r["employment_income"] * w
        total_self_employment += r["self_employment_income"] * w
        total_pension += r["pension_income"] * w
        total_savings += r["savings_interest_income"] * w
        total_dividend += r["dividend_income"] * w

        baseline_total = r["baseline_income_tax"] + r["baseline_employee_ni"]
        reform_total = r["reform_income_tax"] + r["reform_employee_ni"]
        weighted_records.append((w, r["employment_income"], baseline_total, reform_total))

    baseline_revenue = total_baseline_tax + total_baseline_ni + total_baseline_employer_ni
    reform_revenue = total_reform_tax + total_reform_ni + total_reform_employer_ni

    # Decile analysis by employment income
    weighted_records.sort(key=lambda x: x[1])
    n = len(weighted_records)
    decile_size = n // 10
    decile_impacts = []
    for d in range(10):
        start = d * decile_size
        end = n if d == 9 else (d + 1) * decile_size
        sl = weighted_records[start:end]
        count = len(sl)
        if count == 0:
            decile_impacts.append(DecileImpact(decile=d + 1))
            continue
        avg_base = sum(r[2] for r in sl) / count
        avg_reform = sum(r[3] for r in sl) / count
        avg_change = avg_reform - avg_base
        pct_change = 100.0 * avg_change / avg_base if avg_base != 0 else 0.0
        decile_impacts.append(DecileImpact(
            decile=d + 1,
            avg_baseline_income=round(avg_base, 2),
            avg_reform_income=round(avg_reform, 2),
            avg_change=round(avg_change, 2),
            pct_change=round(pct_change, 2),
        ))

    # Winners/losers
    winners_w = losers_w = unchanged_w = total_gain = total_loss = 0.0
    for w, _, bt, rt in weighted_records:
        change = rt - bt  # positive = more tax = loss
        net_change = -change  # income perspective
        if net_change > 1.0:
            winners_w += w
            total_gain += w * net_change
        elif net_change < -1.0:
            losers_w += w
            total_loss += w * abs(net_change)
        else:
            unchanged_w += w
    total_w = winners_w + losers_w + unchanged_w

    fiscal_year = f"{year}/{(year + 1) % 100:02d}"

    return SimulationResult(
        fiscal_year=fiscal_year,
        budgetary_impact=BudgetaryImpact(
            baseline_revenue=baseline_revenue,
            reform_revenue=reform_revenue,
            revenue_change=reform_revenue - baseline_revenue,
            baseline_benefits=0.0,
            reform_benefits=0.0,
            benefit_spending_change=0.0,
            net_cost=-(reform_revenue - baseline_revenue),
        ),
        income_breakdown=IncomeBreakdown(
            employment_income=total_employment,
            self_employment_income=total_self_employment,
            pension_income=total_pension,
            savings_interest_income=total_savings,
            dividend_income=total_dividend,
            property_income=0.0,
            other_income=0.0,
        ),
        program_breakdown=ProgramBreakdown(
            income_tax=total_reform_tax,
            employee_ni=total_reform_ni,
            employer_ni=total_reform_employer_ni,
            universal_credit=0.0, child_benefit=0.0, state_pension=0.0,
            pension_credit=0.0, housing_benefit=0.0, child_tax_credit=0.0,
            working_tax_credit=0.0, income_support=0.0, esa_income_related=0.0,
            jsa_income_based=0.0, carers_allowance=0.0,
            scottish_child_payment=0.0, benefit_cap_reduction=0.0,
            passthrough_benefits=0.0,
        ),
        caseloads=Caseloads(
            income_tax_payers=sum(r["weight"] for r in records if r["reform_income_tax"] > 0),
            ni_payers=sum(r["weight"] for r in records if r["reform_employee_ni"] > 0),
            employer_ni_payers=sum(r["weight"] for r in records if r["reform_employer_ni"] > 0),
            universal_credit=0.0, child_benefit=0.0, state_pension=0.0,
            pension_credit=0.0, housing_benefit=0.0, child_tax_credit=0.0,
            working_tax_credit=0.0, income_support=0.0, esa_income_related=0.0,
            jsa_income_based=0.0, carers_allowance=0.0,
            scottish_child_payment=0.0, benefit_cap_affected=0.0,
        ),
        decile_impacts=decile_impacts,
        winners_losers=WinnersLosers(
            winners_pct=round(100.0 * winners_w / total_w, 1) if total_w > 0 else 0.0,
            losers_pct=round(100.0 * losers_w / total_w, 1) if total_w > 0 else 0.0,
            unchanged_pct=round(100.0 * unchanged_w / total_w, 1) if total_w > 0 else 0.0,
            avg_gain=round(total_gain / winners_w) if winners_w > 0 else 0.0,
            avg_loss=round(total_loss / losers_w) if losers_w > 0 else 0.0,
        ),
        baseline_hbai_incomes=HbaiIncomes(
            mean_equiv_bhc=0.0, mean_equiv_ahc=0.0,
            mean_bhc=0.0, mean_ahc=0.0,
            median_equiv_bhc=0.0, median_equiv_ahc=0.0,
        ),
        reform_hbai_incomes=HbaiIncomes(
            mean_equiv_bhc=0.0, mean_equiv_ahc=0.0,
            mean_bhc=0.0, mean_ahc=0.0,
            median_equiv_bhc=0.0, median_equiv_ahc=0.0,
        ),
        baseline_poverty=PovertyHeadcounts(
            relative_bhc_children=0.0, relative_bhc_working_age=0.0, relative_bhc_pensioners=0.0,
            relative_ahc_children=0.0, relative_ahc_working_age=0.0, relative_ahc_pensioners=0.0,
            absolute_bhc_children=0.0, absolute_bhc_working_age=0.0, absolute_bhc_pensioners=0.0,
            absolute_ahc_children=0.0, absolute_ahc_working_age=0.0, absolute_ahc_pensioners=0.0,
        ),
        reform_poverty=PovertyHeadcounts(
            relative_bhc_children=0.0, relative_bhc_working_age=0.0, relative_bhc_pensioners=0.0,
            relative_ahc_children=0.0, relative_ahc_working_age=0.0, relative_ahc_pensioners=0.0,
            absolute_bhc_children=0.0, absolute_bhc_working_age=0.0, absolute_bhc_pensioners=0.0,
            absolute_ahc_children=0.0, absolute_ahc_working_age=0.0, absolute_ahc_pensioners=0.0,
        ),
        cpi_index=100.0,
    )


class Simulation:
    """Run the PolicyEngine UK microsimulation engine.

    Accepts data via DataFrames (piped to binary stdin), file paths, or
    legacy FRS-specific arguments.

    Usage::

        from policyengine_uk_compiled import Simulation, Parameters, IncomeTaxParams

        # From DataFrames (hypothetical household)
        persons, benunits, households = Simulation.single_person(
            employment_income=50000
        )
        sim = Simulation(year=2025, persons=persons, benunits=benunits,
                         households=households)
        result = sim.run()

        # From a data directory
        sim = Simulation(year=2025, data_dir="data/frs/2023")
        result = sim.run()

        # With a parametric reform
        reform = Parameters(income_tax=IncomeTaxParams(personal_allowance=20000))
        result = sim.run(policy=reform)

        # With a structural reform (pre-hook: mutate inputs before simulation)
        from policyengine_uk_compiled import StructuralReform

        def cap_wages(year, persons, benunits, households):
            persons["employment_income"] = persons["employment_income"].clip(upper=100_000)
            return persons, benunits, households

        result = sim.run(structural=StructuralReform(pre=cap_wages))

        # With a structural reform (post-hook: adjust outputs after simulation)
        def add_ubi(year, persons, benunits, households):
            ubi = 50 * 52  # £50/wk per adult
            adults = persons["age"] >= 18
            adult_counts = persons[adults].groupby("household_id").size()
            households["reform_net_income"] += households["household_id"].map(adult_counts).fillna(0) * ubi
            households["reform_total_tax"] = households["baseline_total_tax"]  # unchanged
            return persons, benunits, households

        result = sim.run(structural=StructuralReform(post=add_ubi))
    """

    def __init__(
        self,
        year: int = 2025,
        *,
        # Generic data interface
        persons=None,
        benunits=None,
        households=None,
        data_dir: Optional[Union[str, Path]] = None,
        dataset: Optional[str] = None,
        # Legacy FRS interface
        clean_frs_base: Optional[str] = None,
        clean_frs: Optional[str] = None,
        frs_raw: Optional[str] = None,
        binary_path: Optional[str] = None,
    ):
        self.year = year
        self.binary_path = binary_path or _find_binary()

        # Determine data mode
        self._stdin_payload = None
        self._data_dir = None
        self._clean_frs_base = clean_frs_base
        self._clean_frs = clean_frs
        self._frs_raw = frs_raw
        self._dataset = dataset
        self._persons_only = dataset in ("spi",)
        # Store DataFrames when passed directly so pre-hooks can use them
        self._persons_df = None
        self._benunits_df = None
        self._households_df = None

        if persons is not None and benunits is not None and households is not None:
            # DataFrame or CSV string mode
            if HAS_PANDAS and hasattr(persons, "to_csv"):
                self._persons_df = persons
                self._benunits_df = benunits
                self._households_df = households
                persons_csv = _df_to_csv(persons)
                benunits_csv = _df_to_csv(benunits)
                households_csv = _df_to_csv(households)
            elif isinstance(persons, str):
                persons_csv = persons
                benunits_csv = benunits
                households_csv = households
            else:
                raise TypeError(
                    "persons/benunits/households must be pandas DataFrames or CSV strings"
                )
            self._stdin_payload = _build_stdin_payload(
                persons_csv, benunits_csv, households_csv
            )
        elif data_dir is not None:
            self._data_dir = str(data_dir)

    def _apply_pre_hook(self, structural: Optional[StructuralReform]) -> Optional[str]:
        """Apply the pre-hook if present and return a stdin payload string.

        For file-based data sources, loads the CSVs into DataFrames first so
        the hook can mutate them, then re-serialises to the stdin protocol.
        Returns None if there is no pre-hook (caller uses the original payload).
        """
        if structural is None or structural.pre is None:
            return self._stdin_payload  # unchanged

        if not HAS_PANDAS:
            raise ImportError("pandas is required for structural pre-hooks")

        import io
        import pandas as pd

        # Obtain DataFrames — either already stored or loaded from files
        if self._persons_df is not None:
            persons = self._persons_df.copy()
            benunits = self._benunits_df.copy()
            households = self._households_df.copy()
        elif self._stdin_payload is not None:
            # Parse the existing stdin payload back into DataFrames
            parsed = _parse_stdin_payload(self._stdin_payload)
            persons = parsed[0]
            benunits = parsed[1]
            households = parsed[2]
        else:
            # File-based source: load the CSVs from disk
            data_path = self._resolve_data_path()
            import os
            year_dir = os.path.join(data_path, str(self.year))
            if not os.path.isdir(year_dir):
                # Try direct path (data_dir may already include year)
                year_dir = data_path
            persons    = pd.read_csv(os.path.join(year_dir, "persons.csv"))
            benunits   = pd.read_csv(os.path.join(year_dir, "benunits.csv"))
            households = pd.read_csv(os.path.join(year_dir, "households.csv"))

        persons, benunits, households = structural.pre(
            self.year, persons, benunits, households
        )
        return _build_stdin_payload(
            _df_to_csv(persons), _df_to_csv(benunits), _df_to_csv(households)
        )

    def _resolve_data_path(self) -> str:
        """Return the base data directory for the current configuration."""
        if self._data_dir:
            return self._data_dir
        if self._clean_frs_base:
            return self._clean_frs_base
        if self._clean_frs:
            return self._clean_frs
        if self._dataset is not None:
            from policyengine_uk_compiled.data import ensure_dataset
            return ensure_dataset(self._dataset, self.year)
        from policyengine_uk_compiled.data import ensure_frs
        return ensure_frs(self.year)

    def _build_cmd(self, policy: Optional[Parameters] = None, extra_args: Optional[list[str]] = None, stdin_override: bool = False) -> list[str]:
        cmd = [self.binary_path, "--year", str(self.year)]

        if self._stdin_payload is not None or stdin_override:
            cmd.append("--stdin-data")
        elif self._data_dir:
            cmd += ["--data", self._data_dir]
        elif self._clean_frs_base:
            cmd += ["--data", self._clean_frs_base]
        elif self._clean_frs:
            cmd += ["--data", self._clean_frs]
        elif self._frs_raw:
            cmd += ["--frs", self._frs_raw]
        elif self._dataset is not None:
            from policyengine_uk_compiled.data import ensure_dataset
            data_path = ensure_dataset(self._dataset, self.year)
            cmd += ["--data", data_path]
        else:
            # No data source specified — try auto-resolving FRS data
            from policyengine_uk_compiled.data import ensure_frs
            frs_path = ensure_frs(self.year)
            cmd += ["--data", frs_path]

        if policy:
            overlay = policy.model_dump(exclude_none=True)
            if overlay:
                cmd += ["--policy-json", json.dumps(overlay)]

        if self._persons_only:
            cmd.append("--persons-only")

        if extra_args:
            cmd += extra_args

        return cmd

    def run(
        self,
        policy: Optional[Parameters] = None,
        structural: Optional[StructuralReform] = None,
        timeout: int = 120,
    ) -> SimulationResult:
        """Run the simulation and return typed results.

        Args:
            policy: Parametric reform overlay (changes parameter values).
            structural: Structural reform with optional pre/post hooks.
                pre(year, persons, benunits, households) mutates inputs before
                the binary runs.  post(year, persons, benunits, households)
                mutates microdata outputs; aggregation is then done in Python.
            timeout: Maximum seconds to wait for the binary.

        Returns:
            SimulationResult with budgetary impact, program breakdown, decile impacts, etc.
        """
        # If a post-hook is present we must go through microdata and re-aggregate
        if structural is not None and structural.post is not None:
            microdata = self.run_microdata(policy=policy, structural=structural, timeout=timeout)
            return aggregate_microdata(
                microdata.persons, microdata.benunits, microdata.households, self.year
            )

        stdin_payload = self._apply_pre_hook(structural)
        cmd = self._build_cmd(policy, extra_args=["--output", "json"], stdin_override=stdin_payload is not None)
        cwd = _find_cwd(self.binary_path)
        result = subprocess.run(
            cmd,
            input=stdin_payload,
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=cwd,
        )
        if result.returncode != 0:
            raise RuntimeError(
                f"Simulation failed (exit {result.returncode}):\n{result.stderr}"
            )
        data = json.loads(result.stdout)
        if self._persons_only:
            return _aggregate_persons_only(data, self.year)
        return SimulationResult(**data)

    def run_microdata(
        self,
        policy: Optional[Parameters] = None,
        structural: Optional[StructuralReform] = None,
        timeout: int = 120,
    ) -> MicrodataResult:
        """Run the simulation and return per-entity microdata as DataFrames.

        If a structural post-hook is provided it is applied to the DataFrames
        after the binary produces its output.
        """
        if not HAS_PANDAS:
            raise ImportError("pandas is required for run_microdata")
        stdin_payload = self._apply_pre_hook(structural)
        cmd = self._build_cmd(policy, extra_args=["--output-microdata-stdout"], stdin_override=stdin_payload is not None)
        cwd = _find_cwd(self.binary_path)
        result = subprocess.run(
            cmd,
            input=stdin_payload,
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=cwd,
        )
        if result.returncode != 0:
            raise RuntimeError(
                f"Simulation failed (exit {result.returncode}):\n{result.stderr}"
            )
        microdata = _parse_microdata_stdout(result.stdout)
        if structural is not None and structural.post is not None:
            persons, benunits, households = structural.post(
                self.year,
                microdata.persons.copy(),
                microdata.benunits.copy(),
                microdata.households.copy(),
            )
            return MicrodataResult(persons=persons, benunits=benunits, households=households)
        return microdata

    def get_baseline_params(self, timeout: int = 10) -> dict:
        """Export the baseline parameters for the configured year as a dict."""
        cmd = [self.binary_path, "--year", str(self.year), "--export-params-json"]
        cwd = _find_cwd(self.binary_path)
        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=timeout, cwd=cwd,
        )
        if result.returncode != 0:
            raise RuntimeError(f"Failed to export params: {result.stderr}")
        return json.loads(result.stdout)

    # ── Convenience constructors for hypothetical households ──────────────

    @staticmethod
    def from_situation(
        situation: dict,
        year: int = 2025,
        **kwargs,
    ) -> "Simulation":
        """Build a Simulation from a PolicyEngine situation-JSON dict.

        The situation dict mirrors the PolicyEngine web-app format::

            {
                "people":     {"<id>": {"<var>": {"<period>": <value>}, ...}, ...},
                "benunits":   {"<id>": {"members": [...], "<var>": ..., ...}, ...},
                "households": {"<id>": {"members": [...], "<var>": ..., ...}, ...},
            }

        Each variable's value may be either a period-keyed dict (e.g.
        ``{"2025": 50000}``) or a plain scalar — scalars are treated as
        applying to ``year``.

        Variable names map directly to the wrapper input columns (see
        ``PERSON_DEFAULTS``, ``BENUNIT_DEFAULTS``, ``HOUSEHOLD_DEFAULTS``).
        ``region`` accepts either the title-case form (``"London"``,
        ``"North East"``) or the upper-snake form used by the
        PolicyEngine web app (``"LONDON"``, ``"NORTH_EAST"``); it is
        normalised before being passed to the Rust engine and
        ``is_in_scotland`` is set automatically. ``gender`` is
        case-insensitive.

        Members lists on benunits/households reference the keys used in
        ``situation["people"]``; people are assigned integer ``person_id``
        values in the order they appear under ``people``, and benunits/
        households receive the ``person_ids`` / ``benunit_ids`` strings
        the engine expects.

        Example::

            sim = Simulation.from_situation(
                {
                    "people": {
                        "you": {"age": 30, "employment_income": {"2025": 50000}},
                    },
                    "benunits":   {"yours": {"members": ["you"]}},
                    "households": {"yours": {"members": ["you"], "region": "LONDON"}},
                },
                year=2025,
            )
            result = sim.run()
        """
        persons_df, benunits_df, households_df = _situation_to_dataframes(
            situation, year
        )
        return Simulation(
            year=year,
            persons=persons_df,
            benunits=benunits_df,
            households=households_df,
            **kwargs,
        )

    @staticmethod
    def single_person(
        age: float = 30,
        employment_income: float = 0.0,
        self_employment_income: float = 0.0,
        pension_income: float = 0.0,
        region: str = "London",
        rent_monthly: float = 0.0,
        council_tax_annual: float = 0.0,
        **person_kwargs,
    ):
        """Build a single-person household dataset.

        Returns (persons_df, benunits_df, households_df) tuple.
        """
        if not HAS_PANDAS:
            raise ImportError("pandas is required for DataFrame construction")
        person = {
            **PERSON_DEFAULTS,
            "age": age,
            "employment_income": employment_income,
            "self_employment_income": self_employment_income,
            "private_pension_income": pension_income,
            "is_in_scotland": region == "Scotland",
            **person_kwargs,
        }
        benunit = {
            **BENUNIT_DEFAULTS,
            "rent_monthly": rent_monthly,
        }
        household = {
            **HOUSEHOLD_DEFAULTS,
            "region": region,
            "rent_annual": rent_monthly * 12,
            "council_tax_annual": council_tax_annual,
        }
        return pd.DataFrame([person]), pd.DataFrame([benunit]), pd.DataFrame([household])

    @staticmethod
    def couple(
        ages: tuple[float, float] = (30, 30),
        incomes: tuple[float, float] = (0.0, 0.0),
        children: int = 0,
        child_ages: Optional[list[float]] = None,
        region: str = "London",
        rent_monthly: float = 0.0,
        council_tax_annual: float = 0.0,
    ):
        """Build a couple household, optionally with children.

        Returns (persons_df, benunits_df, households_df) tuple.
        """
        if not HAS_PANDAS:
            raise ImportError("pandas is required for DataFrame construction")

        if child_ages is None:
            child_ages = [10.0] * children
        else:
            children = len(child_ages)

        persons = []
        n_people = 2 + children
        # Adult 1 (head)
        persons.append({
            **PERSON_DEFAULTS,
            "person_id": 0, "age": ages[0],
            "employment_income": incomes[0],
            "is_benunit_head": True, "is_household_head": True,
            "is_in_scotland": region == "Scotland",
        })
        # Adult 2
        persons.append({
            **PERSON_DEFAULTS,
            "person_id": 1, "age": ages[1],
            "employment_income": incomes[1],
            "is_benunit_head": False, "is_household_head": False,
            "is_in_scotland": region == "Scotland",
        })
        # Children
        for i, cage in enumerate(child_ages):
            persons.append({
                **PERSON_DEFAULTS,
                "person_id": 2 + i, "age": cage,
                "gender": "male",
                "is_benunit_head": False, "is_household_head": False,
                "employment_income": 0.0,
                "is_in_scotland": region == "Scotland",
            })

        person_id_str = ";".join(str(i) for i in range(n_people))
        benunit = {
            **BENUNIT_DEFAULTS,
            "person_ids": person_id_str,
            "rent_monthly": rent_monthly,
        }
        household = {
            **HOUSEHOLD_DEFAULTS,
            "benunit_ids": "0",
            "person_ids": person_id_str,
            "region": region,
            "rent_annual": rent_monthly * 12,
            "council_tax_annual": council_tax_annual,
        }
        return pd.DataFrame(persons), pd.DataFrame([benunit]), pd.DataFrame([household])
