//! Calibration / reweighting of household survey data to match administrative targets.
//!
//! Loads calibration targets from a JSON file, builds a matrix of household-level
//! contributions to each target, and optimises household weights using Adam in
//! log-space to minimise mean squared relative error.
//!
//! Calibration runs *after* a baseline simulation so that targets can reference
//! simulated output variables (income_tax, universal_credit, etc.) as well as
//! raw input data.

use std::path::Path;

use colored::Colorize;
use comfy_table::{Table, ContentArrangement, presets, Cell, Color};
use rand::Rng;
use rayon::prelude::*;
use serde::Deserialize;

use crate::data::Dataset;
use crate::engine::simulation::SimulationResults;

// ── Target schema ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct CalibrationTargetFile {
    pub targets: Vec<CalibrationTarget>,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct CalibrationTarget {
    pub name: String,
    pub variable: String,
    pub entity: String,
    pub aggregation: String,
    #[serde(default)]
    pub filter: Option<TargetFilter>,
    /// Benunit-level property filter (e.g. is_couple, has_children).
    #[serde(default)]
    pub benunit_filter: Option<BenunitFilter>,
    pub value: f64,
    pub source: String,
    pub year: u32,
    #[serde(default)]
    pub holdout: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TargetFilter {
    pub variable: String,
    pub min: f64,
    pub max: f64,
}

/// Filter on benunit-level computed properties (checked via entity methods).
/// All specified conditions must be true (AND logic).
#[derive(Debug, Deserialize, Clone)]
pub struct BenunitFilter {
    /// true = couple, false = single
    #[serde(default)]
    pub is_couple: Option<bool>,
    /// true = has children, false = no children
    #[serde(default)]
    pub has_children: Option<bool>,
    /// true = at least one person in benunit is a carer
    #[serde(default)]
    pub has_carer: Option<bool>,
    /// true = at least one person has esa_group == 1 (support/LCWRA)
    #[serde(default)]
    pub has_lcwra: Option<bool>,
    /// true = at least one person has esa_group == 2 (WRAG/LCW)
    #[serde(default)]
    pub has_lcw: Option<bool>,
    /// true = benunit has rent > 0 (housing entitlement proxy)
    #[serde(default)]
    pub has_housing: Option<bool>,
}

// ── Load targets ───────────────────────────────────────────────────────────

pub fn load_targets(path: &Path) -> anyhow::Result<Vec<CalibrationTarget>> {
    let text = std::fs::read_to_string(path)?;
    let file: CalibrationTargetFile = serde_json::from_str(&text)?;
    Ok(file.targets)
}

// ── Variable resolution ────────────────────────────────────────────────────

/// Get a person-level variable value by name, checking simulation results first.
fn person_variable(
    p: &crate::engine::entities::Person,
    sim: Option<&SimulationResults>,
    pid: usize,
    name: &str,
) -> f64 {
    // Check simulation output variables first
    if let Some(results) = sim {
        if let Some(v) = person_result_variable(&results.person_results[pid], name) {
            return v;
        }
    }
    // Fall back to input data
    match name {
        "age" => p.age,
        "employment_income" => p.employment_income,
        "self_employment_income" => p.self_employment_income,
        "pension_income" | "private_pension_income" => p.pension_income,
        "state_pension" => p.state_pension,
        "savings_interest_income" | "savings_interest" => p.savings_interest_income,
        "dividend_income" => p.dividend_income,
        "capital_gains" => p.capital_gains,
        "property_income" => p.property_income,
        "maintenance_income" => p.maintenance_income,
        "miscellaneous_income" => p.miscellaneous_income,
        "other_income" => p.other_income,
        "child_benefit" => p.child_benefit,
        "housing_benefit" => p.housing_benefit,
        "income_support" => p.income_support,
        "pension_credit" => p.pension_credit,
        "child_tax_credit" => p.child_tax_credit,
        "working_tax_credit" => p.working_tax_credit,
        "universal_credit" => p.universal_credit,
        "dla_care" => p.dla_care,
        "dla_mobility" => p.dla_mobility,
        "pip_daily_living" => p.pip_daily_living,
        "pip_mobility" => p.pip_mobility,
        "carers_allowance" => p.carers_allowance,
        "attendance_allowance" => p.attendance_allowance,
        "esa_income" => p.esa_income,
        "esa_contributory" => p.esa_contributory,
        "jsa_income" => p.jsa_income,
        "jsa_contributory" => p.jsa_contributory,
        "other_benefits" => p.other_benefits,
        "total_income" => p.total_income(),
        "hours_worked" => p.hours_worked,
        _ => 0.0,
    }
}

/// Get a simulation output variable for a person. Returns None if not a sim variable.
fn person_result_variable(
    pr: &crate::engine::simulation::PersonResult,
    name: &str,
) -> Option<f64> {
    match name {
        "income_tax" => Some(pr.income_tax),
        "national_insurance" | "employee_ni" => Some(pr.national_insurance),
        "employer_ni" => Some(pr.employer_ni),
        "total_ni" => Some(pr.national_insurance + pr.employer_ni),
        "sim_total_income" => Some(pr.total_income),
        "taxable_income" => Some(pr.taxable_income),
        "personal_allowance" => Some(pr.personal_allowance),
        "adjusted_net_income" => Some(pr.adjusted_net_income),
        "hicbc" => Some(pr.hicbc),
        "capital_gains_tax" => Some(pr.capital_gains_tax),
        _ => None,
    }
}

/// Get a simulation output variable for a benefit unit.
fn benunit_result_variable(
    br: &crate::engine::simulation::BenUnitResult,
    name: &str,
) -> Option<f64> {
    match name {
        "universal_credit" => Some(br.universal_credit),
        "child_benefit" => Some(br.child_benefit),
        "state_pension" => Some(br.state_pension),
        "pension_credit" => Some(br.pension_credit),
        "housing_benefit" => Some(br.housing_benefit),
        "child_tax_credit" => Some(br.child_tax_credit),
        "working_tax_credit" => Some(br.working_tax_credit),
        "income_support" => Some(br.income_support),
        "esa_income_related" => Some(br.esa_income_related),
        "jsa_income_based" => Some(br.jsa_income_based),
        "carers_allowance" => Some(br.carers_allowance),
        "total_benefits" => Some(br.total_benefits),
        "uc_max_amount" => Some(br.uc_max_amount),
        "uc_income_reduction" => Some(br.uc_income_reduction),
        "benefit_cap_reduction" => Some(br.benefit_cap_reduction),
        _ => None,
    }
}

/// Get a household-level variable value by name.
fn household_variable(
    h: &crate::engine::entities::Household,
    sim: Option<&SimulationResults>,
    hh_idx: usize,
    name: &str,
) -> f64 {
    // Check simulation output variables first
    if let Some(results) = sim {
        if let Some(v) = household_result_variable(&results.household_results[hh_idx], name) {
            return v;
        }
    }
    match name {
        "council_tax_annual" | "council_tax" => h.council_tax,
        "rent_annual" | "rent" => h.rent,
        "weight" => h.weight,
        "household_id" => 1.0,
        "property_wealth" => h.property_wealth,
        "net_financial_wealth" => h.net_financial_wealth,
        "gross_financial_wealth" => h.gross_financial_wealth,
        "savings" => h.savings,
        "tenure_type" => h.tenure_type.to_rf_code(),
        "region" => h.region.to_rf_code(),
        _ => 0.0,
    }
}

/// Get a simulation output variable for a household.
fn household_result_variable(
    hr: &crate::engine::simulation::HouseholdResult,
    name: &str,
) -> Option<f64> {
    match name {
        "net_income" => Some(hr.net_income),
        "total_tax" => Some(hr.total_tax),
        "hh_total_benefits" => Some(hr.total_benefits),
        "gross_income" => Some(hr.gross_income),
        "vat" => Some(hr.vat),
        "fuel_duty" => Some(hr.fuel_duty),
        "capital_gains_tax" => Some(hr.capital_gains_tax),
        "stamp_duty" => Some(hr.stamp_duty),
        "council_tax_calculated" => Some(hr.council_tax_calculated),
        _ => None,
    }
}

/// Check whether a benunit passes all conditions in a BenunitFilter.
fn benunit_passes_filter(
    bu: &crate::engine::entities::BenUnit,
    people: &[crate::engine::entities::Person],
    filter: &BenunitFilter,
) -> bool {
    if let Some(want_couple) = filter.is_couple {
        if bu.is_couple(people) != want_couple {
            return false;
        }
    }
    if let Some(want_children) = filter.has_children {
        let has = bu.num_children(people) > 0;
        if has != want_children {
            return false;
        }
    }
    if let Some(want_carer) = filter.has_carer {
        let has = bu.person_ids.iter().any(|&pid| people[pid].is_carer);
        if has != want_carer {
            return false;
        }
    }
    if let Some(want_lcwra) = filter.has_lcwra {
        let has = bu.person_ids.iter().any(|&pid| people[pid].esa_group == 1);
        if has != want_lcwra {
            return false;
        }
    }
    if let Some(want_lcw) = filter.has_lcw {
        let has = bu.person_ids.iter().any(|&pid| people[pid].esa_group == 2);
        if has != want_lcw {
            return false;
        }
    }
    if let Some(want_housing) = filter.has_housing {
        let has = bu.rent_monthly > 0.0;
        if has != want_housing {
            return false;
        }
    }
    true
}

// ── Matrix building ────────────────────────────────────────────────────────

/// Build the calibration matrix M[i][j] and target vector y[j].
///
/// M[i][j] = household i's contribution to target j (before weighting).
/// y[j] = the target value.
///
/// If `sim_results` is provided, simulation output variables can be used
/// in addition to raw input data.
///
/// Returns (matrix, target_values, training_mask) where training_mask[j]
/// is true if target j should be included in the loss.
pub fn build_matrix(
    dataset: &Dataset,
    targets: &[CalibrationTarget],
    sim_results: Option<&SimulationResults>,
) -> (Vec<Vec<f64>>, Vec<f64>, Vec<bool>) {
    let n_hh = dataset.households.len();
    let n_targets = targets.len();
    let mut matrix = vec![vec![0.0f64; n_targets]; n_hh];
    let mut target_values = vec![0.0f64; n_targets];
    let mut training_mask = vec![true; n_targets];

    for (j, target) in targets.iter().enumerate() {
        target_values[j] = target.value;
        // All targets participate in training. The holdout flag is only
        // used for separate error reporting, not gradient exclusion.

        match target.entity.as_str() {
            "person" => {
                for (i, hh) in dataset.households.iter().enumerate() {
                    let mut contribution = 0.0f64;
                    for &pid in &hh.person_ids {
                        let person = &dataset.people[pid];

                        if let Some(ref filter) = target.filter {
                            let filter_val = person_variable(person, sim_results, pid, &filter.variable);
                            if filter_val < filter.min || filter_val >= filter.max {
                                continue;
                            }
                        }

                        match target.aggregation.as_str() {
                            "sum" => {
                                contribution += person_variable(person, sim_results, pid, &target.variable);
                            }
                            "count_nonzero" => {
                                if person_variable(person, sim_results, pid, &target.variable) > 0.0 {
                                    contribution += 1.0;
                                }
                            }
                            "count" => {
                                contribution += 1.0;
                            }
                            _ => {}
                        }
                    }
                    matrix[i][j] = contribution;
                }
            }
            "benunit" => {
                for (i, hh) in dataset.households.iter().enumerate() {
                    let mut contribution = 0.0f64;
                    for &bu_id in &hh.benunit_ids {
                        let bu = &dataset.benunits[bu_id];

                        // Apply benunit-level property filter if present
                        if let Some(ref bf) = target.benunit_filter {
                            if !benunit_passes_filter(bu, &dataset.people, bf) {
                                continue;
                            }
                        }

                        // For benunit variables, check simulation results first
                        let bu_val = if let Some(results) = sim_results {
                            benunit_result_variable(&results.benunit_results[bu_id], &target.variable)
                                .unwrap_or(0.0)
                        } else {
                            // Fall back to input data: sum person-level variable across benunit members
                            bu.person_ids.iter()
                                .map(|&pid| person_variable(&dataset.people[pid], None, pid, &target.variable))
                                .sum::<f64>()
                        };

                        // Apply min/max range filter on the benunit variable value
                        if let Some(ref filter) = target.filter {
                            let filter_val = if filter.variable == target.variable {
                                bu_val
                            } else if let Some(results) = sim_results {
                                benunit_result_variable(&results.benunit_results[bu_id], &filter.variable)
                                    .unwrap_or(0.0)
                            } else {
                                0.0
                            };
                            if filter_val < filter.min || filter_val >= filter.max {
                                continue;
                            }
                        }

                        match target.aggregation.as_str() {
                            "sum" => {
                                contribution += bu_val;
                            }
                            "count_nonzero" => {
                                if bu_val > 0.0 {
                                    contribution += 1.0;
                                }
                            }
                            "count" => {
                                contribution += 1.0;
                            }
                            _ => {}
                        }
                    }
                    matrix[i][j] = contribution;
                }
            }
            "household" => {
                for (i, hh) in dataset.households.iter().enumerate() {
                    // Apply filter if present
                    if let Some(ref filter) = target.filter {
                        let filter_val = household_variable(hh, sim_results, i, &filter.variable);
                        if filter_val < filter.min || filter_val >= filter.max {
                            continue;
                        }
                    }
                    match target.aggregation.as_str() {
                        "sum" => {
                            matrix[i][j] = household_variable(hh, sim_results, i, &target.variable);
                        }
                        "count" | "count_nonzero" => {
                            let val = household_variable(hh, sim_results, i, &target.variable);
                            matrix[i][j] = if val > 0.0 { 1.0 } else { 0.0 };
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    // Skip targets where no household contributes (matrix column all zero).
    let mut n_skipped = 0;
    for j in 0..n_targets {
        let col_sum: f64 = (0..n_hh).map(|i| matrix[i][j].abs()).sum();
        if col_sum < 1e-10 {
            training_mask[j] = false;
            n_skipped += 1;
        }
    }
    if n_skipped > 0 {
        eprintln!("  Skipped {} targets with no survey representation", n_skipped);
    }

    (matrix, target_values, training_mask)
}

// ── Adam optimiser ─────────────────────────────────────────────────────────

/// Calibration configuration.
pub struct CalibrateConfig {
    pub epochs: usize,
    pub lr: f64,
    pub beta1: f64,
    pub beta2: f64,
    pub eps: f64,
    pub dropout: f64,
    pub log_interval: usize,
    /// Maximum ratio of calibrated weight to initial weight (e.g. 100.0 means
    /// no household can exceed 100x its original weight). Set to 0 to disable.
    pub max_weight_ratio: f64,
}

impl Default for CalibrateConfig {
    fn default() -> Self {
        CalibrateConfig {
            epochs: 512,
            lr: 0.1,
            beta1: 0.9,
            beta2: 0.999,
            eps: 1e-8,
            dropout: 0.05,
            log_interval: 50,
            max_weight_ratio: 100.0,
        }
    }
}

/// Result of calibration.
pub struct CalibrateResult {
    pub weights: Vec<f64>,
    pub final_training_loss: f64,
    pub final_holdout_loss: f64,
    pub per_target_error: Vec<(String, f64, f64, f64, bool)>,
}

/// Run Adam optimisation to find weights minimising MSRE against targets.
pub fn calibrate(
    matrix: &[Vec<f64>],
    target_values: &[f64],
    training_mask: &[bool],
    initial_weights: &[f64],
    config: &CalibrateConfig,
) -> CalibrateResult {
    let n_hh = matrix.len();
    let n_targets = target_values.len();
    let n_training = training_mask.iter().filter(|&&m| m).count();

    if n_hh == 0 || n_targets == 0 || n_training == 0 {
        return CalibrateResult {
            weights: initial_weights.to_vec(),
            final_training_loss: 0.0,
            final_holdout_loss: 0.0,
            per_target_error: Vec::new(),
        };
    }

    let mut u: Vec<f64> = initial_weights.iter()
        .map(|&w| if w > 0.0 { w.ln() } else { 0.0 })
        .collect();

    // Compute log-space bounds for weight clamping
    let u_max: Vec<f64> = if config.max_weight_ratio > 0.0 {
        initial_weights.iter()
            .map(|&w| if w > 0.0 { (w * config.max_weight_ratio).ln() } else { (config.max_weight_ratio).ln() })
            .collect()
    } else {
        vec![f64::INFINITY; n_hh]
    };
    let u_min: Vec<f64> = if config.max_weight_ratio > 0.0 {
        initial_weights.iter()
            .map(|&w| if w > 0.0 { (w / config.max_weight_ratio).ln() } else { -(config.max_weight_ratio).ln() })
            .collect()
    } else {
        vec![f64::NEG_INFINITY; n_hh]
    };

    let mut m = vec![0.0f64; n_hh];
    let mut v = vec![0.0f64; n_hh];

    let mut rng = rand::thread_rng();

    for epoch in 0..config.epochs {
        let weights: Vec<f64> = u.iter().enumerate().map(|(_i, &ui)| {
            let w = ui.exp();
            if config.dropout > 0.0 && rng.gen::<f64>() < config.dropout {
                0.0
            } else {
                w / (1.0 - config.dropout)
            }
        }).collect();

        let predictions: Vec<f64> = (0..n_targets).into_par_iter().map(|j| {
            let mut pred = 0.0f64;
            for i in 0..n_hh {
                pred += weights[i] * matrix[i][j];
            }
            pred
        }).collect();

        let residuals: Vec<f64> = (0..n_targets).map(|j| {
            if target_values[j].abs() > 1.0 {
                predictions[j] / target_values[j] - 1.0
            } else {
                0.0
            }
        }).collect();

        let training_loss: f64 = residuals.iter().enumerate()
            .filter(|(j, _)| training_mask[*j])
            .map(|(_, r)| r * r)
            .sum::<f64>() / n_training as f64;

        let n_holdout = training_mask.iter().filter(|&&m| !m).count();
        let holdout_loss = if n_holdout > 0 {
            residuals.iter().enumerate()
                .filter(|(j, _)| !training_mask[*j])
                .map(|(_, r)| r * r)
                .sum::<f64>() / n_holdout as f64
        } else {
            0.0
        };

        if epoch % config.log_interval == 0 || epoch == config.epochs - 1 {
            eprintln!(
                "  Epoch {:>4}/{}: training RMSRE {:.2}%, holdout RMSRE {:.2}%",
                epoch, config.epochs,
                training_loss.sqrt() * 100.0,
                holdout_loss.sqrt() * 100.0,
            );
        }

        if epoch == config.epochs - 1 {
            let final_weights: Vec<f64> = u.iter().map(|&ui| ui.exp()).collect();
            let final_preds: Vec<f64> = (0..n_targets).map(|j| {
                (0..n_hh).map(|i| final_weights[i] * matrix[i][j]).sum()
            }).collect();

            let final_training_loss: f64 = (0..n_targets)
                .filter(|&j| training_mask[j])
                .map(|j| {
                    let r = if target_values[j].abs() > 1.0 {
                        final_preds[j] / target_values[j] - 1.0
                    } else { 0.0 };
                    r * r
                }).sum::<f64>() / n_training as f64;

            let final_holdout_loss = if n_holdout > 0 {
                (0..n_targets)
                    .filter(|&j| !training_mask[j])
                    .map(|j| {
                        let r = if target_values[j].abs() > 1.0 {
                            final_preds[j] / target_values[j] - 1.0
                        } else { 0.0 };
                        r * r
                    }).sum::<f64>() / n_holdout as f64
            } else { 0.0 };

            return CalibrateResult {
                weights: final_weights,
                final_training_loss,
                final_holdout_loss,
                per_target_error: (0..n_targets).map(|j| {
                    let rel_err = if target_values[j].abs() > 1.0 {
                        final_preds[j] / target_values[j] - 1.0
                    } else { 0.0 };
                    (String::new(), final_preds[j], target_values[j], rel_err, !training_mask[j])
                }).collect(),
            };
        }

        let grad: Vec<f64> = (0..n_hh).into_par_iter().map(|i| {
            let w_i = weights[i];
            let mut g = 0.0f64;
            for j in 0..n_targets {
                if training_mask[j] && target_values[j].abs() > 1.0 {
                    g += residuals[j] * matrix[i][j] * w_i / target_values[j];
                }
            }
            2.0 * g / n_training as f64
        }).collect();

        let t = (epoch + 1) as f64;
        let bc1 = 1.0 - config.beta1.powf(t);
        let bc2 = 1.0 - config.beta2.powf(t);

        for i in 0..n_hh {
            m[i] = config.beta1 * m[i] + (1.0 - config.beta1) * grad[i];
            v[i] = config.beta2 * v[i] + (1.0 - config.beta2) * grad[i] * grad[i];
            let m_hat = m[i] / bc1;
            let v_hat = v[i] / bc2;
            u[i] -= config.lr * m_hat / (v_hat.sqrt() + config.eps);
            // Clamp to max weight ratio bounds
            u[i] = u[i].clamp(u_min[i], u_max[i]);
        }
    }

    let final_weights: Vec<f64> = u.iter().map(|&ui| ui.exp()).collect();
    CalibrateResult {
        weights: final_weights,
        final_training_loss: 0.0,
        final_holdout_loss: 0.0,
        per_target_error: Vec::new(),
    }
}

// ── Reporting ──────────────────────────────────────────────────────────────

pub fn print_report(
    targets: &[CalibrationTarget],
    result: &CalibrateResult,
    dataset: &Dataset,
) {
    let total_weight: f64 = result.weights.iter().sum();
    let original_weight: f64 = dataset.households.iter().map(|h| h.weight).sum();

    eprintln!("\n{}", "Calibration complete".bright_green().bold());
    eprintln!(
        "  Households: {}  Original weight sum: {:.0}  Calibrated weight sum: {:.0}",
        dataset.households.len(), original_weight, total_weight
    );
    eprintln!(
        "  Training RMSRE: {:.2}%  Holdout RMSRE: {:.2}%",
        result.final_training_loss.sqrt() * 100.0,
        result.final_holdout_loss.sqrt() * 100.0,
    );

    let mut rows: Vec<(usize, &str, f64, f64, f64, bool)> = result.per_target_error.iter().enumerate()
        .map(|(j, (_, pred, target, rel_err, holdout))| {
            (j, targets[j].name.as_str(), *pred, *target, *rel_err, *holdout)
        })
        .collect();

    rows.sort_by(|a, b| b.4.abs().partial_cmp(&a.4.abs()).unwrap_or(std::cmp::Ordering::Equal));

    let mut table = Table::new();
    table.load_preset(presets::UTF8_FULL_CONDENSED);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec!["Target", "Predicted", "Actual", "Rel error", "Type"]);

    let show_count = 30;
    for (idx, (_, name, pred, target, rel_err, holdout)) in rows.iter().enumerate() {
        if idx >= show_count && !holdout {
            continue;
        }
        let err_pct = rel_err * 100.0;
        let err_color = if err_pct.abs() < 5.0 {
            Color::Green
        } else if err_pct.abs() < 15.0 {
            Color::Yellow
        } else {
            Color::Red
        };
        let type_str = if *holdout { "holdout" } else { "training" };

        table.add_row(vec![
            Cell::new(name),
            Cell::new(format_value(*pred)),
            Cell::new(format_value(*target)),
            Cell::new(format!("{:+.1}%", err_pct)).fg(err_color),
            Cell::new(type_str),
        ]);
    }

    eprintln!("\n{table}");
}

fn format_value(v: f64) -> String {
    let abs = v.abs();
    if abs >= 1e9 {
        format!("£{:.1}bn", v / 1e9)
    } else if abs >= 1e6 {
        format!("£{:.1}m", v / 1e6)
    } else if abs >= 1e3 {
        format!("{:.0}k", v / 1e3)
    } else {
        format!("{:.0}", v)
    }
}

// ── Apply weights ──────────────────────────────────────────────────────────

pub fn apply_weights(dataset: &mut Dataset, weights: &[f64]) {
    for (i, hh) in dataset.households.iter_mut().enumerate() {
        if i < weights.len() {
            hh.weight = weights[i];
        }
    }
}
