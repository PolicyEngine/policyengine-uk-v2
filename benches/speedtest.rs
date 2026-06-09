//! Speedtest: how long does it take to run the engine over 100k households?
//!
//! This is the bench behind the `Speedtest` GitHub Action (issue #69), which
//! reports how the time to simulate 100k households changes in a PR. It is a
//! plain `harness = false` binary (no Criterion ceremony) so the output is a
//! single, easily-parsed line that the workflow can diff between the PR head
//! and the base commit.
//!
//! The household frame is synthetic — single-adult households spread across the
//! basic / higher / additional rate bands — so the bench is self-contained and
//! needs no FRS data (which is absent on CI runners). This mirrors the
//! `make_frame` helper in `branch.rs`.
//!
//! Configure with env vars:
//!   * `SPEEDTEST_HH`   — household count (default 100_000)
//!   * `SPEEDTEST_RUNS` — timed iterations; the median is reported (default 20)
//!
//! Output (stdout): one machine-readable marker line, e.g.
//!   SPEEDTEST_JSON={"households":100000,"runs":20,"median_ms":83.41,"mean_ms":84.02,"households_per_sec":1198896}
//!
//! Run: `cargo bench --bench speedtest`
//!      `SPEEDTEST_HH=50000 cargo bench --bench speedtest`

// The crate is binary-only (no lib target), so pull the engine modules in by
// path, exactly as `benches/branch.rs` and `tests/parameter_impact.rs` do.
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

use std::hint::black_box;
use std::time::Instant;

use engine::entities::*;
use engine::simulation::Simulation;
use parameters::Parameters;

/// Build `n` single-adult households spanning the basic / higher / additional
/// rate bands. Deterministic (no RNG) so successive runs are comparable.
/// Mirrors `make_frame` in `benches/branch.rs`.
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

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let n = env_usize("SPEEDTEST_HH", 100_000);
    let runs = env_usize("SPEEDTEST_RUNS", 20).max(1);
    let year = 2025u32;

    let params = Parameters::for_year(year).expect("2025 params");
    let (people, benunits, households) = make_frame(n);

    let sim = Simulation::new(
        people,
        benunits,
        households,
        params,
        year,
    );

    // Warm up (allocator, rayon thread pool, caches) so the first timed run
    // isn't an outlier.
    black_box(sim.run());

    let mut times_ms: Vec<f64> = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        black_box(sim.run());
        times_ms.push(start.elapsed().as_secs_f64() * 1_000.0);
    }

    times_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_ms = times_ms[times_ms.len() / 2];
    let mean_ms = times_ms.iter().sum::<f64>() / times_ms.len() as f64;
    let households_per_sec = (n as f64 / (median_ms / 1_000.0)).round() as u64;

    eprintln!(
        "speedtest: {} households, {} runs — median {:.2} ms, mean {:.2} ms ({} hh/s)",
        n, runs, median_ms, mean_ms, households_per_sec
    );
    println!(
        "SPEEDTEST_JSON={{\"households\":{},\"runs\":{},\"median_ms\":{:.2},\"mean_ms\":{:.2},\"households_per_sec\":{}}}",
        n, runs, median_ms, mean_ms, households_per_sec
    );
}
