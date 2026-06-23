import { Code, Tabs } from '../components/Code'

const sigCode = `from policyengine_uk_compiled import StructuralReform

StructuralReform(
    pre=None,   # called before the Rust engine runs
    post=None,  # called after the Rust engine produces microdata
)

# Hook signature (same for pre and post):
def hook(
    year: int,
    persons: pd.DataFrame,
    benunits: pd.DataFrame,
    households: pd.DataFrame,
) -> tuple[pd.DataFrame, pd.DataFrame, pd.DataFrame]:
    ...
    return persons, benunits, households`

const preCode = `from policyengine_uk_compiled import Simulation, StructuralReform

# Pre-hook: cap employment income at £100k before the simulation runs
def cap_wages(year, persons, benunits, households):
    persons = persons.copy()
    persons["employment_income"] = persons["employment_income"].clip(upper=100_000)
    return persons, benunits, households

reform = StructuralReform(pre=cap_wages)
result = sim.run(structural=reform)`

const postCode = `# Post-hook: add a £50/wk UBI to every adult's reform net income
def ubi_post(year, persons, benunits, households):
    households = households.copy()
    adults = persons[persons["age"] >= 18]
    adult_counts = adults.groupby("household_id").size()
    households["reform_net_income"] += (
        households["household_id"].map(adult_counts).fillna(0) * 50 * 52
    )
    return persons, benunits, households

reform = StructuralReform(post=ubi_post)
result = sim.run(structural=reform)`

const combinedCode = `# Combined: structural pre + parametric policy
from policyengine_uk_compiled import Parameters, IncomeTaxParams

def replace_with_flat_rate(year, persons, benunits, households):
    """Replace employment income with a £25k flat for illustration."""
    persons = persons.copy()
    persons["employment_income"] = persons["employment_income"].clip(upper=25_000)
    return persons, benunits, households

structural = StructuralReform(pre=replace_with_flat_rate)
policy = Parameters(income_tax=IncomeTaxParams(personal_allowance=15_000))

result = sim.run(policy=policy, structural=structural)
micro  = sim.run_microdata(policy=policy, structural=structural)`

export default function StructuralSection({ id }) {
  return (
    <section className="section" id={id}>
      <div className="section-tag">05 — Structural reforms</div>
      <h1>StructuralReform</h1>
      <p>
        For reforms that can't be expressed as a parameter change — a new tax, a capped income, a UBI — use{' '}
        <code>StructuralReform</code>. It holds two optional hooks that run before and after the Rust engine.
      </p>

      <table className="api-table">
        <thead><tr><th>Hook</th><th>When it runs</th><th>Typical use</th></tr></thead>
        <tbody>
          <tr>
            <td>pre</td>
            <td>Before the Rust engine sees the data</td>
            <td>Mutate input columns — cap wages, add a new income source, change household composition</td>
          </tr>
          <tr>
            <td>post</td>
            <td>After the Rust engine produces microdata output</td>
            <td>Adjust output columns — apply a new tax on top of results, add a UBI to net income, impose a cap</td>
          </tr>
        </tbody>
      </table>

      <div className="callout info">
        <span className="callout-icon">ℹ</span>
        <p>
          Both hooks receive all three DataFrames and must return all three, even if only one is modified.
          When a post-hook is present, <code>sim.run()</code> automatically re-aggregates the modified
          microdata into a <code>SimulationResult</code> in Python rather than using the Rust aggregation.
        </p>
      </div>

      <h2>Signature</h2>
      <Code code={sigCode} label="StructuralReform" />

      <h2>Examples</h2>
      <Tabs tabs={[
        { label: 'pre-hook', code: preCode },
        { label: 'post-hook', code: postCode },
        { label: 'combined with Parameters', code: combinedCode },
      ]} />
    </section>
  )
}
