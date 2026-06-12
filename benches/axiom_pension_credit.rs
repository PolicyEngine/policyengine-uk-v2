//! Axiom backend pension credit demo: SPCA 2002 s.2 guarantee credit at the
//! SPC Regs 2002 reg 6 weekly rates (standard minimum guarantee plus
//! severe-disability and carer additional amounts), topping claimant income
//! up to the appropriate minimum guarantee, with a single-rate reform.
//!
//! Loads the compiled artifact composed from rulespec-uk, evaluates the
//! baseline over a 100k-pensioner caseload, patches the single standard
//! minimum guarantee, recompiles in memory, and evaluates the reform.
//!
//! Configure with env vars:
//!   * `AXIOM_HH` — pensioner count (default 100_000)
//!
//! Run: `cargo bench --bench axiom_pension_credit`

#[path = "../src/axiom/mod.rs"]
#[allow(dead_code)]
mod axiom;

use std::time::Instant;

use axiom::{calculate, Dataset, Policy};
use chrono::NaiveDate;

const ARTIFACT: &str = include_str!("../src/axiom/artifacts/uk-pension-credit-fy2026.json");
const OUTPUT: &str = "guarantee_credit";
const SINGLE_SMG: &str = "uk:regulations/uksi/2002/1792/6#standard_minimum_guarantee_no_partner";

fn main() -> anyhow::Result<()> {
    let n: usize = std::env::var("AXIOM_HH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);

    // Weekly incomes spread around the single and couple standard minimum
    // guarantees; alternating partner status; severe-disability and carer
    // additions for slices of the caseload.
    let income: Vec<f64> = (0..n).map(|i| (i % 30) as f64 * 15.0).collect();
    let has_partner: Vec<bool> = (0..n).map(|i| i % 2 == 1).collect();
    let severely_disabled: Vec<bool> = (0..n).map(|i| i % 10 == 0).collect();
    let severe_couple_rate: Vec<bool> = (0..n).map(|i| i % 20 == 0).collect();
    let carer: Vec<bool> = (0..n).map(|i| i % 7 == 0).collect();
    let f = vec![false; n];

    let dataset = Dataset::week(NaiveDate::from_ymd_opt(2026, 4, 6).unwrap())
        .with_input("claimant_income", &income)?
        .with_bool_input("claimant_has_partner", &has_partner)?
        .with_bool_input(
            "treated_as_severely_disabled_person_under_schedule_i_part_i_paragraph_1",
            &severely_disabled,
        )?
        .with_bool_input("severe_disability_couple_rate_conditions_satisfied", &severe_couple_rate)?
        .with_bool_input("paragraph_4_of_part_ii_of_schedule_i_satisfied_for_this_partner", &carer)?
        .with_bool_input("awarded_tax_credit_under_tax_credits_act", &f)?
        .with_bool_input("tax_credit_award_circumstances_in_paragraph_15", &f)?
        .with_bool_input("tax_credit_decision_revised_in_favour_after_paragraph_16_event", &f)?
        .with_bool_input("detained_in_custody_for_more_than_52_weeks", &f)?
        .with_bool_input("detained_pending_trial_or_sentence_following_conviction_by_court", &f)?
        .with_bool_input("detained_for_period_not_exceeding_52_weeks", &f)?
        .with_bool_input("detained_in_custody_on_remand_pending_trial", &f)?
        .with_bool_input("required_as_condition_of_bail_to_reside_in_approved_hostel", &f)?
        .with_bool_input("detained_pending_sentence_upon_conviction", &f)?;

    let baseline = Policy::from_artifact_json(ARTIFACT, "Person")?;

    let t = Instant::now();
    let base = calculate(&baseline, dataset.clone(), &[OUTPUT])?;
    let baseline_ms = t.elapsed().as_secs_f64() * 1e3;

    // Reform: single standard minimum guarantee 238.00 -> 250.00 per week.
    let t = Instant::now();
    let reform_policy = baseline.with_parameter(
        SINGLE_SMG,
        NaiveDate::from_ymd_opt(2026, 4, 6).unwrap(),
        250.00,
    )?;
    let recompile_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let reform = calculate(&reform_policy, dataset, &[OUTPUT])?;
    let reform_ms = t.elapsed().as_secs_f64() * 1e3;

    let base_col = base.column(OUTPUT)?;
    let reform_col = reform.column(OUTPUT)?;
    let base_total: f64 = base_col.iter().sum();
    let delta: f64 = reform_col.iter().zip(base_col).map(|(r, b)| r - b).sum();
    let gainers = reform_col.iter().zip(base_col).filter(|(r, b)| r > b).count();

    println!("axiom pension credit demo: single SMG 238.00 -> 250.00, {n} pensioners");
    println!("  baseline eval:    {baseline_ms:.1} ms");
    println!("  reform recompile: {recompile_ms:.1} ms");
    println!("  reform eval:      {reform_ms:.1} ms");
    println!("  unweighted synthetic baseline total: {base_total:.2}/week");
    println!("  unweighted synthetic delta: {delta:.2}/week across {gainers} gainers");
    Ok(())
}
