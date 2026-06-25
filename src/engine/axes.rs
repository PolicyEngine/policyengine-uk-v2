//! Axes: parameter / input sweeps for the simulation engine.
//!
//! PolicyEngine's "axes" let you run the same household repeatedly with one
//! variable swept across a range, producing the per-step results that back
//! marginal-tax-rate, cliff-edge, and budget-constraint charts. Python expresses
//! an axis as `(variable, min, max, count)` (`policyengine_uk/simulation.py`); a
//! sweep runs the engine `count` times with that variable linearly spaced over
//! `[min, max]`.
//!
//! This module builds on the existing clone-and-rerun machinery rather than
//! duplicating it. An [`Axis`] holds a name, the `[min, max]` range, the step
//! `count`, and a *setter* — a closure that writes the swept value into either
//! the input frame (an input axis) or the [`Parameters`] (a parameter axis).
//! [`Simulation::run_axis`] linearly spaces the range, applies the setter for
//! each step, runs the engine, and stacks the [`SimulationResults`]. Steps are
//! independent, so they run in parallel with rayon — mirroring the per-household
//! parallelism already in `Simulation::run`.
//!
//! Two constructors cover the common cases:
//!   * [`Axis::on_people`] — write `value` into every (matching) person via a
//!     field setter. Use it to sweep employment income for a single household to
//!     trace a budget-constraint / MTR curve.
//!   * [`Axis::on_parameters`] — write `value` into the [`Parameters`] via a
//!     mutation closure. Use it to sweep a tax rate or threshold.
//!
//! # Example
//!
//! ```ignore
//! use crate::engine::Simulation;
//! use crate::engine::axes::Axis;
//!
//! // Sweep the single earner's employment income £0 → £200k in 41 steps and
//! // read off net income at each point (a budget-constraint curve).
//! let axis = Axis::on_people("employment_income", 0.0, 200_000.0, 41, |p, v| {
//!     p.employment_income = v;
//! });
//! let sweep = sim.run_axis(&axis);
//! let net: Vec<f64> = sweep.iter()
//!     .map(|step| step.results.household_results[0].net_income)
//!     .collect();
//! ```

use rayon::prelude::*;

use crate::engine::entities::Person;
use crate::engine::simulation::{Simulation, SimulationResults};
use crate::parameters::Parameters;

/// How an [`Axis`] mutates the simulation at each step.
///
/// `Sync` is required because steps run on the rayon thread pool; the closure is
/// shared across worker threads.
enum AxisSetter {
    /// Write the swept value into every person for which `select` returns true.
    /// `select` defaults to "all people" via [`Axis::on_people`].
    Person {
        select: Box<dyn Fn(&Person) -> bool + Sync>,
        set: Box<dyn Fn(&mut Person, f64) + Sync>,
    },
    /// Write the swept value into the parameters.
    Parameter {
        set: Box<dyn Fn(&mut Parameters, f64) + Sync>,
    },
}

/// A swept variable: a name, an inclusive `[min, max]` range, a step `count`,
/// and the setter that applies each value. Construct one with [`Axis::on_people`]
/// or [`Axis::on_parameters`].
#[allow(dead_code)]
pub struct Axis {
    /// Human-readable name of the swept variable (e.g. `"employment_income"`),
    /// echoed back in each [`AxisStep`] for labelling charts.
    pub name: String,
    /// First swept value (inclusive).
    pub min: f64,
    /// Last swept value (inclusive).
    pub max: f64,
    /// Number of steps. Must be >= 1. A count of 1 evaluates `min` only.
    pub count: usize,
    setter: AxisSetter,
}

/// One step of a sweep: the swept value and the results the engine produced for
/// it. `index` is the 0-based step number; `value` is the swept input/parameter
/// value at that step.
#[allow(dead_code)]
pub struct AxisStep {
    pub index: usize,
    pub value: f64,
    pub results: SimulationResults,
}

#[allow(dead_code)]
impl Axis {
    /// Sweep an input field over *every* person.
    ///
    /// `set` writes the swept value into a person (e.g.
    /// `|p, v| p.employment_income = v`). For a single-household frame this
    /// traces that household's response to the variable; for a multi-household
    /// frame every person moves together.
    pub fn on_people(
        name: impl Into<String>,
        min: f64,
        max: f64,
        count: usize,
        set: impl Fn(&mut Person, f64) + Sync + 'static,
    ) -> Self {
        Self::on_people_where(name, min, max, count, |_| true, set)
    }

    /// Sweep an input field over the people matching `select`.
    ///
    /// Use this to confine the sweep to one household / person in a larger frame
    /// (e.g. `|p| p.household_id == 0`).
    pub fn on_people_where(
        name: impl Into<String>,
        min: f64,
        max: f64,
        count: usize,
        select: impl Fn(&Person) -> bool + Sync + 'static,
        set: impl Fn(&mut Person, f64) + Sync + 'static,
    ) -> Self {
        Axis {
            name: name.into(),
            min,
            max,
            count,
            setter: AxisSetter::Person {
                select: Box::new(select),
                set: Box::new(set),
            },
        }
    }

    /// Sweep a parameter value.
    ///
    /// `set` writes the swept value into the [`Parameters`] (e.g.
    /// `|p, v| p.income_tax.uk_brackets[0].rate = v`).
    pub fn on_parameters(
        name: impl Into<String>,
        min: f64,
        max: f64,
        count: usize,
        set: impl Fn(&mut Parameters, f64) + Sync + 'static,
    ) -> Self {
        Axis {
            name: name.into(),
            min,
            max,
            count,
            setter: AxisSetter::Parameter { set: Box::new(set) },
        }
    }

    /// The swept value at step `i`, linearly spaced over `[min, max]`.
    /// Step 0 is `min`; step `count - 1` is `max`. With `count == 1` the single
    /// step is `min`.
    pub fn value_at(&self, i: usize) -> f64 {
        if self.count <= 1 {
            self.min
        } else {
            let t = i as f64 / (self.count - 1) as f64;
            self.min + t * (self.max - self.min)
        }
    }
}

#[allow(dead_code)]
impl Simulation {
    /// Run this simulation `axis.count` times, sweeping `axis` over its range,
    /// and return one [`AxisStep`] per value. Steps are independent and run in
    /// parallel.
    ///
    /// An input axis clones the people frame and applies the setter per step; a
    /// parameter axis clones the parameters and applies the setter per step. The
    /// untouched entity frame / parameters of `self` are reused for the other
    /// half, so a parameter sweep never re-clones the (large) people vec beyond
    /// what `Simulation` already owns.
    pub fn run_axis(&self, axis: &Axis) -> Vec<AxisStep> {
        (0..axis.count)
            .into_par_iter()
            .map(|i| {
                let value = axis.value_at(i);
                let results = self.run_axis_step(axis, value);
                AxisStep { index: i, value, results }
            })
            .collect()
    }

    /// Run a single sweep step at `value` without recording the index.
    fn run_axis_step(&self, axis: &Axis, value: f64) -> SimulationResults {
        match &axis.setter {
            AxisSetter::Person { select, set } => {
                let mut people = self.people.clone();
                for p in people.iter_mut() {
                    if select(p) {
                        set(p, value);
                    }
                }
                let sim = Simulation::new_with_baseline_sp(
                    people,
                    self.benunits.clone(),
                    self.households.clone(),
                    self.parameters.clone(),
                    self.baseline_old_sp_weekly,
                    self.fiscal_year,
                );
                sim.run()
            }
            AxisSetter::Parameter { set } => {
                let mut params = self.parameters.clone();
                set(&mut params, value);
                let sim = Simulation::new_with_baseline_sp(
                    self.people.clone(),
                    self.benunits.clone(),
                    self.households.clone(),
                    params,
                    self.baseline_old_sp_weekly,
                    self.fiscal_year,
                );
                sim.run()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::entities::*;
    use crate::parameters::Parameters;

    /// One single-adult household with zero starting employment income.
    fn one_household() -> (Vec<Person>, Vec<BenUnit>, Vec<Household>) {
        let mut p = Person::default();
        p.id = 0;
        p.benunit_id = 0;
        p.household_id = 0;
        p.age = 35.0;
        p.is_benunit_head = true;
        p.is_household_head = true;
        p.employment_income = 0.0;
        p.hours_worked = 37.5 * 52.0;

        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0], ..BenUnit::default() };
        let hh = Household {
            id: 0, benunit_ids: vec![0], person_ids: vec![0],
            weight: 1.0, region: Region::London, ..Household::default()
        };
        (vec![p], vec![bu], vec![hh])
    }

    #[test]
    fn value_at_spans_the_range() {
        let axis = Axis::on_people("employment_income", 0.0, 200_000.0, 41, |p, v| {
            p.employment_income = v;
        });
        assert_eq!(axis.value_at(0), 0.0);
        assert_eq!(axis.value_at(40), 200_000.0);
        // Midpoint
        assert!((axis.value_at(20) - 100_000.0).abs() < 1e-6);
    }

    #[test]
    fn single_step_axis_uses_min() {
        let axis = Axis::on_people("employment_income", 12_345.0, 99_999.0, 1, |p, v| {
            p.employment_income = v;
        });
        assert_eq!(axis.count, 1);
        assert_eq!(axis.value_at(0), 12_345.0);
    }

    /// The headline axes use-case: sweeping employment income produces a
    /// monotonically non-decreasing net income (you never end up worse off in
    /// cash terms from earning more under the UK schedule), and gross income
    /// tracks the swept value.
    #[test]
    fn earnings_sweep_net_income_is_monotone() {
        let params = Parameters::for_year(2025).unwrap();
        let (people, benunits, households) = one_household();
        let sim = Simulation::new(people, benunits, households, params, 2025);

        let axis = Axis::on_people("employment_income", 0.0, 200_000.0, 21, |p, v| {
            p.employment_income = v;
        });
        let sweep = sim.run_axis(&axis);

        assert_eq!(sweep.len(), 21);
        // Steps come back in order, swept value matches.
        for (i, step) in sweep.iter().enumerate() {
            assert_eq!(step.index, i);
            assert!((step.value - axis.value_at(i)).abs() < 1e-6);
        }

        let net: Vec<f64> = sweep.iter()
            .map(|s| s.results.household_results[0].net_income)
            .collect();
        // Net income rises (weakly) across the whole earnings range.
        for w in net.windows(2) {
            assert!(
                w[1] >= w[0] - 1e-6,
                "Net income should be monotone non-decreasing in earnings: {:.2} -> {:.2}",
                w[0], w[1],
            );
        }
        // And it strictly rises somewhere (the sweep does something).
        assert!(*net.last().unwrap() > net.first().unwrap() + 1.0);
    }

    /// Income tax rises monotonically as the swept earnings rise.
    #[test]
    fn earnings_sweep_income_tax_is_monotone() {
        let params = Parameters::for_year(2025).unwrap();
        let (people, benunits, households) = one_household();
        let sim = Simulation::new(people, benunits, households, params, 2025);

        let axis = Axis::on_people("employment_income", 0.0, 150_000.0, 16, |p, v| {
            p.employment_income = v;
        });
        let sweep = sim.run_axis(&axis);

        let tax: Vec<f64> = sweep.iter()
            .map(|s| s.results.person_results[0].income_tax)
            .collect();
        for w in tax.windows(2) {
            assert!(w[1] >= w[0] - 1e-6, "Income tax should rise with earnings");
        }
        // Bottom of the range is under the personal allowance → no income tax.
        assert!(tax[0] < 1e-6);
        // Top of the range pays substantial tax.
        assert!(*tax.last().unwrap() > 10_000.0);
    }

    /// A parameter axis: sweeping the basic rate up must (weakly) raise income
    /// tax at a fixed earnings level.
    #[test]
    fn basic_rate_parameter_sweep_raises_tax() {
        let params = Parameters::for_year(2025).unwrap();
        let mut p = Person::default();
        p.id = 0; p.benunit_id = 0; p.household_id = 0;
        p.age = 35.0; p.is_benunit_head = true; p.is_household_head = true;
        p.employment_income = 30_000.0;
        p.hours_worked = 37.5 * 52.0;
        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0], ..BenUnit::default() };
        let hh = Household {
            id: 0, benunit_ids: vec![0], person_ids: vec![0],
            weight: 1.0, region: Region::London, ..Household::default()
        };
        let sim = Simulation::new(vec![p], vec![bu], vec![hh], params, 2025);

        let axis = Axis::on_parameters("basic_rate", 0.10, 0.30, 11, |params, v| {
            params.income_tax.uk_brackets[0].rate = v;
        });
        let sweep = sim.run_axis(&axis);

        let tax: Vec<f64> = sweep.iter()
            .map(|s| s.results.person_results[0].income_tax)
            .collect();
        for w in tax.windows(2) {
            assert!(w[1] >= w[0] - 1e-6, "Higher basic rate must not lower tax");
        }
        assert!(*tax.last().unwrap() > tax[0] + 1.0, "Raising the basic rate must raise tax");
    }

    /// `on_people_where` confines the sweep to the selected household.
    #[test]
    fn on_people_where_only_touches_selected() {
        let params = Parameters::for_year(2025).unwrap();
        // Two households; only household 0 is swept.
        let mut people = Vec::new();
        let mut benunits = Vec::new();
        let mut households = Vec::new();
        for i in 0..2 {
            let mut p = Person::default();
            p.id = i; p.benunit_id = i; p.household_id = i;
            p.age = 35.0; p.is_benunit_head = true; p.is_household_head = true;
            p.employment_income = 40_000.0;
            p.hours_worked = 37.5 * 52.0;
            people.push(p);
            benunits.push(BenUnit { id: i, household_id: i, person_ids: vec![i], ..BenUnit::default() });
            households.push(Household {
                id: i, benunit_ids: vec![i], person_ids: vec![i],
                weight: 1.0, region: Region::London, ..Household::default()
            });
        }
        let sim = Simulation::new(people, benunits, households, params, 2025);

        let axis = Axis::on_people_where(
            "employment_income", 0.0, 100_000.0, 5,
            |p| p.household_id == 0,
            |p, v| p.employment_income = v,
        );
        let sweep = sim.run_axis(&axis);

        // Household 1 is untouched across every step → identical net income.
        let hh1_net: Vec<f64> = sweep.iter()
            .map(|s| s.results.household_results[1].net_income)
            .collect();
        for w in hh1_net.windows(2) {
            assert!((w[1] - w[0]).abs() < 1e-6, "Unselected household must be constant across sweep");
        }
        // Household 0 changes.
        let hh0_net: Vec<f64> = sweep.iter()
            .map(|s| s.results.household_results[0].net_income)
            .collect();
        assert!(*hh0_net.last().unwrap() > hh0_net.first().unwrap() + 1.0);
    }
}
