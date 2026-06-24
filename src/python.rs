//! PyO3 bindings: load a dataset once, then score reforms in-process.
//!
//! `Simulation` holds the loaded dataset, the baseline simulation results and
//! the baseline-side labour supply retention rates, so each reform run only
//! pays the reform-dependent work (policy retention sims, the policy
//! simulation, and the analysis). A baseline `run()` reuses the cached
//! baseline results directly.

use std::path::Path;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};

use crate::data::clean::{
    build_microdata_benunits_table, build_microdata_households_table,
    build_microdata_persons_table, ColumnData, Table,
};
use crate::data::Dataset;
use crate::engine::simulation::SimulationResults;
use crate::parameters::Parameters;
use crate::run;
use crate::variables::labour_supply::{self, BaselineRetention};

#[pyclass]
struct Simulation {
    dataset: Dataset,
    year: u32,
    baseline_params: Parameters,
    baseline: Option<SimulationResults>,
    ls_baseline: Option<Option<BaselineRetention>>,
}

fn to_py_err(e: impl std::fmt::Display) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

#[pymethods]
impl Simulation {
    #[new]
    fn new(data_dir: &str, params_dir: &str, year: u32) -> PyResult<Self> {
        let dataset = run::load_dataset_dir(Path::new(data_dir), year).map_err(to_py_err)?;
        let baseline_params =
            Parameters::for_year_in(Path::new(params_dir), year).map_err(to_py_err)?;
        Ok(Self { dataset, year, baseline_params, baseline: None, ls_baseline: None })
    }

    /// Score a policy (a JSON parameter overlay; `None` = baseline) and
    /// return the analysis as a JSON string.
    #[pyo3(signature = (policy_json=None))]
    fn run(&mut self, py: Python<'_>, policy_json: Option<&str>) -> PyResult<String> {
        py.allow_threads(|| self.run_inner(policy_json)).map_err(to_py_err)
    }

    /// Run the simulation and return per-entity microdata as columnar buffers.
    ///
    /// Returns a dict {"persons": cols, "benunits": cols, "households": cols}
    /// where each `cols` is a list of (name, kind, payload):
    ///   kind "i8"/"f8" → payload is little-endian bytes (int64/float64),
    ///   kind "b1"      → payload is bytes of 0/1 (bool),
    ///   kind "str"     → payload is a list[str].
    /// Python wraps the numeric buffers with np.frombuffer (one copy) and the
    /// string lists directly, avoiding the CSV round-trip entirely.
    #[pyo3(signature = (policy_json=None, return_baselines=false))]
    fn run_microdata<'py>(
        &mut self,
        py: Python<'py>,
        policy_json: Option<&str>,
        return_baselines: bool,
    ) -> PyResult<Bound<'py, PyDict>> {
        // Compute baseline + reformed results without holding the GIL.
        let (persons, benunits, households) = py
            .allow_threads(|| self.build_microdata_tables(policy_json, return_baselines))
            .map_err(to_py_err)?;

        let out = PyDict::new(py);
        out.set_item("persons", table_to_py(py, &persons)?)?;
        out.set_item("benunits", table_to_py(py, &benunits)?)?;
        out.set_item("households", table_to_py(py, &households)?)?;
        Ok(out)
    }
}

impl Simulation {
    fn run_inner(&mut self, policy_json: Option<&str>) -> anyhow::Result<String> {
        if self.baseline.is_none() {
            self.baseline =
                Some(run::run_baseline(&self.dataset, &self.baseline_params, self.year));
        }

        let output = match policy_json {
            // With identical parameters the policy pipeline reproduces the
            // baseline exactly (the labour supply response is zero), so a
            // baseline run is analysed straight off the cached results.
            None => {
                let baseline = self.baseline.as_ref().unwrap();
                run::analyse(&self.dataset, &self.baseline_params, baseline, baseline, self.year)
            }
            Some(json) => {
                let policy_params = self.baseline_params.apply_json_overlay(json)?;
                if policy_params.labour_supply.enabled && self.ls_baseline.is_none() {
                    let baseline_net: Vec<f64> = self
                        .baseline
                        .as_ref()
                        .unwrap()
                        .household_results
                        .iter()
                        .map(|hr| hr.net_income)
                        .collect();
                    self.ls_baseline = Some(labour_supply::compute_baseline_retention(
                        &self.dataset.people,
                        &self.dataset.benunits,
                        &self.dataset.households,
                        &self.baseline_params,
                        &baseline_net,
                        self.year,
                    ));
                }
                let baseline = self.baseline.as_ref().unwrap();
                let ls_baseline = self.ls_baseline.as_ref().and_then(|b| b.as_ref());
                let reformed = run::run_reform_with_baseline_retention(
                    &self.dataset,
                    &policy_params,
                    baseline,
                    ls_baseline,
                    self.year,
                );
                run::analyse(&self.dataset, &self.baseline_params, baseline, &reformed, self.year)
            }
        };
        Ok(serde_json::to_string(&output)?)
    }

    /// Compute baseline + reformed results and build the three microdata tables.
    /// The baseline is recomputed on every call (the dataset is held in memory,
    /// but no simulation results are cached). With no policy the reform pipeline
    /// reproduces the baseline exactly, so the baseline is used for both sides.
    fn build_microdata_tables(
        &mut self,
        policy_json: Option<&str>,
        return_baselines: bool,
    ) -> anyhow::Result<(Table, Table, Table)> {
        let baseline = run::run_baseline(&self.dataset, &self.baseline_params, self.year);

        let build = |baseline: &SimulationResults, reformed: &SimulationResults| {
            (
                build_microdata_persons_table(&self.dataset, baseline, reformed, return_baselines),
                build_microdata_benunits_table(&self.dataset, baseline, reformed, return_baselines),
                build_microdata_households_table(
                    &self.dataset, baseline, reformed, self.year, return_baselines,
                ),
            )
        };

        match policy_json {
            None => Ok(build(&baseline, &baseline)),
            Some(json) => {
                let policy_params = self.baseline_params.apply_json_overlay(json)?;
                let reformed = run::run_reform(
                    &self.dataset,
                    &self.baseline_params,
                    &policy_params,
                    &baseline,
                    self.year,
                );
                Ok(build(&baseline, &reformed))
            }
        }
    }
}

/// Convert a microdata `Table` into a list of (name, kind, payload) tuples.
/// Numeric/bool columns become little-endian byte buffers; text columns become
/// Python string lists.
fn table_to_py<'py>(py: Python<'py>, table: &Table) -> PyResult<Bound<'py, PyList>> {
    let cols = PyList::empty(py);
    for col in &table.columns {
        let entry = match &col.data {
            ColumnData::Int(v) => {
                let mut buf = Vec::with_capacity(v.len() * 8);
                for x in v {
                    buf.extend_from_slice(&x.to_le_bytes());
                }
                (col.name, "i8", PyBytes::new(py, &buf).into_any())
            }
            ColumnData::Float { vals, .. } => {
                let mut buf = Vec::with_capacity(vals.len() * 8);
                for x in vals {
                    buf.extend_from_slice(&x.to_le_bytes());
                }
                (col.name, "f8", PyBytes::new(py, &buf).into_any())
            }
            ColumnData::Bool { vals, .. } => {
                let buf: Vec<u8> = vals.iter().map(|&b| b as u8).collect();
                (col.name, "b1", PyBytes::new(py, &buf).into_any())
            }
            ColumnData::Text(v) => {
                (col.name, "str", PyList::new(py, v)?.into_any())
            }
        };
        cols.append(entry)?;
    }
    Ok(cols)
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Simulation>()?;
    Ok(())
}
