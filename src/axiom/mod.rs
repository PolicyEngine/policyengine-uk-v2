//! Axiom rules-engine backend.
//!
//! Evaluates compiled RuleSpec artifacts (composed from rulespec-uk via
//! axiom-compose) over simulation populations, replacing hand-coded variable
//! logic with statute-derived rules. Behavioural responses and dataset
//! handling stay in the Rust layer; axiom owns the rules.
//!
//! Core flow:
//!   * [`Policy`] wraps a compiled artifact. Reforms and uprating assumptions
//!     are expressed as parameter overrides via [`Policy::with_parameter`] —
//!     clone the program, patch the versioned parameter table, recompile in
//!     memory. Future-dated overrides model projected uprating.
//!   * [`Dataset`] holds per-person input columns for one tax year.
//!   * [`calculate`] runs the engine's vectorised fast path and returns
//!     output columns aligned to person order.

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Context, Result};
use axiom_rules_engine::api::{
    execute_compiled_request, CompiledExecutionRequest, ExecutionMode, ExecutionQuery, OutputValue,
};
use axiom_rules_engine::compile::CompiledProgramArtifact;
use axiom_rules_engine::spec::{
    DatasetSpec, InputRecordSpec, IntervalSpec, ParameterVersionSpec, PeriodKindSpec, PeriodSpec,
    ScalarValueSpec,
};
use chrono::NaiveDate;

/// A compiled rule system: baseline law, or baseline plus parameter overrides.
#[derive(Clone)]
pub struct Policy {
    artifact: CompiledProgramArtifact,
}

impl Policy {
    pub fn from_artifact_json(json: &str) -> Result<Self> {
        let artifact = CompiledProgramArtifact::from_json_str(json)
            .map_err(|e| anyhow!("failed to load axiom artifact: {e}"))?;
        Ok(Policy { artifact })
    }

    /// New policy with `parameter_id` (full legal id, e.g.
    /// `uk:statutes/ukpga/2026/11/2#basic_rate`) set to `value` from
    /// `effective_from`. Existing versions before that date are untouched, so
    /// a future-dated override doubles as an uprating assumption.
    pub fn with_parameter(
        &self,
        parameter_id: &str,
        effective_from: NaiveDate,
        value: f64,
    ) -> Result<Self> {
        let mut program = self.artifact.program.clone();
        let parameter = program
            .parameters
            .iter_mut()
            .find(|p| p.id.as_deref() == Some(parameter_id))
            .ok_or_else(|| {
                anyhow!(
                    "unknown parameter {parameter_id}; available: {}",
                    program_parameter_ids(&self.artifact)
                )
            })?;
        if parameter.indexed_by.is_some() {
            bail!("indexed parameter overrides are not supported yet: {parameter_id}");
        }
        parameter.versions.retain(|v| v.effective_from != effective_from);
        parameter.versions.push(ParameterVersionSpec {
            effective_from,
            values: BTreeMap::from([(0, ScalarValueSpec::Decimal { value: value.to_string() })]),
        });
        parameter.versions.sort_by_key(|v| v.effective_from);
        let artifact = CompiledProgramArtifact::compile(program)
            .map_err(|e| anyhow!("recompile after parameter override failed: {e}"))?;
        Ok(Policy { artifact })
    }

}

fn program_parameter_ids(artifact: &CompiledProgramArtifact) -> String {
    artifact
        .program
        .parameters
        .iter()
        .filter_map(|p| p.id.as_deref())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Per-person input columns for one UK tax year (6 April to 6 April).
pub struct Dataset {
    inputs: Vec<InputRecordSpec>,
    n: Option<usize>,
    interval: IntervalSpec,
    period: PeriodSpec,
}

impl Dataset {
    /// `fiscal_year` is the starting calendar year, e.g. 2026 for 2026-27.
    pub fn tax_year(fiscal_year: i32) -> Self {
        let start = NaiveDate::from_ymd_opt(fiscal_year, 4, 6).expect("valid tax year start");
        let end = NaiveDate::from_ymd_opt(fiscal_year + 1, 4, 6).expect("valid tax year end");
        Dataset {
            inputs: Vec::new(),
            n: None,
            interval: IntervalSpec { start, end },
            period: PeriodSpec { kind: PeriodKindSpec::TaxYear, start, end },
        }
    }

    /// Add a person-level input column. `name` is the absolute input
    /// reference, e.g. `uk:statutes/ukpga/2007/3/35#input.adjusted_net_income`.
    /// All columns must have the same length (one value per person).
    pub fn with_input(mut self, name: &str, values: &[f64]) -> Result<Self> {
        match self.n {
            None => self.n = Some(values.len()),
            Some(n) if n != values.len() => {
                bail!("input column {name} has {} values, expected {n}", values.len())
            }
            Some(_) => {}
        }
        for (i, value) in values.iter().enumerate() {
            self.inputs.push(InputRecordSpec {
                name: name.to_string(),
                entity: "Person".to_string(),
                entity_id: i.to_string(),
                interval: self.interval.clone(),
                value: ScalarValueSpec::Decimal { value: value.to_string() },
            });
        }
        Ok(self)
    }

    pub fn len(&self) -> usize {
        self.n.unwrap_or(0)
    }
}

/// Output columns aligned to person order, keyed by output id.
pub struct Outputs {
    pub columns: BTreeMap<String, Vec<f64>>,
}

impl Outputs {
    pub fn column(&self, output: &str) -> Result<&[f64]> {
        self.columns
            .get(output)
            .map(Vec::as_slice)
            .ok_or_else(|| anyhow!("no output column {output}"))
    }
}

/// Evaluate `outputs` (full legal ids) for every person in the dataset using
/// the engine's vectorised fast path.
pub fn calculate(policy: &Policy, dataset: &Dataset, outputs: &[&str]) -> Result<Outputs> {
    let n = dataset.len();
    let output_names: Vec<String> = outputs.iter().map(|s| s.to_string()).collect();
    let queries = (0..n)
        .map(|i| ExecutionQuery {
            entity_id: i.to_string(),
            period: dataset.period.clone(),
            outputs: output_names.clone(),
            assessment_date: None,
        })
        .collect();
    let request = CompiledExecutionRequest {
        mode: ExecutionMode::Fast,
        dataset: DatasetSpec { inputs: dataset.inputs.clone(), relations: vec![] },
        queries,
    };
    let response = execute_compiled_request(policy.artifact.clone(), request)
        .map_err(|e| anyhow!("axiom execution failed: {e}"))?;

    let mut columns: BTreeMap<String, Vec<f64>> = output_names
        .iter()
        .map(|name| (name.clone(), vec![0.0; n]))
        .collect();
    for result in &response.results {
        let i: usize = result
            .entity_id
            .parse()
            .with_context(|| format!("unexpected entity id {}", result.entity_id))?;
        for (name, value) in &result.outputs {
            let column = columns
                .get_mut(name)
                .ok_or_else(|| anyhow!("unrequested output {name} in results"))?;
            column[i] = output_to_f64(value)?;
        }
    }
    Ok(Outputs { columns })
}

fn output_to_f64(value: &OutputValue) -> Result<f64> {
    match value {
        OutputValue::Scalar { value, .. } => match value {
            ScalarValueSpec::Decimal { value } => value
                .parse::<f64>()
                .with_context(|| format!("non-numeric decimal output {value}")),
            ScalarValueSpec::Integer { value } => Ok(*value as f64),
            ScalarValueSpec::Bool { value } => Ok(if *value { 1.0 } else { 0.0 }),
            other => bail!("unsupported scalar output kind: {other:?}"),
        },
        OutputValue::Judgment { name, .. } => bail!("judgment output {name} has no numeric value"),
    }
}
