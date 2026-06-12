//! Speedtest for the `Simulation::branch` + `Comparison` primitive (PR #68 / #49).
//!
//! Nikhil's review ask: "this needs speedtest comparisons to be comfortable
//! merging." The concern is whether `branch` — which deep-clones the owned
//! entity `Vec`s — costs anything relative to the clone-baseline-then-`new`
//! dance the binary already does in `main.rs`. This bench answers that head-on.
//!
//! Three groups:
//!   * `fork`       — the primitive in isolation: `baseline.branch(reform)` vs
//!                    reconstructing the reform `Simulation` from the dataset
//!                    frames the way `main.rs` does (`Simulation::new(..clone..)`).
//!   * `components` — `run()` (one full parallel pass) and `Comparison::between`
//!                    on their own, to size everything relative to the actual
//!                    compute.
//!   * `end_to_end` — the realistic flow: build baseline, produce a reform
//!                    result, and compare. `branch_pipeline` (via the new
//!                    primitive) vs `dance_pipeline` (the current `main.rs`
//!                    sequence: two independent `Simulation`s, each built by
//!                    cloning the dataset frames). If these match, `branch`
//!                    regresses nothing.
//!
//! The frame is synthetic so the bench is self-contained (no FRS data needed)
//! and reproducible in CI. Scale it with `BRANCH_BENCH_HH` (default 20_000 —
//! roughly an FRS-sized cross-section of single-adult households spanning the
//! basic / higher / additional rate bands).
//!
//! Run: `cargo bench --bench branch`
//!      `BRANCH_BENCH_HH=50000 cargo bench --bench branch`

// The crate is binary-only (no lib target), so pull the engine modules in by
// path, exactly as `tests/parameter_impact.rs` does.
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
use engine::simulation::Simulation;
use engine::Comparison;
use parameters::Parameters;

/// Build `n` single-adult households spanning the basic / higher / additional
/// rate bands. Deterministic (no RNG) so successive bench runs are comparable.
/// Mirrors the `three_household_frame` helper in `branch.rs`, scaled up.
fn make_frame(n: usize) -> (Vec<Person>, Vec<BenUnit>, Vec<Household>) {
    let mut people = Vec::with_capacity(n);
    let mut benunits = Vec::with_capacity(n);
    let mut households = Vec::with_capacity(n);

    for i in 0..n {
        // Spread incomes across ~£15k–£215k so every household exercises a real
        // tax position (PA taper, higher rate, additional rate).
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

fn frame_size() -> usize {
    std::env::var("BRANCH_BENCH_HH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000)
}

fn benches(c: &mut Criterion) {
    let n = frame_size();
    let year = 2025u32;

    let baseline_params = Parameters::for_year(year).expect("2025 params");
    // A £5k personal-allowance uplift: a real reform that moves every band.
    let mut reform_params = baseline_params.clone();
    reform_params.income_tax.personal_allowance += 5_000.0;

    let (people, benunits, households) = make_frame(n);
    let baseline = Simulation::new(
        people.clone(),
        benunits.clone(),
        households.clone(),
        baseline_params.clone(),
        year,
    );

    // --- Group 1: the fork primitive in isolation -------------------------
    {
        let mut g = c.benchmark_group(format!("fork/{}hh", n));
        // `branch`: clone the entity frame off an existing baseline + swap params.
        g.bench_function("branch", |b| {
            b.iter(|| black_box(baseline.branch(black_box(reform_params.clone()))))
        });
        // The `main.rs` equivalent: build the reform sim straight from the
        // dataset frames (one clone of each Vec), same as the binary does today.
        g.bench_function("construct_new_from_dataset", |b| {
            b.iter(|| {
                black_box(Simulation::new(
                    black_box(people.clone()),
                    black_box(benunits.clone()),
                    black_box(households.clone()),
                    black_box(reform_params.clone()),
                    year,
                ))
            })
        });
        g.finish();
    }

    // --- Group 2: the heavy components, for context -----------------------
    let baseline_results = baseline.run();
    let reform_results = baseline.branch(reform_params.clone()).run();
    {
        let mut g = c.benchmark_group(format!("components/{}hh", n));
        g.sample_size(30);
        g.bench_function("run", |b| b.iter(|| black_box(baseline.run())));
        g.bench_function("compare", |b| {
            b.iter(|| black_box(Comparison::between(&baseline_results, &reform_results)))
        });
        g.finish();
    }

    // --- Group 3: end-to-end — branch primitive vs the main.rs dance ------
    {
        let mut g = c.benchmark_group(format!("end_to_end/{}hh", n));
        g.sample_size(20);

        // New primitive: one baseline, branch it, run both, compare.
        g.bench_function("branch_pipeline", |b| {
            b.iter(|| {
                let baseline_sim = Simulation::new(
                    people.clone(),
                    benunits.clone(),
                    households.clone(),
                    baseline_params.clone(),
                    year,
                );
                let reform_sim = baseline_sim.branch(reform_params.clone());
                let br = baseline_sim.run();
                let rr = reform_sim.run();
                black_box(Comparison::between(&br, &rr))
            })
        });

        // Current main.rs sequence: two independent Simulations, each built by
        // cloning the dataset frames.
        g.bench_function("dance_pipeline", |b| {
            b.iter(|| {
                let baseline_sim = Simulation::new(
                    people.clone(),
                    benunits.clone(),
                    households.clone(),
                    baseline_params.clone(),
                    year,
                );
                let br = baseline_sim.run();
                let reform_sim = Simulation::new(
                    people.clone(),
                    benunits.clone(),
                    households.clone(),
                    reform_params.clone(),
                    year,
                );
                let rr = reform_sim.run();
                black_box(Comparison::between(&br, &rr))
            })
        });
        g.finish();
    }
}

criterion_group!(benches_group, benches);
criterion_main!(benches_group);
