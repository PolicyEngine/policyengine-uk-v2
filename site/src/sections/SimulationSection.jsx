import { Code, Tabs } from '../components/Code'

const constructorSig = `Simulation(
    year: int = 2025,
    *,
    # Pass DataFrames directly (hypothetical households or custom data)
    persons   = None,      # pd.DataFrame or CSV string
    benunits  = None,
    households = None,

    # Or use a named dataset (auto-downloads)
    dataset   = None,      # "frs" | "spi" | "lcfs" | "was"
)`

const singlePersonCode = `persons, benunits, households = Simulation.single_person(
    age=35,
    employment_income=60_000,
    region="London",
    rent_monthly=1_500,
    council_tax_annual=2_000,
)

sim = Simulation(year=2025, persons=persons, benunits=benunits, households=households)
result = sim.run()
micro = sim.run_microdata()`

const coupleCode = `persons, benunits, households = Simulation.couple(
    ages=(38, 35),
    incomes=(55_000, 30_000),
    children=2,
    child_ages=[4.0, 8.0],
    region="London",
    rent_monthly=2_000,
)

sim = Simulation(year=2025, persons=persons, benunits=benunits, households=households)
micro = sim.run_microdata()
hh = micro.households.iloc[0]
print(hh["baseline_net_income"])    # → 74492.77
print(hh["baseline_total_tax"])     # → 22955.23
print(hh["baseline_total_benefits"])# → 6915.77`

const coupleOutput = `74492.77
22955.23
6915.77`

const customCode = `import pandas as pd
from policyengine_uk_compiled import Simulation
from policyengine_uk_compiled.engine import PERSON_DEFAULTS, BENUNIT_DEFAULTS, HOUSEHOLD_DEFAULTS

# Batch: income sweep £10k–£100k (step £10k)
persons, benunits, households = [], [], []
for i, income in enumerate(range(10_000, 100_001, 10_000)):
    persons.append({**PERSON_DEFAULTS, "person_id": i, "benunit_id": i,
                    "household_id": i, "employment_income": float(income)})
    benunits.append({**BENUNIT_DEFAULTS, "benunit_id": i, "household_id": i,
                     "person_ids": str(i)})
    households.append({**HOUSEHOLD_DEFAULTS, "household_id": i,
                       "benunit_ids": str(i), "person_ids": str(i)})

sim = Simulation(year=2025,
    persons=pd.DataFrame(persons),
    benunits=pd.DataFrame(benunits),
    households=pd.DataFrame(households))

micro = sim.run_microdata()
sweep = micro.households[["baseline_net_income"]].copy()
sweep["employment_income"] = list(range(10_000, 100_001, 10_000))
print(sweep.to_string(index=False))`

const customOutput = ` baseline_net_income  employment_income
             10000.0              10000
             17919.6              20000
             25119.6              30000
             32319.6              40000
             39519.6              50000
             45357.4              60000
             51157.4              70000
             56957.4              80000
             62757.4              90000
             68557.4             100000`

const datasetCode = `import os
os.environ["POLICYENGINE_UK_DATA_TOKEN"] = "your-token"

# FRS (default)
sim = Simulation(year=2025)

# Other datasets
sim_spi  = Simulation(year=2025, dataset="spi")
sim_lcfs = Simulation(year=2025, dataset="lcfs")
sim_was  = Simulation(year=2025, dataset="was")

result = sim.run()`

const runCode = `from policyengine_uk_compiled import Simulation, Parameters, UniversalCreditParams

sim = Simulation(year=2025)

# Baseline
baseline = sim.run()

# Reform
reform = Parameters(universal_credit=UniversalCreditParams(taper_rate=0.50))
result = sim.run(policy=reform)

print(result.budgetary_impact.net_cost)               # → float, £/yr (positive = fiscal cost)
print(result.winners_losers.winners_pct)              # → float, e.g. 42.3
print(result.baseline_poverty.relative_ahc_children)  # → float, e.g. 18.7 (percent)`

const microdataCode = `micro = sim.run_microdata(policy=reform_pa)

# Per-household net incomes
hh = micro.households[["household_id", "weight",
                        "baseline_net_income", "reform_net_income"]]
# → DataFrame, one row per household
#    household_id  weight  baseline_net_income  reform_net_income
#    0             1.0     39519.6              41347.91

# Per-person tax liabilities
persons = micro.persons[["person_id", "household_id",
                          "employment_income",
                          "baseline_income_tax", "reform_income_tax"]]
# → DataFrame, one row per person
#    person_id  household_id  employment_income  baseline_income_tax  reform_income_tax
#    0          0             50000.0            7486.0               7348.33

hh["net_income_change"] = hh["reform_net_income"] - hh["baseline_net_income"]
# hh["net_income_change"].iloc[0] → 1828.31`

export default function SimulationSection({ id }) {
  return (
    <section className="section" id={id}>
      <div className="section-tag">02 — Simulation</div>
      <h1>Simulation</h1>
      <p>
        The main entry point. Accepts household data as DataFrames, a CSV directory, or a dataset name. Call{' '}
        <code>.run()</code> for aggregate statistics, or <code>.run_microdata()</code> for per-entity DataFrames.
      </p>

      <h2>Constructor</h2>
      <div className="sig-block">
        <span className="kw">class </span>
        <span className="cls">Simulation</span>:
        <br />
        <pre style={{ background: 'transparent', padding: 0, marginTop: 8, fontSize: 12.5, lineHeight: 1.8 }}>{constructorSig}</pre>
      </div>

      <h2>Data input</h2>
      <p>Three modes — pick one:</p>
      <table className="api-table">
        <thead><tr><th>Mode</th><th>When to use</th><th>Arguments</th></tr></thead>
        <tbody>
          <tr>
            <td>DataFrames</td>
            <td>Hypothetical households, budget constraints, MTR sweeps</td>
            <td><code>persons=</code>, <code>benunits=</code>, <code>households=</code></td>
          </tr>
          <tr>
            <td>Dataset name</td>
            <td>Full-population run (auto-download)</td>
            <td><code>dataset="frs"</code></td>
          </tr>
        </tbody>
      </table>

      <h2>Static constructors</h2>
      <p>
        Convenience methods that build the three DataFrames for common household shapes. All return a{' '}
        <code>(persons_df, benunits_df, households_df)</code> tuple.
      </p>
      <Tabs tabs={[
        { label: 'single_person', code: singlePersonCode },
        { label: 'couple', code: coupleCode, output: coupleOutput },
        { label: 'custom batch', code: customCode, output: customOutput },
      ]} />

      <div className="callout info">
        <span className="callout-icon">ℹ</span>
        <p>
          Batching is critical for performance — one call with many households is far faster than many individual
          calls. Use <code>PERSON_DEFAULTS</code>, <code>BENUNIT_DEFAULTS</code>, and{' '}
          <code>HOUSEHOLD_DEFAULTS</code> as templates when building DataFrames manually.
        </p>
      </div>

      <h2>Methods</h2>

      <div className="method-block">
        <div className="method-sig">
          sim.<span className="fn-name">run</span>(
          policy=None, structural=None, timeout=120
          ) → <span className="ret">SimulationResult</span>
        </div>
        <div className="method-body">
          <p className="method-desc">
            Run the simulation and return aggregate statistics. Computes baseline and reform in a single call.
            Pass <code>policy</code> to apply a parametric reform; omit it for a baseline-only run.
          </p>
          <ul className="param-list">
            {[
              ['policy', 'Parameters', 'Reform parameter overlay. Only set the fields you want to change.'],
              ['structural', 'StructuralReform', 'Pre/post hooks for structural reforms (add a new tax, mutate incomes, etc.).'],
              ['timeout', 'int = 120', 'Maximum seconds to wait for the binary.'],
            ].map(([k, t, d]) => (
              <li key={k} className="param-item">
                <span className="param-key">{k} <em>{t}</em></span>
                <span className="param-val">{d}</span>
              </li>
            ))}
          </ul>
        </div>
      </div>

      <div className="method-block">
        <div className="method-sig">
          sim.<span className="fn-name">run_microdata</span>(
          policy=None, structural=None, timeout=120
          ) → <span className="ret">MicrodataResult</span>
        </div>
        <div className="method-body">
          <p className="method-desc">
            Run the simulation and return per-entity DataFrames. Each entity (person, benefit unit, household)
            gets one row with both <code>baseline_*</code> and <code>reform_*</code> columns.
          </p>
        </div>
      </div>

      <div className="method-block">
        <div className="method-sig">
          sim.<span className="fn-name">get_baseline_params</span>(timeout=10) → <span className="ret">dict</span>
        </div>
        <div className="method-body">
          <p className="method-desc">
            Export the baseline parameter set for the configured year as a plain dict. Useful for inspecting
            current parameter values before constructing a reform.
          </p>
        </div>
      </div>

      <h2>Usage examples</h2>
      <Tabs tabs={[
        { label: 'run() — aggregate', code: runCode },
        { label: 'run_microdata()', code: microdataCode },
        { label: 'dataset mode', code: datasetCode },
      ]} />
    </section>
  )
}
