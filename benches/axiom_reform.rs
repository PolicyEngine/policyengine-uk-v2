//! Axiom backend reform demo: how long does a rule-change impact take?
//!
//! Loads the compiled income tax artifact (the ITA 2007 s.23 liability
//! calculation over s.10 + s.35 with FA 2026 s.2 rates, composed from
//! rulespec-uk), builds a synthetic population with income components
//! supplied through the s.23 Step 1 Payment -> Person relation, then:
//!   1. evaluates baseline income tax liability for every person,
//!   2. builds a reform policy (basic rate +1p) by patching the parameter and
//!      recompiling in memory,
//!   3. evaluates the reform and reports the per-person and aggregate impact.
//!
//! Configure with env vars:
//!   * `AXIOM_HH` — person count (default 100_000)
//!
//! Run: `cargo bench --bench axiom_reform`

// The crate is binary-only (no lib target), so pull the module in by path,
// exactly as `benches/speedtest.rs` does. The demo only exercises part of
// the module's API, so silence dead-code warnings at the include site.
#[path = "../src/axiom/mod.rs"]
#[allow(dead_code)]
mod axiom;

use std::time::Instant;

use axiom::{calculate, Dataset, Policy};
use chrono::NaiveDate;

const ARTIFACT: &str = include_str!("../src/axiom/artifacts/uk-income-tax-fy2026.json");
const INCOME_TAX: &str = "income_tax_liability";
const INCOME_COMPONENTS: &str = "income_component_of_taxpayer";
const BASIC_RATE: &str = "uk:statutes/ukpga/2026/11/2#basic_rate";

fn main() -> anyhow::Result<()> {
    let n: usize = std::env::var("AXIOM_HH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);

    let baseline = Policy::from_artifact_json(ARTIFACT, "Person")?;

    // Incomes spread across the basic / higher / additional rate bands,
    // supplied as one income component per person through the s.23 Step 1
    // relation (a real population would have several per person).
    let incomes: Vec<f64> = (0..n).map(|i| 10_000.0 + (i % 40) as f64 * 5_000.0).collect();
    let reliefs = vec![0.0; n];
    let counts = vec![1usize; n];

    let t = Instant::now();
    let dataset = Dataset::tax_year(2026)
        .with_relation(INCOME_COMPONENTS, &counts)?
        .with_relation_input(INCOME_COMPONENTS, "amount_charged_to_income_tax", &incomes)?
        .with_relation_input(INCOME_COMPONENTS, "relief_deducted_under_section_24", &reliefs)?;
    let build_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let base = calculate(&baseline, dataset.clone(), &[INCOME_TAX])?;
    let baseline_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let reform_policy =
        baseline.with_parameter(BASIC_RATE, NaiveDate::from_ymd_opt(2026, 4, 6).unwrap(), 0.21)?;
    let recompile_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let reform = calculate(&reform_policy, dataset, &[INCOME_TAX])?;
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
    println!("  unweighted synthetic delta: {revenue_delta:.0} across {losers} losers");
    println!(
        "AXIOM_REFORM_JSON={{\"people\":{n},\"total_ms\":{total_ms:.2},\"baseline_eval_ms\":{baseline_ms:.2},\"reform_eval_ms\":{reform_ms:.2},\"recompile_ms\":{recompile_ms:.2}}}"
    );
    Ok(())
}
