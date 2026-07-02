"""Compute YoY average real change in HBAI household net income (BHC) and its
components, for both the FRS (single-year surveys, 2008–2024) and the EFRS
(pooled/imputed/calibrated microdata, 2016–2030).

Each year is simulated at its own year's policy parameters (baseline, no
reform). Monetary values are deflated to 2026/27 real terms using the CPI
index embedded in the engine.

Usage:
    python data/hbai_income_trend.py                # prints table, writes JSON
    python data/hbai_income_trend.py --json-only    # suppress table
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np
import pandas as pd
from rich.console import Console
from rich.table import Table

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT / "interfaces" / "python"))

from policyengine_uk_compiled import Simulation  # noqa: E402
from policyengine_uk_compiled.realterms import cpi_index_for_year  # noqa: E402

console = Console()

BASE_YEAR = 2026  # real-terms base for deflation

# ── Data sources ──────────────────────────────────────────────────────────────

FRS_DATA_ROOT  = REPO_ROOT / "data" / "clean" / "frs"
EFRS_DATA_ROOT = REPO_ROOT / "data" / "clean" / "efrs"

# Engine expects the *parent* of the year subdir as data_dir.
FRS_YEARS  = sorted(int(p.name) for p in FRS_DATA_ROOT.iterdir()  if p.is_dir() and p.name.isdigit())
EFRS_YEARS = sorted(int(p.name) for p in EFRS_DATA_ROOT.iterdir() if p.is_dir() and p.name.isdigit())


# ── Component aggregation ─────────────────────────────────────────────────────

N_QUANTILES = 20  # vigintiles


def _quantile_means(hh: "pd.DataFrame", w: np.ndarray, eq_col: str) -> dict:
    """Weighted income at each vigintile midpoint of `eq_col` (growth
    incidence curve points), ranking by the same measure.

    Bin means are avoided deliberately: with cross-sectional re-ranking each
    year, households near vigintile boundaries swap bins between years and
    make adjacent-bin YoY growth saw-tooth even at fixed weights. Quantile
    points are robust to that composition churn.
    """
    if eq_col not in hh.columns:
        eq_col = "net_income"
    eq = hh[eq_col].to_numpy()
    order = np.argsort(eq)
    eq_s = eq[order]; w_s = w[order]
    cum_w = np.cumsum(w_s) - 0.5 * w_s
    ps = np.array([(q - 0.5) / N_QUANTILES for q in range(1, N_QUANTILES + 1)])
    vals = np.interp(ps * w_s.sum(), cum_w, eq_s)
    return {q: float(vals[q - 1]) for q in range(1, N_QUANTILES + 1)}


def _agg_one_year(year: int, data_root: Path) -> dict:
    """Run baseline simulation for one year and return weighted mean components.

    Returns a dict with all values in nominal £/household/year.
    """
    # Without a reform policy, run_microdata returns unprefixed columns.
    md = Simulation(year=year, data_dir=str(data_root)).run_microdata()

    hh  = md.households.copy()
    p   = md.persons.copy()
    bu  = md.benunits.copy()

    # RF-style non-pensioner restriction: drop households containing anyone
    # over state pension age (engine simplification: SPA = 66 for all years,
    # matching is_sp_age in src/engine/entities.rs). Ranking into vigintiles
    # then happens within the non-pensioner population only.
    hh_max_age = p.groupby("household_id")["age"].max()
    pensioner_hh = hh_max_age[hh_max_age >= 66.0].index
    hh = hh[~hh["household_id"].isin(pensioner_hh)].reset_index(drop=True)
    w   = hh["weight"].to_numpy()
    tw  = w.sum()

    def wmean_hh(col: str) -> float:
        return float((hh[col].to_numpy() * w).sum() / tw)

    # Aggregate person-level sums to household then weighted-mean
    def wmean_p(col: str) -> float:
        if col not in p.columns:
            return 0.0
        ps = p.groupby("household_id")[col].sum().reindex(hh["household_id"]).fillna(0).to_numpy()
        return float((ps * w).sum() / tw)

    # Aggregate benunit-level sums to household then weighted-mean
    def wmean_bu(col: str) -> float:
        if col not in bu.columns:
            return 0.0
        bs = bu.groupby("household_id")[col].sum().reindex(hh["household_id"]).fillna(0).to_numpy()
        return float((bs * w).sum() / tw)

    # ── Net income (BHC) ──
    net_income_bhc = wmean_hh("net_income")

    # ── Gross income components (person-level inputs) ──
    employment_income      = wmean_p("employment_income")
    self_employment_income = wmean_p("self_employment_income")
    private_pension_income = wmean_p("private_pension_income")
    savings_interest       = wmean_p("savings_interest")
    dividend_income        = wmean_p("dividend_income")
    property_income        = wmean_p("property_income")
    maintenance_income     = wmean_p("maintenance_income")
    other_income           = wmean_p("other_income")
    miscellaneous_income   = wmean_p("miscellaneous_income")

    # ── Direct taxes (person-level engine outputs, no prefix without reform) ──
    income_tax   = wmean_p("income_tax")
    employee_ni  = wmean_p("employee_ni")

    # ── Benefits (benunit-level, no prefix without reform) ──
    universal_credit       = wmean_bu("universal_credit")
    state_pension          = wmean_bu("state_pension")
    child_benefit          = wmean_bu("child_benefit")
    child_tax_credit       = wmean_bu("child_tax_credit")
    working_tax_credit     = wmean_bu("working_tax_credit")
    housing_benefit        = wmean_bu("housing_benefit")
    pension_credit         = wmean_bu("pension_credit")
    carers_allowance       = wmean_bu("carers_allowance")
    esa_income_related     = wmean_bu("esa_income_related")
    jsa_income_based       = wmean_bu("jsa_income_based")
    income_support         = wmean_bu("income_support")
    passthrough_benefits   = wmean_bu("passthrough_benefits")
    scottish_child_payment = wmean_bu("scottish_child_payment")

    # ── Council tax (household input column, not engine output) ──
    ct = hh["council_tax_annual"].to_numpy() if "council_tax_annual" in hh.columns else np.zeros(len(hh))
    council_tax = float((ct * w).sum() / tw)

    quantile_means = _quantile_means(hh, w, "equivalised_net_income")
    quantile_means_ahc = _quantile_means(hh, w, "equivalised_net_income_ahc")

    # Housing-cost aggregates for the AHC-specific deflator: weighted sums of
    # social rent, private rent, mortgage interest and council tax, and of BHC
    # net income, over the analysis population.
    tenure = hh["tenure_type"].astype(int).to_numpy() if "tenure_type" in hh.columns else np.zeros(len(hh), dtype=int)
    rent = hh["rent_annual"].to_numpy(dtype=float) if "rent_annual" in hh.columns else np.zeros(len(hh))
    mortgage = (
        hh["mortgage_interest_annual"].to_numpy(dtype=float)
        if "mortgage_interest_annual" in hh.columns
        else np.zeros(len(hh))
    )
    housing_agg = {
        "rent_social": float((rent * w)[np.isin(tenure, [2, 3])].sum()),
        "rent_private": float((rent * w)[tenure == 4].sum()),
        "mortgage_interest": float((mortgage * w).sum()),
        "council_tax": float((ct * w).sum()),
        "net_income": float((hh["net_income"].to_numpy() * w).sum()),
    }

    return {
        "year": year,
        "nominal_cpi": cpi_index_for_year(year),
        "housing_agg": housing_agg,
        "net_income_bhc": net_income_bhc,
        "quantile_net_income": quantile_means,
        "quantile_net_income_ahc": quantile_means_ahc,
        # Gross income components
        "employment_income": employment_income,
        "self_employment_income": self_employment_income,
        "private_pension_income": private_pension_income,
        "savings_interest": savings_interest,
        "dividend_income": dividend_income,
        "property_income": property_income,
        "maintenance_income": maintenance_income,
        "other_income": other_income,
        "miscellaneous_income": miscellaneous_income,
        # Direct taxes (sign: these reduce net income)
        "income_tax": income_tax,
        "employee_ni": employee_ni,
        "council_tax": council_tax,
        # Benefits
        "universal_credit": universal_credit,
        "state_pension": state_pension,
        "child_benefit": child_benefit,
        "child_tax_credit": child_tax_credit,
        "working_tax_credit": working_tax_credit,
        "housing_benefit": housing_benefit,
        "pension_credit": pension_credit,
        "carers_allowance": carers_allowance,
        "esa_income_related": esa_income_related,
        "jsa_income_based": jsa_income_based,
        "income_support": income_support,
        "passthrough_benefits": passthrough_benefits,
        "scottish_child_payment": scottish_child_payment,
    }


# ── Deflation and YoY ─────────────────────────────────────────────────────────

MONEY_COLS = [
    "net_income_bhc",
    "employment_income", "self_employment_income", "private_pension_income",
    "savings_interest", "dividend_income", "property_income",
    "maintenance_income", "other_income", "miscellaneous_income",
    "income_tax", "employee_ni", "council_tax",
    "universal_credit", "state_pension", "child_benefit",
    "child_tax_credit", "working_tax_credit", "housing_benefit",
    "pension_credit", "carers_allowance", "esa_income_related",
    "jsa_income_based", "income_support", "passthrough_benefits",
    "scottish_child_payment",
]

BASE_CPI = cpi_index_for_year(BASE_YEAR)


def _ahc_factor(df: pd.DataFrame) -> pd.Series:
    """Real-terms factor for AHC incomes using an AHC-specific deflator.

    HBAI convention: AHC incomes are deflated by a variant of CPI with housing
    costs stripped out, so that rent inflation is not double-counted (once as
    a deduction from income, again in the deflator). We decompose:

        CPI(y) = (1 - s) * D_ahc(y) + s * H(y)   =>   D_ahc = (CPI - s*H) / (1 - s)

    where H is a housing-cost price index (social rent, private rent,
    mortgage interest and council tax uprating factors, Laspeyres-weighted by
    the population's base-year expenditure on each) and s is the housing
    share, proxied by
    aggregate housing costs over aggregate BHC net income in the base year.
    The result is most sensitive to s: income understates the consumption
    base, so s errs high and the correction errs generous to AHC growth.
    """
    from uprate import cumulative_factor

    base_year = BASE_YEAR if (df["year"] == BASE_YEAR).any() else int(df["year"].max())
    base = df.loc[df["year"] == base_year].iloc[0]
    agg = base["housing_agg"]
    e = {"social_rent": agg["rent_social"], "rent": agg["rent_private"],
         "mortgage_interest": agg.get("mortgage_interest", 0.0),
         "council_tax": agg["council_tax"]}
    e_total = sum(e.values())
    s = e_total / agg["net_income"]

    base_cpi = float(df.loc[df["year"] == base_year, "nominal_cpi"].iloc[0])
    factors = []
    for _, row in df.iterrows():
        y = int(row["year"])
        c = row["nominal_cpi"] / base_cpi
        h = sum(w * cumulative_factor(base_year, y, idx) for idx, w in e.items()) / e_total
        d_ahc = (c - s * h) / (1.0 - s)
        factors.append(1.0 / d_ahc)
    return pd.Series(factors, index=df.index)


def deflate(rows: list[dict]) -> pd.DataFrame:
    df = pd.DataFrame(rows).sort_values("year").reset_index(drop=True)
    factor = BASE_CPI / df["nominal_cpi"]
    ahc_factor = _ahc_factor(df)
    for col in MONEY_COLS:
        df[f"real_{col}"] = df[col] * factor
    # Deflate per-quintile means (AHC series get the AHC-specific deflator)
    for q in range(1, N_QUANTILES + 1):
        df[f"quintile_{q}_net_income"] = df["quantile_net_income"].apply(lambda x: x[q]) * factor
        df[f"quintile_{q}_net_income_ahc"] = df["quantile_net_income_ahc"].apply(lambda x: x[q]) * ahc_factor
    return df


def yoy_changes(df: pd.DataFrame) -> pd.DataFrame:
    out = []
    for i in range(1, len(df)):
        prev, curr = df.iloc[i - 1], df.iloc[i]
        row = {"year_from": int(prev["year"]), "year_to": int(curr["year"])}
        for col in MONEY_COLS:
            row[f"d_{col}"] = round(curr[f"real_{col}"] - prev[f"real_{col}"], 2)
        # YoY % change per quintile, BHC and AHC
        for suffix, key in (("", "quantile_yoy_pct"), ("_ahc", "quantile_yoy_pct_ahc")):
            quintile_pct = {}
            for q in range(1, N_QUANTILES + 1):
                col = f"quintile_{q}_net_income{suffix}"
                prev_val = prev[col]
                curr_val = curr[col]
                quintile_pct[q] = round((curr_val - prev_val) / prev_val * 100, 3) if prev_val else 0.0
            row[key] = quintile_pct
        out.append(row)
    return pd.DataFrame(out)


# ── Output ────────────────────────────────────────────────────────────────────

def _print_table(title: str, yoy: pd.DataFrame) -> None:
    t = Table(title=title, show_header=True)
    t.add_column("Period")
    for col in MONEY_COLS:
        t.add_column(col.replace("_", " ").title(), justify="right")
    for _, r in yoy.iterrows():
        period = f"{int(r.year_from)}/{int(r.year_to)}"
        vals = []
        for col in MONEY_COLS:
            v = r[f"d_{col}"]
            colour = "green" if v > 0 else "red" if v < 0 else "dim"
            vals.append(f"[{colour}]{v:+.0f}[/{colour}]")
        t.add_row(period, *vals)
    console.print(t)


def run(years: list[int], data_root: Path, label: str) -> tuple[pd.DataFrame, pd.DataFrame]:
    rows = []
    for year in years:
        yr_dir = data_root / str(year)
        if not (yr_dir / "households.csv").exists():
            console.print(f"  [yellow]skip {year}: no data at {yr_dir}[/yellow]")
            continue
        console.print(f"  [{label}] {year}…", end=" ")
        rows.append(_agg_one_year(year, data_root))
        console.print(f"net income BHC £{rows[-1]['net_income_bhc']:,.0f}/yr nominal")
    df = deflate(rows)
    yoy = yoy_changes(df)
    return df, yoy


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json-only", action="store_true")
    parser.add_argument("--frs-only", action="store_true")
    parser.add_argument("--efrs-only", action="store_true")
    args = parser.parse_args()

    results = {}

    if not args.efrs_only:
        console.rule("[bold]FRS[/bold]")
        frs_df, frs_yoy = run(FRS_YEARS, FRS_DATA_ROOT, "FRS")
        if not args.json_only:
            _print_table(f"FRS — YoY real change in household income (£, {BASE_YEAR}/27 prices)", frs_yoy)
        frs_yoy_records = frs_yoy.to_dict(orient="records")
        results["frs"] = {
            "levels": frs_df[[c for c in frs_df.columns if c.startswith("real_") or c == "year"]].to_dict(orient="records"),
            "yoy": frs_yoy_records,
        }

    if not args.frs_only:
        console.rule("[bold]EFRS[/bold]")
        efrs_df, efrs_yoy = run(EFRS_YEARS, EFRS_DATA_ROOT, "EFRS")
        if not args.json_only:
            _print_table(f"EFRS — YoY real change in household income (£, {BASE_YEAR}/27 prices)", efrs_yoy)
        efrs_yoy_records = efrs_yoy.to_dict(orient="records")
        results["efrs"] = {
            "levels": efrs_df[[c for c in efrs_df.columns if c.startswith("real_") or c == "year"]].to_dict(orient="records"),
            "yoy": efrs_yoy_records,
        }

    out = REPO_ROOT / "data" / "hbai_income_trend.json"
    out.write_text(json.dumps(results, indent=2))
    console.print(f"\n[green]Wrote {out.relative_to(REPO_ROOT)}[/green]")


if __name__ == "__main__":
    main()
