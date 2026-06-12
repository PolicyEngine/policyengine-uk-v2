use serde::{Deserialize, Serialize};
use std::path::Path;

mod tree;
#[allow(unused_imports)]
pub use tree::ParameterTree;

/// UK tax-benefit system parameters for a given fiscal year.
///
/// All monetary values are annual unless noted otherwise.
/// UC amounts are monthly (per assessment period) as in legislation.
/// Child benefit and state pension are weekly as in legislation.
///
/// Sources: UK legislation via Lex API, OBR March 2026 EFO for growth factors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameters {
    pub fiscal_year: String,
    pub income_tax: IncomeTaxParams,
    pub national_insurance: NationalInsuranceParams,
    pub universal_credit: UniversalCreditParams,
    pub child_benefit: ChildBenefitParams,
    pub state_pension: StatePensionParams,
    pub pension_credit: PensionCreditParams,
    #[serde(default)]
    pub benefit_cap: Option<BenefitCapParams>,
    #[serde(default)]
    pub housing_benefit: Option<HousingBenefitParams>,
    #[serde(default)]
    pub tax_credits: Option<TaxCreditsParams>,
    #[serde(default)]
    pub scottish_child_payment: Option<ScottishChildPaymentParams>,
    /// Disability premiums for IS/HB/ESA applicable amounts.
    /// Source: Income Support (General) Regs 1987 (SI 1987/1967) Sch.2
    #[serde(default)]
    pub disability_premiums: Option<DisabilityPremiumParams>,
    /// Income-related benefits: ESA(IR), JSA(IB), Carers Allowance.
    #[serde(default)]
    pub income_related_benefits: Option<IncomeRelatedBenefitParams>,
    /// VAT parameters. Standard rate 20%, reduced rate 5% (energy), zero rate 0% (food).
    #[serde(default)]
    pub vat: Option<VatParams>,
    /// Fuel duty on petrol and diesel. HODA 1979 s.6; 52.95p/litre since 2022.
    #[serde(default)]
    pub fuel_duty: Option<FuelDutyParams>,
    /// Alcohol duty (effective rate on household alcohol spending).
    #[serde(default)]
    pub alcohol_duty: Option<AlcoholDutyParams>,
    /// Tobacco duty (effective rate on household tobacco spending).
    #[serde(default)]
    pub tobacco_duty: Option<TobaccoDutyParams>,
    /// Council tax (calculated). Allows reform modelling via band_d rate override.
    #[serde(default)]
    pub council_tax: Option<CouncilTaxParams>,
    /// Capital gains tax. TCGA 1992; 18%/24% from October 2024 Budget.
    #[serde(default)]
    pub capital_gains_tax: Option<CapitalGainsTaxParams>,
    /// Stamp duty land tax on residential property (England + NI). FA 2003 s.55.
    #[serde(default)]
    pub stamp_duty: Option<StampDutyParams>,
    /// Land and Buildings Transaction Tax — Scotland's devolved replacement for
    /// SDLT. Land and Buildings Transaction Tax (Scotland) Act 2013, s.24.
    #[serde(default)]
    pub lbtt: Option<StampDutyParams>,
    /// Land Transaction Tax — Wales's devolved replacement for SDLT.
    /// Land Transaction Tax and Anti-avoidance of Devolved Taxes (Wales) Act 2017.
    #[serde(default)]
    pub ltt: Option<StampDutyParams>,
    /// Annual wealth tax (hypothetical — disabled by default).
    #[serde(default)]
    pub wealth_tax: Option<WealthTaxParams>,
    /// Local Housing Allowance cap parameters.
    /// When present, caps eligible rent for private renters at the regional LHA rate
    /// for their bedroom entitlement category. Authority: HB Regs 2006 reg.13D.
    #[serde(default)]
    pub lha: Option<LhaParams>,
    /// Disability Living Allowance weekly rates (under-16 successor: now PIP/ADP).
    /// SSCBA 1992 Sch.2 paras 2–3.
    #[serde(default)]
    pub dla: Option<DlaParams>,
    /// Attendance Allowance weekly rates (over-SP-age disability benefit).
    /// SSCBA 1992 s.64.
    #[serde(default)]
    pub aa: Option<AaParams>,
    /// Personal Independence Payment weekly rates. Welfare Reform Act 2012 s.79.
    #[serde(default)]
    pub pip: Option<PipParams>,
    /// OBR labour supply response elasticities.
    /// When enabled, the Slutsky-decomposition elasticities from OBR (2023) are applied
    /// to estimate intensive-margin labour supply responses to tax-benefit reforms.
    /// Defaults to enabled (elasticities on).
    #[serde(default = "LabourSupplyParams::default")]
    pub labour_supply: LabourSupplyParams,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxBracket {
    pub rate: f64,
    pub threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomeTaxParams {
    pub personal_allowance: f64,
    pub pa_taper_threshold: f64,
    pub pa_taper_rate: f64,
    pub uk_brackets: Vec<TaxBracket>,
    pub scottish_brackets: Vec<TaxBracket>,
    pub dividend_allowance: f64,
    pub dividend_basic_rate: f64,
    pub dividend_higher_rate: f64,
    pub dividend_additional_rate: f64,
    pub savings_starter_rate_band: f64,
    /// Marriage Allowance: fraction of PA transferable (default 10%)
    #[serde(default = "default_ma_fraction")]
    pub marriage_allowance_max_fraction: f64,
    /// Rounding increment for marriage allowance (default £10)
    #[serde(default = "default_ma_rounding")]
    pub marriage_allowance_rounding: f64,
}

fn default_ma_fraction() -> f64 { 0.10 }
fn default_ma_rounding() -> f64 { 10.0 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NationalInsuranceParams {
    // Class 1 employee (primary)
    pub primary_threshold_annual: f64,
    pub upper_earnings_limit_annual: f64,
    pub main_rate: f64,
    pub additional_rate: f64,
    // Class 1 employer (secondary)
    #[serde(default = "default_secondary_threshold")]
    pub secondary_threshold_annual: f64,
    #[serde(default = "default_employer_rate")]
    pub employer_rate: f64,
    // Class 2 (self-employed flat rate)
    #[serde(default = "default_class2_flat_rate")]
    pub class2_flat_rate_weekly: f64,
    #[serde(default = "default_class2_spt")]
    pub class2_small_profits_threshold: f64,
    // Class 4 (self-employed)
    pub class4_lower_profits_limit: f64,
    pub class4_upper_profits_limit: f64,
    pub class4_main_rate: f64,
    pub class4_additional_rate: f64,
}

fn default_secondary_threshold() -> f64 { 5000.0 }
fn default_employer_rate() -> f64 { 0.15 }
// Class 2 abolished from 6 April 2024 (NIC Act 2024); default to 0 for post-2024 years
fn default_class2_flat_rate() -> f64 { 0.0 }
fn default_class2_spt() -> f64 { 0.0 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniversalCreditParams {
    /// Monthly standard allowance amounts (per assessment period)
    pub standard_allowance_single_under25: f64,
    pub standard_allowance_single_over25: f64,
    pub standard_allowance_couple_under25: f64,
    pub standard_allowance_couple_over25: f64,
    /// Monthly child element amounts
    pub child_element_first: f64,
    pub child_element_subsequent: f64,
    pub disabled_child_lower: f64,
    pub disabled_child_higher: f64,
    /// LCWRA and carer elements (monthly)
    pub lcwra_element: f64,
    pub carer_element: f64,
    /// Taper rate and work allowances (monthly)
    pub taper_rate: f64,
    pub work_allowance_higher: f64,
    pub work_allowance_lower: f64,
    pub child_limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildBenefitParams {
    /// Weekly rates
    pub eldest_weekly: f64,
    pub additional_weekly: f64,
    pub hicbc_threshold: f64,
    pub hicbc_taper_end: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatePensionParams {
    /// Weekly rates
    pub new_state_pension_weekly: f64,
    pub old_basic_pension_weekly: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PensionCreditParams {
    /// Weekly rates
    pub standard_minimum_single: f64,
    pub standard_minimum_couple: f64,
    pub savings_credit_threshold_single: f64,
    pub savings_credit_threshold_couple: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenefitCapParams {
    pub single_london: f64,
    pub single_outside_london: f64,
    pub non_single_london: f64,
    pub non_single_outside_london: f64,
    /// Net earned income threshold for exemption (annual)
    pub earnings_exemption_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HousingBenefitParams {
    /// Taper/withdrawal rate (65%)
    pub withdrawal_rate: f64,
    /// Personal allowances for applicable amount (weekly)
    pub personal_allowance_single_under25: f64,
    pub personal_allowance_single_25_plus: f64,
    pub personal_allowance_couple: f64,
    pub child_allowance: f64,
    pub family_premium: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxCreditsParams {
    // WTC elements (annual)
    pub wtc_basic_element: f64,
    pub wtc_couple_element: f64,
    pub wtc_lone_parent_element: f64,
    pub wtc_30_hour_element: f64,
    // CTC elements (annual)
    pub ctc_child_element: f64,
    pub ctc_family_element: f64,
    pub ctc_disabled_child_element: f64,
    pub ctc_severely_disabled_child_element: f64,
    // Income thresholds and taper
    pub income_threshold: f64,
    pub taper_rate: f64,
    /// Minimum hours per week to qualify for WTC
    pub wtc_min_hours_single: f64,
    pub wtc_min_hours_couple: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScottishChildPaymentParams {
    /// Weekly amount per eligible child
    pub weekly_amount: f64,
    /// Maximum age of child
    pub max_age: f64,
}

/// Disability premiums added to IS/HB/ESA applicable amounts.
///
/// Source: Income Support (General) Regs 1987 (SI 1987/1967) Sch.2 paras 11-14,
/// as uprated annually by the Social Security Benefits Up-rating Orders.
/// All amounts are weekly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisabilityPremiumParams {
    /// Disability Premium (DP): person has lower-rate PIP/DLA or is in WRAG.
    /// Sch.2 para.11: £42.50/wk (single), £60.60/wk (couple) in 2025/26.
    pub disability_premium_single: f64,
    pub disability_premium_couple: f64,
    /// Enhanced Disability Premium (EDP): highest-rate DLA care or enhanced PIP DL.
    /// Sch.2 para.13: £27.90/wk (single), £39.85/wk (couple) in 2025/26.
    pub enhanced_disability_premium_single: f64,
    pub enhanced_disability_premium_couple: f64,
    /// Severe Disability Premium (SDP): enhanced PIP DL/highest DLA care, lives alone,
    /// no carer receiving CA. Sch.2 para.14: £81.50/wk in 2025/26.
    pub severe_disability_premium: f64,
    /// Carer Premium: for persons receiving CA. Sch.2 para.14D: £46.00/wk in 2025/26.
    pub carer_premium: f64,
}

/// Income-related benefits: ESA(IR), JSA(IB), Carers Allowance.
///
/// ESA(IR) is structurally Income Support + a work-related component.
/// JSA(IB) is structurally Income Support conditioned on availability for work.
///
/// Sources:
///   - Welfare Reform Act 2007 c.5 s.2-4 (ESA)
///   - Employment and Support Allowance Regs 2008 (SI 2008/794)
///   - Jobseekers Act 1995 c.18 s.4-5 (JSA)
///   - Social Security Contributions and Benefits Act 1992 s.70 (CA)
///   - SS (Carers Allowance) Regs 2002 (SI 2002/2690)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomeRelatedBenefitParams {
    /// ESA personal allowances (weekly) — same as IS personal allowances.
    /// ESA Regs 2008 reg.67 / Sch.4.
    pub esa_allowance_single_under25: f64,
    pub esa_allowance_single_25_plus: f64,
    pub esa_allowance_couple: f64,
    /// ESA work-related activity component (WRAG): SI 2008/794 reg.67 Sch.4 col.2.
    /// £35.95/wk in 2025/26.
    pub esa_wrag_component: f64,
    /// ESA support component (support group): SI 2008/794 reg.67 Sch.4 col.2.
    /// £48.95/wk in 2025/26.
    pub esa_support_component: f64,
    /// JSA(IB) personal allowances (weekly) — same as IS.
    /// Jobseeker's Allowance Regs 1996 (SI 1996/207) Sch.1.
    pub jsa_allowance_single_under25: f64,
    pub jsa_allowance_single_25_plus: f64,
    pub jsa_allowance_couple: f64,
    /// Carers Allowance: weekly flat rate.
    /// SSCBA 1992 s.70(1); SS (CA) Regs 2002 reg.4.
    /// £81.90/wk in 2025/26.
    pub carers_allowance_weekly: f64,
    /// CA earnings disregard: SS (CA) Regs 2002 reg.8.
    /// £151.00/wk net earnings after deductions in 2025/26.
    pub ca_earnings_disregard_weekly: f64,
    /// Minimum hours of caring per week to qualify for CA.
    /// SSCBA 1992 s.70(1)(b): 35 hours/week.
    pub ca_min_hours_caring: f64,
    /// Minimum age of care recipient for CA: 16.
    pub ca_care_recipient_min_age: f64,
}

/// VAT parameters.
///
/// UK VAT (Value Added Tax Act 1994 c.23) applies to most goods and services.
/// Three rate bands: standard (20%), reduced (5% — domestic energy), zero (0% — food, children's clothing).
/// The VAT paid by a household is computed from COICOP consumption categories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VatParams {
    /// Standard rate (VATA 1994 s.2(1)): 20% since 4 Jan 2011.
    pub standard_rate: f64,
    /// Reduced rate (VATA 1994 s.29A, Sch.7A): 5% on domestic energy.
    pub reduced_rate: f64,
    /// Zero rate (VATA 1994 Sch.8): 0% on food, children's clothing, books.
    pub zero_rate: f64,
    /// Fraction of total consumption subject to standard rate (for non-EFRS estimation).
    /// ONS 2023: ~60% of household spending is standard-rated.
    #[serde(default = "default_standard_share")]
    pub standard_rated_share: f64,
    /// Fraction subject to reduced rate (domestic energy ~5% of spending).
    #[serde(default = "default_reduced_share")]
    pub reduced_rated_share: f64,
}

fn default_standard_share() -> f64 { 0.60 }
fn default_reduced_share() -> f64 { 0.05 }

/// Fuel duty parameters.
///
/// Hydrocarbon Oil Duties Act 1979 s.6; rates set by Finance Act orders.
/// Fuel duty is levied per litre of petrol/diesel. We convert from household £ spending
/// to litres using average pump prices, then apply duty rate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuelDutyParams {
    /// Duty rate on petrol (£ per litre). 52.95p/litre for 2025/26.
    pub petrol_rate_per_litre: f64,
    /// Duty rate on diesel (£ per litre). 52.95p/litre for 2025/26.
    pub diesel_rate_per_litre: f64,
    /// Average pump price for petrol (£ per litre, inc. duty and VAT).
    /// Used to convert £ spending to litres consumed.
    pub average_petrol_price_per_litre: f64,
    /// Average pump price for diesel (£ per litre, inc. duty and VAT).
    pub average_diesel_price_per_litre: f64,
}

/// Alcohol duty parameters (simplified effective rate).
///
/// Alcoholic Liquor Duties Act 1979; reformed August 2023 to ABV-based bands.
/// Since the LCFS gives us total £ alcohol spending (not quantities by ABV),
/// we use an effective duty rate: duty as a fraction of total tax-inclusive spending.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlcoholDutyParams {
    /// Effective alcohol duty rate (duty / tax-inclusive spending).
    /// OBR 2025/26: £11.9bn revenue from ~£30bn household alcohol spending ≈ 0.40.
    pub effective_rate: f64,
}

/// Tobacco duty parameters (simplified effective rate).
///
/// Tobacco Products Duty Act 1979; duty escalator RPI + 2%.
/// Effective rate: duty as a fraction of total tax-inclusive spending.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TobaccoDutyParams {
    /// Effective tobacco duty rate (duty / tax-inclusive spending).
    /// OBR 2025/26: £8bn revenue from ~£11bn tobacco spending ≈ 0.72.
    pub effective_rate: f64,
}

/// Council tax parameters (for reform modelling).
///
/// Local Government Finance Act 1992. Council tax is currently reported from the FRS.
/// These parameters allow modelling reforms (e.g. changing the Band D rate) while
/// keeping the baseline as the reported amount.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouncilTaxParams {
    /// Average Band D rate (£/year). England average £2,280 for 2025/26.
    pub average_band_d: f64,
    /// Band multipliers as fractions of Band D: A=6/9, B=7/9, ... H=18/9.
    #[serde(default = "default_band_multipliers")]
    pub band_multipliers: Vec<f64>,
    /// Property value thresholds for bands A–H (1991 values, England).
    #[serde(default = "default_band_thresholds")]
    pub band_thresholds: Vec<f64>,
    /// Single-person discount: fraction subtracted from council tax when only
    /// one adult (18+) is resident. 25% in England/Wales/Scotland — Local
    /// Government Finance Act 1992 s.11(1)(a).
    #[serde(default = "default_single_person_discount")]
    pub single_person_discount_rate: f64,
}

fn default_single_person_discount() -> f64 { 0.25 }

fn default_band_multipliers() -> Vec<f64> {
    vec![6.0/9.0, 7.0/9.0, 8.0/9.0, 1.0, 11.0/9.0, 13.0/9.0, 15.0/9.0, 18.0/9.0]
}

fn default_band_thresholds() -> Vec<f64> {
    vec![0.0, 40001.0, 52001.0, 68001.0, 88001.0, 120001.0, 160001.0, 320001.0]
}

/// Capital gains tax parameters.
///
/// Taxation of Chargeable Gains Act 1992. Rates raised to 18%/24% from October 2024.
/// AEA reduced to £3,000 from April 2024.
///
/// `residential_surcharge` is the additional rate (in points) applied to the
/// residential-property slice of taxable gains, on top of the basic/higher rate.
/// Pre-April-2024 the surcharge was 8 pp (yielding 28 % residential vs 20 %
/// non-residential at higher rate); from April 2025 rates unified so the surcharge
/// is zero. Reforms can set it to model a re-introduced residential surcharge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapitalGainsTaxParams {
    /// Annual exempt amount (£3,000 for 2025/26).
    pub annual_exempt_amount: f64,
    /// CGT rate for basic-rate taxpayers (18% from 2025/26).
    pub basic_rate: f64,
    /// CGT rate for higher/additional-rate taxpayers (24% from 2025/26).
    pub higher_rate: f64,
    /// Additional rate applied to residential-property gains (default 0.0 from
    /// April 2025; historically up to 0.08 — the 28 % rate in 2023/24).
    #[serde(default = "default_cgt_residential_surcharge")]
    pub residential_surcharge: f64,
}

fn default_cgt_residential_surcharge() -> f64 { 0.0 }

/// Stamp duty land tax bands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StampDutyBand {
    pub rate: f64,
    pub threshold: f64,
}

/// Stamp duty land tax parameters.
///
/// Finance Act 2003 s.55; rates revised 1 April 2025. SDLT is a marginal-rate tax
/// on residential property purchases. Annualised using average holding period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StampDutyParams {
    /// Marginal rate bands (sorted by threshold ascending).
    pub bands: Vec<StampDutyBand>,
    /// Annual purchase probability (1 / average holding period in years).
    /// Average UK holding period ~23 years → 0.043.
    #[serde(default = "default_purchase_probability")]
    pub annual_purchase_probability: f64,
}

fn default_purchase_probability() -> f64 { 0.043 }

/// Disability Living Allowance weekly component rates.
///
/// DLA has a care component (lowest, middle, highest) and a mobility component
/// (lower, higher). Working-age claimants migrated to PIP from 2013; remaining
/// DLA caseload is mostly children and pre-PIP-migration adults.
/// SSCBA 1992 Sch.2 para.2 (care) and para.3 (mobility).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DlaParams {
    pub care_low_weekly: f64,
    pub care_mid_weekly: f64,
    pub care_high_weekly: f64,
    pub mobility_low_weekly: f64,
    pub mobility_high_weekly: f64,
}

/// Attendance Allowance weekly rates.
///
/// Non-means-tested benefit for people over State Pension age who need help
/// with personal care. SSCBA 1992 s.64. Two rates: lower (day-only or night-only
/// care needed) and higher (both).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AaParams {
    pub low_weekly: f64,
    pub high_weekly: f64,
}

/// Personal Independence Payment weekly component rates.
///
/// PIP has two components — daily living and mobility — each at a standard or
/// enhanced rate. Welfare Reform Act 2012 s.79 / Social Security (Personal
/// Independence Payment) Regulations 2013 (SI 2013/377). Rates uprated annually.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipParams {
    pub daily_living_standard_weekly: f64,
    pub daily_living_enhanced_weekly: f64,
    pub mobility_standard_weekly: f64,
    pub mobility_enhanced_weekly: f64,
}

/// OBR labour supply response elasticities (Slutsky decomposition).
///
/// Source: OBR (2023) "Costing a cut in National Insurance contributions: the
/// impact on labour supply" <https://obr.uk/docs/dlm_uploads/NICS-Cut-Impact-on-Labour-Supply-Note.pdf>
/// Table A1 (substitution elasticities) and Table A2 (income elasticities).
///
/// The elasticities are segmented by gender, marital status, and youngest-child age.
/// The intensive-margin (hours) response to a policy change is:
///   ΔE = E_base × (η_s × Δw/w + η_i × Δy/y)
/// where η_s is the substitution elasticity, η_i the income elasticity,
/// Δw/w the relative change in the marginal net wage, and Δy/y the relative
/// change in net income.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabourSupplyParams {
    /// Whether to apply labour supply responses. Default true.
    #[serde(default = "default_true")]
    pub enabled: bool,

    // ── Substitution elasticities ────────────────────────────────────────────
    /// Married/cohabiting women, no children.
    #[serde(default = "default_0_14")]
    pub subst_married_women_no_children: f64,
    /// Married/cohabiting women, youngest child 0–2.
    #[serde(default = "default_0_301")]
    pub subst_married_women_child_0_2: f64,
    /// Married/cohabiting women, youngest child 3–4.
    #[serde(default = "default_0_439")]
    pub subst_married_women_child_3_4: f64,
    /// Married/cohabiting women, youngest child 5–10.
    #[serde(default = "default_0_173")]
    pub subst_married_women_child_5_10: f64,
    /// Married/cohabiting women, youngest child 11+.
    #[serde(default = "default_0_160")]
    pub subst_married_women_child_11_plus: f64,
    /// Lone parents, youngest child 0–4.
    #[serde(default = "default_0_094")]
    pub subst_lone_parents_child_0_4: f64,
    /// Lone parents, youngest child 5–10.
    #[serde(default = "default_0_128")]
    pub subst_lone_parents_child_5_10: f64,
    /// Lone parents, youngest child 11–18.
    #[serde(default = "default_0_136")]
    pub subst_lone_parents_child_11_18: f64,
    /// Men (excl. lone fathers) and single women without children.
    #[serde(default = "default_0_15")]
    pub subst_men_and_single_women: f64,

    // ── Income elasticities ──────────────────────────────────────────────────
    /// Married/cohabiting women, no children.
    #[serde(default = "default_0_0")]
    pub income_married_women_no_children: f64,
    /// Married/cohabiting women, youngest child 0–2.
    #[serde(default = "default_neg_0_185")]
    pub income_married_women_child_0_2: f64,
    /// Married/cohabiting women, youngest child 3–4.
    #[serde(default = "default_neg_0_173")]
    pub income_married_women_child_3_4: f64,
    /// Married/cohabiting women, youngest child 5–10.
    #[serde(default = "default_neg_0_102")]
    pub income_married_women_child_5_10: f64,
    /// Married/cohabiting women, youngest child 11+.
    #[serde(default = "default_neg_0_063")]
    pub income_married_women_child_11_plus: f64,
    /// Lone parents, youngest child 0–4.
    #[serde(default = "default_neg_0_037")]
    pub income_lone_parents_child_0_4: f64,
    /// Lone parents, youngest child 5–10.
    #[serde(default = "default_neg_0_075")]
    pub income_lone_parents_child_5_10: f64,
    /// Lone parents, youngest child 11–18.
    #[serde(default = "default_neg_0_054")]
    pub income_lone_parents_child_11_18: f64,
    /// Men (excl. lone fathers) and single women without children.
    #[serde(default = "default_neg_0_05")]
    pub income_men_and_single_women: f64,
}

fn default_true() -> bool { true }
fn default_0_14() -> f64 { 0.14 }
fn default_0_301() -> f64 { 0.301 }
fn default_0_439() -> f64 { 0.439 }
fn default_0_173() -> f64 { 0.173 }
fn default_0_160() -> f64 { 0.160 }
fn default_0_094() -> f64 { 0.094 }
fn default_0_128() -> f64 { 0.128 }
fn default_0_136() -> f64 { 0.136 }
fn default_0_15() -> f64 { 0.15 }
fn default_0_0() -> f64 { 0.0 }
fn default_neg_0_185() -> f64 { -0.185 }
fn default_neg_0_173() -> f64 { -0.173 }
fn default_neg_0_102() -> f64 { -0.102 }
fn default_neg_0_063() -> f64 { -0.063 }
fn default_neg_0_037() -> f64 { -0.037 }
fn default_neg_0_075() -> f64 { -0.075 }
fn default_neg_0_054() -> f64 { -0.054 }
fn default_neg_0_05() -> f64 { -0.05 }

impl Default for LabourSupplyParams {
    fn default() -> Self {
        LabourSupplyParams {
            enabled: true,
            subst_married_women_no_children: 0.14,
            subst_married_women_child_0_2: 0.301,
            subst_married_women_child_3_4: 0.439,
            subst_married_women_child_5_10: 0.173,
            subst_married_women_child_11_plus: 0.160,
            subst_lone_parents_child_0_4: 0.094,
            subst_lone_parents_child_5_10: 0.128,
            subst_lone_parents_child_11_18: 0.136,
            subst_men_and_single_women: 0.15,
            income_married_women_no_children: 0.0,
            income_married_women_child_0_2: -0.185,
            income_married_women_child_3_4: -0.173,
            income_married_women_child_5_10: -0.102,
            income_married_women_child_11_plus: -0.063,
            income_lone_parents_child_0_4: -0.037,
            income_lone_parents_child_5_10: -0.075,
            income_lone_parents_child_11_18: -0.054,
            income_men_and_single_women: -0.05,
        }
    }
}

/// Local Housing Allowance (LHA) parameters.
///
/// LHA caps the eligible rent for private renters on HB/UC at the 30th percentile of
/// local rents in each Broad Rental Market Area (BRMA), by bedroom entitlement category
/// (A = shared, B = 1-bed, C = 2-bed, D = 3-bed, E = 4+-bed).
///
/// Since the FRS suppresses BRMA identifiers for disclosure control, we use region-level
/// 30th percentile rates derived from the VOA rent data (same underlying source as BRMA
/// rates, aggregated to GOR). This understates within-region variation but captures the
/// main regional gradient.
///
/// Rate history:
///   - Frozen: April 2020 – March 2024 (SI 2019/1303)
///   - Reset to 30th percentile: April 2024 (gov.uk announcement, 28 Nov 2023)
///   - Re-frozen: April 2025 (OBR EFO March 2025 assumption)
///
/// Uprating: `private_rent_index` uprates the baseline rates for reform scenarios;
/// e.g. setting `private_rent_index: 1.10` models a 10% LHA increase.
/// For the baseline (frozen), this field is 1.0.
///
/// Source: VOA list of rents (via policyengine-uk), uprated using ONS Index of Private
/// Housing Rental Prices (base: April 2020 = 100, April 2024 ≈ 114.7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LhaParams {
    /// Whether LHA cap is active. If false, eligible rent = actual rent (pre-reform default).
    #[serde(default = "default_true_lha")]
    pub enabled: bool,
    /// Multiplier applied to all rates for reform scenarios (e.g. 1.1 = 10% increase).
    /// For baseline frozen scenario this is 1.0.
    #[serde(default = "default_one")]
    pub private_rent_index: f64,
    /// LHA rates by region (11 regions matching Region enum) then by bedroom category
    /// (index 0=shared/A, 1=1-bed/B, 2=2-bed/C, 3=3-bed/D, 4=4+bed/E).
    /// Values are monthly amounts (£).
    /// Order: NorthEast, NorthWest, Yorkshire, EastMidlands, WestMidlands,
    ///        EastOfEngland, London, SouthEast, SouthWest, Wales, Scotland, NorthernIreland.
    pub rates_monthly: Vec<[f64; 5]>,
}

fn default_true_lha() -> bool { true }
fn default_one() -> f64 { 1.0 }

impl LhaParams {
    /// Return the monthly LHA cap (£) for a given region and bedroom entitlement.
    ///
    /// `region_idx` maps to Region::to_rf_code() (0=NE, 1=NW, 2=Yorks, 3=EM, 4=WM,
    /// 5=EofE, 6=London, 7=SE, 8=SW, 9=Wales/NI, 10=Scotland, 11=NI).
    /// `bedrooms` is the LHA bedroom entitlement (1–4+), or 0 for shared accommodation.
    /// Returns `None` if rates_monthly is empty or region_idx out of range.
    pub fn monthly_cap(&self, region_idx: usize, bedrooms: u32) -> Option<f64> {
        let row = self.rates_monthly.get(region_idx)?;
        let col = match bedrooms {
            0 => 0, // shared accommodation (Category A)
            1 => 1,
            2 => 2,
            3 => 3,
            _ => 4, // 4+ bedrooms → Category E
        };
        Some(row[col] * self.private_rent_index)
    }
}

/// Annual wealth tax parameters (hypothetical — disabled by default).
///
/// No current UK wealth tax exists. These parameters support modelling
/// proposals such as the Wealth Tax Commission's 1% above £10m.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WealthTaxParams {
    /// Whether the wealth tax is active. Default false.
    #[serde(default)]
    pub enabled: bool,
    /// Threshold above which wealth is taxed (£).
    pub threshold: f64,
    /// Tax rate on wealth above the threshold.
    pub rate: f64,
}

/// Convert a fiscal year start year (e.g. 2029) to the YAML filename format
fn fiscal_year_filename(year: u32) -> String {
    format!("{}_{:02}.yaml", year, (year + 1) % 100)
}

impl Parameters {
    /// Load parameters for a given fiscal year from the embedded YAML files.
    /// `year` is the start year of the fiscal year, e.g. 2029 for FY 2029/30.
    pub fn for_year(year: u32) -> anyhow::Result<Self> {
        let filename = fiscal_year_filename(year);

        // Try loading from the parameters/ directory relative to the executable,
        // then from cargo manifest dir (for development)
        let paths_to_try = vec![
            format!("parameters/{}", filename),
            format!("{}/parameters/{}", env!("CARGO_MANIFEST_DIR"), filename),
        ];

        for path_str in &paths_to_try {
            let path = Path::new(path_str);
            if path.exists() {
                let contents = std::fs::read_to_string(path)?;
                let params: Parameters = serde_yaml::from_str(&contents)?;
                return Ok(params);
            }
        }

        anyhow::bail!(
            "No parameter file found for fiscal year {}/{}. Looked for: {}",
            year, year + 1, paths_to_try.join(", ")
        )
    }

    /// Load parameters from a YAML string.
    #[allow(dead_code)]
    pub fn from_yaml(yaml_str: &str) -> anyhow::Result<Self> {
        let params: Parameters = serde_yaml::from_str(yaml_str)?;
        Ok(params)
    }

    /// Serialise parameters to YAML for human-readable reform files.
    pub fn to_yaml(&self) -> String {
        serde_yaml::to_string(self).unwrap_or_default()
    }

    /// Serialise parameters to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Apply a JSON overlay (reform) on top of these parameters.
    pub fn apply_json_overlay(&self, json_str: &str) -> anyhow::Result<Self> {
        let json_val: serde_json::Value = serde_json::from_str(json_str)?;
        let yaml_str = serde_yaml::to_string(&json_val)?;
        self.apply_yaml_overlay(&yaml_str)
    }

    /// Apply a YAML overlay (reform) on top of these parameters.
    /// Only the fields specified in the overlay are changed.
    pub fn apply_yaml_overlay(&self, overlay_yaml: &str) -> anyhow::Result<Self> {
        let base_value = serde_yaml::to_value(self)?;
        let overlay_value: serde_yaml::Value = serde_yaml::from_str(overlay_yaml)?;
        let merged = merge_yaml(base_value, &overlay_value);
        let merged_params: Parameters = serde_yaml::from_value(merged)?;
        Ok(merged_params)
    }

    /// Available fiscal years (hardcoded list of embedded parameter files).
    #[allow(dead_code)]
    pub fn available_years() -> Vec<u32> {
        // 1994/95 through 2029/30
        (1994..=2029).collect()
    }
}

/// Deep-merge two YAML values. `overlay` wins on conflict.
fn merge_yaml(mut base: serde_yaml::Value, overlay: &serde_yaml::Value) -> serde_yaml::Value {
    match (&mut base, overlay) {
        (serde_yaml::Value::Mapping(base_map), serde_yaml::Value::Mapping(overlay_map)) => {
            for (key, overlay_val) in overlay_map {
                if let Some(base_val) = base_map.get(key).cloned() {
                    let merged = merge_yaml(base_val, overlay_val);
                    base_map.insert(key.clone(), merged);
                } else {
                    base_map.insert(key.clone(), overlay_val.clone());
                }
            }
            base
        }
        (_, overlay) => overlay.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_2025_26() {
        let params = Parameters::for_year(2025).unwrap();
        assert_eq!(params.fiscal_year, "2025/26");
        assert!((params.income_tax.personal_allowance - 12570.0).abs() < 0.01);
        assert!((params.national_insurance.main_rate - 0.08).abs() < 0.001);
    }

    #[test]
    fn test_load_2029_30() {
        let params = Parameters::for_year(2029).unwrap();
        assert_eq!(params.fiscal_year, "2029/30");
        assert!(params.income_tax.personal_allowance > 12570.0); // Should be uprated
    }

    #[test]
    fn test_load_all_years() {
        for year in 1994..=2029 {
            let params = Parameters::for_year(year);
            assert!(params.is_ok(), "Failed to load parameters for {}/{}", year, year + 1);
        }
    }

    #[test]
    fn test_yaml_overlay() {
        let base = Parameters::for_year(2025).unwrap();
        let overlay = "income_tax:\n  personal_allowance: 20000.0\n";
        let reformed = base.apply_yaml_overlay(overlay).unwrap();
        assert!((reformed.income_tax.personal_allowance - 20000.0).abs() < 0.01);
        // Other values should be unchanged
        assert!((reformed.national_insurance.main_rate - 0.08).abs() < 0.001);
    }
}
