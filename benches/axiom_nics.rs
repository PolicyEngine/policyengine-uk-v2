//! Axiom backend National Insurance demo: Class 1 (weekly, employees) and
//! Class 4 (annual, self-employed) contributions with a rate reform each.
//!
//! Loads the compiled SSCBA 1992 s.8 + SSCR 2001 reg 10 artifact (primary
//! Class 1 over a tax week) and the s.15 artifact (Class 4 over a tax
//! year), composed from rulespec-uk, then for each: evaluates the baseline,
//! patches the main percentage up by one point, recompiles in memory, and
//! evaluates the reform.
//!
//! Configure with env vars:
//!   * `AXIOM_HH` — person count per program (default 100_000)
//!
//! Run: `cargo bench --bench axiom_nics`

#[path = "../src/axiom/mod.rs"]
#[allow(dead_code)]
mod axiom;

use std::time::Instant;

use axiom::{calculate, Dataset, Policy};
use chrono::NaiveDate;

const CLASS_1: &str = include_str!("../src/axiom/artifacts/uk-nics-class-1-fy2026.json");
const CLASS_4: &str = include_str!("../src/axiom/artifacts/uk-nics-class-4-fy2026.json");

fn main() -> anyhow::Result<()> {
    let n: usize = std::env::var("AXIOM_HH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);

    // Class 1: weekly earnings spread across the primary threshold and the
    // upper earnings limit, first tax week of 2026-27.
    let weekly_earnings: Vec<f64> = (0..n).map(|i| (i % 30) as f64 * 50.0).collect();
    let dataset = Dataset::week(NaiveDate::from_ymd_opt(2026, 4, 6).unwrap())
        .with_input("earnings_paid_in_tax_week_in_respect_of_employment", &weekly_earnings)?;
    run_reform(
        "Class 1 primary, main rate 0.08 -> 0.09",
        &Policy::from_artifact_json(CLASS_1, "Person")?,
        &dataset,
        "primary_class_1_contribution",
        "uk:statutes/ukpga/1992/4/8#main_primary_percentage",
        0.09,
        n,
    )?;

    // Class 4: annual profits spread across the lower and upper profits
    // limits, tax year 2026-27.
    let profits: Vec<f64> = (0..n).map(|i| (i % 30) as f64 * 2_500.0).collect();
    let dataset = Dataset::tax_year(2026)
        .with_input("profits_chargeable_to_class_4_contributions", &profits)?;
    run_reform(
        "Class 4, main rate 0.06 -> 0.07",
        &Policy::from_artifact_json(CLASS_4, "Person")?,
        &dataset,
        "class_4_contribution_before_annual_maximum",
        "uk:statutes/ukpga/1992/4/15#main_class_4_percentage",
        0.07,
        n,
    )?;
    Ok(())
}

fn run_reform(
    label: &str,
    baseline: &Policy,
    dataset: &Dataset,
    output: &str,
    parameter: &str,
    value: f64,
    n: usize,
) -> anyhow::Result<()> {
    let t = Instant::now();
    let base = calculate(baseline, dataset.clone(), &[output])?;
    let baseline_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let reform_policy =
        baseline.with_parameter(parameter, NaiveDate::from_ymd_opt(2026, 4, 6).unwrap(), value)?;
    let recompile_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let reform = calculate(&reform_policy, dataset.clone(), &[output])?;
    let reform_ms = t.elapsed().as_secs_f64() * 1e3;

    let base_col = base.column(output)?;
    let reform_col = reform.column(output)?;
    let base_total: f64 = base_col.iter().sum();
    let delta: f64 = reform_col.iter().zip(base_col).map(|(r, b)| r - b).sum();
    let losers = reform_col.iter().zip(base_col).filter(|(r, b)| r > b).count();

    println!("axiom NICs demo: {label}, {n} people");
    println!("  baseline eval:    {baseline_ms:.1} ms");
    println!("  reform recompile: {recompile_ms:.1} ms");
    println!("  reform eval:      {reform_ms:.1} ms");
    println!("  unweighted synthetic baseline total: {base_total:.2}");
    println!("  unweighted synthetic delta: {delta:.2} across {losers} losers");
    Ok(())
}
