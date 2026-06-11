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
//!   * [`Dataset`] holds per-person input columns for one tax year, plus
//!     one-to-many relations (e.g. income components per person) declared
//!     with per-person counts and flat related-row columns.
//!   * [`calculate`] evaluates output columns aligned to person order.
//!
//! Rules, inputs, and relations are referred to by their bare RuleSpec names
//! within the composed program (e.g. `income_tax_liability`,
//! `income_component_of_taxpayer`), which axiom-compose keeps unique.

pub mod backend;

use std::collections::{BTreeMap, HashMap};

use anyhow::{anyhow, bail, Result};
use axiom_rules_engine::compile::CompiledProgramArtifact;
use axiom_rules_engine::dense::{
    DenseBatchSpec, DenseColumn, DenseCompiledProgram, DenseOutputValue, DenseRelationBatchSpec,
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
    /// `entity` is the program's root entity, i.e. what one dataset row
    /// represents: `"Person"` for income tax, `"Family"` for Universal
    /// Credit.
    pub fn from_artifact_json(json: &str, entity: &str) -> Result<Self> {
        let artifact = CompiledProgramArtifact::from_json_str(json)
            .map_err(|e| anyhow!("failed to load axiom artifact: {e}"))?;
        Self::from_artifact(artifact, entity)
    }

    fn from_artifact(artifact: CompiledProgramArtifact, entity: &str) -> Result<Self> {
        let dense = DenseCompiledProgram::from_artifact(&artifact, Some(entity))
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
        Self::from_artifact(artifact, &self.dense.root_entity().to_string())
    }

    /// Bare names of the person-level inputs the rules require.
    #[allow(dead_code)]
    pub fn inputs(&self) -> &[String] {
        self.dense.root_inputs()
    }

    /// Bare names of every rule the policy can calculate.
    #[allow(dead_code)]
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

/// Input columns for one period, with one row per root entity (person,
/// family, ...).
#[derive(Clone)]
pub struct Dataset {
    columns: HashMap<String, DenseColumn>,
    relations: HashMap<String, RelationData>,
    n: Option<usize>,
    period: Period,
}

#[derive(Clone)]
struct RelationData {
    offsets: Vec<usize>,
    inputs: HashMap<String, DenseColumn>,
}

impl Dataset {
    /// One UK tax year (6 April to 6 April); `fiscal_year` is the starting
    /// calendar year, e.g. 2026 for 2026-27.
    pub fn tax_year(fiscal_year: i32) -> Self {
        let start = NaiveDate::from_ymd_opt(fiscal_year, 4, 6).expect("valid tax year start");
        let end = NaiveDate::from_ymd_opt(fiscal_year + 1, 4, 6).expect("valid tax year end");
        Self::for_period(Period { kind: PeriodKind::TaxYear, start, end })
    }

    /// One week starting on `start`, e.g. a NICs tax week.
    pub fn week(start: NaiveDate) -> Self {
        let end = start + chrono::Duration::days(7);
        Self::for_period(Period { kind: PeriodKind::BenefitWeek, start, end })
    }

    /// One calendar month, e.g. a Universal Credit assessment period.
    #[allow(dead_code)]
    pub fn month(year: i32, month: u32) -> Self {
        let start = NaiveDate::from_ymd_opt(year, month, 1).expect("valid month start");
        let end = match month {
            12 => NaiveDate::from_ymd_opt(year + 1, 1, 1),
            _ => NaiveDate::from_ymd_opt(year, month + 1, 1),
        }
        .expect("valid month end");
        Self::for_period(Period { kind: PeriodKind::Month, start, end })
    }

    fn for_period(period: Period) -> Self {
        Dataset { columns: HashMap::new(), relations: HashMap::new(), n: None, period }
    }

    /// Add a numeric input column by bare name, e.g. `adjusted_net_income`.
    /// All columns must have one value per row.
    pub fn with_input(mut self, name: &str, values: &[f64]) -> Result<Self> {
        self.check_row_count(name, values.len())?;
        let column = decimal_column(name, values)?;
        self.columns.insert(name.to_string(), column);
        Ok(self)
    }

    /// Add a boolean input column by bare name, e.g.
    /// `claim_is_for_joint_claimants`.
    #[allow(dead_code)]
    pub fn with_bool_input(mut self, name: &str, values: &[bool]) -> Result<Self> {
        self.check_row_count(name, values.len())?;
        self.columns.insert(name.to_string(), DenseColumn::Bool(values.to_vec()));
        Ok(self)
    }

    /// Add a numeric input column with the same value in every row,
    /// converting to decimal once instead of per element. The row count must
    /// already be established by an earlier column.
    pub fn with_const_input(mut self, name: &str, value: f64) -> Result<Self> {
        let n = self.n.ok_or_else(|| anyhow!("add a per-row column before constant columns"))?;
        let value =
            Decimal::from_f64(value).ok_or_else(|| anyhow!("non-finite value in column {name}"))?;
        self.columns.insert(name.to_string(), DenseColumn::Decimal(vec![value; n]));
        Ok(self)
    }

    /// Declare a one-to-many relation by bare name (e.g.
    /// `income_component_of_taxpayer`) with the number of related rows per
    /// root row. Related input columns are then added flat, in row order,
    /// via [`Dataset::with_relation_input`].
    #[allow(dead_code)]
    pub fn with_relation(mut self, name: &str, counts: &[usize]) -> Result<Self> {
        self.check_row_count(name, counts.len())?;
        let mut offsets = Vec::with_capacity(counts.len() + 1);
        let mut total = 0;
        offsets.push(0);
        for count in counts {
            total += count;
            offsets.push(total);
        }
        self.relations.insert(name.to_string(), RelationData { offsets, inputs: HashMap::new() });
        Ok(self)
    }

    /// Add a numeric related-row input column for a declared relation, flat
    /// across all rows (length must equal the sum of the relation's counts).
    #[allow(dead_code)]
    pub fn with_relation_input(mut self, relation: &str, name: &str, values: &[f64]) -> Result<Self> {
        let column = decimal_column(name, values)?;
        self.add_relation_column(relation, name, column, values.len())?;
        Ok(self)
    }

    /// Add a boolean related-row input column for a declared relation.
    #[allow(dead_code)]
    pub fn with_relation_bool_input(
        mut self,
        relation: &str,
        name: &str,
        values: &[bool],
    ) -> Result<Self> {
        self.add_relation_column(relation, name, DenseColumn::Bool(values.to_vec()), values.len())?;
        Ok(self)
    }

    pub fn len(&self) -> usize {
        self.n.unwrap_or(0)
    }

    fn check_row_count(&mut self, name: &str, len: usize) -> Result<()> {
        match self.n {
            None => self.n = Some(len),
            Some(n) if n != len => bail!("column {name} has {len} values, expected {n}"),
            Some(_) => {}
        }
        Ok(())
    }

    fn add_relation_column(
        &mut self,
        relation: &str,
        name: &str,
        column: DenseColumn,
        len: usize,
    ) -> Result<()> {
        let data = self.relations.get_mut(relation).ok_or_else(|| {
            anyhow!("unknown relation {relation}; declare it with with_relation first")
        })?;
        let expected = *data.offsets.last().expect("offsets are never empty");
        if len != expected {
            bail!("relation input {relation}.{name} has {len} values, expected {expected}");
        }
        data.inputs.insert(name.to_string(), column);
        Ok(())
    }
}

fn decimal_column(name: &str, values: &[f64]) -> Result<DenseColumn> {
    use rayon::prelude::*;
    let column = values
        .par_iter()
        .with_min_len(8192)
        .map(|v| Decimal::from_f64(*v).ok_or_else(|| anyhow!("non-finite value in column {name}")))
        .collect::<Result<Vec<Decimal>>>()?;
    Ok(DenseColumn::Decimal(column))
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
/// Consumes the dataset, which is built fresh per call, so columns move into
/// the batch without cloning.
pub fn calculate(policy: &Policy, mut dataset: Dataset, outputs: &[&str]) -> Result<Outputs> {
    // The dense program keys relations by full legal id (e.g.
    // `uk:statutes/ukpga/2007/3/23#relation.income_component_of_taxpayer`);
    // the dataset declares them by bare name, so match on the suffix.
    let mut relations = HashMap::new();
    for schema in policy.dense.relations() {
        let key = &schema.key;
        let name = dataset
            .relations
            .keys()
            .find(|name| key.name == **name || key.name.ends_with(&format!("#relation.{name}")))
            .cloned()
            .ok_or_else(|| anyhow!("dataset is missing relation {}", key.name))?;
        let data = dataset.relations.remove(&name).expect("key found above");
        relations.insert(
            key.clone(),
            DenseRelationBatchSpec { offsets: data.offsets, inputs: data.inputs },
        );
    }
    let batch = DenseBatchSpec {
        row_count: dataset.len(),
        inputs: std::mem::take(&mut dataset.columns),
        relations,
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
