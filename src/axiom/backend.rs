//! Statute-derived programs via the axiom rules engine, behind the standard
//! pe-uk-rust surface: the familiar `Parameters` values are translated onto
//! the underlying legal parameters (annual thresholds to the SSCR 2001
//! reg 10 weekly amounts, weekly benefit rates as-is), annual microdata onto
//! the statutory periods, and the statute outputs back onto annual amounts.
//! Because the arithmetic is linear, results match the hand-coded annual
//! formulas exactly.

use anyhow::Result;
use chrono::NaiveDate;

use super::{calculate, Dataset, Policy};

const CLASS_1: &str = include_str!("artifacts/uk-nics-class-1-fy2026.json");
const CLASS_4: &str = include_str!("artifacts/uk-nics-class-4-fy2026.json");
const CHILD_BENEFIT: &str = include_str!("artifacts/uk-child-benefit-fy2026.json");
const PENSION_CREDIT: &str = include_str!("artifacts/uk-pension-credit-fy2026.json");
const UNIVERSAL_CREDIT: &str = include_str!("artifacts/uk-universal-credit-fy2026.json");

const WEEKS_PER_YEAR: f64 = 52.0;

const CB_CHILDREN: &str =
    "child_benefit_children_or_qualifying_young_persons_for_whom_person_responsible";

/// The pe-uk-rust National Insurance parameters the axiom programs accept.
pub struct NicsParameters {
    pub main_rate: f64,
    pub additional_rate: f64,
    pub primary_threshold_annual: f64,
    pub upper_earnings_limit_annual: f64,
    pub class4_main_rate: f64,
    pub class4_additional_rate: f64,
    pub class4_lower_profits_limit: f64,
    pub class4_upper_profits_limit: f64,
}

/// The pe-uk-rust child benefit weekly rates.
pub struct ChildBenefitParameters {
    pub eldest_weekly: f64,
    pub additional_weekly: f64,
}

/// The pe-uk-rust pension credit weekly standard minimum guarantees.
pub struct PensionCreditParameters {
    pub minimum_guarantee_single_weekly: f64,
    pub minimum_guarantee_couple_weekly: f64,
}

/// The pe-uk-rust universal credit monthly amounts and taper.
pub struct UniversalCreditParameters {
    pub standard_allowance_single_under25: f64,
    pub standard_allowance_single_over25: f64,
    pub standard_allowance_couple_under25: f64,
    pub standard_allowance_couple_over25: f64,
    pub child_element_first: f64,
    pub child_element_subsequent: f64,
    pub disabled_child_lower: f64,
    pub disabled_child_higher: f64,
    pub lcwra_element: f64,
    pub carer_element: f64,
    pub taper_rate: f64,
    pub work_allowance_higher: f64,
    pub work_allowance_lower: f64,
    pub child_limit: usize,
}

/// Per-benefit-unit UC inputs (annual money amounts except the monthly
/// rents), plus per-child disability flags in benunit-then-child order.
pub struct UcCaseload<'a> {
    pub joint: &'a [bool],
    pub eldest_adult_25_or_over: &'a [bool],
    pub net_earned_income_annual: &'a [f64],
    pub unearned_income_annual: &'a [f64],
    pub has_lcwra: &'a [bool],
    pub has_carer: &'a [bool],
    pub rent_monthly: &'a [f64],
    pub rent_cap_monthly: &'a [f64],
    pub num_children: &'a [usize],
    pub child_disabled_higher: &'a [bool],
    pub child_disabled_lower: &'a [bool],
}

/// Compiled statute programs with pe-uk-rust parameters applied.
pub struct Backend {
    class_1: Policy,
    class_4: Policy,
    child_benefit: Policy,
    pension_credit: Policy,
    universal_credit: Policy,
    uc_lcwra_element: f64,
    uc_carer_element: f64,
    uc_child_limit: usize,
    fiscal_year: u32,
}

impl Backend {
    pub fn new(
        ni: &NicsParameters,
        cb: &ChildBenefitParameters,
        pc: &PensionCreditParameters,
        uc: &UniversalCreditParameters,
        fiscal_year: u32,
    ) -> Result<Self> {
        let from = NaiveDate::from_ymd_opt(fiscal_year as i32, 4, 6).expect("valid tax year start");

        let class_1 = Policy::from_artifact_json(CLASS_1, "Person")?
            .with_parameter("uk:statutes/ukpga/1992/4/8#main_primary_percentage", from, ni.main_rate)?
            .with_parameter(
                "uk:statutes/ukpga/1992/4/8#additional_primary_percentage",
                from,
                ni.additional_rate,
            )?
            .with_parameter(
                "uk:regulations/uksi/2001/1004/10#primary_threshold",
                from,
                ni.primary_threshold_annual / WEEKS_PER_YEAR,
            )?
            .with_parameter(
                "uk:regulations/uksi/2001/1004/10#upper_earnings_limit",
                from,
                ni.upper_earnings_limit_annual / WEEKS_PER_YEAR,
            )?;

        let class_4 = Policy::from_artifact_json(CLASS_4, "Person")?
            .with_parameter("uk:statutes/ukpga/1992/4/15#main_class_4_percentage", from, ni.class4_main_rate)?
            .with_parameter(
                "uk:statutes/ukpga/1992/4/15#additional_class_4_percentage",
                from,
                ni.class4_additional_rate,
            )?
            .with_parameter(
                "uk:statutes/ukpga/1992/4/15#lower_profits_limit",
                from,
                ni.class4_lower_profits_limit,
            )?
            .with_parameter(
                "uk:statutes/ukpga/1992/4/15#upper_profits_limit",
                from,
                ni.class4_upper_profits_limit,
            )?;

        let child_benefit = Policy::from_artifact_json(CHILD_BENEFIT, "Family")?
            .with_parameter(
                "uk:regulations/uksi/2006/965/2#child_benefit_enhanced_weekly_rate",
                from,
                cb.eldest_weekly,
            )?
            .with_parameter(
                "uk:regulations/uksi/2006/965/2#child_benefit_other_weekly_rate",
                from,
                cb.additional_weekly,
            )?;

        let pension_credit = Policy::from_artifact_json(PENSION_CREDIT, "Person")?
            .with_parameter(
                "uk:regulations/uksi/2002/1792/6#standard_minimum_guarantee_no_partner",
                from,
                pc.minimum_guarantee_single_weekly,
            )?
            .with_parameter(
                "uk:regulations/uksi/2002/1792/6#standard_minimum_guarantee_with_partner",
                from,
                pc.minimum_guarantee_couple_weekly,
            )?;

        // UC runs over the April assessment period, so the patched values
        // must be in force by 1 April rather than the 6 April tax-year start.
        let from_month = NaiveDate::from_ymd_opt(fiscal_year as i32, 4, 1).expect("valid month start");
        let universal_credit = Policy::from_artifact_json(UNIVERSAL_CREDIT, "Family")?
            .with_parameter("uk:regulations/uksi/2013/376/22#earned_income_taper_rate", from_month, uc.taper_rate)?
            .with_parameter(
                "uk:regulations/uksi/2013/376/22#higher_work_allowance_single_claimant_amount",
                from_month,
                uc.work_allowance_higher,
            )?
            .with_parameter(
                "uk:regulations/uksi/2013/376/22#higher_work_allowance_joint_claimants_amount",
                from_month,
                uc.work_allowance_higher,
            )?
            .with_parameter(
                "uk:regulations/uksi/2013/376/22#lower_work_allowance_single_claimant_amount",
                from_month,
                uc.work_allowance_lower,
            )?
            .with_parameter(
                "uk:regulations/uksi/2013/376/22#lower_work_allowance_joint_claimants_amount",
                from_month,
                uc.work_allowance_lower,
            )?
            .with_parameter(
                "uk:regulations/uksi/2013/376/36#standard_allowance_single_under_25_amount",
                from_month,
                uc.standard_allowance_single_under25,
            )?
            .with_parameter(
                "uk:regulations/uksi/2013/376/36#standard_allowance_single_25_or_over_amount",
                from_month,
                uc.standard_allowance_single_over25,
            )?
            .with_parameter(
                "uk:regulations/uksi/2013/376/36#standard_allowance_joint_both_under_25_amount",
                from_month,
                uc.standard_allowance_couple_under25,
            )?
            .with_parameter(
                "uk:regulations/uksi/2013/376/36#standard_allowance_joint_either_25_or_over_amount",
                from_month,
                uc.standard_allowance_couple_over25,
            )?
            .with_parameter(
                "uk:regulations/uksi/2013/376/36#first_child_element_amount",
                from_month,
                uc.child_element_first,
            )?
            .with_parameter(
                "uk:regulations/uksi/2013/376/36#second_and_subsequent_child_element_amount",
                from_month,
                uc.child_element_subsequent,
            )?
            .with_parameter(
                "uk:regulations/uksi/2013/376/36#disabled_child_lower_rate_amount",
                from_month,
                uc.disabled_child_lower,
            )?
            .with_parameter(
                "uk:regulations/uksi/2013/376/36#disabled_child_higher_rate_amount",
                from_month,
                uc.disabled_child_higher,
            )?;

        Ok(Backend {
            class_1,
            class_4,
            child_benefit,
            pension_credit,
            universal_credit,
            uc_lcwra_element: uc.lcwra_element,
            uc_carer_element: uc.carer_element,
            uc_child_limit: uc.child_limit,
            fiscal_year,
        })
    }

    fn week_start(&self) -> NaiveDate {
        NaiveDate::from_ymd_opt(self.fiscal_year as i32, 4, 6).expect("valid tax year start")
    }

    /// Annual Class 1 primary and Class 4 contributions per person, from
    /// annual employment income and self-employment profits.
    pub fn national_insurance(
        &self,
        employment_income: &[f64],
        self_employment_income: &[f64],
    ) -> Result<(Vec<f64>, Vec<f64>)> {
        let week_start = self.week_start();
        let weekly_earnings: Vec<f64> =
            employment_income.iter().map(|e| e / WEEKS_PER_YEAR).collect();
        let dataset = Dataset::week(week_start)
            .with_input("earnings_paid_in_tax_week_in_respect_of_employment", &weekly_earnings)?;
        let class_1: Vec<f64> = calculate(&self.class_1, dataset, &["primary_class_1_contribution"])?
            .column("primary_class_1_contribution")?
            .iter()
            .map(|v| v * WEEKS_PER_YEAR)
            .collect();

        let profits: Vec<f64> =
            self_employment_income.iter().map(|p| p.max(0.0)).collect();
        let dataset = Dataset::tax_year(self.fiscal_year as i32)
            .with_input("profits_chargeable_to_class_4_contributions", &profits)?;
        let class_4 = calculate(&self.class_4, dataset, &["class_4_contribution_before_annual_maximum"])?
            .column("class_4_contribution_before_annual_maximum")?
            .to_vec();

        Ok((class_1, class_4))
    }

    /// Annual child benefit per family, from each family's number of
    /// children (the first child is the only/eldest, paid the enhanced rate).
    pub fn child_benefit(&self, num_children: &[usize]) -> Result<Vec<f64>> {
        let total: usize = num_children.iter().sum();
        let mut eldest = Vec::with_capacity(total);
        for &c in num_children {
            for j in 0..c {
                eldest.push(j == 0);
            }
        }
        let dataset = Dataset::week(self.week_start())
            .with_relation(CB_CHILDREN, num_children)?
            .with_relation_bool_input(
                CB_CHILDREN,
                "child_or_qualifying_young_person_is_only_elder_or_eldest_for_payee",
                &eldest,
            )?
            .with_relation_bool_input(
                CB_CHILDREN,
                "child_or_qualifying_young_person_is_elder_or_eldest_among_paragraph_2_children",
                &vec![false; total],
            )?
            .with_relation_bool_input(
                CB_CHILDREN,
                "is_child_or_qualifying_young_person_for_child_benefit",
                &vec![true; total],
            )?;
        Ok(calculate(&self.child_benefit, dataset, &["child_benefit_weekly_entitlement"])?
            .column("child_benefit_weekly_entitlement")?
            .iter()
            .map(|v| v * WEEKS_PER_YEAR)
            .collect())
    }

    /// Annual pension credit guarantee credit per claimant unit, from annual
    /// qualifying income and couple status. Severe-disability / carer
    /// additions and the SPCA 2002 entitlement conditions modelled elsewhere
    /// (state pension age, claiming behaviour) are not applied here.
    pub fn guarantee_credit(
        &self,
        annual_income: &[f64],
        has_partner: &[bool],
    ) -> Result<Vec<f64>> {
        let weekly_income: Vec<f64> =
            annual_income.iter().map(|x| x / WEEKS_PER_YEAR).collect();
        let f = vec![false; annual_income.len()];
        let dataset = Dataset::week(self.week_start())
            .with_input("claimant_income", &weekly_income)?
            .with_bool_input("claimant_has_partner", has_partner)?
            .with_bool_input(
                "treated_as_severely_disabled_person_under_schedule_i_part_i_paragraph_1",
                &f,
            )?
            .with_bool_input("severe_disability_couple_rate_conditions_satisfied", &f)?
            .with_bool_input("paragraph_4_of_part_ii_of_schedule_i_satisfied_for_this_partner", &f)?
            .with_bool_input("awarded_tax_credit_under_tax_credits_act", &f)?
            .with_bool_input("tax_credit_award_circumstances_in_paragraph_15", &f)?
            .with_bool_input("tax_credit_decision_revised_in_favour_after_paragraph_16_event", &f)?
            .with_bool_input("detained_in_custody_for_more_than_52_weeks", &f)?
            .with_bool_input("detained_pending_trial_or_sentence_following_conviction_by_court", &f)?
            .with_bool_input("detained_for_period_not_exceeding_52_weeks", &f)?
            .with_bool_input("detained_in_custody_on_remand_pending_trial", &f)?
            .with_bool_input("required_as_condition_of_bail_to_reside_in_approved_hostel", &f)?
            .with_bool_input("detained_pending_sentence_upon_conviction", &f)?;
        Ok(calculate(&self.pension_credit, dataset, &["guarantee_credit"])?
            .column("guarantee_credit")?
            .iter()
            .map(|v| v * WEEKS_PER_YEAR)
            .collect())
    }

    /// Annual UC award and maximum amount per benefit unit, from the April
    /// assessment period ×12. Eligibility (working-age) and claiming gates
    /// stay in the model; the two-child limit is applied here via the
    /// per-child element flags. LCWRA and carer element amounts enter as
    /// inputs (statute leaves them to reg 36 determinations).
    pub fn universal_credit(&self, c: &UcCaseload) -> Result<(Vec<f64>, Vec<f64>)> {
        const MONTHS_PER_YEAR: f64 = 12.0;
        let n = c.joint.len();
        let total_children: usize = c.num_children.iter().sum();

        let single: Vec<bool> = c.joint.iter().map(|j| !j).collect();
        let has_children: Vec<bool> = c.num_children.iter().map(|&n| n > 0).collect();
        let earned_monthly: Vec<f64> =
            c.net_earned_income_annual.iter().map(|e| e / MONTHS_PER_YEAR).collect();
        let unearned_monthly: Vec<f64> =
            c.unearned_income_annual.iter().map(|u| u / MONTHS_PER_YEAR).collect();
        let has_housing: Vec<bool> = c.rent_monthly.iter().map(|r| *r > 0.0).collect();

        // Per-child element flags: only the first child_limit children carry
        // the child element (first then subsequent); disability additions are
        // not subject to the limit.
        let mut child_is_first = Vec::with_capacity(total_children);
        let mut child_is_later = Vec::with_capacity(total_children);
        for &count in c.num_children {
            let capped = count.min(self.uc_child_limit);
            for j in 0..count {
                child_is_first.push(j == 0 && capped > 0);
                child_is_later.push(j >= 1 && j < capped);
            }
        }
        let child_trues = vec![true; total_children];

        // One adult row per benunit carries the benunit's carer element.
        let adult_counts = vec![1usize; n];
        let carer_amounts: Vec<f64> = c
            .has_carer
            .iter()
            .map(|&h| if h { self.uc_carer_element } else { 0.0 })
            .collect();
        let adult_falses = vec![false; n];

        let falses = vec![false; n];
        let dataset = Dataset::month(self.fiscal_year as i32, 4)
            // Claim structure.
            .with_bool_input("claim_is_for_joint_claimants", c.joint)?
            .with_bool_input("award_is_for_joint_claimants", c.joint)?
            .with_bool_input("claimant_is_member_of_couple", c.joint)?
            .with_bool_input("claimant_makes_claim_as_single_person", &single)?
            .with_bool_input("either_joint_claimant_is_aged_25_or_over", c.eldest_adult_25_or_over)?
            .with_bool_input("single_claimant_is_aged_25_or_over", c.eldest_adult_25_or_over)?
            .with_bool_input("joint_claimants_responsible_for_child_or_qualifying_young_person", &has_children)?
            .with_bool_input("single_claimant_responsible_for_child_or_qualifying_young_person", &has_children)?
            // Income (net earned and unearned, reg 55 / reg 66 computed in the model).
            .with_input("joint_claimants_combined_earned_income_in_assessment_period", &earned_monthly)?
            .with_input("claimant_earned_income_in_assessment_period", &earned_monthly)?
            .with_input("joint_claimants_combined_unearned_income_in_assessment_period", &unearned_monthly)?
            .with_input("claimant_unearned_income_in_assessment_period", &unearned_monthly)?
            // Limited capability for work (LCWRA proxy) and LCWRA element amounts.
            .with_bool_input("one_or_both_joint_claimants_have_limited_capability_for_work", c.has_lcwra)?
            .with_bool_input("single_claimant_has_limited_capability_for_work", c.has_lcwra)?
            .with_bool_input("first_joint_claimant_has_limited_capability_for_work_and_work_related_activity", c.has_lcwra)?
            .with_bool_input("second_joint_claimant_has_limited_capability_for_work_and_work_related_activity", &falses)?
            .with_bool_input("single_claimant_has_limited_capability_for_work_and_work_related_activity", c.has_lcwra)?
            .with_const_input("lcwra_element_amount_given_in_regulation_36_for_first_joint_claimant", self.uc_lcwra_element)?
            .with_const_input("lcwra_element_amount_given_in_regulation_36_for_second_joint_claimant", 0.0)?
            .with_const_input("lcwra_element_amount_given_in_regulation_36_for_single_claimant", self.uc_lcwra_element)?
            // Carer element delivered via the adult relation, never doubled.
            .with_bool_input("both_joint_claimants_qualify_for_carer_element", &falses)?
            .with_bool_input("joint_claimants_are_caring_for_the_same_severely_disabled_person", &falses)?
            // Housing: renters element is min(core, cap) with no non-dependant
            // contributions, matching rent capped at the LHA rate.
            .with_bool_input("award_contains_housing_costs_element", &has_housing)?
            .with_input("renters_core_rent", c.rent_monthly)?
            .with_input("renters_cap_rent", c.rent_cap_monthly)?
            .with_const_input("housing_cost_contribution_count_required_under_paragraph_13_in_renters_case", 0.0)?
            .with_const_input("amount_resulting_from_all_other_steps_in_parts_4_and_5_calculation", 0.0)?
            // No childcare element in the model.
            .with_const_input("charges_paid_for_relevant_childcare_attributable_to_assessment_period", 0.0)?
            .with_const_input("amount_considered_excessive_having_regard_to_paid_work_extent", 0.0)?
            .with_const_input("amount_met_or_reimbursed_by_employer_or_some_other_person", 0.0)?
            .with_const_input("amount_from_funds_provided_by_secretary_of_state_or_scottish_or_welsh_ministers_for_work_related_activity_or_training", 0.0)?
            .with_const_input("secretary_of_state_work_transition_childcare_payment_amount", 0.0)?
            .with_bool_input("secretary_of_state_work_transition_childcare_payment_meets_non_other_relevant_support_conditions", &falses)?
            .with_const_input("maximum_amount_specified_in_table_in_regulation_36", 0.0)?
            .with_const_input("childcare_costs_element_child_count", 0.0)?
            // Children of each benefit unit.
            .with_relation("child_of_benefit_unit", c.num_children)?
            .with_relation_bool_input("child_of_benefit_unit", "claimant_responsible_for_child_or_qualifying_young_person", &child_trues)?
            .with_relation_bool_input("child_of_benefit_unit", "child_is_first_child_or_qualifying_young_person", &child_is_first)?
            .with_relation_bool_input("child_of_benefit_unit", "child_is_second_or_subsequent_child_or_qualifying_young_person", &child_is_later)?
            .with_relation_bool_input("child_of_benefit_unit", "disabled_child_higher_rate_applies", c.child_disabled_higher)?
            .with_relation_bool_input("child_of_benefit_unit", "disabled_child_lower_rate_applies", c.child_disabled_lower)?
            // Adults of each benefit unit (one row, carrying the carer element).
            .with_relation("adult_of_benefit_unit", &adult_counts)?
            .with_relation_input("adult_of_benefit_unit", "carer_element_amount", &carer_amounts)?
            .with_relation_bool_input("adult_of_benefit_unit", "claim_is_for_joint_claimants", c.joint)?
            .with_relation_bool_input("adult_of_benefit_unit", "claimant_has_regular_and_substantial_caring_responsibilities_for_severely_disabled_person", c.has_carer)?
            .with_relation_bool_input("adult_of_benefit_unit", "claimant_is_the_only_relevant_carer_or_is_elected_or_determined_for_carer_element", c.has_carer)?
            .with_relation_bool_input("adult_of_benefit_unit", "contributions_and_benefits_act_section_70_does_not_displace_carer_element", c.has_carer)?
            .with_relation_bool_input("adult_of_benefit_unit", "claimant_has_limited_capability_for_work_and_work_related_activity", &adult_falses)?
            .with_relation_bool_input("adult_of_benefit_unit", "lcwra_element_included_in_respect_of_other_joint_claimant", &adult_falses)?
            .with_relation_bool_input("adult_of_benefit_unit", "scottish_carer_benefit_coordination_applies_for_same_day_and_same_severely_disabled_person", &adult_falses)?
            .with_relation_bool_input("adult_of_benefit_unit", "claimant_and_other_person_jointly_elect_claimant_has_carer_element_and_other_person_has_no_scottish_carer_benefit_entitlement", &adult_falses)?
            .with_relation_bool_input("adult_of_benefit_unit", "secretary_of_state_after_consulting_scottish_ministers_is_satisfied_claimant_has_carer_element_and_other_person_has_no_scottish_carer_benefit_entitlement", &adult_falses)?
            // No owner-occupier service charge payments.
            .with_relation("owner_occupier_service_charge_payments", &vec![0; n])?
            .with_relation_bool_input("owner_occupier_service_charge_payments", "payment_is_relevant_service_charge_payment_taken_into_account_under_paragraph_8", &[])?
            .with_relation_bool_input("owner_occupier_service_charge_payments", "service_charge_free_period_arrangements_apply_to_payment", &[])?
            .with_relation_bool_input("owner_occupier_service_charge_payments", "service_charge_payment_period_is_month", &[])?
            .with_relation_bool_input("owner_occupier_service_charge_payments", "service_charge_payment_period_is_week", &[])?
            .with_relation_bool_input("owner_occupier_service_charge_payments", "service_charge_payment_period_is_two_weeks", &[])?
            .with_relation_bool_input("owner_occupier_service_charge_payments", "service_charge_payment_period_is_four_weeks", &[])?
            .with_relation_bool_input("owner_occupier_service_charge_payments", "service_charge_payment_period_is_three_months", &[])?
            .with_relation_bool_input("owner_occupier_service_charge_payments", "service_charge_payment_period_is_annual", &[])?
            .with_relation_input("owner_occupier_service_charge_payments", "service_charge_payment_amount", &[])?
            .with_relation_input("owner_occupier_service_charge_payments", "service_charge_free_periods_in_12_month_period", &[])?
            .with_relation_input("owner_occupier_service_charge_payments", "total_service_charge_payments_liable_in_12_month_period", &[])?;

        let outputs = calculate(
            &self.universal_credit,
            dataset,
            &["universal_credit_award_amount", "universal_credit_maximum_amount"],
        )?;
        let award: Vec<f64> = outputs
            .column("universal_credit_award_amount")?
            .iter()
            .map(|v| v * MONTHS_PER_YEAR)
            .collect();
        let max_amount: Vec<f64> = outputs
            .column("universal_credit_maximum_amount")?
            .iter()
            .map(|v| v * MONTHS_PER_YEAR)
            .collect();
        Ok((award, max_amount))
    }
}
