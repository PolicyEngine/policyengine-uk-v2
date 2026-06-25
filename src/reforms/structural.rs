//! Structural reforms: a Rust reform abstraction that goes beyond the YAML
//! parameter overlay implemented by [`crate::reforms::Reform`].
//!
//! This mirrors the structural half of PolicyEngine's Python `Reform` system
//! (issue #41). Where the Python side rewrites variable *classes*, the Rust
//! engine computes everything in a fixed pipeline (`Simulation::run`), so the
//! least-invasive integration is a **post-compute transform keyed by variable
//! name**, applied to the finalised [`SimulationResults`] on the reform side
//! only (the baseline run is untouched). This is the same hook the existing
//! [`Reform::apply_to_results`](crate::reforms::Reform::apply_to_results)
//! neutralisation uses.
//!
//! A [`StructuralReform`] can:
//!
//! 1. **Parameter override** — wrap a parameter-overlay [`Reform`] so the
//!    existing behaviour composes with structural changes. (The parameter
//!    overlay still flows through `Simulation::run`; this variant only carries
//!    the reform so its `neutralise` list is applied alongside structural ops.)
//! 2. **Neutralise** a named benefit variable (force its output to zero
//!    everywhere), keeping household aggregates consistent by delta.
//! 3. **Override / replace** a named benefit variable's computed output with a
//!    custom closure `fn(old_value, &BenUnit) -> new_value`. This is the
//!    output-level analogue of replacing a `compute_*` formula.
//! 4. **Compose** any number of the above, applied in sequence.
//!
//! # Scope note (issue #41)
//!
//! Full mid-pipeline formula replacement (swapping the body of a `compute_*`
//! function *before* downstream variables read it) is intentionally out of
//! scope for this slice — see the PR's **Remaining** section. Output-override
//! covers the common case where a reform changes a leaf benefit/credit and the
//! delta can be reconciled into the aggregates afterwards.

// This is a public reform API surface (mirroring the Python `Reform` system).
// `main.rs` does not yet route a CLI flag into it, so the items read as unused
// to the binary's dead-code analysis; the unit tests exercise every path.
#![allow(dead_code)]

use crate::engine::entities::{BenUnit, Household};
use crate::engine::simulation::{BenUnitResult, SimulationResults};
use crate::reforms::{Reform, NEUTRALISABLE_BENEFITS};

/// A closure that maps a benefit unit's current output value for a variable to
/// a new value, given read-only access to the benefit unit. Used by
/// [`StructuralReform::OverrideOutput`].
pub type OutputOverrideFn = std::sync::Arc<dyn Fn(f64, &BenUnit) -> f64 + Send + Sync>;

/// A structural reform: a transform applied to finalised reform-side results.
///
/// Variants are additive and compose via [`StructuralReform::Compose`]. Every
/// variant is a no-op-safe post-compute transform; none of them touch the
/// baseline run.
#[derive(Clone)]
pub enum StructuralReform {
    /// Carry an existing parameter-overlay [`Reform`]. The parameter overlay is
    /// applied by `Simulation::run` (via `reform.parameters`); here we only
    /// re-apply the reform's `neutralise` list so the two paths compose.
    Parametric(Reform),
    /// Zero the named benefit variables everywhere, reconciling aggregates.
    /// Names must be members of [`NEUTRALISABLE_BENEFITS`].
    Neutralise(Vec<String>),
    /// Replace a named benefit variable's per-benunit output with the result of
    /// a closure, reconciling aggregates by the delta.
    OverrideOutput {
        /// Variable name; must be a member of [`NEUTRALISABLE_BENEFITS`].
        variable: String,
        /// `fn(old_value, &BenUnit) -> new_value`.
        formula: OutputOverrideFn,
    },
    /// Apply a sequence of structural reforms in order.
    Compose(Vec<StructuralReform>),
}

impl std::fmt::Debug for StructuralReform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StructuralReform::Parametric(r) => f.debug_tuple("Parametric").field(&r.name).finish(),
            StructuralReform::Neutralise(v) => f.debug_tuple("Neutralise").field(v).finish(),
            StructuralReform::OverrideOutput { variable, .. } => f
                .debug_struct("OverrideOutput")
                .field("variable", variable)
                .field("formula", &"<closure>")
                .finish(),
            StructuralReform::Compose(v) => f.debug_tuple("Compose").field(v).finish(),
        }
    }
}

impl StructuralReform {
    /// Neutralise a single named benefit variable.
    pub fn neutralise(variable: &str) -> Self {
        StructuralReform::Neutralise(vec![variable.to_string()])
    }

    /// Override a named benefit variable's output with a closure.
    pub fn override_output<F>(variable: &str, formula: F) -> Self
    where
        F: Fn(f64, &BenUnit) -> f64 + Send + Sync + 'static,
    {
        StructuralReform::OverrideOutput {
            variable: variable.to_string(),
            formula: std::sync::Arc::new(formula),
        }
    }

    /// Compose this reform with another, applied after this one.
    pub fn then(self, next: StructuralReform) -> Self {
        match self {
            StructuralReform::Compose(mut v) => {
                v.push(next);
                StructuralReform::Compose(v)
            }
            first => StructuralReform::Compose(vec![first, next]),
        }
    }

    /// Validate that all referenced variable names are supported. Returns the
    /// first offending name, if any.
    pub fn validate(&self) -> anyhow::Result<()> {
        match self {
            StructuralReform::Parametric(_) => Ok(()),
            StructuralReform::Neutralise(names) => {
                for name in names {
                    check_supported(name)?;
                }
                Ok(())
            }
            StructuralReform::OverrideOutput { variable, .. } => check_supported(variable),
            StructuralReform::Compose(reforms) => {
                for r in reforms {
                    r.validate()?;
                }
                Ok(())
            }
        }
    }

    /// Apply this structural reform to a finalised reform-side result set.
    ///
    /// Each variant computes a per-benunit delta against the named field,
    /// applies it, and reconciles the benunit `total_benefits` and the
    /// household income aggregates (`total_benefits`, `net_income`,
    /// `net_income_ahc`, `extended_net_income`, `equivalised_*`) by the same
    /// delta. This matches the reconciliation done by
    /// [`Reform::apply_to_results`].
    ///
    /// No-op when there is nothing to change.
    pub fn apply_to_results(
        &self,
        results: &mut SimulationResults,
        benunits: &[BenUnit],
        households: &[Household],
    ) {
        match self {
            StructuralReform::Parametric(reform) => {
                // Parameter overlay already applied upstream by `Simulation::run`;
                // re-apply the reform's neutralise list so both paths compose.
                reform.apply_to_results(results, benunits, households);
            }
            StructuralReform::Neutralise(names) => {
                // delta_for(old) = old  →  new value is 0, removed amount is `old`.
                self.transform_fields(results, benunits, households, |_| names.clone(), |old, _| {
                    -old // new - old = 0 - old
                });
            }
            StructuralReform::OverrideOutput { variable, formula } => {
                let var = variable.clone();
                self.transform_fields(
                    results,
                    benunits,
                    households,
                    |_| vec![var.clone()],
                    |old, bu| formula(old, bu) - old,
                );
            }
            StructuralReform::Compose(reforms) => {
                for r in reforms {
                    r.apply_to_results(results, benunits, households);
                }
            }
        }
    }

    /// Shared engine for the field-mutating variants.
    ///
    /// `fields(bu)` selects which variable names to touch for a benunit (lets
    /// the caller share one code path for neutralise and override). `delta(old,
    /// bu)` returns `new_value - old_value` for a field; the field is set to
    /// `old + delta` and the aggregates are decremented by `-delta` (i.e. a
    /// reduction in a benefit reduces income).
    fn transform_fields<Fsel, Fdelta>(
        &self,
        results: &mut SimulationResults,
        benunits: &[BenUnit],
        households: &[Household],
        fields: Fsel,
        delta: Fdelta,
    ) where
        Fsel: Fn(&BenUnit) -> Vec<String>,
        Fdelta: Fn(f64, &BenUnit) -> f64,
    {
        let mut bu_delta = vec![0.0_f64; benunits.len()];
        for (bid, br) in results.benunit_results.iter_mut().enumerate() {
            let bu = &benunits[bid];
            for name in fields(bu) {
                if let Some(field) = benunit_field_mut(br, &name) {
                    let old = *field;
                    let d = delta(old, bu);
                    if d == 0.0 {
                        continue;
                    }
                    *field = old + d;
                    bu_delta[bid] += d;
                }
            }
            br.total_benefits += bu_delta[bid];
        }

        for (hid, hr) in results.household_results.iter_mut().enumerate() {
            let hh = &households[hid];
            let d: f64 = hh.benunit_ids.iter().map(|&bid| bu_delta[bid]).sum();
            if d == 0.0 {
                continue;
            }
            hr.total_benefits += d;
            hr.net_income += d;
            hr.net_income_ahc += d;
            hr.extended_net_income += d;
            let eq = hr.equivalisation_factor.max(1e-9);
            hr.equivalised_net_income = hr.net_income / eq;
            hr.equivalised_net_income_ahc = hr.net_income_ahc / eq;
        }
    }
}

fn check_supported(name: &str) -> anyhow::Result<()> {
    if !NEUTRALISABLE_BENEFITS.contains(&name) {
        anyhow::bail!(
            "Structural reform cannot target `{name}` — supported variables are: {}",
            NEUTRALISABLE_BENEFITS.join(", "),
        );
    }
    Ok(())
}

/// Mutable accessor for a named benefit field on [`BenUnitResult`]. Returns
/// `None` for names outside [`NEUTRALISABLE_BENEFITS`]. This is the structural
/// twin of the private `zero_benunit_field` helper in the parent module, but
/// returns the borrow so callers can both read and write.
fn benunit_field_mut<'a>(br: &'a mut BenUnitResult, name: &str) -> Option<&'a mut f64> {
    Some(match name {
        "universal_credit" => &mut br.universal_credit,
        "child_benefit" => &mut br.child_benefit,
        "pension_credit" => &mut br.pension_credit,
        "housing_benefit" => &mut br.housing_benefit,
        "child_tax_credit" => &mut br.child_tax_credit,
        "working_tax_credit" => &mut br.working_tax_credit,
        "income_support" => &mut br.income_support,
        "esa_income_related" => &mut br.esa_income_related,
        "jsa_income_based" => &mut br.jsa_income_based,
        "carers_allowance" => &mut br.carers_allowance,
        "scottish_child_payment" => &mut br.scottish_child_payment,
        "passthrough_benefits" => &mut br.passthrough_benefits,
        _ => return None,
    })
}

/// A small registry of named example structural reforms, analogous to the
/// Python `policyengine_uk/reforms/` examples referenced in issue #41.
pub mod registry {
    use super::StructuralReform;

    /// Neutralise the main simulated means-tested benefits, mirroring
    /// `policyengine_uk/reforms/policyengine/disable_simulated_benefits.py`.
    pub fn disable_simulated_benefits() -> StructuralReform {
        StructuralReform::Neutralise(vec![
            "universal_credit".into(),
            "pension_credit".into(),
            "housing_benefit".into(),
            "child_tax_credit".into(),
            "working_tax_credit".into(),
            "income_support".into(),
            "esa_income_related".into(),
            "jsa_income_based".into(),
        ])
    }

    /// Neutralise a single named benefit (e.g. `"universal_credit"`).
    pub fn neutralise_benefit(variable: &str) -> StructuralReform {
        StructuralReform::neutralise(variable)
    }

    /// Halve child benefit for every benefit unit — an output-override example
    /// analogous to a structural rewrite of a leaf benefit.
    pub fn halve_child_benefit() -> StructuralReform {
        StructuralReform::override_output("child_benefit", |old, _bu| old * 0.5)
    }

    /// Look up an example reform by slug. Returns `None` for unknown slugs.
    pub fn by_name(slug: &str) -> Option<StructuralReform> {
        match slug {
            "disable_simulated_benefits" => Some(disable_simulated_benefits()),
            "halve_child_benefit" => Some(halve_child_benefit()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::registry;
    use super::*;
    use crate::engine::simulation::{HouseholdResult, PersonResult};

    fn bu(id: usize) -> BenUnit {
        let mut b = BenUnit::default();
        b.id = id;
        b.household_id = 0;
        b
    }

    fn hh(benunit_ids: Vec<usize>) -> Household {
        let mut h = Household::default();
        h.id = 0;
        h.benunit_ids = benunit_ids;
        h
    }

    /// One-household, one-benunit fixture: UC 6000 + housing 2500 + child benefit
    /// 1000 = 9500 total benefits; net income 30000.
    fn fixture() -> SimulationResults {
        let mut br = BenUnitResult::default();
        br.universal_credit = 6_000.0;
        br.housing_benefit = 2_500.0;
        br.child_benefit = 1_000.0;
        br.total_benefits = 9_500.0;

        let mut hr = HouseholdResult::default();
        hr.total_benefits = 9_500.0;
        hr.net_income = 30_000.0;
        hr.net_income_ahc = 25_000.0;
        hr.extended_net_income = 29_500.0;
        hr.equivalisation_factor = 1.0;
        hr.equivalised_net_income = 30_000.0;
        hr.equivalised_net_income_ahc = 25_000.0;

        SimulationResults {
            person_results: vec![PersonResult::default()],
            benunit_results: vec![br],
            household_results: vec![hr],
        }
    }

    #[test]
    fn neutralise_zeros_variable_and_reconciles_aggregates() {
        let mut r = fixture();
        let reform = StructuralReform::neutralise("universal_credit");
        reform.validate().unwrap();
        reform.apply_to_results(&mut r, &[bu(0)], &[hh(vec![0])]);

        assert_eq!(r.benunit_results[0].universal_credit, 0.0);
        assert!((r.benunit_results[0].total_benefits - 3_500.0).abs() < 1e-9);
        assert!((r.household_results[0].total_benefits - 3_500.0).abs() < 1e-9);
        assert!((r.household_results[0].net_income - 24_000.0).abs() < 1e-9);
        assert!((r.household_results[0].extended_net_income - 23_500.0).abs() < 1e-9);
    }

    #[test]
    fn override_output_applies_closure_and_reconciles() {
        let mut r = fixture();
        // Halve child benefit: 1000 -> 500, delta -500.
        let reform = registry::halve_child_benefit();
        reform.apply_to_results(&mut r, &[bu(0)], &[hh(vec![0])]);

        assert!((r.benunit_results[0].child_benefit - 500.0).abs() < 1e-9);
        assert!((r.benunit_results[0].total_benefits - 9_000.0).abs() < 1e-9);
        assert!((r.household_results[0].net_income - 29_500.0).abs() < 1e-9);
    }

    #[test]
    fn override_output_can_increase_a_benefit() {
        let mut r = fixture();
        // Double UC: 6000 -> 12000, delta +6000.
        let reform = StructuralReform::override_output("universal_credit", |old, _| old * 2.0);
        reform.apply_to_results(&mut r, &[bu(0)], &[hh(vec![0])]);

        assert!((r.benunit_results[0].universal_credit - 12_000.0).abs() < 1e-9);
        assert!((r.household_results[0].net_income - 36_000.0).abs() < 1e-9);
    }

    #[test]
    fn compose_applies_both_reforms_in_sequence() {
        let mut r = fixture();
        // Neutralise housing benefit (-2500) then halve child benefit (-500).
        let reform = StructuralReform::neutralise("housing_benefit")
            .then(registry::halve_child_benefit());
        reform.apply_to_results(&mut r, &[bu(0)], &[hh(vec![0])]);

        assert_eq!(r.benunit_results[0].housing_benefit, 0.0);
        assert!((r.benunit_results[0].child_benefit - 500.0).abs() < 1e-9);
        // 9500 - 2500 - 500 = 6500
        assert!((r.benunit_results[0].total_benefits - 6_500.0).abs() < 1e-9);
        assert!((r.household_results[0].net_income - 27_000.0).abs() < 1e-9);
    }

    #[test]
    fn compose_flattens_into_single_compose() {
        let r = StructuralReform::neutralise("universal_credit")
            .then(StructuralReform::neutralise("housing_benefit"))
            .then(StructuralReform::neutralise("pension_credit"));
        match r {
            StructuralReform::Compose(v) => assert_eq!(v.len(), 3),
            other => panic!("expected Compose, got {other:?}"),
        }
    }

    #[test]
    fn disable_simulated_benefits_zeros_means_tested() {
        let mut r = fixture();
        registry::disable_simulated_benefits().apply_to_results(&mut r, &[bu(0)], &[hh(vec![0])]);
        assert_eq!(r.benunit_results[0].universal_credit, 0.0);
        assert_eq!(r.benunit_results[0].housing_benefit, 0.0);
        // Child benefit is contributory-style, not in the means-tested list.
        assert!((r.benunit_results[0].child_benefit - 1_000.0).abs() < 1e-9);
        // total = 9500 - 6000 - 2500 = 1000 (child benefit only)
        assert!((r.benunit_results[0].total_benefits - 1_000.0).abs() < 1e-9);
    }

    #[test]
    fn validate_rejects_unsupported_variable() {
        let err = StructuralReform::neutralise("income_tax")
            .validate()
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot target"));
    }

    #[test]
    fn validate_recurses_into_compose() {
        let reform = StructuralReform::neutralise("universal_credit")
            .then(StructuralReform::neutralise("not_a_variable"));
        assert!(reform.validate().is_err());
    }

    #[test]
    fn parametric_variant_applies_reforms_neutralise_list() {
        // The Parametric variant carries a parameter-overlay Reform; applying it
        // re-runs that reform's neutralise list so the two paths compose.
        let baseline = Parameters::for_year(2025).unwrap();
        let reform = Reform {
            name: "p".into(),
            parameters: baseline,
            neutralise: vec!["universal_credit".into()],
        };
        let mut r = fixture();
        StructuralReform::Parametric(reform).apply_to_results(&mut r, &[bu(0)], &[hh(vec![0])]);
        assert_eq!(r.benunit_results[0].universal_credit, 0.0);
    }

    use crate::parameters::Parameters;

    #[test]
    fn registry_lookup_by_name() {
        assert!(registry::by_name("disable_simulated_benefits").is_some());
        assert!(registry::by_name("halve_child_benefit").is_some());
        assert!(registry::by_name("nope").is_none());
    }
}
