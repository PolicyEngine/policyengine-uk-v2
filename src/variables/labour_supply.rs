/// OBR labour supply response (intensive margin).
///
/// Implements the Slutsky decomposition from:
/// OBR (2023) "Costing a cut in National Insurance contributions: the impact on labour supply"
/// https://obr.uk/docs/dlm_uploads/NICS-Cut-Impact-on-Labour-Supply-Note.pdf
///
/// For each working adult we compute:
///   ΔE = E_base × (η_s × Δw/w  +  η_i × Δy/y)
///
/// where:
///   η_s = substitution elasticity (marginal net wage change)
///   η_i = income elasticity (net income change)
///   Δw/w = relative change in marginal net wage (1 − marginal effective tax rate)
///   Δy/y = relative change in household net income
///
/// ## Batched derivative computation
///
/// Computing marginal retention for every worker individually would require one full
/// simulation per worker — O(n_workers) runs. Instead we batch by "adult slot":
///
///   Slot 0 = first eligible adult in each household
///   Slot 1 = second eligible adult in each household
///   ...
///
/// For each slot, we run exactly one perturbed simulation in which every worker
/// assigned to that slot has their employment income raised by DELTA simultaneously.
/// Because each slot contains at most one worker per household, the change in
/// household net income is attributable to that one person, giving us their
/// individual marginal retention rate cleanly.
///
/// Total simulation count = 2 (unperturbed) + 2 × max_slots.
/// For typical FRS data (≤2 working adults per household) this is 6 sims total
/// regardless of dataset size.

use crate::engine::entities::{Gender, Person, BenUnit, Household};
use crate::engine::simulation::Simulation;
use crate::parameters::{LabourSupplyParams, Parameters};

/// Perturbation size for numerical marginal retention derivative (£).
const DELTA: f64 = 1_000.0;

/// Whether a person is excluded from labour supply responses.
/// Excludes self-employed (FRS EMPSTATB=1), aged 60+, and zero employment income.
fn is_excluded(person: &Person) -> bool {
    person.age >= 60.0
        || person.emp_status == 1
        || person.employment_income <= 0.0
}

/// Youngest child age in a benefit unit (f64::MAX if no children).
fn youngest_child_age(bu: &BenUnit, people: &[Person]) -> f64 {
    bu.person_ids.iter()
        .filter(|&&pid| people[pid].is_child())
        .map(|&pid| people[pid].age)
        .fold(f64::MAX, f64::min)
}

/// OBR substitution elasticity (η_s) for a person.
pub fn substitution_elasticity(
    person: &Person,
    bu: &BenUnit,
    people: &[Person],
    ls: &LabourSupplyParams,
) -> f64 {
    let is_female = person.gender == Gender::Female;
    let is_coupled = bu.is_couple(people);
    let has_children = bu.num_children(people) > 0;

    if is_female && is_coupled {
        if !has_children {
            ls.subst_married_women_no_children
        } else {
            let yca = youngest_child_age(bu, people);
            if yca <= 2.0 { ls.subst_married_women_child_0_2 }
            else if yca <= 4.0 { ls.subst_married_women_child_3_4 }
            else if yca <= 10.0 { ls.subst_married_women_child_5_10 }
            else { ls.subst_married_women_child_11_plus }
        }
    } else if is_female && !is_coupled && has_children {
        let yca = youngest_child_age(bu, people);
        if yca <= 4.0 { ls.subst_lone_parents_child_0_4 }
        else if yca <= 10.0 { ls.subst_lone_parents_child_5_10 }
        else { ls.subst_lone_parents_child_11_18 }
    } else {
        ls.subst_men_and_single_women
    }
}

/// OBR income elasticity (η_i) for a person.
pub fn income_elasticity(
    person: &Person,
    bu: &BenUnit,
    people: &[Person],
    ls: &LabourSupplyParams,
) -> f64 {
    let is_female = person.gender == Gender::Female;
    let is_coupled = bu.is_couple(people);
    let has_children = bu.num_children(people) > 0;

    if is_female && is_coupled {
        if !has_children {
            ls.income_married_women_no_children
        } else {
            let yca = youngest_child_age(bu, people);
            if yca <= 2.0 { ls.income_married_women_child_0_2 }
            else if yca <= 4.0 { ls.income_married_women_child_3_4 }
            else if yca <= 10.0 { ls.income_married_women_child_5_10 }
            else { ls.income_married_women_child_11_plus }
        }
    } else if is_female && !is_coupled && has_children {
        let yca = youngest_child_age(bu, people);
        if yca <= 4.0 { ls.income_lone_parents_child_0_4 }
        else if yca <= 10.0 { ls.income_lone_parents_child_5_10 }
        else { ls.income_lone_parents_child_11_18 }
    } else {
        ls.income_men_and_single_women
    }
}

/// Run one simulation and return household net incomes indexed by household id.
fn run_net_incomes(
    people: &[Person],
    benunits: &[BenUnit],
    households: &[Household],
    params: &Parameters,
    fiscal_year: u32,
    baseline_old_sp: f64,
    baseline_new_sp: f64,
) -> Vec<f64> {
    let sim = Simulation::new_with_baseline_sp(
        people.to_vec(),
        benunits.to_vec(),
        households.to_vec(),
        params.clone(),
        baseline_old_sp,
        baseline_new_sp,
        fiscal_year,
    );
    sim.run().household_results.iter().map(|hr| hr.net_income).collect()
}

/// Assign each eligible worker a slot index (0 = first eligible adult in their
/// household, 1 = second, ...). Workers in the same slot are in different
/// households, so perturbing all of them simultaneously is safe — each
/// household's net income change is attributable to exactly one person.
///
/// Returns a vec of length `n_people` where `slot[pid]` is Some(slot_index)
/// for eligible workers and None for excluded persons.
fn assign_adult_slots(people: &[Person], households: &[Household]) -> Vec<Option<usize>> {
    let mut slots = vec![None; people.len()];
    // Track how many eligible workers have been assigned per household
    let mut hh_count = vec![0usize; households.len()];

    for pid in 0..people.len() {
        if is_excluded(&people[pid]) { continue; }
        let hid = people[pid].household_id;
        slots[pid] = Some(hh_count[hid]);
        hh_count[hid] += 1;
    }
    slots
}

/// Compute marginal retention rates for all eligible workers using batched sims.
///
/// For each slot index, build a perturbed people vec where every worker in that
/// slot has employment_income += DELTA. Run one sim, compare to `unperturbed_net`.
/// The result is a vec of length `n_people` with the retention rate for each worker.
fn batch_marginal_retention(
    people: &[Person],
    benunits: &[BenUnit],
    households: &[Household],
    params: &Parameters,
    fiscal_year: u32,
    baseline_old_sp: f64,
    baseline_new_sp: f64,
    unperturbed_net: &[f64],
    slots: &[Option<usize>],
    max_slot: usize,
) -> Vec<f64> {
    let n = people.len();
    let mut retention = vec![f64::NAN; n];

    for slot in 0..=max_slot {
        // Build perturbed vec: bump every worker assigned to this slot
        let mut perturbed = people.to_vec();
        for pid in 0..n {
            if slots[pid] == Some(slot) {
                perturbed[pid].employment_income += DELTA;
            }
        }

        let perturbed_net = run_net_incomes(
            &perturbed, benunits, households,
            params, fiscal_year, baseline_old_sp, baseline_new_sp,
        );

        // Each perturbed household has exactly one bumped worker — attribute the
        // net income change to that worker.
        for pid in 0..n {
            if slots[pid] == Some(slot) {
                let hid = people[pid].household_id;
                retention[pid] = ((perturbed_net[hid] - unperturbed_net[hid]) / DELTA)
                    .clamp(0.0, 1.0);
            }
        }
    }

    retention
}

/// Apply OBR labour supply responses, returning an adjusted copy of `people`
/// with employment incomes updated.
///
/// Uses batched derivative computation — total simulation count is
/// 2 (unperturbed) + 2 × max_adult_slots, typically 6 for FRS data.
pub fn apply_labour_supply_responses(
    people: &[Person],
    benunits: &[BenUnit],
    households: &[Household],
    baseline_params: &Parameters,
    policy_params: &Parameters,
    baseline_net: &[f64],
    fiscal_year: u32,
) -> Vec<Person> {
    let ls = &policy_params.labour_supply;
    if !ls.enabled {
        return people.to_vec();
    }

    let baseline_old_sp = baseline_params.state_pension.old_basic_pension_weekly;
    let baseline_new_sp = baseline_params.state_pension.new_state_pension_weekly;

    // Assign adult slots (O(n), no simulations)
    let slots = assign_adult_slots(people, households);
    let max_slot = slots.iter().filter_map(|s| *s).max();
    let max_slot = match max_slot {
        Some(s) => s,
        None => return people.to_vec(), // no eligible workers
    };

    // Unperturbed policy net incomes (the income-effect denominator)
    let unperturbed_policy_net = run_net_incomes(
        people, benunits, households,
        policy_params, fiscal_year, baseline_old_sp, baseline_new_sp,
    );

    // Batched marginal retention: 1 sim per slot per scenario
    let baseline_retention = batch_marginal_retention(
        people, benunits, households,
        baseline_params, fiscal_year, baseline_old_sp, baseline_new_sp,
        baseline_net, &slots, max_slot,
    );
    let policy_retention = batch_marginal_retention(
        people, benunits, households,
        policy_params, fiscal_year, baseline_old_sp, baseline_new_sp,
        &unperturbed_policy_net, &slots, max_slot,
    );

    // Apply Slutsky decomposition
    let mut adjusted = people.to_vec();

    for pid in 0..people.len() {
        if slots[pid].is_none() { continue; }

        let person = &people[pid];
        let hid = person.household_id;
        let bu = &benunits[person.benunit_id];

        let w_base = baseline_retention[pid];
        let w_policy = policy_retention[pid];
        let dw_over_w = if w_base > 1e-9 {
            ((w_policy - w_base) / w_base).clamp(-1.0, 1.0)
        } else {
            0.0
        };

        let y_base = baseline_net[hid];
        let y_policy = unperturbed_policy_net[hid];
        let dy_over_y = if y_base.abs() > 1e-9 {
            ((y_policy - y_base) / y_base).clamp(-1.0, 1.0)
        } else {
            0.0
        };

        let eta_s = substitution_elasticity(person, bu, people, ls);
        let eta_i = income_elasticity(person, bu, people, ls);

        let delta_e = person.employment_income * (eta_s * dw_over_w + eta_i * dy_over_y);
        adjusted[pid].employment_income = (person.employment_income + delta_e).max(0.0);
    }

    adjusted
}
