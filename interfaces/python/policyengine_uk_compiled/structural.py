"""Structural reform hooks and Python-side aggregation.

A StructuralReform holds two optional callables:

    pre(year, persons, benunits, households) -> (persons, benunits, households)
        Runs before the Rust binary sees the data.  Use to mutate input
        columns — add a new income source, change household composition,
        set benefit eligibility flags, etc.

    post(year, persons, benunits, households) -> (persons, benunits, households)
        Runs after the binary produces microdata output.  All
        baseline_*/reform_* columns are populated at this point.  Use to
        apply a new tax on top of simulated results, offset a benefit,
        impose a cap, etc.  Aggregation is then done in Python rather than
        by the binary.

Both hooks receive and must return all three DataFrames even if only one is
modified, so the caller can always unpack a consistent triple.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Callable, Optional, TYPE_CHECKING

if TYPE_CHECKING:
    import pandas as pd

# Type alias for the hook signature
HookFn = Callable[
    [int, "pd.DataFrame", "pd.DataFrame", "pd.DataFrame"],
    tuple["pd.DataFrame", "pd.DataFrame", "pd.DataFrame"],
]


@dataclass
class StructuralReform:
    """Container for pre- and post-simulation structural reform hooks.

    Both hooks are optional.  Omit whichever you don't need.

    Hook signature (same for pre and post):

        def hook(
            year: int,
            persons: pd.DataFrame,
            benunits: pd.DataFrame,
            households: pd.DataFrame,
        ) -> tuple[pd.DataFrame, pd.DataFrame, pd.DataFrame]:
            ...
            return persons, benunits, households

    Example — add a £50/wk UBI to every adult's reform net income::

        def ubi_post(year, persons, benunits, households):
            ubi_annual = 50 * 52
            mask = persons["age"] >= 18
            persons.loc[mask, "reform_income_tax"] = 0  # illustrative
            households["reform_net_income"] += ubi_annual  # per-household
            return persons, benunits, households

        reform = StructuralReform(post=ubi_post)

    Example — replace employment income with a flat wage in 2025 only::

        def flat_wage_pre(year, persons, benunits, households):
            if year == 2025:
                persons["employment_income"] = persons["employment_income"].clip(upper=50_000)
            return persons, benunits, households

        reform = StructuralReform(pre=flat_wage_pre)
    """

    pre: Optional[HookFn] = field(default=None)
    post: Optional[HookFn] = field(default=None)


# ── Python-side aggregation ───────────────────────────────────────────────────
#
# Used whenever a post-hook is present (or for persons-only datasets).
# Reads the microdata columns produced by the Rust binary and aggregates
# them into a SimulationResult.  Column names mirror write_microdata_csv_* in
# src/data/clean.rs.


def aggregate_microdata(
    persons: "pd.DataFrame",
    benunits: "pd.DataFrame",
    households: "pd.DataFrame",
    year: int,
) -> "SimulationResult":  # noqa: F821 – imported lazily to avoid circular import
    """Aggregate post-simulation microdata DataFrames into a SimulationResult.

    This mirrors the aggregation logic in src/main.rs but runs in Python,
    allowing post-hooks to modify result columns before the final roll-up.

    Deciles and winners/losers are based on reform_net_income (equivalised by
    equivalisation_factor where available).  This approximates the Rust engine's
    use of extended_net_income; the difference only matters for VAT/stamp duty/
    wealth-tax reforms, which are unlikely to be applied as post-hooks.
    """
    import numpy as np
    import pandas as pd
    from policyengine_uk_compiled.models import (
        BudgetaryImpact, IncomeBreakdown, ProgramBreakdown, Caseloads,
        DecileImpact, WinnersLosers, SimulationResult,
        HbaiIncomes, PovertyHeadcounts,
    )

    def cpi_index_for_year(year: int) -> float:
        # CPI index by fiscal year, rebased to 2010/11 = 100 (the absolute-poverty
        # reference year). Source: OBR EFO March 2026 table 1.7 CPI (2015=100),
        # with pre-2010 fiscal years from ONS series D7BT financial-year averages.
        table = {
            1994: 72.916542, 1995: 74.863074, 1996: 76.532848, 1997: 77.879738,
            1998: 79.079023, 1999: 79.983099, 2000: 80.647319, 2001: 81.772802,
            2002: 82.787582, 2003: 83.876164, 2004: 85.084674, 2005: 86.874377,
            2006: 89.116118, 2007: 91.071875, 2008: 94.485225, 2009: 96.616263,
            2010: 100.000000, 2011: 104.300545, 2012: 107.068495, 2013: 109.535701,
            2014: 110.686646, 2015: 110.798825, 2016: 112.025879, 2017: 115.190516,
            2018: 117.802559, 2019: 119.851492, 2020: 120.557502, 2021: 125.368849,
            2022: 137.951381, 2023: 145.773765, 2024: 149.171329, 2025: 153.921493,
            2026: 156.889890, 2027: 160.021436, 2028: 163.222508, 2029: 166.486583,
        }
        return table.get(year, 100.0)

    def weighted_median(values: np.ndarray, weights: np.ndarray) -> float:
        if len(values) == 0:
            return 0.0
        order = np.argsort(values)
        values = values[order]
        weights = weights[order]
        cutoff = weights.sum() / 2.0
        return float(values[np.searchsorted(np.cumsum(weights), cutoff, side="left")])

    def refresh_household_hbai_columns(hh: "pd.DataFrame") -> "pd.DataFrame":
        """Recompute derived HBAI household columns from primitive inputs.

        Post-hooks often mutate household net income directly. Re-deriving the
        AHC and equivalised fields here keeps poverty metrics consistent with
        those changes, rather than trusting stale columns emitted before the
        hook ran.
        """
        hh = hh.copy()

        if "housing_costs_ahc_annual" in hh.columns:
            housing_costs = hh["housing_costs_ahc_annual"].fillna(0.0)
        elif {"baseline_net_income", "baseline_net_income_ahc"}.issubset(hh.columns):
            housing_costs = (
                hh["baseline_net_income"].fillna(0.0)
                - hh["baseline_net_income_ahc"].fillna(0.0)
            )
        else:
            housing_costs = 0.0

        for prefix in ("baseline", "reform"):
            net_col = f"{prefix}_net_income"
            net_ahc_col = f"{prefix}_net_income_ahc"
            eq_factor_col = f"{prefix}_equivalisation_factor"
            eq_col = f"{prefix}_equivalised_net_income"
            eq_factor_ahc_col = f"{prefix}_equivalisation_factor_ahc"
            eq_ahc_col = f"{prefix}_equivalised_net_income_ahc"

            if net_col not in hh.columns:
                continue

            hh[net_ahc_col] = hh[net_col].fillna(0.0) - housing_costs

            if eq_factor_col in hh.columns:
                eq_factor = hh[eq_factor_col].fillna(1.0).clip(lower=1e-9)
                hh[eq_col] = hh[net_col].fillna(0.0) / eq_factor
            elif eq_col not in hh.columns:
                hh[eq_col] = hh[net_col].fillna(0.0)

            if eq_factor_ahc_col in hh.columns:
                eq_factor_ahc = hh[eq_factor_ahc_col].fillna(1.0).clip(lower=1e-9)
                hh[eq_ahc_col] = hh[net_ahc_col].fillna(0.0) / eq_factor_ahc
            elif eq_ahc_col not in hh.columns:
                hh[eq_ahc_col] = hh[net_ahc_col].fillna(0.0)

        return hh

    households = refresh_household_hbai_columns(households)

    person_counts = persons.groupby("household_id").size() if "household_id" in persons.columns else pd.Series(dtype=float)

    def hbai_for(prefix: str) -> HbaiIncomes:
        hh = households.copy()
        hh["person_count"] = hh["household_id"].map(person_counts).fillna(0.0)
        person_weights = hh["weight"].values * hh["person_count"].values
        equiv_ahc_col = f"{prefix}_equivalised_net_income_ahc"
        net_ahc_col = f"{prefix}_net_income_ahc"
        if equiv_ahc_col not in hh.columns:
            hh[equiv_ahc_col] = hh[f"{prefix}_equivalised_net_income"]
        if net_ahc_col not in hh.columns:
            hh[net_ahc_col] = hh[f"{prefix}_net_income"]
        return HbaiIncomes(
            mean_equiv_bhc=float((hh["weight"] * hh[f"{prefix}_equivalised_net_income"]).sum() / hh["weight"].sum()) if len(hh) else 0.0,
            mean_equiv_ahc=float((hh["weight"] * hh[equiv_ahc_col]).sum() / hh["weight"].sum()) if len(hh) else 0.0,
            mean_bhc=float((hh["weight"] * hh[f"{prefix}_net_income"]).sum() / hh["weight"].sum()) if len(hh) else 0.0,
            mean_ahc=float((hh["weight"] * hh[net_ahc_col]).sum() / hh["weight"].sum()) if len(hh) else 0.0,
            median_equiv_bhc=weighted_median(hh[f"{prefix}_equivalised_net_income"].values, person_weights),
            median_equiv_ahc=weighted_median(hh[equiv_ahc_col].values, person_weights),
        )

    w = households["weight"].values

    # ── Join weights to persons and benunits ─────────────────────────────────
    p_with_w = persons.merge(
        households[["household_id", "weight"]], on="household_id", how="left"
    )
    pw = p_with_w["weight"].fillna(1.0).values

    bu_with_w = benunits.merge(
        households[["household_id", "weight"]], on="household_id", how="left"
    )
    bw = bu_with_w["weight"].fillna(1.0).values

    def _wsum(col: str) -> float:
        return float((pw * p_with_w[col].fillna(0.0).values).sum()) if col in p_with_w.columns else 0.0

    def _bwsum(col: str) -> float:
        return float((bw * bu_with_w[col].fillna(0.0).values).sum()) if col in bu_with_w.columns else 0.0

    # ── Person-level tax totals ──────────────────────────────────────────────
    it_baseline = _wsum("baseline_income_tax")
    it_reform   = _wsum("reform_income_tax")
    eni_baseline = _wsum("baseline_employee_ni")
    eni_reform   = _wsum("reform_employee_ni")
    enr_baseline = _wsum("baseline_employer_ni")
    enr_reform   = _wsum("reform_employer_ni")

    # ── Benefit program totals (from individual benunit columns) ─────────────
    _benefit_programs = [
        "universal_credit", "child_benefit", "state_pension", "pension_credit",
        "housing_benefit", "child_tax_credit", "working_tax_credit",
        "income_support", "esa_income_related", "jsa_income_based",
        "carers_allowance", "scottish_child_payment", "passthrough_benefits",
    ]

    # Recompute total benefits from individual program columns so that
    # post-hooks modifying individual benefit columns are reflected in totals.
    baseline_benefits_from_programs = sum(
        _bwsum(f"baseline_{prog}") for prog in _benefit_programs
    ) - _bwsum("baseline_benefit_cap_reduction")
    reform_benefits_from_programs = sum(
        _bwsum(f"reform_{prog}") for prog in _benefit_programs
    ) - _bwsum("reform_benefit_cap_reduction")

    # Use program-level sums for benefits (captures individual column changes)
    baseline_benefits = float(baseline_benefits_from_programs)
    reform_benefits   = float(reform_benefits_from_programs)

    # For revenue, recompute from person-level tax columns
    baseline_revenue = float(it_baseline + eni_baseline + enr_baseline)
    reform_revenue   = float(it_reform + eni_reform + enr_reform)

    # If hooks modified reform_net_income directly on households (without
    # touching individual benefit/tax columns), capture that as additional
    # benefit spending.  Only attribute the residual when household
    # net_income actually changed — if net_income is unchanged but program
    # columns changed, trust the program columns.
    net_income_change = float(
        (w * (households["reform_net_income"].values - households["baseline_net_income"].values)).sum()
    )
    if abs(net_income_change) > 1.0:
        program_net_change = (reform_benefits - baseline_benefits) - (reform_revenue - baseline_revenue)
        residual = net_income_change - program_net_change
        if abs(residual) > 1.0:
            reform_benefits += residual

    revenue_change = reform_revenue - baseline_revenue
    benefit_change = reform_benefits - baseline_benefits
    net_cost       = -revenue_change + benefit_change

    # ── Income breakdown (from person-level inputs) ──────────────────────────
    income_breakdown = IncomeBreakdown(
        employment_income=_wsum("employment_income"),
        self_employment_income=_wsum("self_employment_income"),
        pension_income=_wsum("private_pension_income"),
        savings_interest_income=_wsum("savings_interest"),
        dividend_income=_wsum("dividend_income"),
        property_income=_wsum("property_income"),
        other_income=_wsum("other_income"),
    )

    # ── Program breakdown ────────────────────────────────────────────────────
    program_breakdown = ProgramBreakdown(
        income_tax=it_reform,
        employee_ni=eni_reform,
        employer_ni=enr_reform,
        universal_credit=_bwsum("reform_universal_credit"),
        child_benefit=_bwsum("reform_child_benefit"),
        state_pension=_bwsum("reform_state_pension"),
        pension_credit=_bwsum("reform_pension_credit"),
        housing_benefit=_bwsum("reform_housing_benefit"),
        child_tax_credit=_bwsum("reform_child_tax_credit"),
        working_tax_credit=_bwsum("reform_working_tax_credit"),
        income_support=_bwsum("reform_income_support"),
        esa_income_related=_bwsum("reform_esa_income_related"),
        jsa_income_based=_bwsum("reform_jsa_income_based"),
        carers_allowance=_bwsum("reform_carers_allowance"),
        scottish_child_payment=_bwsum("reform_scottish_child_payment"),
        benefit_cap_reduction=_bwsum("reform_benefit_cap_reduction"),
        passthrough_benefits=_bwsum("reform_passthrough_benefits"),
    )

    # ── Caseloads ─────────────────────────────────────────────────────────────
    caseloads = Caseloads(
        income_tax_payers=float((pw * (p_with_w.get("reform_income_tax", 0) > 0)).sum()) if "reform_income_tax" in p_with_w.columns else 0.0,
        ni_payers=float((pw * (p_with_w.get("reform_employee_ni", 0) > 0)).sum()) if "reform_employee_ni" in p_with_w.columns else 0.0,
        employer_ni_payers=float((pw * (p_with_w.get("reform_employer_ni", 0) > 0)).sum()) if "reform_employer_ni" in p_with_w.columns else 0.0,
        universal_credit=float((bw * (bu_with_w.get("reform_universal_credit", 0) > 0)).sum()) if "reform_universal_credit" in bu_with_w.columns else 0.0,
        child_benefit=float((bw * (bu_with_w.get("reform_child_benefit", 0) > 0)).sum()) if "reform_child_benefit" in bu_with_w.columns else 0.0,
        state_pension=float((bw * (bu_with_w.get("reform_state_pension", 0) > 0)).sum()) if "reform_state_pension" in bu_with_w.columns else 0.0,
        pension_credit=float((bw * (bu_with_w.get("reform_pension_credit", 0) > 0)).sum()) if "reform_pension_credit" in bu_with_w.columns else 0.0,
        housing_benefit=float((bw * (bu_with_w.get("reform_housing_benefit", 0) > 0)).sum()) if "reform_housing_benefit" in bu_with_w.columns else 0.0,
        child_tax_credit=float((bw * (bu_with_w.get("reform_child_tax_credit", 0) > 0)).sum()) if "reform_child_tax_credit" in bu_with_w.columns else 0.0,
        working_tax_credit=float((bw * (bu_with_w.get("reform_working_tax_credit", 0) > 0)).sum()) if "reform_working_tax_credit" in bu_with_w.columns else 0.0,
        income_support=float((bw * (bu_with_w.get("reform_income_support", 0) > 0)).sum()) if "reform_income_support" in bu_with_w.columns else 0.0,
        esa_income_related=float((bw * (bu_with_w.get("reform_esa_income_related", 0) > 0)).sum()) if "reform_esa_income_related" in bu_with_w.columns else 0.0,
        jsa_income_based=float((bw * (bu_with_w.get("reform_jsa_income_based", 0) > 0)).sum()) if "reform_jsa_income_based" in bu_with_w.columns else 0.0,
        carers_allowance=float((bw * (bu_with_w.get("reform_carers_allowance", 0) > 0)).sum()) if "reform_carers_allowance" in bu_with_w.columns else 0.0,
        scottish_child_payment=float((bw * (bu_with_w.get("reform_scottish_child_payment", 0) > 0)).sum()) if "reform_scottish_child_payment" in bu_with_w.columns else 0.0,
        benefit_cap_affected=float((bw * (bu_with_w.get("reform_benefit_cap_reduction", 0) < 0)).sum()) if "reform_benefit_cap_reduction" in bu_with_w.columns else 0.0,
    )

    # ── Decile impacts ────────────────────────────────────────────────────────
    # Rank households by baseline equivalised net income; measure change on
    # reform equivalised net income.
    eq = households["baseline_equivalisation_factor"].clip(lower=1e-9) if "baseline_equivalisation_factor" in households.columns else 1.0
    bl_equiv = households["baseline_net_income"].values / (eq.values if hasattr(eq, "values") else eq)
    rf_equiv = households["reform_net_income"].values  / (eq.values if hasattr(eq, "values") else eq)

    order = np.argsort(bl_equiv)
    bl_sorted = bl_equiv[order]
    rf_sorted = rf_equiv[order]

    n = len(order)
    decile_size = n // 10
    decile_impacts = []
    for d in range(10):
        start = d * decile_size
        end = n if d == 9 else (d + 1) * decile_size
        bl_sl = bl_sorted[start:end]
        rf_sl = rf_sorted[start:end]
        count = len(bl_sl)
        if count == 0:
            decile_impacts.append(DecileImpact(decile=d + 1))
            continue
        avg_base  = float(bl_sl.mean())
        avg_ref   = float(rf_sl.mean())
        avg_chg   = avg_ref - avg_base
        pct_chg   = 100.0 * avg_chg / avg_base if avg_base != 0 else 0.0
        decile_impacts.append(DecileImpact(
            decile=d + 1,
            avg_baseline_income=round(avg_base, 2),
            avg_reform_income=round(avg_ref, 2),
            avg_change=round(avg_chg, 2),
            pct_change=round(pct_chg, 2),
        ))

    # ── Winners and losers ────────────────────────────────────────────────────
    change = households["reform_net_income"].values - households["baseline_net_income"].values
    winners_w   = float((w * (change >  1.0)).sum())
    losers_w    = float((w * (change < -1.0)).sum())
    unchanged_w = float((w * (np.abs(change) <= 1.0)).sum())
    total_gain  = float((w * change * (change >  1.0)).sum())
    total_loss  = float((w * np.abs(change) * (change < -1.0)).sum())
    total_w     = winners_w + losers_w + unchanged_w

    winners_losers = WinnersLosers(
        winners_pct=round(100.0 * winners_w / total_w, 1) if total_w > 0 else 0.0,
        losers_pct=round(100.0 * losers_w / total_w, 1) if total_w > 0 else 0.0,
        unchanged_pct=round(100.0 * unchanged_w / total_w, 1) if total_w > 0 else 0.0,
        avg_gain=round(total_gain / winners_w) if winners_w > 0 else 0.0,
        avg_loss=round(total_loss / losers_w) if losers_w > 0 else 0.0,
    )

    fiscal_year = f"{year}/{(year + 1) % 100:02d}"
    baseline_hbai_incomes = hbai_for("baseline")
    reform_hbai_incomes = hbai_for("reform")

    rel_line_bhc = 0.60 * baseline_hbai_incomes.median_equiv_bhc
    rel_line_ahc = 0.60 * baseline_hbai_incomes.median_equiv_ahc
    cpi_index = cpi_index_for_year(year)
    abs_line_bhc = 14_400.0 * (cpi_index / 100.0)
    abs_line_ahc = 11_600.0 * (cpi_index / 100.0)

    hh_for_poverty = households.copy()
    for prefix in ("baseline", "reform"):
        equiv_ahc_col = f"{prefix}_equivalised_net_income_ahc"
        if equiv_ahc_col not in hh_for_poverty.columns:
            hh_for_poverty[equiv_ahc_col] = hh_for_poverty[f"{prefix}_equivalised_net_income"]

    pw = persons.merge(
        hh_for_poverty[[
            "household_id", "weight",
            "baseline_equivalised_net_income", "baseline_equivalised_net_income_ahc",
            "reform_equivalised_net_income", "reform_equivalised_net_income_ahc",
        ]],
        on="household_id",
        how="left",
    )

    def poverty_for(prefix: str) -> PovertyHeadcounts:
        eq_bhc = pw[f"{prefix}_equivalised_net_income"]
        eq_ahc = pw[f"{prefix}_equivalised_net_income_ahc"]
        weights = pw["weight"].fillna(1.0)
        ages = pw["age"]

        child = ages < 16.0
        working = (ages >= 16.0) & (ages < 66.0)
        pensioner = ages >= 66.0

        def pct(mask, denom_mask):
            denom = float(weights[denom_mask].sum())
            num = float(weights[mask & denom_mask].sum())
            return round(100.0 * num / denom, 1) if denom > 0 else 0.0

        return PovertyHeadcounts(
            relative_bhc_children=pct(eq_bhc < rel_line_bhc, child),
            relative_bhc_working_age=pct(eq_bhc < rel_line_bhc, working),
            relative_bhc_pensioners=pct(eq_bhc < rel_line_bhc, pensioner),
            relative_ahc_children=pct(eq_ahc < rel_line_ahc, child),
            relative_ahc_working_age=pct(eq_ahc < rel_line_ahc, working),
            relative_ahc_pensioners=pct(eq_ahc < rel_line_ahc, pensioner),
            absolute_bhc_children=pct(eq_bhc < abs_line_bhc, child),
            absolute_bhc_working_age=pct(eq_bhc < abs_line_bhc, working),
            absolute_bhc_pensioners=pct(eq_bhc < abs_line_bhc, pensioner),
            absolute_ahc_children=pct(eq_ahc < abs_line_ahc, child),
            absolute_ahc_working_age=pct(eq_ahc < abs_line_ahc, working),
            absolute_ahc_pensioners=pct(eq_ahc < abs_line_ahc, pensioner),
        )

    baseline_poverty = poverty_for("baseline")
    reform_poverty = poverty_for("reform")

    return SimulationResult(
        fiscal_year=fiscal_year,
        budgetary_impact=BudgetaryImpact(
            baseline_revenue=float(baseline_revenue),
            reform_revenue=float(reform_revenue),
            revenue_change=float(revenue_change),
            baseline_benefits=float(baseline_benefits),
            reform_benefits=float(reform_benefits),
            benefit_spending_change=float(benefit_change),
            net_cost=float(net_cost),
        ),
        income_breakdown=income_breakdown,
        program_breakdown=program_breakdown,
        caseloads=caseloads,
        decile_impacts=decile_impacts,
        winners_losers=winners_losers,
        baseline_hbai_incomes=baseline_hbai_incomes,
        reform_hbai_incomes=reform_hbai_incomes,
        baseline_poverty=baseline_poverty,
        reform_poverty=reform_poverty,
        cpi_index=cpi_index,
    )


def combine_microdata(
    baseline: "MicrodataResult",  # noqa: F821
    reform: "MicrodataResult",  # noqa: F821
) -> "MicrodataResult":  # noqa: F821
    """Combine baseline-run and reform-run microdata into one comparison view.

    Baseline columns come from the original run. Reform columns come from the
    structurally/policy-modified run. Unprefixed columns come from the reform run.
    """
    from policyengine_uk_compiled.models import MicrodataResult

    def combine_entity(baseline_df, reform_df, id_col: str):
        if reform_df is None or len(reform_df) == 0:
            return reform_df
        combined = reform_df.copy()
        if baseline_df is None or len(baseline_df) == 0 or id_col not in baseline_df.columns or id_col not in reform_df.columns:
            return combined
        baseline_prefixed = [c for c in baseline_df.columns if c.startswith("baseline_")]
        if not baseline_prefixed:
            return combined
        baseline_slice = baseline_df[[id_col] + baseline_prefixed].copy()
        combined = combined.drop(columns=[c for c in baseline_prefixed if c in combined.columns])
        return combined.merge(baseline_slice, on=id_col, how="left")

    return MicrodataResult(
        persons=combine_entity(baseline.persons, reform.persons, "person_id"),
        benunits=combine_entity(baseline.benunits, reform.benunits, "benunit_id"),
        households=combine_entity(baseline.households, reform.households, "household_id"),
    )
