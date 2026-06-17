pub mod frs;
pub mod clean;
pub mod stdin;
pub mod spi;
pub mod lcfs;
pub mod was;

use crate::engine::entities::*;

/// A complete dataset ready for microsimulation
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Dataset {
    pub people: Vec<Person>,
    pub benunits: Vec<BenUnit>,
    pub households: Vec<Household>,
    pub name: String,
    pub year: u32,
}

#[allow(dead_code)]
impl Dataset {
    pub fn num_households(&self) -> usize {
        self.households.len()
    }

    pub fn weighted_population(&self) -> f64 {
        self.households.iter().map(|h| h.weight).sum()
    }

    /// Uprate all monetary amounts from the dataset's current year to `target_year`
    /// using variable-specific OBR growth indices matching policyengine-uk's uprating_indices.yaml.
    pub fn uprate_to(&mut self, target_year: u32) {
        if target_year == self.year {
            self.year = target_year;
            return;
        }

        let earnings   = cumulative_factor(self.year, target_year, UpratingIndex::AverageEarnings);
        let cpi        = cumulative_factor(self.year, target_year, UpratingIndex::CPI);
        let gdp_pc     = cumulative_factor(self.year, target_year, UpratingIndex::GDPPerCapita);
        let mixed_pc   = cumulative_factor(self.year, target_year, UpratingIndex::MixedIncomePerCapita);
        let rent       = cumulative_factor(self.year, target_year, UpratingIndex::Rent);
        let council_tax = cumulative_factor(self.year, target_year, UpratingIndex::CouncilTaxEngland);
        let population = cumulative_factor(self.year, target_year, UpratingIndex::Population);
        let interest   = cumulative_factor(self.year, target_year, UpratingIndex::HouseholdInterestIncome);


        for p in &mut self.people {
            // Earnings-uprated
            p.employment_income *= earnings;
            p.employee_pension_contributions *= earnings;
            p.personal_pension_contributions *= earnings;

            // Mixed income per capita
            p.self_employment_income *= mixed_pc;

            // GDP per capita (capital/investment income)
            p.pension_income *= gdp_pc;
            p.dividend_income *= gdp_pc;
            p.property_income *= gdp_pc;
            p.maintenance_income *= gdp_pc;
            p.miscellaneous_income *= gdp_pc;
            p.other_income *= gdp_pc;

            // Household interest income index
            p.savings_interest_income *= interest;

            // CPI-uprated (benefits)
            p.state_pension *= cpi;
            p.child_benefit *= cpi;
            p.housing_benefit *= cpi;
            p.income_support *= cpi;
            p.pension_credit *= cpi;
            p.child_tax_credit *= cpi;
            p.working_tax_credit *= cpi;
            p.universal_credit *= cpi;
            p.dla_care *= cpi;
            p.dla_mobility *= cpi;
            p.pip_daily_living *= cpi;
            p.pip_mobility *= cpi;
            p.carers_allowance *= cpi;
            p.attendance_allowance *= cpi;
            p.esa_income *= cpi;
            p.esa_contributory *= cpi;
            p.jsa_income *= cpi;
            p.jsa_contributory *= cpi;
            p.other_benefits *= cpi;
            p.adp_daily_living *= cpi;
            p.adp_mobility *= cpi;
            p.cdp_care *= cpi;
            p.cdp_mobility *= cpi;
            p.childcare_expenses *= cpi;
        }
        for h in &mut self.households {
            h.rent *= rent;
            h.council_tax *= council_tax;
            // Wealth (uprated by earnings as rough proxy)
            h.owned_land *= earnings;
            h.property_wealth *= earnings;
            h.corporate_wealth *= earnings;
            h.gross_financial_wealth *= earnings;
            h.net_financial_wealth *= earnings;
            h.main_residence_value *= earnings;
            h.other_residential_property_value *= earnings;
            h.non_residential_property_value *= earnings;
            h.savings *= earnings;
            // num_vehicles: count, not uprated
            // Consumption (CPI-uprated)
            h.food_consumption *= cpi;
            h.alcohol_consumption *= cpi;
            h.tobacco_consumption *= cpi;
            h.clothing_consumption *= cpi;
            h.housing_water_electricity_consumption *= cpi;
            h.furnishings_consumption *= cpi;
            h.health_consumption *= cpi;
            h.transport_consumption *= cpi;
            h.communication_consumption *= cpi;
            h.recreation_consumption *= cpi;
            h.education_consumption *= cpi;
            h.restaurants_consumption *= cpi;
            h.miscellaneous_consumption *= cpi;
            h.petrol_spending *= cpi;
            h.diesel_spending *= cpi;
            h.domestic_energy_consumption *= cpi;
            h.electricity_consumption *= cpi;
            h.gas_consumption *= cpi;
        }
        // Population growth adjusts weights
        for h in &mut self.households {
            h.weight *= population;
        }
        self.year = target_year;
        self.name = format!("{} (uprated to {}/{})", self.name, target_year, (target_year + 1) % 100);
    }
}

// ── Uprating indices ────────────────────────────────────────────────────────
// All rates from OBR EFO November 2025, matching policyengine-uk's
// parameters/gov/economic_assumptions/yoy_growth.yaml

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum UpratingIndex {
    AverageEarnings,
    CPI,
    GDPPerCapita,
    MixedIncomePerCapita,
    Rent,
    CouncilTaxEngland,
    Population,
    HouseholdInterestIncome,
    MortgageInterest,
}

/// Year-on-year growth rates by index. Each entry is (fiscal_year, rate) where
/// the rate applies to the transition *into* that fiscal year.
fn yoy_rates(index: UpratingIndex) -> &'static [(u32, f64)] {
    match index {
        UpratingIndex::AverageEarnings => &[
            (2022, 0.0614), (2023, 0.0622), (2024, 0.0493),
            (2025, 0.0517), (2026, 0.0333), (2027, 0.0225),
            (2028, 0.0210), (2029, 0.0221), (2030, 0.0232),
        ],
        UpratingIndex::CPI => &[
            (2022, 0.0907), (2023, 0.0730), (2024, 0.0253),
            (2025, 0.0345), (2026, 0.0248), (2027, 0.0202),
            (2028, 0.0204), (2029, 0.0204), (2030, 0.0200),
        ],
        UpratingIndex::GDPPerCapita => &[
            (2022, 0.1019), (2023, 0.0532), (2024, 0.0372),
            (2025, 0.0418), (2026, 0.0327), (2027, 0.0326),
            (2028, 0.0302), (2029, 0.0294), (2030, 0.0306),
        ],
        UpratingIndex::MixedIncomePerCapita => &[
            (2022, 0.0296), (2023, -0.0060), (2024, 0.0273),
            (2025, 0.0024), (2026, 0.0362), (2027, 0.0374),
            (2028, 0.0351), (2029, 0.0358), (2030, 0.0364),
        ],
        UpratingIndex::Rent => &[
            (2022, 0.0347), (2023, 0.0575), (2024, 0.0716),
            (2025, 0.0546), (2026, 0.0334), (2027, 0.0311),
            (2028, 0.0243), (2029, 0.0234), (2030, 0.0254),
        ],
        UpratingIndex::CouncilTaxEngland => &[
            (2023, 0.051), (2024, 0.051),
            (2025, 0.0781), (2026, 0.0530), (2027, 0.0579),
            (2028, 0.0565), (2029, 0.0547), (2030, 0.0542),
        ],
        UpratingIndex::Population => &[
            (2022, 0.0093), (2023, 0.0131), (2024, 0.0107),
            (2025, 0.0072), (2026, 0.0038), (2027, 0.0037),
            (2028, 0.0040), (2029, 0.0044), (2030, 0.0045),
        ],
        UpratingIndex::HouseholdInterestIncome => &[
            (2022, 1.210), (2023, 0.987), (2024, 0.142),
            (2025, 0.0519), (2026, 0.0565), (2027, 0.0474),
            (2028, 0.0364), (2029, 0.0302), (2030, 0.0292),
        ],
        UpratingIndex::MortgageInterest => &[
            (2022, 0.1462), (2023, 0.5224), (2024, 0.2730),
            (2025, 0.1098), (2026, 0.1435), (2027, 0.1032),
            (2028, 0.0470), (2029, 0.0466), (2030, 0.0553),
        ],
    }
}

/// Default long-run growth rate when year is outside the table.
fn default_rate(index: UpratingIndex) -> f64 {
    match index {
        UpratingIndex::AverageEarnings => 0.0383,        // OBR long-run
        UpratingIndex::CPI => 0.0200,                     // BoE 2% target
        UpratingIndex::GDPPerCapita => 0.0306,            // last forecast year
        UpratingIndex::MixedIncomePerCapita => 0.0364,
        UpratingIndex::Rent => 0.0254,
        UpratingIndex::CouncilTaxEngland => 0.0542,
        UpratingIndex::Population => 0.0045,
        UpratingIndex::HouseholdInterestIncome => 0.0292,
        UpratingIndex::MortgageInterest => 0.0553,
    }
}

/// Cumulative growth factor from `base_year` to `target_year` using the given index.
fn cumulative_factor(base_year: u32, target_year: u32, index: UpratingIndex) -> f64 {
    let rates = yoy_rates(index);
    let rate_for = |y: u32| -> f64 {
        rates.iter().find(|(yr, _)| *yr == y).map(|(_, r)| *r).unwrap_or(default_rate(index))
    };

    if target_year == base_year {
        return 1.0;
    }
    if target_year > base_year {
        let mut factor = 1.0;
        for y in (base_year + 1)..=target_year {
            factor *= 1.0 + rate_for(y);
        }
        factor
    } else {
        let mut factor = 1.0;
        for y in (target_year + 1)..=base_year {
            factor /= 1.0 + rate_for(y);
        }
        factor
    }
}

/// Public accessor for CPI cumulative factor (used by main.rs for CPI index).
#[allow(dead_code)]
pub fn cpi_cumulative_factor(base_year: u32, target_year: u32) -> f64 {
    cumulative_factor(base_year, target_year, UpratingIndex::CPI)
}
