use crate::engine::entities::{BenUnit, Household};
use crate::engine::simulation::{BenUnitResult, SimulationResults};
use crate::parameters::Parameters;
use std::path::Path;

/// A reform is a named bundle of policy changes applied on top of baseline law.
///
/// Today a reform can do two things:
/// 1. **Override parameters** — a YAML mapping that mirrors the parameter struct
///    (e.g. `income_tax: { personal_allowance: 20000.0 }`). Existing behaviour.
/// 2. **Neutralise benefits** — set named benefit outputs to zero on the reform
///    side, mirroring `policyengine_uk/reforms/policyengine/disable_simulated_benefits.py`.
///    The supported names are listed in `NEUTRALISABLE_BENEFITS`. Aggregate fields
///    (`total_benefits`, `net_income`, etc.) are recomputed by delta so the result
///    set stays internally consistent. State pension and `benefit_cap_reduction`
///    have cross-variable feedback and are out of scope for this slice.
///
/// # Example reform file (raise_pa.yaml):
///
/// ```yaml
/// income_tax:
///   personal_allowance: 20000.0
/// ```
///
/// # Example reform file with neutralisation (disable_means_tested.yaml):
///
/// ```yaml
/// neutralise:
///   - universal_credit
///   - housing_benefit
///   - pension_credit
/// income_tax:
///   personal_allowance: 20000.0
/// ```
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Reform {
    pub name: String,
    pub parameters: Parameters,
    /// Names of benefit outputs to zero on the reform side. Populated from a
    /// top-level `neutralise:` sequence in the reform YAML; defaults to empty.
    pub neutralise: Vec<String>,
}

/// Whitelist of benefit fields safe to neutralise via this slice.
///
/// Each entry maps a public-facing variable name to a `BenUnitResult` field
/// that flows additively into `total_benefits`. State pension and
/// `benefit_cap_reduction` are excluded because they have cross-variable
/// feedback (SP feeds income tax, the cap is an inverse adjustment).
pub const NEUTRALISABLE_BENEFITS: &[&str] = &[
    "universal_credit",
    "child_benefit",
    "pension_credit",
    "housing_benefit",
    "child_tax_credit",
    "working_tax_credit",
    "income_support",
    "esa_income_related",
    "jsa_income_based",
    "carers_allowance",
    "scottish_child_payment",
    "passthrough_benefits",
];

impl Reform {
    /// Create a reform by overlaying YAML parameter overrides onto baseline.
    ///
    /// A top-level `neutralise:` sequence is consumed before parameter merge,
    /// so it does not have to be a known parameter key.
    pub fn from_yaml(name: &str, yaml_str: &str, baseline: &Parameters) -> anyhow::Result<Self> {
        let mut value: serde_yaml::Value = serde_yaml::from_str(yaml_str)
            .unwrap_or(serde_yaml::Value::Null);
        let neutralise = pop_neutralise(&mut value)?;
        validate_neutralise(&neutralise)?;

        let cleaned_yaml = serde_yaml::to_string(&value)?;
        let parameters = baseline.apply_yaml_overlay(&cleaned_yaml)?;
        Ok(Reform {
            name: name.to_string(),
            parameters,
            neutralise,
        })
    }

    /// Load reform from a YAML file.
    pub fn from_file(path: &Path, baseline: &Parameters) -> anyhow::Result<Self> {
        let name = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("reform")
            .to_string();
        let contents = std::fs::read_to_string(path)?;
        Self::from_yaml(&name, &contents, baseline)
    }

    /// Convenience: create the "PA to £20k" reform.
    pub fn personal_allowance_20k(baseline: &Parameters) -> Self {
        let yaml = "income_tax:\n  personal_allowance: 20000.0\n";
        Self::from_yaml("Personal Allowance to £20,000", yaml, baseline).unwrap()
    }

    /// Apply the reform's neutralisation list to a freshly-computed result set.
    ///
    /// For each variable in `neutralise`, zeros the corresponding `BenUnitResult`
    /// field and decrements the household-level aggregates (`total_benefits`,
    /// `net_income`, `net_income_ahc`, `equivalised_*`, `extended_net_income`)
    /// by the same amount. Does not re-run the benefit cap, UC taper, or any
    /// upstream computation — neutralisation is a post-calc zeroing, matching
    /// Python's `disable_simulated_benefits` reform.
    ///
    /// No-op when `neutralise` is empty.
    pub fn apply_to_results(
        &self,
        results: &mut SimulationResults,
        benunits: &[BenUnit],
        households: &[Household],
    ) {
        if self.neutralise.is_empty() {
            return;
        }

        // Per-benunit delta = sum of values in named fields *before* zeroing.
        // We zero in the same pass so the BenUnitResult state stays consistent.
        let mut bu_delta = vec![0.0_f64; benunits.len()];
        for (bid, br) in results.benunit_results.iter_mut().enumerate() {
            for name in &self.neutralise {
                bu_delta[bid] += zero_benunit_field(br, name);
            }
            br.total_benefits -= bu_delta[bid];
        }

        // Per-household delta = sum of member-benunit deltas. Update the four
        // benefit-dependent income measures and their equivalised counterparts.
        for (hid, hr) in results.household_results.iter_mut().enumerate() {
            let hh = &households[hid];
            let delta: f64 = hh.benunit_ids.iter().map(|&bid| bu_delta[bid]).sum();
            if delta == 0.0 {
                continue;
            }
            hr.total_benefits -= delta;
            hr.net_income -= delta;
            hr.net_income_ahc -= delta;
            hr.extended_net_income -= delta;
            let eq = hr.equivalisation_factor.max(1e-9);
            hr.equivalised_net_income = hr.net_income / eq;
            hr.equivalised_net_income_ahc = hr.net_income_ahc / eq;
        }
    }
}

/// Pop the `neutralise` sequence out of a YAML value, returning the names found.
fn pop_neutralise(value: &mut serde_yaml::Value) -> anyhow::Result<Vec<String>> {
    let serde_yaml::Value::Mapping(map) = value else {
        return Ok(Vec::new());
    };
    let key = serde_yaml::Value::String("neutralise".to_string());
    let Some(raw) = map.remove(&key) else {
        return Ok(Vec::new());
    };
    let names: Vec<String> = serde_yaml::from_value(raw)
        .map_err(|e| anyhow::anyhow!("`neutralise` must be a list of strings: {e}"))?;
    Ok(names)
}

fn validate_neutralise(names: &[String]) -> anyhow::Result<()> {
    for name in names {
        if !NEUTRALISABLE_BENEFITS.contains(&name.as_str()) {
            anyhow::bail!(
                "Cannot neutralise `{name}` — supported variables are: {}",
                NEUTRALISABLE_BENEFITS.join(", "),
            );
        }
    }
    Ok(())
}

/// Zero a benefit field on `BenUnitResult` and return the previous value.
/// Caller is responsible for adjusting `total_benefits` and household aggregates.
fn zero_benunit_field(br: &mut BenUnitResult, name: &str) -> f64 {
    let prev = match name {
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
        // Validated in `validate_neutralise`; unreachable in practice.
        _ => return 0.0,
    };
    let old = *prev;
    *prev = 0.0;
    old
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::entities::{BenUnit, Household};
    use crate::engine::simulation::{BenUnitResult, HouseholdResult, PersonResult};
    use crate::parameters::Parameters;

    fn baseline() -> Parameters {
        Parameters::for_year(2025).unwrap()
    }

    #[test]
    fn parses_yaml_without_neutralise() {
        let r = Reform::from_yaml(
            "x",
            "income_tax:\n  personal_allowance: 20000.0\n",
            &baseline(),
        )
        .unwrap();
        assert!(r.neutralise.is_empty());
        assert!((r.parameters.income_tax.personal_allowance - 20_000.0).abs() < 1e-6);
    }

    #[test]
    fn parses_yaml_with_neutralise_only() {
        let r = Reform::from_yaml(
            "x",
            "neutralise:\n  - universal_credit\n  - housing_benefit\n",
            &baseline(),
        )
        .unwrap();
        assert_eq!(r.neutralise, vec!["universal_credit", "housing_benefit"]);
    }

    #[test]
    fn parses_yaml_with_both() {
        let r = Reform::from_yaml(
            "x",
            "neutralise: [universal_credit]\nincome_tax:\n  personal_allowance: 20000.0\n",
            &baseline(),
        )
        .unwrap();
        assert_eq!(r.neutralise, vec!["universal_credit"]);
        assert!((r.parameters.income_tax.personal_allowance - 20_000.0).abs() < 1e-6);
    }

    #[test]
    fn rejects_unknown_neutralise_target() {
        let err = Reform::from_yaml("x", "neutralise: [income_tax]\n", &baseline())
            .unwrap_err()
            .to_string();
        assert!(err.contains("Cannot neutralise"));
    }

    #[test]
    fn neutralise_is_no_op_when_empty() {
        let mut results = single_hh_results();
        let before = results.clone();
        let r = Reform {
            name: "noop".into(),
            parameters: baseline(),
            neutralise: Vec::new(),
        };
        r.apply_to_results(&mut results, &[BenUnit::default()], &[Household::default()]);
        assert_eq!(results.benunit_results[0].total_benefits, before.benunit_results[0].total_benefits);
        assert_eq!(results.household_results[0].net_income, before.household_results[0].net_income);
    }

    #[test]
    fn neutralise_zeros_named_field_and_updates_aggregates() {
        let mut results = single_hh_results();
        // Sanity: pre-state matches the synthetic fixture.
        assert!((results.benunit_results[0].universal_credit - 6_000.0).abs() < 1e-9);
        assert!((results.benunit_results[0].total_benefits - 8_500.0).abs() < 1e-9);
        assert!((results.household_results[0].net_income - 30_000.0).abs() < 1e-9);

        let r = Reform {
            name: "no_uc".into(),
            parameters: baseline(),
            neutralise: vec!["universal_credit".into()],
        };
        let bu = {
            let mut b = BenUnit::default();
            b.id = 0;
            b.household_id = 0;
            b
        };
        let hh = {
            let mut h = Household::default();
            h.id = 0;
            h.benunit_ids = vec![0];
            h
        };
        r.apply_to_results(&mut results, &[bu], &[hh]);

        assert_eq!(results.benunit_results[0].universal_credit, 0.0);
        assert!((results.benunit_results[0].total_benefits - 2_500.0).abs() < 1e-9);
        assert!((results.household_results[0].total_benefits - 2_500.0).abs() < 1e-9);
        assert!((results.household_results[0].net_income - 24_000.0).abs() < 1e-9);
        assert!((results.household_results[0].net_income_ahc - 19_000.0).abs() < 1e-9);
        assert!((results.household_results[0].extended_net_income - 23_500.0).abs() < 1e-9);
        // equivalised = net / eq_factor
        let eq = results.household_results[0].equivalisation_factor;
        assert!((results.household_results[0].equivalised_net_income - 24_000.0 / eq).abs() < 1e-6);
    }

    /// One-household, one-benunit fixture with a non-trivial UC + housing benefit.
    fn single_hh_results() -> SimulationResults {
        let mut br = BenUnitResult::default();
        br.universal_credit = 6_000.0;
        br.housing_benefit = 2_500.0;
        br.total_benefits = 8_500.0;

        let mut hr = HouseholdResult::default();
        hr.gross_income = 35_000.0;
        hr.total_tax = 13_500.0;
        hr.total_benefits = 8_500.0;
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
}
