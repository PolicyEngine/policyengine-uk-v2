import { useState } from 'react'
import { Code } from '../components/Code'

const PARAM_GROUPS = [
  {
    key: 'income_tax',
    cls: 'IncomeTaxParams',
    fields: [
      ['personal_allowance', 'float', 'Income tax personal allowance (£/yr)'],
      ['pa_taper_threshold', 'float', 'Income above which personal allowance tapers away (£/yr)'],
      ['pa_taper_rate', 'float', 'Rate at which personal allowance tapers'],
      ['uk_brackets', 'list[TaxBracket]', 'UK income tax rate bands; list of {rate, threshold} objects'],
      ['scottish_brackets', 'list[TaxBracket]', 'Scottish income tax rate bands'],
      ['dividend_allowance', 'float', 'Dividend allowance (£/yr)'],
      ['dividend_basic_rate', 'float', 'Dividend tax rate for basic-rate taxpayers'],
      ['dividend_higher_rate', 'float', 'Dividend tax rate for higher-rate taxpayers'],
      ['dividend_additional_rate', 'float', 'Dividend tax rate for additional-rate taxpayers'],
      ['savings_starter_rate_band', 'float', 'Savings starter rate band (£/yr)'],
      ['marriage_allowance_max_fraction', 'float', 'Maximum fraction of personal allowance transferable'],
      ['marriage_allowance_rounding', 'float', 'Rounding applied to marriage allowance transfer'],
    ],
  },
  {
    key: 'national_insurance',
    cls: 'NationalInsuranceParams',
    fields: [
      ['primary_threshold_annual', 'float', 'Employee NI primary threshold (£/yr)'],
      ['upper_earnings_limit_annual', 'float', 'Upper earnings limit (£/yr)'],
      ['main_rate', 'float', 'Employee NI main rate (below UEL)'],
      ['additional_rate', 'float', 'Employee NI additional rate (above UEL)'],
      ['secondary_threshold_annual', 'float', 'Employer NI secondary threshold (£/yr)'],
      ['employer_rate', 'float', 'Employer NI rate'],
      ['class2_flat_rate_weekly', 'float', 'Class 2 NI flat rate (£/wk)'],
      ['class2_small_profits_threshold', 'float', 'Class 2 small profits threshold (£/yr)'],
      ['class4_lower_profits_limit', 'float', 'Class 4 NI lower profits limit (£/yr)'],
      ['class4_upper_profits_limit', 'float', 'Class 4 NI upper profits limit (£/yr)'],
      ['class4_main_rate', 'float', 'Class 4 main rate'],
      ['class4_additional_rate', 'float', 'Class 4 additional rate (above UPL)'],
    ],
  },
  {
    key: 'universal_credit',
    cls: 'UniversalCreditParams',
    fields: [
      ['standard_allowance_single_under25', 'float', 'Single claimant under 25 (£/month)'],
      ['standard_allowance_single_over25', 'float', 'Single claimant 25+ (£/month)'],
      ['standard_allowance_couple_under25', 'float', 'Couple, both under 25 (£/month)'],
      ['standard_allowance_couple_over25', 'float', 'Couple, at least one 25+ (£/month)'],
      ['child_element_first', 'float', 'Child element for first/only child (£/month)'],
      ['child_element_subsequent', 'float', 'Child element for subsequent children (£/month)'],
      ['disabled_child_lower', 'float', 'Disabled child element — lower rate (£/month)'],
      ['disabled_child_higher', 'float', 'Disabled child element — higher rate (£/month)'],
      ['lcwra_element', 'float', 'Limited capability for work-related activity element (£/month)'],
      ['carer_element', 'float', 'Carer element (£/month)'],
      ['taper_rate', 'float', 'UC taper rate (fraction of earned income above work allowance)'],
      ['work_allowance_higher', 'float', 'Higher work allowance — no housing costs element (£/month)'],
      ['work_allowance_lower', 'float', 'Lower work allowance — housing costs element present (£/month)'],
      ['child_limit', 'int', 'Maximum number of children eligible for child element'],
    ],
  },
  {
    key: 'child_benefit',
    cls: 'ChildBenefitParams',
    fields: [
      ['eldest_weekly', 'float', 'Weekly rate for eldest/only child (£/wk)'],
      ['additional_weekly', 'float', 'Weekly rate for additional children (£/wk)'],
      ['hicbc_threshold', 'float', 'High Income Child Benefit Charge threshold (£/yr)'],
      ['hicbc_taper_end', 'float', 'Income at which Child Benefit is fully clawed back (£/yr)'],
    ],
  },
  {
    key: 'state_pension',
    cls: 'StatePensionParams',
    fields: [
      ['new_state_pension_weekly', 'float', 'New State Pension full weekly rate (£/wk)'],
    ],
  },
  {
    key: 'pension_credit',
    cls: 'PensionCreditParams',
    fields: [
      ['standard_minimum_single', 'float', 'Guarantee credit minimum income — single (£/wk)'],
      ['standard_minimum_couple', 'float', 'Guarantee credit minimum income — couple (£/wk)'],
      ['savings_credit_threshold_single', 'float', 'Savings credit threshold — single (£/wk)'],
      ['savings_credit_threshold_couple', 'float', 'Savings credit threshold — couple (£/wk)'],
    ],
  },
  {
    key: 'benefit_cap',
    cls: 'BenefitCapParams',
    fields: [
      ['single_london', 'float', 'Benefit cap — single claimant, London (£/wk)'],
      ['single_outside_london', 'float', 'Benefit cap — single claimant, outside London (£/wk)'],
      ['non_single_london', 'float', 'Benefit cap — couples/families, London (£/wk)'],
      ['non_single_outside_london', 'float', 'Benefit cap — couples/families, outside London (£/wk)'],
      ['earnings_exemption_threshold', 'float', 'Weekly earnings above which the cap does not apply (£/wk)'],
    ],
  },
  {
    key: 'housing_benefit',
    cls: 'HousingBenefitParams',
    fields: [
      ['withdrawal_rate', 'float', 'Taper rate above applicable amount'],
      ['personal_allowance_single_under25', 'float', 'Applicable amount — single under 25 (£/wk)'],
      ['personal_allowance_single_25_plus', 'float', 'Applicable amount — single 25+ (£/wk)'],
      ['personal_allowance_couple', 'float', 'Applicable amount — couple (£/wk)'],
      ['child_allowance', 'float', 'Child allowance per child (£/wk)'],
      ['family_premium', 'float', 'Family premium (£/wk)'],
    ],
  },
  {
    key: 'tax_credits',
    cls: 'TaxCreditsParams',
    fields: [
      ['wtc_basic_element', 'float', 'WTC basic element (£/yr)'],
      ['wtc_couple_element', 'float', 'WTC couple element (£/yr)'],
      ['wtc_lone_parent_element', 'float', 'WTC lone parent element (£/yr)'],
      ['wtc_30_hour_element', 'float', 'WTC 30-hour element (£/yr)'],
      ['ctc_child_element', 'float', 'CTC child element per child (£/yr)'],
      ['ctc_family_element', 'float', 'CTC family element (£/yr)'],
      ['ctc_disabled_child_element', 'float', 'CTC disabled child element (£/yr)'],
      ['ctc_severely_disabled_child_element', 'float', 'CTC severely disabled child element (£/yr)'],
      ['income_threshold', 'float', 'Income threshold before taper (£/yr)'],
      ['taper_rate', 'float', 'Award reduction rate above income threshold'],
      ['wtc_min_hours_single', 'float', 'Minimum hours for single WTC claimant'],
      ['wtc_min_hours_couple', 'float', 'Minimum hours for couple WTC claimant'],
    ],
  },
  {
    key: 'capital_gains_tax',
    cls: 'CapitalGainsTaxParams',
    fields: [
      ['annual_exempt_amount', 'float', 'Annual exempt amount (£/yr)'],
      ['basic_rate', 'float', 'CGT rate for basic-rate taxpayers'],
      ['higher_rate', 'float', 'CGT rate for higher/additional-rate taxpayers'],
      ['residential_surcharge', 'float', 'Additional rate on residential property gains (0.0 from April 2025)'],
    ],
  },
  {
    key: 'stamp_duty',
    cls: 'StampDutyParams',
    fields: [
      ['bands', 'list[StampDutyBand]', 'Rate bands: list of {rate: float, threshold: float} objects'],
      ['annual_purchase_probability', 'float', 'Annual probability of purchase (for annualised SDLT)'],
    ],
  },
  {
    key: 'wealth_tax',
    cls: 'WealthTaxParams',
    fields: [
      ['enabled', 'bool', 'Whether the wealth tax is active'],
      ['threshold', 'float', 'Wealth above which the tax applies (£)'],
      ['rate', 'float', 'Annual rate applied to wealth above threshold'],
    ],
  },
  {
    key: 'scottish_child_payment',
    cls: 'ScottishChildPaymentParams',
    fields: [
      ['weekly_amount', 'float', 'Weekly payment amount (£/wk)'],
      ['max_age', 'float', 'Maximum child age for eligibility'],
    ],
  },
  {
    key: 'council_tax',
    cls: 'CouncilTaxParams',
    fields: [
      ['average_band_d', 'float', 'Average Band D council tax (£/yr)'],
      ['band_multipliers', 'list[float]', 'Multipliers for bands A–H relative to Band D'],
      ['band_thresholds', 'list[float]', 'Property value thresholds for each band (£)'],
      ['single_person_discount_rate', 'float', 'Single-person discount rate (default 0.25)'],
    ],
  },
  {
    key: 'dla',
    cls: 'DlaParams',
    fields: [
      ['care_low_weekly', 'float', 'DLA care component — lowest rate (£/wk)'],
      ['care_mid_weekly', 'float', 'DLA care component — middle rate (£/wk)'],
      ['care_high_weekly', 'float', 'DLA care component — highest rate (£/wk)'],
      ['mobility_low_weekly', 'float', 'DLA mobility component — lower rate (£/wk)'],
      ['mobility_high_weekly', 'float', 'DLA mobility component — higher rate (£/wk)'],
    ],
  },
  {
    key: 'aa',
    cls: 'AaParams',
    fields: [
      ['low_weekly', 'float', 'Attendance Allowance — lower rate (£/wk)'],
      ['high_weekly', 'float', 'Attendance Allowance — higher rate (£/wk)'],
    ],
  },
  {
    key: 'pip',
    cls: 'PipParams',
    fields: [
      ['daily_living_standard_weekly', 'float', 'PIP daily living — standard rate (£/wk)'],
      ['daily_living_enhanced_weekly', 'float', 'PIP daily living — enhanced rate (£/wk)'],
      ['mobility_standard_weekly', 'float', 'PIP mobility — standard rate (£/wk)'],
      ['mobility_enhanced_weekly', 'float', 'PIP mobility — enhanced rate (£/wk)'],
    ],
  },
  {
    key: 'labour_supply',
    cls: 'LabourSupplyParams',
    fields: [
      ['enabled', 'bool', 'Enable OBR labour supply responses (default: False)'],
      ['subst_married_women_no_children', 'float', 'Substitution elasticity — married women, no children'],
      ['subst_married_women_child_0_2', 'float', 'Substitution elasticity — married women, child 0–2'],
      ['subst_married_women_child_3_4', 'float', 'Substitution elasticity — married women, child 3–4'],
      ['subst_married_women_child_5_10', 'float', 'Substitution elasticity — married women, child 5–10'],
      ['subst_married_women_child_11_plus', 'float', 'Substitution elasticity — married women, child 11+'],
      ['subst_lone_parents_child_0_4', 'float', 'Substitution elasticity — lone parents, child 0–4'],
      ['subst_lone_parents_child_5_10', 'float', 'Substitution elasticity — lone parents, child 5–10'],
      ['subst_lone_parents_child_11_18', 'float', 'Substitution elasticity — lone parents, child 11–18'],
      ['subst_men_and_single_women', 'float', 'Substitution elasticity — men and single women'],
      ['income_married_women_no_children', 'float', 'Income elasticity — married women, no children'],
      ['income_men_and_single_women', 'float', 'Income elasticity — men and single women'],
    ],
  },
]

const exampleCode = `from policyengine_uk_compiled import (
    Parameters, IncomeTaxParams, NationalInsuranceParams,
    UniversalCreditParams, TaxBracket,
)

# Raise personal allowance, cut NI main rate, reduce UC taper
reform = Parameters(
    income_tax=IncomeTaxParams(
        personal_allowance=15_000,
    ),
    national_insurance=NationalInsuranceParams(
        main_rate=0.08,
    ),
    universal_credit=UniversalCreditParams(
        taper_rate=0.50,
    ),
)

result = sim.run(policy=reform)
print(f"Net cost: £{result.budgetary_impact.net_cost / 1e9:.1f}bn")`

const bracketCode = `from policyengine_uk_compiled import Parameters, IncomeTaxParams, TaxBracket

# Introduce a new 45% band starting at £125,140
reform = Parameters(
    income_tax=IncomeTaxParams(
        uk_brackets=[
            TaxBracket(rate=0.20, threshold=12_570),
            TaxBracket(rate=0.40, threshold=50_270),
            TaxBracket(rate=0.45, threshold=125_140),
        ]
    )
)
result = sim.run(policy=reform)`

const labourSupplyCode = `from policyengine_uk_compiled import Parameters, NationalInsuranceParams, LabourSupplyParams

# NI cut with labour supply responses
reform = Parameters(
    national_insurance=NationalInsuranceParams(main_rate=0.10),
    labour_supply=LabourSupplyParams(enabled=True),
)
result = sim.run(policy=reform)`

function FieldGroup({ group }) {
  const [open, setOpen] = useState(false)
  return (
    <div className="field-group">
      <div className="field-group-header" onClick={() => setOpen(o => !o)}>
        <span>{group.key}</span>
        <span style={{ color: 'var(--text2)', fontSize: 11, marginLeft: 8 }}>{group.cls}</span>
        <span className={`field-group-chevron ${open ? 'open' : ''}`}>▶</span>
      </div>
      {open && (
        <div className="field-group-body">
          {group.fields.map(([name, type, desc]) => (
            <div key={name} className="field-row">
              <div className="field-name">{name}</div>
              <div className="field-desc">
                <span style={{ color: 'var(--text3)', fontSize: 11, marginRight: 6 }}>{type}</span>
                {desc}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

export default function ParametersSection({ id }) {
  return (
    <section className="section" id={id}>
      <div className="section-tag">03 — Parameters</div>
      <h1>Parameters</h1>
      <p>
        The reform overlay. All fields are optional — omit any sub-object or field to keep its baseline value.
        Only the fields you set are changed; the rest remain as calibrated for the fiscal year.
      </p>
      <p>
        Import sub-objects individually: <code>from policyengine_uk_compiled import IncomeTaxParams</code>, etc.
      </p>

      <div className="callout info">
        <span className="callout-icon">ℹ</span>
        <p>
          All monetary values are annual (£/yr) unless the field name says <code>_weekly</code> or{' '}
          <code>_monthly</code>. Rates are fractions (e.g. <code>taper_rate=0.55</code>), not percentages.
        </p>
      </div>

      <h2>Reference — click any group to expand</h2>
      {PARAM_GROUPS.map(g => <FieldGroup key={g.key} group={g} />)}

      <h2>Examples</h2>
      <Code code={exampleCode} label="Multi-programme reform" />
      <Code code={bracketCode} label="Custom tax brackets" />
      <Code code={labourSupplyCode} label="Labour supply responses" />
    </section>
  )
}
