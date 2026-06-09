//! Branch + comparison primitive for the simulation engine.
//!
//! Today the binary runs two simulations side-by-side (baseline + policy) by
//! cloning input frames and instantiating two `Simulation` values. That works
//! for aggregate scoring but couples the orchestration to `main.rs`, makes it
//! awkward to compute per-household reform effects from library code, and
//! provides no shared structure for the axes / dynamics primitives that #49
//! also calls out.
//!
//! `Simulation::branch` packages the "clone-and-reparameterise" idiom: given a
//! baseline `Simulation` and a counterfactual `Parameters`, it returns a new
//! `Simulation` that shares the same entity frame but runs against the reform
//! rules. The original baseline-old-SP rate carries over so reform-side state
//! pension scales relative to the original baseline (matching the existing
//! `new_with_baseline_sp` constructor used by `main.rs`).
//!
//! `Comparison::between` produces a flat per-household diff between two
//! `SimulationResults` plus a handful of population aggregates (net cost,
//! winners / losers / unchanged shares). It is intentionally small — axes
//! (`SimulationResults` per axis step) and dynamics (re-run downstream after a
//! behavioural-response edit) are follow-up slices.
//!
//! # Example
//!
//! ```ignore
//! use crate::engine::Simulation;
//!
//! let baseline = Simulation::new(people, benunits, households, baseline_params, 2025);
//! let reform   = baseline.branch(reform_params);
//!
//! let cmp = Comparison::between(&baseline.run(), &reform.run());
//! assert_eq!(cmp.net_income_diff.len(), baseline.households.len());
//! cmp.net_cost;        // £ change in (taxes − benefits) at population level
//! cmp.winners_pct;     // share of households with net_income_diff > tolerance
//! ```

use crate::engine::simulation::{Simulation, SimulationResults};
use crate::parameters::Parameters;

/// Default rounding noise tolerance: a household with a net-income change
/// smaller than this (£1) is treated as unchanged for winners/losers. Matches
/// the tolerance used in the existing `parity.py` harness.
pub const COMPARISON_TOLERANCE: f64 = 1.0;

/// Per-household and aggregate comparison between a baseline and reform
/// `SimulationResults`.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Comparison {
    /// `reform.net_income - baseline.net_income` for each household (HBAI net,
    /// matching `HouseholdResult::net_income`). Index aligned with the
    /// households slice the simulations were constructed from.
    pub net_income_diff: Vec<f64>,
    /// `reform_total_tax − baseline_total_tax`, summed over all households —
    /// positive for revenue-raising reforms, negative for revenue-losing ones.
    /// Matches the existing `BudgetaryImpact.revenue_change` sign convention
    /// used by the binary's JSON output.
    pub revenue_change: f64,
    /// `reform_total_benefits − baseline_total_benefits`, summed.
    pub benefit_spending_change: f64,
    /// `−revenue_change + benefit_spending_change` — the net fiscal cost of
    /// the reform (positive = costs money).
    pub net_cost: f64,
    /// Share of households with `net_income_diff > tolerance`.
    pub winners_pct: f64,
    /// Share of households with `net_income_diff < -tolerance`.
    pub losers_pct: f64,
    /// Share of households with `|net_income_diff| <= tolerance`.
    pub unchanged_pct: f64,
}

#[allow(dead_code)]
impl Comparison {
    /// Diff two result sets. Both must have been produced from the *same*
    /// entity frame (identical lengths) — a length mismatch is a programmer
    /// error and panics.
    pub fn between(baseline: &SimulationResults, reform: &SimulationResults) -> Self {
        Self::between_with_tolerance(baseline, reform, COMPARISON_TOLERANCE)
    }

    /// As `between` but with an explicit winners/losers tolerance (in £).
    pub fn between_with_tolerance(
        baseline: &SimulationResults,
        reform: &SimulationResults,
        tolerance: f64,
    ) -> Self {
        assert_eq!(
            baseline.household_results.len(),
            reform.household_results.len(),
            "Comparison requires baseline + reform to have the same number of households",
        );

        let n = baseline.household_results.len();
        let mut net_income_diff = Vec::with_capacity(n);
        let mut revenue_change = 0.0;
        let mut benefit_spending_change = 0.0;
        let mut winners = 0usize;
        let mut losers = 0usize;

        for (b, r) in baseline.household_results.iter().zip(&reform.household_results) {
            let d = r.net_income - b.net_income;
            net_income_diff.push(d);
            revenue_change += r.total_tax - b.total_tax;
            benefit_spending_change += r.total_benefits - b.total_benefits;
            if d > tolerance {
                winners += 1;
            } else if d < -tolerance {
                losers += 1;
            }
        }

        let unchanged = n.saturating_sub(winners + losers);
        let pct = |k: usize| if n == 0 { 0.0 } else { (k as f64) / (n as f64) * 100.0 };

        Comparison {
            net_income_diff,
            revenue_change,
            benefit_spending_change,
            net_cost: -revenue_change + benefit_spending_change,
            winners_pct: pct(winners),
            losers_pct: pct(losers),
            unchanged_pct: pct(unchanged),
        }
    }
}

#[allow(dead_code)]
impl Simulation {
    /// Fork this simulation under a counterfactual `parameters`, reusing the
    /// entity frame.
    ///
    /// The original simulation's baseline SP rates carry through, so reformed
    /// state pension scales correctly when reform parameters change the basic or
    /// new SP rate (mirroring the existing `new_with_baseline_sp` pattern in
    /// `main.rs`).
    pub fn branch(&self, parameters: Parameters) -> Self {
        Simulation {
            people: self.people.clone(),
            benunits: self.benunits.clone(),
            households: self.households.clone(),
            parameters,
            baseline_old_sp_weekly: self.baseline_old_sp_weekly,
            baseline_new_sp_weekly: self.baseline_new_sp_weekly,
            fiscal_year: self.fiscal_year,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::entities::*;
    use crate::engine::simulation::Simulation;
    use crate::parameters::Parameters;

    /// Three single-adult households spanning basic / higher / additional rate.
    fn three_household_frame() -> (Vec<Person>, Vec<BenUnit>, Vec<Household>) {
        let mut people = Vec::new();
        let mut benunits = Vec::new();
        let mut households = Vec::new();
        for (i, income) in [25_000.0, 60_000.0, 200_000.0].iter().enumerate() {
            let mut p = Person::default();
            p.id = i;
            p.benunit_id = i;
            p.household_id = i;
            p.age = 35.0;
            p.is_benunit_head = true;
            p.is_household_head = true;
            p.employment_income = *income;
            p.hours_worked = 37.5 * 52.0;
            people.push(p);

            let mut bu = BenUnit::default();
            bu.id = i;
            bu.household_id = i;
            bu.person_ids = vec![i];
            benunits.push(bu);

            let mut hh = Household::default();
            hh.id = i;
            hh.benunit_ids = vec![i];
            hh.person_ids = vec![i];
            hh.weight = 1.0;
            households.push(hh);
        }
        (people, benunits, households)
    }

    #[test]
    fn branch_produces_independent_simulation() {
        let baseline_params = Parameters::for_year(2025).unwrap();
        let (people, benunits, households) = three_household_frame();
        let baseline = Simulation::new(people, benunits, households, baseline_params.clone(), 2025);

        let mut reform_params = baseline_params.clone();
        reform_params.income_tax.personal_allowance += 5_000.0;
        let reform = baseline.branch(reform_params.clone());

        // Branch reuses the entity frame — same household IDs in the same order.
        assert_eq!(baseline.households.len(), reform.households.len());
        for (b, r) in baseline.households.iter().zip(&reform.households) {
            assert_eq!(b.id, r.id);
        }
        // But carries the new parameters.
        assert!(
            (reform.parameters.income_tax.personal_allowance
                - baseline.parameters.income_tax.personal_allowance
                - 5_000.0)
                .abs()
                < 1e-9,
        );
        // Mutating the reform must not bleed into the baseline.
        assert!(
            (baseline.parameters.income_tax.personal_allowance - 12_570.0).abs() < 1e-9,
        );
    }

    #[test]
    fn comparison_signs_a_pa_uplift_correctly() {
        // Raising the personal allowance: every household pays less income tax,
        // so winners > 0, losers == 0, revenue_change < 0, net_cost > 0.
        let baseline_params = Parameters::for_year(2025).unwrap();
        let (people, benunits, households) = three_household_frame();
        let baseline = Simulation::new(people, benunits, households, baseline_params.clone(), 2025);

        let mut reform_params = baseline_params.clone();
        reform_params.income_tax.personal_allowance += 5_000.0;
        let reform = baseline.branch(reform_params);

        let baseline_results = baseline.run();
        let reform_results = reform.run();

        let cmp = Comparison::between(&baseline_results, &reform_results);

        assert_eq!(cmp.net_income_diff.len(), 3);
        assert!(cmp.net_income_diff.iter().all(|&d| d >= 0.0));
        assert!(cmp.winners_pct > 0.0, "Some households should gain from a £5k PA uplift");
        assert_eq!(cmp.losers_pct, 0.0, "No household should lose from a £5k PA uplift");
        assert!(cmp.revenue_change < 0.0, "PA uplift must lose revenue ({})", cmp.revenue_change);
        assert!(cmp.net_cost > 0.0, "PA uplift must cost money ({})", cmp.net_cost);
    }

    #[test]
    fn comparison_no_op_when_baseline_equals_reform() {
        let baseline_params = Parameters::for_year(2025).unwrap();
        let (people, benunits, households) = three_household_frame();
        let baseline = Simulation::new(people, benunits, households, baseline_params.clone(), 2025);

        let reform = baseline.branch(baseline_params);
        let cmp = Comparison::between(&baseline.run(), &reform.run());

        assert!(cmp.net_income_diff.iter().all(|&d| d.abs() < 1e-6));
        assert!(cmp.revenue_change.abs() < 1e-6);
        assert!(cmp.benefit_spending_change.abs() < 1e-6);
        assert!(cmp.net_cost.abs() < 1e-6);
        assert_eq!(cmp.winners_pct, 0.0);
        assert_eq!(cmp.losers_pct, 0.0);
        assert!((cmp.unchanged_pct - 100.0).abs() < 1e-9);
    }
}
