use crate::engine::entities::{Person, BenUnit};
use crate::engine::simulation::PersonResult;
use crate::parameters::{Parameters, TaxBracket};

/// Calculate all person-level tax results: income tax (earned + savings + dividend) + NI.
///
/// `state_pension` is the calculated (reform-adjusted) annual state pension for this person,
/// computed upstream by `benefits::person_state_pension`. This replaces the raw reported
/// amount so that SP reforms flow through to income tax correctly.
pub fn calculate(person: &Person, params: &Parameters, state_pension: f64) -> PersonResult {
    // Total income using calculated SP instead of reported
    let total_income = person.employment_income + person.self_employment_income
        + person.pension_income + state_pension
        + person.savings_interest_income + person.dividend_income
        + person.property_income + person.maintenance_income
        + person.miscellaneous_income + person.other_income;

    // Step 1: Adjusted net income (for PA taper)
    let pension_relief = person.employee_pension_contributions + person.personal_pension_contributions;
    let adjusted_net_income = (total_income - pension_relief).max(0.0);

    // Step 2: Personal allowance (tapered for high earners)
    let personal_allowance = calculate_personal_allowance(adjusted_net_income, params);

    // Step 3: Allocate PA across income types (earned first, then savings, then dividends)
    let earned_income = person.employment_income + person.self_employment_income
        + person.pension_income + state_pension
        + person.property_income + person.maintenance_income
        + person.miscellaneous_income + person.other_income;

    let pa_against_earned = personal_allowance.min(earned_income);
    let pa_remaining = personal_allowance - pa_against_earned;
    let pa_against_savings = pa_remaining.min(person.savings_interest_income);
    let pa_remaining2 = pa_remaining - pa_against_savings;
    let pa_against_dividends = pa_remaining2.min(person.dividend_income);

    let earned_taxable = (earned_income - pa_against_earned).max(0.0);
    let savings_taxable = (person.savings_interest_income - pa_against_savings).max(0.0);
    let dividend_taxable = (person.dividend_income - pa_against_dividends).max(0.0);

    let taxable_income = earned_taxable + savings_taxable + dividend_taxable;

    // Unused personal allowance (for marriage allowance)
    let used_pa = pa_against_earned + pa_against_savings + pa_against_dividends;
    let unused_pa = (personal_allowance - used_pa).max(0.0);

    // Step 4: Earned income tax (UK or Scottish rates)
    let brackets = if person.is_in_scotland {
        &params.income_tax.scottish_brackets
    } else {
        &params.income_tax.uk_brackets
    };
    let earned_income_tax = apply_brackets(earned_taxable, brackets);

    // Step 5: Savings income tax (stacked on top of earned)
    let savings_income_tax = calculate_savings_tax(earned_taxable, savings_taxable, params);

    // Step 6: Dividend income tax (stacked on top of earned + savings)
    let dividend_income_tax = calculate_dividend_tax(
        earned_taxable + savings_taxable,
        dividend_taxable,
        params,
    );

    let income_tax = earned_income_tax + savings_income_tax + dividend_income_tax;

    // Step 7: National Insurance
    let ni_class1 = calculate_ni_class1(person, params);
    let ni_class2 = calculate_ni_class2(person, params);
    let ni_class4 = calculate_ni_class4(person, params);
    let ni_employer = calculate_ni_employer(person, params);
    let national_insurance = ni_class1 + ni_class2 + ni_class4;

    PersonResult {
        income_tax,
        national_insurance,
        ni_class1_employee: ni_class1,
        ni_class2,
        ni_class4,
        employer_ni: ni_employer,
        total_income,
        taxable_income,
        personal_allowance,
        adjusted_net_income,
        unused_personal_allowance: unused_pa,
        marriage_allowance_deduction: 0.0, // Set later by apply_marriage_allowance
        hicbc: 0.0, // Set later by simulation Phase 2b
        capital_gains_tax: 0.0, // Set later by simulation Phase 2c
    }
}

/// Apply marriage allowance for a benefit unit.
///
/// ITA 2007 ss.55B-55C: One spouse (the transferor) can transfer up to 10%
/// of their personal allowance to the other spouse (the recipient), provided:
///
/// 1. The couple is married/civil partners
/// 2. The recipient is a basic rate taxpayer (or starter/intermediate in Scotland)
/// 3. The transferor has unused personal allowance
///
/// The recipient gets a tax reduction (not a PA increase) equal to
/// `transferred_amount * basic_rate`, capped at 20%.
///
/// The transferor's PA is reduced by the transferred amount (but since they
/// had unused PA, this doesn't change their tax).
///
/// We identify the transferor as the adult with higher unused PA, and the
/// recipient as the other adult (if they're a basic rate taxpayer).
pub fn apply_marriage_allowance(
    bu: &BenUnit,
    people: &[Person],
    person_results: &mut [PersonResult],
    params: &Parameters,
) {
    // Must be a couple
    if bu.num_adults(people) < 2 {
        return;
    }

    // Collect adult person IDs
    let adult_ids: Vec<usize> = bu.person_ids.iter()
        .copied()
        .filter(|&pid| people[pid].is_adult())
        .collect();

    if adult_ids.len() != 2 {
        return; // Only handle exactly 2 adults
    }

    let pid_a = adult_ids[0];
    let pid_b = adult_ids[1];

    let it = &params.income_tax;
    let max_transfer = it.personal_allowance * it.marriage_allowance_max_fraction;

    // Determine basic rate limit for eligibility check
    let basic_limit = it.uk_brackets.get(1).map_or(37700.0, |b| b.threshold);

    // Check each configuration: A transfers to B, B transfers to A
    // Pick the one that saves the most tax
    let (transferor, recipient) = {
        let a_unused = person_results[pid_a].unused_personal_allowance;
        let b_unused = person_results[pid_b].unused_personal_allowance;

        let a_taxable = person_results[pid_a].taxable_income;
        let b_taxable = person_results[pid_b].taxable_income;

        // A is basic rate taxpayer if taxable income is within basic rate band
        let a_is_basic = a_taxable > 0.0 && a_taxable <= basic_limit;
        let b_is_basic = b_taxable > 0.0 && b_taxable <= basic_limit;

        // Scottish basic/starter/intermediate: all below higher rate threshold
        let scottish_higher_start = it.scottish_brackets.get(3)
            .map_or(31092.0, |b| b.threshold);
        let a_is_basic_scot = a_taxable > 0.0 && a_taxable <= scottish_higher_start;
        let b_is_basic_scot = b_taxable > 0.0 && b_taxable <= scottish_higher_start;

        let a_eligible_recipient = if people[pid_a].is_in_scotland { a_is_basic_scot } else { a_is_basic };
        let b_eligible_recipient = if people[pid_b].is_in_scotland { b_is_basic_scot } else { b_is_basic };

        // Try B→A (B transfers unused PA to A)
        let b_to_a = if b_unused > 0.0 && a_eligible_recipient {
            let transfer = b_unused.min(max_transfer);
            round_up(transfer, it.marriage_allowance_rounding)
        } else { 0.0 };

        // Try A→B (A transfers unused PA to B)
        let a_to_b = if a_unused > 0.0 && b_eligible_recipient {
            let transfer = a_unused.min(max_transfer);
            round_up(transfer, it.marriage_allowance_rounding)
        } else { 0.0 };

        // Tax saving = transferred_amount * 0.20 (basic rate)
        let saving_b_to_a = b_to_a * 0.20;
        let saving_a_to_b = a_to_b * 0.20;

        if saving_b_to_a >= saving_a_to_b && saving_b_to_a > 0.0 {
            (pid_b, pid_a)
        } else if saving_a_to_b > 0.0 {
            (pid_a, pid_b)
        } else {
            return; // No benefit from marriage allowance
        }
    };

    let unused = person_results[transferor].unused_personal_allowance;
    let transfer = round_up(unused.min(max_transfer), it.marriage_allowance_rounding);
    let tax_reduction = transfer * 0.20; // Always at basic rate, even in Scotland

    // Apply: reduce recipient's income tax, record the deduction
    person_results[recipient].income_tax = (person_results[recipient].income_tax - tax_reduction).max(0.0);
    person_results[recipient].marriage_allowance_deduction = tax_reduction;

    // The transferor loses PA but since it was unused, no tax impact.
    // We reduce their unused_pa for accuracy.
    person_results[transferor].unused_personal_allowance -= transfer;
}

/// Round up to nearest increment (e.g. £10)
fn round_up(value: f64, increment: f64) -> f64 {
    if increment <= 0.0 { return value; }
    (value / increment).ceil() * increment
}

/// Personal allowance with taper: reduced by £1 for every £2 above threshold
fn calculate_personal_allowance(adjusted_net_income: f64, params: &Parameters) -> f64 {
    let pa = params.income_tax.personal_allowance;
    let excess = (adjusted_net_income - params.income_tax.pa_taper_threshold).max(0.0);
    let reduction = excess * params.income_tax.pa_taper_rate;
    (pa - reduction).max(0.0)
}

/// Apply graduated tax brackets to taxable income
fn apply_brackets(taxable_income: f64, brackets: &[TaxBracket]) -> f64 {
    let mut tax = 0.0;
    for i in 0..brackets.len() {
        let lower = brackets[i].threshold;
        let upper = if i + 1 < brackets.len() {
            brackets[i + 1].threshold
        } else {
            f64::INFINITY
        };
        let band_income = (taxable_income - lower).min(upper - lower).max(0.0);
        tax += band_income * brackets[i].rate;
    }
    tax
}

/// Savings income tax using stacking (savings sit on top of earned income).
///
/// ITA 2007 s.12: Starting rate for savings (0%) on first £5,000, reduced £1 for £1 by
/// non-savings taxable income.
///
/// ITA 2007 s.57: Personal Savings Allowance (PSA) — £1000 basic, £500 higher, £0 additional.
fn calculate_savings_tax(earned_taxable: f64, savings_taxable: f64, params: &Parameters) -> f64 {
    if savings_taxable <= 0.0 {
        return 0.0;
    }

    let basic_limit = params.income_tax.uk_brackets.get(1)
        .map_or(37700.0, |b| b.threshold);
    let higher_limit = params.income_tax.uk_brackets.get(2)
        .map_or(125140.0, |b| b.threshold);

    // Savings starter rate band: 0% on first £5,000, reduced by earned taxable income
    let starter_band = (params.income_tax.savings_starter_rate_band - earned_taxable).max(0.0);
    let in_starter = savings_taxable.min(starter_band);
    let savings_after_starter = savings_taxable - in_starter;

    // PSA depends on marginal rate band
    let psa = if earned_taxable >= higher_limit {
        0.0  // Additional rate taxpayer: no PSA
    } else if earned_taxable >= basic_limit {
        500.0  // Higher rate taxpayer: £500 PSA
    } else {
        1000.0  // Basic rate taxpayer: £1000 PSA
    };

    let taxable_after_psa = (savings_after_starter - psa).max(0.0);
    if taxable_after_psa <= 0.0 {
        return 0.0;
    }

    // Stack remaining savings on top of earned + starter-band savings to get rates
    let base = earned_taxable + in_starter; // starter band taxed at 0%, so just shifts the stack
    let tax_with = apply_brackets(base + taxable_after_psa, &params.income_tax.uk_brackets);
    let tax_without = apply_brackets(base, &params.income_tax.uk_brackets);
    (tax_with - tax_without).max(0.0)
}

/// Dividend income tax using stacking.
fn calculate_dividend_tax(other_taxable: f64, dividend_taxable: f64, params: &Parameters) -> f64 {
    if dividend_taxable <= 0.0 {
        return 0.0;
    }

    let after_allowance = (dividend_taxable - params.income_tax.dividend_allowance).max(0.0);
    if after_allowance <= 0.0 {
        return 0.0;
    }

    let basic_limit = params.income_tax.uk_brackets.get(1)
        .map_or(37700.0, |b| b.threshold);
    let higher_limit = params.income_tax.uk_brackets.get(2)
        .map_or(125140.0, |b| b.threshold);

    let basic_remaining = (basic_limit - other_taxable).max(0.0);
    let higher_remaining = (higher_limit - other_taxable).max(0.0) - basic_remaining;

    let in_basic = after_allowance.min(basic_remaining);
    let in_higher = (after_allowance - in_basic).min(higher_remaining.max(0.0));
    let in_additional = (after_allowance - in_basic - in_higher).max(0.0);

    in_basic * params.income_tax.dividend_basic_rate
        + in_higher * params.income_tax.dividend_higher_rate
        + in_additional * params.income_tax.dividend_additional_rate
}

/// National Insurance: Class 1 employee contributions (on employment income)
fn calculate_ni_class1(person: &Person, params: &Parameters) -> f64 {
    let earnings = person.employment_income;
    let ni = &params.national_insurance;

    let main_band = (earnings.min(ni.upper_earnings_limit_annual) - ni.primary_threshold_annual).max(0.0);
    let additional_band = (earnings - ni.upper_earnings_limit_annual).max(0.0);

    main_band * ni.main_rate + additional_band * ni.additional_rate
}

/// National Insurance: Class 2 self-employed flat-rate contributions.
/// SSCBA 1992 s.11: flat weekly rate if self-employment profits exceed small profits threshold.
fn calculate_ni_class2(person: &Person, params: &Parameters) -> f64 {
    let profits = person.self_employment_income;
    if profits < params.national_insurance.class2_small_profits_threshold {
        return 0.0;
    }
    params.national_insurance.class2_flat_rate_weekly * (365.25 / 7.0)
}

/// National Insurance: Class 1 employer contributions
fn calculate_ni_employer(person: &Person, params: &Parameters) -> f64 {
    let earnings = person.employment_income;
    let ni = &params.national_insurance;

    let above_secondary = (earnings - ni.secondary_threshold_annual).max(0.0);
    above_secondary * ni.employer_rate
}

/// National Insurance: Class 4 contributions (on self-employment profits)
fn calculate_ni_class4(person: &Person, params: &Parameters) -> f64 {
    let profits = person.self_employment_income;
    if profits <= 0.0 {
        return 0.0;
    }
    let ni = &params.national_insurance;

    let main_band = (profits.min(ni.class4_upper_profits_limit) - ni.class4_lower_profits_limit).max(0.0);
    let additional_band = (profits - ni.class4_upper_profits_limit).max(0.0);

    main_band * ni.class4_main_rate + additional_band * ni.class4_additional_rate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::entities::{Person, BenUnit, Gender};
    use crate::parameters::Parameters;

    fn test_person(employment_income: f64) -> Person {
        let mut p = Person::default();
        p.age = 35.0;
        p.employment_income = employment_income;
        p.hours_worked = 37.5 * 52.0;
        p
    }

    fn test_person_se(self_employment_income: f64) -> Person {
        let mut p = Person::default();
        p.age = 35.0;
        p.self_employment_income = self_employment_income;
        p.hours_worked = 37.5 * 52.0;
        p
    }

    #[test]
    fn test_basic_rate_taxpayer() {
        let params = Parameters::for_year(2025).unwrap();
        let result = calculate(&test_person(30000.0), &params, 0.0);
        assert!((result.income_tax - 3486.0).abs() < 1.0);
        assert!((result.personal_allowance - 12570.0).abs() < 0.01);
    }

    #[test]
    fn test_higher_rate_taxpayer() {
        let params = Parameters::for_year(2025).unwrap();
        let result = calculate(&test_person(60000.0), &params, 0.0);
        assert!((result.income_tax - 11432.0).abs() < 1.0);
    }

    #[test]
    fn test_pa_taper() {
        let params = Parameters::for_year(2025).unwrap();
        let result = calculate(&test_person(125140.0), &params, 0.0);
        assert!(result.personal_allowance < 1.0);
    }

    #[test]
    fn test_ni_class1() {
        let params = Parameters::for_year(2025).unwrap();
        let result = calculate(&test_person(30000.0), &params, 0.0);
        assert!((result.national_insurance - 1394.40).abs() < 1.0);
    }

    #[test]
    fn test_ni_class4() {
        let params = Parameters::for_year(2025).unwrap();
        let result = calculate(&test_person_se(40000.0), &params, 0.0);
        // Class 4: (40000 - 12570) × 0.06 = £1,645.80
        // Class 2: £3.45 × 52.18 = ~£179.96
        let expected = 1645.80 + params.national_insurance.class2_flat_rate_weekly * (365.25 / 7.0);
        assert!((result.national_insurance - expected).abs() < 1.0,
            "Expected ~{:.2}, got {:.2}", expected, result.national_insurance);
    }

    #[test]
    fn test_dividend_income_tax() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 35.0;
        p.employment_income = 30000.0;
        p.dividend_income = 5000.0;
        let result = calculate(&p, &params, p.state_pension);
        // Earned taxable: 17430 at 20% = 3486
        // Dividend: 5000 - 500 allowance = 4500 at 8.75% = 393.75
        assert!((result.income_tax - 3879.75).abs() < 2.0,
            "Expected ~3879.75, got {}", result.income_tax);
    }

    #[test]
    fn test_savings_income_tax() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 35.0;
        p.employment_income = 30000.0;
        p.savings_interest_income = 3000.0;
        let result = calculate(&p, &params, p.state_pension);
        // Savings: 3000 - 1000 PSA = 2000 at 20% = 400
        assert!((result.income_tax - 3886.0).abs() < 2.0,
            "Expected ~3886, got {}", result.income_tax);
    }

    #[test]
    fn test_employer_ni() {
        let params = Parameters::for_year(2025).unwrap();
        let result = calculate(&test_person(50000.0), &params, 0.0);
        assert!(result.employer_ni > 0.0);
    }

    #[test]
    fn test_ni_class_breakdown_sums_to_total() {
        // A person with both employment and self-employment income. Each NI
        // class must hold the *correct* slice — pinning exact values guards
        // against the class-1/class-4 fields being swapped or mislabelled,
        // which a sum-only check could not catch.
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 35.0;
        p.employment_income = 30_000.0;
        p.self_employment_income = 20_000.0;
        p.hours_worked = 37.5 * 52.0;

        let result = calculate(&p, &params, 0.0);

        // Class 1 employee: (30,000 - 12,570) × 8% = £1,394.40
        assert!((result.ni_class1_employee - 1_394.40).abs() < 0.01,
            "Class 1 employee: expected 1394.40, got {:.2}", result.ni_class1_employee);
        // Class 2: abolished from 2024/25 — flat rate is 0 in 2025/26.
        assert!(result.ni_class2.abs() < 0.01,
            "Class 2: expected 0.00, got {:.2}", result.ni_class2);
        // Class 4: (20,000 - 12,570) × 6% = £445.80
        assert!((result.ni_class4 - 445.80).abs() < 0.01,
            "Class 4: expected 445.80, got {:.2}", result.ni_class4);

        // The three classes must sum to the back-compat `national_insurance` total.
        let sum = result.ni_class1_employee + result.ni_class2 + result.ni_class4;
        assert!(
            (sum - result.national_insurance).abs() < 0.01,
            "Class breakdown {sum:.2} should equal national_insurance {:.2}",
            result.national_insurance,
        );
        assert!((result.national_insurance - 1_840.20).abs() < 0.01,
            "national_insurance: expected 1840.20, got {:.2}", result.national_insurance);

        // Employer NI is reported separately from the employee total.
        assert!(result.employer_ni > 0.0);
    }

    #[test]
    fn test_ni_breakdown_employment_only() {
        // A pure employee has Class 1 only; the self-employed classes stay
        // zero and the whole NI total lands in `ni_class1_employee`.
        let params = Parameters::for_year(2025).unwrap();
        let result = calculate(&test_person(30_000.0), &params, 0.0);

        assert!(result.ni_class1_employee > 0.0, "Class 1 employee should be > 0");
        assert!(result.ni_class2.abs() < 0.01, "Class 2 should be 0 for an employee");
        assert!(result.ni_class4.abs() < 0.01, "Class 4 should be 0 for an employee");
        assert!(
            (result.ni_class1_employee - result.national_insurance).abs() < 0.01,
            "All NI should be Class 1 for a pure employee",
        );
    }

    #[test]
    fn test_ni_breakdown_self_employment_only() {
        // A pure self-employed person has Class 2/4 only; Class 1 employee
        // stays zero. Guards against employment NI leaking into the SE classes.
        let params = Parameters::for_year(2025).unwrap();
        let result = calculate(&test_person_se(40_000.0), &params, 0.0);

        assert!(result.ni_class1_employee.abs() < 0.01,
            "Class 1 employee should be 0 for a pure self-employed person");
        // Class 4: (40,000 - 12,570) × 6% = £1,645.80
        assert!((result.ni_class4 - 1_645.80).abs() < 0.01,
            "Class 4: expected 1645.80, got {:.2}", result.ni_class4);
        assert!(
            (result.ni_class2 + result.ni_class4 - result.national_insurance).abs() < 0.01,
            "All NI should be Class 2 + Class 4 for a pure self-employed person",
        );
    }

    #[test]
    fn test_unused_personal_allowance() {
        let params = Parameters::for_year(2025).unwrap();
        // Person earning £5,000 — well below PA of £12,570
        let result = calculate(&test_person(5000.0), &params, 0.0);
        assert!((result.unused_personal_allowance - 7570.0).abs() < 1.0,
            "Expected ~7570 unused PA, got {}", result.unused_personal_allowance);
        assert!(result.income_tax < 0.01, "Should pay no tax");
    }

    #[test]
    fn test_marriage_allowance_basic_case() {
        let params = Parameters::for_year(2025).unwrap();

        // Couple: Person A earns £5,000 (unused PA = £7,570)
        //         Person B earns £30,000 (basic rate taxpayer)
        let mut person_a = Person::default();
        person_a.id = 0;
        person_a.age = 35.0;
        person_a.gender = Gender::Female;
        person_a.employment_income = 5000.0;

        let mut person_b = Person::default();
        person_b.id = 1;
        person_b.age = 35.0;
        person_b.gender = Gender::Male;
        person_b.employment_income = 30000.0;

        let people = vec![person_a, person_b];
        let bu = BenUnit {
            id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: false,
            rent_monthly: 0.0, is_lone_parent: false,
            ..BenUnit::default()
        };

        let mut results: Vec<PersonResult> = people.iter()
            .map(|p| calculate(p, &params, p.state_pension))
            .collect();

        let tax_before = results[1].income_tax;
        apply_marriage_allowance(&bu, &people, &mut results, &params);

        // Marriage allowance: 10% of £12,570 = £1,257 → rounded up to £1,260
        // Tax reduction: £1,260 × 0.20 = £252
        let expected_reduction = 252.0;
        let actual_reduction = tax_before - results[1].income_tax;

        assert!((actual_reduction - expected_reduction).abs() < 5.0,
            "Expected ~£252 tax reduction, got £{:.2}", actual_reduction);
        assert!((results[1].marriage_allowance_deduction - expected_reduction).abs() < 5.0);
    }

    #[test]
    fn test_marriage_allowance_higher_rate_ineligible() {
        let params = Parameters::for_year(2025).unwrap();

        // Couple: Person A earns £5,000 (has unused PA)
        //         Person B earns £80,000 (higher rate — NOT eligible as recipient)
        let mut person_a = Person::default();
        person_a.id = 0;
        person_a.age = 35.0;
        person_a.employment_income = 5000.0;

        let mut person_b = Person::default();
        person_b.id = 1;
        person_b.age = 35.0;
        person_b.employment_income = 80000.0;

        let people = vec![person_a, person_b];
        let bu = BenUnit {
            id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: false,
            rent_monthly: 0.0, is_lone_parent: false,
            ..BenUnit::default()
        };

        let mut results: Vec<PersonResult> = people.iter()
            .map(|p| calculate(p, &params, p.state_pension))
            .collect();

        let tax_before_b = results[1].income_tax;
        apply_marriage_allowance(&bu, &people, &mut results, &params);

        // Person B is higher rate — should NOT get marriage allowance
        assert!((results[1].income_tax - tax_before_b).abs() < 0.01,
            "Higher rate taxpayer should not receive marriage allowance");
    }

    // ── Scottish income tax (worked examples) ───────────────────────────────
    //
    // Scotland Act 1998 s.80C; rates set by the Scottish Rate Resolution 2025/26.
    // A Scottish taxpayer pays the five/six-band Scottish rates on non-savings,
    // non-dividend income; rUK taxpayers pay the three-band UK rates. The
    // personal allowance (£12,570) is reserved UK-wide, so the Scottish band
    // thresholds below apply to *taxable* income (income after the PA).
    //
    // Expected figures computed directly from the 2025/26 Scottish bands:
    //   starter 19% to £2,827, basic 20% to £14,921, intermediate 21% to
    //   £31,092, higher 42% to £62,430, advanced 45% to £125,140, top 48%.

    fn scottish_person(employment_income: f64) -> Person {
        let mut p = test_person(employment_income);
        p.is_in_scotland = true;
        p
    }

    #[test]
    fn test_scottish_income_tax_basic_band() {
        // £30,000 income → taxable £17,430.
        //   £2,827 @ 19%  = £537.13
        //   £12,094 @ 20% = £2,418.80  (basic band: 14,921 - 2,827)
        //   £2,509 @ 21%  = £526.89    (17,430 - 14,921)
        //   total ≈ £3,482.82
        let params = Parameters::for_year(2025).unwrap();
        let result = calculate(&scottish_person(30_000.0), &params, 0.0);
        assert!((result.income_tax - 3_482.82).abs() < 1.0,
            "Scottish £30k: expected ~£3,482.82, got £{:.2}", result.income_tax);
    }

    #[test]
    fn test_scottish_income_tax_higher_band() {
        // £50,000 income → taxable £37,430.
        //   starter  £2,827 @ 19%  = £537.13
        //   basic    £12,094 @ 20% = £2,418.80
        //   interm.  £16,171 @ 21% = £3,395.91 (31,092 - 14,921)
        //   higher   £6,338 @ 42%  = £2,661.96 (37,430 - 31,092)
        //   total ≈ £9,013.80
        let params = Parameters::for_year(2025).unwrap();
        let result = calculate(&scottish_person(50_000.0), &params, 0.0);
        assert!((result.income_tax - 9_013.80).abs() < 1.0,
            "Scottish £50k: expected ~£9,013.80, got £{:.2}", result.income_tax);
    }

    #[test]
    fn test_scottish_income_tax_advanced_band() {
        // £75,000 income → taxable £62,430 (exactly the advanced threshold).
        //   starter  £2,827 @ 19%  = £537.13
        //   basic    £12,094 @ 20% = £2,418.80
        //   interm.  £16,171 @ 21% = £3,395.91
        //   higher   £31,338 @ 42% = £13,161.96 (62,430 - 31,092)
        //   total ≈ £19,513.80
        let params = Parameters::for_year(2025).unwrap();
        let result = calculate(&scottish_person(75_000.0), &params, 0.0);
        assert!((result.income_tax - 19_513.80).abs() < 1.0,
            "Scottish £75k: expected ~£19,513.80, got £{:.2}", result.income_tax);
    }

    #[test]
    fn test_scottish_income_tax_top_band() {
        // £150,000 income → PA fully tapered (income > £125,140), taxable £150,000.
        //   starter  £2,827 @ 19%   = £537.13
        //   basic    £12,094 @ 20%  = £2,418.80
        //   interm.  £16,171 @ 21%  = £3,395.91
        //   higher   £31,338 @ 42%  = £13,161.96
        //   advanced £62,710 @ 45%  = £28,219.50 (125,140 - 62,430)
        //   top      £24,860 @ 48%  = £11,932.80 (150,000 - 125,140)
        //   total ≈ £59,666.10
        let params = Parameters::for_year(2025).unwrap();
        let result = calculate(&scottish_person(150_000.0), &params, 0.0);
        assert!((result.income_tax - 59_666.10).abs() < 1.5,
            "Scottish £150k: expected ~£59,666.10, got £{:.2}", result.income_tax);
    }

    #[test]
    fn test_scotland_vs_ruk_divergence_at_same_income() {
        // The same £50,000 earner: a Scottish taxpayer pays materially more
        // than an rUK taxpayer because of the 42% higher rate (vs 40%) and the
        // earlier higher-rate threshold (£31,092 taxable vs £37,700).
        //   rUK:      £7,486.00   Scotland: £9,013.80   gap ≈ £1,527.80
        let params = Parameters::for_year(2025).unwrap();
        let ruk = calculate(&test_person(50_000.0), &params, 0.0);
        let scot = calculate(&scottish_person(50_000.0), &params, 0.0);

        assert!((ruk.income_tax - 7_486.0).abs() < 1.0,
            "rUK £50k: expected ~£7,486, got £{:.2}", ruk.income_tax);
        assert!(scot.income_tax > ruk.income_tax,
            "Scottish taxpayer should pay more than rUK at £50k");
        assert!((scot.income_tax - ruk.income_tax - 1_527.80).abs() < 2.0,
            "Scotland-vs-rUK gap at £50k: expected ~£1,527.80, got £{:.2}",
            scot.income_tax - ruk.income_tax);

        // Low earners diverge the other way: the 19% starter rate makes Scotland
        // slightly cheaper at £15,000.
        let ruk_low = calculate(&test_person(15_000.0), &params, 0.0);
        let scot_low = calculate(&scottish_person(15_000.0), &params, 0.0);
        assert!(scot_low.income_tax < ruk_low.income_tax,
            "Scottish starter rate should make a £15k earner pay less than rUK");
    }
}

#[cfg(test)]
mod parameter_impact_tests {
    use super::*;
    use crate::parameters::Parameters;

    fn calc(p: &Person, params: &Parameters) -> PersonResult {
        calculate(p, params, p.state_pension)
    }

    fn basic_earner() -> Person {
        let mut p = Person::default();
        p.age = 35.0;
        p.employment_income = 30000.0;
        p.hours_worked = 37.5 * 52.0;
        p
    }

    fn higher_earner() -> Person {
        let mut p = Person::default();
        p.age = 35.0;
        p.employment_income = 60000.0;
        p.hours_worked = 37.5 * 52.0;
        p
    }

    fn se_earner() -> Person {
        let mut p = Person::default();
        p.age = 35.0;
        p.self_employment_income = 40000.0;
        p.hours_worked = 37.5 * 52.0;
        p
    }

    // ── Income Tax ────────────────────────────────────────────────────────────

    #[test]
    fn param_it_personal_allowance() {
        let mut params = Parameters::for_year(2025).unwrap();
        let p = basic_earner();
        let base = calc(&p, &params).income_tax;
        params.income_tax.personal_allowance += 1000.0;
        let reformed = calc(&p, &params).income_tax;
        assert!(reformed < base, "Raising PA should reduce income tax");
    }

    #[test]
    fn param_it_pa_taper_threshold() {
        let mut params = Parameters::for_year(2025).unwrap();
        // Need earner in taper zone (income > 100k)
        let mut p = Person::default(); p.age = 35.0; p.employment_income = 110000.0;
        let base = calc(&p, &params).income_tax;
        params.income_tax.pa_taper_threshold += 5000.0;
        let reformed = calc(&p, &params).income_tax;
        assert!(reformed < base, "Raising PA taper threshold should reduce tax for high earner");
    }

    #[test]
    fn param_it_pa_taper_rate() {
        let mut params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default(); p.age = 35.0; p.employment_income = 110000.0;
        let base = calc(&p, &params).income_tax;
        params.income_tax.pa_taper_rate += 0.10;
        let reformed = calc(&p, &params).income_tax;
        assert!(reformed > base, "Increasing PA taper rate should increase tax for high earner");
    }

    #[test]
    fn param_it_uk_brackets_rate() {
        let mut params = Parameters::for_year(2025).unwrap();
        let p = basic_earner();
        let base = calc(&p, &params).income_tax;
        // Increase basic rate
        if let Some(br) = params.income_tax.uk_brackets.iter_mut().find(|b| (b.rate - 0.20).abs() < 0.01) {
            br.rate += 0.05;
        }
        let reformed = calc(&p, &params).income_tax;
        assert!(reformed > base, "Raising basic rate should increase income tax");
    }

    #[test]
    fn param_it_uk_brackets_threshold() {
        let mut params = Parameters::for_year(2025).unwrap();
        let p = higher_earner();
        let base = calc(&p, &params).income_tax;
        // Raise higher-rate threshold
        if let Some(br) = params.income_tax.uk_brackets.iter_mut().find(|b| (b.rate - 0.40).abs() < 0.01) {
            br.threshold += 5000.0;
        }
        let reformed = calc(&p, &params).income_tax;
        assert!(reformed < base, "Raising higher-rate threshold should reduce tax for higher earner");
    }

    #[test]
    fn param_it_scottish_brackets() {
        let mut params = Parameters::for_year(2025).unwrap();
        let mut p = basic_earner();
        p.is_in_scotland = true;
        let base = calc(&p, &params).income_tax;
        if let Some(br) = params.income_tax.scottish_brackets.iter_mut().find(|b| b.rate > 0.15 && b.rate < 0.25) {
            br.rate += 0.05;
        }
        let reformed = calc(&p, &params).income_tax;
        assert!(reformed > base, "Raising Scottish intermediate rate should increase tax for Scottish taxpayer");
    }

    #[test]
    fn param_it_dividend_allowance() {
        let mut params = Parameters::for_year(2025).unwrap();
        // Need employment income to use up PA, then dividends above allowance (£500)
        let mut p = Person::default(); p.age = 35.0; p.employment_income = 30000.0; p.dividend_income = 2000.0;
        let base = calc(&p, &params).income_tax;
        params.income_tax.dividend_allowance += 500.0;
        let reformed = calc(&p, &params).income_tax;
        assert!(reformed < base, "Raising dividend allowance should reduce tax on dividends");
    }

    #[test]
    fn param_it_dividend_basic_rate() {
        let mut params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default(); p.age = 35.0; p.employment_income = 20000.0; p.dividend_income = 5000.0;
        let base = calc(&p, &params).income_tax;
        params.income_tax.dividend_basic_rate += 0.05;
        let reformed = calc(&p, &params).income_tax;
        assert!(reformed > base, "Raising dividend basic rate should increase tax on dividends");
    }

    #[test]
    fn param_it_dividend_higher_rate() {
        let mut params = Parameters::for_year(2025).unwrap();
        // Need earner in higher rate with dividends
        let mut p = Person::default(); p.age = 35.0; p.employment_income = 60000.0; p.dividend_income = 5000.0;
        let base = calc(&p, &params).income_tax;
        params.income_tax.dividend_higher_rate += 0.05;
        let reformed = calc(&p, &params).income_tax;
        assert!(reformed > base, "Raising dividend higher rate should increase tax");
    }

    #[test]
    fn param_it_dividend_additional_rate() {
        let mut params = Parameters::for_year(2025).unwrap();
        // Need earner in additional rate (>150k)
        let mut p = Person::default(); p.age = 35.0; p.employment_income = 160000.0; p.dividend_income = 5000.0;
        let base = calc(&p, &params).income_tax;
        params.income_tax.dividend_additional_rate += 0.05;
        let reformed = calc(&p, &params).income_tax;
        assert!(reformed > base, "Raising dividend additional rate should increase tax");
    }

    #[test]
    fn param_it_savings_starter_rate_band() {
        let mut params = Parameters::for_year(2025).unwrap();
        // earned_taxable = 13000 - 12570 = 430 (below starter band of 5000)
        // savings_taxable = 8000 (above starter band + PSA), so starter band matters
        let mut p = Person::default(); p.age = 35.0; p.employment_income = 13000.0; p.savings_interest_income = 8000.0;
        let base = calc(&p, &params).income_tax;
        params.income_tax.savings_starter_rate_band += 1000.0;
        let reformed = calc(&p, &params).income_tax;
        assert!(reformed < base, "Raising savings starter rate band should reduce tax on savings income");
    }

    #[test]
    fn param_it_marriage_allowance_max_fraction() {
        let mut params = Parameters::for_year(2025).unwrap();
        let mut pa = Person::default(); pa.age = 35.0; pa.employment_income = 5000.0;
        let mut pb = Person::default(); pb.id = 1; pb.age = 35.0; pb.employment_income = 30000.0;
        let bu = crate::engine::entities::BenUnit {
            id: 0, household_id: 0, person_ids: vec![0, 1], ..Default::default()
        };
        let people = vec![pa.clone(), pb.clone()];
        let mut results_base: Vec<PersonResult> = people.iter().map(|p| calculate(p, &params, p.state_pension)).collect();
        apply_marriage_allowance(&bu, &people, &mut results_base, &params);
        let base_tax = results_base[1].income_tax;

        params.income_tax.marriage_allowance_max_fraction = 0.15;
        let mut results_reformed: Vec<PersonResult> = people.iter().map(|p| calculate(p, &params, p.state_pension)).collect();
        apply_marriage_allowance(&bu, &people, &mut results_reformed, &params);
        assert!(results_reformed[1].income_tax < base_tax,
            "Raising MA fraction should reduce recipient's tax");
    }

    #[test]
    fn param_it_marriage_allowance_rounding() {
        let mut params = Parameters::for_year(2025).unwrap();
        let mut pa = Person::default(); pa.age = 35.0; pa.employment_income = 5000.0;
        let mut pb = Person::default(); pb.id = 1; pb.age = 35.0; pb.employment_income = 30000.0;
        let bu = crate::engine::entities::BenUnit {
            id: 0, household_id: 0, person_ids: vec![0, 1], ..Default::default()
        };
        let people = vec![pa.clone(), pb.clone()];
        let mut results_r1: Vec<PersonResult> = people.iter().map(|p| calculate(p, &params, p.state_pension)).collect();
        apply_marriage_allowance(&bu, &people, &mut results_r1, &params);

        params.income_tax.marriage_allowance_rounding = 1.0; // finer rounding → slightly different amount
        let mut results_r2: Vec<PersonResult> = people.iter().map(|p| calculate(p, &params, p.state_pension)).collect();
        apply_marriage_allowance(&bu, &people, &mut results_r2, &params);
        // With rounding=10 vs rounding=1, the transferred amount differs (1257 vs 1257 exactly)
        // This may or may not differ at integer PA; check that rounding field is at least used
        // by verifying rounding=1000 gives a different result
        params.income_tax.marriage_allowance_rounding = 1000.0;
        let mut results_r3: Vec<PersonResult> = people.iter().map(|p| calculate(p, &params, p.state_pension)).collect();
        apply_marriage_allowance(&bu, &people, &mut results_r3, &params);
        assert!(results_r1[1].income_tax != results_r3[1].income_tax
            || results_r1[0].income_tax != results_r3[0].income_tax,
            "Changing MA rounding should affect allowance amount");
    }

    // ── National Insurance ────────────────────────────────────────────────────

    #[test]
    fn param_ni_primary_threshold() {
        let mut params = Parameters::for_year(2025).unwrap();
        let p = basic_earner();
        let base = calc(&p, &params).national_insurance;
        params.national_insurance.primary_threshold_annual += 1000.0;
        let reformed = calc(&p, &params).national_insurance;
        assert!(reformed < base, "Raising NI primary threshold should reduce employee NI");
    }

    #[test]
    fn param_ni_upper_earnings_limit() {
        let mut params = Parameters::for_year(2025).unwrap();
        let p = higher_earner();
        let base = calc(&p, &params).national_insurance;
        params.national_insurance.upper_earnings_limit_annual += 5000.0;
        let reformed = calc(&p, &params).national_insurance;
        assert!(reformed > base, "Raising UEL should increase NI for higher earner (more at main rate)");
    }

    #[test]
    fn param_ni_main_rate() {
        let mut params = Parameters::for_year(2025).unwrap();
        let p = basic_earner();
        let base = calc(&p, &params).national_insurance;
        params.national_insurance.main_rate += 0.02;
        let reformed = calc(&p, &params).national_insurance;
        assert!(reformed > base, "Raising NI main rate should increase employee NI");
    }

    #[test]
    fn param_ni_additional_rate() {
        let mut params = Parameters::for_year(2025).unwrap();
        // Need earner above UEL (£50270)
        let mut p = Person::default(); p.age = 35.0; p.employment_income = 80000.0;
        let base = calc(&p, &params).national_insurance;
        params.national_insurance.additional_rate += 0.02;
        let reformed = calc(&p, &params).national_insurance;
        assert!(reformed > base, "Raising NI additional rate should increase NI above UEL");
    }

    #[test]
    fn param_ni_secondary_threshold() {
        let mut params = Parameters::for_year(2025).unwrap();
        let p = basic_earner();
        let base = calc(&p, &params).employer_ni;
        params.national_insurance.secondary_threshold_annual += 1000.0;
        let reformed = calc(&p, &params).employer_ni;
        assert!(reformed < base, "Raising secondary threshold should reduce employer NI");
    }

    #[test]
    fn param_ni_employer_rate() {
        let mut params = Parameters::for_year(2025).unwrap();
        let p = basic_earner();
        let base = calc(&p, &params).employer_ni;
        params.national_insurance.employer_rate += 0.02;
        let reformed = calc(&p, &params).employer_ni;
        assert!(reformed > base, "Raising employer rate should increase employer NI");
    }

    #[test]
    fn param_ni_class2_flat_rate() {
        // Class 2 was abolished from April 2024; test against 2023/24 where it applied
        let mut params = Parameters::for_year(2023).unwrap();
        let p = se_earner();
        let base = calc(&p, &params).national_insurance;
        params.national_insurance.class2_flat_rate_weekly += 1.0;
        let reformed = calc(&p, &params).national_insurance;
        assert!(reformed > base, "Raising Class 2 flat rate should increase NI for self-employed");
    }

    #[test]
    fn param_ni_class2_small_profits_threshold() {
        // Class 2 was abolished from April 2024; test against 2023/24 where it applied
        let mut params = Parameters::for_year(2023).unwrap();
        // Person with SE income just above SPT
        let mut p = Person::default(); p.age = 35.0; p.self_employment_income = 7000.0;
        let base = calc(&p, &params).national_insurance;
        // Raise SPT above person's income → no more Class 2
        params.national_insurance.class2_small_profits_threshold = 8000.0;
        let reformed = calc(&p, &params).national_insurance;
        assert!(reformed < base, "Raising SPT above income should remove Class 2 liability");
    }

    #[test]
    fn param_ni_class4_lower_profits_limit() {
        let mut params = Parameters::for_year(2025).unwrap();
        let p = se_earner();
        let base = calc(&p, &params).national_insurance;
        params.national_insurance.class4_lower_profits_limit += 1000.0;
        let reformed = calc(&p, &params).national_insurance;
        assert!(reformed < base, "Raising Class 4 lower limit should reduce NI for self-employed");
    }

    #[test]
    fn param_ni_class4_upper_profits_limit() {
        let mut params = Parameters::for_year(2025).unwrap();
        // Need SE earner above upper limit (~£50270)
        let mut p = Person::default(); p.age = 35.0; p.self_employment_income = 80000.0;
        let base = calc(&p, &params).national_insurance;
        params.national_insurance.class4_upper_profits_limit += 5000.0;
        let reformed = calc(&p, &params).national_insurance;
        assert!(reformed > base, "Raising Class 4 upper limit should increase NI above it (more at main rate)");
    }

    #[test]
    fn param_ni_class4_main_rate() {
        let mut params = Parameters::for_year(2025).unwrap();
        let p = se_earner();
        let base = calc(&p, &params).national_insurance;
        params.national_insurance.class4_main_rate += 0.02;
        let reformed = calc(&p, &params).national_insurance;
        assert!(reformed > base, "Raising Class 4 main rate should increase NI for self-employed");
    }

    #[test]
    fn param_ni_class4_additional_rate() {
        let mut params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default(); p.age = 35.0; p.self_employment_income = 80000.0;
        let base = calc(&p, &params).national_insurance;
        params.national_insurance.class4_additional_rate += 0.02;
        let reformed = calc(&p, &params).national_insurance;
        assert!(reformed > base, "Raising Class 4 additional rate should increase NI above upper limit");
    }
}
