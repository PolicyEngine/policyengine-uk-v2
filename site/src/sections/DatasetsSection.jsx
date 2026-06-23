import { Code } from '../components/Code'

const downloadCode = `import os
os.environ["POLICYENGINE_UK_DATA_TOKEN"] = "your-token"

from policyengine_uk_compiled import download_all, capabilities

# Download all datasets
download_all()

# Or specific datasets
download_all(datasets=("frs", "spi"))

# Force re-download
download_all(force=True)

# Check what's available locally
avail = capabilities()
print(avail)`

const useCode = `from policyengine_uk_compiled import Simulation

# FRS (default — most detailed household survey)
sim = Simulation(year=2025)

# SPI — self-assessment income data (persons only; no household structure)
sim_spi = Simulation(year=2025, dataset="spi")

# LCFS — expenditure survey (adds VAT, fuel/alcohol/tobacco duty)
sim_lcfs = Simulation(year=2025, dataset="lcfs")

# WAS — wealth data (needed for wealth tax, CGT modelling)
sim_was = Simulation(year=2025, dataset="was")

# EFRS — enhanced FRS with additional imputed fields
sim_efrs = Simulation(year=2025, dataset="efrs")`

const yearCode = `# Any year 1994–2029
sim_past   = Simulation(year=2010)  # historical
sim_future = Simulation(year=2028)  # OBR-forecast uprated

result = sim_past.run()
result = sim_future.run()`

const DATASETS = [
  {
    id: 'frs',
    name: 'FRS',
    full: 'Family Resources Survey',
    desc: 'DWP\'s main household income survey. The default dataset. Covers income, benefits, housing costs, and household composition.',
  },
  {
    id: 'efrs',
    name: 'EFRS',
    full: 'Enhanced FRS',
    desc: 'FRS extended with additional imputed fields. Use when the standard FRS lacks required variables.',
  },
  {
    id: 'spi',
    name: 'SPI',
    full: 'Survey of Personal Incomes',
    desc: 'HMRC self-assessment data. Person-level only (no household structure). Best for income tax modelling at the top of the distribution.',
  },
  {
    id: 'lcfs',
    name: 'LCFS',
    full: 'Living Costs and Food Survey',
    desc: 'Expenditure survey. Required for VAT, fuel duty, alcohol duty, and tobacco duty modelling.',
  },
  {
    id: 'was',
    name: 'WAS',
    full: 'Wealth and Assets Survey',
    desc: 'ONS wealth survey. Required for wealth tax and detailed Capital Gains Tax modelling.',
  },
]

export default function DatasetsSection({ id }) {
  return (
    <section className="section" id={id}>
      <div className="section-tag">06 — Datasets</div>
      <h1>Datasets</h1>
      <p>
        Full-population datasets download automatically to <code>~/.policyengine-uk-data/</code> on first use when{' '}
        <code>POLICYENGINE_UK_DATA_TOKEN</code> is set. Available fiscal years: 1994–2029.
      </p>

      <div className="dataset-grid">
        {DATASETS.map(d => (
          <div key={d.id} className="dataset-card">
            <h4>{d.id}</h4>
            <p style={{ color: 'var(--text3)', fontSize: 11, marginBottom: 4 }}>{d.full}</p>
            <p>{d.desc}</p>
          </div>
        ))}
      </div>

      <h2>API reference</h2>
      <table className="api-table">
        <thead><tr><th>Function / constant</th><th>Description</th></tr></thead>
        <tbody>
          <tr><td>DATASETS</td><td>Tuple of available dataset names: <code>("frs", "efrs", "lcfs", "spi", "was")</code></td></tr>
          <tr>
            <td>download_all(force=False, datasets=DATASETS)</td>
            <td>Download and cache all (or specified) datasets. Set <code>force=True</code> to overwrite existing files.</td>
          </tr>
          <tr>
            <td>capabilities() → dict</td>
            <td>Returns a dict describing which datasets and years are available locally.</td>
          </tr>
          <tr>
            <td>POLICYENGINE_UK_DATA_TOKEN</td>
            <td>Environment variable. Required to download datasets. Hypothetical households work without it.</td>
          </tr>
        </tbody>
      </table>

      <h2>Examples</h2>
      <Code code={downloadCode} label="Downloading datasets" />
      <Code code={useCode} label="Using different datasets" />
      <Code code={yearCode} label="Historical and forecast years" />

      <div className="callout warn">
        <span className="callout-icon">⚠</span>
        <p>
          SPI is a persons-only dataset. <code>Simulation(dataset="spi").run()</code> returns results for income
          tax and NI only — benefit programmes require household structure and will be zeroed.
        </p>
      </div>
    </section>
  )
}
