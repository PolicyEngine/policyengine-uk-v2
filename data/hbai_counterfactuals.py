"""Per-decile YoY income decomposition via policy counterfactuals.

For each EFRS year, runs baseline + 4 counterfactuals and records per-decile
mean net income. The YoY change per decile is then decomposed into policy
attributions (PA freeze, NI cut, benefit uprating, 2CL repeal, residual).

Usage:
    python data/hbai_counterfactuals.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np
from rich.console import Console

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT / "interfaces" / "python"))

from policyengine_uk_compiled import Simulation, Parameters  # noqa: E402
from policyengine_uk_compiled.models import (  # noqa: E402
    IncomeTaxParams,
    NationalInsuranceParams,
    UniversalCreditParams,
    ChildBenefitParams,
    StatePensionParams,
)
from policyengine_uk_compiled.realterms import cpi_index_for_year  # noqa: E402

console = Console()

EFRS_DATA_ROOT = REPO_ROOT / "data" / "clean" / "efrs"
BASE_YEAR = 2026
N_DECILES = 10

PA_2020 = 12500.0
UC_SINGLE_OVER25_2024  = 393.45
UC_SINGLE_UNDER25_2024 = 311.68
UC_COUPLE_OVER25_2024  = 617.60
UC_COUPLE_UNDER25_2024 = 489.23
UC_CHILD_FIRST_2024    = 333.33
UC_CHILD_SUBSEQ_2024   = 287.92
SP_WEEKLY_2024         = 221.20
CB_ELDEST_2024         = 25.60
CB_ADDL_2024           = 16.95


def _cpi_uprated_pa(year: int) -> float:
    return PA_2020 * cpi_index_for_year(year) / cpi_index_for_year(2021)


def _decile_means(hh, w, ni, eq) -> list[float]:
    """Weighted mean net income per decile, ranked by equivalised net income."""
    tw = w.sum()
    order = np.argsort(eq)
    w_s, ni_s, cum_w = w[order], ni[order], np.cumsum(w[order])
    out = []
    for d in range(N_DECILES):
        lo, hi = d / N_DECILES * tw, (d + 1) / N_DECILES * tw
        mask = (cum_w > lo) & (cum_w <= hi)
        dw = w_s[mask]
        out.append(float((ni_s[mask] * dw).sum() / dw.sum()) if dw.sum() > 0 else 0.0)
    return out


def _run_year(year: int) -> dict:
    sim = Simulation(year=year, data_dir=str(EFRS_DATA_ROOT))
    cpi = cpi_index_for_year(year)
    real = cpi_index_for_year(BASE_YEAR) / cpi

    md = sim.run_microdata()
    hh = md.households
    w  = hh["weight"].to_numpy()
    ni = hh["net_income"].to_numpy()
    eq = hh["equivalised_net_income"].to_numpy() if "equivalised_net_income" in hh.columns else ni
    baseline_deciles = _decile_means(hh, w, ni, eq)
    baseline_mean = float((ni * w).sum() / w.sum())
    console.print(f"  {year}: baseline £{baseline_mean:,.0f}", end="")

    def _cf_deciles(policy) -> list[float]:
        md2 = sim.run_microdata(policy=policy)
        hh2 = md2.households
        ni2 = hh2["reform_net_income"].to_numpy() if "reform_net_income" in hh2.columns else hh2["net_income"].to_numpy()
        return _decile_means(hh2, w, ni2, eq)

    uprated_pa = _cpi_uprated_pa(year)
    cf_no_pa   = _cf_deciles(Parameters(income_tax=IncomeTaxParams(personal_allowance=uprated_pa)))
    cf_pre_ni  = _cf_deciles(Parameters(national_insurance=NationalInsuranceParams(main_rate=0.12)))
    cf_no_ben  = _cf_deciles(Parameters(
        universal_credit=UniversalCreditParams(
            standard_allowance_single_over25=UC_SINGLE_OVER25_2024,
            standard_allowance_single_under25=UC_SINGLE_UNDER25_2024,
            standard_allowance_couple_over25=UC_COUPLE_OVER25_2024,
            standard_allowance_couple_under25=UC_COUPLE_UNDER25_2024,
            child_element_first=UC_CHILD_FIRST_2024,
            child_element_subsequent=UC_CHILD_SUBSEQ_2024,
        ),
        state_pension=StatePensionParams(new_state_pension_weekly=SP_WEEKLY_2024),
        child_benefit=ChildBenefitParams(eldest_weekly=CB_ELDEST_2024, additional_weekly=CB_ADDL_2024),
    ))
    cf_2cl = _cf_deciles(Parameters(universal_credit=UniversalCreditParams(child_limit=2)))

    console.print(f" · done")
    return {
        "year": year,
        "real_factor": real,
        # All decile arrays are nominal; multiply by real_factor to compare across years
        "baseline": baseline_deciles,
        "cf_no_pa_freeze": cf_no_pa,
        "cf_pre_ni_cut":   cf_pre_ni,
        "cf_no_ben_uprate": cf_no_ben,
        "cf_no_2cl_repeal": cf_2cl,
    }


def _decompose(rows: list[dict]) -> list[dict]:
    """YoY per-decile decomposition in real terms (2026/27 prices)."""
    out = []
    for i in range(1, len(rows)):
        prev, curr = rows[i - 1], rows[i]
        rp, rc = prev["real_factor"], curr["real_factor"]

        def real(arr, rf): return [x * rf for x in arr]

        b_prev = real(prev["baseline"], rp)
        b_curr = real(curr["baseline"], rc)

        def attr(key):
            # impact in curr year = baseline - counterfactual (real)
            # YoY change in impact = curr impact - prev impact
            eff_curr = [b - c for b, c in zip(real(curr["baseline"], rc), real(curr[key], rc))]
            eff_prev = [b - c for b, c in zip(real(prev["baseline"], rp), real(prev[key], rp))]
            return [round(ec - ep, 2) for ec, ep in zip(eff_curr, eff_prev)]

        pa_attr  = attr("cf_no_pa_freeze")
        ni_attr  = attr("cf_pre_ni_cut")
        ben_attr = attr("cf_no_ben_uprate")
        tcl_attr = attr("cf_no_2cl_repeal")

        yoy = [round(c - p, 2) for c, p in zip(b_curr, b_prev)]
        residual = [round(y - pa - ni - ben - tcl, 2)
                    for y, pa, ni, ben, tcl in zip(yoy, pa_attr, ni_attr, ben_attr, tcl_attr)]

        out.append({
            "year_from": prev["year"],
            "year_to":   curr["year"],
            "yoy":       yoy,
            "pa_freeze":  pa_attr,
            "ni_cut":     ni_attr,
            "ben_uprate": ben_attr,
            "twocl":      tcl_attr,
            "residual":   residual,
        })
    return out


def main() -> None:
    years = sorted(
        int(p.name) for p in EFRS_DATA_ROOT.iterdir()
        if p.is_dir() and p.name.isdigit() and (p / "households.csv").exists()
    )
    console.rule("[bold]EFRS decile counterfactuals[/bold]")
    rows = [_run_year(y) for y in years]
    decomp = _decompose(rows)

    levels = [
        {"year": r["year"], "baseline_real": [x * r["real_factor"] for x in r["baseline"]]}
        for r in rows
    ]
    out = REPO_ROOT / "data" / "hbai_counterfactuals.json"
    out.write_text(json.dumps({"yoy_decomposition": decomp, "levels": levels, "n_deciles": N_DECILES, "base_year": BASE_YEAR}, indent=2))
    console.print(f"\n[green]Wrote {out.relative_to(REPO_ROOT)}[/green]")


if __name__ == "__main__":
    main()
