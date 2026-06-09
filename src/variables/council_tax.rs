//! Council Tax Reduction / Council Tax Benefit (CTR/CTB).
//!
//! Council tax *liability* is computed in [`crate::variables::wealth_taxes`]
//! (band × LA-multiplier, with the single-person discount). This module adds the
//! means-tested support that reduces that liability:
//!
//! - **Council Tax Benefit (CTB)** until March 2013.
//! - **Council Tax Reduction (CTR)** from April 2013, after CTB was abolished and
//!   localised (Local Government Finance Act 2012 s.10).
//!
//! Scheme detail:
//! - **Pension-age** claimants are covered by a single national scheme broadly
//!   mirroring the old CTB and able to reduce the bill to nil (Council Tax
//!   Reduction Schemes (Prescribed Requirements) (England) Regulations 2012,
//!   SI 2012/2885).
//! - **Working-age** claimants are subject to a locally-designed scheme. Because
//!   most billing authorities now require a minimum contribution, we model a
//!   representative England-average scheme with a maximum-support cap
//!   (`max_support_working_age`, default 0.90).
//!
//! The means test mirrors Housing Benefit (weekly applicable amount vs weekly
//! income, with the excess tapered) but uses the 20% CTR/CTB taper rather than
//! HB's 65% (SI 2012/2885 Sch.1 para.30).

use crate::engine::entities::{BenUnit, Person};
use crate::parameters::CouncilTaxReductionParams;

/// Calculate annual Council Tax Reduction / Benefit for a benefit unit.
///
/// `council_tax_liability` is the household's (post-discount) annual council tax.
/// The result is the annual reduction, capped at the maximum supportable share of
/// the liability (1.0 for pension-age claimants, `max_support_working_age` for
/// working-age claimants).
///
/// CTR = min(max_support × liability,
///           max(0, liability − (income − applicable_amount) × taper))
pub fn calculate_council_tax_reduction(
    bu: &BenUnit,
    people: &[Person],
    params: &CouncilTaxReductionParams,
    council_tax_liability: f64,
) -> f64 {
    if council_tax_liability <= 0.0 {
        return 0.0;
    }

    let is_couple = bu.is_couple(people);
    let eldest_age = bu.eldest_adult_age(people);
    let num_children = bu.num_children(people);

    // Pension-age claimants can have the bill reduced to nil; working-age
    // claimants are capped at the scheme's maximum support fraction.
    let is_pension_age = bu.person_ids.iter()
        .filter(|&&pid| people[pid].is_adult())
        .all(|&pid| people[pid].is_sp_age())
        && bu.num_adults(people) > 0;
    let max_support = if is_pension_age { 1.0 } else { params.max_support_working_age };
    let max_reduction = council_tax_liability * max_support;

    // Applicable amount (weekly → annual), mirroring Housing Benefit.
    let personal_allowance_weekly = if is_couple {
        params.personal_allowance_couple
    } else if eldest_age >= 25.0 {
        params.personal_allowance_single_25_plus
    } else {
        params.personal_allowance_single_under25
    };
    let family_premium_weekly = if num_children > 0 { params.family_premium } else { 0.0 };
    let child_allowance_weekly = params.child_allowance * num_children as f64;
    let applicable_amount = (personal_allowance_weekly + family_premium_weekly
        + child_allowance_weekly) * 52.0;

    // Income for CTR purposes (same measure as Housing Benefit).
    let income: f64 = bu.person_ids.iter()
        .map(|&pid| {
            let p = &people[pid];
            p.employment_income + p.self_employment_income
                + p.pension_income + p.state_pension
                + p.savings_interest_income + p.other_income
        })
        .sum();

    let excess_income = (income - applicable_amount).max(0.0);
    let reduction = (council_tax_liability - excess_income * params.taper_rate).max(0.0);

    reduction.min(max_reduction)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::entities::{BenUnit, Person};

    fn ctr_params() -> CouncilTaxReductionParams {
        CouncilTaxReductionParams {
            taper_rate: 0.20,
            max_support_working_age: 0.90,
            personal_allowance_single_under25: 71.70,
            personal_allowance_single_25_plus: 90.50,
            personal_allowance_couple: 142.25,
            child_allowance: 83.73,
            family_premium: 18.53,
        }
    }

    /// One working-age adult, person id 0, given income.
    fn single_adult(income: f64, age: f64) -> (Vec<Person>, BenUnit) {
        let mut p = Person::default();
        p.id = 0; p.benunit_id = 0; p.household_id = 0;
        p.age = age;
        p.employment_income = income;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0], ..BenUnit::default() };
        (vec![p], bu)
    }

    #[test]
    fn no_liability_no_reduction() {
        let params = ctr_params();
        let (people, bu) = single_adult(0.0, 40.0);
        assert_eq!(calculate_council_tax_reduction(&bu, &people, &params, 0.0), 0.0);
    }

    #[test]
    fn low_income_working_age_gets_capped_full_support() {
        // Income below the applicable amount → no taper reduction, so CTR = full
        // support, capped at 90% of the bill for a working-age claimant.
        let params = ctr_params();
        let (people, bu) = single_adult(2000.0, 40.0); // well below applicable amount
        let liability = 1800.0;
        let ctr = calculate_council_tax_reduction(&bu, &people, &params, liability);
        assert!((ctr - liability * 0.90).abs() < 0.01, "got {}", ctr);
    }

    #[test]
    fn pension_age_gets_full_100pct_support() {
        // Pension-age claimant on a low income can have the whole bill covered.
        let params = ctr_params();
        let (people, bu) = single_adult(2000.0, 70.0);
        let liability = 1800.0;
        let ctr = calculate_council_tax_reduction(&bu, &people, &params, liability);
        assert!((ctr - liability).abs() < 0.01, "got {}", ctr);
    }

    #[test]
    fn high_income_no_reduction() {
        // Income far above the applicable amount tapers the reduction to zero.
        let params = ctr_params();
        let (people, bu) = single_adult(60_000.0, 40.0);
        let liability = 1800.0;
        let ctr = calculate_council_tax_reduction(&bu, &people, &params, liability);
        assert_eq!(ctr, 0.0);
    }

    #[test]
    fn partial_taper() {
        // Applicable amount (single 25+): 90.50 × 52 = £4,706.
        // Income £10,000 → excess £5,294; reduction = liability − 0.20 × 5,294
        //   = 1,800 − 1,058.80 = £741.20. Below the 90% cap (£1,620), so uncapped.
        let params = ctr_params();
        let (people, bu) = single_adult(10_000.0, 40.0);
        let liability = 1800.0;
        let aa = 90.50 * 52.0;
        let expected = liability - 0.20 * (10_000.0 - aa);
        let ctr = calculate_council_tax_reduction(&bu, &people, &params, liability);
        assert!((ctr - expected).abs() < 0.01, "got {} expected {}", ctr, expected);
    }
}
