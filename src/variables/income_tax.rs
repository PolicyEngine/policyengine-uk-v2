use crate::engine::entities::{Person, BenUnit};
use crate::engine::simulation::PersonResult;
use crate::parameters::{Parameters, PensionsParams, TaxBracket};

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

    // Step 1: Pension contributions and relief.
    //
    // FA 2004 Part 4. Both net-pay and relief-at-source contributions reduce
    // adjusted net income (and so reduce the PA taper). They differ in how
    // higher-rate relief is delivered:
    //   - Net pay: the contribution is taken before tax, so it also reduces
    //     taxable income directly (relief at the member's marginal rate "for
    //     free").
    //   - Relief at source: the member pays from net income; basic-rate relief
    //     is added by the provider, and higher-rate relief is delivered by
    //     extending the basic-rate band by the grossed-up contribution.
    let total_contributions =
        person.employee_pension_contributions + person.personal_pension_contributions;
    let relief_at_source = params.pensions.as_ref().map_or(false, |p| p.relief_at_source);
    let pension_basic_rate = params.pensions.as_ref().map_or(0.20, |p| p.basic_rate);

    // Adjusted net income deducts the (gross) contribution for the PA taper.
    let gross_contributions = if relief_at_source {
        gross_up_contribution(total_contributions, pension_basic_rate)
    } else {
        total_contributions
    };
    let adjusted_net_income = (total_income - gross_contributions).max(0.0);

    // Net-pay contributions also come off earned income before tax.
    let net_pay_deduction = if relief_at_source { 0.0 } else { total_contributions };

    // Step 2: Personal allowance (tapered for high earners)
    let personal_allowance = calculate_personal_allowance(adjusted_net_income, params);

    // Step 3: Allocate PA across income types (earned first, then savings, then dividends)
    // Net-pay contributions are deducted from earned income before tax.
    let earned_income = (person.employment_income + person.self_employment_income
        + person.pension_income + state_pension
        + person.property_income + person.maintenance_income
        + person.miscellaneous_income + person.other_income
        - net_pay_deduction).max(0.0);

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

    // Step 4: Earned income tax (UK or Scottish rates).
    // For relief-at-source contributions, extend the basic-rate band by the
    // grossed-up contribution so higher-rate relief is delivered.
    let base_brackets = if person.is_in_scotland {
        &params.income_tax.scottish_brackets
    } else {
        &params.income_tax.uk_brackets
    };
    let brackets: Vec<TaxBracket> = if relief_at_source && gross_contributions > 0.0 {
        extend_basic_rate_band(base_brackets, gross_contributions)
    } else {
        base_brackets.clone()
    };
    let earned_income_tax = apply_brackets(earned_taxable, &brackets);

    // Step 5: Savings income tax (stacked on top of earned)
    let savings_income_tax = calculate_savings_tax(earned_taxable, savings_taxable, params);

    // Step 6: Dividend income tax (stacked on top of earned + savings)
    let dividend_income_tax = calculate_dividend_tax(
        earned_taxable + savings_taxable,
        dividend_taxable,
        params,
    );

    let mut income_tax = earned_income_tax + savings_income_tax + dividend_income_tax;

    // Step 6b: Pension annual-allowance charge (FA 2004 s.227). Contributions
    // above the (tapered) annual allowance are taxed at the member's marginal
    // rate, clawing back the relief. Carry-forward is not modelled.
    if let Some(pensions) = params.pensions.as_ref() {
        // Bracket layout: [basic@0, higher@1, additional@2]. Thresholds give the
        // *top* of the basic and higher bands; rates are the band's own rate.
        let bk = &params.income_tax.uk_brackets;
        let basic_band_top = bk.get(1).map_or(37_700.0, |b| b.threshold);
        let higher_band_top = bk.get(2).map_or(125_140.0, |b| b.threshold);
        let basic_rate = bk.get(0).map_or(0.20, |b| b.rate);
        let higher_rate = bk.get(1).map_or(0.40, |b| b.rate);
        let additional_rate = bk.get(2).map_or(0.45, |b| b.rate);
        let marginal_rate = if taxable_income > higher_band_top {
            additional_rate
        } else if taxable_income > basic_band_top {
            higher_rate
        } else {
            basic_rate
        };
        income_tax += annual_allowance_charge(
            total_contributions, adjusted_net_income, marginal_rate, pensions,
        );
    }

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

/// Gross up a relief-at-source contribution paid from net income.
///
/// The member pays `net` and the provider reclaims basic-rate relief, so the
/// gross contribution into the pension is `net / (1 − basic_rate)`
/// (e.g. £80 net ⇒ £100 gross at a 0.20 basic rate). FA 2004 s.192.
fn gross_up_contribution(net: f64, basic_rate: f64) -> f64 {
    if basic_rate >= 1.0 || basic_rate < 0.0 {
        return net;
    }
    net / (1.0 - basic_rate)
}

/// Extend the basic-rate band by `extension`, shifting the higher (and
/// additional) rate thresholds up by the same amount (ITA 2007 s.10(3A);
/// FA 2004 s.192(4)).
///
/// In this engine bracket index 0 is the basic-rate band (it starts at £0; the
/// personal allowance is handled separately), index 1 is the higher-rate
/// threshold, index 2 the additional-rate threshold. To widen the basic-rate
/// band we push every threshold *after* index 0 up by `extension`, so more
/// income is taxed at the basic rate — delivering higher-rate relief at source.
fn extend_basic_rate_band(brackets: &[TaxBracket], extension: f64) -> Vec<TaxBracket> {
    brackets
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let threshold = if i >= 1 { b.threshold + extension } else { b.threshold };
            TaxBracket { rate: b.rate, threshold }
        })
        .collect()
}

/// Tapered annual allowance for pension contributions (FA 2004 s.228ZA).
///
/// The standard annual allowance is reduced by `taper_rate` (£1 per £2 = 0.5)
/// for every £1 of adjusted income above `annual_allowance_taper_threshold`,
/// down to `annual_allowance_minimum`. Returns the standard allowance when the
/// taper does not apply.
pub fn tapered_annual_allowance(adjusted_income: f64, params: &PensionsParams) -> f64 {
    let excess = (adjusted_income - params.annual_allowance_taper_threshold).max(0.0);
    let reduction = excess * params.annual_allowance_taper_rate;
    (params.annual_allowance - reduction).max(params.annual_allowance_minimum)
}

/// Annual-allowance charge on contributions above the (tapered) annual allowance.
///
/// FA 2004 s.227. Contributions in excess of the available annual allowance are
/// added back to taxable income and taxed at the member's marginal rate, which
/// claws back the relief given. Carry-forward of unused allowance from the three
/// prior years is **not** modelled (simplification — the FRS does not record
/// historical contributions); this overstates the charge for members with unused
/// prior-year allowance.
///
/// `marginal_rate` is the member's top income-tax rate (e.g. 0.40 for a
/// higher-rate taxpayer). `adjusted_income` drives the taper.
pub fn annual_allowance_charge(
    total_contributions: f64,
    adjusted_income: f64,
    marginal_rate: f64,
    params: &PensionsParams,
) -> f64 {
    let allowance = tapered_annual_allowance(adjusted_income, params);
    let excess = (total_contributions - allowance).max(0.0);
    excess * marginal_rate
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
}

#[cfg(test)]
mod pension_tests {
    use super::*;
    use crate::engine::entities::Person;
    use crate::parameters::{Parameters, PensionsParams};

    fn pensions_2025() -> PensionsParams {
        PensionsParams {
            annual_allowance: 60_000.0,
            annual_allowance_taper_threshold: 260_000.0,
            annual_allowance_taper_rate: 0.5,
            annual_allowance_minimum: 10_000.0,
            relief_at_source: false,
            basic_rate: 0.20,
        }
    }

    #[test]
    fn net_pay_reduces_taxable_income() {
        // £40,000 salary, £5,000 net-pay contribution. Taxable earned income
        // falls to £35,000; minus PA £12,570 = £22,430 at 20% = £4,486.
        let mut params = Parameters::for_year(2025).unwrap();
        params.pensions = Some(pensions_2025());
        let mut p = Person::default();
        p.age = 35.0;
        p.employment_income = 40_000.0;
        p.employee_pension_contributions = 5_000.0;
        let r = calculate(&p, &params, 0.0);
        assert!((r.income_tax - 4_486.0).abs() < 1.0, "got {}", r.income_tax);
    }

    #[test]
    fn relief_at_source_extends_basic_band() {
        // Higher-rate earner £60,000, £8,000 net relief-at-source contribution.
        // Gross-up at 20% → £10,000 extension of the basic-rate band.
        // Taxable income stays £60,000 - £12,570 PA = £47,430. Without extension,
        // basic band is £37,700 so £9,730 is taxed at 40%. With a £10,000
        // extension the basic band becomes £47,700, so all £47,430 is at 20%.
        let mut params = Parameters::for_year(2025).unwrap();
        let mut pens = pensions_2025();
        pens.relief_at_source = true;
        params.pensions = Some(pens);
        let mut p = Person::default();
        p.age = 35.0;
        p.employment_income = 60_000.0;
        p.personal_pension_contributions = 8_000.0;
        let r = calculate(&p, &params, 0.0);
        // All £47,430 at 20% = £9,486.
        assert!((r.income_tax - 9_486.0).abs() < 2.0, "got {}", r.income_tax);
    }

    #[test]
    fn relief_at_source_does_not_reduce_taxable_income() {
        // Basic-rate earner: relief-at-source contribution must NOT reduce
        // taxable income (the relief is given inside the pension, not by a
        // deduction). £30,000 - £12,570 = £17,430 at 20% = £3,486.
        let mut params = Parameters::for_year(2025).unwrap();
        let mut pens = pensions_2025();
        pens.relief_at_source = true;
        params.pensions = Some(pens);
        let mut p = Person::default();
        p.age = 35.0;
        p.employment_income = 30_000.0;
        p.personal_pension_contributions = 4_000.0;
        let r = calculate(&p, &params, 0.0);
        assert!((r.income_tax - 3_486.0).abs() < 1.0, "got {}", r.income_tax);
    }

    #[test]
    fn tapered_annual_allowance_worked_example() {
        let pens = pensions_2025();
        // Adjusted income £300,000: £40,000 over threshold × 0.5 = £20,000 cut
        // → £60,000 - £20,000 = £40,000.
        assert!((tapered_annual_allowance(300_000.0, &pens) - 40_000.0).abs() < 0.01);
        // Very high income floors at the £10,000 minimum.
        assert!((tapered_annual_allowance(500_000.0, &pens) - 10_000.0).abs() < 0.01);
        // Below threshold: full allowance.
        assert!((tapered_annual_allowance(100_000.0, &pens) - 60_000.0).abs() < 0.01);
    }

    #[test]
    fn annual_allowance_charge_worked_example() {
        let pens = pensions_2025();
        // £70,000 contribution, £100,000 adjusted income (no taper), higher rate.
        // Excess = £70,000 - £60,000 = £10,000 × 40% = £4,000.
        let charge = annual_allowance_charge(70_000.0, 100_000.0, 0.40, &pens);
        assert!((charge - 4_000.0).abs() < 0.01, "got {}", charge);
        // Within allowance: no charge.
        assert_eq!(annual_allowance_charge(50_000.0, 100_000.0, 0.40, &pens), 0.0);
    }

    #[test]
    fn gross_up_contribution_worked_example() {
        // £80 net at 20% → £100 gross.
        assert!((gross_up_contribution(80.0, 0.20) - 100.0).abs() < 0.01);
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
