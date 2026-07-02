/// Entity types in the UK tax-benefit system.
/// Person → BenUnit (benefit unit / family) → Household
///
/// A household contains one or more benefit units, each containing one or more persons.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gender {
    Male,
    Female,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Person {
    pub id: usize,
    pub benunit_id: usize,
    pub household_id: usize,
    pub age: f64,
    pub gender: Gender,
    pub is_benunit_head: bool,
    pub is_household_head: bool,

    // Income sources (annual)
    pub employment_income: f64,
    pub self_employment_income: f64,
    pub pension_income: f64,          // private pension income
    pub state_pension: f64,
    pub savings_interest_income: f64,
    pub dividend_income: f64,
    pub capital_gains: f64,
    /// Fraction of `capital_gains` that came from residential property disposals
    /// (TCGA 1992 s.4 — residential property gains). Multiplied with the residential
    /// surcharge in `CapitalGainsTaxParams` to apply the higher residential rate to
    /// the relevant slice of taxable gains. Default 0.0 — entire gain treated as
    /// non-residential (which from April 2025 is the same rate, since rates unified).
    pub capital_gains_residential_share: f64,
    pub property_income: f64,
    pub maintenance_income: f64,
    pub miscellaneous_income: f64,
    pub other_income: f64,

    // Employment
    pub is_in_scotland: bool,
    pub hours_worked: f64,             // annual hours

    // Disability/carer status — granular rate-band flags derived from FRS benefit amounts
    // DLA care component (SSCBA 1992 Sch.2 para.2 as amended)
    pub dla_care_low: bool,     // lowest rate
    pub dla_care_mid: bool,     // middle rate
    pub dla_care_high: bool,    // highest rate
    // DLA mobility component (SSCBA 1992 Sch.2 para.3)
    pub dla_mob_low: bool,
    pub dla_mob_high: bool,
    // PIP daily living component (WRA 2012 s.79 / PIP Regs 2013 SI 2013/377)
    pub pip_dl_std: bool,
    pub pip_dl_enh: bool,
    // PIP mobility component (WRA 2012 s.79)
    pub pip_mob_std: bool,
    pub pip_mob_enh: bool,
    // Attendance Allowance (SSCBA 1992 s.64)
    pub aa_low: bool,
    pub aa_high: bool,
    // Convenience aggregates (kept for backwards compat with UC/IS/HB logic)
    pub is_disabled: bool,          // any PIP/DLA/AA receipt
    pub is_enhanced_disabled: bool, // DLA care high OR PIP DL enhanced (disabled child higher rate)
    pub is_severely_disabled: bool, // PIP DL enhanced or DLA care high (SDP proxy)
    pub is_carer: bool,             // CA receipt
    // Employment/health status from FRS (for ESA/JSA eligibility)
    pub limitill: bool,     // LIMITILL: has limiting long-standing illness
    pub esa_group: i64,     // ESAGRP: 1=support, 2=WRAG, 3=assessment, 0=none/unknown
    pub emp_status: i64,    // EMPSTATB: 1=employed, 2=self-employed, 3=unemployed, 4=inactive
    pub looking_for_work: bool,     // LOOKWK: actively looking for work
    pub is_self_identified_carer: bool, // CARER1: identifies as unpaid carer

    // Pension contributions (annual)
    pub employee_pension_contributions: f64,
    pub personal_pension_contributions: f64,

    // Childcare (annual)
    pub childcare_expenses: f64,

    // Benefit amounts (annual) — from FRS microdata, used for take-up and passthrough
    pub child_benefit: f64,
    pub housing_benefit: f64,
    pub income_support: f64,
    pub pension_credit: f64,
    pub child_tax_credit: f64,
    pub working_tax_credit: f64,
    pub universal_credit: f64,
    pub dla_care: f64,
    pub dla_mobility: f64,
    pub pip_daily_living: f64,
    pub pip_mobility: f64,
    pub carers_allowance: f64,
    pub attendance_allowance: f64,
    pub esa_income: f64,
    pub esa_contributory: f64,
    pub jsa_income: f64,
    pub jsa_contributory: f64,
    /// Aggregate of unmodelled passthrough benefits (bereavement, maternity, winter fuel, etc.)
    pub other_benefits: f64,
    /// Scottish disability replacements (ADP replaces PIP for Scottish adults)
    pub adp_daily_living: f64,
    pub adp_mobility: f64,
    /// Scottish child disability (CDP replaces DLA for Scottish children)
    pub cdp_care: f64,
    pub cdp_mobility: f64,
}

impl Default for Person {
    fn default() -> Self {
        Person {
            id: 0, benunit_id: 0, household_id: 0,
            age: 30.0,
            gender: Gender::Male,
            is_benunit_head: false,
            is_household_head: false,
            employment_income: 0.0,
            self_employment_income: 0.0,
            pension_income: 0.0,
            state_pension: 0.0,
            savings_interest_income: 0.0,
            dividend_income: 0.0,
            capital_gains: 0.0,
            capital_gains_residential_share: 0.0,
            property_income: 0.0,
            maintenance_income: 0.0,
            miscellaneous_income: 0.0,
            other_income: 0.0,
            is_in_scotland: false,
            hours_worked: 0.0,
            dla_care_low: false,
            dla_care_mid: false,
            dla_care_high: false,
            dla_mob_low: false,
            dla_mob_high: false,
            pip_dl_std: false,
            pip_dl_enh: false,
            pip_mob_std: false,
            pip_mob_enh: false,
            aa_low: false,
            aa_high: false,
            is_disabled: false,
            is_enhanced_disabled: false,
            is_severely_disabled: false,
            is_carer: false,
            limitill: false,
            esa_group: 0,
            emp_status: 0,
            looking_for_work: false,
            is_self_identified_carer: false,
            employee_pension_contributions: 0.0,
            personal_pension_contributions: 0.0,
            childcare_expenses: 0.0,
            child_benefit: 0.0,
            housing_benefit: 0.0,
            income_support: 0.0,
            pension_credit: 0.0,
            child_tax_credit: 0.0,
            working_tax_credit: 0.0,
            universal_credit: 0.0,
            dla_care: 0.0,
            dla_mobility: 0.0,
            pip_daily_living: 0.0,
            pip_mobility: 0.0,
            carers_allowance: 0.0,
            attendance_allowance: 0.0,
            esa_income: 0.0,
            esa_contributory: 0.0,
            jsa_income: 0.0,
            jsa_contributory: 0.0,
            other_benefits: 0.0,
            adp_daily_living: 0.0,
            adp_mobility: 0.0,
            cdp_care: 0.0,
            cdp_mobility: 0.0,
        }
    }
}

impl Person {
    /// Total gross income from all sources (excluding reported benefits).
    pub fn total_income(&self) -> f64 {
        self.employment_income
            + self.self_employment_income
            + self.pension_income
            + self.state_pension
            + self.savings_interest_income
            + self.dividend_income
            + self.property_income
            + self.maintenance_income
            + self.miscellaneous_income
            + self.other_income
    }

    /// Earned income (employment + self-employment).
    #[allow(dead_code)]
    pub fn earned_income(&self) -> f64 {
        self.employment_income + self.self_employment_income
    }

    pub fn is_adult(&self) -> bool {
        self.age >= 18.0
    }

    pub fn is_child(&self) -> bool {
        self.age < 18.0
    }

    /// Whether person is over state pension age (simplified: 66 for all).
    pub fn is_sp_age(&self) -> bool {
        self.age >= 66.0
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct BenUnit {
    pub id: usize,
    pub household_id: usize,
    pub person_ids: Vec<usize>,
    /// Whether this benunit reported UC receipt in the FRS.
    pub on_uc: bool,
    pub rent_monthly: f64,
    pub is_lone_parent: bool,
    /// Claim every means-tested benefit (UC, HB, CTC, WTC, IS, ESA, JSA) the
    /// benunit is eligible for, rather than gating on reported receipt. Persisted
    /// data field, defaults true so hypothetical households take up benefits; the
    /// data pipeline sets it to `on_uc` for survey records so take-up matches the
    /// FRS-reported claim status.
    pub claims_uc_if_eligible: bool,

    // In-kind benefits (annual, from FRS DVs — included in HBAI net income)
    pub free_school_meals: f64,      // FSMBU
    pub free_school_fruit_veg: f64,  // FSFVBU
    pub free_school_milk: f64,       // FSMLKBU
    pub healthy_start_vouchers: f64, // HEARTBU
    pub free_tv_licence: f64,        // BUTVLIC
}

impl Default for BenUnit {
    fn default() -> Self {
        Self {
            id: 0,
            household_id: 0,
            person_ids: Vec::new(),
            on_uc: false,
            rent_monthly: 0.0,
            is_lone_parent: false,
            claims_uc_if_eligible: true,
            free_school_meals: 0.0,
            free_school_fruit_veg: 0.0,
            free_school_milk: 0.0,
            healthy_start_vouchers: 0.0,
            free_tv_licence: 0.0,
        }
    }
}

impl BenUnit {
    /// Whether the benunit claims a benefit: eligibility-based take-up, or any
    /// member reported receipt of it in the survey.
    pub fn claims(&self, people: &[Person], reported: impl Fn(&Person) -> f64) -> bool {
        self.claims_uc_if_eligible || self.person_ids.iter().any(|&pid| reported(&people[pid]) > 0.0)
    }

    pub fn num_adults(&self, people: &[Person]) -> usize {
        self.person_ids.iter()
            .filter(|&&pid| people[pid].is_adult())
            .count()
    }

    pub fn num_children(&self, people: &[Person]) -> usize {
        self.person_ids.iter()
            .filter(|&&pid| people[pid].is_child())
            .count()
    }

    /// Children qualifying for per-child benefit elements under the two-child
    /// limit. Children born before 6 April 2017 are exempt (transitional
    /// protection), so the limit's caseload grows each year as pre-2017
    /// children age out. Birth year is approximated as `year - floor(age)`.
    pub fn num_qualifying_children(&self, people: &[Person], limit: usize, year: u32) -> usize {
        let (mut pre, mut post) = (0usize, 0usize);
        for &pid in &self.person_ids {
            let p = &people[pid];
            if !p.is_child() {
                continue;
            }
            if year.saturating_sub(p.age as u32) < 2017 {
                pre += 1;
            } else {
                post += 1;
            }
        }
        pre + post.min(limit.saturating_sub(pre))
    }

    pub fn is_couple(&self, people: &[Person]) -> bool {
        self.num_adults(people) >= 2
    }

    pub fn eldest_adult_age(&self, people: &[Person]) -> f64 {
        self.person_ids.iter()
            .filter(|&&pid| people[pid].is_adult())
            .map(|&pid| people[pid].age)
            .fold(0.0_f64, f64::max)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Household {
    pub id: usize,
    pub benunit_ids: Vec<usize>,
    pub person_ids: Vec<usize>,
    pub weight: f64,
    pub region: Region,
    pub rent: f64,
    /// Mortgage interest paid (annual) — an AHC housing cost, not capital repayment
    pub mortgage_interest: f64,
    pub council_tax: f64,

    // Auxiliary (FRS housing variables, used as RF predictors)
    pub num_bedrooms: u32,
    pub tenure_type: TenureType,
    pub accommodation_type: AccommodationType,
    pub council_tax_band: u8,  // FRS CTBAND 1–8 (A–H), 0 = unknown/N/A

    // Wealth (from WAS imputation)
    pub owned_land: f64,
    pub property_wealth: f64,
    pub corporate_wealth: f64,
    pub gross_financial_wealth: f64,
    pub net_financial_wealth: f64,
    pub main_residence_value: f64,
    pub other_residential_property_value: f64,
    pub non_residential_property_value: f64,
    pub savings: f64,
    pub num_vehicles: f64,

    // Consumption (from LCFS imputation, annual)
    pub food_consumption: f64,
    pub alcohol_consumption: f64,
    pub tobacco_consumption: f64,
    pub clothing_consumption: f64,
    pub housing_water_electricity_consumption: f64,
    pub furnishings_consumption: f64,
    pub health_consumption: f64,
    pub transport_consumption: f64,
    pub communication_consumption: f64,
    pub recreation_consumption: f64,
    pub education_consumption: f64,
    pub restaurants_consumption: f64,
    pub miscellaneous_consumption: f64,
    pub petrol_spending: f64,
    pub diesel_spending: f64,
    pub domestic_energy_consumption: f64,
    pub electricity_consumption: f64,
    pub gas_consumption: f64,
}

impl Default for Household {
    fn default() -> Self {
        Household {
            id: 0,
            benunit_ids: Vec::new(),
            person_ids: Vec::new(),
            weight: 0.0,
            region: Region::London,
            rent: 0.0,
            mortgage_interest: 0.0,
            council_tax: 0.0,
            num_bedrooms: 0,
            tenure_type: TenureType::default(),
            accommodation_type: AccommodationType::default(),
            council_tax_band: 0,
            owned_land: 0.0,
            property_wealth: 0.0,
            corporate_wealth: 0.0,
            gross_financial_wealth: 0.0,
            net_financial_wealth: 0.0,
            main_residence_value: 0.0,
            other_residential_property_value: 0.0,
            non_residential_property_value: 0.0,
            savings: 0.0,
            num_vehicles: 0.0,
            food_consumption: 0.0,
            alcohol_consumption: 0.0,
            tobacco_consumption: 0.0,
            clothing_consumption: 0.0,
            housing_water_electricity_consumption: 0.0,
            furnishings_consumption: 0.0,
            health_consumption: 0.0,
            transport_consumption: 0.0,
            communication_consumption: 0.0,
            recreation_consumption: 0.0,
            education_consumption: 0.0,
            restaurants_consumption: 0.0,
            miscellaneous_consumption: 0.0,
            petrol_spending: 0.0,
            diesel_spending: 0.0,
            domestic_energy_consumption: 0.0,
            electricity_consumption: 0.0,
            gas_consumption: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TenureType {
    OwnedOutright,
    OwnedWithMortgage,
    RentFromCouncil,
    RentFromHA,
    RentPrivately,
    Other,
}

impl Default for TenureType {
    fn default() -> Self { TenureType::Other }
}

impl TenureType {
    /// Map FRS TENTYP2 codes to enum.
    pub fn from_frs_code(code: i32) -> Self {
        match code {
            1 => TenureType::RentFromCouncil,
            2 => TenureType::RentFromHA,
            3 => TenureType::RentPrivately,
            4 => TenureType::RentPrivately,  // rent-free treated as private
            5 => TenureType::OwnedWithMortgage,
            6 => TenureType::OwnedWithMortgage,  // shared ownership
            7 => TenureType::OwnedOutright,
            _ => TenureType::Other,
        }
    }

    /// Integer code for RF feature encoding and clean CSV serialisation.
    pub fn to_rf_code(&self) -> f64 {
        match self {
            TenureType::OwnedOutright => 0.0,
            TenureType::OwnedWithMortgage => 1.0,
            TenureType::RentFromCouncil => 2.0,
            TenureType::RentFromHA => 3.0,
            TenureType::RentPrivately => 4.0,
            TenureType::Other => 5.0,
        }
    }

    /// Deserialise from the clean CSV rf_code (inverse of to_rf_code).
    pub fn from_rf_code(code: i32) -> Self {
        match code {
            0 => TenureType::OwnedOutright,
            1 => TenureType::OwnedWithMortgage,
            2 => TenureType::RentFromCouncil,
            3 => TenureType::RentFromHA,
            4 => TenureType::RentPrivately,
            _ => TenureType::Other,
        }
    }

    pub fn is_renting(&self) -> bool {
        matches!(self, TenureType::RentFromCouncil | TenureType::RentFromHA | TenureType::RentPrivately)
    }

    /// NEED calibration category (3 groups).
    pub fn need_category(&self) -> usize {
        match self {
            TenureType::OwnedOutright | TenureType::OwnedWithMortgage => 0, // owner
            TenureType::RentPrivately => 1,                                  // private rent
            TenureType::RentFromCouncil | TenureType::RentFromHA => 2,       // social rent
            TenureType::Other => 0,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccommodationType {
    HouseDetached,
    HouseSemiDetached,
    HouseTerraced,
    Flat,
    Mobile,
    Other,
}

impl Default for AccommodationType {
    fn default() -> Self { AccommodationType::Other }
}

#[allow(dead_code)]
impl AccommodationType {
    /// Map FRS TYPEACC codes to enum.
    pub fn from_frs_code(code: i32) -> Self {
        match code {
            1 => AccommodationType::HouseDetached,
            2 => AccommodationType::HouseSemiDetached,
            3 => AccommodationType::HouseTerraced,
            4 | 5 => AccommodationType::Flat,   // purpose-built + converted
            6 => AccommodationType::Mobile,      // caravan/mobile home
            _ => AccommodationType::Other,
        }
    }

    /// Integer code for RF feature encoding.
    pub fn to_rf_code(&self) -> f64 {
        match self {
            AccommodationType::HouseDetached => 0.0,
            AccommodationType::HouseSemiDetached => 1.0,
            AccommodationType::HouseTerraced => 2.0,
            AccommodationType::Flat => 3.0,
            AccommodationType::Mobile => 4.0,
            AccommodationType::Other => 5.0,
        }
    }

    /// NEED calibration category (5 groups).
    pub fn need_category(&self) -> usize {
        match self {
            AccommodationType::HouseDetached => 0,
            AccommodationType::HouseSemiDetached => 1,
            AccommodationType::HouseTerraced => 2,
            AccommodationType::Flat => 3,
            AccommodationType::Mobile | AccommodationType::Other => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Region {
    #[default]
    NorthEast,
    NorthWest,
    Yorkshire,
    EastMidlands,
    WestMidlands,
    EastOfEngland,
    London,
    SouthEast,
    SouthWest,
    Wales,
    Scotland,
    NorthernIreland,
}

#[allow(dead_code)]
impl Region {
    pub fn is_scotland(&self) -> bool {
        matches!(self, Region::Scotland)
    }

    pub fn from_frs_code(code: i32) -> Self {
        match code {
            1 => Region::NorthEast,
            2 => Region::NorthWest,
            4 => Region::Yorkshire,
            5 => Region::EastMidlands,
            6 => Region::WestMidlands,
            7 => Region::EastOfEngland,
            8 => Region::London,
            9 => Region::SouthEast,
            10 => Region::SouthWest,
            11 => Region::Wales,
            12 => Region::Scotland,
            13 => Region::NorthernIreland,
            _ => Region::London,
        }
    }

    /// Integer code for RF feature encoding.
    pub fn to_rf_code(&self) -> f64 {
        match self {
            Region::NorthEast => 0.0,
            Region::NorthWest => 1.0,
            Region::Yorkshire => 2.0,
            Region::EastMidlands => 3.0,
            Region::WestMidlands => 4.0,
            Region::EastOfEngland => 5.0,
            Region::London => 6.0,
            Region::SouthEast => 7.0,
            Region::SouthWest => 8.0,
            Region::Wales => 9.0,
            Region::Scotland => 10.0,
            Region::NorthernIreland => 11.0,
        }
    }

    /// NEED calibration region index (0-10, NI mapped to Wales).
    pub fn need_region(&self) -> usize {
        match self {
            Region::NorthEast => 0,
            Region::NorthWest => 1,
            Region::Yorkshire => 2,
            Region::EastMidlands => 3,
            Region::WestMidlands => 4,
            Region::EastOfEngland => 5,
            Region::London => 6,
            Region::SouthEast => 7,
            Region::SouthWest => 8,
            Region::Wales | Region::NorthernIreland => 9,
            Region::Scotland => 10,
        }
    }

    /// LHA region index for rate table lookup (0–11, matching rates_monthly row order).
    /// Order: NE=0, NW=1, Yorks=2, EM=3, WM=4, EofE=5, London=6, SE=7, SW=8,
    ///        Wales=9, Scotland=10, NI=11.
    pub fn to_lha_region_idx(&self) -> usize {
        match self {
            Region::NorthEast => 0,
            Region::NorthWest => 1,
            Region::Yorkshire => 2,
            Region::EastMidlands => 3,
            Region::WestMidlands => 4,
            Region::EastOfEngland => 5,
            Region::London => 6,
            Region::SouthEast => 7,
            Region::SouthWest => 8,
            Region::Wales => 9,
            Region::Scotland => 10,
            Region::NorthernIreland => 11,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Region::NorthEast => "North East",
            Region::NorthWest => "North West",
            Region::Yorkshire => "Yorkshire",
            Region::EastMidlands => "East Midlands",
            Region::WestMidlands => "West Midlands",
            Region::EastOfEngland => "East of England",
            Region::London => "London",
            Region::SouthEast => "South East",
            Region::SouthWest => "South West",
            Region::Wales => "Wales",
            Region::Scotland => "Scotland",
            Region::NorthernIreland => "Northern Ireland",
        }
    }
}
