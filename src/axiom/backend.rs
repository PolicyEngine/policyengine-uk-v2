//! National Insurance via the axiom rules engine, behind the standard
//! pe-uk-rust surface: the familiar `Parameters` values are translated onto
//! the underlying legal parameters (annual thresholds to the SSCR 2001
//! reg 10 weekly amounts), annual microdata onto the statutory periods, and
//! the statute outputs back onto annual per-person amounts. Because the
//! band arithmetic is linear, results match the hand-coded annual formulas
//! exactly.

use anyhow::Result;
use chrono::NaiveDate;

use super::{calculate, Dataset, Policy};

const CLASS_1: &str = include_str!("artifacts/uk-nics-class-1-fy2026.json");
const CLASS_4: &str = include_str!("artifacts/uk-nics-class-4-fy2026.json");

const WEEKS_PER_YEAR: f64 = 52.0;

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

/// Compiled NICs programs with pe-uk-rust parameters applied.
pub struct Backend {
    class_1: Policy,
    class_4: Policy,
    fiscal_year: u32,
}

impl Backend {
    pub fn new(ni: &NicsParameters, fiscal_year: u32) -> Result<Self> {
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

        Ok(Backend { class_1, class_4, fiscal_year })
    }

    /// Annual Class 1 primary and Class 4 contributions per person, from
    /// annual employment income and self-employment profits.
    pub fn national_insurance(
        &self,
        employment_income: &[f64],
        self_employment_income: &[f64],
    ) -> Result<(Vec<f64>, Vec<f64>)> {
        let week_start =
            NaiveDate::from_ymd_opt(self.fiscal_year as i32, 4, 6).expect("valid tax year start");
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
}
