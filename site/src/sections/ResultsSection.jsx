import { useState } from 'react'
import { Code } from '../components/Code'

function ColGrid({ cols }) {
  return (
    <div className="col-grid">
      {cols.map(([name, prefix]) => (
        <div key={name} className={`col-pill prefix-${prefix}`}>{name}</div>
      ))}
    </div>
  )
}

const PERSON_COLS = [
  ['person_id', 'none'], ['benunit_id', 'none'], ['household_id', 'none'],
  ['age', 'none'], ['gender', 'none'],
  ['employment_income', 'none'], ['self_employment_income', 'none'],
  ['private_pension_income', 'none'], ['state_pension', 'none'],
  ['savings_interest', 'none'], ['dividend_income', 'none'],
  ['capital_gains', 'none'], ['property_income', 'none'],
  ['is_in_scotland', 'none'], ['hours_worked_annual', 'none'],
  ['is_disabled', 'none'], ['is_carer', 'none'],
  ['baseline_income_tax', 'baseline'], ['baseline_employee_ni', 'baseline'],
  ['baseline_employer_ni', 'baseline'], ['baseline_ni_class1_employee', 'baseline'],
  ['baseline_ni_class2', 'baseline'], ['baseline_ni_class4', 'baseline'],
  ['baseline_total_income', 'baseline'], ['baseline_taxable_income', 'baseline'],
  ['baseline_personal_allowance', 'baseline'], ['baseline_capital_gains_tax', 'baseline'],
  ['reform_income_tax', 'reform'], ['reform_employee_ni', 'reform'],
  ['reform_employer_ni', 'reform'], ['reform_total_income', 'reform'],
  ['reform_taxable_income', 'reform'], ['reform_personal_allowance', 'reform'],
  ['reform_capital_gains_tax', 'reform'],
]

const BENUNIT_COLS = [
  ['benunit_id', 'none'], ['household_id', 'none'], ['person_ids', 'none'],
  ['on_uc', 'none'], ['rent_monthly', 'none'], ['is_lone_parent', 'none'],
  ['claims_uc_if_eligible', 'none'],
  ['baseline_universal_credit', 'baseline'], ['baseline_child_benefit', 'baseline'],
  ['baseline_state_pension', 'baseline'], ['baseline_pension_credit', 'baseline'],
  ['baseline_housing_benefit', 'baseline'], ['baseline_child_tax_credit', 'baseline'],
  ['baseline_working_tax_credit', 'baseline'], ['baseline_income_support', 'baseline'],
  ['baseline_esa_income_related', 'baseline'], ['baseline_jsa_income_based', 'baseline'],
  ['baseline_carers_allowance', 'baseline'], ['baseline_scottish_child_payment', 'baseline'],
  ['baseline_benefit_cap_reduction', 'baseline'], ['baseline_passthrough_benefits', 'baseline'],
  ['baseline_total_benefits', 'baseline'],
  ['reform_universal_credit', 'reform'], ['reform_child_benefit', 'reform'],
  ['reform_state_pension', 'reform'], ['reform_pension_credit', 'reform'],
  ['reform_housing_benefit', 'reform'], ['reform_child_tax_credit', 'reform'],
  ['reform_working_tax_credit', 'reform'], ['reform_income_support', 'reform'],
  ['reform_esa_income_related', 'reform'], ['reform_jsa_income_based', 'reform'],
  ['reform_carers_allowance', 'reform'], ['reform_scottish_child_payment', 'reform'],
  ['reform_benefit_cap_reduction', 'reform'], ['reform_passthrough_benefits', 'reform'],
  ['reform_total_benefits', 'reform'],
]

const HH_COLS = [
  ['household_id', 'none'], ['weight', 'none'], ['region', 'none'],
  ['rent_annual', 'none'], ['council_tax_annual', 'none'], ['tenure_type', 'none'],
  ['baseline_net_income', 'baseline'], ['baseline_gross_income', 'baseline'],
  ['baseline_total_tax', 'baseline'], ['baseline_total_benefits', 'baseline'],
  ['baseline_council_tax_calculated', 'baseline'], ['baseline_property_transaction_tax', 'baseline'],
  ['baseline_vat', 'baseline'], ['baseline_fuel_duty', 'baseline'],
  ['baseline_equivalisation_factor', 'baseline'], ['baseline_equivalised_net_income', 'baseline'],
  ['baseline_net_income_ahc', 'baseline'], ['baseline_equivalised_net_income_ahc', 'baseline'],
  ['baseline_in_relative_poverty_bhc', 'baseline'], ['baseline_in_relative_poverty_ahc', 'baseline'],
  ['baseline_in_absolute_poverty_bhc', 'baseline'], ['baseline_in_absolute_poverty_ahc', 'baseline'],
  ['reform_net_income', 'reform'], ['reform_gross_income', 'reform'],
  ['reform_total_tax', 'reform'], ['reform_total_benefits', 'reform'],
  ['reform_council_tax_calculated', 'reform'], ['reform_property_transaction_tax', 'reform'],
  ['reform_vat', 'reform'], ['reform_fuel_duty', 'reform'],
  ['reform_equivalisation_factor', 'reform'], ['reform_equivalised_net_income', 'reform'],
  ['reform_net_income_ahc', 'reform'], ['reform_equivalised_net_income_ahc', 'reform'],
  ['reform_in_relative_poverty_bhc', 'reform'], ['reform_in_relative_poverty_ahc', 'reform'],
  ['reform_in_absolute_poverty_bhc', 'reform'], ['reform_in_absolute_poverty_ahc', 'reform'],
]

const aggregateCode = `result = sim.run(policy=reform)

# Fiscal impact
bi = result.budgetary_impact
print(f"Net cost:        £{bi.net_cost / 1e9:.1f}bn")       # → "Net cost:        £X.Xbn"
print(f"Revenue change:  £{bi.revenue_change / 1e9:.1f}bn")  # → "Revenue change:  £X.Xbn"
print(f"Benefit change:  £{bi.benefit_spending_change / 1e9:.1f}bn")

# Winners and losers
wl = result.winners_losers
print(f"Winners: {wl.winners_pct:.1f}%  avg gain £{wl.avg_gain:.0f}/yr")
# → "Winners: N.N%  avg gain £NNN/yr"
print(f"Losers:  {wl.losers_pct:.1f}%  avg loss £{wl.avg_loss:.0f}/yr")

# Poverty rates (%)
print(f"Baseline child poverty (AHC): {result.baseline_poverty.relative_ahc_children:.1f}%")
print(f"Reform child poverty  (AHC):  {result.reform_poverty.relative_ahc_children:.1f}%")

# HBAI incomes (£/yr)
print(f"Baseline median equiv BHC: £{result.baseline_hbai_incomes.median_equiv_bhc:.0f}")

# Programme breakdown (£/yr)
pb = result.program_breakdown
print(f"Reform UC spend: £{pb.universal_credit / 1e9:.1f}bn")

# Decile impacts — 10 rows, one per decile
for d in result.decile_impacts:
    print(f"Decile {d.decile:2d}: £{d.avg_change:+.0f}/yr  ({d.pct_change:+.1f}%)")
# → "Decile  1: £+NN/yr  (+N.N%)"  ...  "Decile 10: £+NNN/yr  (+N.N%)"`

const microdataCode = `micro = sim.run_microdata(policy=reform)

# Household-level winners and losers
hh = micro.households
hh["income_change"] = hh["reform_net_income"] - hh["baseline_net_income"]
hh["winner"] = hh["income_change"] > 1
# hh.dtypes → household_id: int64, weight: float64, reform_net_income: float64, ...

# Poverty flag: binary 0/1, already computed by the engine
poor_baseline = hh["baseline_in_relative_poverty_ahc"].sum()
poor_reform   = hh["reform_in_relative_poverty_ahc"].sum()
# → counts of households below the poverty line (unweighted)

# Benefit unit UC amounts
bu = micro.benunits
gainers = bu[bu["reform_universal_credit"] > bu["baseline_universal_credit"]]
print(f"{len(gainers)} benefit units gain UC under reform")
# → "N benefit units gain UC under reform"`

function FieldTable({ rows }) {
  return (
    <table className="api-table">
      <thead><tr><th>Field</th><th>Type</th><th>Description</th></tr></thead>
      <tbody>
        {rows.map(([f, t, d]) => (
          <tr key={f}><td>{f}</td><td><code>{t}</code></td><td>{d}</td></tr>
        ))}
      </tbody>
    </table>
  )
}

const SIMULATION_RESULT_FIELDS = [
  ['fiscal_year', 'str', 'Fiscal year string, e.g. "2025/26"'],
  ['budgetary_impact', 'BudgetaryImpact', 'Baseline and reform revenue and benefit totals, plus net cost'],
  ['income_breakdown', 'IncomeBreakdown', 'Weighted aggregate income by source (employment, pension, savings, etc.)'],
  ['program_breakdown', 'ProgramBreakdown', 'Weighted reform totals per programme (taxes and benefits)'],
  ['caseloads', 'Caseloads', 'Weighted counts of claimants/payers under the reform'],
  ['decile_impacts', 'list[DecileImpact]', 'Per-decile average baseline and reform income and change'],
  ['winners_losers', 'WinnersLosers', 'Share of households gaining, losing, or unchanged; average amounts'],
  ['baseline_hbai_incomes', 'HbaiIncomes', 'Baseline mean/median equivalised household income (BHC and AHC)'],
  ['reform_hbai_incomes', 'HbaiIncomes', 'Reform mean/median equivalised household income (BHC and AHC)'],
  ['baseline_poverty', 'PovertyHeadcounts', 'Baseline poverty rates by group, relative and absolute, BHC and AHC'],
  ['reform_poverty', 'PovertyHeadcounts', 'Reform poverty rates'],
  ['cpi_index', 'float', 'CPI index for the simulation year (2010/11 = 100), for deflating to real terms'],
]

const BUDGETARY_FIELDS = [
  ['baseline_revenue', 'float', 'Weighted total tax revenue under baseline (£/yr)'],
  ['reform_revenue', 'float', 'Weighted total tax revenue under reform (£/yr)'],
  ['revenue_change', 'float', 'Reform minus baseline revenue (negative = tax cut)'],
  ['baseline_benefits', 'float', 'Weighted total benefit spending under baseline (£/yr)'],
  ['reform_benefits', 'float', 'Weighted total benefit spending under reform (£/yr)'],
  ['benefit_spending_change', 'float', 'Reform minus baseline benefit spending (positive = more spending)'],
  ['net_cost', 'float', '–revenue_change + benefit_spending_change (positive = net fiscal cost)'],
]

const HBAI_FIELDS = [
  ['mean_equiv_bhc', 'float', 'Person-weighted mean equivalised net income BHC (£/yr)'],
  ['mean_equiv_ahc', 'float', 'Person-weighted mean equivalised net income AHC (£/yr)'],
  ['mean_bhc', 'float', 'Household-weighted mean net income BHC (£/yr)'],
  ['mean_ahc', 'float', 'Household-weighted mean net income AHC (£/yr)'],
  ['median_equiv_bhc', 'float', 'Person-weighted median equivalised net income BHC (£/yr)'],
  ['median_equiv_ahc', 'float', 'Person-weighted median equivalised net income AHC (£/yr)'],
]

const WINNERS_LOSERS_FIELDS = [
  ['winners_pct', 'float', '% of households with income gain > £1/yr'],
  ['losers_pct', 'float', '% of households with income loss > £1/yr'],
  ['unchanged_pct', 'float', '% of households with change within ±£1/yr'],
  ['avg_gain', 'float', 'Average annual gain among winners (£/yr)'],
  ['avg_loss', 'float', 'Average annual loss among losers (£/yr)'],
]

const DECILE_FIELDS = [
  ['decile', 'int', 'Decile number 1–10 (1 = lowest income)'],
  ['avg_baseline_income', 'float', 'Average equivalised income in decile under baseline (£/yr)'],
  ['avg_reform_income', 'float', 'Average equivalised income in decile under reform (£/yr)'],
  ['avg_change', 'float', 'Average change (reform minus baseline) in decile (£/yr)'],
  ['pct_change', 'float', 'Percentage change in average income (%)'],
]

const POVERTY_FIELDS = [
  ['relative_bhc_children', 'float', '% of children below 60% median equivalised income BHC'],
  ['relative_bhc_working_age', 'float', '% of working-age adults below 60% median BHC'],
  ['relative_bhc_pensioners', 'float', '% of pensioners below 60% median BHC'],
  ['relative_ahc_children', 'float', '% of children below 60% median equivalised income AHC'],
  ['relative_ahc_working_age', 'float', '% of working-age adults below 60% median AHC'],
  ['relative_ahc_pensioners', 'float', '% of pensioners below 60% median AHC'],
  ['absolute_bhc_children', 'float', '% of children below 2010/11 absolute line uprated by CPI, BHC'],
  ['absolute_bhc_working_age', 'float', '% of working-age adults below absolute line, BHC'],
  ['absolute_bhc_pensioners', 'float', '% of pensioners below absolute line, BHC'],
  ['absolute_ahc_children', 'float', '% of children below absolute line, AHC'],
  ['absolute_ahc_working_age', 'float', '% of working-age adults below absolute line, AHC'],
  ['absolute_ahc_pensioners', 'float', '% of pensioners below absolute line, AHC'],
]

function Expandable({ title, children }) {
  const [open, setOpen] = useState(false)
  return (
    <div className="field-group" style={{ marginBottom: 8 }}>
      <div className="field-group-header" onClick={() => setOpen(o => !o)}>
        <span>{title}</span>
        <span className={`field-group-chevron ${open ? 'open' : ''}`}>▶</span>
      </div>
      {open && <div style={{ padding: '12px 16px' }}>{children}</div>}
    </div>
  )
}

function MicrodataTab({ label, cols }) {
  const legend = [
    { cls: 'prefix-none', label: 'input / identifier' },
    { cls: 'prefix-baseline', label: 'baseline_* output' },
    { cls: 'prefix-reform', label: 'reform_* output' },
  ]
  return (
    <div>
      <div style={{ display: 'flex', gap: 16, marginBottom: 12, flexWrap: 'wrap' }}>
        {legend.map(l => (
          <div key={l.cls} style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11, color: 'var(--text3)' }}>
            <div className={`col-pill ${l.cls}`} style={{ padding: '2px 8px' }}>{l.label}</div>
          </div>
        ))}
      </div>
      <ColGrid cols={cols} />
    </div>
  )
}

export default function ResultsSection({ id }) {
  const [microTab, setMicroTab] = useState(0)
  const microTabs = [
    { label: 'persons', cols: PERSON_COLS },
    { label: 'benunits', cols: BENUNIT_COLS },
    { label: 'households', cols: HH_COLS },
  ]

  return (
    <section className="section" id={id}>
      <div className="section-tag">04 — Results</div>
      <h1>SimulationResult &amp; MicrodataResult</h1>
      <p>
        <code>sim.run()</code> returns a <code>SimulationResult</code> — aggregate statistics across the whole
        population. <code>sim.run_microdata()</code> returns a <code>MicrodataResult</code> — per-entity DataFrames
        with one row per person, benefit unit, and household.
      </p>

      <h2>SimulationResult</h2>
      <FieldTable rows={SIMULATION_RESULT_FIELDS} />

      <h3>BudgetaryImpact</h3>
      <FieldTable rows={BUDGETARY_FIELDS} />

      <h3>HbaiIncomes</h3>
      <p style={{ fontSize: 12, color: 'var(--text3)', marginBottom: 8 }}>
        Applies to both <code>baseline_hbai_incomes</code> and <code>reform_hbai_incomes</code>.
        BHC = before housing costs; AHC = after housing costs.
      </p>
      <FieldTable rows={HBAI_FIELDS} />

      <h3>WinnersLosers</h3>
      <FieldTable rows={WINNERS_LOSERS_FIELDS} />

      <h3>DecileImpact</h3>
      <FieldTable rows={DECILE_FIELDS} />

      <h3>PovertyHeadcounts</h3>
      <p style={{ fontSize: 12, color: 'var(--text3)', marginBottom: 8 }}>
        Relative poverty line = 60% of the baseline person-weighted median equivalised income.
        Absolute poverty line = 2010/11 reference uprated by CPI to the simulation year.
      </p>
      <FieldTable rows={POVERTY_FIELDS} />

      <Expandable title="ProgramBreakdown fields">
        <FieldTable rows={[
          ['income_tax','float','Total income tax under reform (£/yr)'],
          ['hicbc','float','High Income Child Benefit Charge (£/yr)'],
          ['employee_ni','float','Employee National Insurance (£/yr)'],
          ['employer_ni','float','Employer National Insurance (£/yr)'],
          ['vat','float','VAT (£/yr)'],
          ['fuel_duty','float','Fuel duty (£/yr)'],
          ['alcohol_duty','float','Alcohol duty (£/yr)'],
          ['tobacco_duty','float','Tobacco duty (£/yr)'],
          ['capital_gains_tax','float','Capital Gains Tax (£/yr)'],
          ['stamp_duty','float','Stamp Duty / LBTT / LTT (£/yr)'],
          ['wealth_tax','float','Wealth tax (£/yr)'],
          ['council_tax','float','Council tax (£/yr)'],
          ['universal_credit','float','Universal Credit (£/yr)'],
          ['child_benefit','float','Child Benefit (£/yr)'],
          ['state_pension','float','State Pension (£/yr)'],
          ['pension_credit','float','Pension Credit (£/yr)'],
          ['housing_benefit','float','Housing Benefit (£/yr)'],
          ['child_tax_credit','float','Child Tax Credit (£/yr)'],
          ['working_tax_credit','float','Working Tax Credit (£/yr)'],
          ['income_support','float','Income Support (£/yr)'],
          ['esa_income_related','float','ESA income-related (£/yr)'],
          ['jsa_income_based','float','JSA income-based (£/yr)'],
          ['carers_allowance','float','Carer\'s Allowance (£/yr)'],
          ['scottish_child_payment','float','Scottish Child Payment (£/yr)'],
          ['benefit_cap_reduction','float','Benefit cap reduction (negative = reduction in awards) (£/yr)'],
          ['passthrough_benefits','float','Passthrough benefits (£/yr)'],
        ]} />
      </Expandable>

      <Expandable title="Caseloads fields">
        <FieldTable rows={[
          ['income_tax_payers','float','Weighted count of people paying income tax'],
          ['ni_payers','float','Weighted count of people paying employee NI'],
          ['employer_ni_payers','float','Weighted count of people with employer NI liability'],
          ['universal_credit','float','Weighted count of benefit units receiving UC'],
          ['child_benefit','float','Weighted count of benefit units receiving Child Benefit'],
          ['state_pension','float','Weighted count of benefit units receiving State Pension'],
          ['pension_credit','float','Weighted count of benefit units receiving Pension Credit'],
          ['housing_benefit','float','Weighted count of benefit units receiving Housing Benefit'],
          ['child_tax_credit','float','Weighted count of benefit units receiving CTC'],
          ['working_tax_credit','float','Weighted count of benefit units receiving WTC'],
          ['income_support','float','Weighted count of benefit units receiving Income Support'],
          ['esa_income_related','float','Weighted count receiving ESA (IR)'],
          ['jsa_income_based','float','Weighted count receiving JSA (IB)'],
          ['carers_allowance','float','Weighted count receiving Carer\'s Allowance'],
          ['scottish_child_payment','float','Weighted count receiving Scottish Child Payment'],
          ['benefit_cap_affected','float','Weighted count of benefit units subject to the benefit cap'],
        ]} />
      </Expandable>

      <Code code={aggregateCode} label="Working with SimulationResult" />

      <hr className="divider" />

      <h2>MicrodataResult</h2>
      <p>
        Three DataFrames — <code>persons</code>, <code>benunits</code>, <code>households</code> — each with input
        columns plus <code>baseline_*</code> and <code>reform_*</code> output columns side by side.
      </p>

      <div className="tabs" style={{ marginTop: 20 }}>
        <div className="tab-list">
          {microTabs.map((t, i) => (
            <button key={t.label} className={`tab-btn ${i === microTab ? 'active' : ''}`} onClick={() => setMicroTab(i)}>
              .{t.label}
            </button>
          ))}
        </div>
        <div style={{ border: '1px solid var(--border)', borderTop: 'none', padding: 16, background: 'var(--bg2)', borderRadius: '0 0 8px 8px' }}>
          <MicrodataTab {...microTabs[microTab]} />
        </div>
      </div>

      <Code code={microdataCode} label="Working with MicrodataResult" />
    </section>
  )
}
