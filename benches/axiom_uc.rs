//! Axiom backend Universal Credit demo: a taper-rate reform over a
//! synthetic caseload.
//!
//! Loads the compiled Universal Credit artifact (WRA 2012 s.8 plus the UC
//! Regulations 2013 component, housing, childcare, and income-deduction
//! provisions, composed from rulespec-uk via the published
//! axiom-programs/uk/universal-credit spec), builds a synthetic caseload of
//! single and joint claims with varying earnings and children, then:
//!   1. evaluates the baseline award for one assessment period,
//!   2. builds a reform policy (earned income taper 55% -> 50%) by patching
//!      the parameter and recompiling in memory,
//!   3. evaluates the reform and reports the aggregate impact.
//!
//! The caseload models the award surface only: no housing costs element, no
//! childcare, no carer or LCWRA elements, no unearned income.
//!
//! Configure with env vars:
//!   * `AXIOM_HH` — family count (default 100_000)
//!
//! Run: `cargo bench --bench axiom_uc`

#[path = "../src/axiom/mod.rs"]
#[allow(dead_code)]
mod axiom;

use std::time::Instant;

use axiom::{calculate, Dataset, Policy};
use chrono::NaiveDate;

const ARTIFACT: &str = include_str!("../src/axiom/artifacts/uk-universal-credit-fy2026.json");
const AWARD: &str = "universal_credit_award_amount";
const TAPER: &str = "uk:regulations/uksi/2013/376/22#earned_income_taper_rate";
const CHILDREN: &str = "child_of_benefit_unit";
const ADULTS: &str = "adult_of_benefit_unit";
const SERVICE_CHARGES: &str = "owner_occupier_service_charge_payments";

fn main() -> anyhow::Result<()> {
    let n: usize = std::env::var("AXIOM_HH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);

    let baseline = Policy::from_artifact_json(ARTIFACT, "Family")?;

    // Caseload: alternate single/joint claims, all claimants 25 or over,
    // 0-2 children, monthly earnings spread from 0 upwards.
    let joint: Vec<bool> = (0..n).map(|i| i % 2 == 1).collect();
    let children: Vec<usize> = (0..n).map(|i| i % 3).collect();
    let earned: Vec<f64> = (0..n).map(|i| ((i / 2) % 8) as f64 * 200.0).collect();

    let t = Instant::now();
    let dataset = build_dataset(n, &joint, &children, &earned)?;
    let build_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let base = calculate(&baseline, dataset.clone(), &[AWARD])?;
    let baseline_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let reform_policy =
        baseline.with_parameter(TAPER, NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(), 0.50)?;
    let recompile_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let reform = calculate(&reform_policy, dataset, &[AWARD])?;
    let reform_ms = t.elapsed().as_secs_f64() * 1e3;

    let base_award = base.column(AWARD)?;
    let reform_award = reform.column(AWARD)?;
    let base_total: f64 = base_award.iter().sum();
    let cost: f64 = reform_award.iter().zip(base_award).map(|(r, b)| r - b).sum();
    let gainers = reform_award.iter().zip(base_award).filter(|(r, b)| r > b).count();
    let on_uc = base_award.iter().filter(|a| **a > 0.0).count();
    let total_ms = build_ms + baseline_ms + recompile_ms + reform_ms;

    println!("axiom UC demo: earned income taper 0.55 -> 0.50, {n} families");
    println!("  dataset build:    {build_ms:.1} ms");
    println!("  baseline eval:    {baseline_ms:.1} ms");
    println!("  reform recompile: {recompile_ms:.1} ms");
    println!("  reform eval:      {reform_ms:.1} ms");
    println!("  total:            {total_ms:.1} ms");
    println!("  unweighted synthetic baseline awards: {base_total:.2}/month across {on_uc} families on UC");
    println!("  unweighted synthetic delta: {cost:.2}/month across {gainers} gainers");
    println!(
        "AXIOM_UC_JSON={{\"families\":{n},\"total_ms\":{total_ms:.2},\"baseline_eval_ms\":{baseline_ms:.2},\"reform_eval_ms\":{reform_ms:.2},\"recompile_ms\":{recompile_ms:.2}}}"
    );
    Ok(())
}

fn build_dataset(
    n: usize,
    joint: &[bool],
    children: &[usize],
    earned: &[f64],
) -> anyhow::Result<Dataset> {
    let falses = vec![false; n];
    let zeros = vec![0.0; n];
    let single: Vec<bool> = joint.iter().map(|j| !j).collect();
    let joint_with_children: Vec<bool> =
        joint.iter().zip(children).map(|(j, c)| *j && *c > 0).collect();
    let single_with_children: Vec<bool> =
        joint.iter().zip(children).map(|(j, c)| !*j && *c > 0).collect();
    let joint_earned: Vec<f64> =
        joint.iter().zip(earned).map(|(j, e)| if *j { *e } else { 0.0 }).collect();
    let single_earned: Vec<f64> =
        joint.iter().zip(earned).map(|(j, e)| if *j { 0.0 } else { *e }).collect();

    // Children: the first child per family carries the first-child element,
    // the rest the second-and-subsequent one; no disability additions.
    let child_total: usize = children.iter().sum();
    let mut child_is_first = Vec::with_capacity(child_total);
    let mut child_is_later = Vec::with_capacity(child_total);
    for count in children {
        for j in 0..*count {
            child_is_first.push(j == 0);
            child_is_later.push(j > 0);
        }
    }
    let child_falses = vec![false; child_total];
    let child_trues = vec![true; child_total];

    // Adults: one per single claim, two per joint claim; nobody is a carer.
    let adult_counts: Vec<usize> = joint.iter().map(|j| 1 + *j as usize).collect();
    let adult_total: usize = adult_counts.iter().sum();
    let mut adult_joint = Vec::with_capacity(adult_total);
    for (j, count) in joint.iter().zip(&adult_counts) {
        adult_joint.extend(std::iter::repeat(*j).take(*count));
    }
    let adult_falses = vec![false; adult_total];
    let adult_zeros = vec![0.0; adult_total];

    Dataset::month(2026, 4)
        // Claim structure.
        .with_bool_input("claim_is_for_joint_claimants", joint)?
        .with_bool_input("award_is_for_joint_claimants", joint)?
        .with_bool_input("claimant_is_member_of_couple", joint)?
        .with_bool_input("claimant_makes_claim_as_single_person", &single)?
        .with_bool_input("either_joint_claimant_is_aged_25_or_over", joint)?
        .with_bool_input("single_claimant_is_aged_25_or_over", &single)?
        .with_bool_input("joint_claimants_responsible_for_child_or_qualifying_young_person", &joint_with_children)?
        .with_bool_input("single_claimant_responsible_for_child_or_qualifying_young_person", &single_with_children)?
        // Earnings; no unearned income.
        .with_input("joint_claimants_combined_earned_income_in_assessment_period", &joint_earned)?
        .with_input("claimant_earned_income_in_assessment_period", &single_earned)?
        .with_input("joint_claimants_combined_unearned_income_in_assessment_period", &zeros)?
        .with_input("claimant_unearned_income_in_assessment_period", &zeros)?
        // No limited capability for work, carers, or housing element.
        .with_bool_input("one_or_both_joint_claimants_have_limited_capability_for_work", &falses)?
        .with_bool_input("single_claimant_has_limited_capability_for_work", &falses)?
        .with_bool_input("first_joint_claimant_has_limited_capability_for_work_and_work_related_activity", &falses)?
        .with_bool_input("second_joint_claimant_has_limited_capability_for_work_and_work_related_activity", &falses)?
        .with_bool_input("single_claimant_has_limited_capability_for_work_and_work_related_activity", &falses)?
        .with_bool_input("both_joint_claimants_qualify_for_carer_element", &falses)?
        .with_bool_input("joint_claimants_are_caring_for_the_same_severely_disabled_person", &falses)?
        .with_bool_input("award_contains_housing_costs_element", &falses)?
        .with_input("lcwra_element_amount_given_in_regulation_36_for_first_joint_claimant", &zeros)?
        .with_input("lcwra_element_amount_given_in_regulation_36_for_second_joint_claimant", &zeros)?
        .with_input("lcwra_element_amount_given_in_regulation_36_for_single_claimant", &zeros)?
        // Housing inputs (unused with no housing element, but required).
        .with_input("amount_resulting_from_all_other_steps_in_parts_4_and_5_calculation", &zeros)?
        .with_input("housing_cost_contribution_count_required_under_paragraph_13_in_renters_case", &zeros)?
        .with_input("renters_core_rent", &zeros)?
        .with_input("renters_cap_rent", &zeros)?
        // No childcare.
        .with_input("charges_paid_for_relevant_childcare_attributable_to_assessment_period", &zeros)?
        .with_input("amount_considered_excessive_having_regard_to_paid_work_extent", &zeros)?
        .with_input("amount_met_or_reimbursed_by_employer_or_some_other_person", &zeros)?
        .with_input("amount_from_funds_provided_by_secretary_of_state_or_scottish_or_welsh_ministers_for_work_related_activity_or_training", &zeros)?
        .with_input("secretary_of_state_work_transition_childcare_payment_amount", &zeros)?
        .with_bool_input("secretary_of_state_work_transition_childcare_payment_meets_non_other_relevant_support_conditions", &falses)?
        .with_input("maximum_amount_specified_in_table_in_regulation_36", &zeros)?
        .with_input("childcare_costs_element_child_count", &zeros)?
        // Children of each benefit unit.
        .with_relation(CHILDREN, children)?
        .with_relation_bool_input(CHILDREN, "claimant_responsible_for_child_or_qualifying_young_person", &child_trues)?
        .with_relation_bool_input(CHILDREN, "child_is_first_child_or_qualifying_young_person", &child_is_first)?
        .with_relation_bool_input(CHILDREN, "child_is_second_or_subsequent_child_or_qualifying_young_person", &child_is_later)?
        .with_relation_bool_input(CHILDREN, "disabled_child_higher_rate_applies", &child_falses)?
        .with_relation_bool_input(CHILDREN, "disabled_child_lower_rate_applies", &child_falses)?
        // Adults of each benefit unit.
        .with_relation(ADULTS, &adult_counts)?
        .with_relation_input(ADULTS, "carer_element_amount", &adult_zeros)?
        .with_relation_bool_input(ADULTS, "claim_is_for_joint_claimants", &adult_joint)?
        .with_relation_bool_input(ADULTS, "claimant_has_regular_and_substantial_caring_responsibilities_for_severely_disabled_person", &adult_falses)?
        .with_relation_bool_input(ADULTS, "claimant_is_the_only_relevant_carer_or_is_elected_or_determined_for_carer_element", &adult_falses)?
        .with_relation_bool_input(ADULTS, "contributions_and_benefits_act_section_70_does_not_displace_carer_element", &adult_falses)?
        .with_relation_bool_input(ADULTS, "claimant_has_limited_capability_for_work_and_work_related_activity", &adult_falses)?
        .with_relation_bool_input(ADULTS, "lcwra_element_included_in_respect_of_other_joint_claimant", &adult_falses)?
        .with_relation_bool_input(ADULTS, "scottish_carer_benefit_coordination_applies_for_same_day_and_same_severely_disabled_person", &adult_falses)?
        .with_relation_bool_input(ADULTS, "claimant_and_other_person_jointly_elect_claimant_has_carer_element_and_other_person_has_no_scottish_carer_benefit_entitlement", &adult_falses)?
        .with_relation_bool_input(ADULTS, "secretary_of_state_after_consulting_scottish_ministers_is_satisfied_claimant_has_carer_element_and_other_person_has_no_scottish_carer_benefit_entitlement", &adult_falses)?
        // No owner-occupier service charge payments.
        .with_relation(SERVICE_CHARGES, &vec![0; n])?
        .with_relation_bool_input(SERVICE_CHARGES, "payment_is_relevant_service_charge_payment_taken_into_account_under_paragraph_8", &[])?
        .with_relation_bool_input(SERVICE_CHARGES, "service_charge_free_period_arrangements_apply_to_payment", &[])?
        .with_relation_bool_input(SERVICE_CHARGES, "service_charge_payment_period_is_month", &[])?
        .with_relation_bool_input(SERVICE_CHARGES, "service_charge_payment_period_is_week", &[])?
        .with_relation_bool_input(SERVICE_CHARGES, "service_charge_payment_period_is_two_weeks", &[])?
        .with_relation_bool_input(SERVICE_CHARGES, "service_charge_payment_period_is_four_weeks", &[])?
        .with_relation_bool_input(SERVICE_CHARGES, "service_charge_payment_period_is_three_months", &[])?
        .with_relation_bool_input(SERVICE_CHARGES, "service_charge_payment_period_is_annual", &[])?
        .with_relation_input(SERVICE_CHARGES, "service_charge_payment_amount", &[])?
        .with_relation_input(SERVICE_CHARGES, "service_charge_free_periods_in_12_month_period", &[])?
        .with_relation_input(SERVICE_CHARGES, "total_service_charge_payments_liable_in_12_month_period", &[])
}
