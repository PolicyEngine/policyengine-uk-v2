//! Axiom rules-engine backend.
//!
//! Evaluates compiled RuleSpec artifacts (composed from rulespec-uk via
//! axiom-compose) over simulation populations, replacing hand-coded variable
//! logic with statute-derived rules. Behavioural responses and dataset
//! handling stay in the Rust layer; axiom owns the rules.
//!
//! Uses the engine's dense (columnar) executor: input columns in, output
//! columns out, no per-person record or query objects.
//!
//!   * [`Policy`] wraps a compiled artifact plus its dense program. Reforms
//!     and uprating assumptions are parameter overrides via
//!     [`Policy::with_parameter`] — clone the program, patch the versioned
//!     parameter table, recompile in memory (sub-millisecond). Future-dated
//!     overrides model projected uprating.
//!   * [`Dataset`] holds per-person input columns for one tax year.
//!   * [`calculate`] evaluates output columns aligned to person order.
//!
//! Rules and inputs are referred to by their bare RuleSpec names within the
//! composed program (e.g. `income_tax_on_section_10_income`), which
//! axiom-compose keeps unique.

use std::collections::{BTreeMap, HashMap};

use anyhow::{anyhow, bail, Result};
use axiom_rules_engine::compile::CompiledProgramArtifact;
use axiom_rules_engine::dense::{
    DenseBatchSpec, DenseColumn, DenseCompiledProgram, DenseOutputValue,
};
use axiom_rules_engine::model::{Period, PeriodKind};
use axiom_rules_engine::spec::{ParameterVersionSpec, ScalarValueSpec};
use chrono::NaiveDate;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;

/// A compiled rule system: baseline law, or baseline plus parameter overrides.
#[derive(Clone)]
pub struct Policy {
    artifact: CompiledProgramArtifact,
    dense: DenseCompiledProgram,
}

impl Policy {
    pub fn from_artifact_json(json: &str) -> Result<Self> {
        let artifact = CompiledProgramArtifact::from_json_str(json)
            .map_err(|e| anyhow!("failed to load axiom artifact: {e}"))?;
        Self::from_artifact(artifact)
    }

    fn from_artifact(artifact: CompiledProgramArtifact) -> Result<Self> {
        let dense = DenseCompiledProgram::from_artifact(&artifact, Some("Person"))
            .map_err(|e| anyhow!("dense compile failed: {e}"))?;
        Ok(Policy { artifact, dense })
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
                    self.parameter_ids().join(", ")
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
        Self::from_artifact(artifact)
    }

    /// Bare names of the person-level inputs the rules require.
    pub fn inputs(&self) -> &[String] {
        self.dense.root_inputs()
    }

    /// Bare names of every rule the policy can calculate.
    pub fn outputs(&self) -> Vec<String> {
        self.dense.output_names()
    }

    fn parameter_ids(&self) -> Vec<String> {
        self.artifact
            .program
            .parameters
            .iter()
            .filter_map(|p| p.id.clone())
            .collect()
    }
}

/// Per-person input columns for one UK tax year (6 April to 6 April).
pub struct Dataset {
    columns: HashMap<String, DenseColumn>,
    n: Option<usize>,
    period: Period,
}

impl Dataset {
    /// `fiscal_year` is the starting calendar year, e.g. 2026 for 2026-27.
    pub fn tax_year(fiscal_year: i32) -> Self {
        let start = NaiveDate::from_ymd_opt(fiscal_year, 4, 6).expect("valid tax year start");
        let end = NaiveDate::from_ymd_opt(fiscal_year + 1, 4, 6).expect("valid tax year end");
        Dataset {
            columns: HashMap::new(),
            n: None,
            period: Period { kind: PeriodKind::TaxYear, start, end },
        }
    }

    /// Add a person-level input column by bare name, e.g.
    /// `adjusted_net_income`. All columns must have one value per person.
    pub fn with_input(mut self, name: &str, values: &[f64]) -> Result<Self> {
        match self.n {
            None => self.n = Some(values.len()),
            Some(n) if n != values.len() => {
                bail!("input column {name} has {} values, expected {n}", values.len())
            }
            Some(_) => {}
        }
        let column = values
            .iter()
            .map(|v| {
                Decimal::from_f64(*v).ok_or_else(|| anyhow!("non-finite value in column {name}"))
            })
            .collect::<Result<Vec<Decimal>>>()?;
        self.columns.insert(name.to_string(), DenseColumn::Decimal(column));
        Ok(self)
    }

    pub fn len(&self) -> usize {
        self.n.unwrap_or(0)
    }
}

/// Output columns aligned to person order, keyed by bare rule name.
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

/// Evaluate `outputs` (bare rule names) for every person in the dataset.
pub fn calculate(policy: &Policy, dataset: &Dataset, outputs: &[&str]) -> Result<Outputs> {
    let batch = DenseBatchSpec {
        row_count: dataset.len(),
        inputs: dataset.columns.clone(),
        relations: HashMap::new(),
    };
    let output_names: Vec<String> = outputs.iter().map(|s| s.to_string()).collect();
    let result = policy
        .dense
        .execute(&dataset.period, batch, &output_names)
        .map_err(|e| anyhow!("axiom dense execution failed: {e}"))?;

    let mut columns = BTreeMap::new();
    for name in output_names {
        let value = result
            .outputs
            .get(&name)
            .ok_or_else(|| anyhow!("missing output {name} in dense result"))?;
        columns.insert(name, output_to_f64_column(value)?);
    }
    Ok(Outputs { columns })
}

fn output_to_f64_column(value: &DenseOutputValue) -> Result<Vec<f64>> {
    match value {
        DenseOutputValue::Scalar(column) => match column {
            DenseColumn::Decimal(values) => values
                .iter()
                .map(|v| v.to_f64().ok_or_else(|| anyhow!("decimal {v} not representable as f64")))
                .collect(),
            DenseColumn::Integer(values) => Ok(values.iter().map(|v| *v as f64).collect()),
            DenseColumn::Bool(values) => {
                Ok(values.iter().map(|v| if *v { 1.0 } else { 0.0 }).collect())
            }
            other => bail!("unsupported dense output column: {other:?}"),
        },
        DenseOutputValue::Judgment(_) => bail!("judgment outputs have no numeric column"),
    }
}
