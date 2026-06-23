"""Interface to the compiled PolicyEngine UK Rust binary."""

from __future__ import annotations

import io
import json
import subprocess
from pathlib import Path
from typing import Optional, Union

# Native in-process engine (PyO3 extension). Falls back to the subprocess
# binary when the extension is not bundled.
try:
    from policyengine_uk_compiled import _native
except ImportError:
    _native = None

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
    "on_uc": False,
    "rent_monthly": 0.0, "is_lone_parent": False,
    # Hypothetical households carry no reported receipt, so claim every
    # means-tested benefit they're eligible for. Survey microdata overrides this
    # to the reported claim status.
    "claims_uc_if_eligible": True,
}

HOUSEHOLD_DEFAULTS = {
    "household_id": 0, "benunit_ids": "0", "person_ids": "0",
    "weight": 1.0, "region": "London",
    "rent_annual": 0.0, "council_tax_annual": 0.0, "council_tax_band": 0,
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


def _parse_microdata_stdout(raw: str) -> MicrodataResult:
    """Parse the concatenated CSV protocol output into a MicrodataResult."""
    import pandas as pd
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
        # Lazily-constructed in-process engine (loads the dataset once)
        self._native_sim = None
        # Store DataFrames when passed directly so pre-hooks can use them
        self._persons_df = None
        self._benunits_df = None
        self._households_df = None

        if persons is not None and benunits is not None and households is not None:
            # DataFrame or CSV string mode
            if hasattr(persons, "to_csv"):
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

        # In-process fast path: file-based data, no structural reform.
        # The native engine keeps the dataset and baseline results loaded, so
        # repeated runs only pay the reform-dependent work.
        if (
            _native is not None
            and structural is None
            and self._stdin_payload is None
            and self._frs_raw is None
            and not self._persons_only
        ):
            if self._native_sim is None:
                params_dir = str(Path(_find_cwd(self.binary_path)) / "parameters")
                self._native_sim = _native.Simulation(
                    self._resolve_data_path(), params_dir, self.year
                )
            policy_json = None
            if policy:
                overlay = policy.model_dump(exclude_none=True)
                if overlay:
                    policy_json = json.dumps(overlay)
            data = json.loads(self._native_sim.run(policy_json))
            return SimulationResult(**data)

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
        return_baselines: bool = False,
    ) -> MicrodataResult:
        """Run the simulation and return per-entity microdata as DataFrames.

        When neither policy nor return_baselines is set, output columns have
        plain names (e.g. net_income, income_tax). Pass return_baselines=True
        to get both baseline_* and reform_* columns side by side.
        """
        extra_args = ["--output-microdata-stdout"]
        if return_baselines:
            extra_args.append("--microdata-return-baselines")
        stdin_payload = self._apply_pre_hook(structural)
        cmd = self._build_cmd(policy, extra_args=extra_args, stdin_override=stdin_payload is not None)
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
        import pandas as pd
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
        import pandas as pd

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
