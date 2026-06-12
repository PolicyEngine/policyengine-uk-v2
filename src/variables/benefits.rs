use crate::engine::entities::*;
use crate::engine::simulation::*;
use crate::parameters::Parameters;

/// Calculate all benefit-unit-level benefits.
///
/// UC replaces six legacy benefits (HB, IS, CTC, WTC, income-based JSA, income-related ESA).
/// A benunit is on either UC or legacy, not both.
/// Whether a benunit receives a benefit is gated by reported receipt in the survey
/// (BenUnit::claims), or unconditionally under full take-up (hypothetical households).
pub fn calculate_benunit(
    bu: &BenUnit,
    people: &[Person],
    person_results: &[PersonResult],
    household: &Household,
    params: &Parameters,
    fiscal_year: u32,
) -> BenUnitResult {
    // Non-means-tested / universal benefits (available regardless of UC/legacy)
    let child_benefit = calculate_child_benefit(bu, people, person_results, params);
    let state_pension = calculate_state_pension(bu, people, params, fiscal_year);
    // Carers Allowance: non-means-tested flat rate for informal carers.
    // Paid to individual, regardless of UC/legacy system.
    let carers_allowance = calculate_carers_allowance(bu, people, person_results, params);

    // System routing follows reported receipt: benunits that reported UC are on
    // the UC system; benunits that reported a legacy benefit stay on legacy.
    // Full take-up (hypothetical households) routes to UC, the system open to
    // new claims.
    let claims_hb  = bu.claims(people, |p| p.housing_benefit);
    let claims_ctc = bu.claims(people, |p| p.child_tax_credit);
    let claims_wtc = bu.claims(people, |p| p.working_tax_credit);
    let claims_is  = bu.claims(people, |p| p.income_support);
    let on_uc_system = bu.on_uc || bu.full_take_up;
    let on_legacy = claims_hb || claims_ctc || claims_wtc || claims_is;

    let (uc, pension_credit, housing_benefit, ctc, wtc, income_support, esa_ir, jsa_ib, scp);
    if on_uc_system {
        let would_claim = bu.claims(people, |p| p.universal_credit);
        let raw_uc = calculate_universal_credit(bu, people, person_results, household, params);
        uc = if would_claim { raw_uc } else { (0.0, raw_uc.1, raw_uc.2) };
        pension_credit = calculate_pension_credit(bu, people, params);
        housing_benefit = 0.0;
        ctc = 0.0;
        wtc = 0.0;
        income_support = 0.0;
        esa_ir = 0.0;
        jsa_ib = 0.0;
        scp = if would_claim { calculate_scottish_child_payment(bu, people, household, params) } else { 0.0 };
    } else if on_legacy {
        // Not yet migrated: still on legacy system
        uc = (0.0, 0.0, 0.0);
        pension_credit = calculate_pension_credit(bu, people, params);
        let raw_hb = calculate_housing_benefit(bu, people, person_results, household, params);
        housing_benefit = if raw_hb > 0.0 && claims_hb { raw_hb } else { 0.0 };
        let tc = calculate_tax_credits(bu, people, person_results, params);
        ctc = if tc.0 > 0.0 && claims_ctc { tc.0 } else { 0.0 };
        wtc = if tc.1 > 0.0 && claims_wtc { tc.1 } else { 0.0 };
        // Route ESA(IR), JSA(IB), IS based on eligibility.
        // ESA(IR) replaces IS for claimants with limited capability for work.
        let has_esa_eligible = bu.person_ids.iter().any(|&pid| people[pid].esa_income > 0.0 || people[pid].esa_group > 0);
        let has_jsa_eligible = bu.person_ids.iter().any(|&pid| people[pid].jsa_income > 0.0 || people[pid].looking_for_work || people[pid].emp_status == 3);
        let raw_is = calculate_income_support(bu, people, person_results, params);
        income_support = if raw_is > 0.0 && !has_esa_eligible && claims_is { raw_is } else { 0.0 };
        let raw_esa = calculate_esa_income_related(bu, people, person_results, params);
        esa_ir = if raw_esa > 0.0 && has_esa_eligible && bu.claims(people, |p| p.esa_income) { raw_esa } else { 0.0 };
        let raw_jsa = calculate_jsa_income_based(bu, people, person_results, params);
        jsa_ib = if raw_jsa > 0.0 && has_jsa_eligible && bu.claims(people, |p| p.jsa_income) { raw_jsa } else { 0.0 };
        scp = 0.0;
    } else {
        // Not on any means-tested system
        uc = (0.0, 0.0, 0.0);
        pension_credit = calculate_pension_credit(bu, people, params);
        housing_benefit = 0.0;
        ctc = 0.0;
        wtc = 0.0;
        income_support = 0.0;
        esa_ir = 0.0;
        jsa_ib = 0.0;
        scp = 0.0;
    }

    // Sum pre-cap benefits
    let pre_cap_benefits = uc.0 + child_benefit + state_pension + pension_credit
        + housing_benefit + ctc + wtc + income_support + esa_ir + jsa_ib + carers_allowance + scp;

    // Apply benefit cap
    let benefit_cap_reduction = calculate_benefit_cap(
        bu, people, person_results, household, params,
        pre_cap_benefits, child_benefit, state_pension,
    );

    // Passthrough benefits: reported amounts for benefits we don't model.
    // Includes disability benefits (PIP/DLA/AA/ADP/CDP), contributory ESA/JSA,
    // and other unmodelled benefits (bereavement, maternity, winter fuel, etc.).
    // All exempt from the benefit cap.
    let passthrough_benefits: f64 = bu.person_ids.iter().map(|&pid| {
        let p = &people[pid];
        pip_daily_living_amount(p, params) + pip_mobility_amount(p, params)
            + dla_care_amount(p, params) + dla_mobility_amount(p, params)
            + attendance_allowance_amount(p, params)
            + p.esa_contributory
            + p.jsa_contributory
            + p.other_benefits
            + p.adp_daily_living + p.adp_mobility
            + p.cdp_care + p.cdp_mobility
    }).sum();

    let modelled_benefits = (pre_cap_benefits - benefit_cap_reduction).max(0.0);
    let total_benefits = modelled_benefits + passthrough_benefits;

    BenUnitResult {
        universal_credit: uc.0,
        child_benefit,
        state_pension,
        pension_credit,
        housing_benefit,
        child_tax_credit: ctc,
        working_tax_credit: wtc,
        income_support,
        esa_income_related: esa_ir,
        jsa_income_based: jsa_ib,
        carers_allowance,
        scottish_child_payment: scp,
        benefit_cap_reduction,
        passthrough_benefits,
        total_benefits,
        uc_max_amount: uc.1,
        uc_income_reduction: uc.2,
    }
}

/// Child Benefit: eldest child gets higher rate, others get additional rate.
/// HICBC is now a separate income tax charge (applied in simulation Phase 2b),
/// so child benefit is paid in full here.
fn calculate_child_benefit(
    bu: &BenUnit,
    people: &[Person],
    _person_results: &[PersonResult],
    params: &Parameters,
) -> f64 {
    let num_children = bu.num_children(people);
    if num_children == 0 {
        return 0.0;
    }

    let weekly = params.child_benefit.eldest_weekly
        + params.child_benefit.additional_weekly * (num_children as f64 - 1.0).max(0.0);
    let annual = weekly * 52.0;

    if annual > 0.0 && !bu.claims(people, |p| p.child_benefit) { return 0.0; }
    annual
}

/// Universal Credit calculation.
///
/// MaxUC = standard_allowance + child_elements + housing + disability + LCWRA + carer
/// Earned income (after work allowance, tax, pension contribs) tapered at 55%.
/// Unearned income reduces UC pound-for-pound.
///
/// Returns (uc_amount, max_amount, income_reduction) — all annual.
fn calculate_universal_credit(
    bu: &BenUnit,
    people: &[Person],
    person_results: &[PersonResult],
    household: &Household,
    params: &Parameters,
) -> (f64, f64, f64) {
    // Basic eligibility: at least one working-age adult (not SP age)
    if !uc_has_working_age_adult(bu, people) {
        return (0.0, 0.0, 0.0);
    }

    // Maximum amount = sum of all elements at the per-month rate.
    let standard_allowance = uc_standard_allowance_monthly(bu, people, params);
    let child_element = uc_child_element_monthly(bu, people, params);
    let disabled_child_element = uc_disabled_child_element_monthly(bu, people, params);
    let has_lcwra = uc_has_lcwra(bu, people);
    let lcwra_element = uc_lcwra_element_monthly(has_lcwra, params);
    let carer_element = uc_carer_element_monthly(bu, people, params);
    let housing_element = uc_housing_element_monthly(bu, people, household, params);

    let max_amount_monthly = standard_allowance
        + child_element
        + disabled_child_element
        + lcwra_element
        + carer_element
        + housing_element;
    let max_amount_annual = max_amount_monthly * 12.0;

    // Reductions: earned income via taper after a work allowance, unearned income £-for-£.
    let work_allowance_annual = uc_work_allowance_annual(bu, people, has_lcwra, params);
    let earned_after_allowance = (uc_net_earned_income(bu, people, person_results)
        - work_allowance_annual)
        .max(0.0);
    let earned_income_reduction = earned_after_allowance * params.universal_credit.taper_rate;
    let unearned_income = uc_unearned_income(bu, people);

    let total_reduction = (earned_income_reduction + unearned_income).min(max_amount_annual);
    let uc_amount = (max_amount_annual - total_reduction).max(0.0);

    (uc_amount, max_amount_annual, total_reduction)
}

/// True when the benunit has at least one working-age adult — UC is closed to
/// pensioner-only benunits (those instead claim Pension Credit). UC Regs 2013 reg.3.
pub(crate) fn uc_has_working_age_adult(bu: &BenUnit, people: &[Person]) -> bool {
    bu.person_ids.iter()
        .filter(|&&pid| people[pid].is_adult())
        .any(|&pid| !people[pid].is_sp_age())
}

/// UC standard allowance (monthly) — UC Regs 2013 reg.36 / Sch.4 para.1.
/// Four bands by couple status × eldest-adult ≥ 25.
pub(crate) fn uc_standard_allowance_monthly(
    bu: &BenUnit, people: &[Person], params: &Parameters,
) -> f64 {
    let uc = &params.universal_credit;
    let is_couple = bu.is_couple(people);
    let eldest_age = bu.eldest_adult_age(people);
    if is_couple {
        if eldest_age >= 25.0 { uc.standard_allowance_couple_over25 }
        else { uc.standard_allowance_couple_under25 }
    } else if eldest_age >= 25.0 {
        uc.standard_allowance_single_over25
    } else {
        uc.standard_allowance_single_under25
    }
}

/// UC child element (monthly) — UC Regs 2013 reg.24 / Sch.4 para.4.
/// Two-child limit (`uc.child_limit`, normally 2) caps the number of qualifying children;
/// the first counts at the higher `child_element_first` rate and the rest at
/// `child_element_subsequent`.
pub(crate) fn uc_child_element_monthly(
    bu: &BenUnit, people: &[Person], params: &Parameters,
) -> f64 {
    let uc = &params.universal_credit;
    let capped_children = bu.num_children(people).min(uc.child_limit);
    if capped_children == 0 {
        return 0.0;
    }
    uc.child_element_first
        + uc.child_element_subsequent * (capped_children as f64 - 1.0).max(0.0)
}

/// UC disabled child element (monthly) — UC Regs 2013 Sch.4 para.5.
/// Higher rate for the severely / enhanced-disabled, lower for any other PIP/DLA/AA receipt.
pub(crate) fn uc_disabled_child_element_monthly(
    bu: &BenUnit, people: &[Person], params: &Parameters,
) -> f64 {
    let uc = &params.universal_credit;
    bu.person_ids.iter()
        .filter(|&&pid| people[pid].is_child())
        .map(|&pid| {
            let p = &people[pid];
            if p.is_severely_disabled || p.is_enhanced_disabled {
                uc.disabled_child_higher
            } else if p.is_disabled {
                uc.disabled_child_lower
            } else {
                0.0
            }
        })
        .sum()
}

/// True when any adult in the benunit qualifies for the LCWRA element.
///
/// UC Regs 2013 reg.27 — limited capability for work-related activity. FRS doesn't carry
/// the WCA outcome directly, so we use PIP daily living (any rate), DLA care mid/high,
/// or ESA support group as the proxy (LIMITILL is too broad).
pub(crate) fn uc_has_lcwra(bu: &BenUnit, people: &[Person]) -> bool {
    bu.person_ids.iter()
        .filter(|&&pid| people[pid].is_adult())
        .any(|&pid| {
            let p = &people[pid];
            p.pip_dl_std || p.pip_dl_enh || p.dla_care_mid || p.dla_care_high || p.esa_group == 1
        })
}

/// UC LCWRA element (monthly) — UC Regs 2013 Sch.4 para.7. Awarded once per benunit.
pub(crate) fn uc_lcwra_element_monthly(has_lcwra: bool, params: &Parameters) -> f64 {
    if has_lcwra { params.universal_credit.lcwra_element } else { 0.0 }
}

/// UC carer element (monthly) — UC Regs 2013 Sch.4 para.8.
/// Awarded when any adult in the benunit is a CA recipient (`is_carer` flag).
pub(crate) fn uc_carer_element_monthly(
    bu: &BenUnit, people: &[Person], params: &Parameters,
) -> f64 {
    let has_carer = bu.person_ids.iter()
        .filter(|&&pid| people[pid].is_adult())
        .any(|&pid| people[pid].is_carer);
    if has_carer { params.universal_credit.carer_element } else { 0.0 }
}

/// UC housing element (monthly) — UC Regs 2013 reg.25 / Sch.4.
///
/// For private renters, capped at the LHA rate for the household's region + bedroom
/// entitlement (30th-percentile rents; SI 2010/2591 / HB Regs 2006 reg.13D). Social
/// renters are not subject to LHA — the bedroom tax (reg.B13) applies separately and is
/// not modelled here.
pub(crate) fn uc_housing_element_monthly(
    bu: &BenUnit, people: &[Person], household: &Household, params: &Parameters,
) -> f64 {
    if let Some(cap) = lha_monthly_cap(bu, people, household, params) {
        bu.rent_monthly.min(cap)
    } else {
        bu.rent_monthly
    }
}

/// UC work allowance (annual) — UC Regs 2013 reg.22.
///
/// Available only to claimants with responsibility for a child / qualifying young
/// person, or with limited capability for work. The lower rate applies when the
/// benunit has housing costs; the higher rate otherwise. Having housing costs does
/// **not** confer entitlement to the allowance — it only determines which rate applies.
pub(crate) fn uc_work_allowance_annual(
    bu: &BenUnit, people: &[Person], has_lcwra: bool, params: &Parameters,
) -> f64 {
    let uc = &params.universal_credit;
    let has_work_allowance = bu.num_children(people) > 0 || has_lcwra;
    if !has_work_allowance {
        return 0.0;
    }
    let has_housing_costs = bu.rent_monthly > 0.0;
    if has_housing_costs {
        uc.work_allowance_lower * 12.0
    } else {
        uc.work_allowance_higher * 12.0
    }
}

/// Earned income net of income tax, NI, and pension contributions — the figure that
/// flows into the UC taper after the work allowance. UC Regs 2013 reg.55.
pub(crate) fn uc_net_earned_income(
    bu: &BenUnit, people: &[Person], person_results: &[PersonResult],
) -> f64 {
    let gross_earned: f64 = bu.person_ids.iter()
        .map(|&pid| people[pid].employment_income + people[pid].self_employment_income)
        .sum();
    let tax_and_ni: f64 = bu.person_ids.iter()
        .map(|&pid| person_results[pid].income_tax + person_results[pid].national_insurance)
        .sum();
    let pension_contribs: f64 = bu.person_ids.iter()
        .map(|&pid| people[pid].employee_pension_contributions + people[pid].personal_pension_contributions)
        .sum();
    (gross_earned - tax_and_ni - pension_contribs).max(0.0)
}

/// Unearned income — UC Regs 2013 reg.66. Reduces the UC entitlement pound-for-pound.
pub(crate) fn uc_unearned_income(bu: &BenUnit, people: &[Person]) -> f64 {
    bu.person_ids.iter()
        .map(|&pid| {
            let p = &people[pid];
            p.savings_interest_income
                + p.pension_income
                + p.maintenance_income
                + p.property_income
                + p.other_income
        })
        .sum()
}

/// State Pension calculation following policyengine-uk logic.
///
/// New SP (reached SP age after April 2016): full parameter rate directly.
/// Basic SP (reached SP age before April 2016): reported amount, capped at
/// basic SP max, scaled by reform_rate/baseline_rate for basic SP parameter.
///
/// New SP started April 2016. SP age is 66. So in fiscal year Y, the cutoff
/// is: anyone aged > 66 + (Y - 2016) was already SP-age when new SP began,
/// and is therefore on basic SP. Everyone else on SP is on new SP.
/// DLA care component amount (annual).
///
/// If the person has a recorded amount (`p.dla_care > 0`), returns that amount
/// unchanged — preserves FRS-recorded values which may reflect partial-year
/// claims or amounts predating a reform. If the recorded amount is 0 but a rate
/// flag is set, returns the computed weekly rate × 52 from `params.dla`. Returns
/// 0 when neither holds or when no DLA parameters are loaded.
/// SSCBA 1992 Sch.2 para.2.
pub fn dla_care_amount(p: &Person, params: &Parameters) -> f64 {
    if p.dla_care > 0.0 {
        return p.dla_care;
    }
    let dla = match &params.dla { Some(p) => p, None => return 0.0 };
    if p.dla_care_high {
        dla.care_high_weekly * 52.0
    } else if p.dla_care_mid {
        dla.care_mid_weekly * 52.0
    } else if p.dla_care_low {
        dla.care_low_weekly * 52.0
    } else {
        0.0
    }
}

/// PIP daily-living component amount (annual).
///
/// If the person has a recorded amount (`p.pip_daily_living > 0`), returns that
/// amount unchanged — preserves FRS-recorded values which may reflect partial-
/// year claims, transitional protection, or amounts predating a reform. If the
/// recorded amount is 0 but the standard or enhanced flag is set, returns the
/// computed weekly rate × 52 from `params.pip`. Returns 0 when neither holds
/// or when no PIP parameters are loaded.
pub fn pip_daily_living_amount(p: &Person, params: &Parameters) -> f64 {
    if p.pip_daily_living > 0.0 {
        return p.pip_daily_living;
    }
    let pip = match &params.pip { Some(p) => p, None => return 0.0 };
    if p.pip_dl_enh {
        pip.daily_living_enhanced_weekly * 52.0
    } else if p.pip_dl_std {
        pip.daily_living_standard_weekly * 52.0
    } else {
        0.0
    }
}

/// DLA mobility component amount (annual). SSCBA 1992 Sch.2 para.3.
pub fn dla_mobility_amount(p: &Person, params: &Parameters) -> f64 {
    if p.dla_mobility > 0.0 {
        return p.dla_mobility;
    }
    let dla = match &params.dla { Some(p) => p, None => return 0.0 };
    if p.dla_mob_high {
        dla.mobility_high_weekly * 52.0
    } else if p.dla_mob_low {
        dla.mobility_low_weekly * 52.0
    } else {
        0.0
    }
}

/// Attendance Allowance amount (annual). SSCBA 1992 s.64.
pub fn attendance_allowance_amount(p: &Person, params: &Parameters) -> f64 {
    if p.attendance_allowance > 0.0 {
        return p.attendance_allowance;
    }
    let aa = match &params.aa { Some(p) => p, None => return 0.0 };
    if p.aa_high {
        aa.high_weekly * 52.0
    } else if p.aa_low {
        aa.low_weekly * 52.0
    } else {
        0.0
    }
}

/// PIP mobility component amount (annual). See `pip_daily_living_amount`.
pub fn pip_mobility_amount(p: &Person, params: &Parameters) -> f64 {
    if p.pip_mobility > 0.0 {
        return p.pip_mobility;
    }
    let pip = match &params.pip { Some(p) => p, None => return 0.0 };
    if p.pip_mob_enh {
        pip.mobility_enhanced_weekly * 52.0
    } else if p.pip_mob_std {
        pip.mobility_standard_weekly * 52.0
    } else {
        0.0
    }
}

/// Calculate state pension for a single person.
/// New SP recipients get the parameter rate; basic (pre-2016) SP recipients
/// have entitlements set by contribution histories the model cannot
/// reconstruct, so their reported amount is taken as-is and is not
/// parametrised.
pub fn person_state_pension(
    person: &Person,
    params: &Parameters,
    fiscal_year: u32,
) -> f64 {
    if !person.is_sp_age() || !person.is_adult() {
        return 0.0;
    }

    let basic_sp_min_age = 66.0 + (fiscal_year as f64 - 2016.0);

    if person.age >= basic_sp_min_age {
        person.state_pension
    } else {
        params.state_pension.new_state_pension_weekly * 52.0
    }
}

fn calculate_state_pension(
    bu: &BenUnit,
    people: &[Person],
    params: &Parameters,
    fiscal_year: u32,
) -> f64 {
    bu.person_ids.iter()
        .map(|&pid| person_state_pension(&people[pid], params, fiscal_year))
        .sum()
}

/// Pension Credit: Guarantee Credit + Savings Credit.
///
/// Guarantee Credit: max(0, minimum_guarantee - income).
/// Savings Credit: max(0, min(income - threshold, max_savings_credit) - max(0, income - minimum_guarantee) * 0.40).
/// But savings credit only applies to those reaching SP age before 6 April 2016 — we include it
/// but the data should flag eligibility. Here we calculate it for all SP-age claimants.
fn calculate_pension_credit(
    bu: &BenUnit,
    people: &[Person],
    params: &Parameters,
) -> f64 {
    let any_sp_age = bu.person_ids.iter()
        .filter(|&&pid| people[pid].is_adult())
        .any(|&pid| people[pid].is_sp_age());
    if !any_sp_age {
        return 0.0;
    }

    let is_couple = bu.is_couple(people);
    let pc = &params.pension_credit;

    let min_guarantee_weekly = if is_couple {
        pc.standard_minimum_couple
    } else {
        pc.standard_minimum_single
    };
    let min_guarantee_annual = min_guarantee_weekly * 52.0;

    // Income for PC purposes
    let income: f64 = bu.person_ids.iter()
        .map(|&pid| {
            let p = &people[pid];
            p.state_pension
                + p.pension_income
                + p.employment_income
                + p.self_employment_income
                + p.savings_interest_income
        })
        .sum();

    // Guarantee Credit
    let gc = (min_guarantee_annual - income).max(0.0);

    // Savings Credit (for those who reached SP age before 6 Apr 2016)
    let sc_threshold = if is_couple {
        pc.savings_credit_threshold_couple
    } else {
        pc.savings_credit_threshold_single
    };
    let _sc_threshold_annual = sc_threshold * 52.0;

    // Maximum savings credit = 60% of (minimum guarantee - savings credit threshold)
    let max_sc_weekly = (min_guarantee_weekly - sc_threshold) * 0.60;
    let qualifying_income_weekly = income / 52.0;

    let sc = if qualifying_income_weekly > sc_threshold && max_sc_weekly > 0.0 {
        let credit = (qualifying_income_weekly - sc_threshold).min(max_sc_weekly);
        let excess_over_mg = (qualifying_income_weekly - min_guarantee_weekly).max(0.0);
        let sc_weekly = (credit - excess_over_mg * 0.40).max(0.0);
        sc_weekly * 52.0
    } else {
        0.0
    };

    let amount = gc + sc;
    if amount > 0.0 && !bu.claims(people, |p| p.pension_credit) { return 0.0; }
    amount
}

/// Calculate LHA bedroom entitlement for a benefit unit.
///
/// Implements UC Regs 2013 Sch.4 / HB Regs 2006 Sch.B1.
/// Rules:
///   - 1 room for the benefit unit adults (single or couple)
///   - 1 room per non-dependant over 16 living in the same household but outside the benunit
///   - Children under 16 must share unless same-gender sharing is impossible:
///     * Children 10–15 share in same-gender pairs first
///     * Spaces left by an odd-numbered gender group can be filled by under-10s
///     * Remaining under-10s share in mixed pairs
///
/// Returns bedroom entitlement (1–4+; 0 = shared accommodation for single under threshold).
/// The shared accommodation rate (0) is not applied here — callers handle it separately.
pub fn lha_bedroom_entitlement(bu: &BenUnit, people: &[Person], household: &Household) -> u32 {
    // Non-dependants: household members aged 16+ not in this benefit unit
    let non_dependants = household.person_ids.iter()
        .filter(|&&pid| {
            people[pid].age >= 16.0 && people[pid].benunit_id != bu.id
        })
        .count() as u32;

    // Children: under-16s in this benefit unit
    let boys_over_10: u32 = bu.person_ids.iter()
        .filter(|&&pid| {
            let p = &people[pid];
            p.age >= 10.0 && p.age < 16.0 && p.gender == Gender::Male
        })
        .count() as u32;
    let girls_over_10: u32 = bu.person_ids.iter()
        .filter(|&&pid| {
            let p = &people[pid];
            p.age >= 10.0 && p.age < 16.0 && p.gender == Gender::Female
        })
        .count() as u32;
    let boys_under_10: u32 = bu.person_ids.iter()
        .filter(|&&pid| {
            let p = &people[pid];
            p.age < 10.0 && p.gender == Gender::Male
        })
        .count() as u32;
    let girls_under_10: u32 = bu.person_ids.iter()
        .filter(|&&pid| {
            let p = &people[pid];
            p.age < 10.0 && p.gender == Gender::Female
        })
        .count() as u32;

    // Over-10s share in same-gender pairs
    let over_10_rooms = (boys_over_10 + 1) / 2 + (girls_over_10 + 1) / 2;

    // Spaces available in over-10 rooms for under-10s of the same gender
    let space_for_boy_under_10 = boys_over_10 % 2;
    let space_for_girl_under_10 = girls_over_10 % 2;

    let leftover_boys = boys_under_10.saturating_sub(space_for_boy_under_10);
    let leftover_girls = girls_under_10.saturating_sub(space_for_girl_under_10);

    // Remaining under-10s share in pairs (mixed is allowed for under-10s)
    let under_10_rooms = (leftover_boys + leftover_girls + 1) / 2;

    let bedrooms = 1 + non_dependants + over_10_rooms + under_10_rooms;
    bedrooms.min(4) // Cap at 4 (Category E covers 4+)
}

/// Return the monthly LHA cap for a benefit unit, or None if LHA doesn't apply.
///
/// LHA applies only to private renters (TenureType::RentPrivately).
/// Social renters (council / HA) and owner-occupiers are not subject to LHA caps.
pub(crate) fn lha_monthly_cap(
    bu: &BenUnit,
    people: &[Person],
    household: &Household,
    params: &Parameters,
) -> Option<f64> {
    let lha = params.lha.as_ref()?;
    if !lha.enabled { return None; }
    if household.tenure_type != TenureType::RentPrivately { return None; }
    let bedrooms = lha_bedroom_entitlement(bu, people, household);
    let region_idx = household.region.to_lha_region_idx();
    lha.monthly_cap(region_idx, bedrooms)
}

/// Housing Benefit (legacy system).
///
/// HB = max(0, eligible_rent - max(0, (income - applicable_amount) * 65%))
///
/// Applicable amount = personal allowance + family premium + child allowances.
fn calculate_housing_benefit(
    bu: &BenUnit,
    people: &[Person],
    _person_results: &[PersonResult],
    household: &Household,
    params: &Parameters,
) -> f64 {
    let hb_params = match &params.housing_benefit {
        Some(hb) => hb,
        None => return 0.0,
    };

    // For private renters, eligible rent is capped at the LHA rate for the household's
    // region and bedroom entitlement (HB Regs 2006 reg.13D; 30th percentile from SI 2010/2591).
    // For social renters and owner-occupiers, full rent is used (no LHA cap).
    let rent_monthly_capped = if let Some(cap) = lha_monthly_cap(bu, people, household, params) {
        bu.rent_monthly.min(cap)
    } else {
        bu.rent_monthly
    };
    let eligible_rent = rent_monthly_capped * 12.0;
    if eligible_rent <= 0.0 {
        return 0.0;
    }

    // Applicable amount (weekly → annual)
    let is_couple = bu.is_couple(people);
    let eldest_age = bu.eldest_adult_age(people);
    let num_children = bu.num_children(people);

    let personal_allowance_weekly = if is_couple {
        hb_params.personal_allowance_couple
    } else if eldest_age >= 25.0 {
        hb_params.personal_allowance_single_25_plus
    } else {
        hb_params.personal_allowance_single_under25
    };

    let family_premium_weekly = if num_children > 0 { hb_params.family_premium } else { 0.0 };
    let child_allowance_weekly = hb_params.child_allowance * num_children as f64;
    let dp_weekly = disability_premiums_weekly(bu, people, params);

    let applicable_amount = (personal_allowance_weekly + family_premium_weekly
        + child_allowance_weekly + dp_weekly) * 52.0;

    // Income for HB purposes
    let income: f64 = bu.person_ids.iter()
        .map(|&pid| {
            let p = &people[pid];
            p.employment_income + p.self_employment_income
                + p.pension_income + p.state_pension
                + p.savings_interest_income + p.other_income
        })
        .sum();

    let excess_income = (income - applicable_amount).max(0.0);
    let reduction = excess_income * hb_params.withdrawal_rate;

    let amount = (eligible_rent - reduction).max(0.0);
    amount
}

/// Tax Credits: Working Tax Credit (WTC) and Child Tax Credit (CTC).
///
/// Maximum = WTC elements + CTC elements.
/// Income reduction = max(0, (income - threshold) * 41%).
/// CTC reduced first, then WTC.
///
/// Returns (ctc, wtc).
fn calculate_tax_credits(
    bu: &BenUnit,
    people: &[Person],
    _person_results: &[PersonResult],
    params: &Parameters,
) -> (f64, f64) {
    let tc = match &params.tax_credits {
        Some(tc) => tc,
        None => return (0.0, 0.0),
    };

    let num_children = bu.num_children(people);
    let is_couple = bu.is_couple(people);

    // CTC: available if there are children
    let max_ctc = if num_children > 0 {
        tc.ctc_family_element + tc.ctc_child_element * num_children as f64
            + bu.person_ids.iter()
                .filter(|&&pid| people[pid].is_child())
                .map(|&pid| {
                    let p = &people[pid];
                    if p.is_severely_disabled || p.is_enhanced_disabled {
                        tc.ctc_severely_disabled_child_element + tc.ctc_disabled_child_element
                    } else if p.is_disabled {
                        tc.ctc_disabled_child_element
                    } else {
                        0.0
                    }
                })
                .sum::<f64>()
    } else {
        0.0
    };

    // WTC: available if working sufficient hours
    let total_hours_weekly: f64 = bu.person_ids.iter()
        .filter(|&&pid| people[pid].is_adult())
        .map(|&pid| people[pid].hours_worked / 52.0)
        .sum();

    let min_hours = if is_couple {
        tc.wtc_min_hours_couple
    } else {
        tc.wtc_min_hours_single
    };

    let max_wtc = if total_hours_weekly >= min_hours {
        let mut wtc = tc.wtc_basic_element;
        if is_couple {
            wtc += tc.wtc_couple_element;
        } else if bu.is_lone_parent {
            wtc += tc.wtc_lone_parent_element;
        }
        if total_hours_weekly >= 30.0 {
            wtc += tc.wtc_30_hour_element;
        }
        wtc
    } else {
        0.0
    };

    if max_ctc + max_wtc <= 0.0 {
        return (0.0, 0.0);
    }

    // Income for tax credits
    let income: f64 = bu.person_ids.iter()
        .map(|&pid| {
            let p = &people[pid];
            p.employment_income + p.self_employment_income
                + p.pension_income + p.state_pension
                + p.savings_interest_income + p.dividend_income
                + p.property_income + p.other_income
        })
        .sum();

    let excess = (income - tc.income_threshold).max(0.0);
    let reduction = excess * tc.taper_rate;

    // CTC reduced first, then WTC
    let ctc = (max_ctc - reduction).max(0.0);
    let remaining_reduction = (reduction - max_ctc).max(0.0);
    let wtc = (max_wtc - remaining_reduction).max(0.0);

    (ctc, wtc)
}

/// Income Support: legacy means-tested benefit for specific groups
/// (lone parents with young children, carers, disabled).
///
/// IS = max(0, applicable_amount - income).
/// Applicable amount includes disability premiums where applicable.
/// Source: Income Support (General) Regs 1987 (SI 1987/1967) regs 17-22 and Sch.2.
fn calculate_income_support(
    bu: &BenUnit,
    people: &[Person],
    _person_results: &[PersonResult],
    params: &Parameters,
) -> f64 {
    let hb_params = match &params.housing_benefit {
        Some(hb) => hb,
        None => return 0.0,
    };

    let is_couple = bu.is_couple(people);
    let eldest_age = bu.eldest_adult_age(people);
    let num_children = bu.num_children(people);

    let personal_allowance_weekly = if is_couple {
        hb_params.personal_allowance_couple
    } else if eldest_age >= 25.0 {
        hb_params.personal_allowance_single_25_plus
    } else {
        hb_params.personal_allowance_single_under25
    };

    let family_premium_weekly = if num_children > 0 { hb_params.family_premium } else { 0.0 };
    let child_allowance_weekly = hb_params.child_allowance * num_children as f64;
    let dp_weekly = disability_premiums_weekly(bu, people, params);

    let applicable_amount = (personal_allowance_weekly + family_premium_weekly
        + child_allowance_weekly + dp_weekly) * 52.0;

    let income: f64 = bu.person_ids.iter()
        .map(|&pid| {
            let p = &people[pid];
            p.employment_income + p.self_employment_income
                + p.pension_income + p.state_pension
                + p.savings_interest_income + p.other_income
        })
        .sum();

    (applicable_amount - income).max(0.0)
}

/// Compute disability premium additions to an applicable amount (weekly).
///
/// Used by IS, HB (applicable amount), ESA, and JSA to add disability premiums.
///
/// Premiums:
///  - Disability Premium (DP): person has lower-rate DLA/PIP or is in WRAG/assessment.
///    IS (General) Regs 1987 Sch.2 para.11.
///  - Enhanced Disability Premium (EDP): highest DLA care or enhanced PIP DL.
///    Sch.2 para.13.
///  - Severe Disability Premium (SDP): enhanced PIP/highest DLA care, lives alone (or both
///    disabled in a couple), no non-disabled carer receiving CA.
///    Sch.2 para.14.
///  - Carer Premium: at least one person in the bu receives CA.
///    Sch.2 para.14D.
fn disability_premiums_weekly(
    bu: &BenUnit,
    people: &[Person],
    params: &crate::parameters::Parameters,
) -> f64 {
    let dp_params = match &params.disability_premiums {
        Some(p) => p,
        None => return 0.0,
    };

    let is_couple = bu.is_couple(people);
    let adults: Vec<&Person> = bu.person_ids.iter()
        .filter(|&&pid| people[pid].is_adult())
        .map(|&pid| &people[pid])
        .collect();

    // DP: any adult has disability flag (lower/mid DLA care, any DLA mob, any PIP, AA, or WRAG)
    let any_dp = adults.iter().any(|p| {
        p.dla_care_low || p.dla_care_mid || p.dla_care_high
        || p.dla_mob_low || p.dla_mob_high
        || p.pip_dl_std || p.pip_dl_enh
        || p.pip_mob_std || p.pip_mob_enh
        || p.aa_low || p.aa_high
        || p.esa_group == 2 || p.esa_group == 3  // WRAG or assessment phase
    });
    let dp_weekly = if any_dp {
        if is_couple { dp_params.disability_premium_couple }
        else { dp_params.disability_premium_single }
    } else { 0.0 };

    // EDP: any adult has enhanced PIP DL or highest DLA care
    let any_edp = adults.iter().any(|p| p.pip_dl_enh || p.dla_care_high);
    let edp_weekly = if any_edp {
        if is_couple { dp_params.enhanced_disability_premium_couple }
        else { dp_params.enhanced_disability_premium_single }
    } else { 0.0 };

    // SDP: severely disabled adult(s), living alone or both disabled in couple, no CA carer.
    // Simplified: if any adult has pip_dl_enh or dla_care_high AND is_carer is false for all adults
    // and no non-disabled person in the bu.
    let num_severely_disabled = adults.iter().filter(|p| p.is_severely_disabled).count();
    let any_carer_in_bu = adults.iter().any(|p| p.is_carer);
    let sdp_weekly = if !any_carer_in_bu && num_severely_disabled > 0 {
        if is_couple && num_severely_disabled >= 2 {
            dp_params.severe_disability_premium * 2.0
        } else if !is_couple {
            dp_params.severe_disability_premium
        } else {
            0.0  // One disabled, one non-disabled in couple → no SDP (non-disabled = potential carer)
        }
    } else { 0.0 };

    // Carer Premium: any adult receives CA
    let carer_premium_weekly = if any_carer_in_bu { dp_params.carer_premium } else { 0.0 };

    dp_weekly + edp_weekly + sdp_weekly + carer_premium_weekly
}

/// ESA (Income-Related): IS equivalent for claimants with limited capability for work.
///
/// ESA(IR) = max(0, applicable_amount - income).
/// Applicable amount = personal allowance + disability premiums + work-related/support component.
///
/// Source: Welfare Reform Act 2007 c.5 s.2; ESA Regs 2008 (SI 2008/794) regs 67-74.
fn calculate_esa_income_related(
    bu: &BenUnit,
    people: &[Person],
    _person_results: &[PersonResult],
    params: &Parameters,
) -> f64 {
    let irb = match &params.income_related_benefits {
        Some(p) => p,
        None => return 0.0,
    };

    let is_couple = bu.is_couple(people);
    let eldest_age = bu.eldest_adult_age(people);
    let num_children = bu.num_children(people);

    let personal_allowance_weekly = if is_couple {
        irb.esa_allowance_couple
    } else if eldest_age >= 25.0 {
        irb.esa_allowance_single_25_plus
    } else {
        irb.esa_allowance_single_under25
    };

    // Work-related component or support component based on ESA group.
    // Support group (esa_group=1) gets support component; WRAG (esa_group=2) or assessment
    // phase gets WRAG component. Only for adults with an ESA group.
    let extra_component_weekly: f64 = bu.person_ids.iter()
        .filter(|&&pid| people[pid].is_adult())
        .map(|&pid| {
            match people[pid].esa_group {
                1 => irb.esa_support_component,
                2 | 3 => irb.esa_wrag_component,
                _ => 0.0,
            }
        })
        .fold(0.0_f64, f64::max); // Only one component per bu

    let family_premium_weekly: f64 = if num_children > 0 {
        params.housing_benefit.as_ref().map_or(0.0, |hb| hb.family_premium)
    } else { 0.0 };
    let child_allowance_weekly: f64 = params.housing_benefit.as_ref()
        .map_or(0.0, |hb| hb.child_allowance * num_children as f64);

    let dp_weekly = disability_premiums_weekly(bu, people, params);

    let applicable_amount = (personal_allowance_weekly + extra_component_weekly
        + family_premium_weekly + child_allowance_weekly + dp_weekly) * 52.0;

    let income: f64 = bu.person_ids.iter()
        .map(|&pid| {
            let p = &people[pid];
            p.employment_income + p.self_employment_income
                + p.pension_income + p.state_pension
                + p.savings_interest_income + p.other_income
        })
        .sum();

    (applicable_amount - income).max(0.0)
}

/// JSA (Income-Based): IS equivalent for jobseekers available for and actively seeking work.
///
/// JSA(IB) = max(0, applicable_amount - income).
/// Applicable amount = personal allowance + disability premiums (if applicable) + family premiums.
///
/// Source: Jobseekers Act 1995 c.18 s.4-5; JSA Regs 1996 (SI 1996/207) regs 83-96.
fn calculate_jsa_income_based(
    bu: &BenUnit,
    people: &[Person],
    _person_results: &[PersonResult],
    params: &Parameters,
) -> f64 {
    let irb = match &params.income_related_benefits {
        Some(p) => p,
        None => return 0.0,
    };

    // At least one adult must be looking for work (LOOKWK=1) or have JSA-eligible emp_status
    let any_available = bu.person_ids.iter()
        .filter(|&&pid| people[pid].is_adult())
        .any(|&pid| {
            let p = &people[pid];
            p.looking_for_work || p.emp_status == 3  // 3=unemployed
        });
    if !any_available {
        return 0.0;
    }

    let is_couple = bu.is_couple(people);
    let eldest_age = bu.eldest_adult_age(people);
    let num_children = bu.num_children(people);

    let personal_allowance_weekly = if is_couple {
        irb.jsa_allowance_couple
    } else if eldest_age >= 25.0 {
        irb.jsa_allowance_single_25_plus
    } else {
        irb.jsa_allowance_single_under25
    };

    let family_premium_weekly: f64 = if num_children > 0 {
        params.housing_benefit.as_ref().map_or(0.0, |hb| hb.family_premium)
    } else { 0.0 };
    let child_allowance_weekly: f64 = params.housing_benefit.as_ref()
        .map_or(0.0, |hb| hb.child_allowance * num_children as f64);

    let dp_weekly = disability_premiums_weekly(bu, people, params);

    let applicable_amount = (personal_allowance_weekly + family_premium_weekly
        + child_allowance_weekly + dp_weekly) * 52.0;

    let income: f64 = bu.person_ids.iter()
        .map(|&pid| {
            let p = &people[pid];
            p.employment_income + p.self_employment_income
                + p.pension_income + p.state_pension
                + p.savings_interest_income + p.other_income
        })
        .sum();

    (applicable_amount - income).max(0.0)
}

/// Carers Allowance: non-means-tested flat rate for informal carers.
///
/// Eligibility:
///   - Aged 16+
///   - Spends ≥35 hours/week caring for a severely disabled person
///   - Net earnings after deductions ≤ earnings disregard
///   - Not in full-time education
///
/// In the FRS, we use:
///   - `is_self_identified_carer` (CARER1=1) as the caring hours proxy
///   - The disabled person must be in the same or different household (FRS can't tell, so
///     we award CA to any reported carer who passes the earnings test)
///   - Reported CA receipt used as the take-up gate (passthrough for reported claimants,
///     no modelling of new claimants — CA eligibility is hard to fully determine from FRS)
///
/// Source: SSCBA 1992 s.70; SS (Carers Allowance) Regs 2002 (SI 2002/2690).
fn calculate_carers_allowance(
    bu: &BenUnit,
    people: &[Person],
    person_results: &[PersonResult],
    params: &Parameters,
) -> f64 {
    let irb = match &params.income_related_benefits {
        Some(p) => p,
        None => return 0.0,
    };

    bu.person_ids.iter()
        .filter(|&&pid| people[pid].is_adult())
        .map(|&pid| {
            let p = &people[pid];
            // Passthrough for reported claimants, subject to earnings test
            let reported_ca = p.carers_allowance > 0.0;
            let is_eligible_carer = p.is_carer || p.is_self_identified_carer;
            if !reported_ca && !is_eligible_carer { return 0.0; }

            // Earnings test: net earnings must be <= ca_earnings_disregard_weekly
            let gross_earned = p.employment_income + p.self_employment_income;
            let ni_deduction = person_results[pid].national_insurance;
            let pension_contribs = p.employee_pension_contributions + p.personal_pension_contributions;
            let net_earned_weekly = (gross_earned - ni_deduction - pension_contribs).max(0.0) / 52.0;
            let passes_earnings_test = net_earned_weekly <= irb.ca_earnings_disregard_weekly;

            if (reported_ca || is_eligible_carer) && passes_earnings_test {
                irb.carers_allowance_weekly * 52.0
            } else {
                0.0
            }
        })
        .sum()
}

/// Scottish Child Payment: £26.70/week per eligible child under 16.
/// Only available in Scotland to UC/legacy benefit claimants.
fn calculate_scottish_child_payment(
    bu: &BenUnit,
    people: &[Person],
    household: &Household,
    params: &Parameters,
) -> f64 {
    let scp = match &params.scottish_child_payment {
        Some(scp) => scp,
        None => return 0.0,
    };

    if !household.region.is_scotland() {
        return 0.0;
    }

    let eligible_children = bu.person_ids.iter()
        .filter(|&&pid| {
            let p = &people[pid];
            p.is_child() && p.age < scp.max_age
        })
        .count();

    scp.weekly_amount * 52.0 * eligible_children as f64
}

/// Benefit Cap: limits total benefits to a maximum level.
///
/// Welfare Reform Act 2012 s.96. Different caps for London/outside London,
/// single/non-single. Exempt if earning above threshold.
///
/// Returns the reduction amount (to be subtracted from total benefits).
fn calculate_benefit_cap(
    bu: &BenUnit,
    people: &[Person],
    person_results: &[PersonResult],
    household: &Household,
    params: &Parameters,
    total_benefits: f64,
    _child_benefit: f64,
    state_pension: f64,
) -> f64 {
    let cap_params = match &params.benefit_cap {
        Some(bc) => bc,
        None => return 0.0,
    };

    // Exempt if earnings above threshold
    let net_earnings: f64 = bu.person_ids.iter()
        .map(|&pid| {
            let p = &people[pid];
            let gross = p.employment_income + p.self_employment_income;
            let deductions = person_results[pid].income_tax + person_results[pid].national_insurance;
            (gross - deductions).max(0.0)
        })
        .sum();

    if net_earnings >= cap_params.earnings_exemption_threshold {
        return 0.0;
    }

    // SP-age exempt
    let any_sp_age = bu.person_ids.iter()
        .filter(|&&pid| people[pid].is_adult())
        .any(|&pid| people[pid].is_sp_age());
    if any_sp_age {
        return 0.0;
    }

    // Exempt if anyone in the benunit receives disability benefits (PIP, DLA, AA)
    // or carer's allowance or ESA support group
    let any_disability_exempt = bu.person_ids.iter().any(|&pid| {
        let p = &people[pid];
        p.pip_daily_living > 0.0
            || p.pip_mobility > 0.0
            || p.dla_care > 0.0
            || p.dla_mobility > 0.0
            || p.attendance_allowance > 0.0
            || p.carers_allowance > 0.0
            || p.esa_income > 0.0
            || p.esa_contributory > 0.0
    });
    if any_disability_exempt {
        return 0.0;
    }

    let is_single_no_children = !bu.is_couple(people) && bu.num_children(people) == 0;
    let is_london = household.region == Region::London;

    let annual_cap = if is_single_no_children {
        if is_london { cap_params.single_london } else { cap_params.single_outside_london }
    } else {
        if is_london { cap_params.non_single_london } else { cap_params.non_single_outside_london }
    };

    // Benefits subject to cap (exclude state pension and some disability benefits)
    let capped_benefits = total_benefits - state_pension;

    (capped_benefits - annual_cap).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_single_bu(employment_income: f64, num_children: usize) -> (Vec<Person>, BenUnit, Household) {
        let mut people = vec![{
            let mut p = Person::default();
            p.age = 30.0;
            p.employment_income = employment_income;
            p.hours_worked = 37.5 * 52.0;
            p
        }];
        let mut ids = vec![0];
        for i in 0..num_children {
            let mut child = Person::default();
            child.id = i + 1;
            child.age = 5.0;
            people.push(child);
            ids.push(i + 1);
        }
        let bu = BenUnit {
            id: 0,
            household_id: 0,
            person_ids: ids,
            on_uc: true,
            rent_monthly: 800.0,
            is_lone_parent: num_children > 0,
            full_take_up: true,
            ..BenUnit::default()
        };
        let hh = Household {
            id: 0,
            benunit_ids: vec![0],
            person_ids: (0..=num_children).collect(),
            weight: 1.0,
            region: Region::London,
            rent: 800.0 * 12.0,
            council_tax: 1500.0,
            ..Household::default()
        };
        (people, bu, hh)
    }

    #[test]
    fn test_child_benefit_two_children() {
        let params = Parameters::for_year(2025).unwrap();
        let (people, bu, hh) = make_single_bu(25000.0, 2);
        let person_results: Vec<PersonResult> = people.iter()
            .map(|p| crate::variables::income_tax::calculate(p, &params, p.state_pension))
            .collect();
        let result = calculate_benunit(&bu, &people, &person_results, &hh, &params, 2025);
        let expected_cb = params.child_benefit.eldest_weekly * 52.0
            + params.child_benefit.additional_weekly * 52.0;
        assert!((result.child_benefit - expected_cb).abs() < 1.0);
    }

    #[test]
    fn test_uc_low_earner() {
        let params = Parameters::for_year(2025).unwrap();
        let (people, bu, hh) = make_single_bu(10000.0, 1);
        let person_results: Vec<PersonResult> = people.iter()
            .map(|p| crate::variables::income_tax::calculate(p, &params, p.state_pension))
            .collect();
        let result = calculate_benunit(&bu, &people, &person_results, &hh, &params, 2025);
        assert!(result.universal_credit > 0.0, "Low earner should receive UC");
    }

    #[test]
    fn test_uc_disabled_child_element() {
        let params = Parameters::for_year(2025).unwrap();
        let (mut people, bu, hh) = make_single_bu(10000.0, 1);
        people[1].is_disabled = true;
        let person_results: Vec<PersonResult> = people.iter()
            .map(|p| crate::variables::income_tax::calculate(p, &params, p.state_pension))
            .collect();
        let result = calculate_benunit(&bu, &people, &person_results, &hh, &params, 2025);
        assert!(result.uc_max_amount > 0.0);

        let (people2, bu2, hh2) = make_single_bu(10000.0, 1);
        let pr2: Vec<PersonResult> = people2.iter()
            .map(|p| crate::variables::income_tax::calculate(p, &params, p.state_pension))
            .collect();
        let result2 = calculate_benunit(&bu2, &people2, &pr2, &hh2, &params, 2025);
        assert!(result.uc_max_amount > result2.uc_max_amount,
            "Disabled child should increase UC max amount");
    }

    #[test]
    fn test_uc_with_lcwra() {
        let params = Parameters::for_year(2025).unwrap();
        let (mut people, bu, hh) = make_single_bu(0.0, 0);
        people[0].is_disabled = true;
        people[0].pip_dl_std = true; // PIP DL standard rate → LCWRA eligible
        let person_results: Vec<PersonResult> = people.iter()
            .map(|p| crate::variables::income_tax::calculate(p, &params, p.state_pension))
            .collect();
        let result = calculate_benunit(&bu, &people, &person_results, &hh, &params, 2025);
        let expected_min = (params.universal_credit.standard_allowance_single_over25
            + params.universal_credit.lcwra_element
            + 800.0) * 12.0;
        assert!((result.uc_max_amount - expected_min).abs() < 1.0,
            "Expected max ~{}, got {}", expected_min, result.uc_max_amount);
    }

    #[test]
    fn test_uc_unearned_income_reduces() {
        let params = Parameters::for_year(2025).unwrap();
        let (mut people, bu, hh) = make_single_bu(0.0, 0);
        people[0].savings_interest_income = 5000.0;
        let person_results: Vec<PersonResult> = people.iter()
            .map(|p| crate::variables::income_tax::calculate(p, &params, p.state_pension))
            .collect();
        let result = calculate_benunit(&bu, &people, &person_results, &hh, &params, 2025);
        assert!(result.uc_income_reduction >= 5000.0,
            "£5000 unearned income should reduce UC by at least £5000, got {}", result.uc_income_reduction);
    }

    #[test]
    fn test_pension_credit_guarantee() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 70.0;
        p.state_pension = 9000.0; // Below minimum guarantee
        let people = vec![p];
        let bu = BenUnit {
            id: 0, household_id: 0, person_ids: vec![0],
            on_uc: false,
            rent_monthly: 0.0, is_lone_parent: false,
            full_take_up: true,
            ..BenUnit::default()
        };
        let hh = Household {
            id: 0, benunit_ids: vec![0], person_ids: vec![0],
            weight: 1.0, region: Region::London, rent: 0.0, council_tax: 0.0,
            ..Household::default()
        };
        let pr: Vec<PersonResult> = people.iter()
            .map(|p| crate::variables::income_tax::calculate(p, &params, p.state_pension))
            .collect();
        let result = calculate_benunit(&bu, &people, &pr, &hh, &params, 2025);
        let mg_annual = params.pension_credit.standard_minimum_single * 52.0;
        // GC = mg - income
        assert!(result.pension_credit > 0.0, "Should receive pension credit");
        assert!((result.pension_credit - (mg_annual - 9000.0)).abs() < 200.0,
            "Expected ~{}, got {}", mg_annual - 9000.0, result.pension_credit);
    }

    #[test]
    fn test_housing_benefit() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 30.0;
        p.employment_income = 10000.0;
        p.housing_benefit = 1000.0; // reported receipt → claims HB only
        let people = vec![p];
        let bu = BenUnit {
            id: 0, household_id: 0, person_ids: vec![0],
            on_uc: false,
            rent_monthly: 600.0, is_lone_parent: false,
            ..BenUnit::default()
        };
        let hh = Household {
            id: 0, benunit_ids: vec![0], person_ids: vec![0],
            weight: 1.0, region: Region::London, rent: 7200.0, council_tax: 0.0,
            ..Household::default()
        };
        let pr: Vec<PersonResult> = people.iter()
            .map(|p| crate::variables::income_tax::calculate(p, &params, p.state_pension))
            .collect();
        let result = calculate_benunit(&bu, &people, &pr, &hh, &params, 2025);
        // Reported HB receipt, no reported UC → stays on the legacy system
        assert!(result.housing_benefit > 0.0, "Reported HB recipient should get HB");
        assert!(result.housing_benefit <= 7200.0, "HB should not exceed rent");
    }

    #[test]
    fn test_tax_credits() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 30.0;
        p.employment_income = 15000.0;
        p.hours_worked = 35.0 * 52.0;
        let mut child = Person::default();
        child.id = 1;
        child.age = 5.0;
        let people = vec![p, child];
        let bu = BenUnit {
            id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: false,
            rent_monthly: 0.0, is_lone_parent: true,
            full_take_up: true,
            ..BenUnit::default()
        };
        let hh = Household {
            id: 0, benunit_ids: vec![0], person_ids: vec![0, 1],
            weight: 1.0, region: Region::London, rent: 0.0, council_tax: 0.0,
            ..Household::default()
        };
        let pr: Vec<PersonResult> = people.iter()
            .map(|p| crate::variables::income_tax::calculate(p, &params, p.state_pension))
            .collect();
        let result = calculate_benunit(&bu, &people, &pr, &hh, &params, 2025);
        // Full take-up routes to the UC system
        assert!(result.universal_credit > 0.0,
            "Low-income lone parent under full take-up should receive UC. UC={}",
            result.universal_credit);
    }

    #[test]
    fn test_benefit_cap() {
        let params = Parameters::for_year(2025).unwrap();
        // Non-working single person in London with massive UC entitlement
        let (people, mut bu, hh) = make_single_bu(0.0, 4);
        bu.rent_monthly = 3000.0; // Very high rent to push above cap
        let pr: Vec<PersonResult> = people.iter()
            .map(|p| crate::variables::income_tax::calculate(p, &params, p.state_pension))
            .collect();
        let result = calculate_benunit(&bu, &people, &pr, &hh, &params, 2025);
        // With 4 children and £3000/month rent, total benefits should hit cap
        if let Some(bc) = &params.benefit_cap {
            let cap = bc.non_single_london;
            // Total benefits after cap should not exceed cap + state pension (which is exempt)
            assert!(result.total_benefits <= cap + result.state_pension + 1.0,
                "Benefits after cap should be <= £{}, got £{}", cap, result.total_benefits);
        }
    }

    #[test]
    fn test_scottish_child_payment() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 30.0;
        let mut child = Person::default();
        child.id = 1;
        child.age = 5.0;
        let people = vec![p, child];
        let bu = BenUnit {
            id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: true,
            rent_monthly: 0.0, is_lone_parent: true,
            full_take_up: true,
            ..BenUnit::default()
        };
        let hh = Household {
            id: 0, benunit_ids: vec![0], person_ids: vec![0, 1],
            weight: 1.0, region: Region::Scotland, rent: 0.0, council_tax: 0.0,
            ..Household::default()
        };
        let pr: Vec<PersonResult> = people.iter()
            .map(|p| crate::variables::income_tax::calculate(p, &params, p.state_pension))
            .collect();
        let result = calculate_benunit(&bu, &people, &pr, &hh, &params, 2025);
        if let Some(scp) = &params.scottish_child_payment {
            let expected = scp.weekly_amount * 52.0;
            assert!((result.scottish_child_payment - expected).abs() < 1.0,
                "Expected SCP ~£{}, got £{}", expected, result.scottish_child_payment);
        }
    }

    // ── UC element-level tests ────────────────────────────────────────────
    //
    // These exercise the extracted `uc_*` functions in isolation so each
    // element is covered by a focused, fast test that doesn't touch the rest
    // of the benunit pipeline. Aggregate behaviour is still covered by the
    // `test_uc_*` tests above.

    #[test]
    fn uc_standard_allowance_picks_couple_band() {
        // A 30+30 couple should receive the couple_over25 rate.
        let params = Parameters::for_year(2025).unwrap();
        let p1 = { let mut p = Person::default(); p.id = 0; p.age = 30.0; p };
        let p2 = { let mut p = Person::default(); p.id = 1; p.age = 30.0; p };
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1], ..BenUnit::default() };
        let amount = uc_standard_allowance_monthly(&bu, &[p1, p2], &params);
        assert!((amount - params.universal_credit.standard_allowance_couple_over25).abs() < 1e-6);
    }

    #[test]
    fn uc_standard_allowance_under_25_single() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default(); p.age = 22.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0], ..BenUnit::default() };
        let amount = uc_standard_allowance_monthly(&bu, &[p], &params);
        assert!((amount - params.universal_credit.standard_allowance_single_under25).abs() < 1e-6);
    }

    #[test]
    fn uc_child_element_respects_two_child_limit() {
        let params = Parameters::for_year(2025).unwrap();
        let (people3, bu3, _) = make_single_bu(0.0, 3); // 3 children
        let (people2, bu2, _) = make_single_bu(0.0, 2); // 2 children
        // Three-child benunit gets the same child element as two-child (limit binds).
        let amount3 = uc_child_element_monthly(&bu3, &people3, &params);
        let amount2 = uc_child_element_monthly(&bu2, &people2, &params);
        assert!((amount3 - amount2).abs() < 1e-6,
            "Three-child element {amount3} should equal two-child element {amount2} (cap)");
    }

    #[test]
    fn uc_child_element_zero_when_no_children() {
        let params = Parameters::for_year(2025).unwrap();
        let (people, bu, _) = make_single_bu(0.0, 0);
        assert_eq!(uc_child_element_monthly(&bu, &people, &params), 0.0);
    }

    #[test]
    fn uc_work_allowance_gated_on_children_or_lcwra() {
        let params = Parameters::for_year(2025).unwrap();
        let (no_kids_people, no_kids_bu, _) = make_single_bu(0.0, 0);
        // Childless, no LCWRA → no work allowance regardless of housing costs.
        assert_eq!(uc_work_allowance_annual(&no_kids_bu, &no_kids_people, false, &params), 0.0);

        // With a child → entitled. With housing costs → lower rate (lower < higher).
        let (kids_people, kids_bu, _) = make_single_bu(0.0, 1);
        let with_housing = uc_work_allowance_annual(&kids_bu, &kids_people, false, &params);
        let mut no_housing_bu = kids_bu.clone();
        no_housing_bu.rent_monthly = 0.0;
        let without_housing = uc_work_allowance_annual(&no_housing_bu, &kids_people, false, &params);
        assert!(with_housing > 0.0);
        assert!(without_housing > with_housing,
            "no-housing rate {without_housing} should exceed has-housing rate {with_housing}");
    }
}

/// Tests asserting that every parameter has a measurable impact on simulation output.
/// Each test: baseline vs reformed params, assert direction of change.
#[cfg(test)]
mod parameter_impact_tests {
    use super::*;
    use crate::parameters::Parameters;

    fn base_person_uc() -> (Parameters, Person, BenUnit, Household) {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 30.0;
        p.employment_income = 8000.0;
        let bu = BenUnit {
            id: 0, household_id: 0, person_ids: vec![0],
            on_uc: true,
            rent_monthly: 500.0, is_lone_parent: false,
            full_take_up: true,
            ..BenUnit::default()
        };
        let hh = Household {
            id: 0, benunit_ids: vec![0], person_ids: vec![0],
            weight: 1.0, region: Region::London, rent: 6000.0, council_tax: 0.0,
            ..Household::default()
        };
        (params, p, bu, hh)
    }

    fn calc(params: &Parameters, people: &[Person], bu: &BenUnit, hh: &Household) -> BenUnitResult {
        let pr: Vec<PersonResult> = people.iter()
            .map(|p| crate::variables::income_tax::calculate(p, params, p.state_pension))
            .collect();
        calculate_benunit(bu, people, &pr, hh, params, 2025)
    }

    // ── UC parameters ────────────────────────────────────────────────────────

    #[test]
    fn param_uc_standard_allowance_single_over25() {
        let (p, mut params) = (base_person_uc().1, base_person_uc().0);
        let (bu, hh) = (base_person_uc().2, base_person_uc().3);
        let base = calc(&params, &[p.clone()], &bu, &hh).universal_credit;
        params.universal_credit.standard_allowance_single_over25 += 100.0;
        let reformed = calc(&params, &[p], &bu, &hh).universal_credit;
        assert!(reformed > base, "Increasing UC standard allowance (25+) should increase UC");
    }

    #[test]
    fn param_uc_standard_allowance_single_under25() {
        let (mut params, _, bu, hh) = base_person_uc();
        let mut p = Person::default(); p.age = 22.0; p.employment_income = 5000.0;
        let base = calc(&params, &[p.clone()], &bu, &hh).universal_credit;
        params.universal_credit.standard_allowance_single_under25 += 100.0;
        let reformed = calc(&params, &[p], &bu, &hh).universal_credit;
        assert!(reformed > base, "Increasing UC standard allowance (under 25) should increase UC");
    }

    #[test]
    fn param_uc_standard_allowance_couple_over25() {
        let (mut params, _, _, hh) = base_person_uc();
        let mut p1 = Person::default(); p1.age = 35.0; p1.employment_income = 5000.0;
        let mut p2 = Person::default(); p2.id = 1; p2.age = 33.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: true, rent_monthly: 500.0, full_take_up: true, ..BenUnit::default() };
        let base = calc(&params, &[p1.clone(), p2.clone()], &bu, &hh).universal_credit;
        params.universal_credit.standard_allowance_couple_over25 += 100.0;
        let reformed = calc(&params, &[p1, p2], &bu, &hh).universal_credit;
        assert!(reformed > base, "Increasing UC couple allowance (25+) should increase UC");
    }

    #[test]
    fn param_uc_standard_allowance_couple_under25() {
        let (mut params, _, _, hh) = base_person_uc();
        let mut p1 = Person::default(); p1.age = 22.0; p1.employment_income = 5000.0;
        let mut p2 = Person::default(); p2.id = 1; p2.age = 21.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: true, rent_monthly: 500.0, full_take_up: true, ..BenUnit::default() };
        let base = calc(&params, &[p1.clone(), p2.clone()], &bu, &hh).universal_credit;
        params.universal_credit.standard_allowance_couple_under25 += 100.0;
        let reformed = calc(&params, &[p1, p2], &bu, &hh).universal_credit;
        assert!(reformed > base, "Increasing UC couple allowance (under 25) should increase UC");
    }

    #[test]
    fn param_uc_child_element_first() {
        let (mut params, p, _, hh) = base_person_uc();
        let mut child = Person::default(); child.id = 1; child.age = 5.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: true, rent_monthly: 0.0, full_take_up: true, is_lone_parent: true, ..BenUnit::default() };
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).universal_credit;
        params.universal_credit.child_element_first += 100.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).universal_credit;
        assert!(reformed > base, "Increasing UC first child element should increase UC");
    }

    #[test]
    fn param_uc_child_element_subsequent() {
        let (mut params, p, _, hh) = base_person_uc();
        let mut c1 = Person::default(); c1.id = 1; c1.age = 5.0;
        let mut c2 = Person::default(); c2.id = 2; c2.age = 3.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1, 2],
            on_uc: true, rent_monthly: 0.0, full_take_up: true, is_lone_parent: true, ..BenUnit::default() };
        let base = calc(&params, &[p.clone(), c1.clone(), c2.clone()], &bu, &hh).universal_credit;
        params.universal_credit.child_element_subsequent += 100.0;
        let reformed = calc(&params, &[p, c1, c2], &bu, &hh).universal_credit;
        assert!(reformed > base, "Increasing UC subsequent child element should increase UC");
    }

    #[test]
    fn param_uc_disabled_child_lower() {
        let (mut params, p, _, hh) = base_person_uc();
        let mut child = Person::default(); child.id = 1; child.age = 5.0; child.is_disabled = true;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: true, rent_monthly: 0.0, full_take_up: true, is_lone_parent: true, ..BenUnit::default() };
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).universal_credit;
        params.universal_credit.disabled_child_lower += 100.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).universal_credit;
        assert!(reformed > base, "Increasing disabled child lower element should increase UC");
    }

    #[test]
    fn param_uc_disabled_child_higher() {
        let (mut params, p, _, hh) = base_person_uc();
        let mut child = Person::default(); child.id = 1; child.age = 5.0; child.is_enhanced_disabled = true;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: true, rent_monthly: 0.0, full_take_up: true, is_lone_parent: true, ..BenUnit::default() };
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).universal_credit;
        params.universal_credit.disabled_child_higher += 100.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).universal_credit;
        assert!(reformed > base, "Increasing disabled child higher element should increase UC");
    }

    #[test]
    fn param_uc_lcwra_element() {
        let (mut params, mut p, bu, hh) = base_person_uc();
        p.is_disabled = true;
        p.pip_dl_std = true; // PIP DL standard rate → LCWRA eligible
        let base = calc(&params, &[p.clone()], &bu, &hh).universal_credit;
        params.universal_credit.lcwra_element += 100.0;
        let reformed = calc(&params, &[p], &bu, &hh).universal_credit;
        assert!(reformed > base, "Increasing LCWRA element should increase UC");
    }

    #[test]
    fn param_uc_carer_element() {
        let (mut params, mut p, bu, hh) = base_person_uc();
        p.is_carer = true;
        let base = calc(&params, &[p.clone()], &bu, &hh).universal_credit;
        params.universal_credit.carer_element += 100.0;
        let reformed = calc(&params, &[p], &bu, &hh).universal_credit;
        assert!(reformed > base, "Increasing carer element should increase UC");
    }

    #[test]
    fn param_uc_taper_rate() {
        let (mut params, p, bu, hh) = base_person_uc();
        let base = calc(&params, &[p.clone()], &bu, &hh).universal_credit;
        params.universal_credit.taper_rate += 0.10;
        let reformed = calc(&params, &[p], &bu, &hh).universal_credit;
        assert!(reformed < base, "Increasing taper rate should reduce UC for earner");
    }

    #[test]
    fn param_uc_work_allowance_higher() {
        let (mut params, mut p, _, hh) = base_person_uc();
        // No housing costs → higher work allowance applies
        // Need income above work_allowance_higher (684/mo=8208/yr) for taper to bite
        p.employment_income = 15000.0;
        let mut child = Person::default(); child.id = 1; child.age = 5.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: true, rent_monthly: 0.0, full_take_up: true, is_lone_parent: true, ..BenUnit::default() };
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).universal_credit;
        params.universal_credit.work_allowance_higher += 500.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).universal_credit;
        assert!(reformed > base, "Increasing higher work allowance should increase UC");
    }

    #[test]
    fn param_uc_work_allowance_lower() {
        let (mut params, p, _bu, hh) = base_person_uc();
        // Has housing costs → lower work allowance applies
        let mut child = Person::default(); child.id = 1; child.age = 5.0;
        let bu2 = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: true, rent_monthly: 500.0, full_take_up: true, is_lone_parent: true, ..BenUnit::default() };
        let base = calc(&params, &[p.clone(), child.clone()], &bu2, &hh).universal_credit;
        params.universal_credit.work_allowance_lower += 500.0;
        let reformed = calc(&params, &[p, child], &bu2, &hh).universal_credit;
        assert!(reformed > base, "Increasing lower work allowance should increase UC");
    }

    #[test]
    fn param_uc_child_limit() {
        let (mut params, p, _, hh) = base_person_uc();
        let mut c1 = Person::default(); c1.id = 1; c1.age = 5.0;
        let mut c2 = Person::default(); c2.id = 2; c2.age = 3.0;
        let mut c3 = Person::default(); c3.id = 3; c3.age = 1.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1, 2, 3],
            on_uc: true, rent_monthly: 0.0, full_take_up: true, is_lone_parent: true, ..BenUnit::default() };
        params.universal_credit.child_limit = 2;
        let base = calc(&params, &[p.clone(), c1.clone(), c2.clone(), c3.clone()], &bu, &hh).universal_credit;
        params.universal_credit.child_limit = 3;
        let reformed = calc(&params, &[p, c1, c2, c3], &bu, &hh).universal_credit;
        assert!(reformed > base, "Increasing child limit should increase UC for 3-child family");
    }

    // ── Child Benefit parameters ──────────────────────────────────────────────

    #[test]
    fn param_cb_eldest_weekly() {
        let (mut params, p, _, hh) = base_person_uc();
        let mut child = Person::default(); child.id = 1; child.age = 5.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: false, full_take_up: true, ..BenUnit::default() };
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).child_benefit;
        params.child_benefit.eldest_weekly += 10.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).child_benefit;
        assert!(reformed > base, "Increasing eldest CB rate should increase CB");
    }

    #[test]
    fn param_cb_additional_weekly() {
        let (mut params, p, _, hh) = base_person_uc();
        let mut c1 = Person::default(); c1.id = 1; c1.age = 5.0;
        let mut c2 = Person::default(); c2.id = 2; c2.age = 3.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1, 2],
            full_take_up: true, ..BenUnit::default() };
        let base = calc(&params, &[p.clone(), c1.clone(), c2.clone()], &bu, &hh).child_benefit;
        params.child_benefit.additional_weekly += 10.0;
        let reformed = calc(&params, &[p, c1, c2], &bu, &hh).child_benefit;
        assert!(reformed > base, "Increasing additional child CB rate should increase CB");
    }

    // HICBC parameter tests moved to simulation-level tests (see simulation.rs)

    // ── State Pension parameters ──────────────────────────────────────────────

    #[test]
    fn param_state_pension_new_weekly() {
        let (mut params, _, _, hh) = base_person_uc();
        let mut p = Person::default(); p.age = 68.0; // SP age, no reported SP
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0],
            ..BenUnit::default() };
        let base = calc(&params, &[p.clone()], &bu, &hh).state_pension;
        params.state_pension.new_state_pension_weekly += 10.0;
        let reformed = calc(&params, &[p], &bu, &hh).state_pension;
        assert!(reformed > base, "Increasing new SP weekly rate should increase state pension");
    }

    #[test]
    fn basic_sp_taken_as_reported() {
        let (params, _, _, hh) = base_person_uc();
        let mut p = Person::default(); p.age = 82.0; // Old cohort (80+)
        p.state_pension = 7_000.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0],
            ..BenUnit::default() };
        let result = calc(&params, &[p], &bu, &hh).state_pension;
        assert_eq!(result, 7_000.0, "Basic SP should pass through the reported amount unchanged");
    }

    // ── Pension Credit parameters ─────────────────────────────────────────────

    #[test]
    fn param_pc_standard_minimum_single() {
        let (mut params, _, _, hh) = base_person_uc();
        let mut p = Person::default(); p.age = 68.0; p.state_pension = 5000.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0],
            full_take_up: true, ..BenUnit::default() };
        let base = calc(&params, &[p.clone()], &bu, &hh).pension_credit;
        params.pension_credit.standard_minimum_single += 10.0;
        let reformed = calc(&params, &[p], &bu, &hh).pension_credit;
        assert!(reformed > base, "Increasing PC single minimum should increase pension credit");
    }

    #[test]
    fn param_pc_standard_minimum_couple() {
        let (mut params, _, _, hh) = base_person_uc();
        let mut p1 = Person::default(); p1.age = 68.0; p1.state_pension = 3000.0;
        let mut p2 = Person::default(); p2.id = 1; p2.age = 67.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            full_take_up: true, ..BenUnit::default() };
        let base = calc(&params, &[p1.clone(), p2.clone()], &bu, &hh).pension_credit;
        params.pension_credit.standard_minimum_couple += 10.0;
        let reformed = calc(&params, &[p1, p2], &bu, &hh).pension_credit;
        assert!(reformed > base, "Increasing PC couple minimum should increase pension credit");
    }

    #[test]
    fn param_pc_savings_credit_threshold_single() {
        let (mut params, _, _, hh) = base_person_uc();
        let mut p = Person::default(); p.age = 68.0; p.state_pension = 10000.0; p.savings_interest_income = 2000.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0],
            full_take_up: true, ..BenUnit::default() };
        let base = calc(&params, &[p.clone()], &bu, &hh).pension_credit;
        params.pension_credit.savings_credit_threshold_single += 500.0;
        let reformed = calc(&params, &[p], &bu, &hh).pension_credit;
        assert!(reformed != base, "Changing PC savings credit threshold single should affect pension credit");
    }

    #[test]
    fn param_pc_savings_credit_threshold_couple() {
        let (mut params, _, _, hh) = base_person_uc();
        // SC threshold couple = £314.34/wk = ~£16.3k/yr; need income above it
        // Use income ~£18k to be above threshold but near guarantee (£346.60*52=~£18k)
        let mut p1 = Person::default(); p1.age = 68.0; p1.state_pension = 10000.0; p1.savings_interest_income = 8000.0;
        let mut p2 = Person::default(); p2.id = 1; p2.age = 67.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            full_take_up: true, ..BenUnit::default() };
        let base = calc(&params, &[p1.clone(), p2.clone()], &bu, &hh).pension_credit;
        // Raising threshold reduces SC (fewer people qualify / lower credit)
        params.pension_credit.savings_credit_threshold_couple += 500.0;
        let reformed = calc(&params, &[p1, p2], &bu, &hh).pension_credit;
        assert!(reformed != base, "Changing PC savings credit threshold couple should affect pension credit");
    }

    // ── Housing Benefit parameters ────────────────────────────────────────────

    #[test]
    fn param_hb_withdrawal_rate() {
        let (mut params, mut p, _, hh) = base_person_uc();
        p.employment_income = 5000.0;
        p.housing_benefit = 1000.0; // reported receipt → legacy HB
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0],
            rent_monthly: 500.0, ..BenUnit::default() };
        let base = calc(&params, &[p.clone()], &bu, &hh).housing_benefit;
        params.housing_benefit.as_mut().unwrap().withdrawal_rate -= 0.10;
        let reformed = calc(&params, &[p], &bu, &hh).housing_benefit;
        assert!(reformed > base, "Reducing HB withdrawal rate should increase HB for earner");
    }

    #[test]
    fn param_hb_personal_allowance_single_25_plus() {
        let (mut params, mut p, _, hh) = base_person_uc();
        p.employment_income = 5000.0;
        p.housing_benefit = 1000.0; // reported receipt → legacy HB
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0],
            rent_monthly: 500.0, ..BenUnit::default() };
        let base = calc(&params, &[p.clone()], &bu, &hh).housing_benefit;
        params.housing_benefit.as_mut().unwrap().personal_allowance_single_25_plus += 20.0;
        let reformed = calc(&params, &[p], &bu, &hh).housing_benefit;
        assert!(reformed > base, "Increasing HB personal allowance (25+) should increase HB");
    }

    #[test]
    fn param_hb_personal_allowance_single_under25() {
        let (mut params, _, _, hh) = base_person_uc();
        // Under-25 personal allowance ~£71.70/wk = ~£3728/yr; use income clearly above it
        let mut p = Person::default(); p.age = 22.0; p.employment_income = 6000.0;
        p.housing_benefit = 1000.0; // reported receipt → legacy HB
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0],
            rent_monthly: 500.0, ..BenUnit::default() };
        let base = calc(&params, &[p.clone()], &bu, &hh).housing_benefit;
        params.housing_benefit.as_mut().unwrap().personal_allowance_single_under25 += 20.0;
        let reformed = calc(&params, &[p], &bu, &hh).housing_benefit;
        assert!(reformed > base, "Increasing HB personal allowance (under 25) should increase HB");
    }

    #[test]
    fn param_hb_personal_allowance_couple() {
        let (mut params, _, _, hh) = base_person_uc();
        // Couple allowance ~£142.25/wk = ~£7397/yr; use income clearly above it
        let mut p1 = Person::default(); p1.age = 35.0; p1.employment_income = 10000.0;
        p1.housing_benefit = 1000.0; // reported receipt → legacy HB
        let mut p2 = Person::default(); p2.id = 1; p2.age = 33.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            rent_monthly: 500.0, ..BenUnit::default() };
        let base = calc(&params, &[p1.clone(), p2.clone()], &bu, &hh).housing_benefit;
        params.housing_benefit.as_mut().unwrap().personal_allowance_couple += 20.0;
        let reformed = calc(&params, &[p1, p2], &bu, &hh).housing_benefit;
        assert!(reformed > base, "Increasing HB couple allowance should increase HB");
    }

    #[test]
    fn param_hb_child_allowance() {
        let (mut params, _, _, hh) = base_person_uc();
        // Single + child: applicable ~(90.50 + 18.53 + 83.73) * 52 = ~£10k; use income above
        let mut p = Person::default(); p.age = 30.0; p.employment_income = 15000.0;
        p.housing_benefit = 1000.0; // reported receipt → legacy HB
        let mut child = Person::default(); child.id = 1; child.age = 5.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            rent_monthly: 500.0, is_lone_parent: true, ..BenUnit::default() };
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).housing_benefit;
        params.housing_benefit.as_mut().unwrap().child_allowance += 20.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).housing_benefit;
        assert!(reformed > base, "Increasing HB child allowance should increase HB");
    }

    #[test]
    fn param_hb_family_premium() {
        let (mut params, _, _, hh) = base_person_uc();
        let mut p = Person::default(); p.age = 30.0; p.employment_income = 15000.0;
        p.housing_benefit = 1000.0; // reported receipt → legacy HB
        let mut child = Person::default(); child.id = 1; child.age = 5.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            rent_monthly: 500.0, is_lone_parent: true, ..BenUnit::default() };
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).housing_benefit;
        params.housing_benefit.as_mut().unwrap().family_premium += 10.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).housing_benefit;
        assert!(reformed > base, "Increasing HB family premium should increase HB");
    }

    // ── Tax Credits parameters ────────────────────────────────────────────────

    fn legacy_tc_setup() -> (Parameters, Person, Person, BenUnit, Household) {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default(); p.age = 30.0; p.employment_income = 12000.0; p.hours_worked = 35.0 * 52.0;
        p.working_tax_credit = 1000.0; p.child_tax_credit = 1000.0; // reported receipt → legacy TC
        let mut child = Person::default(); child.id = 1; child.age = 5.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            rent_monthly: 0.0, is_lone_parent: true, ..BenUnit::default() };
        let hh = Household { id: 0, benunit_ids: vec![0], person_ids: vec![0, 1],
            weight: 1.0, region: Region::London, rent: 0.0, council_tax: 0.0, ..Household::default() };
        (params, p, child, bu, hh)
    }

    #[test]
    fn param_tc_wtc_basic_element() {
        let (mut params, p, child, bu, hh) = legacy_tc_setup();
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).working_tax_credit;
        params.tax_credits.as_mut().unwrap().wtc_basic_element += 500.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).working_tax_credit;
        assert!(reformed > base, "Increasing WTC basic element should increase WTC");
    }

    #[test]
    fn param_tc_wtc_couple_element() {
        let (mut params, _, _, _, hh) = legacy_tc_setup();
        let mut p1 = Person::default(); p1.age = 30.0; p1.employment_income = 8000.0; p1.hours_worked = 35.0 * 52.0;
        p1.working_tax_credit = 1000.0; // reported receipt → legacy TC
        let mut p2 = Person::default(); p2.id = 1; p2.age = 28.0;
        let mut child = Person::default(); child.id = 2; child.age = 5.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1, 2],
            ..BenUnit::default() };
        let base = calc(&params, &[p1.clone(), p2.clone(), child.clone()], &bu, &hh).working_tax_credit;
        params.tax_credits.as_mut().unwrap().wtc_couple_element += 500.0;
        let reformed = calc(&params, &[p1, p2, child], &bu, &hh).working_tax_credit;
        assert!(reformed > base, "Increasing WTC couple element should increase WTC");
    }

    #[test]
    fn param_tc_wtc_lone_parent_element() {
        let (mut params, p, child, bu, hh) = legacy_tc_setup();
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).working_tax_credit;
        params.tax_credits.as_mut().unwrap().wtc_lone_parent_element += 500.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).working_tax_credit;
        assert!(reformed > base, "Increasing WTC lone parent element should increase WTC");
    }

    #[test]
    fn param_tc_wtc_30_hour_element() {
        let (mut params, p, child, bu, hh) = legacy_tc_setup();
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).working_tax_credit;
        params.tax_credits.as_mut().unwrap().wtc_30_hour_element += 500.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).working_tax_credit;
        assert!(reformed > base, "Increasing WTC 30-hour element should increase WTC");
    }

    #[test]
    fn param_tc_ctc_child_element() {
        let (mut params, p, child, bu, hh) = legacy_tc_setup();
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).child_tax_credit;
        params.tax_credits.as_mut().unwrap().ctc_child_element += 500.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).child_tax_credit;
        assert!(reformed > base, "Increasing CTC child element should increase CTC");
    }

    #[test]
    fn param_tc_ctc_family_element() {
        let (mut params, p, child, bu, hh) = legacy_tc_setup();
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).child_tax_credit;
        params.tax_credits.as_mut().unwrap().ctc_family_element += 200.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).child_tax_credit;
        assert!(reformed > base, "Increasing CTC family element should increase CTC");
    }

    #[test]
    fn param_tc_ctc_disabled_child_element() {
        let (mut params, p, mut child, bu, hh) = legacy_tc_setup();
        child.is_disabled = true;
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).child_tax_credit;
        params.tax_credits.as_mut().unwrap().ctc_disabled_child_element += 500.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).child_tax_credit;
        assert!(reformed > base, "Increasing CTC disabled child element should increase CTC");
    }

    #[test]
    fn param_tc_ctc_severely_disabled_child_element() {
        let (mut params, p, mut child, bu, hh) = legacy_tc_setup();
        child.is_enhanced_disabled = true; child.is_severely_disabled = true;
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).child_tax_credit;
        params.tax_credits.as_mut().unwrap().ctc_severely_disabled_child_element += 500.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).child_tax_credit;
        assert!(reformed > base, "Increasing CTC severely disabled child element should increase CTC");
    }

    #[test]
    fn param_tc_income_threshold() {
        let (mut params, p, child, bu, hh) = legacy_tc_setup();
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).child_tax_credit;
        params.tax_credits.as_mut().unwrap().income_threshold += 2000.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).child_tax_credit;
        assert!(reformed > base, "Increasing TC income threshold should reduce taper, increasing CTC");
    }

    #[test]
    fn param_tc_taper_rate() {
        let (mut params, p, child, bu, hh) = legacy_tc_setup();
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).child_tax_credit;
        params.tax_credits.as_mut().unwrap().taper_rate += 0.05;
        let reformed = calc(&params, &[p, child], &bu, &hh).child_tax_credit;
        assert!(reformed < base, "Increasing TC taper rate should reduce CTC for earner");
    }

    #[test]
    fn param_tc_wtc_min_hours_single() {
        let (mut params, mut p, child, bu, hh) = legacy_tc_setup();
        p.hours_worked = 28.0 * 52.0; // Works 28h — below current 30h threshold
        params.tax_credits.as_mut().unwrap().wtc_min_hours_single = 30.0;
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).working_tax_credit;
        params.tax_credits.as_mut().unwrap().wtc_min_hours_single = 25.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).working_tax_credit;
        assert!(reformed > base, "Reducing min hours threshold should enable WTC for 28h worker");
    }

    #[test]
    fn param_tc_wtc_min_hours_couple() {
        let (mut params, _, _, _, hh) = legacy_tc_setup();
        let mut p1 = Person::default(); p1.age = 30.0; p1.employment_income = 8000.0; p1.hours_worked = 22.0 * 52.0;
        p1.working_tax_credit = 1000.0; // reported receipt → legacy TC
        let mut p2 = Person::default(); p2.id = 1; p2.age = 28.0;
        let mut child = Person::default(); child.id = 2; child.age = 5.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1, 2],
            ..BenUnit::default() };
        params.tax_credits.as_mut().unwrap().wtc_min_hours_couple = 24.0;
        let base = calc(&params, &[p1.clone(), p2.clone(), child.clone()], &bu, &hh).working_tax_credit;
        params.tax_credits.as_mut().unwrap().wtc_min_hours_couple = 20.0;
        let reformed = calc(&params, &[p1, p2, child], &bu, &hh).working_tax_credit;
        assert!(reformed > base, "Reducing min hours couple threshold should enable WTC");
    }

    // ── Benefit Cap parameters ────────────────────────────────────────────────

    #[test]
    fn param_benefit_cap_non_single_london() {
        let (mut params, _, _, _) = base_person_uc();
        let mut p = Person::default(); p.age = 30.0;
        let mut c1 = Person::default(); c1.id = 1; c1.age = 3.0;
        let mut c2 = Person::default(); c2.id = 2; c2.age = 5.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1, 2],
            on_uc: true, rent_monthly: 2000.0, full_take_up: true, is_lone_parent: true, ..BenUnit::default() };
        let hh = Household { id: 0, benunit_ids: vec![0], person_ids: vec![0, 1, 2],
            weight: 1.0, region: Region::London, rent: 24000.0, council_tax: 0.0, ..Household::default() };
        let base = calc(&params, &[p.clone(), c1.clone(), c2.clone()], &bu, &hh).benefit_cap_reduction;
        params.benefit_cap.as_mut().unwrap().non_single_london += 2000.0;
        let reformed = calc(&params, &[p, c1, c2], &bu, &hh).benefit_cap_reduction;
        assert!(reformed < base, "Raising benefit cap (London family) should reduce cap reduction");
    }

    #[test]
    fn param_benefit_cap_non_single_outside_london() {
        let (mut params, _, _, _) = base_person_uc();
        let mut p = Person::default(); p.age = 30.0;
        let mut c1 = Person::default(); c1.id = 1; c1.age = 3.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: true, rent_monthly: 1500.0, full_take_up: true, is_lone_parent: true, ..BenUnit::default() };
        let hh = Household { id: 0, benunit_ids: vec![0], person_ids: vec![0, 1],
            weight: 1.0, region: Region::NorthEast, rent: 18000.0, council_tax: 0.0, ..Household::default() };
        let base = calc(&params, &[p.clone(), c1.clone()], &bu, &hh).benefit_cap_reduction;
        params.benefit_cap.as_mut().unwrap().non_single_outside_london += 2000.0;
        let reformed = calc(&params, &[p, c1], &bu, &hh).benefit_cap_reduction;
        assert!(reformed < base, "Raising benefit cap (outside London family) should reduce cap reduction");
    }

    #[test]
    fn param_benefit_cap_single_london() {
        let (mut params, _, _, _) = base_person_uc();
        let mut p = Person::default(); p.age = 30.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0],
            on_uc: true, rent_monthly: 1500.0, full_take_up: true, ..BenUnit::default() };
        let hh = Household { id: 0, benunit_ids: vec![0], person_ids: vec![0],
            weight: 1.0, region: Region::London, rent: 18000.0, council_tax: 0.0, ..Household::default() };
        let base = calc(&params, &[p.clone()], &bu, &hh).benefit_cap_reduction;
        params.benefit_cap.as_mut().unwrap().single_london += 2000.0;
        let reformed = calc(&params, &[p], &bu, &hh).benefit_cap_reduction;
        assert!(reformed < base, "Raising benefit cap (single London) should reduce cap reduction");
    }

    #[test]
    fn param_benefit_cap_single_outside_london() {
        let (mut params, _, _, _) = base_person_uc();
        let mut p = Person::default(); p.age = 30.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0],
            on_uc: true, rent_monthly: 1200.0, full_take_up: true, ..BenUnit::default() };
        let hh = Household { id: 0, benunit_ids: vec![0], person_ids: vec![0],
            weight: 1.0, region: Region::NorthEast, rent: 14400.0, council_tax: 0.0, ..Household::default() };
        let base = calc(&params, &[p.clone()], &bu, &hh).benefit_cap_reduction;
        params.benefit_cap.as_mut().unwrap().single_outside_london += 2000.0;
        let reformed = calc(&params, &[p], &bu, &hh).benefit_cap_reduction;
        assert!(reformed < base, "Raising benefit cap (single outside London) should reduce cap reduction");
    }

    #[test]
    fn param_benefit_cap_earnings_exemption_threshold() {
        let (mut params, _, _, _) = base_person_uc();
        let mut p = Person::default(); p.age = 30.0; p.employment_income = 7500.0;
        let mut c1 = Person::default(); c1.id = 1; c1.age = 3.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: true, rent_monthly: 1500.0, full_take_up: true, is_lone_parent: true, ..BenUnit::default() };
        let hh = Household { id: 0, benunit_ids: vec![0], person_ids: vec![0, 1],
            weight: 1.0, region: Region::London, rent: 18000.0, council_tax: 0.0, ..Household::default() };
        // At £7,500 earnings, below the exemption threshold → cap applies
        params.benefit_cap.as_mut().unwrap().earnings_exemption_threshold = 10000.0;
        let base = calc(&params, &[p.clone(), c1.clone()], &bu, &hh).benefit_cap_reduction;
        params.benefit_cap.as_mut().unwrap().earnings_exemption_threshold = 6000.0;
        let reformed = calc(&params, &[p, c1], &bu, &hh).benefit_cap_reduction;
        // Lowering threshold means £7,500 earner NOW exceeds threshold → exempt
        assert!(reformed < base, "Lowering earnings exemption threshold should exempt higher earner from cap");
    }

    // ── Scottish Child Payment ────────────────────────────────────────────────

    #[test]
    fn param_scp_weekly_amount() {
        let (mut params, p, _, _) = base_person_uc();
        let mut child = Person::default(); child.id = 1; child.age = 5.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: true, full_take_up: true, is_lone_parent: true, ..BenUnit::default() };
        let hh = Household { id: 0, benunit_ids: vec![0], person_ids: vec![0, 1],
            weight: 1.0, region: Region::Scotland, rent: 0.0, council_tax: 0.0, ..Household::default() };
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).scottish_child_payment;
        params.scottish_child_payment.as_mut().unwrap().weekly_amount += 5.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).scottish_child_payment;
        assert!(reformed > base, "Increasing SCP weekly amount should increase SCP");
    }

    #[test]
    fn param_scp_max_age() {
        let (mut params, p, _, _) = base_person_uc();
        let mut child = Person::default(); child.id = 1; child.age = 15.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1],
            on_uc: true, full_take_up: true, is_lone_parent: true, ..BenUnit::default() };
        let hh = Household { id: 0, benunit_ids: vec![0], person_ids: vec![0, 1],
            weight: 1.0, region: Region::Scotland, rent: 0.0, council_tax: 0.0, ..Household::default() };
        params.scottish_child_payment.as_mut().unwrap().max_age = 14.0;
        let base = calc(&params, &[p.clone(), child.clone()], &bu, &hh).scottish_child_payment;
        params.scottish_child_payment.as_mut().unwrap().max_age = 16.0;
        let reformed = calc(&params, &[p, child], &bu, &hh).scottish_child_payment;
        assert!(reformed > base, "Raising SCP max age should include 15-year-old");
    }

    // ── LHA bedroom entitlement tests ────────────────────────────────────────

    #[test]
    fn lha_bedroom_single_adult() {
        // Single adult, no children → 1 bedroom (Category B)
        let p = Person::default();
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0], ..BenUnit::default() };
        let hh = Household { id: 0, benunit_ids: vec![0], person_ids: vec![0], ..Household::default() };
        assert_eq!(lha_bedroom_entitlement(&bu, &[p], &hh), 1);
    }

    #[test]
    fn lha_bedroom_couple_no_children() {
        // Couple, no children → 1 bedroom
        let mut p1 = Person::default(); p1.id = 0; p1.age = 30.0;
        let mut p2 = Person::default(); p2.id = 1; p2.age = 28.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1], ..BenUnit::default() };
        let hh = Household { id: 0, benunit_ids: vec![0], person_ids: vec![0, 1], ..Household::default() };
        assert_eq!(lha_bedroom_entitlement(&bu, &[p1, p2], &hh), 1);
    }

    #[test]
    fn lha_bedroom_two_same_sex_children_under_10() {
        // Single adult + 2 boys under 10 → 1 (adults) + 1 (2 boys share) = 2 bedrooms
        let mut p = Person::default(); p.id = 0; p.age = 30.0;
        let mut c1 = Person::default(); c1.id = 1; c1.age = 7.0; c1.gender = Gender::Male;
        let mut c2 = Person::default(); c2.id = 2; c2.age = 5.0; c2.gender = Gender::Male;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1, 2], ..BenUnit::default() };
        let hh = Household { id: 0, benunit_ids: vec![0], person_ids: vec![0, 1, 2], ..Household::default() };
        assert_eq!(lha_bedroom_entitlement(&bu, &[p, c1, c2], &hh), 2);
    }

    #[test]
    fn lha_bedroom_boy_over_10_and_girl_over_10() {
        // Single adult + boy 12 + girl 13 → can't share (opposite sex, both over 10)
        // → 1 (adult) + 1 (boy) + 1 (girl) = 3 bedrooms
        let mut p = Person::default(); p.id = 0; p.age = 35.0;
        let mut c1 = Person::default(); c1.id = 1; c1.age = 12.0; c1.gender = Gender::Male;
        let mut c2 = Person::default(); c2.id = 2; c2.age = 13.0; c2.gender = Gender::Female;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0, 1, 2], ..BenUnit::default() };
        let hh = Household { id: 0, benunit_ids: vec![0], person_ids: vec![0, 1, 2], ..Household::default() };
        assert_eq!(lha_bedroom_entitlement(&bu, &[p, c1, c2], &hh), 3);
    }

    #[test]
    fn lha_cap_applied_for_private_renter() {
        // Private renter with rent above LHA cap should have UC housing element capped.
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default(); p.age = 30.0; p.employment_income = 0.0;

        // London 1-bed LHA cap = £1,200.81/month. Set rent to £2,000/month.
        let bu = BenUnit {
            id: 0, household_id: 0, person_ids: vec![0],
            on_uc: true,
            rent_monthly: 2000.0, full_take_up: true,
            ..BenUnit::default()
        };
        let hh = Household {
            id: 0, benunit_ids: vec![0], person_ids: vec![0],
            weight: 1.0, region: Region::London,
            tenure_type: TenureType::RentPrivately,
            rent: 24000.0, council_tax: 0.0,
            ..Household::default()
        };
        let pr: Vec<PersonResult> = vec![crate::variables::income_tax::calculate(&p, &params, 0.0)];
        let result = calculate_benunit(&bu, &[p.clone()], &pr, &hh, &params, 2025);

        // UC housing element should be capped at 1-bed London LHA rate (£1,200.81/month)
        // uc_max_amount includes all elements; housing element monthly = 1200.81, annual = 14409.72
        // Full rent would give housing element of 2000*12 = 24000. Check it's below that.
        assert!(
            result.uc_max_amount < 2000.0 * 12.0 + 6000.0, // less than full rent + standard allowance
            "UC max amount should be capped by LHA, not at full rent: {}",
            result.uc_max_amount
        );

        // Without LHA (social housing tenure), full rent used
        let hh_social = Household {
            tenure_type: TenureType::RentFromCouncil,
            ..hh.clone()
        };
        let result_social = calculate_benunit(&bu, &[p], &pr, &hh_social, &params, 2025);
        assert!(
            result_social.uc_max_amount > result.uc_max_amount,
            "Social renter should get higher UC housing element (no LHA cap) vs private renter above cap"
        );
    }

    #[test]
    fn lha_hb_capped_for_private_renter() {
        // HB legacy: private renter with rent above LHA should be capped.
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default(); p.age = 35.0; p.employment_income = 0.0;
        p.housing_benefit = 1000.0; // reported receipt → legacy HB

        let bu = BenUnit {
            id: 0, household_id: 0, person_ids: vec![0],
            rent_monthly: 2500.0,
            ..BenUnit::default()
        };
        let hh_private = Household {
            id: 0, benunit_ids: vec![0], person_ids: vec![0],
            weight: 1.0, region: Region::London,
            tenure_type: TenureType::RentPrivately,
            rent: 30000.0, council_tax: 0.0,
            ..Household::default()
        };
        let hh_social = Household { tenure_type: TenureType::RentFromCouncil, ..hh_private.clone() };

        let pr: Vec<PersonResult> = vec![crate::variables::income_tax::calculate(&p, &params, 0.0)];
        let hb_private = calculate_benunit(&bu, &[p.clone()], &pr, &hh_private, &params, 2025).housing_benefit;
        let hb_social  = calculate_benunit(&bu, &[p], &pr, &hh_social,  &params, 2025).housing_benefit;

        assert!(hb_private > 0.0, "Private renter should still get some HB");
        assert!(hb_social > hb_private, "Social renter (no cap) should get more HB than private renter above cap");
        // HB for private renter at £2500/month rent in London should be capped at 1-bed LHA £1200.81/month
        assert!(hb_private <= 1200.81 * 12.0 + 1.0, "HB should not exceed LHA cap for private renter");
    }

    // ── DLA amount-from-flags ─────────────────────────────────────────────────

    #[test]
    fn dla_care_high_from_flag() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 12.0;
        p.dla_care_high = true;
        // £110.40 × 52 = £5,740.80
        assert!((dla_care_amount(&p, &params) - 5_740.80).abs() < 0.01);
    }

    #[test]
    fn dla_care_mid_from_flag() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 12.0;
        p.dla_care_mid = true;
        assert!((dla_care_amount(&p, &params) - 3_842.80).abs() < 0.01);
    }

    #[test]
    fn dla_mobility_high_from_flag() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 12.0;
        p.dla_mob_high = true;
        // £77.05 × 52 = £4,006.60
        assert!((dla_mobility_amount(&p, &params) - 4_006.60).abs() < 0.01);
    }

    #[test]
    fn dla_recorded_amount_overrides_flag() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.dla_care_high = true;
        p.dla_care = 4_000.0;
        assert_eq!(dla_care_amount(&p, &params), 4_000.0);
    }

    #[test]
    fn dla_returns_zero_when_no_flag() {
        let params = Parameters::for_year(2025).unwrap();
        let p = Person::default();
        assert_eq!(dla_care_amount(&p, &params), 0.0);
        assert_eq!(dla_mobility_amount(&p, &params), 0.0);
    }

    // ── AA amount-from-flags ──────────────────────────────────────────────────

    #[test]
    fn aa_high_from_flag() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 70.0;
        p.aa_high = true;
        assert!((attendance_allowance_amount(&p, &params) - 5_740.80).abs() < 0.01);
    }

    #[test]
    fn aa_low_from_flag() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 70.0;
        p.aa_low = true;
        assert!((attendance_allowance_amount(&p, &params) - 3_842.80).abs() < 0.01);
    }

    #[test]
    fn aa_recorded_amount_overrides_flag() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.aa_high = true;
        p.attendance_allowance = 3_000.0;
        assert_eq!(attendance_allowance_amount(&p, &params), 3_000.0);
    }

    #[test]
    fn aa_returns_zero_when_no_flag() {
        let params = Parameters::for_year(2025).unwrap();
        let p = Person::default();
        assert_eq!(attendance_allowance_amount(&p, &params), 0.0);
    }

    #[test]
    fn dla_aa_flow_into_passthrough_benefits() {
        // A child on DLA care high + mobility high should see both flow into
        // total_benefits, in parallel to the PIP test.
        let (params, mut p, bu, hh) = base_person_uc();
        p.age = 10.0;
        p.dla_care_high = true;
        p.dla_mob_high = true;
        let result = calc(&params, &[p], &bu, &hh);
        // £5,740.80 + £4,006.60 = £9,747.40
        assert!(result.passthrough_benefits >= 9_747.40 - 0.01,
                "passthrough_benefits = {}, expected at least 9747.40",
                result.passthrough_benefits);
    }

    #[test]
    fn dla_param_change_flows_through() {
        // Reform: doubling the DLA care high rate should double the synthetic
        // household's DLA care amount.
        let (mut params, mut p, bu, hh) = base_person_uc();
        p.age = 10.0;
        p.dla_care_high = true;
        let baseline = calc(&params, &[p.clone()], &bu, &hh).passthrough_benefits;
        if let Some(dla) = params.dla.as_mut() {
            dla.care_high_weekly *= 2.0;
        }
        let reformed = calc(&params, &[p], &bu, &hh).passthrough_benefits;
        // Reform should add another £5,740.80 of DLA care high.
        assert!((reformed - baseline - 5_740.80).abs() < 0.01,
                "baseline={}, reformed={}, delta={}", baseline, reformed, reformed - baseline);
    }

    // ── PIP amount-from-flags ─────────────────────────────────────────────────

    #[test]
    fn pip_dl_enhanced_from_flag_when_amount_zero() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 35.0;
        p.pip_dl_enh = true;
        // 2025/26 PIP DL enhanced: £110.40/week × 52 = £5,740.80
        let amount = pip_daily_living_amount(&p, &params);
        assert!((amount - 5_740.80).abs() < 0.01, "got {}", amount);
    }

    #[test]
    fn pip_dl_standard_from_flag_when_amount_zero() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 35.0;
        p.pip_dl_std = true;
        // £73.90 × 52 = £3,842.80
        assert!((pip_daily_living_amount(&p, &params) - 3_842.80).abs() < 0.01);
    }

    #[test]
    fn pip_mob_enhanced_from_flag() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 35.0;
        p.pip_mob_enh = true;
        // £77.05 × 52 = £4,006.60
        assert!((pip_mobility_amount(&p, &params) - 4_006.60).abs() < 0.01);
    }

    #[test]
    fn pip_recorded_amount_overrides_flag() {
        // FRS data: amount may differ from full annual rate (partial year, etc.).
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.age = 35.0;
        p.pip_dl_enh = true;
        p.pip_daily_living = 4_000.0;  // recorded — should pass through unchanged
        assert_eq!(pip_daily_living_amount(&p, &params), 4_000.0);
    }

    #[test]
    fn pip_no_flag_no_recorded_returns_zero() {
        let params = Parameters::for_year(2025).unwrap();
        let p = Person::default();
        assert_eq!(pip_daily_living_amount(&p, &params), 0.0);
        assert_eq!(pip_mobility_amount(&p, &params), 0.0);
    }

    #[test]
    fn pip_returns_zero_when_params_missing() {
        let mut params = Parameters::for_year(2025).unwrap();
        params.pip = None;
        let mut p = Person::default();
        p.pip_dl_enh = true;
        assert_eq!(pip_daily_living_amount(&p, &params), 0.0);
    }

    #[test]
    fn pip_flows_into_passthrough_benefits() {
        // A synthetic household with PIP enhanced flag should see the benefit
        // amount appear in `total_benefits`.
        let (params, mut p, bu, hh) = base_person_uc();
        p.pip_dl_enh = true;
        p.pip_mob_std = true;
        let result = calc(&params, &[p], &bu, &hh);
        // Passthrough = DL enhanced (£5740.80) + Mob standard (£1518.40) = £7259.20
        let expected_passthrough = 5_740.80 + 1_518.40;
        assert!(result.passthrough_benefits >= expected_passthrough - 0.01,
                "passthrough_benefits = {}, expected at least {}",
                result.passthrough_benefits, expected_passthrough);
    }

    #[test]
    fn pip_param_change_flows_through() {
        // Reform: doubling the DL enhanced rate should double the synthetic
        // household's PIP DL amount.
        let (mut params, mut p, bu, hh) = base_person_uc();
        p.pip_dl_enh = true;
        let baseline = calc(&params, &[p.clone()], &bu, &hh).passthrough_benefits;
        if let Some(pip) = params.pip.as_mut() {
            pip.daily_living_enhanced_weekly *= 2.0;
        }
        let reformed = calc(&params, &[p], &bu, &hh).passthrough_benefits;
        // Reform should add another £5,740.80 of PIP DL enhanced.
        assert!((reformed - baseline - 5_740.80).abs() < 0.01,
                "baseline={}, reformed={}, delta={}", baseline, reformed, reformed - baseline);
    }
}
