//! Speedtest for the `ParameterTree` tree-walking read path (PR #67 / #50).
//!
//! `ParameterTree` is a *parallel* read path: it parses a year's parameter YAML
//! into a generic `serde_yaml::Value` tree and looks values up by dot-path,
//! rather than into the hand-coded `Parameters` struct. Two timing questions
//! decide whether that's safe to add:
//!
//!   * `load`   — does building the tree cost more than the existing struct
//!                load? (Both read + parse the same YAML file.)
//!   * `lookup` — how much slower is a dot-path tree walk than a direct struct
//!                field access? This is the one that matters: the tree walk
//!                splits a string and does mapping lookups, so it must NOT be
//!                used in a hot per-entity loop — only for setup/config reads.
//!
//! The numbers let a reviewer confirm the read path is fine for its intended
//! use (load-once, occasional lookup) and quantify the cost if someone is
//! tempted to call it in the engine's inner loop.
//!
//! Run: `cargo bench --bench parameter_tree`

// Binary-only crate (no lib target), so pull the self-contained parameters
// module in by path, exactly as tests/parameter_impact.rs does for others.
#[path = "../src/parameters/mod.rs"]
mod parameters;

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use parameters::{ParameterTree, Parameters};

fn benches(c: &mut Criterion) {
    let year = 2025u32;

    // --- Group 1: load-time (read file + parse) ---------------------------
    {
        let mut g = c.benchmark_group("load");
        // Existing path: YAML -> Parameters struct.
        g.bench_function("Parameters::for_year", |b| {
            b.iter(|| black_box(Parameters::for_year(black_box(year)).unwrap()))
        });
        // New path: YAML -> generic Value tree.
        g.bench_function("ParameterTree::for_year", |b| {
            b.iter(|| black_box(ParameterTree::for_year(black_box(year)).unwrap()))
        });
        g.finish();
    }

    // --- Group 2: per-lookup (tree walk vs struct field access) -----------
    {
        let params = Parameters::for_year(year).unwrap();
        let tree = ParameterTree::for_year(year).unwrap();

        let mut g = c.benchmark_group("lookup");
        // Direct struct field access — the baseline the engine uses today.
        g.bench_function("struct_field", |b| {
            b.iter(|| black_box(black_box(&params).income_tax.personal_allowance))
        });
        // Scalar dot-path lookup.
        g.bench_function("tree_scalar", |b| {
            b.iter(|| black_box(black_box(&tree).lookup_f64("income_tax.personal_allowance")))
        });
        // Sequence-indexed dot-path lookup (deeper walk: key -> seq -> index -> key).
        g.bench_function("tree_indexed", |b| {
            b.iter(|| black_box(black_box(&tree).lookup_f64("income_tax.uk_brackets[1].threshold")))
        });
        g.finish();
    }
}

criterion_group!(benches_group, benches);
criterion_main!(benches_group);
