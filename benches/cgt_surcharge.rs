//! Speedtest for the CGT residential-property surcharge (PR #64 / #45).
//!
//! The surcharge change is purely arithmetic inside `calculate_capital_gains_tax`:
//! after the annual exempt amount, split taxable gains into residential /
//! non-residential and apply the surcharge to the residential slice. The split
//! runs on every call regardless of the residential share, so the question is
//! (a) does the surcharge path cost more than the plain path, and (b) how much
//! does the whole CGT pass cost over a population — it's called once per person
//! in `run()`'s Phase 2c.
//!
//! Groups:
//!   * `per_call` — the function across the four shapes (no gain / non-resi /
//!                  mixed / full residential). All should be the same handful
//!                  of nanoseconds: the surcharge adds no branch, just a clamp
//!                  and a couple of multiplies.
//!   * `population` — the function looped over N persons, mimicking Phase 2c,
//!                    to size the whole CGT pass in absolute terms.
//!
//! Self-contained (scale via `CGT_BENCH_N`, default 20_000).
//! Run: `cargo bench --bench cgt_surcharge`.

// Binary-only crate (no lib target): pull modules in by path, as the other
// benches do.
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

use engine::entities::Person;
use parameters::CapitalGainsTaxParams;
use variables::wealth_taxes::calculate_capital_gains_tax;

/// 2023/24-style residential surcharge: higher rate 20%, surcharge 8pp → 28%.
fn surcharge_params() -> CapitalGainsTaxParams {
    CapitalGainsTaxParams {
        annual_exempt_amount: 3_000.0,
        basic_rate: 0.10,
        higher_rate: 0.20,
        residential_surcharge: 0.08,
    }
}

fn person_with(gains: f64, residential_share: f64) -> Person {
    let mut p = Person::default();
    p.capital_gains = gains;
    p.capital_gains_residential_share = residential_share;
    p
}

fn n() -> usize {
    std::env::var("CGT_BENCH_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000)
}

fn benches(c: &mut Criterion) {
    let params = surcharge_params();

    // --- Group 1: per-call across the four shapes ------------------------
    {
        let no_gain = person_with(2_000.0, 0.0); // below AEA → early return
        let non_resi = person_with(8_000.0, 0.0); // all non-residential
        let mixed = person_with(8_000.0, 0.4); // 40% residential
        let full = person_with(8_000.0, 1.0); // all residential (surcharge on all)

        let mut g = c.benchmark_group("per_call");
        g.bench_function("no_gain", |b| {
            b.iter(|| black_box(calculate_capital_gains_tax(black_box(&no_gain), black_box(&params), true)))
        });
        g.bench_function("non_residential", |b| {
            b.iter(|| black_box(calculate_capital_gains_tax(black_box(&non_resi), black_box(&params), true)))
        });
        g.bench_function("residential_mixed", |b| {
            b.iter(|| black_box(calculate_capital_gains_tax(black_box(&mixed), black_box(&params), true)))
        });
        g.bench_function("residential_full", |b| {
            b.iter(|| black_box(calculate_capital_gains_tax(black_box(&full), black_box(&params), true)))
        });
        g.finish();
    }

    // --- Group 2: the whole CGT pass over a population -------------------
    {
        let n = n();
        // Deterministic spread of gains and residential shares.
        let people: Vec<Person> = (0..n)
            .map(|i| person_with(((i * 211) % 30_000) as f64, ((i % 11) as f64) / 10.0))
            .collect();

        let mut g = c.benchmark_group(format!("population/{}", n));
        g.bench_function("cgt_pass", |b| {
            b.iter(|| {
                let mut total = 0.0;
                for p in black_box(&people) {
                    total += calculate_capital_gains_tax(p, black_box(&params), true);
                }
                black_box(total)
            })
        });
        g.finish();
    }
}

criterion_group!(benches_group, benches);
criterion_main!(benches_group);
