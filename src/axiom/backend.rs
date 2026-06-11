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

/// Compiled statute programs with pe-uk-rust parameters applied.
pub struct Backend {
    class_1: Policy,
    class_4: Policy,
    child_benefit: Policy,
    pension_credit: Policy,
    fiscal_year: u32,
}

impl Backend {
    pub fn new(
        ni: &NicsParameters,
        cb: &ChildBenefitParameters,
        pc: &PensionCreditParameters,
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

        Ok(Backend { class_1, class_4, child_benefit, pension_credit, fiscal_year })
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
        let class_1: Vec<f64> = calculate(&self.class_1, &dataset, &["primary_class_1_contribution"])?
            .column("primary_class_1_contribution")?
            .iter()
            .map(|v| v * WEEKS_PER_YEAR)
            .collect();

        let profits: Vec<f64> =
            self_employment_income.iter().map(|p| p.max(0.0)).collect();
        let dataset = Dataset::tax_year(self.fiscal_year as i32)
            .with_input("profits_chargeable_to_class_4_contributions", &profits)?;
        let class_4 = calculate(&self.class_4, &dataset, &["class_4_contribution_before_annual_maximum"])?
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
        Ok(calculate(&self.child_benefit, &dataset, &["child_benefit_weekly_entitlement"])?
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
        Ok(calculate(&self.pension_credit, &dataset, &["guarantee_credit"])?
            .column("guarantee_credit")?
            .iter()
            .map(|v| v * WEEKS_PER_YEAR)
            .collect())
    }
}
