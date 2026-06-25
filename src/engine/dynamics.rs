//! Dynamics: behavioural-response passes applied between a baseline run and a
//! reform run.
//!
//! PolicyEngine's `apply_dynamics()` (`policyengine_uk/simulation.py`) runs a
//! sequence of behavioural adjustments after the baseline is computed but before
//! the reform is scored: incomes settle, marginal/participation tax rates are
//! read off, a behavioural response mutates inputs, and the downstream variables
//! are re-computed. Rust already has the OBR labour-supply *formulas*
//! ([`crate::variables::labour_supply`]) but, before this module, no shared
//! interface to slot them (or any other response) into a baseline→reform branch.
//!
//! A [`Dynamics`] is one such pass. Given the baseline [`SimulationResults`] and
//! the input frame, it returns an adjusted frame (people + benefit units) that
//! the reform simulation should run on. The contract is deliberately small so
//! responses compose: [`apply_dynamics`] threads a slice of passes in order, so
//! e.g. a labour-supply response can run before a take-up response. The engine
//! itself is untouched — a dynamics pass is pure "edit the inputs, then re-run".
//!
//! Two implementations ship here:
//!   * [`LabourSupplyDynamics`] — wraps the existing OBR Slutsky-decomposition
//!     [`apply_labour_supply_responses`], so the half-built `labour_supply.rs`
//!     formulas now have a formal hook. No logic is duplicated.
//!   * [`TakeUpDynamics`] — a deterministic benefit take-up response: a campaign
//!     that raises take-up of a set of means-tested benefits by flipping the
//!     `would_claim_*` flags for benefit units whose `migration_seed` falls under
//!     a configurable rate. Mirrors the existing deterministic take-up gate in
//!     `variables::benefits`.
//!
//! # Example
//!
//! ```ignore
//! use crate::engine::dynamics::{apply_dynamics, LabourSupplyDynamics};
//!
//! let baseline = baseline_sim.run();
//! let (people, benunits) = apply_dynamics(
//!     &[&LabourSupplyDynamics],
//!     &baseline,
//!     &dataset.people, &dataset.benunits, &dataset.households,
//!     &baseline_params, &reform_params, fiscal_year,
//! );
//! let reform = Simulation::new_with_baseline_sp(
//!     people, benunits, dataset.households.clone(), reform_params,
//!     baseline_params.state_pension.old_basic_pension_weekly, fiscal_year,
//! ).run();
//! ```

use crate::engine::entities::{BenUnit, Household, Person};
use crate::engine::simulation::SimulationResults;
use crate::parameters::Parameters;
use crate::variables::labour_supply::apply_labour_supply_responses;

/// A behavioural-response pass run between baseline and reform.
///
/// Implementors read the baseline outcome and the input frame and return an
/// adjusted `(people, benunits)`. They must not change the *shape* of the frame
/// (ids, lengths, household membership) — only field values — so that the reform
/// run lines up household-for-household with the baseline for comparison.
#[allow(dead_code)]
pub trait Dynamics: Sync {
    /// Short name for logging / debugging (e.g. `"labour_supply"`).
    fn name(&self) -> &'static str;

    /// Produce the adjusted input frame the reform should run on.
    ///
    /// `baseline` is the already-computed baseline result set. `people` /
    /// `benunits` / `households` are the (possibly already partly-adjusted by an
    /// earlier pass) input frame. `baseline_params` and `reform_params` are the
    /// two policies being compared.
    fn apply(
        &self,
        baseline: &SimulationResults,
        people: &[Person],
        benunits: &[BenUnit],
        households: &[Household],
        baseline_params: &Parameters,
        reform_params: &Parameters,
        fiscal_year: u32,
    ) -> (Vec<Person>, Vec<BenUnit>);
}

/// Thread a sequence of dynamics passes, in order, over the input frame.
///
/// Each pass sees the output of the previous one. Returns the final adjusted
/// `(people, benunits)`. With an empty slice this is the identity (a static run).
#[allow(dead_code)]
pub fn apply_dynamics(
    passes: &[&dyn Dynamics],
    baseline: &SimulationResults,
    people: &[Person],
    benunits: &[BenUnit],
    households: &[Household],
    baseline_params: &Parameters,
    reform_params: &Parameters,
    fiscal_year: u32,
) -> (Vec<Person>, Vec<BenUnit>) {
    let mut people = people.to_vec();
    let mut benunits = benunits.to_vec();
    for pass in passes {
        let (p, b) = pass.apply(
            baseline,
            &people,
            &benunits,
            households,
            baseline_params,
            reform_params,
            fiscal_year,
        );
        people = p;
        benunits = b;
    }
    (people, benunits)
}

/// OBR labour-supply response (intensive margin). Formalises the existing
/// [`apply_labour_supply_responses`] behind the [`Dynamics`] interface. Honours
/// `reform_params.labour_supply.enabled` (a no-op when disabled).
#[allow(dead_code)]
pub struct LabourSupplyDynamics;

impl Dynamics for LabourSupplyDynamics {
    fn name(&self) -> &'static str {
        "labour_supply"
    }

    fn apply(
        &self,
        baseline: &SimulationResults,
        people: &[Person],
        benunits: &[BenUnit],
        households: &[Household],
        baseline_params: &Parameters,
        reform_params: &Parameters,
        fiscal_year: u32,
    ) -> (Vec<Person>, Vec<BenUnit>) {
        let baseline_net: Vec<f64> = baseline
            .household_results
            .iter()
            .map(|hr| hr.net_income)
            .collect();
        let adjusted = apply_labour_supply_responses(
            people,
            benunits,
            households,
            baseline_params,
            reform_params,
            &baseline_net,
            fiscal_year,
        );
        (adjusted, benunits.to_vec())
    }
}

/// Deterministic benefit take-up response: a take-up campaign that raises
/// claiming of a set of means-tested benefits.
///
/// The existing engine gates take-up on `would_claim_*` flags combined with a
/// per-unit `migration_seed` drawn in `[0, 1)` (`variables::benefits`). This
/// dynamics raises take-up by flipping the relevant `would_claim_*` flag to
/// `true` for any benefit unit whose `migration_seed` is below `target_rate` —
/// i.e. modelling that a campaign lifts take-up to `target_rate`. A
/// `target_rate` of 1.0 models full take-up; 0.0 is a no-op.
///
/// Note: this edits the input frame (eligibility intent), so the reform run picks
/// up the extra claimants when it recomputes benefits downstream.
#[allow(dead_code)]
pub struct TakeUpDynamics {
    /// Target take-up rate in `[0, 1]`. Benefit units with
    /// `migration_seed < target_rate` are switched to claiming.
    pub target_rate: f64,
    /// Which benefits the campaign covers.
    pub benefits: TakeUpBenefits,
}

/// Which means-tested benefits a [`TakeUpDynamics`] campaign covers.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TakeUpBenefits {
    pub universal_credit: bool,
    pub pension_credit: bool,
    pub housing_benefit: bool,
    pub child_benefit: bool,
}

#[allow(dead_code)]
impl TakeUpBenefits {
    /// A campaign covering every means-tested benefit modelled here.
    pub fn all() -> Self {
        TakeUpBenefits {
            universal_credit: true,
            pension_credit: true,
            housing_benefit: true,
            child_benefit: true,
        }
    }
}

#[allow(dead_code)]
impl TakeUpDynamics {
    /// A full-take-up campaign across every benefit (`target_rate == 1.0`).
    pub fn full() -> Self {
        TakeUpDynamics { target_rate: 1.0, benefits: TakeUpBenefits::all() }
    }
}

impl Dynamics for TakeUpDynamics {
    fn name(&self) -> &'static str {
        "take_up"
    }

    fn apply(
        &self,
        _baseline: &SimulationResults,
        people: &[Person],
        benunits: &[BenUnit],
        _households: &[Household],
        _baseline_params: &Parameters,
        _reform_params: &Parameters,
        _fiscal_year: u32,
    ) -> (Vec<Person>, Vec<BenUnit>) {
        let rate = self.target_rate.clamp(0.0, 1.0);
        let mut benunits = benunits.to_vec();
        if rate <= 0.0 {
            return (people.to_vec(), benunits);
        }
        for bu in benunits.iter_mut() {
            if bu.migration_seed < rate {
                if self.benefits.universal_credit {
                    bu.would_claim_uc = true;
                }
                if self.benefits.pension_credit {
                    bu.would_claim_pc = true;
                }
                if self.benefits.housing_benefit {
                    bu.would_claim_hb = true;
                }
                if self.benefits.child_benefit {
                    bu.would_claim_cb = true;
                }
            }
        }
        (people.to_vec(), benunits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::entities::*;
    use crate::engine::simulation::Simulation;
    use crate::parameters::Parameters;

    fn single_worker(income: f64) -> (Vec<Person>, Vec<BenUnit>, Vec<Household>) {
        let mut p = Person::default();
        p.id = 0;
        p.benunit_id = 0;
        p.household_id = 0;
        p.age = 40.0;
        p.gender = Gender::Male;
        p.is_benunit_head = true;
        p.is_household_head = true;
        p.employment_income = income;
        p.hours_worked = 37.5 * 52.0;
        p.emp_status = 2; // full-time employee

        let bu = BenUnit { id: 0, household_id: 0, person_ids: vec![0], ..BenUnit::default() };
        let hh = Household {
            id: 0, benunit_ids: vec![0], person_ids: vec![0],
            weight: 1.0, region: Region::London, ..Household::default()
        };
        (vec![p], vec![bu], vec![hh])
    }

    /// An empty pass list is the identity (a static run): inputs unchanged.
    #[test]
    fn apply_dynamics_empty_is_identity() {
        let params = Parameters::for_year(2025).unwrap();
        let (people, benunits, households) = single_worker(30_000.0);
        let baseline = Simulation::new(
            people.clone(), benunits.clone(), households.clone(), params.clone(), 2025,
        ).run();

        let (p, b) = apply_dynamics(
            &[], &baseline, &people, &benunits, &households, &params, &params, 2025,
        );
        assert_eq!(p[0].employment_income, people[0].employment_income);
        assert_eq!(b.len(), benunits.len());
    }

    /// The labour-supply dynamics, behind the trait, reproduces the direct call:
    /// a basic-rate NI cut raises employment income.
    #[test]
    fn labour_supply_dynamics_raises_earnings_on_ni_cut() {
        let baseline_params = Parameters::for_year(2025).unwrap();
        let mut reform_params = baseline_params.clone();
        reform_params.national_insurance.main_rate -= 0.02;

        let (people, benunits, households) = single_worker(35_000.0);
        let baseline = Simulation::new(
            people.clone(), benunits.clone(), households.clone(), baseline_params.clone(), 2025,
        ).run();

        let (adjusted, _) = apply_dynamics(
            &[&LabourSupplyDynamics],
            &baseline, &people, &benunits, &households,
            &baseline_params, &reform_params, 2025,
        );
        assert!(
            adjusted[0].employment_income > people[0].employment_income,
            "NI cut should raise labour supply via the dynamics hook",
        );
    }

    /// With labour supply disabled, the dynamics pass is a no-op.
    #[test]
    fn labour_supply_dynamics_disabled_is_static() {
        let baseline_params = Parameters::for_year(2025).unwrap();
        let mut reform_params = baseline_params.clone();
        reform_params.national_insurance.main_rate -= 0.02;
        reform_params.labour_supply.enabled = false;

        let (people, benunits, households) = single_worker(35_000.0);
        let baseline = Simulation::new(
            people.clone(), benunits.clone(), households.clone(), baseline_params.clone(), 2025,
        ).run();

        let (adjusted, _) = apply_dynamics(
            &[&LabourSupplyDynamics],
            &baseline, &people, &benunits, &households,
            &baseline_params, &reform_params, 2025,
        );
        assert_eq!(adjusted[0].employment_income, people[0].employment_income);
    }

    /// A full take-up campaign flips the `would_claim_*` flags for an unclaimed
    /// benefit unit, and the reform run pays the previously-unclaimed benefit.
    #[test]
    fn take_up_dynamics_increases_benefit_spending() {
        let params = Parameters::for_year(2025).unwrap();

        // A low-income lone parent renting, who is NOT currently claiming UC
        // (would_claim_uc = false) but is entitled. migration_seed = 0 so the
        // campaign reaches them.
        let mut adult = Person::default();
        adult.id = 0; adult.benunit_id = 0; adult.household_id = 0;
        adult.age = 30.0; adult.is_benunit_head = true; adult.is_household_head = true;
        adult.employment_income = 4_000.0;
        let mut child = Person::default();
        child.id = 1; child.benunit_id = 0; child.household_id = 0; child.age = 4.0;

        let bu = BenUnit {
            id: 0, household_id: 0, person_ids: vec![0, 1],
            migration_seed: 0.0, on_uc: true, rent_monthly: 600.0,
            is_lone_parent: true,
            would_claim_uc: false, would_claim_cb: false,
            ..BenUnit::default()
        };
        let hh = Household {
            id: 0, benunit_ids: vec![0], person_ids: vec![0, 1],
            weight: 1.0, region: Region::London, rent: 7_200.0,
            ..Household::default()
        };

        let people = vec![adult, child];
        let benunits = vec![bu];
        let households = vec![hh];

        let baseline = Simulation::new(
            people.clone(), benunits.clone(), households.clone(), params.clone(), 2025,
        ).run();
        let baseline_spend: f64 = baseline.benunit_results.iter()
            .map(|r| r.total_benefits).sum();

        let (p, b) = apply_dynamics(
            &[&TakeUpDynamics::full()],
            &baseline, &people, &benunits, &households,
            &params, &params, 2025,
        );
        assert!(b[0].would_claim_uc, "Campaign should flip would_claim_uc on");
        assert!(b[0].would_claim_cb, "Campaign should flip would_claim_cb on");

        let reform = Simulation::new(p, b, households.clone(), params.clone(), 2025).run();
        let reform_spend: f64 = reform.benunit_results.iter()
            .map(|r| r.total_benefits).sum();

        assert!(
            reform_spend > baseline_spend + 1.0,
            "Full take-up should raise benefit spending: baseline {:.2} -> reform {:.2}",
            baseline_spend, reform_spend,
        );
    }

    /// A zero-rate take-up campaign is a no-op.
    #[test]
    fn take_up_dynamics_zero_rate_is_noop() {
        let params = Parameters::for_year(2025).unwrap();
        let (people, benunits, households) = single_worker(20_000.0);
        let baseline = Simulation::new(
            people.clone(), benunits.clone(), households.clone(), params.clone(), 2025,
        ).run();

        let dyn0 = TakeUpDynamics { target_rate: 0.0, benefits: TakeUpBenefits::all() };
        let (_, b) = apply_dynamics(
            &[&dyn0], &baseline, &people, &benunits, &households, &params, &params, 2025,
        );
        assert_eq!(b[0].would_claim_uc, benunits[0].would_claim_uc);
        assert_eq!(b[0].would_claim_cb, benunits[0].would_claim_cb);
    }
}
