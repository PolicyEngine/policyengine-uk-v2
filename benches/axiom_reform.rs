//! Axiom backend reform demo: how long does a rule-change impact take?
//!
//! Loads the compiled income tax artifact (ITA 2007 s.10 + s.35, FA 2026 s.2
//! rates, composed from rulespec-uk), builds a synthetic population, then:
//!   1. evaluates baseline income tax for every person,
//!   2. builds a reform policy (basic rate +1p) by patching the parameter and
//!      recompiling in memory,
//!   3. evaluates the reform and reports the per-person and aggregate impact.
//!
//! Configure with env vars:
//!   * `AXIOM_HH` — person count (default 100_000)
//!
//! Run: `cargo bench --bench axiom_reform`

// The crate is binary-only (no lib target), so pull the module in by path,
// exactly as `benches/speedtest.rs` does.
#[path = "../src/axiom/mod.rs"]
mod axiom;

use std::time::Instant;

use axiom::{calculate, Dataset, Policy};
use chrono::NaiveDate;

const ARTIFACT: &str = include_str!("../src/axiom/artifacts/uk-income-tax-fy2026.json");
const INCOME_TAX: &str = "uk:statutes/ukpga/2007/3/10#income_tax_on_section_10_income";
const BASIC_RATE: &str = "uk:statutes/ukpga/2026/11/2#basic_rate";

fn main() -> anyhow::Result<()> {
    let n: usize = std::env::var("AXIOM_HH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);

    let baseline = Policy::from_artifact_json(ARTIFACT)?;

    // Incomes spread across the basic / higher / additional rate bands.
    let incomes: Vec<f64> = (0..n).map(|i| 10_000.0 + (i % 40) as f64 * 5_000.0).collect();

    let t = Instant::now();
    let dataset = Dataset::tax_year(2026)
        .with_input(
            "uk:statutes/ukpga/2007/3/10#input.income_charged_under_section_10",
            &incomes,
        )?
        .with_input("uk:statutes/ukpga/2007/3/35#input.adjusted_net_income", &incomes)?;
    let build_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let base = calculate(&baseline, &dataset, &[INCOME_TAX])?;
    let baseline_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let reform_policy =
        baseline.with_parameter(BASIC_RATE, NaiveDate::from_ymd_opt(2026, 4, 6).unwrap(), 0.21)?;
    let recompile_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let reform = calculate(&reform_policy, &dataset, &[INCOME_TAX])?;
    let reform_ms = t.elapsed().as_secs_f64() * 1e3;

    let base_tax = base.column(INCOME_TAX)?;
    let reform_tax = reform.column(INCOME_TAX)?;
    let revenue_delta: f64 = reform_tax.iter().zip(base_tax).map(|(r, b)| r - b).sum();
    let losers = reform_tax.iter().zip(base_tax).filter(|(r, b)| r > b).count();
    let total_ms = build_ms + baseline_ms + recompile_ms + reform_ms;

    println!("axiom reform demo: basic rate 0.20 -> 0.21, {n} people");
    println!("  dataset build:    {build_ms:.1} ms");
    println!("  baseline eval:    {baseline_ms:.1} ms");
    println!("  reform recompile: {recompile_ms:.1} ms");
    println!("  reform eval:      {reform_ms:.1} ms");
    println!("  total:            {total_ms:.1} ms");
    println!("  revenue delta: {revenue_delta:.0} across {losers} losers");
    println!(
        "AXIOM_REFORM_JSON={{\"people\":{n},\"total_ms\":{total_ms:.2},\"baseline_eval_ms\":{baseline_ms:.2},\"reform_eval_ms\":{reform_ms:.2},\"recompile_ms\":{recompile_ms:.2}}}"
    );
    Ok(())
}
