"""Per-decile YoY income decomposition by income source.

For each EFRS year, computes the weighted mean of each income component
within each decile (ranked by equivalised net income), then expresses the
YoY change in each component as % of prior year's decile net income.

Groups:
  earned          = employment_income + self_employment_income
  private_pension = private_pension_income
  investment      = savings_interest + dividend_income + property_income
                    + other_income + maintenance_income + miscellaneous_income
  state_pension   = state_pension
  working_age_ben = universal_credit + child_tax_credit + working_tax_credit
                    + housing_benefit + income_support + esa_income_related
                    + jsa_income_based + carers_allowance
  other_benefits  = child_benefit + pension_credit + passthrough_benefits
                    + scottish_child_payment
  taxes           = income_tax + employee_ni + council_tax  (negative contribution)

Usage:
    python data/hbai_income_decomp.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np
import pandas as pd
from rich.console import Console

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT / "interfaces" / "python"))

from policyengine_uk_compiled import Simulation  # noqa: E402
from policyengine_uk_compiled.realterms import cpi_index_for_year  # noqa: E402

console = Console()

EFRS_DATA_ROOT = REPO_ROOT / "data" / "clean" / "efrs"
BASE_YEAR  = 2026
ANCHOR_YEAR = 2016  # decile thresholds set here, uprated by CPI in later years
N_DECILES  = 10

_ANCHOR_THRESHOLDS: list[float] | None = None  # nominal £ thresholds for ANCHOR_YEAR


def _get_anchor_thresholds() -> list[float]:
    """Compute the N_DECILES-1 equivalised income thresholds from ANCHOR_YEAR.

    Returns thresholds in ANCHOR_YEAR nominal £, as the D1/D2, D2/D3, … D9/D10
    boundary values (N_DECILES-1 values).
    """
    global _ANCHOR_THRESHOLDS
    if _ANCHOR_THRESHOLDS is not None:
        return _ANCHOR_THRESHOLDS
    md = Simulation(year=ANCHOR_YEAR, data_dir=str(EFRS_DATA_ROOT)).run_microdata()
    hh = md.households.copy()
    w  = hh["weight"].to_numpy()
    eq_col = "equivalised_net_income" if "equivalised_net_income" in hh.columns else "net_income"
    eq = hh[eq_col].to_numpy()
    order   = np.argsort(eq)
    cum_w   = np.cumsum(w[order])
    tw      = cum_w[-1]
    thresholds = []
    for d in range(1, N_DECILES):
        # find the eq value at which cumulative weight crosses d/N_DECILES
        idx = np.searchsorted(cum_w, d / N_DECILES * tw)
        idx = min(idx, len(eq) - 1)
        thresholds.append(float(eq[order[idx]]))
    _ANCHOR_THRESHOLDS = thresholds
    return _ANCHOR_THRESHOLDS

GROUPS = {
    "earned":          ["employment_income", "self_employment_income"],
    "private_pension": ["private_pension_income"],
    "investment":      ["savings_interest", "dividend_income", "property_income",
                        "other_income", "maintenance_income", "miscellaneous_income"],
    "state_pension":   ["state_pension"],
    "working_age_ben": ["universal_credit", "child_tax_credit", "working_tax_credit",
                        "housing_benefit", "income_support", "esa_income_related",
                        "jsa_income_based", "carers_allowance"],
    "other_benefits":  ["child_benefit", "pension_credit", "passthrough_benefits",
                        "scottish_child_payment"],
    "taxes":           ["income_tax", "employee_ni", "council_tax"],
}

# Which source level each column comes from
PERSON_COLS = {
    "employment_income", "self_employment_income", "private_pension_income",
    "savings_interest", "dividend_income", "property_income",
    "other_income", "maintenance_income", "miscellaneous_income",
    "income_tax", "employee_ni",
}
BENUNIT_COLS = {
    "universal_credit", "child_tax_credit", "working_tax_credit",
    "housing_benefit", "income_support", "esa_income_related",
    "jsa_income_based", "carers_allowance", "child_benefit",
    "pension_credit", "passthrough_benefits", "scottish_child_payment",
    "state_pension",
}


def _run_year(year: int) -> dict:
    md = Simulation(year=year, data_dir=str(EFRS_DATA_ROOT)).run_microdata()

    hh = md.households.copy()
    p  = md.persons.copy()
    bu = md.benunits.copy()

    w   = hh["weight"].to_numpy()
    tw  = w.sum()
    ni  = hh["net_income"].to_numpy()
    eq_col = "equivalised_net_income" if "equivalised_net_income" in hh.columns else "net_income"
    eq  = hh[eq_col].to_numpy()

    # Decile assignment by cumulative weight within year
    order    = np.argsort(eq)
    cum_w    = np.cumsum(w[order])
    decile   = np.empty(len(hh), dtype=int)
    for i, idx in enumerate(order):
        d = min(int(cum_w[i] / tw * N_DECILES), N_DECILES - 1)
        decile[idx] = d

    hh["_decile"] = decile
    hh["_w"] = w

    # Aggregate person and benunit columns up to household
    for col in PERSON_COLS:
        if col in p.columns:
            agg = p.groupby("household_id")[col].sum()
            hh[col] = agg.reindex(hh["household_id"]).fillna(0).values
        else:
            hh[col] = 0.0

    for col in BENUNIT_COLS:
        if col in bu.columns:
            agg = bu.groupby("household_id")[col].sum()
            hh[col] = agg.reindex(hh["household_id"]).fillna(0).values
        else:
            hh[col] = 0.0

    # council_tax from household columns
    hh["council_tax"] = hh["council_tax_annual"].to_numpy() if "council_tax_annual" in hh.columns else 0.0

    # Weighted mean of each component per decile
    def _decile_wmean(col: str) -> list[float]:
        vals = hh[col].to_numpy()
        out = []
        for d in range(N_DECILES):
            mask = decile == d
            dw = w[mask]
            dv = vals[mask]
            out.append(float((dv * dw).sum() / dw.sum()) if dw.sum() > 0 else 0.0)
        return out

    net_income_by_decile = _decile_wmean("net_income")

    cpi = cpi_index_for_year(year)
    real = cpi_index_for_year(BASE_YEAR) / cpi

    result = {
        "year": year,
        "real_factor": real,
        "net_income": [x * real for x in net_income_by_decile],
    }
    for grp, cols in GROUPS.items():
        total = np.zeros(N_DECILES)
        for col in cols:
            arr = np.array(_decile_wmean(col))
            total += arr
        result[grp] = [x * real for x in total.tolist()]

    console.print(f"  {year}: mean net income (real) £{np.average(net_income_by_decile) * real:,.0f}")
    return result


def _decompose(rows: list[dict]) -> list[dict]:
    out = []
    for i in range(1, len(rows)):
        prev, curr = rows[i - 1], rows[i]
        yoy = [round(c - p, 2) for c, p in zip(curr["net_income"], prev["net_income"])]
        decile_pcts: dict[str, list[float]] = {"yoy": []}

        for d in range(N_DECILES):
            base = prev["net_income"][d]
            decile_pcts["yoy"].append(round(yoy[d] / base * 100, 3) if base else 0.0)

        for grp in GROUPS:
            pcts = []
            for d in range(N_DECILES):
                base = prev["net_income"][d]
                diff = curr[grp][d] - prev[grp][d]
                # taxes are a cost; invert so positive = benefit
                if grp == "taxes":
                    diff = -diff
                pcts.append(round(diff / base * 100, 3) if base else 0.0)
            decile_pcts[grp] = pcts

        out.append({
            "year_from":    prev["year"],
            "year_to":      curr["year"],
            "decile_pcts":  decile_pcts,
        })
    return out


def main() -> None:
    years = sorted(
        int(p.name) for p in EFRS_DATA_ROOT.iterdir()
        if p.is_dir() and p.name.isdigit() and (p / "households.csv").exists()
    )
    console.rule("[bold]EFRS per-decile income decomposition[/bold]")
    rows = [_run_year(y) for y in years]
    decomp = _decompose(rows)

    levels = [{"year": r["year"], "net_income": r["net_income"]} for r in rows]
    out = REPO_ROOT / "data" / "hbai_income_decomp.json"
    out.write_text(json.dumps(
        {"yoy_decomposition": decomp, "levels": levels, "n_deciles": N_DECILES,
         "base_year": BASE_YEAR, "groups": list(GROUPS.keys())},
        indent=2,
    ))
    console.print(f"\n[green]Wrote {out.relative_to(REPO_ROOT)}[/green]")


if __name__ == "__main__":
    main()
