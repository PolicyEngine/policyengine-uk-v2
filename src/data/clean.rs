use std::collections::HashMap;
use std::path::Path;
use crate::engine::entities::*;
use crate::engine::simulation::SimulationResults;
use crate::data::Dataset;

/// Typed column data for a microdata table. This is the single source of truth
/// for microdata output: CSV serialisation (used by the file and stdout paths)
/// and the in-process columnar transfer to Python both consume `Table`.
///
/// CSV rendering matches the historical formatting exactly (per-column decimal
/// places, bool word vs 0/1), while the native Python path receives full
/// precision and clean dtypes (int64, float64, bool, object).
pub enum ColumnData {
    Int(Vec<i64>),
    /// Full-precision floats. `dp` controls CSV decimal places only.
    Float { vals: Vec<f64>, dp: usize },
    /// `csv_word` true → "true"/"false"; false → "1"/"0". Always bool to Python.
    Bool { vals: Vec<bool>, csv_word: bool },
    Text(Vec<String>),
}

pub struct Column {
    pub name: &'static str,
    pub data: ColumnData,
}

pub struct Table {
    pub columns: Vec<Column>,
}

impl Column {
    fn len(&self) -> usize {
        match &self.data {
            ColumnData::Int(v) => v.len(),
            ColumnData::Float { vals, .. } => vals.len(),
            ColumnData::Bool { vals, .. } => vals.len(),
            ColumnData::Text(v) => v.len(),
        }
    }

    fn csv_cell(&self, row: usize) -> String {
        match &self.data {
            ColumnData::Int(v) => v[row].to_string(),
            ColumnData::Float { vals, dp } => format!("{:.*}", dp, vals[row]),
            ColumnData::Bool { vals, csv_word } => {
                if *csv_word {
                    vals[row].to_string()
                } else if vals[row] {
                    "1".to_string()
                } else {
                    "0".to_string()
                }
            }
            ColumnData::Text(v) => v[row].clone(),
        }
    }
}

impl Table {
    fn nrows(&self) -> usize {
        self.columns.first().map(|c| c.len()).unwrap_or(0)
    }

    /// Serialise to CSV using the same writer/quoting behaviour as before.
    pub fn write_csv<W: std::io::Write>(&self, writer: W) -> anyhow::Result<()> {
        let mut wtr = csv::Writer::from_writer(writer);
        let header: Vec<&str> = self.columns.iter().map(|c| c.name).collect();
        wtr.write_record(&header)?;
        let nrows = self.nrows();
        let mut record: Vec<String> = Vec::with_capacity(self.columns.len());
        for row in 0..nrows {
            record.clear();
            for col in &self.columns {
                record.push(col.csv_cell(row));
            }
            wtr.write_record(&record)?;
        }
        wtr.flush()?;
        Ok(())
    }
}

/// CPI index by fiscal year, rebased to 2010/11 = 100 (the absolute-poverty
/// reference year). Source: OBR EFO March 2026 table 1.7 CPI (2015=100), with
/// pre-2010 fiscal years from ONS series D7BT financial-year averages.
pub fn cpi_index_for_year(year: u32) -> f64 {
    let table: &[(u32, f64)] = &[
        (1994, 72.916542), (1995, 74.863074), (1996, 76.532848), (1997, 77.879738),
        (1998, 79.079023), (1999, 79.983099), (2000, 80.647319), (2001, 81.772802),
        (2002, 82.787582), (2003, 83.876164), (2004, 85.084674), (2005, 86.874377),
        (2006, 89.116118), (2007, 91.071875), (2008, 94.485225), (2009, 96.616263),
        (2010, 100.000000), (2011, 104.300545), (2012, 107.068495), (2013, 109.535701),
        (2014, 110.686646), (2015, 110.798825), (2016, 112.025879), (2017, 115.190516),
        (2018, 117.802559), (2019, 119.851492), (2020, 120.557502), (2021, 125.368849),
        (2022, 137.951381), (2023, 145.773765), (2024, 149.171329), (2025, 153.921493),
        (2026, 156.889890), (2027, 160.021436), (2028, 163.222508), (2029, 166.486583),
    ];
    table.iter()
        .find(|(y, _)| *y == year)
        .map(|(_, v)| *v)
        .unwrap_or(100.0)
}

/// Write a Dataset to clean CSVs with descriptive column names.
///
/// Produces three files in `output_dir`:
///   - persons.csv: one row per person, annual values
///   - benunits.csv: one row per benefit unit
///   - households.csv: one row per household
pub fn write_clean_csvs(dataset: &mut Dataset, output_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(output_dir)?;

    write_persons(dataset, output_dir)?;
    write_benunits(dataset, output_dir)?;
    write_households(dataset, output_dir)?;

    Ok(())
}

fn write_persons(dataset: &Dataset, output_dir: &Path) -> anyhow::Result<()> {
    let path = output_dir.join("persons.csv");
    let mut wtr = csv::Writer::from_path(&path)?;

    wtr.write_record(&[
        "person_id", "benunit_id", "household_id",
        "age", "gender", "is_benunit_head", "is_household_head",
        // Income (annual)
        "employment_income", "self_employment_income",
        "private_pension_income", "state_pension",
        "savings_interest", "dividend_income", "capital_gains",
        "capital_gains_residential_share",
        "property_income", "maintenance_income",
        "miscellaneous_income", "other_income",
        // Employment
        "is_in_scotland", "hours_worked_annual",
        // Disability rate-band flags
        "dla_care_low", "dla_care_mid", "dla_care_high",
        "dla_mob_low", "dla_mob_high",
        "pip_dl_std", "pip_dl_enh",
        "pip_mob_std", "pip_mob_enh",
        "aa_low", "aa_high",
        // Status
        "is_disabled", "is_enhanced_disabled", "is_severely_disabled", "is_carer",
        "limitill", "esa_group", "emp_status", "looking_for_work",
        "is_self_identified_carer",
        // Contributions (annual)
        "employee_pension_contributions", "personal_pension_contributions",
        "childcare_expenses",
        // Benefits (annual)
        "child_benefit", "housing_benefit",
        "income_support", "pension_credit",
        "child_tax_credit", "working_tax_credit",
        "universal_credit",
        "dla_care", "dla_mobility",
        "pip_daily_living", "pip_mobility",
        "carers_allowance", "attendance_allowance",
        "esa_income", "esa_contributory",
        "jsa_income", "jsa_contributory",
        "other_benefits",
        "adp_daily_living", "adp_mobility",
        "cdp_care", "cdp_mobility",
    ])?;

    for p in &dataset.people {
        wtr.write_record(&[
            p.id.to_string(),
            p.benunit_id.to_string(),
            p.household_id.to_string(),
            format!("{:.0}", p.age),
            if p.gender == Gender::Male { "male".to_string() } else { "female".to_string() },
            p.is_benunit_head.to_string(),
            p.is_household_head.to_string(),
            format!("{:.2}", p.employment_income),
            format!("{:.2}", p.self_employment_income),
            format!("{:.2}", p.pension_income),
            format!("{:.2}", p.state_pension),
            format!("{:.2}", p.savings_interest_income),
            format!("{:.2}", p.dividend_income),
            format!("{:.2}", p.capital_gains),
            format!("{:.4}", p.capital_gains_residential_share),
            format!("{:.2}", p.property_income),
            format!("{:.2}", p.maintenance_income),
            format!("{:.2}", p.miscellaneous_income),
            format!("{:.2}", p.other_income),
            p.is_in_scotland.to_string(),
            format!("{:.1}", p.hours_worked),
            p.dla_care_low.to_string(),
            p.dla_care_mid.to_string(),
            p.dla_care_high.to_string(),
            p.dla_mob_low.to_string(),
            p.dla_mob_high.to_string(),
            p.pip_dl_std.to_string(),
            p.pip_dl_enh.to_string(),
            p.pip_mob_std.to_string(),
            p.pip_mob_enh.to_string(),
            p.aa_low.to_string(),
            p.aa_high.to_string(),
            p.is_disabled.to_string(),
            p.is_enhanced_disabled.to_string(),
            p.is_severely_disabled.to_string(),
            p.is_carer.to_string(),
            p.limitill.to_string(),
            p.esa_group.to_string(),
            p.emp_status.to_string(),
            p.looking_for_work.to_string(),
            p.is_self_identified_carer.to_string(),
            format!("{:.2}", p.employee_pension_contributions),
            format!("{:.2}", p.personal_pension_contributions),
            format!("{:.2}", p.childcare_expenses),
            format!("{:.2}", p.child_benefit),
            format!("{:.2}", p.housing_benefit),
            format!("{:.2}", p.income_support),
            format!("{:.2}", p.pension_credit),
            format!("{:.2}", p.child_tax_credit),
            format!("{:.2}", p.working_tax_credit),
            format!("{:.2}", p.universal_credit),
            format!("{:.2}", p.dla_care),
            format!("{:.2}", p.dla_mobility),
            format!("{:.2}", p.pip_daily_living),
            format!("{:.2}", p.pip_mobility),
            format!("{:.2}", p.carers_allowance),
            format!("{:.2}", p.attendance_allowance),
            format!("{:.2}", p.esa_income),
            format!("{:.2}", p.esa_contributory),
            format!("{:.2}", p.jsa_income),
            format!("{:.2}", p.jsa_contributory),
            format!("{:.2}", p.other_benefits),
            format!("{:.2}", p.adp_daily_living),
            format!("{:.2}", p.adp_mobility),
            format!("{:.2}", p.cdp_care),
            format!("{:.2}", p.cdp_mobility),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

fn write_benunits(dataset: &Dataset, output_dir: &Path) -> anyhow::Result<()> {
    let path = output_dir.join("benunits.csv");
    let mut wtr = csv::Writer::from_path(&path)?;

    wtr.write_record(&[
        "benunit_id", "household_id",
        "person_ids",
        "on_uc",
        "rent_monthly", "is_lone_parent",
        "claims_uc_if_eligible",
    ])?;

    for bu in &dataset.benunits {
        let ids: String = bu.person_ids.iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(";");

        wtr.write_record(&[
            bu.id.to_string(),
            bu.household_id.to_string(),
            ids,
            bu.on_uc.to_string(),
            format!("{:.2}", bu.rent_monthly),
            bu.is_lone_parent.to_string(),
            bu.claims_uc_if_eligible.to_string(),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

fn write_households(dataset: &Dataset, output_dir: &Path) -> anyhow::Result<()> {
    let path = output_dir.join("households.csv");
    let mut wtr = csv::Writer::from_path(&path)?;

    wtr.write_record(&[
        "household_id",
        "benunit_ids", "person_ids",
        "weight", "region",
        "rent_annual", "council_tax_annual", "council_tax_band",
        // Auxiliary
        "num_bedrooms", "tenure_type", "accommodation_type",
        // Wealth
        "owned_land", "property_wealth", "corporate_wealth",
        "gross_financial_wealth", "net_financial_wealth",
        "main_residence_value", "other_residential_property_value",
        "non_residential_property_value", "savings", "num_vehicles",
        // Consumption
        "food_consumption", "alcohol_consumption", "tobacco_consumption", "clothing_consumption",
        "housing_water_electricity_consumption", "furnishings_consumption",
        "health_consumption", "transport_consumption", "communication_consumption",
        "recreation_consumption", "education_consumption", "restaurants_consumption",
        "miscellaneous_consumption", "petrol_spending", "diesel_spending",
        "domestic_energy_consumption", "electricity_consumption", "gas_consumption",
    ])?;

    for hh in &dataset.households {
        let bu_ids: String = hh.benunit_ids.iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(";");
        let p_ids: String = hh.person_ids.iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(";");

        wtr.write_record(&[
            hh.id.to_string(),
            bu_ids,
            p_ids,
            format!("{:.4}", hh.weight),
            hh.region.name().to_string(),
            format!("{:.2}", hh.rent),
            format!("{:.2}", hh.council_tax),
            hh.council_tax_band.to_string(),
            // Auxiliary
            hh.num_bedrooms.to_string(),
            (hh.tenure_type.to_rf_code() as i32).to_string(),
            (hh.accommodation_type.to_rf_code() as i32).to_string(),
            // Wealth
            format!("{:.2}", hh.owned_land),
            format!("{:.2}", hh.property_wealth),
            format!("{:.2}", hh.corporate_wealth),
            format!("{:.2}", hh.gross_financial_wealth),
            format!("{:.2}", hh.net_financial_wealth),
            format!("{:.2}", hh.main_residence_value),
            format!("{:.2}", hh.other_residential_property_value),
            format!("{:.2}", hh.non_residential_property_value),
            format!("{:.2}", hh.savings),
            format!("{:.2}", hh.num_vehicles),
            // Consumption
            format!("{:.2}", hh.food_consumption),
            format!("{:.2}", hh.alcohol_consumption),
            format!("{:.2}", hh.tobacco_consumption),
            format!("{:.2}", hh.clothing_consumption),
            format!("{:.2}", hh.housing_water_electricity_consumption),
            format!("{:.2}", hh.furnishings_consumption),
            format!("{:.2}", hh.health_consumption),
            format!("{:.2}", hh.transport_consumption),
            format!("{:.2}", hh.communication_consumption),
            format!("{:.2}", hh.recreation_consumption),
            format!("{:.2}", hh.education_consumption),
            format!("{:.2}", hh.restaurants_consumption),
            format!("{:.2}", hh.miscellaneous_consumption),
            format!("{:.2}", hh.petrol_spending),
            format!("{:.2}", hh.diesel_spending),
            format!("{:.2}", hh.domestic_energy_consumption),
            format!("{:.2}", hh.electricity_consumption),
            format!("{:.2}", hh.gas_consumption),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

/// Write enhanced microdata: input data + simulation outputs in one CSV per entity.
pub fn write_microdata(
    dataset: &Dataset,
    baseline: &SimulationResults,
    reformed: &SimulationResults,
    output_dir: &Path,
    year: u32,
    return_baselines: bool,
) -> anyhow::Result<()> {
    write_microdata_persons(dataset, baseline, reformed, output_dir, return_baselines)?;
    write_microdata_benunits(dataset, baseline, reformed, output_dir, return_baselines)?;
    write_microdata_households(dataset, baseline, reformed, output_dir, year, return_baselines)?;
    Ok(())
}

/// Write enhanced microdata to stdout using the concatenated CSV protocol.
pub fn write_microdata_to_stdout(
    dataset: &Dataset,
    baseline: &SimulationResults,
    reformed: &SimulationResults,
    year: u32,
    return_baselines: bool,
) -> anyhow::Result<()> {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    write!(out, "===PERSONS===\n")?;
    write_microdata_csv_persons(&mut out, dataset, baseline, reformed, return_baselines)?;

    write!(out, "===BENUNITS===\n")?;
    write_microdata_csv_benunits(&mut out, dataset, baseline, reformed, return_baselines)?;

    write!(out, "===HOUSEHOLDS===\n")?;
    write_microdata_csv_households(&mut out, dataset, baseline, reformed, year, return_baselines)?;

    out.flush()?;
    Ok(())
}

fn write_microdata_persons(
    dataset: &Dataset,
    baseline: &SimulationResults,
    reformed: &SimulationResults,
    output_dir: &Path,
    return_baselines: bool,
) -> anyhow::Result<()> {
    let path = output_dir.join("persons.csv");
    let file = std::fs::File::create(&path)?;
    write_microdata_csv_persons(file, dataset, baseline, reformed, return_baselines)
}

fn write_microdata_csv_persons<W: std::io::Write>(
    writer: W,
    dataset: &Dataset,
    baseline: &SimulationResults,
    reformed: &SimulationResults,
    return_baselines: bool,
) -> anyhow::Result<()> {
    build_microdata_persons_table(dataset, baseline, reformed, return_baselines).write_csv(writer)
}

/// Build the typed persons microdata table (single source of truth for both
/// CSV and the in-process columnar transfer).
pub fn build_microdata_persons_table(
    dataset: &Dataset,
    baseline: &SimulationResults,
    reformed: &SimulationResults,
    return_baselines: bool,
) -> Table {
    let people = &dataset.people;
    let icol = |name, f: &dyn Fn(&Person) -> i64| Column {
        name, data: ColumnData::Int(people.iter().map(f).collect()),
    };
    let fcol = |name, dp, f: &dyn Fn(&Person) -> f64| Column {
        name, data: ColumnData::Float { vals: people.iter().map(f).collect(), dp },
    };
    let bcol = |name, f: &dyn Fn(&Person) -> bool| Column {
        name, data: ColumnData::Bool { vals: people.iter().map(f).collect(), csv_word: true },
    };
    let bcol01 = |name, f: &dyn Fn(&Person) -> bool| Column {
        name, data: ColumnData::Bool { vals: people.iter().map(f).collect(), csv_word: false },
    };

    let mut columns = vec![
        icol("person_id", &|p| p.id as i64),
        icol("benunit_id", &|p| p.benunit_id as i64),
        icol("household_id", &|p| p.household_id as i64),
        fcol("age", 0, &|p| p.age),
        Column { name: "gender", data: ColumnData::Text(
            people.iter().map(|p| if p.gender == Gender::Male { "male".to_string() } else { "female".to_string() }).collect()) },
        bcol("is_benunit_head", &|p| p.is_benunit_head),
        bcol("is_household_head", &|p| p.is_household_head),
        fcol("employment_income", 2, &|p| p.employment_income),
        fcol("self_employment_income", 2, &|p| p.self_employment_income),
        fcol("private_pension_income", 2, &|p| p.pension_income),
        fcol("state_pension", 2, &|p| p.state_pension),
        fcol("savings_interest", 2, &|p| p.savings_interest_income),
        fcol("dividend_income", 2, &|p| p.dividend_income),
        fcol("capital_gains", 2, &|p| p.capital_gains),
        fcol("capital_gains_residential_share", 4, &|p| p.capital_gains_residential_share),
        fcol("property_income", 2, &|p| p.property_income),
        fcol("maintenance_income", 2, &|p| p.maintenance_income),
        fcol("miscellaneous_income", 2, &|p| p.miscellaneous_income),
        fcol("other_income", 2, &|p| p.other_income),
        bcol("is_in_scotland", &|p| p.is_in_scotland),
        fcol("hours_worked_annual", 1, &|p| p.hours_worked),
        bcol01("is_employed", &|p| p.emp_status == 1 || p.emp_status == 2),
        bcol01("is_unemployed", &|p| p.emp_status == 3),
        bcol("is_disabled", &|p| p.is_disabled),
        bcol("is_carer", &|p| p.is_carer),
        fcol("dla_care", 2, &|p| p.dla_care),
        fcol("dla_mobility", 2, &|p| p.dla_mobility),
        fcol("pip_daily_living", 2, &|p| p.pip_daily_living),
        fcol("pip_mobility", 2, &|p| p.pip_mobility),
        fcol("attendance_allowance", 2, &|p| p.attendance_allowance),
        fcol("adp_daily_living", 2, &|p| p.adp_daily_living),
        fcol("adp_mobility", 2, &|p| p.adp_mobility),
        fcol("cdp_care", 2, &|p| p.cdp_care),
        fcol("cdp_mobility", 2, &|p| p.cdp_mobility),
        fcol("employee_pension_contributions", 2, &|p| p.employee_pension_contributions),
        fcol("personal_pension_contributions", 2, &|p| p.personal_pension_contributions),
        fcol("childcare_expenses", 2, &|p| p.childcare_expenses),
        fcol("child_benefit", 2, &|p| p.child_benefit),
        fcol("housing_benefit", 2, &|p| p.housing_benefit),
        fcol("income_support", 2, &|p| p.income_support),
        fcol("pension_credit", 2, &|p| p.pension_credit),
        fcol("child_tax_credit", 2, &|p| p.child_tax_credit),
        fcol("working_tax_credit", 2, &|p| p.working_tax_credit),
        fcol("universal_credit", 2, &|p| p.universal_credit),
        fcol("carers_allowance", 2, &|p| p.carers_allowance),
    ];

    // Output columns, indexed by person id into the result vectors.
    let res_fcol = |name, results: &SimulationResults, f: &dyn Fn(&crate::engine::simulation::PersonResult) -> f64| Column {
        name, data: ColumnData::Float {
            vals: people.iter().map(|p| f(&results.person_results[p.id])).collect(), dp: 2 },
    };
    let outputs = |bl_or_rf: &SimulationResults, names: [&'static str; 10]| -> Vec<Column> {
        vec![
            res_fcol(names[0], bl_or_rf, &|r| r.income_tax),
            res_fcol(names[1], bl_or_rf, &|r| r.national_insurance),
            res_fcol(names[2], bl_or_rf, &|r| r.employer_ni),
            res_fcol(names[3], bl_or_rf, &|r| r.ni_class1_employee),
            res_fcol(names[4], bl_or_rf, &|r| r.ni_class2),
            res_fcol(names[5], bl_or_rf, &|r| r.ni_class4),
            res_fcol(names[6], bl_or_rf, &|r| r.total_income),
            res_fcol(names[7], bl_or_rf, &|r| r.taxable_income),
            res_fcol(names[8], bl_or_rf, &|r| r.personal_allowance),
            res_fcol(names[9], bl_or_rf, &|r| r.capital_gains_tax),
        ]
    };

    if return_baselines {
        columns.extend(outputs(baseline, [
            "baseline_income_tax", "baseline_employee_ni", "baseline_employer_ni",
            "baseline_ni_class1_employee", "baseline_ni_class2", "baseline_ni_class4",
            "baseline_total_income", "baseline_taxable_income",
            "baseline_personal_allowance", "baseline_capital_gains_tax",
        ]));
        columns.extend(outputs(reformed, [
            "reform_income_tax", "reform_employee_ni", "reform_employer_ni",
            "reform_ni_class1_employee", "reform_ni_class2", "reform_ni_class4",
            "reform_total_income", "reform_taxable_income",
            "reform_personal_allowance", "reform_capital_gains_tax",
        ]));
    } else {
        columns.extend(outputs(reformed, [
            "income_tax", "employee_ni", "employer_ni",
            "ni_class1_employee", "ni_class2", "ni_class4",
            "total_income", "taxable_income",
            "personal_allowance", "capital_gains_tax",
        ]));
    }

    Table { columns }
}

fn write_microdata_benunits(
    dataset: &Dataset,
    baseline: &SimulationResults,
    reformed: &SimulationResults,
    output_dir: &Path,
    return_baselines: bool,
) -> anyhow::Result<()> {
    let path = output_dir.join("benunits.csv");
    let file = std::fs::File::create(&path)?;
    write_microdata_csv_benunits(file, dataset, baseline, reformed, return_baselines)
}

fn write_microdata_csv_benunits<W: std::io::Write>(
    writer: W,
    dataset: &Dataset,
    baseline: &SimulationResults,
    reformed: &SimulationResults,
    return_baselines: bool,
) -> anyhow::Result<()> {
    build_microdata_benunits_table(dataset, baseline, reformed, return_baselines).write_csv(writer)
}

/// Build the typed benunits microdata table.
pub fn build_microdata_benunits_table(
    dataset: &Dataset,
    baseline: &SimulationResults,
    reformed: &SimulationResults,
    return_baselines: bool,
) -> Table {
    let benunits = &dataset.benunits;
    let icol = |name, f: &dyn Fn(&BenUnit) -> i64| Column {
        name, data: ColumnData::Int(benunits.iter().map(f).collect()),
    };
    let fcol = |name, f: &dyn Fn(&BenUnit) -> f64| Column {
        name, data: ColumnData::Float { vals: benunits.iter().map(f).collect(), dp: 2 },
    };
    let bcol = |name, f: &dyn Fn(&BenUnit) -> bool| Column {
        name, data: ColumnData::Bool { vals: benunits.iter().map(f).collect(), csv_word: true },
    };

    let mut columns = vec![
        icol("benunit_id", &|b| b.id as i64),
        icol("household_id", &|b| b.household_id as i64),
        Column { name: "person_ids", data: ColumnData::Text(
            benunits.iter().map(|b| b.person_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(";")).collect()) },
        bcol("on_uc", &|b| b.on_uc),
        fcol("rent_monthly", &|b| b.rent_monthly),
        bcol("is_lone_parent", &|b| b.is_lone_parent),
        bcol("claims_uc_if_eligible", &|b| b.claims_uc_if_eligible),
    ];

    let res_fcol = |name, results: &SimulationResults, f: &dyn Fn(&crate::engine::simulation::BenUnitResult) -> f64| Column {
        name, data: ColumnData::Float {
            vals: benunits.iter().map(|b| f(&results.benunit_results[b.id])).collect(), dp: 2 },
    };
    let outputs = |r: &SimulationResults, names: [&'static str; 21]| -> Vec<Column> {
        vec![
            res_fcol(names[0], r, &|x| x.universal_credit),
            res_fcol(names[1], r, &|x| x.child_benefit),
            res_fcol(names[2], r, &|x| x.state_pension),
            res_fcol(names[3], r, &|x| x.pension_credit),
            res_fcol(names[4], r, &|x| x.housing_benefit),
            res_fcol(names[5], r, &|x| x.child_tax_credit),
            res_fcol(names[6], r, &|x| x.working_tax_credit),
            res_fcol(names[7], r, &|x| x.income_support),
            res_fcol(names[8], r, &|x| x.esa_income_related),
            res_fcol(names[9], r, &|x| x.jsa_income_based),
            res_fcol(names[10], r, &|x| x.carers_allowance),
            res_fcol(names[11], r, &|x| x.scottish_child_payment),
            res_fcol(names[12], r, &|x| x.benefit_cap_reduction),
            res_fcol(names[13], r, &|x| x.passthrough_benefits),
            res_fcol(names[14], r, &|x| x.total_benefits),
            res_fcol(names[15], r, &|x| x.uc_standard_allowance),
            res_fcol(names[16], r, &|x| x.uc_child_element),
            res_fcol(names[17], r, &|x| x.uc_disabled_child_element),
            res_fcol(names[18], r, &|x| x.uc_lcwra_element),
            res_fcol(names[19], r, &|x| x.uc_carer_element),
            res_fcol(names[20], r, &|x| x.uc_housing_element),
        ]
    };

    if return_baselines {
        columns.extend(outputs(baseline, [
            "baseline_universal_credit", "baseline_child_benefit",
            "baseline_state_pension", "baseline_pension_credit",
            "baseline_housing_benefit",
            "baseline_child_tax_credit", "baseline_working_tax_credit",
            "baseline_income_support",
            "baseline_esa_income_related", "baseline_jsa_income_based",
            "baseline_carers_allowance", "baseline_scottish_child_payment",
            "baseline_benefit_cap_reduction", "baseline_passthrough_benefits",
            "baseline_total_benefits",
            "baseline_uc_standard_allowance", "baseline_uc_child_element",
            "baseline_uc_disabled_child_element", "baseline_uc_lcwra_element",
            "baseline_uc_carer_element", "baseline_uc_housing_element",
        ]));
        columns.extend(outputs(reformed, [
            "reform_universal_credit", "reform_child_benefit",
            "reform_state_pension", "reform_pension_credit",
            "reform_housing_benefit",
            "reform_child_tax_credit", "reform_working_tax_credit",
            "reform_income_support",
            "reform_esa_income_related", "reform_jsa_income_based",
            "reform_carers_allowance", "reform_scottish_child_payment",
            "reform_benefit_cap_reduction", "reform_passthrough_benefits",
            "reform_total_benefits",
            "reform_uc_standard_allowance", "reform_uc_child_element",
            "reform_uc_disabled_child_element", "reform_uc_lcwra_element",
            "reform_uc_carer_element", "reform_uc_housing_element",
        ]));
    } else {
        columns.extend(outputs(reformed, [
            "universal_credit", "child_benefit",
            "state_pension", "pension_credit",
            "housing_benefit",
            "child_tax_credit", "working_tax_credit",
            "income_support",
            "esa_income_related", "jsa_income_based",
            "carers_allowance", "scottish_child_payment",
            "benefit_cap_reduction", "passthrough_benefits",
            "total_benefits",
            "uc_standard_allowance", "uc_child_element",
            "uc_disabled_child_element", "uc_lcwra_element",
            "uc_carer_element", "uc_housing_element",
        ]));
    }

    Table { columns }
}

fn write_microdata_households(
    dataset: &Dataset,
    baseline: &SimulationResults,
    reformed: &SimulationResults,
    output_dir: &Path,
    year: u32,
    return_baselines: bool,
) -> anyhow::Result<()> {
    let path = output_dir.join("households.csv");
    let file = std::fs::File::create(&path)?;
    write_microdata_csv_households(file, dataset, baseline, reformed, year, return_baselines)
}

/// Person-weighted median equivalised income (BHC and AHC) over all households,
/// matching the HBAI aggregation in `run::analyse`. Each person carries the
/// household's weight; the median is taken over the per-person distribution.
fn person_weighted_median_equiv(
    dataset: &Dataset,
    results: &SimulationResults,
) -> (f64, f64) {
    let total_person_weight: f64 = dataset.households.iter()
        .map(|h| h.weight * h.person_ids.len() as f64)
        .sum();
    let median = |mut vals: Vec<(f64, f64)>| -> f64 {
        vals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let half = total_person_weight / 2.0;
        let mut cum = 0.0;
        for (v, w) in &vals {
            cum += w;
            if cum >= half { return *v; }
        }
        vals.last().map(|&(v, _)| v).unwrap_or(0.0)
    };
    let bhc = median(dataset.households.iter()
        .map(|h| (results.household_results[h.id].equivalised_net_income,
                  h.weight * h.person_ids.len() as f64))
        .collect());
    let ahc = median(dataset.households.iter()
        .map(|h| (results.household_results[h.id].equivalised_net_income_ahc,
                  h.weight * h.person_ids.len() as f64))
        .collect());
    (bhc, ahc)
}

fn write_microdata_csv_households<W: std::io::Write>(
    writer: W,
    dataset: &Dataset,
    baseline: &SimulationResults,
    reformed: &SimulationResults,
    year: u32,
    return_baselines: bool,
) -> anyhow::Result<()> {
    build_microdata_households_table(dataset, baseline, reformed, year, return_baselines).write_csv(writer)
}

/// Build the typed households microdata table. Poverty flags depend on
/// year-specific CPI and the person-weighted median equivalised income.
pub fn build_microdata_households_table(
    dataset: &Dataset,
    baseline: &SimulationResults,
    reformed: &SimulationResults,
    year: u32,
    return_baselines: bool,
) -> Table {
    let (median_equiv_bhc, median_equiv_ahc) =
        person_weighted_median_equiv(dataset, baseline);
    let rel_line_bhc = 0.60 * median_equiv_bhc;
    let rel_line_ahc = 0.60 * median_equiv_ahc;
    let cpi = cpi_index_for_year(year) / 100.0;
    let abs_line_bhc = 14_400.0 * cpi;
    let abs_line_ahc = 11_600.0 * cpi;

    let households = &dataset.households;
    let icol = |name, f: &dyn Fn(&Household) -> i64| Column {
        name, data: ColumnData::Int(households.iter().map(f).collect()),
    };
    let fcol = |name, dp, f: &dyn Fn(&Household) -> f64| Column {
        name, data: ColumnData::Float { vals: households.iter().map(f).collect(), dp },
    };

    let mut columns = vec![
        icol("household_id", &|h| h.id as i64),
        fcol("weight", 4, &|h| h.weight),
        Column { name: "region", data: ColumnData::Text(
            households.iter().map(|h| h.region.name().to_string()).collect()) },
        fcol("rent_annual", 2, &|h| h.rent),
        fcol("council_tax_annual", 2, &|h| h.council_tax),
        icol("tenure_type", &|h| h.tenure_type.to_rf_code() as i64),
        icol("council_tax_band", &|h| h.council_tax_band as i64),
    ];

    type HR = crate::engine::simulation::HouseholdResult;
    let res_fcol = |name, dp, results: &SimulationResults, f: &dyn Fn(&HR) -> f64| Column {
        name, data: ColumnData::Float {
            vals: households.iter().map(|h| f(&results.household_results[h.id])).collect(), dp },
    };
    let res_bcol = |name, results: &SimulationResults, f: &dyn Fn(&HR) -> bool| Column {
        name, data: ColumnData::Bool {
            vals: households.iter().map(|h| f(&results.household_results[h.id])).collect(), csv_word: false },
    };
    let outputs = |r: &SimulationResults, names: [&'static str; 15]| -> Vec<Column> {
        vec![
            res_fcol(names[0], 2, r, &|x| x.net_income),
            res_fcol(names[1], 2, r, &|x| x.gross_income),
            res_fcol(names[2], 2, r, &|x| x.total_tax),
            res_fcol(names[3], 2, r, &|x| x.total_benefits),
            res_fcol(names[4], 2, r, &|x| x.stamp_duty),
            res_fcol(names[5], 2, r, &|x| x.vat),
            res_fcol(names[6], 2, r, &|x| x.fuel_duty),
            res_fcol(names[7], 4, r, &|x| x.equivalisation_factor),
            res_fcol(names[8], 2, r, &|x| x.equivalised_net_income),
            res_fcol(names[9], 2, r, &|x| x.net_income_ahc),
            res_fcol(names[10], 2, r, &|x| x.equivalised_net_income_ahc),
            res_bcol(names[11], r, &|x| x.equivalised_net_income < rel_line_bhc),
            res_bcol(names[12], r, &|x| x.equivalised_net_income_ahc < rel_line_ahc),
            res_bcol(names[13], r, &|x| x.equivalised_net_income < abs_line_bhc),
            res_bcol(names[14], r, &|x| x.equivalised_net_income_ahc < abs_line_ahc),
        ]
    };

    if return_baselines {
        columns.extend(outputs(baseline, [
            "baseline_net_income", "baseline_gross_income",
            "baseline_total_tax", "baseline_total_benefits",
            "baseline_property_transaction_tax",
            "baseline_vat", "baseline_fuel_duty",
            "baseline_equivalisation_factor", "baseline_equivalised_net_income",
            "baseline_net_income_ahc", "baseline_equivalised_net_income_ahc",
            "baseline_in_relative_poverty_bhc", "baseline_in_relative_poverty_ahc",
            "baseline_in_absolute_poverty_bhc", "baseline_in_absolute_poverty_ahc",
        ]));
        columns.extend(outputs(reformed, [
            "reform_net_income", "reform_gross_income",
            "reform_total_tax", "reform_total_benefits",
            "reform_property_transaction_tax",
            "reform_vat", "reform_fuel_duty",
            "reform_equivalisation_factor", "reform_equivalised_net_income",
            "reform_net_income_ahc", "reform_equivalised_net_income_ahc",
            "reform_in_relative_poverty_bhc", "reform_in_relative_poverty_ahc",
            "reform_in_absolute_poverty_bhc", "reform_in_absolute_poverty_ahc",
        ]));
    } else {
        columns.extend(outputs(reformed, [
            "net_income", "gross_income",
            "total_tax", "total_benefits",
            "property_transaction_tax",
            "vat", "fuel_duty",
            "equivalisation_factor", "equivalised_net_income",
            "net_income_ahc", "equivalised_net_income_ahc",
            "in_relative_poverty_bhc", "in_relative_poverty_ahc",
            "in_absolute_poverty_bhc", "in_absolute_poverty_ahc",
        ]));
    }

    Table { columns }
}

/// Remap entity IDs to contiguous 0..N so they can be used as Vec indices.
/// This handles non-contiguous or sparse IDs from external input (e.g. API).
fn remap_entity_ids(
    people: &mut Vec<Person>,
    benunits: &mut Vec<BenUnit>,
    households: &mut Vec<Household>,
) {
    // Build old→new mappings based on position in the Vec
    let person_map: HashMap<usize, usize> = people.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    let benunit_map: HashMap<usize, usize> = benunits.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
    let household_map: HashMap<usize, usize> = households.iter().enumerate().map(|(i, h)| (h.id, i)).collect();

    // Check if remapping is needed (all IDs already contiguous 0..N)
    let needs_remap = people.iter().enumerate().any(|(i, p)| p.id != i)
        || benunits.iter().enumerate().any(|(i, b)| b.id != i)
        || households.iter().enumerate().any(|(i, h)| h.id != i);

    if !needs_remap {
        return;
    }

    // Remap person fields
    for p in people.iter_mut() {
        p.id = *person_map.get(&p.id).unwrap_or(&p.id);
        p.benunit_id = *benunit_map.get(&p.benunit_id).unwrap_or(&p.benunit_id);
        p.household_id = *household_map.get(&p.household_id).unwrap_or(&p.household_id);
    }

    // Remap benunit fields
    for bu in benunits.iter_mut() {
        bu.id = *benunit_map.get(&bu.id).unwrap_or(&bu.id);
        bu.household_id = *household_map.get(&bu.household_id).unwrap_or(&bu.household_id);
        bu.person_ids = bu.person_ids.iter().filter_map(|pid| person_map.get(pid).copied()).collect();
    }

    // Remap household fields
    for hh in households.iter_mut() {
        hh.id = *household_map.get(&hh.id).unwrap_or(&hh.id);
        hh.benunit_ids = hh.benunit_ids.iter().filter_map(|bid| benunit_map.get(bid).copied()).collect();
        hh.person_ids = hh.person_ids.iter().filter_map(|pid| person_map.get(pid).copied()).collect();
    }
}

/// Load a Dataset from clean CSVs (persons.csv, benunits.csv, households.csv).
pub fn load_clean_dataset(data_dir: &Path, year: u32) -> anyhow::Result<Dataset> {
    let mut households = parse_households_csv(std::fs::File::open(data_dir.join("households.csv"))?)?;
    let mut benunits = parse_benunits_csv(std::fs::File::open(data_dir.join("benunits.csv"))?)?;
    let mut people = parse_persons_csv(std::fs::File::open(data_dir.join("persons.csv"))?)?;

    // Remap sparse IDs to contiguous 0..N for Vec indexing
    remap_entity_ids(&mut people, &mut benunits, &mut households);

    Ok(Dataset {
        people,
        benunits,
        households,
        name: "dataset".to_string(),
        year,
    })
}

/// Assemble a Dataset from pre-parsed entity vectors.
pub fn assemble_dataset(
    mut people: Vec<Person>,
    mut benunits: Vec<BenUnit>,
    mut households: Vec<Household>,
    year: u32,
) -> Dataset {
    // Remap sparse IDs to contiguous 0..N for Vec indexing
    remap_entity_ids(&mut people, &mut benunits, &mut households);

    // Auto-derive is_in_scotland from household region
    for p in &mut people {
        if let Some(hh) = households.get(p.household_id) {
            if hh.region.is_scotland() {
                p.is_in_scotland = true;
            }
        }
    }
    Dataset {
        people,
        benunits,
        households,
        name: "dataset".to_string(),
        year,
    }
}

fn parse_bool(s: &str) -> bool {
    s == "true" || s == "1" || s == "True" || s == "TRUE"
}

fn parse_f64(s: &str) -> f64 {
    s.parse::<f64>().unwrap_or(0.0)
}

fn parse_usize(s: &str) -> usize {
    s.parse::<usize>().unwrap_or(0)
}

fn parse_i64(s: &str) -> i64 {
    s.parse::<i64>().unwrap_or(0)
}

fn parse_id_list(s: &str) -> Vec<usize> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(|c| c == ';' || c == ',').filter_map(|x| x.trim().parse::<usize>().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::parse_id_list;

    #[test]
    fn parse_id_list_semicolons() {
        assert_eq!(parse_id_list("0;1;2"), vec![0, 1, 2]);
    }

    #[test]
    fn parse_id_list_commas() {
        assert_eq!(parse_id_list("0,1"), vec![0, 1]);
    }

    #[test]
    fn parse_id_list_single() {
        assert_eq!(parse_id_list("3"), vec![3]);
    }

    #[test]
    fn parse_id_list_empty() {
        assert_eq!(parse_id_list(""), Vec::<usize>::new());
    }
}

fn parse_region(s: &str) -> Region {
    match s {
        "North East" => Region::NorthEast,
        "North West" => Region::NorthWest,
        "Yorkshire" => Region::Yorkshire,
        "East Midlands" => Region::EastMidlands,
        "West Midlands" => Region::WestMidlands,
        "East of England" => Region::EastOfEngland,
        "London" => Region::London,
        "South East" => Region::SouthEast,
        "South West" => Region::SouthWest,
        "Wales" => Region::Wales,
        "Scotland" => Region::Scotland,
        "Northern Ireland" => Region::NorthernIreland,
        _ => Region::London,
    }
}

// ── Header-based CSV helpers ──────────────────────────────────────────────

struct HeaderIndex {
    headers: csv::StringRecord,
}

impl HeaderIndex {
    fn new(headers: csv::StringRecord) -> Self {
        Self { headers }
    }

    fn idx(&self, name: &str) -> Option<usize> {
        self.headers.iter().position(|h| h == name)
    }

    fn get_str(&self, r: &csv::StringRecord, name: &str) -> String {
        self.idx(name).and_then(|i| r.get(i)).unwrap_or("").to_string()
    }

    fn get_bool(&self, r: &csv::StringRecord, name: &str) -> bool {
        self.idx(name).map(|i| parse_bool(r.get(i).unwrap_or(""))).unwrap_or(false)
    }

    fn get_bool_default(&self, r: &csv::StringRecord, name: &str, default: bool) -> bool {
        match self.idx(name) {
            Some(i) => {
                let s = r.get(i).unwrap_or("");
                if s.is_empty() { default } else { parse_bool(s) }
            }
            None => default,
        }
    }

    fn get_f64(&self, r: &csv::StringRecord, name: &str) -> f64 {
        self.idx(name).map(|i| parse_f64(r.get(i).unwrap_or(""))).unwrap_or(0.0)
    }

    fn get_f64_default(&self, r: &csv::StringRecord, name: &str, default: f64) -> f64 {
        match self.idx(name) {
            Some(i) => {
                let s = r.get(i).unwrap_or("");
                if s.is_empty() { default } else { parse_f64(s) }
            }
            None => default,
        }
    }

    fn get_i64(&self, r: &csv::StringRecord, name: &str) -> i64 {
        self.idx(name).map(|i| parse_i64(r.get(i).unwrap_or(""))).unwrap_or(0)
    }

    fn get_usize(&self, r: &csv::StringRecord, name: &str) -> usize {
        self.idx(name).map(|i| parse_usize(r.get(i).unwrap_or(""))).unwrap_or(0)
    }
}

// ── Generic reader-based CSV parsers ──────────────────────────────────────

/// Parse persons from any CSV reader. Header-based: missing columns use defaults.
pub fn parse_persons_csv<R: std::io::Read>(reader: R) -> anyhow::Result<Vec<Person>> {
    let mut rdr = csv::Reader::from_reader(reader);
    let h = HeaderIndex::new(rdr.headers()?.clone());
    let mut people = Vec::new();

    for result in rdr.records() {
        let r = result?;
        people.push(Person {
            id: h.get_usize(&r, "person_id"),
            benunit_id: h.get_usize(&r, "benunit_id"),
            household_id: h.get_usize(&r, "household_id"),
            age: h.get_f64_default(&r, "age", 30.0),
            gender: if h.get_str(&r, "gender") == "female" { Gender::Female } else { Gender::Male },
            is_benunit_head: h.get_bool(&r, "is_benunit_head"),
            is_household_head: h.get_bool(&r, "is_household_head"),
            employment_income: h.get_f64(&r, "employment_income"),
            self_employment_income: h.get_f64(&r, "self_employment_income"),
            pension_income: h.get_f64(&r, "private_pension_income"),
            state_pension: h.get_f64(&r, "state_pension"),
            savings_interest_income: h.get_f64(&r, "savings_interest"),
            dividend_income: h.get_f64(&r, "dividend_income"),
            capital_gains: h.get_f64(&r, "capital_gains"),
            capital_gains_residential_share: h.get_f64(&r, "capital_gains_residential_share"),
            property_income: h.get_f64(&r, "property_income"),
            maintenance_income: h.get_f64(&r, "maintenance_income"),
            miscellaneous_income: h.get_f64(&r, "miscellaneous_income"),
            other_income: h.get_f64(&r, "other_income"),
            is_in_scotland: h.get_bool(&r, "is_in_scotland"),
            hours_worked: h.get_f64(&r, "hours_worked_annual"),
            dla_care_low: h.get_bool(&r, "dla_care_low"),
            dla_care_mid: h.get_bool(&r, "dla_care_mid"),
            dla_care_high: h.get_bool(&r, "dla_care_high"),
            dla_mob_low: h.get_bool(&r, "dla_mob_low"),
            dla_mob_high: h.get_bool(&r, "dla_mob_high"),
            pip_dl_std: h.get_bool(&r, "pip_dl_std"),
            pip_dl_enh: h.get_bool(&r, "pip_dl_enh"),
            pip_mob_std: h.get_bool(&r, "pip_mob_std"),
            pip_mob_enh: h.get_bool(&r, "pip_mob_enh"),
            aa_low: h.get_bool(&r, "aa_low"),
            aa_high: h.get_bool(&r, "aa_high"),
            is_disabled: h.get_bool(&r, "is_disabled"),
            is_enhanced_disabled: h.get_bool(&r, "is_enhanced_disabled"),
            is_severely_disabled: h.get_bool(&r, "is_severely_disabled"),
            is_carer: h.get_bool(&r, "is_carer"),
            limitill: h.get_bool(&r, "limitill"),
            esa_group: h.get_i64(&r, "esa_group"),
            emp_status: h.get_i64(&r, "emp_status"),
            looking_for_work: h.get_bool(&r, "looking_for_work"),
            is_self_identified_carer: h.get_bool(&r, "is_self_identified_carer"),
            employee_pension_contributions: h.get_f64(&r, "employee_pension_contributions"),
            personal_pension_contributions: h.get_f64(&r, "personal_pension_contributions"),
            childcare_expenses: h.get_f64(&r, "childcare_expenses"),
            child_benefit: h.get_f64(&r, "child_benefit"),
            housing_benefit: h.get_f64(&r, "housing_benefit"),
            income_support: h.get_f64(&r, "income_support"),
            pension_credit: h.get_f64(&r, "pension_credit"),
            child_tax_credit: h.get_f64(&r, "child_tax_credit"),
            working_tax_credit: h.get_f64(&r, "working_tax_credit"),
            universal_credit: h.get_f64(&r, "universal_credit"),
            dla_care: h.get_f64(&r, "dla_care"),
            dla_mobility: h.get_f64(&r, "dla_mobility"),
            pip_daily_living: h.get_f64(&r, "pip_daily_living"),
            pip_mobility: h.get_f64(&r, "pip_mobility"),
            carers_allowance: h.get_f64(&r, "carers_allowance"),
            attendance_allowance: h.get_f64(&r, "attendance_allowance"),
            esa_income: h.get_f64(&r, "esa_income"),
            esa_contributory: h.get_f64(&r, "esa_contributory"),
            jsa_income: h.get_f64(&r, "jsa_income"),
            jsa_contributory: h.get_f64(&r, "jsa_contributory"),
            other_benefits: h.get_f64(&r, "other_benefits"),
            adp_daily_living: h.get_f64(&r, "adp_daily_living"),
            adp_mobility: h.get_f64(&r, "adp_mobility"),
            cdp_care: h.get_f64(&r, "cdp_care"),
            cdp_mobility: h.get_f64(&r, "cdp_mobility"),
        });
    }

    Ok(people)
}

/// Parse benefit units from any CSV reader. Header-based: missing columns use defaults.
pub fn parse_benunits_csv<R: std::io::Read>(reader: R) -> anyhow::Result<Vec<BenUnit>> {
    let mut rdr = csv::Reader::from_reader(reader);
    let h = HeaderIndex::new(rdr.headers()?.clone());

    let mut benunits = Vec::new();
    for result in rdr.records() {
        let r = result?;
        let on_uc = h.get_bool(&r, "on_uc");
        benunits.push(BenUnit {
            id: h.get_usize(&r, "benunit_id"),
            household_id: h.get_usize(&r, "household_id"),
            person_ids: parse_id_list(&h.get_str(&r, "person_ids")),
            on_uc,
            rent_monthly: h.get_f64(&r, "rent_monthly"),
            is_lone_parent: h.get_bool(&r, "is_lone_parent"),
            // Survey records gate take-up on reported receipt: absent the column,
            // fall back to the FRS-reported claim status (on_uc) rather than the
            // hypothetical-household default (true), so clean data behaves as before.
            claims_uc_if_eligible: h.get_bool_default(&r, "claims_uc_if_eligible", on_uc),
            ..BenUnit::default()
        });
    }

    Ok(benunits)
}

/// Parse households from any CSV reader. Header-based: missing columns use defaults.
pub fn parse_households_csv<R: std::io::Read>(reader: R) -> anyhow::Result<Vec<Household>> {
    let mut rdr = csv::Reader::from_reader(reader);
    let h = HeaderIndex::new(rdr.headers()?.clone());
    let mut households = Vec::new();

    for result in rdr.records() {
        let r = result?;
        households.push(Household {
            id: h.get_usize(&r, "household_id"),
            benunit_ids: parse_id_list(&h.get_str(&r, "benunit_ids")),
            person_ids: parse_id_list(&h.get_str(&r, "person_ids")),
            weight: h.get_f64_default(&r, "weight", 1.0),
            region: parse_region(&h.get_str(&r, "region")),
            rent: h.get_f64(&r, "rent_annual"),
            council_tax: h.get_f64(&r, "council_tax_annual"),
            council_tax_band: h.get_i64(&r, "council_tax_band").clamp(0, 8) as u8,
            // Auxiliary
            num_bedrooms: h.get_usize(&r, "num_bedrooms") as u32,
            tenure_type: TenureType::from_rf_code(h.get_i64(&r, "tenure_type") as i32),
            accommodation_type: AccommodationType::from_frs_code(h.get_i64(&r, "accommodation_type") as i32),
            // Wealth
            owned_land: h.get_f64(&r, "owned_land"),
            property_wealth: h.get_f64(&r, "property_wealth"),
            corporate_wealth: h.get_f64(&r, "corporate_wealth"),
            gross_financial_wealth: h.get_f64(&r, "gross_financial_wealth"),
            net_financial_wealth: h.get_f64(&r, "net_financial_wealth"),
            main_residence_value: h.get_f64(&r, "main_residence_value"),
            other_residential_property_value: h.get_f64(&r, "other_residential_property_value"),
            non_residential_property_value: h.get_f64(&r, "non_residential_property_value"),
            savings: h.get_f64(&r, "savings"),
            num_vehicles: h.get_f64(&r, "num_vehicles"),
            // Consumption
            food_consumption: h.get_f64(&r, "food_consumption"),
            alcohol_consumption: h.get_f64(&r, "alcohol_consumption"),
            tobacco_consumption: h.get_f64(&r, "tobacco_consumption"),
            clothing_consumption: h.get_f64(&r, "clothing_consumption"),
            housing_water_electricity_consumption: h.get_f64(&r, "housing_water_electricity_consumption"),
            furnishings_consumption: h.get_f64(&r, "furnishings_consumption"),
            health_consumption: h.get_f64(&r, "health_consumption"),
            transport_consumption: h.get_f64(&r, "transport_consumption"),
            communication_consumption: h.get_f64(&r, "communication_consumption"),
            recreation_consumption: h.get_f64(&r, "recreation_consumption"),
            education_consumption: h.get_f64(&r, "education_consumption"),
            restaurants_consumption: h.get_f64(&r, "restaurants_consumption"),
            miscellaneous_consumption: h.get_f64(&r, "miscellaneous_consumption"),
            petrol_spending: h.get_f64(&r, "petrol_spending"),
            diesel_spending: h.get_f64(&r, "diesel_spending"),
            domestic_energy_consumption: h.get_f64(&r, "domestic_energy_consumption"),
            electricity_consumption: h.get_f64(&r, "electricity_consumption"),
            gas_consumption: h.get_f64(&r, "gas_consumption"),
        });
    }

    Ok(households)
}
