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
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Simulation>()?;
    Ok(())
}
