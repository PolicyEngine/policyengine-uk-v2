//! Axiom backend child benefit demo: SSCBA 1992 s.141 weekly entitlement at
//! the SI 2006/965 reg 2 rates, summed over the children for whom each
//! family is responsible (enhanced rate for the eldest child, other rate
//! for the rest), with an enhanced-rate reform.
//!
//! Loads the compiled artifact composed from rulespec-uk, evaluates the
//! baseline over a 100k-family caseload, patches the enhanced weekly rate,
//! recompiles in memory, and evaluates the reform.
//!
//! Configure with env vars:
//!   * `AXIOM_HH` — family count (default 100_000)
//!
//! Run: `cargo bench --bench axiom_child_benefit`

#[path = "../src/axiom/mod.rs"]
#[allow(dead_code)]
mod axiom;

use std::time::Instant;

use axiom::{calculate, Dataset, Policy};
use chrono::NaiveDate;

const ARTIFACT: &str = include_str!("../src/axiom/artifacts/uk-child-benefit-fy2026.json");
const OUTPUT: &str = "child_benefit_weekly_entitlement";
const CHILDREN: &str = "child_benefit_children_or_qualifying_young_persons_for_whom_person_responsible";
const ENHANCED_RATE: &str = "uk:regulations/uksi/2006/965/2#child_benefit_enhanced_weekly_rate";

fn main() -> anyhow::Result<()> {
    let n: usize = std::env::var("AXIOM_HH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);

    // Families with 0..=3 children; the first child in each family is the
    // only or eldest child for the payee.
    let counts: Vec<usize> = (0..n).map(|i| i % 4).collect();
    let total_children: usize = counts.iter().sum();
    let mut eldest = Vec::with_capacity(total_children);
    for &c in &counts {
        for j in 0..c {
            eldest.push(j == 0);
        }
    }
    let qualifies = vec![true; total_children];
    let not_paragraph_2 = vec![false; total_children];

    let dataset = Dataset::week(NaiveDate::from_ymd_opt(2026, 4, 6).unwrap())
        .with_relation(CHILDREN, &counts)?
        .with_relation_bool_input(
            CHILDREN,
            "child_or_qualifying_young_person_is_only_elder_or_eldest_for_payee",
            &eldest,
        )?
        .with_relation_bool_input(
            CHILDREN,
            "child_or_qualifying_young_person_is_elder_or_eldest_among_paragraph_2_children",
            &not_paragraph_2,
        )?
        .with_relation_bool_input(
            CHILDREN,
            "is_child_or_qualifying_young_person_for_child_benefit",
            &qualifies,
        )?;

    let baseline = Policy::from_artifact_json(ARTIFACT, "Family")?;

    let t = Instant::now();
    let base = calculate(&baseline, &dataset, &[OUTPUT])?;
    let baseline_ms = t.elapsed().as_secs_f64() * 1e3;

    // Reform: enhanced (eldest child) weekly rate 27.05 -> 30.00.
    let t = Instant::now();
    let reform_policy = baseline.with_parameter(
        ENHANCED_RATE,
        NaiveDate::from_ymd_opt(2026, 4, 6).unwrap(),
        30.00,
    )?;
    let recompile_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let reform = calculate(&reform_policy, &dataset, &[OUTPUT])?;
    let reform_ms = t.elapsed().as_secs_f64() * 1e3;

    let base_col = base.column(OUTPUT)?;
    let reform_col = reform.column(OUTPUT)?;
    let base_total: f64 = base_col.iter().sum();
    let delta: f64 = reform_col.iter().zip(base_col).map(|(r, b)| r - b).sum();
    let gainers = reform_col.iter().zip(base_col).filter(|(r, b)| r > b).count();

    println!("axiom child benefit demo: enhanced rate 27.05 -> 30.00, {n} families");
    println!("  baseline eval:    {baseline_ms:.1} ms");
    println!("  reform recompile: {recompile_ms:.1} ms");
    println!("  reform eval:      {reform_ms:.1} ms");
    println!("  baseline total: {base_total:.2}/week");
    println!("  cost: {delta:.2}/week across {gainers} gainers");
    Ok(())
}
