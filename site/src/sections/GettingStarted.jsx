import { useState } from 'react'
import { Code, Tabs } from '../components/Code'

function InstallStrip() {
  const [copied, setCopied] = useState(false)
  const copy = () => {
    navigator.clipboard.writeText('pip install policyengine-uk-compiled')
    setCopied(true)
    setTimeout(() => setCopied(false), 1800)
  }
  return (
    <div className="install-strip">
      <code>pip install policyengine-uk-compiled</code>
      <button className={`copy-btn ${copied ? 'copied' : ''}`} onClick={copy}>
        {copied ? 'copied' : 'copy'}
      </button>
    </div>
  )
}

const quickstartCode = `from policyengine_uk_compiled import Simulation, Parameters, IncomeTaxParams

# Single person earning £50,000
persons, benunits, households = Simulation.single_person(employment_income=50_000)
sim = Simulation(year=2025, persons=persons, benunits=benunits, households=households)

# Baseline
baseline = sim.run()
print(baseline.budgetary_impact.baseline_revenue)

# Reform: raise personal allowance to £15,000
reform = Parameters(income_tax=IncomeTaxParams(personal_allowance=15_000))
result = sim.run(policy=reform)
print(result.budgetary_impact.net_cost)`

const quickstartOutput = `13580.66
-43.24`

const populationCode = `from policyengine_uk_compiled import Simulation, Parameters, UniversalCreditParams

# Full FRS population (requires POLICYENGINE_UK_DATA_TOKEN)
sim = Simulation(year=2025)

# Reduce UC taper rate from 55% to 50%
reform = Parameters(universal_credit=UniversalCreditParams(taper_rate=0.50))
result = sim.run(policy=reform)

net_cost = result.budgetary_impact.net_cost
print(f"UC taper 55→50%: £{net_cost / 1e9:.1f}bn/yr")
# → "UC taper 55→50%: £X.Xbn/yr"

# Decile impacts
for d in result.decile_impacts:
    print(f"Decile {d.decile}: avg gain £{d.avg_change:.0f}/yr ({d.pct_change:.1f}%)")
# → "Decile 1: avg gain £N/yr (N.N%)"  ... × 10`

export default function GettingStarted({ id }) {
  return (
    <section className="section" id={id}>
      <div className="section-tag">01 — Getting started</div>
      <h1>policyengine-uk-compiled</h1>
      <p>
        A high-performance UK tax-benefit microsimulation engine. The core is compiled Rust (~0.1 ms per household);
        this Python package wraps it via a <code>Simulation</code> class. It simulates income tax, National Insurance,
        Universal Credit, Child Benefit, and more than ten other programmes. Reforms are expressed as a{' '}
        <code>Parameters</code> overlay — no recompilation needed.
      </p>
      <p>Available fiscal years: 1994–2029.</p>

      <InstallStrip />

      <div className="callout info">
        <span className="callout-icon">ℹ</span>
        <p>
          Full-population datasets (FRS, SPI, LCFS, WAS) download automatically on first use when the environment
          variable <code>POLICYENGINE_UK_DATA_TOKEN</code> is set. Hypothetical households work without a token.
        </p>
      </div>

      <h2>Quick start</h2>
      <Tabs tabs={[
        { label: 'single household', code: quickstartCode, output: quickstartOutput },
        { label: 'full population', code: populationCode },
      ]} />

      <h2>What it simulates</h2>
      <table className="api-table">
        <thead>
          <tr><th>Programme</th><th>Coverage</th></tr>
        </thead>
        <tbody>
          {[
            ['Income tax', 'UK and Scottish rates, personal allowance taper, dividends, savings starter band, HICBC'],
            ['National Insurance', 'Classes 1, 2, 4; employee and employer; all thresholds and rates'],
            ['Universal Credit', 'Standard allowances, child/disability elements, taper, work allowances, benefit cap'],
            ['Child Benefit', 'Eldest and additional child rates; HICBC taper'],
            ['State pension', 'New State Pension weekly rate'],
            ['Pension Credit', 'Standard minimum (single and couple), savings credit'],
            ['Housing Benefit', 'Legacy HB for non-UC claimants; withdrawal rate'],
            ['Tax credits', 'Working Tax Credit, Child Tax Credit; taper and elements'],
            ['Other benefits', 'ESA (IR), JSA (IB), Income Support, Carer\'s Allowance, Scottish Child Payment, DLA, PIP, Attendance Allowance'],
            ['Indirect taxes', 'VAT, fuel duty, alcohol duty, tobacco duty'],
            ['Capital taxes', 'Capital Gains Tax, Stamp Duty, LBTT, LTT, wealth tax (model only)'],
            ['Council tax', 'Calculated from band and region; single-person discount'],
          ].map(([prog, cov]) => (
            <tr key={prog}><td>{prog}</td><td>{cov}</td></tr>
          ))}
        </tbody>
      </table>

      <div className="callout warn" style={{ marginTop: 24 }}>
        <span className="callout-icon">⚠</span>
        <p>
          FRS under-reports Universal Credit receipt at roughly 60% of the DWP administrative total, so UC reform
          costings will be proportionally lower than OBR/DWP estimates. The same applies to most means-tested
          benefits — the survey captures take-up, not eligibility. Results are appropriate for distributional and
          reform analysis, not absolute benefit expenditure benchmarking.
        </p>
      </div>
    </section>
  )
}
