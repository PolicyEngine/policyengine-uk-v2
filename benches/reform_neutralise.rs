//! Speedtest for reform benefit-neutralisation (PR #65 / #41).
//!
//! `Reform::apply_to_results` is a post-calc pass over the reform's
//! `SimulationResults`: it zeros named benefit fields on each `BenUnitResult`
//! and decrements the household income aggregates by the per-household delta.
//! Two linear passes (benunits, then households) of trivial arithmetic — the
//! question is just how much it adds on top of `run()` in the reform pipeline.
//!
//! Groups:
//!   * `end_to_end` — `run` vs `run_then_neutralise` on a real engine frame.
//!                    The gap is the realistic overhead of switching the
//!                    feature on.
//!   * `apply`      — `clone` vs `clone_then_neutralise` on a synthetic result
//!                    set where *every* household carries a non-zero benefit
//!                    (worst case: the household loop never short-circuits).
//!                    apply cost ≈ clone_then_neutralise − clone.
//!
//! Self-contained on a synthetic frame (scale via `NEUTRALISE_BENCH_HH`,
//! default 20_000). Run: `cargo bench --bench reform_neutralise`.

// Binary-only crate (no lib target): pull modules in by path, as the other
// benches and tests/parameter_impact.rs do.
#[path = "../src/axiom/mod.rs"]
#[allow(dead_code)]
mod axiom;
#[path = "../src/engine/mod.rs"]
mod engine;
#[path = "../src/parameters/mod.rs"]
mod parameters;
#[path = "../src/variables/mod.rs"]
mod variables;
#[path = "../src/reforms/mod.rs"]
mod reforms;
#[path = "../src/data/mod.rs"]
mod data;

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use engine::entities::*;
use engine::simulation::{BenUnitResult, HouseholdResult, PersonResult, Simulation, SimulationResults};
use parameters::Parameters;
use reforms::Reform;

/// The means-tested suite the worked example (`disable_means_tested.yaml`)
/// switches off — a representative multi-field neutralise list.
const NEUTRALISE: &[&str] = &[
    "universal_credit",
    "housing_benefit",
    "pension_credit",
    "child_tax_credit",
    "working_tax_credit",
];

fn frame_size() -> usize {
    std::env::var("NEUTRALISE_BENCH_HH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000)
}

/// `n` single-adult households spanning the rate bands (mirrors the other
/// benches' generator). Deterministic — no RNG.
fn make_frame(n: usize) -> (Vec<Person>, Vec<BenUnit>, Vec<Household>) {
    let mut people = Vec::with_capacity(n);
    let mut benunits = Vec::with_capacity(n);
    let mut households = Vec::with_capacity(n);
    for i in 0..n {
        let income = 15_000.0 + ((i * 137) % 200) as f64 * 1_000.0;
        let mut p = Person::default();
        p.id = i;
        p.benunit_id = i;
        p.household_id = i;
        p.age = 35.0;
        p.is_benunit_head = true;
        p.is_household_head = true;
        p.employment_income = income;
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

/// A result set where every benunit carries UC + HB, so the neutralise pass
/// always has work to do and no household short-circuits (worst case).
fn populated_results(n: usize) -> SimulationResults {
    let benunit_results = (0..n)
        .map(|_| {
            let mut br = BenUnitResult::default();
            br.universal_credit = 6_000.0;
            br.housing_benefit = 2_500.0;
            br.total_benefits = 8_500.0;
            br
        })
        .collect();
    let household_results = (0..n)
        .map(|_| {
            let mut hr = HouseholdResult::default();
            hr.total_benefits = 8_500.0;
            hr.net_income = 30_000.0;
            hr.net_income_ahc = 25_000.0;
            hr.extended_net_income = 29_500.0;
            hr.equivalisation_factor = 1.0;
            hr.equivalised_net_income = 30_000.0;
            hr.equivalised_net_income_ahc = 25_000.0;
            hr
        })
        .collect();
    SimulationResults {
        person_results: vec![PersonResult::default(); n],
        benunit_results,
        household_results,
    }
}

fn benches(c: &mut Criterion) {
    let n = frame_size();
    let year = 2025u32;
    let params = Parameters::for_year(year).unwrap();

    let reform = Reform {
        name: "disable_means_tested".into(),
        parameters: params.clone(),
        neutralise: NEUTRALISE.iter().map(|s| s.to_string()).collect(),
    };

    let (people, benunits, households) = make_frame(n);
    let sim = Simulation::new(
        people.clone(),
        benunits.clone(),
        households.clone(),
        params.clone(),
        year,
    );

    // --- Group 1: end-to-end — does turning neutralisation on cost much? ---
    {
        let mut g = c.benchmark_group(format!("end_to_end/{}hh", n));
        g.sample_size(30);
        g.bench_function("run", |b| b.iter(|| black_box(sim.run())));
        g.bench_function("run_then_neutralise", |b| {
            b.iter(|| {
                let mut res = sim.run();
                reform.apply_to_results(&mut res, &benunits, &households);
                black_box(res)
            })
        });
        g.finish();
    }

    // --- Group 2: the apply pass in isolation (worst case) ----------------
    {
        let seed = populated_results(n);
        let mut g = c.benchmark_group(format!("apply/{}hh", n));
        // clone baseline — apply mutates, so the iter must start from a fresh
        // copy each time; subtract this to isolate the pass itself.
        g.bench_function("clone", |b| b.iter(|| black_box(seed.clone())));
        g.bench_function("clone_then_neutralise", |b| {
            b.iter(|| {
                let mut res = seed.clone();
                reform.apply_to_results(&mut res, &benunits, &households);
                black_box(res)
            })
        });
        g.finish();
    }
}

criterion_group!(benches_group, benches);
criterion_main!(benches_group);
