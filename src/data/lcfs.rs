use std::collections::HashMap;
use std::path::Path;
use crate::engine::entities::*;
use crate::data::Dataset;
use crate::data::frs::{load_table_cols, get_f64, get_i64, region_from_gvtregno};

const WEEKS_IN_YEAR: f64 = 365.25 / 7.0;

/// Parse Living Costs and Food Survey (LCFS) microdata from UKDS tab-delimited files.
///
/// The LCFS is a household + person survey covering consumption and expenditure.
/// Person-level income data is limited but sufficient for basic tax-benefit simulation.
///
/// Expected directory structure:
///   data_dir/lcfs_{YYYY}_dvhh_ukanon*.tab   (household derived variables)
///   data_dir/lcfs_{YYYY}_dvper_ukanon*.tab   (person derived variables)
///
/// LCFS income values are WEEKLY — we annualise by multiplying by WEEKS_IN_YEAR.
pub fn load_lcfs(data_dir: &Path, fiscal_year: u32) -> anyhow::Result<Dataset> {
    let (hh_file, person_file) = find_lcfs_files(data_dir, fiscal_year)?;

    let hh_cols: &[&str] = &[
        "case", "gorx", "weighta",
        "p389p", "p344p",  // total expenditure, total income
        "g018", "g019",    // num adults, num children
        "a122",            // tenure type (1=own outright,2=mortgage,3=social rent,4=private rent)
        "a121",            // weekly rent gross (0 for non-renters)
        "p055p",           // total weekly benefit income (household)
        // COICOP top-level consumption categories (weekly £)
        "p601", "p602", "p603", "p604", "p605", "p606",
        "p607", "p608", "p609", "p610", "p611", "p612",
        "c72211", "c72212",  // petrol, diesel
        "c021", "c022",      // alcohol subtotal, tobacco subtotal (COICOP 02.1, 02.2)
    ];

    let hh_table = load_table_cols(data_dir, &hh_file, Some(hh_cols))?;

    let person_table = load_table_cols(data_dir, &person_file, Some(&[
        "case", "person",
        "a003", "a004", "a002",  // age (two variants), sex
        "wkgrossp",              // weekly gross pay (employee, well-populated)
        "p047p", "b3262p",       // SE income: main job, subsidiary job
        "p048p",                 // investment income (weekly)
        "b3381", "p049p",        // state pension, private pension income
    ]))?;

    // Group persons by household case number
    let mut persons_by_case: HashMap<i64, Vec<&HashMap<String, String>>> = HashMap::new();
    for row in &person_table {
        let case = get_i64(row, "case");
        persons_by_case.entry(case).or_default().push(row);
    }

    // weighta is a design weight summing to roughly the sample size (~28,000-30,000).
    // Rescale to UK household population (~28.3m) so that weighted sums are population totals.
    let weighta_sum: f64 = hh_table.iter().map(|r| get_f64(r, "weighta").max(0.0)).sum();
    const UK_HOUSEHOLDS: f64 = 28_300_000.0;
    let weight_scale = if weighta_sum > 0.0 { UK_HOUSEHOLDS / weighta_sum } else { 1.0 };

    let mut people = Vec::new();
    let mut benunits = Vec::new();
    let mut households = Vec::new();

    for hh_row in &hh_table {
        let case = get_i64(hh_row, "case");
        let weight = get_f64(hh_row, "weighta") * weight_scale;
        if weight <= 0.0 { continue; }

        let region = region_from_gvtregno(get_i64(hh_row, "gorx"));
        let hh_id = households.len();
        let bu_id = benunits.len();

        // Rent: a121 is weekly gross rent; non-renters have 0 or missing
        let rent_weekly = get_f64(hh_row, "a121").max(0.0);
        let rent_annual = rent_weekly * WEEKS_IN_YEAR;
        let rent_monthly = rent_annual / 12.0;

        // Total benefit income (weekly) — use as passthrough on household head
        let benefits_annual = get_f64(hh_row, "p055p").max(0.0) * WEEKS_IN_YEAR;

        // Derive lone parent from household composition
        let num_adults_hh = get_i64(hh_row, "g018").max(0) as usize;
        let num_children_hh = get_i64(hh_row, "g019").max(0) as usize;
        let is_lone_parent = num_adults_hh == 1 && num_children_hh > 0;

        let mut hh_person_ids = Vec::new();

        if let Some(person_rows) = persons_by_case.get(&case) {
            for (i, prow) in person_rows.iter().enumerate() {
                let pid = people.len();
                hh_person_ids.push(pid);

                let age = {
                    let a = get_f64(prow, "a004");
                    if a > 0.0 { a } else { get_f64(prow, "a003") }
                };

                let is_head = i == 0;
                let person = Person {
                    id: pid,
                    benunit_id: bu_id,
                    household_id: hh_id,
                    age,
                    gender: if get_i64(prow, "a002") == 1 { Gender::Male } else { Gender::Female },
                    is_benunit_head: is_head,
                    is_household_head: is_head,
                    is_in_scotland: region.is_scotland(),
                    employment_income: get_f64(prow, "wkgrossp").max(0.0) * WEEKS_IN_YEAR,
                    self_employment_income: (get_f64(prow, "p047p") + get_f64(prow, "b3262p")).max(0.0) * WEEKS_IN_YEAR,
                    savings_interest_income: get_f64(prow, "p048p").max(0.0) * WEEKS_IN_YEAR,
                    state_pension: get_f64(prow, "b3381").max(0.0) * WEEKS_IN_YEAR,
                    pension_income: get_f64(prow, "p049p").max(0.0) * WEEKS_IN_YEAR,
                    // Allocate total household benefit income to head as passthrough
                    other_benefits: if is_head { benefits_annual } else { 0.0 },
                    ..Person::default()
                };
                people.push(person);
            }
        } else {
            // No person records — create synthetic persons from household counts
            let num_adults = get_i64(hh_row, "g018").max(1) as usize;
            let num_children = get_i64(hh_row, "g019").max(0) as usize;

            for i in 0..(num_adults + num_children) {
                let pid = people.len();
                hh_person_ids.push(pid);
                let is_child = i >= num_adults;
                let is_head = i == 0;

                let person = Person {
                    id: pid,
                    benunit_id: bu_id,
                    household_id: hh_id,
                    age: if is_child { 8.0 } else { 40.0 },
                    gender: Gender::Male,
                    is_benunit_head: is_head,
                    is_household_head: is_head,
                    is_in_scotland: region.is_scotland(),
                    other_benefits: if is_head { benefits_annual } else { 0.0 },
                    ..Person::default()
                };
                people.push(person);
            }
        }

        if hh_person_ids.is_empty() {
            let pid = people.len();
            hh_person_ids.push(pid);
            people.push(Person {
                id: pid,
                benunit_id: bu_id,
                household_id: hh_id,
                age: 40.0,
                gender: Gender::Male,
                is_benunit_head: true,
                is_household_head: true,
                is_in_scotland: region.is_scotland(),
                ..Person::default()
            });
        }

        // LCFS total benefit income flows through as other_benefits passthrough on the head.
        // Benefit simulation stays off (no reported per-benefit receipt, so nothing
        // is claimed) to avoid overcounting; use --full-take-up to simulate benefits.
        benunits.push(BenUnit {
            id: bu_id,
            household_id: hh_id,
            person_ids: hh_person_ids.clone(),
            rent_monthly,
            is_lone_parent,
            ..BenUnit::default()
        });

        households.push(Household {
            id: hh_id,
            benunit_ids: vec![bu_id],
            person_ids: hh_person_ids,
            weight,
            region,
            rent: rent_annual,
            food_consumption:                          get_f64(hh_row, "p601").max(0.0) * WEEKS_IN_YEAR,
            alcohol_consumption: {
                // Try COICOP subcodes first; fall back to 70/30 split of p602 (ONS avg)
                let c021 = get_f64(hh_row, "c021");
                let p602 = get_f64(hh_row, "p602").max(0.0);
                if c021 > 0.0 { c021.max(0.0) * WEEKS_IN_YEAR } else { p602 * 0.70 * WEEKS_IN_YEAR }
            },
            tobacco_consumption: {
                let c022 = get_f64(hh_row, "c022");
                let p602 = get_f64(hh_row, "p602").max(0.0);
                if c022 > 0.0 { c022.max(0.0) * WEEKS_IN_YEAR } else { p602 * 0.30 * WEEKS_IN_YEAR }
            },
            clothing_consumption:                      get_f64(hh_row, "p603").max(0.0) * WEEKS_IN_YEAR,
            housing_water_electricity_consumption:     get_f64(hh_row, "p604").max(0.0) * WEEKS_IN_YEAR,
            furnishings_consumption:                   get_f64(hh_row, "p605").max(0.0) * WEEKS_IN_YEAR,
            health_consumption:                        get_f64(hh_row, "p606").max(0.0) * WEEKS_IN_YEAR,
            transport_consumption:                     get_f64(hh_row, "p607").max(0.0) * WEEKS_IN_YEAR,
            communication_consumption:                 get_f64(hh_row, "p608").max(0.0) * WEEKS_IN_YEAR,
            recreation_consumption:                    get_f64(hh_row, "p609").max(0.0) * WEEKS_IN_YEAR,
            education_consumption:                     get_f64(hh_row, "p610").max(0.0) * WEEKS_IN_YEAR,
            restaurants_consumption:                   get_f64(hh_row, "p611").max(0.0) * WEEKS_IN_YEAR,
            miscellaneous_consumption:                 get_f64(hh_row, "p612").max(0.0) * WEEKS_IN_YEAR,
            petrol_spending:                           get_f64(hh_row, "c72211").max(0.0) * WEEKS_IN_YEAR,
            diesel_spending:                           get_f64(hh_row, "c72212").max(0.0) * WEEKS_IN_YEAR,
            ..Household::default()
        });
    }

    Ok(Dataset {
        people,
        benunits,
        households,
        name: format!("Living Costs and Food Survey {}/{:02}", fiscal_year, (fiscal_year + 1) % 100),
        year: fiscal_year,
    })
}

/// Find LCFS tab file names in the directory.
fn find_lcfs_files(data_dir: &Path, fiscal_year: u32) -> anyhow::Result<(String, String)> {
    let mut hh_file = None;
    let mut person_file = None;

    let entries = std::fs::read_dir(data_dir)?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_lowercase();

        if (name.contains("dvhh") || name.contains("dv_hh")) && (name.ends_with(".tab") || name.ends_with(".csv")) {
            let stem = name.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or(name.clone());
            hh_file = Some(stem);
        }
        if (name.contains("dvper") || name.contains("dv_per")) && (name.ends_with(".tab") || name.ends_with(".csv")) {
            let stem = name.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or(name.clone());
            person_file = Some(stem);
        }
    }

    let hh = hh_file.ok_or_else(|| anyhow::anyhow!(
        "No LCFS household file (dvhh*.tab) found in {:?} for {}/{}",
        data_dir, fiscal_year, (fiscal_year + 1) % 100
    ))?;
    let per = person_file.ok_or_else(|| anyhow::anyhow!(
        "No LCFS person file (dvper*.tab) found in {:?} for {}/{}",
        data_dir, fiscal_year, (fiscal_year + 1) % 100
    ))?;

    Ok((hh, per))
}
