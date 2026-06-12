use std::path::Path;
use crate::engine::entities::*;
use crate::data::Dataset;
use crate::data::frs::{load_table_cols, get_f64, get_i64};

/// Parse Wealth and Assets Survey (WAS) microdata from UKDS tab-delimited files.
///
/// WAS is a household-level survey focused on wealth, savings, and assets.
/// Income and wealth data are household aggregates allocated to the household head.
/// Individual-level ages are not available in the EUL version — synthetic ages are used.
///
/// WAS does not sample Northern Ireland; NI region codes are mapped to Wales.
///
/// Column naming varies by round:
///   Round 5-8: NumAdultR{N}, NumCh18R{N}, DVCTaxAmtAnnualR{N}, etc.
///   Earlier waves: numadultw{N}, numch18w{N}, ctamtw{N} etc.
/// The loader detects which convention applies.
///
/// Expected directory: contains a file matching `was_round_*_hhold_*.tab` or `was_wave_*_hhold_*.tab`.
pub fn load_was(data_dir: &Path, fiscal_year: u32) -> anyhow::Result<Dataset> {
    let file_name = find_was_file(data_dir)?;
    let round = detect_round(&file_name);
    let is_round = file_name.contains("round");  // round = R5+, wave = W1-4

    // All column names must be lowercase — load_table_cols normalises headers to lowercase
    // and the needed-columns filter uses exact string matching on the lowercased headers.
    let weight_col      = format!("r{}xshhwgt", round);
    let region_col      = format!("gorr{}", round);

    // Household size columns differ between rounds and waves (lowercased)
    let adults_col   = if is_round { format!("numadultr{}", round) }
                       else        { format!("numadultw{}", round) };
    let children_col = if is_round { format!("numch18r{}", round) }
                       else        { format!("numch18w{}", round) };

    // Income columns (all lowercase after load_table_cols normalises headers)
    let emp_income_col    = format!("dvgiempr{}_aggr", round);
    let se_income_col     = format!("dvgiser{}_aggr", round);
    let gross_pension_col = format!("dvgippenr{}_aggr", round);   // gross private pension income
    let state_pen_col     = format!("dvnippenr{}_aggr", round);   // NI/state pension only (DVNIPPenR{N}_aggr)
    let invest_income_col = format!("dvgiinvr{}_aggr", round);
    let other_income_col  = format!("dvgiothr{}_aggr", round);
    let benefits_col      = format!("dvbenefitannualr{}_aggr", round); // total benefits (annual)

    // Council tax (lowercased)
    let council_tax_col = if is_round { format!("dvctaxamtannualr{}", round) }
                          else        { format!("ctamtw{}", round) };

    // Rent paid annually (rounds 5+: dvrentpaidr{N}; waves: rentpaidw{N})
    // Value is -9 for non-renters (owners/mortgage) → treat as 0
    let rent_col = if is_round { format!("dvrentpaidr{}", round) }
                   else        { format!("rentpaidw{}", round) };

    // Wealth columns (rounds 5+; absent in early waves)
    let fin_wealth_col  = format!("hfinwr{}_sum", round);
    let prop_wealth_col = format!("hpropwr{}", round);
    let phys_wealth_col = format!("hphyswr{}", round);
    let tot_wealth_col  = format!("totwlth_oldr{}", round);

    let needed: Vec<&str> = vec![
        &weight_col, &region_col, &adults_col, &children_col,
        &emp_income_col, &se_income_col, &gross_pension_col, &state_pen_col,
        &invest_income_col, &other_income_col, &benefits_col,
        &council_tax_col, &rent_col,
        &fin_wealth_col, &prop_wealth_col, &phys_wealth_col, &tot_wealth_col,
    ];

    let table = load_table_cols(data_dir, &file_name, Some(&needed))?;

    let mut people = Vec::new();
    let mut benunits = Vec::new();
    let mut households = Vec::new();

    for row in &table {
        let weight = get_f64(row, &weight_col);
        if weight <= 0.0 { continue; }

        let region_code = get_i64(row, &region_col);
        let region = was_region(region_code);

        let num_adults   = get_i64(row, &adults_col).max(1) as usize;
        let num_children = get_i64(row, &children_col).max(0) as usize;

        // Income (annual in WAS)
        let employment_income      = get_f64(row, &emp_income_col).max(0.0);
        let self_employment_income = get_f64(row, &se_income_col).max(0.0);
        // DVGIPPenR{N}_aggr = gross private pension income (excl. state pension)
        // DVNIPPenR{N}_aggr = NI/state pension income
        let private_pension        = get_f64(row, &gross_pension_col).max(0.0);
        let state_pension          = get_f64(row, &state_pen_col).max(0.0);
        let investment_income      = get_f64(row, &invest_income_col).max(0.0);
        let other_income           = get_f64(row, &other_income_col).max(0.0);
        // Total benefits: used as passthrough — UC/HB etc. not broken out in EUL
        let total_benefits         = get_f64(row, &benefits_col).max(0.0);

        let council_tax  = get_f64(row, &council_tax_col).max(0.0);

        // Rent paid (annual; -9 = not applicable for owners/mortgagors)
        let rent_annual_raw = get_f64(row, &rent_col);
        let rent_annual = if rent_annual_raw < 0.0 { 0.0 } else { rent_annual_raw };
        let rent_monthly = rent_annual / 12.0;

        // Lone parent: 1 adult with at least 1 child
        let is_lone_parent = num_adults == 1 && num_children > 0;

        // Wealth (zero if column absent in older waves)
        let financial_wealth = get_f64(row, &fin_wealth_col);
        let property_wealth  = get_f64(row, &prop_wealth_col);
        let _physical_wealth  = get_f64(row, &phys_wealth_col);
        let _total_wealth     = get_f64(row, &tot_wealth_col);

        let hh_id = households.len();
        let bu_id = benunits.len();
        let mut hh_person_ids = Vec::new();

        // Allocate all income to the household head; other adults have zero income
        for i in 0..num_adults {
            let pid = people.len();
            hh_person_ids.push(pid);
            let is_head = i == 0;

            let person = Person {
                id: pid,
                benunit_id: bu_id,
                household_id: hh_id,
                age: 40.0,
                gender: if i % 2 == 0 { Gender::Male } else { Gender::Female },
                is_benunit_head: is_head,
                is_household_head: is_head,
                is_in_scotland: region.is_scotland(),
                employment_income:      if is_head { employment_income } else { 0.0 },
                self_employment_income: if is_head { self_employment_income } else { 0.0 },
                pension_income:         if is_head { private_pension } else { 0.0 },
                state_pension:          if is_head { state_pension } else { 0.0 },
                savings_interest_income: if is_head { investment_income } else { 0.0 },
                other_income:           if is_head { other_income } else { 0.0 },
                other_benefits:         if is_head { total_benefits } else { 0.0 },
                ..Person::default()
            };
            people.push(person);
        }

        for _ in 0..num_children {
            let pid = people.len();
            hh_person_ids.push(pid);
            people.push(Person {
                id: pid,
                benunit_id: bu_id,
                household_id: hh_id,
                age: 8.0,
                gender: Gender::Male,
                is_in_scotland: region.is_scotland(),
                ..Person::default()
            });
        }

        // WAS total benefits flow through as other_benefits passthrough on the head.
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
            council_tax,
            net_financial_wealth: financial_wealth,
            property_wealth,
            // WAS records total property wealth but not main residence separately;
            // use property_wealth as a proxy for stamp duty reform modelling.
            main_residence_value: property_wealth,
            ..Household::default()
        });
    }

    Ok(Dataset {
        people,
        benunits,
        households,
        name: format!("Wealth and Assets Survey Round {} ({}/{})", round, fiscal_year, (fiscal_year + 1) % 100),
        year: fiscal_year,
    })
}

fn find_was_file(data_dir: &Path) -> anyhow::Result<String> {
    let entries = std::fs::read_dir(data_dir)?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.contains("hhold") && (name.ends_with(".tab") || name.ends_with(".csv")) {
            // Prefer the most recent round
            let stem = name.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or(name);
            return Ok(stem);
        }
    }
    anyhow::bail!("No WAS household file (*hhold*.tab) found in {:?}", data_dir)
}

fn detect_round(file_name: &str) -> u32 {
    let lower = file_name.to_lowercase();
    // "round_N" or "wave_N"
    for prefix in &["round_", "wave_"] {
        if let Some(pos) = lower.find(prefix) {
            let after = &lower[pos + prefix.len()..];
            if let Some(digit) = after.chars().next().and_then(|c| c.to_digit(10)) {
                return digit;
            }
        }
    }
    // "_rN_" fallback
    for r in (1..=9).rev() {
        if lower.contains(&format!("r{}_", r)) || lower.contains(&format!("w{}_", r)) {
            return r;
        }
    }
    8
}

fn was_region(code: i64) -> Region {
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
        13 => Region::Wales,  // NI not sampled
        _ => Region::London,
    }
}
