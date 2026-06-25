//! Reusable scoring pipeline shared by the CLI binary and the python
//! bindings: dataset loading, baseline and reform runs, and the
//! population-level analysis that produces the JSON output.

use serde::Serialize;
use std::path::Path;

use crate::data::clean::load_clean_dataset;
use crate::data::Dataset;
use crate::engine::simulation::SimulationResults;
use crate::engine::Simulation;
use crate::parameters::Parameters;
use crate::variables::labour_supply;

#[derive(Serialize, Clone)]
pub struct HbaiIncomes {
    /// Weighted mean equivalised net income BHC
    pub mean_equiv_bhc: f64,
    /// Weighted mean equivalised net income AHC
    pub mean_equiv_ahc: f64,
    /// Weighted mean net income BHC (non-equivalised)
    pub mean_bhc: f64,
    /// Weighted mean net income AHC (non-equivalised)
    pub mean_ahc: f64,
    /// Median equivalised net income BHC (poverty reference line = 60% of this)
    pub median_equiv_bhc: f64,
    /// Median equivalised net income AHC
    pub median_equiv_ahc: f64,
}

#[derive(Serialize)]
pub struct PovertyHeadcounts {
    /// Relative poverty (60% median BHC equiv), children
    pub relative_bhc_children: f64,
    /// Relative poverty (60% median BHC equiv), working-age adults
    pub relative_bhc_working_age: f64,
    /// Relative poverty (60% median BHC equiv), pensioners
    pub relative_bhc_pensioners: f64,
    /// Relative poverty (60% median AHC equiv), children
    pub relative_ahc_children: f64,
    /// Relative poverty (60% median AHC equiv), working-age adults
    pub relative_ahc_working_age: f64,
    /// Relative poverty (60% median AHC equiv), pensioners
    pub relative_ahc_pensioners: f64,
    /// Absolute poverty (60% median BHC equiv fixed at 2010/11 baseline), BHC, children
    pub absolute_bhc_children: f64,
    /// Absolute poverty BHC, working-age adults
    pub absolute_bhc_working_age: f64,
    /// Absolute poverty BHC, pensioners
    pub absolute_bhc_pensioners: f64,
    /// Absolute poverty AHC, children
    pub absolute_ahc_children: f64,
    /// Absolute poverty AHC, working-age adults
    pub absolute_ahc_working_age: f64,
    /// Absolute poverty AHC, pensioners
    pub absolute_ahc_pensioners: f64,
}

#[derive(Serialize)]
pub struct JsonOutput {
    pub fiscal_year: String,
    pub budgetary_impact: BudgetaryImpact,
    pub income_breakdown: IncomeBreakdown,
    pub program_breakdown: ProgramBreakdown,
    pub caseloads: Caseloads,
    pub decile_impacts: Vec<DecileImpact>,
    pub winners_losers: WinnersLosers,
    pub baseline_hbai_incomes: HbaiIncomes,
    pub reform_hbai_incomes: HbaiIncomes,
    pub baseline_poverty: PovertyHeadcounts,
    pub reform_poverty: PovertyHeadcounts,
    /// CPI index (2025/26 = 100) for deflating nominal values to real terms.
    pub cpi_index: f64,
}

pub use crate::data::clean::cpi_index_for_year;

#[derive(Serialize, Clone, Copy)]
pub struct BudgetaryImpact {
    pub baseline_revenue: f64,
    pub reform_revenue: f64,
    pub revenue_change: f64,
    pub baseline_benefits: f64,
    pub reform_benefits: f64,
    pub benefit_spending_change: f64,
    pub net_cost: f64,
}

#[derive(Serialize)]
pub struct IncomeBreakdown {
    pub employment_income: f64,
    pub self_employment_income: f64,
    pub pension_income: f64,
    pub savings_interest_income: f64,
    pub dividend_income: f64,
    pub property_income: f64,
    pub other_income: f64,
}

#[derive(Serialize)]
pub struct ProgramBreakdown {
    pub income_tax: f64,
    pub hicbc: f64,
    pub employee_ni: f64,
    pub employer_ni: f64,
    pub vat: f64,
    pub fuel_duty: f64,
    pub alcohol_duty: f64,
    pub tobacco_duty: f64,
    pub capital_gains_tax: f64,
    pub stamp_duty: f64,
    pub wealth_tax: f64,
    pub council_tax: f64,
    pub universal_credit: f64,
    pub child_benefit: f64,
    pub state_pension: f64,
    pub pension_credit: f64,
    pub housing_benefit: f64,
    pub child_tax_credit: f64,
    pub working_tax_credit: f64,
    pub income_support: f64,
    pub esa_income_related: f64,
    pub jsa_income_based: f64,
    pub carers_allowance: f64,
    pub scottish_child_payment: f64,
    pub benefit_cap_reduction: f64,
    pub passthrough_benefits: f64,
}

#[derive(Serialize)]
pub struct Caseloads {
    pub income_tax_payers: f64,
    pub ni_payers: f64,
    pub employer_ni_payers: f64,
    pub universal_credit: f64,
    pub child_benefit: f64,
    pub state_pension: f64,
    pub pension_credit: f64,
    pub housing_benefit: f64,
    pub child_tax_credit: f64,
    pub working_tax_credit: f64,
    pub income_support: f64,
    pub esa_income_related: f64,
    pub jsa_income_based: f64,
    pub carers_allowance: f64,
    pub scottish_child_payment: f64,
    pub benefit_cap_affected: f64,
}

#[derive(Serialize)]
pub struct DecileImpact {
    pub decile: usize,
    pub avg_baseline_income: f64,
    pub avg_reform_income: f64,
    pub avg_change: f64,
    pub pct_change: f64,
}

#[derive(Serialize)]
pub struct WinnersLosers {
    pub winners_pct: f64,
    pub losers_pct: f64,
    pub unchanged_pct: f64,
    pub avg_gain: f64,
    pub avg_loss: f64,
}

/// Load a clean dataset from a base dir with per-year subdirs, falling back
/// to the latest available year and uprating to the requested one.
/// Uses YAML-backed growth factors when `params_dir` is provided.
pub fn load_dataset_dir(base: &Path, year: u32) -> anyhow::Result<Dataset> {
    load_dataset_dir_inner(base, year, None)
}

pub fn load_dataset_dir_with_params(base: &Path, params_dir: &Path, year: u32) -> anyhow::Result<Dataset> {
    load_dataset_dir_inner(base, year, Some(params_dir))
}

fn load_dataset_dir_inner(base: &Path, year: u32, params_dir: Option<&Path>) -> anyhow::Result<Dataset> {
    let year_dir = base.join(year.to_string());
    if year_dir.is_dir() {
        load_clean_dataset(&year_dir, year)
    } else {
        let latest = (1994..=year).rev()
            .find(|y| base.join(y.to_string()).is_dir())
            .ok_or_else(|| anyhow::anyhow!("No clean data found in {}", base.display()))?;
        let mut ds = load_clean_dataset(&base.join(latest.to_string()), latest)?;
        match params_dir {
            Some(p) => ds.uprate_to_dir(year, p),
            None    => ds.uprate_to(year),
        }
        Ok(ds)
    }
}

/// Run the baseline simulation over the dataset.
pub fn run_baseline(dataset: &Dataset, params: &Parameters, year: u32) -> SimulationResults {
    Simulation::new(
        dataset.people.clone(),
        dataset.benunits.clone(),
        dataset.households.clone(),
        params.clone(),
        year,
    ).run()
}

/// Labour supply responses plus the policy simulation.
pub fn run_reform(
    dataset: &Dataset,
    baseline_params: &Parameters,
    policy_params: &Parameters,
    baseline: &SimulationResults,
    year: u32,
) -> SimulationResults {
    let ls_baseline = if policy_params.labour_supply.enabled {
        let baseline_net: Vec<f64> = baseline.household_results.iter()
            .map(|hr| hr.net_income)
            .collect();
        labour_supply::compute_baseline_retention(
            &dataset.people, &dataset.benunits, &dataset.households,
            baseline_params, &baseline_net, year,
        )
    } else {
        None
    };
    run_reform_with_baseline_retention(
        dataset, policy_params, baseline, ls_baseline.as_ref(), year,
    )
}

/// As [`run_reform`], but with the reform-independent baseline retention
/// stage supplied by the caller (so it can be cached across reforms).
/// `ls_baseline` of `None` means labour supply is disabled or there are no
/// eligible workers.
pub fn run_reform_with_baseline_retention(
    dataset: &Dataset,
    policy_params: &Parameters,
    baseline: &SimulationResults,
    ls_baseline: Option<&labour_supply::BaselineRetention>,
    year: u32,
) -> SimulationResults {
    let policy_people = match ls_baseline {
        Some(base) if policy_params.labour_supply.enabled => {
            let baseline_net: Vec<f64> = baseline.household_results.iter()
                .map(|hr| hr.net_income)
                .collect();
            labour_supply::apply_labour_supply_responses_with_baseline(
                &dataset.people, &dataset.benunits, &dataset.households,
                policy_params, &baseline_net, base, year,
            )
        }
        _ => dataset.people.clone(),
    };
    Simulation::new(
        policy_people,
        dataset.benunits.clone(),
        dataset.households.clone(),
        policy_params.clone(),
        year,
    ).run()
}

/// Population-level analysis of a baseline/reform pair.
pub fn analyse(
    dataset: &Dataset,
    baseline_params: &Parameters,
    baseline: &SimulationResults,
    reformed: &SimulationResults,
    year: u32,
) -> JsonOutput {
    // Analysis
    let households = &dataset.households;

    let baseline_revenue: f64 = households.iter()
        .map(|h| h.weight * baseline.household_results[h.id].total_tax)
        .sum();
    let reform_revenue: f64 = households.iter()
        .map(|h| h.weight * reformed.household_results[h.id].total_tax)
        .sum();
    let revenue_change = reform_revenue - baseline_revenue;

    let baseline_benefits: f64 = households.iter()
        .map(|h| h.weight * baseline.household_results[h.id].total_benefits)
        .sum();
    let reform_benefits: f64 = households.iter()
        .map(|h| h.weight * reformed.household_results[h.id].total_benefits)
        .sum();
    let benefit_change = reform_benefits - baseline_benefits;
    let net_cost = -revenue_change + benefit_change;

    // Decile analysis — ranked by equivalised HBAI net income BHC (baseline).
    // Changes are measured on equivalised extended net income (HBAI minus stamp duty/wealth tax),
    // so that reforms to those taxes show up in decile impacts and winners/losers.
    let mut hh_incomes: Vec<(usize, f64, f64, f64)> = households.iter().map(|hh| {
        let bl = &baseline.household_results[hh.id];
        let rf = &reformed.household_results[hh.id];
        let eq = bl.equivalisation_factor.max(1e-9);
        (hh.id,
         bl.equivalised_net_income,
         bl.extended_net_income / eq,
         rf.extended_net_income / eq)
    }).collect();
    hh_incomes.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    let decile_size = hh_incomes.len() / 10;
    let mut decile_impacts = Vec::new();
    for d in 0..10 {
        let start = d * decile_size;
        let end = if d == 9 { hh_incomes.len() } else { (d + 1) * decile_size };
        let slice = &hh_incomes[start..end];
        let n = slice.len() as f64;
        let avg_base: f64 = slice.iter().map(|h| h.2).sum::<f64>() / n;   // baseline extended
        let avg_reform: f64 = slice.iter().map(|h| h.3).sum::<f64>() / n; // reform extended
        let avg_change = avg_reform - avg_base;
        let pct_change = if avg_base != 0.0 { 100.0 * avg_change / avg_base } else { 0.0 };
        decile_impacts.push(DecileImpact {
            decile: d + 1,
            avg_baseline_income: (avg_base * 100.0).round() / 100.0,
            avg_reform_income: (avg_reform * 100.0).round() / 100.0,
            avg_change: (avg_change * 100.0).round() / 100.0,
            pct_change: (pct_change * 100.0).round() / 100.0,
        });
    }

    // Winners and losers
    let mut winners = 0.0f64;
    let mut losers = 0.0f64;
    let mut unchanged = 0.0f64;
    let mut total_gain = 0.0f64;
    let mut total_loss = 0.0f64;

    for hh in households {
        let change = reformed.household_results[hh.id].extended_net_income
            - baseline.household_results[hh.id].extended_net_income;
        if change > 1.0 {
            winners += hh.weight;
            total_gain += hh.weight * change;
        } else if change < -1.0 {
            losers += hh.weight;
            total_loss += hh.weight * change;
        } else {
            unchanged += hh.weight;
        }
    }

    let total_hh = winners + losers + unchanged;
    let winners_losers = WinnersLosers {
        winners_pct: (1000.0 * winners / total_hh).round() / 10.0,
        losers_pct: (1000.0 * losers / total_hh).round() / 10.0,
        unchanged_pct: (1000.0 * unchanged / total_hh).round() / 10.0,
        avg_gain: if winners > 0.0 { (total_gain / winners).round() } else { 0.0 },
        avg_loss: if losers > 0.0 { (total_loss.abs() / losers).round() } else { 0.0 },
    };

    // Program-level breakdown and caseloads (weighted totals from reform)
    let benunits = &dataset.benunits;
    let people = &dataset.people;
    let (income_breakdown, program_breakdown, caseloads) = {
        // Income aggregates
        let mut total_employment = 0.0f64;
        let mut total_self_employment = 0.0f64;
        let mut total_pension = 0.0f64;
        let mut total_savings = 0.0f64;
        let mut total_dividend = 0.0f64;
        let mut total_property = 0.0f64;
        let mut total_other = 0.0f64;
        // Tax spending and caseloads
        let mut income_tax = 0.0f64;
        let mut hicbc_total = 0.0f64;
        let mut employee_ni = 0.0f64;
        let mut employer_ni = 0.0f64;
        let mut vat_total = 0.0f64;
        let mut fuel_duty_total = 0.0f64;
        let mut alcohol_duty_total = 0.0f64;
        let mut tobacco_duty_total = 0.0f64;
        let mut cgt_total = 0.0f64;
        let mut stamp_duty_total = 0.0f64;
        let mut wealth_tax_total = 0.0f64;
        let mut council_tax_total = 0.0f64;
        let mut it_payers = 0.0f64;
        let mut ni_payers = 0.0f64;
        let mut eni_payers = 0.0f64;
        for hh in households {
            let hr = &reformed.household_results[hh.id];
            vat_total += hh.weight * hr.vat;
            fuel_duty_total += hh.weight * hr.fuel_duty;
            alcohol_duty_total += hh.weight * hr.alcohol_duty;
            tobacco_duty_total += hh.weight * hr.tobacco_duty;
            cgt_total += hh.weight * hr.capital_gains_tax;
            stamp_duty_total += hh.weight * hr.stamp_duty;
            wealth_tax_total += hh.weight * hr.wealth_tax;
            council_tax_total += hh.weight * hh.council_tax;
            for &pid in &hh.person_ids {
                let person = &people[pid];
                total_employment += hh.weight * person.employment_income;
                total_self_employment += hh.weight * person.self_employment_income;
                total_pension += hh.weight * person.pension_income;
                total_savings += hh.weight * person.savings_interest_income;
                total_dividend += hh.weight * person.dividend_income;
                total_property += hh.weight * person.property_income;
                total_other += hh.weight * (person.maintenance_income + person.miscellaneous_income + person.other_income);
                let pr = &reformed.person_results[pid];
                income_tax += hh.weight * pr.income_tax;
                hicbc_total += hh.weight * pr.hicbc;
                employee_ni += hh.weight * pr.national_insurance;
                employer_ni += hh.weight * pr.employer_ni;
                if pr.income_tax > 0.0 { it_payers += hh.weight; }
                if pr.national_insurance > 0.0 { ni_payers += hh.weight; }
                if pr.employer_ni > 0.0 { eni_payers += hh.weight; }
            }
        }
        // Benefit spending and caseloads
        let mut uc = 0.0f64;
        let mut cb = 0.0f64;
        let mut sp = 0.0f64;
        let mut pc = 0.0f64;
        let mut hb = 0.0f64;
        let mut ctc = 0.0f64;
        let mut wtc = 0.0f64;
        let mut is_val = 0.0f64;
        let mut esa_ir = 0.0f64;
        let mut jsa_ib = 0.0f64;
        let mut ca = 0.0f64;
        let mut scp = 0.0f64;
        let mut cap = 0.0f64;
        let mut passthrough = 0.0f64;
        let mut cl_uc = 0.0f64;
        let mut cl_cb = 0.0f64;
        let mut cl_sp = 0.0f64;
        let mut cl_pc = 0.0f64;
        let mut cl_hb = 0.0f64;
        let mut cl_ctc = 0.0f64;
        let mut cl_wtc = 0.0f64;
        let mut cl_is = 0.0f64;
        let mut cl_esa = 0.0f64;
        let mut cl_jsa = 0.0f64;
        let mut cl_ca = 0.0f64;
        let mut cl_scp = 0.0f64;
        let mut cl_cap = 0.0f64;
        for bu in benunits {
            let w = households[bu.household_id].weight;
            let br = &reformed.benunit_results[bu.id];
            uc += w * br.universal_credit;
            cb += w * br.child_benefit;
            sp += w * br.state_pension;
            pc += w * br.pension_credit;
            hb += w * br.housing_benefit;
            ctc += w * br.child_tax_credit;
            wtc += w * br.working_tax_credit;
            is_val += w * br.income_support;
            esa_ir += w * br.esa_income_related;
            jsa_ib += w * br.jsa_income_based;
            ca += w * br.carers_allowance;
            scp += w * br.scottish_child_payment;
            cap += w * br.benefit_cap_reduction;
            passthrough += w * br.passthrough_benefits;
            if br.universal_credit > 0.0 { cl_uc += w; }
            if br.child_benefit > 0.0 { cl_cb += w; }
            if br.state_pension > 0.0 { cl_sp += w; }
            if br.pension_credit > 0.0 { cl_pc += w; }
            if br.housing_benefit > 0.0 { cl_hb += w; }
            if br.child_tax_credit > 0.0 { cl_ctc += w; }
            if br.working_tax_credit > 0.0 { cl_wtc += w; }
            if br.income_support > 0.0 { cl_is += w; }
            if br.esa_income_related > 0.0 { cl_esa += w; }
            if br.jsa_income_based > 0.0 { cl_jsa += w; }
            if br.carers_allowance > 0.0 { cl_ca += w; }
            if br.scottish_child_payment > 0.0 { cl_scp += w; }
            if br.benefit_cap_reduction > 0.0 { cl_cap += w; }
        }
        (IncomeBreakdown {
            employment_income: total_employment,
            self_employment_income: total_self_employment,
            pension_income: total_pension,
            savings_interest_income: total_savings,
            dividend_income: total_dividend,
            property_income: total_property,
            other_income: total_other,
        }, ProgramBreakdown {
            income_tax,
            hicbc: hicbc_total,
            employee_ni,
            employer_ni,
            vat: vat_total,
            fuel_duty: fuel_duty_total,
            alcohol_duty: alcohol_duty_total,
            tobacco_duty: tobacco_duty_total,
            capital_gains_tax: cgt_total,
            stamp_duty: stamp_duty_total,
            wealth_tax: wealth_tax_total,
            council_tax: council_tax_total,
            universal_credit: uc,
            child_benefit: cb,
            state_pension: sp,
            pension_credit: pc,
            housing_benefit: hb,
            child_tax_credit: ctc,
            working_tax_credit: wtc,
            income_support: is_val,
            esa_income_related: esa_ir,
            jsa_income_based: jsa_ib,
            carers_allowance: ca,
            scottish_child_payment: scp,
            benefit_cap_reduction: cap,
            passthrough_benefits: passthrough,
        }, Caseloads {
            income_tax_payers: it_payers,
            ni_payers,
            employer_ni_payers: eni_payers,
            universal_credit: cl_uc,
            child_benefit: cl_cb,
            state_pension: cl_sp,
            pension_credit: cl_pc,
            housing_benefit: cl_hb,
            child_tax_credit: cl_ctc,
            working_tax_credit: cl_wtc,
            income_support: cl_is,
            esa_income_related: cl_esa,
            jsa_income_based: cl_jsa,
            carers_allowance: cl_ca,
            scottish_child_payment: cl_scp,
            benefit_cap_affected: cl_cap,
        })
    };

    // ── HBAI income aggregates ────────────────────────────────────────────────
    let total_weight: f64 = households.iter().map(|h| h.weight).sum();

    let compute_hbai_incomes = |results: &crate::engine::simulation::SimulationResults| -> HbaiIncomes {
        // Weighted median over individuals: each person carries the household's weight.
        let total_person_weight: f64 = households.iter()
            .map(|h| h.weight * (h.person_ids.len() as f64))
            .sum();
        let weighted_median = |vals: &mut Vec<(f64, f64)>| -> f64 {
            vals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let half = total_person_weight / 2.0;
            let mut cum = 0.0;
            for &(v, w) in vals.iter() {
                cum += w;
                if cum >= half { return v; }
            }
            vals.last().map(|&(v, _)| v).unwrap_or(0.0)
        };

        let mut equiv_bhc: Vec<(f64, f64)> = households.iter()
            .map(|h| {
                let w = h.weight * (h.person_ids.len() as f64);
                (results.household_results[h.id].equivalised_net_income, w)
            })
            .collect();
        let mut equiv_ahc: Vec<(f64, f64)> = households.iter()
            .map(|h| {
                let w = h.weight * (h.person_ids.len() as f64);
                (results.household_results[h.id].equivalised_net_income_ahc, w)
            })
            .collect();

        let median_equiv_bhc = weighted_median(&mut equiv_bhc);
        let median_equiv_ahc = weighted_median(&mut equiv_ahc);

        // HBAI mean equivalised income is person-weighted: each person is assigned their
        // household's equivalised income, then averaged across all persons.
        let total_person_weight: f64 = households.iter()
            .map(|h| h.weight * h.person_ids.len() as f64)
            .sum();
        let mean_equiv_bhc = households.iter()
            .map(|h| h.weight * (h.person_ids.len() as f64) * results.household_results[h.id].equivalised_net_income)
            .sum::<f64>() / total_person_weight;
        let mean_equiv_ahc = households.iter()
            .map(|h| h.weight * (h.person_ids.len() as f64) * results.household_results[h.id].equivalised_net_income_ahc)
            .sum::<f64>() / total_person_weight;
        let mean_bhc = households.iter()
            .map(|h| h.weight * results.household_results[h.id].net_income)
            .sum::<f64>() / total_weight;
        let mean_ahc = households.iter()
            .map(|h| h.weight * results.household_results[h.id].net_income_ahc)
            .sum::<f64>() / total_weight;

        HbaiIncomes { mean_equiv_bhc, mean_equiv_ahc, mean_bhc, mean_ahc,
                      median_equiv_bhc, median_equiv_ahc }
    };
    let baseline_hbai_incomes = compute_hbai_incomes(&baseline);
    let reform_hbai_incomes = compute_hbai_incomes(&reformed);

    // ── Poverty headcounts ────────────────────────────────────────────────────
    // Relative lines: 60% of baseline weighted median equivalised income
    let rel_line_bhc = 0.60 * baseline_hbai_incomes.median_equiv_bhc;
    let rel_line_ahc = 0.60 * baseline_hbai_incomes.median_equiv_ahc;
    // Absolute lines: 60% of median in 2010/11 (ONS HBAI reference, uprated by CPI to nominal)
    // 2010/11 median equivalised BHC ~£14,400/yr; AHC ~£11,600/yr (2010/11 prices)
    // Uprate to simulation year using CPI index
    let cpi = cpi_index_for_year(year) / 100.0;
    let abs_line_bhc = 14_400.0 * cpi;
    let abs_line_ahc = 11_600.0 * cpi;

    let compute_poverty = |results: &crate::engine::simulation::SimulationResults| -> PovertyHeadcounts {
        let mut rc_children = 0.0f64; let mut rc_working = 0.0f64; let mut rc_pensioners = 0.0f64;
        let mut ra_children = 0.0f64; let mut ra_working = 0.0f64; let mut ra_pensioners = 0.0f64;
        let mut ac_children = 0.0f64; let mut ac_working = 0.0f64; let mut ac_pensioners = 0.0f64;
        let mut aa_children = 0.0f64; let mut aa_working = 0.0f64; let mut aa_pensioners = 0.0f64;
        let mut total_children = 0.0f64; let mut total_working = 0.0f64; let mut total_pensioners = 0.0f64;

        for hh in households {
            let hr = &results.household_results[hh.id];
            let eq_bhc = hr.equivalised_net_income;
            let eq_ahc = hr.equivalised_net_income_ahc;
            let w = hh.weight;

            for &pid in &hh.person_ids {
                let age = dataset.people[pid].age;
                let pw = w; // person weight = household weight (no person-level weights)
                let (child, working, pensioner) = if age < 16.0 {
                    (true, false, false)
                } else if age < 66.0 {
                    (false, true, false)
                } else {
                    (false, false, true)
                };

                if child   { total_children   += pw; }
                if working { total_working    += pw; }
                if pensioner { total_pensioners += pw; }

                if eq_bhc < rel_line_bhc {
                    if child { rc_children += pw; } else if working { rc_working += pw; } else { rc_pensioners += pw; }
                }
                if eq_ahc < rel_line_ahc {
                    if child { ra_children += pw; } else if working { ra_working += pw; } else { ra_pensioners += pw; }
                }
                if eq_bhc < abs_line_bhc {
                    if child { ac_children += pw; } else if working { ac_working += pw; } else { ac_pensioners += pw; }
                }
                if eq_ahc < abs_line_ahc {
                    if child { aa_children += pw; } else if working { aa_working += pw; } else { aa_pensioners += pw; }
                }
            }
        }

        let pct = |n: f64, d: f64| if d > 0.0 { (n / d * 1000.0).round() / 10.0 } else { 0.0 };
        PovertyHeadcounts {
            relative_bhc_children:    pct(rc_children,    total_children),
            relative_bhc_working_age: pct(rc_working,     total_working),
            relative_bhc_pensioners:  pct(rc_pensioners,  total_pensioners),
            relative_ahc_children:    pct(ra_children,    total_children),
            relative_ahc_working_age: pct(ra_working,     total_working),
            relative_ahc_pensioners:  pct(ra_pensioners,  total_pensioners),
            absolute_bhc_children:    pct(ac_children,    total_children),
            absolute_bhc_working_age: pct(ac_working,     total_working),
            absolute_bhc_pensioners:  pct(ac_pensioners,  total_pensioners),
            absolute_ahc_children:    pct(aa_children,    total_children),
            absolute_ahc_working_age: pct(aa_working,     total_working),
            absolute_ahc_pensioners:  pct(aa_pensioners,  total_pensioners),
        }
    };

    let baseline_poverty = compute_poverty(&baseline);
    let reform_poverty   = compute_poverty(&reformed);

    JsonOutput {
        fiscal_year: baseline_params.fiscal_year.clone(),
        budgetary_impact: BudgetaryImpact {
            baseline_revenue,
            reform_revenue,
            revenue_change,
            baseline_benefits,
            reform_benefits,
            benefit_spending_change: benefit_change,
            net_cost,
        },
        income_breakdown,
        program_breakdown,
        caseloads,
        decile_impacts,
        winners_losers,
        baseline_hbai_incomes,
        reform_hbai_incomes,
        baseline_poverty,
        reform_poverty,
        cpi_index: cpi_index_for_year(year),
    }
}
