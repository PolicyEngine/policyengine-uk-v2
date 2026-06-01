"""Pydantic models mirroring the Rust parameter and output structures."""

from __future__ import annotations

from typing import Any, Optional
from pydantic import BaseModel, Field


# ── Parameter models (mirror src/parameters/mod.rs) ──────────────────────────


class TaxBracket(BaseModel):
    rate: float
    threshold: float


class IncomeTaxParams(BaseModel):
    personal_allowance: Optional[float] = None
    pa_taper_threshold: Optional[float] = None
    pa_taper_rate: Optional[float] = None
    uk_brackets: Optional[list[TaxBracket]] = None
    scottish_brackets: Optional[list[TaxBracket]] = None
    dividend_allowance: Optional[float] = None
    dividend_basic_rate: Optional[float] = None
    dividend_higher_rate: Optional[float] = None
    dividend_additional_rate: Optional[float] = None
    savings_starter_rate_band: Optional[float] = None
    marriage_allowance_max_fraction: Optional[float] = None
    marriage_allowance_rounding: Optional[float] = None


class NationalInsuranceParams(BaseModel):
    primary_threshold_annual: Optional[float] = None
    upper_earnings_limit_annual: Optional[float] = None
    main_rate: Optional[float] = None
    additional_rate: Optional[float] = None
    secondary_threshold_annual: Optional[float] = None
    employer_rate: Optional[float] = None
    class2_flat_rate_weekly: Optional[float] = None
    class2_small_profits_threshold: Optional[float] = None
    class4_lower_profits_limit: Optional[float] = None
    class4_upper_profits_limit: Optional[float] = None
    class4_main_rate: Optional[float] = None
    class4_additional_rate: Optional[float] = None


class UniversalCreditParams(BaseModel):
    standard_allowance_single_under25: Optional[float] = None
    standard_allowance_single_over25: Optional[float] = None
    standard_allowance_couple_under25: Optional[float] = None
    standard_allowance_couple_over25: Optional[float] = None
    child_element_first: Optional[float] = None
    child_element_subsequent: Optional[float] = None
    disabled_child_lower: Optional[float] = None
    disabled_child_higher: Optional[float] = None
    lcwra_element: Optional[float] = None
    carer_element: Optional[float] = None
    taper_rate: Optional[float] = None
    work_allowance_higher: Optional[float] = None
    work_allowance_lower: Optional[float] = None
    child_limit: Optional[int] = None


class ChildBenefitParams(BaseModel):
    eldest_weekly: Optional[float] = None
    additional_weekly: Optional[float] = None
    hicbc_threshold: Optional[float] = None
    hicbc_taper_end: Optional[float] = None


class StatePensionParams(BaseModel):
    new_state_pension_weekly: Optional[float] = None
    old_basic_pension_weekly: Optional[float] = None


class PensionCreditParams(BaseModel):
    standard_minimum_single: Optional[float] = None
    standard_minimum_couple: Optional[float] = None
    savings_credit_threshold_single: Optional[float] = None
    savings_credit_threshold_couple: Optional[float] = None


class BenefitCapParams(BaseModel):
    single_london: Optional[float] = None
    single_outside_london: Optional[float] = None
    non_single_london: Optional[float] = None
    non_single_outside_london: Optional[float] = None
    earnings_exemption_threshold: Optional[float] = None


class HousingBenefitParams(BaseModel):
    withdrawal_rate: Optional[float] = None
    personal_allowance_single_under25: Optional[float] = None
    personal_allowance_single_25_plus: Optional[float] = None
    personal_allowance_couple: Optional[float] = None
    child_allowance: Optional[float] = None
    family_premium: Optional[float] = None


class TaxCreditsParams(BaseModel):
    wtc_basic_element: Optional[float] = None
    wtc_couple_element: Optional[float] = None
    wtc_lone_parent_element: Optional[float] = None
    wtc_30_hour_element: Optional[float] = None
    ctc_child_element: Optional[float] = None
    ctc_family_element: Optional[float] = None
    ctc_disabled_child_element: Optional[float] = None
    ctc_severely_disabled_child_element: Optional[float] = None
    income_threshold: Optional[float] = None
    taper_rate: Optional[float] = None
    wtc_min_hours_single: Optional[float] = None
    wtc_min_hours_couple: Optional[float] = None


class ScottishChildPaymentParams(BaseModel):
    weekly_amount: Optional[float] = None
    max_age: Optional[float] = None


class DisabilityPremiumParams(BaseModel):
    disability_premium_single: Optional[float] = None
    disability_premium_couple: Optional[float] = None
    enhanced_disability_premium_single: Optional[float] = None
    enhanced_disability_premium_couple: Optional[float] = None
    severe_disability_premium: Optional[float] = None
    carer_premium: Optional[float] = None


class IncomeRelatedBenefitParams(BaseModel):
    esa_allowance_single_under25: Optional[float] = None
    esa_allowance_single_25_plus: Optional[float] = None
    esa_allowance_couple: Optional[float] = None
    esa_wrag_component: Optional[float] = None
    esa_support_component: Optional[float] = None
    jsa_allowance_single_under25: Optional[float] = None
    jsa_allowance_single_25_plus: Optional[float] = None
    jsa_allowance_couple: Optional[float] = None
    carers_allowance_weekly: Optional[float] = None
    ca_earnings_disregard_weekly: Optional[float] = None
    ca_min_hours_caring: Optional[float] = None
    ca_care_recipient_min_age: Optional[float] = None


class UcMigrationRates(BaseModel):
    housing_benefit: Optional[float] = None
    tax_credits: Optional[float] = None
    income_support: Optional[float] = None


class CouncilTaxParams(BaseModel):
    """Council tax parameters.

    Local Government Finance Act 1992. Used for reform modelling — baseline
    runs use the FRS-recorded `council_tax` amount per household. Set
    `single_person_discount_rate` to model reforms to the s.11(1)(a) discount.
    """
    average_band_d: Optional[float] = None
    band_multipliers: Optional[list[float]] = None
    band_thresholds: Optional[list[float]] = None
    single_person_discount_rate: Optional[float] = None


class StampDutyBand(BaseModel):
    rate: float
    threshold: float


class StampDutyParams(BaseModel):
    bands: Optional[list[StampDutyBand]] = None
    annual_purchase_probability: Optional[float] = None


class CapitalGainsTaxParams(BaseModel):
    annual_exempt_amount: Optional[float] = None
    basic_rate: Optional[float] = None
    higher_rate: Optional[float] = None
    # Additional rate (in points) on the residential-property slice of taxable
    # gains; 0.0 from April 2025 onwards. Reforms can set non-zero to model a
    # re-introduced residential surcharge.
    residential_surcharge: Optional[float] = None


class WealthTaxParams(BaseModel):
    enabled: Optional[bool] = None
    threshold: Optional[float] = None
    rate: Optional[float] = None


class DlaParams(BaseModel):
    """Disability Living Allowance weekly rates.

    SSCBA 1992 Sch.2 paras 2–3. Recipients are identified by the
    `dla_care_low` / `dla_care_mid` / `dla_care_high` and
    `dla_mob_low` / `dla_mob_high` flags on each Person.
    """
    care_low_weekly:     Optional[float] = None
    care_mid_weekly:     Optional[float] = None
    care_high_weekly:    Optional[float] = None
    mobility_low_weekly: Optional[float] = None
    mobility_high_weekly: Optional[float] = None


class AaParams(BaseModel):
    """Attendance Allowance weekly rates.

    SSCBA 1992 s.64. Recipients are identified by the `aa_low` / `aa_high`
    flags on each Person.
    """
    low_weekly:  Optional[float] = None
    high_weekly: Optional[float] = None
class PipParams(BaseModel):
    """Personal Independence Payment weekly rates.

    Welfare Reform Act 2012 s.79; SI 2013/377. Set any of the four weekly
    rates to model PIP-rate reforms; recipients are identified by the
    `pip_dl_std` / `pip_dl_enh` / `pip_mob_std` / `pip_mob_enh` flags on
    each Person.
    """
    daily_living_standard_weekly: Optional[float] = None
    daily_living_enhanced_weekly: Optional[float] = None
    mobility_standard_weekly:     Optional[float] = None
    mobility_enhanced_weekly:     Optional[float] = None


class LabourSupplyParams(BaseModel):
    """OBR labour supply elasticities (Slutsky decomposition).

    Source: OBR (2023) "Costing a cut in National Insurance contributions:
    the impact on labour supply"
    https://obr.uk/docs/dlm_uploads/NICS-Cut-Impact-on-Labour-Supply-Note.pdf

    Set `enabled=False` to suppress labour supply responses. All elasticity
    fields are optional; omitted fields retain OBR defaults.
    """

    enabled: Optional[bool] = None

    # Substitution elasticities (Table A1)
    subst_married_women_no_children: Optional[float] = None
    subst_married_women_child_0_2: Optional[float] = None
    subst_married_women_child_3_4: Optional[float] = None
    subst_married_women_child_5_10: Optional[float] = None
    subst_married_women_child_11_plus: Optional[float] = None
    subst_lone_parents_child_0_4: Optional[float] = None
    subst_lone_parents_child_5_10: Optional[float] = None
    subst_lone_parents_child_11_18: Optional[float] = None
    subst_men_and_single_women: Optional[float] = None

    # Income elasticities (Table A2)
    income_married_women_no_children: Optional[float] = None
    income_married_women_child_0_2: Optional[float] = None
    income_married_women_child_3_4: Optional[float] = None
    income_married_women_child_5_10: Optional[float] = None
    income_married_women_child_11_plus: Optional[float] = None
    income_lone_parents_child_0_4: Optional[float] = None
    income_lone_parents_child_5_10: Optional[float] = None
    income_lone_parents_child_11_18: Optional[float] = None
    income_men_and_single_women: Optional[float] = None


class Parameters(BaseModel):
    """Full parameter set. All fields optional for use as reform overlay."""

    fiscal_year: Optional[str] = None
    income_tax: Optional[IncomeTaxParams] = None
    national_insurance: Optional[NationalInsuranceParams] = None
    universal_credit: Optional[UniversalCreditParams] = None
    child_benefit: Optional[ChildBenefitParams] = None
    state_pension: Optional[StatePensionParams] = None
    pension_credit: Optional[PensionCreditParams] = None
    benefit_cap: Optional[BenefitCapParams] = None
    housing_benefit: Optional[HousingBenefitParams] = None
    tax_credits: Optional[TaxCreditsParams] = None
    scottish_child_payment: Optional[ScottishChildPaymentParams] = None
    uc_migration: Optional[UcMigrationRates] = None
    disability_premiums: Optional[DisabilityPremiumParams] = None
    income_related_benefits: Optional[IncomeRelatedBenefitParams] = None
    council_tax: Optional[CouncilTaxParams] = None
    capital_gains_tax: Optional[CapitalGainsTaxParams] = None
    stamp_duty: Optional[StampDutyParams] = None
    dla:  Optional["DlaParams"] = None
    aa:   Optional["AaParams"] = None
    lbtt: Optional[StampDutyParams] = None
    ltt:  Optional[StampDutyParams] = None
    pip:  Optional["PipParams"] = None
    wealth_tax: Optional[WealthTaxParams] = None
    labour_supply: Optional[LabourSupplyParams] = None


# ── Simulation config ─────────────────────────────────────────────────────────


class SimulationConfig(BaseModel):
    """Configuration for running a simulation."""

    year: int = 2025
    policy: Optional[Parameters] = Field(
        None, description="Reform parameters (overlay on baseline)"
    )
    clean_frs_base: Optional[str] = Field(
        None, description="Base dir with per-year clean FRS subdirs"
    )
    clean_frs: Optional[str] = Field(
        None, description="Single clean FRS directory"
    )
    frs_raw: Optional[str] = Field(
        None, description="Base dir with per-year raw FRS tab files"
    )
    binary_path: Optional[str] = Field(
        None, description="Path to compiled policyengine-uk-rust binary"
    )


# ── Output models (mirror JsonOutput in src/main.rs) ─────────────────────────


class BudgetaryImpact(BaseModel):
    baseline_revenue: float
    reform_revenue: float
    revenue_change: float
    baseline_benefits: float
    reform_benefits: float
    benefit_spending_change: float
    net_cost: float


class IncomeBreakdown(BaseModel):
    employment_income: float
    self_employment_income: float
    pension_income: float
    savings_interest_income: float
    dividend_income: float
    property_income: float
    other_income: float


class ProgramBreakdown(BaseModel):
    income_tax: float
    hicbc: float = 0.0
    employee_ni: float
    employer_ni: float
    vat: float = 0.0
    fuel_duty: float = 0.0
    alcohol_duty: float = 0.0
    tobacco_duty: float = 0.0
    capital_gains_tax: float = 0.0
    stamp_duty: float = 0.0
    wealth_tax: float = 0.0
    council_tax: float = 0.0
    universal_credit: float
    child_benefit: float
    state_pension: float
    pension_credit: float
    housing_benefit: float
    child_tax_credit: float
    working_tax_credit: float
    income_support: float
    esa_income_related: float
    jsa_income_based: float
    carers_allowance: float
    scottish_child_payment: float
    benefit_cap_reduction: float
    passthrough_benefits: float


class Caseloads(BaseModel):
    income_tax_payers: float
    ni_payers: float
    employer_ni_payers: float
    universal_credit: float
    child_benefit: float
    state_pension: float
    pension_credit: float
    housing_benefit: float
    child_tax_credit: float
    working_tax_credit: float
    income_support: float
    esa_income_related: float
    jsa_income_based: float
    carers_allowance: float
    scottish_child_payment: float
    benefit_cap_affected: float


class DecileImpact(BaseModel):
    decile: int
    avg_baseline_income: Optional[float] = None
    avg_reform_income: Optional[float] = None
    avg_change: Optional[float] = None
    pct_change: Optional[float] = None


class WinnersLosers(BaseModel):
    winners_pct: float
    losers_pct: float
    unchanged_pct: float
    avg_gain: float
    avg_loss: float


class HbaiIncomes(BaseModel):
    mean_equiv_bhc: float
    mean_equiv_ahc: float
    mean_bhc: float
    mean_ahc: float
    median_equiv_bhc: float
    median_equiv_ahc: float


class PovertyHeadcounts(BaseModel):
    relative_bhc_children: float
    relative_bhc_working_age: float
    relative_bhc_pensioners: float
    relative_ahc_children: float
    relative_ahc_working_age: float
    relative_ahc_pensioners: float
    absolute_bhc_children: float
    absolute_bhc_working_age: float
    absolute_bhc_pensioners: float
    absolute_ahc_children: float
    absolute_ahc_working_age: float
    absolute_ahc_pensioners: float


class SimulationResult(BaseModel):
    fiscal_year: str
    budgetary_impact: BudgetaryImpact
    income_breakdown: IncomeBreakdown
    program_breakdown: ProgramBreakdown
    caseloads: Caseloads
    decile_impacts: list[DecileImpact]
    winners_losers: WinnersLosers
    baseline_hbai_incomes: HbaiIncomes
    reform_hbai_incomes: HbaiIncomes
    baseline_poverty: PovertyHeadcounts
    reform_poverty: PovertyHeadcounts
    cpi_index: float


class MicrodataResult(BaseModel):
    """Per-entity simulation results as DataFrames."""

    model_config = {"arbitrary_types_allowed": True}

    persons: Any  # pd.DataFrame
    benunits: Any  # pd.DataFrame
    households: Any  # pd.DataFrame
