use std::collections::HashMap;
use std::path::Path;
use crate::engine::entities::*;
use crate::data::Dataset;

const WEEKS_IN_YEAR: f64 = 365.25 / 7.0;

/// Parse real FRS microdata from UKDS tab-delimited files.
///
/// Expected directory structure (tab-delimited, as distributed by UKDS):
///   data_dir/adult.tab
///   data_dir/child.tab
///   data_dir/househol.tab
///   data_dir/benunit.tab
///   data_dir/accounts.tab
///   data_dir/benefits.tab
///   data_dir/job.tab
///   data_dir/pension.tab
///   data_dir/penprov.tab
///
/// Also supports .csv extension as fallback.
///
/// FRS income variables are WEEKLY — we annualise by multiplying by WEEKS_IN_YEAR.
/// Load FRS data for a given fiscal year. `fiscal_year` is the start year (e.g. 2023 for 2023/24).
/// Variable names are resolved explicitly based on the FRS era for that year.
pub fn load_frs(data_dir: &Path, fiscal_year: u32) -> anyhow::Result<Dataset> {
    let era = era_for_year(fiscal_year);

    // Load tables with only the columns we need (raw FRS has 400+ cols per table)
    let household_table = load_table_cols(data_dir, "househol", Some(&[
        "sernum", "gross3", "gross4", "stdregn", "gvtregn", "gvtregno",
        "ctannual", "hhrent", "subrent", "cvpay",
        "bedroom6", "tentyp2", "typeacc",
    ]))?;
    let benunit_table = load_table_cols(data_dir, "benunit", Some(&[
        "sernum", "benunit", "buuc", "burent",
        "fsmbu", "fsfvbu", "fsmlkbu", "heartbu", "butvlic",
    ]))?;
    let adult_table = load_table_cols(data_dir, "adult", Some(&[
        "sernum", "benunit", "person", "sex", "age", "age80", "tothours",
        "uperson", "hrpid", "limitill", "esagrp", "empstatb", "lookwk", "carer1",
        "inearns", "seincam2", "inseinc", "inpeninc", "royyr1", "dividgro",
        "mntus1", "mntus2", "mntusam1", "mntusam2", "mntamt1", "mntamt2",
        "allow1", "allow2", "allow3", "allow4",
        "allpay1", "allpay2", "allpay3", "allpay4",
        "apamt", "apdamt", "pareamt", "aliamt",
    ]))?;
    let child_table = load_table_cols(data_dir, "child", Some(&[
        "sernum", "benunit", "person", "sex", "age", "chearns", "chrinc",
    ]))?;

    // Optional tables
    let accounts_table = load_table_cols(data_dir, "accounts", Some(&[
        "sernum", "person", "accint", "account", "acctax", "invtax",
    ])).ok();
    let benefits_table = load_table_cols(data_dir, "benefits", Some(&[
        "sernum", "person", "benefit", "benamt", "benpd", "var2",
    ])).ok();
    let job_table = load_table_cols(data_dir, "job", Some(&[
        "sernum", "person", "deduc1",
    ])).ok();
    let pension_table = load_table_cols(data_dir, "pension", Some(&[
        "sernum", "person", "penpay", "ptamt", "ptinc", "poamt", "poinc", "penoth",
    ])).ok();
    let penprov_table = load_table_cols(data_dir, "penprov", Some(&[
        "sernum", "person", "stemppen", "stemppay", "penamt", "penamtpd",
    ])).ok();
    let oddjob_table = load_table_cols(data_dir, "oddjob", Some(&[
        "sernum", "person", "ojamt",
    ])).ok();

    // Phase 1: Build household records
    let hh_data = parse_households(&household_table, era);

    // Phase 2: Build benefit unit records
    let bu_data = parse_benunits(&benunit_table, era);

    // Phase 3: Build person-level aggregates from sub-tables
    let account_agg = accounts_table.as_ref().map(|t| aggregate_accounts(t));
    let benefit_agg = benefits_table.as_ref().map(|t| aggregate_benefits(t));
    let job_agg = job_table.as_ref().map(|t| aggregate_jobs(t));
    let pension_agg = pension_table.as_ref().map(|t| aggregate_pensions(t));
    let penprov_agg = penprov_table.as_ref().map(|t| aggregate_penprov(t));
    let oddjob_agg = oddjob_table.as_ref().map(|t| aggregate_oddjobs(t));

    // Build HH-level property income map (subrent + cvpay, assigned to HRP in parse_adults)
    let hh_property_map: HashMap<i64, f64> = hh_data.iter()
        .map(|hh| (hh.sernum, hh.subrent_weekly + hh.cvpay_weekly))
        .collect();

    // Phase 4: Build adult records
    let adult_records = parse_adults(&adult_table, &account_agg, &benefit_agg, &job_agg, &pension_agg, &penprov_agg, &oddjob_agg, &hh_property_map, era);

    // Phase 5: Build child records
    let child_records = parse_children(&child_table);

    // Phase 6: Assemble into entity hierarchy
    assemble_dataset(hh_data, bu_data, adult_records, child_records)
}

// ── Table loading ────────────────────────────────────────────────────────

pub(crate) type Table = Vec<HashMap<String, String>>;

pub(crate) fn load_table_cols(data_dir: &Path, name: &str, needed: Option<&[&str]>) -> anyhow::Result<Table> {
    let tab_path = data_dir.join(format!("{}.tab", name));
    let csv_path = data_dir.join(format!("{}.csv", name));

    let (path, delimiter) = if tab_path.exists() {
        (tab_path, b'\t')
    } else if csv_path.exists() {
        (csv_path, b',')
    } else {
        anyhow::bail!("Neither {}.tab nor {}.csv found in {:?}", name, name, data_dir);
    };

    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .from_path(&path)?;

    let headers: Vec<String> = rdr.headers()?.iter().map(|h| h.to_lowercase()).collect();

    // Build index of which columns to keep
    let keep: Vec<bool> = match needed {
        Some(cols) => headers.iter().map(|h| cols.contains(&h.as_str())).collect(),
        None => vec![true; headers.len()],
    };

    let mut table = Vec::new();
    for result in rdr.records() {
        let record = result?;
        let row: HashMap<String, String> = headers.iter()
            .zip(record.iter())
            .zip(keep.iter())
            .filter(|(_, &k)| k)
            .map(|((h, v), _)| (h.clone(), v.to_string()))
            .collect();
        table.push(row);
    }

    Ok(table)
}

pub(crate) fn get_f64(row: &HashMap<String, String>, key: &str) -> f64 {
    row.get(key)
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
}

pub(crate) fn get_i64(row: &HashMap<String, String>, key: &str) -> i64 {
    row.get(key)
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(0)
}

pub(crate) fn get_positive_f64(row: &HashMap<String, String>, key: &str) -> f64 {
    get_f64(row, key).max(0.0)
}

/// FRS variable naming era, determined by fiscal year.
/// Variable names and available columns changed across FRS releases.
///
/// Verified against actual UKDS tab files for each year:
///   - Weight: GROSS3 (1994–2001), GROSS4 (2002+)
///   - Region: STDREGN (1994–2001), GVTREGN (2002+)
///   - Age:    AGE only (1994–2001), AGE80 (2002+)
///   - HRP:    UPERSON only (1994–2001), HRPID (2002+)
///   - ESA:    ESAGRP (2008+)
///   - UC:     BUUC (2022+)
#[derive(Debug, Clone, Copy, PartialEq)]
enum FrsEra {
    /// 1994/95–2001/02: STDREGN region, GROSS3 weight, AGE (not top-coded),
    /// no HRPID, no ESA, no UC
    Early,     // fiscal year 1994–2001
    /// 2002/03–2007/08: GVTREGN region, GROSS4 weight, AGE80, HRPID, no ESA
    Mid,       // fiscal year 2002–2007
    /// 2008/09–2021/22: as Mid plus ESAGRP, LIMITILL, CARER1
    Late,      // fiscal year 2008–2021
    /// 2022/23+: as Late plus BUUC, GVTREGNO
    Current,   // fiscal year 2022+
}

fn era_for_year(fiscal_year: u32) -> FrsEra {
    match fiscal_year {
        ..=2001 => FrsEra::Early,
        2002..=2007 => FrsEra::Mid,
        2008..=2021 => FrsEra::Late,
        _ => FrsEra::Current,
    }
}

/// Region from STDREGN coding (1994/95–2001/02 FRS).
fn region_from_stdregn(code: i64) -> Region {
    match code {
        1 => Region::NorthEast,     // North
        2 => Region::Yorkshire,     // Yorkshire & Humberside
        3 => Region::NorthWest,     // North West / Merseyside
        4 => Region::EastMidlands,
        5 => Region::WestMidlands,
        6 => Region::EastOfEngland, // East Anglia / Eastern
        7 => Region::London,        // Greater London / Inner London
        8 => Region::SouthEast,     // South East / Outer London
        9 => Region::SouthWest,
        10 => Region::Wales,
        11 => Region::Scotland,
        12 => Region::NorthernIreland,
        _ => Region::London,
    }
}

/// Region from GVTREGN coding (2002/03–2021/22 FRS).
/// Same numeric coding as GVTREGNO but column name differs.
fn region_from_gvtregn(code: i64) -> Region {
    region_from_gvtregno(code)
}

// ── Household parsing ────────────────────────────────────────────────────

struct HouseholdRecord {
    sernum: i64,
    weight: f64,
    region: Region,
    rent_weekly: f64,
    council_tax_annual: f64,
    /// Sub-tenant rent received (SUBRENT, weekly) — assigned to HRP
    subrent_weekly: f64,
    /// Boarders/lodgers income net of HB (CVPAY, weekly) — assigned to HRP
    cvpay_weekly: f64,
    // Housing characteristics for EFRS imputation
    num_bedrooms: u32,
    tenure_type: TenureType,
    accommodation_type: AccommodationType,
}

pub(crate) fn region_from_gvtregno(code: i64) -> Region {
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

fn parse_households(table: &Table, era: FrsEra) -> Vec<HouseholdRecord> {
    table.iter().map(|row| {
        let ct = get_f64(row, "ctannual");

        let weight = match era {
            FrsEra::Early => get_f64(row, "gross3"),
            FrsEra::Mid | FrsEra::Late | FrsEra::Current => get_f64(row, "gross4"),
        };

        let region = match era {
            FrsEra::Early => region_from_stdregn(get_i64(row, "stdregn")),
            FrsEra::Mid | FrsEra::Late => region_from_gvtregn(get_i64(row, "gvtregn")),
            FrsEra::Current => region_from_gvtregno(get_i64(row, "gvtregno")),
        };

        HouseholdRecord {
            sernum: get_i64(row, "sernum"),
            weight,
            region,
            rent_weekly: get_positive_f64(row, "hhrent"),
            council_tax_annual: if ct > 0.0 { ct } else { 1800.0 },
            subrent_weekly: get_positive_f64(row, "subrent"),
            cvpay_weekly: get_positive_f64(row, "cvpay"),
            num_bedrooms: get_i64(row, "bedroom6").max(0) as u32,
            tenure_type: TenureType::from_frs_code(get_i64(row, "tentyp2") as i32),
            accommodation_type: AccommodationType::from_frs_code(get_i64(row, "typeacc") as i32),
        }
    }).collect()
}

// ── Benefit unit parsing ─────────────────────────────────────────────────

#[allow(dead_code)]
struct BenUnitRecord {
    sernum: i64,
    benunit: i64,
    claims_uc: bool,
    rent_weekly: f64,
    // In-kind benefits (weekly DVs from FRS benunit table, for HBAI net income)
    free_school_meals_weekly: f64,
    free_school_fruit_veg_weekly: f64,
    free_school_milk_weekly: f64,
    healthy_start_vouchers_weekly: f64,
    free_tv_licence_weekly: f64,
}

fn parse_benunits(table: &Table, era: FrsEra) -> Vec<BenUnitRecord> {
    table.iter().map(|row| {
        BenUnitRecord {
            sernum: get_i64(row, "sernum"),
            benunit: get_i64(row, "benunit"),
            // BUUC only exists from ~2013 when UC was introduced
            claims_uc: match era {
                FrsEra::Early | FrsEra::Mid | FrsEra::Late => false,
                FrsEra::Current => get_positive_f64(row, "buuc") > 0.0,
            },
            rent_weekly: get_positive_f64(row, "burent"),
            free_school_meals_weekly: get_f64(row, "fsmbu").max(0.0),
            free_school_fruit_veg_weekly: get_f64(row, "fsfvbu").max(0.0),
            free_school_milk_weekly: get_f64(row, "fsmlkbu").max(0.0),
            healthy_start_vouchers_weekly: get_f64(row, "heartbu").max(0.0),
            free_tv_licence_weekly: get_f64(row, "butvlic").max(0.0),
        }
    }).collect()
}

// ── Person-level sub-table aggregation ───────────────────────────────────

type PersonKey = (i64, i64); // (sernum * 1000 + person)

fn person_key(sernum: i64, person: i64) -> PersonKey {
    (sernum, person)
}

#[derive(Default)]
struct AccountAgg {
    savings_interest_weekly: f64,
    dividend_income_weekly: f64,
}

fn aggregate_accounts(table: &Table) -> HashMap<PersonKey, AccountAgg> {
    let mut map: HashMap<PersonKey, AccountAgg> = HashMap::new();
    for row in table {
        let sernum = get_i64(row, "sernum");
        let person = get_i64(row, "person");
        let accint = get_f64(row, "accint");
        let account_type = get_i64(row, "account");
        let acctax = get_i64(row, "acctax");
        let invtax = get_i64(row, "invtax");

        let entry = map.entry(person_key(sernum, person)).or_default();

        // All account types feed into savings interest (ININV per FRS income tree):
        //   1=current, 3=NS&I investment, 5=savings, 27=basic, 28=credit union → gross up 1.25 if acctax==1
        //   2=NS&I Direct Saver → deduct £70/yr weekly exemption
        //   6=gilts → gross up 1.25 if invtax==1
        //   7=unit trusts, 8=stocks/shares/bonds → ININV (not dividends; director divs loaded via DIVIDGRO)
        //   9=PEP, 21=ISA, 24=peer-to-peer → non-taxable, no gross-up
        // Note: types 7 & 8 are NOT dividend income here — DIVIDGRO from adult.tab covers director
        // dividends separately. Mixing them caused double-counting.
        if [1, 3, 5, 27, 28].contains(&account_type) {
            let gross = if acctax == 1 { accint * 1.25 } else { accint };
            entry.savings_interest_weekly += gross.max(0.0);
        } else if account_type == 2 {
            // NS&I Direct Saver: deduct £70/yr = £70/52 weekly exemption per UKMOD
            entry.savings_interest_weekly += (accint - 70.0 / 52.0).max(0.0);
        } else if [9, 21, 24].contains(&account_type) {
            // PEP, ISA, Peer-to-peer: non-taxable — included in savings, no gross-up
            entry.savings_interest_weekly += accint.max(0.0);
        } else if account_type == 6 {
            // Gilts: taxable interest, gross up if invtax==1
            let gross = if invtax == 1 { accint * 1.25 } else { accint };
            entry.savings_interest_weekly += gross.max(0.0);
        } else if [7, 8].contains(&account_type) {
            // Unit trusts / stocks & shares: investment return, part of ININV
            entry.savings_interest_weekly += accint.max(0.0);
        }
    }
    map
}

#[derive(Default)]
struct OddjobAgg {
    oddjob_weekly: f64,
}

/// INRINC component: odd job income (OJAMT, weekly) from oddjob table.
fn aggregate_oddjobs(table: &Table) -> HashMap<PersonKey, OddjobAgg> {
    let mut map: HashMap<PersonKey, OddjobAgg> = HashMap::new();
    for row in table {
        let sernum = get_i64(row, "sernum");
        let person = get_i64(row, "person");
        let amt = get_positive_f64(row, "ojamt");
        map.entry(person_key(sernum, person)).or_default().oddjob_weekly += amt;
    }
    map
}


/// Benefit codes from FRS benefits table
#[derive(Default)]
struct BenefitAgg {
    state_pension: f64,
    child_benefit: f64,
    income_support: f64,
    housing_benefit: f64,
    attendance_allowance: f64,
    dla_sc: f64,
    dla_m: f64,
    carers_allowance: f64,
    pension_credit: f64,
    child_tax_credit: f64,
    working_tax_credit: f64,
    universal_credit: f64,
    pip_m: f64,
    pip_dl: f64,
    esa_income: f64,
    esa_contrib: f64,
    jsa_income: f64,
    jsa_contrib: f64,
    // Passthrough benefits we don't model but need in net income
    bereavement: f64,          // code 6: Bereavement Support Payment / Widowed Parent's Allowance
    maternity_allowance: f64,  // code 21: Maternity Allowance
    winter_fuel: f64,          // code 62: Winter Fuel Payments
    industrial_injuries: f64,  // code 15: Industrial Injuries Disablement Benefit
    sda: f64,                  // code 10: Severe Disablement Allowance
    war_pension: f64,          // codes 8,9: Armed Forces Compensation / War Widow's Pension
    other_ni_state: f64,       // code 30: any other NI or State benefit
    // Scottish disability replacements (equivalent to PIP/DLA)
    adp_dl: f64,               // code 117: Adult Disability Payment (Daily Living) Scotland
    adp_m: f64,                // code 118: Adult Disability Payment (Mobility) Scotland
    cdp_care: f64,             // code 121: Child Disability Payment (Care) Scotland
    cdp_mob: f64,              // code 122: Child Disability Payment (Mobility) Scotland
    scp: f64,                  // code 112: Scottish Child Payment
}

fn aggregate_benefits(table: &Table) -> HashMap<PersonKey, BenefitAgg> {
    let mut map: HashMap<PersonKey, BenefitAgg> = HashMap::new();
    for row in table {
        let sernum = get_i64(row, "sernum");
        let person = get_i64(row, "person");
        let benefit = get_i64(row, "benefit");
        let benpd = get_i64(row, "benpd");
        let benamt_raw = get_positive_f64(row, "benamt");
        let var2 = get_i64(row, "var2");
        // BENAMT is a weekly equivalent for regular benefits (benpd 1-52).
        // For benpd=0/90/95/97 (lump sums, one-offs), treat as annual and convert to weekly.
        // benpd=-1 is used for Winter Fuel Payment (code 62) which is always an annual lump sum.
        let benamt = match benpd {
            0 | 90 | 95 | 97 => benamt_raw / 52.0,
            -1 if benefit == 62 => benamt_raw / 52.0,
            _ => benamt_raw,
        };

        let entry = map.entry(person_key(sernum, person)).or_default();
        match benefit {
            5 => entry.state_pension += benamt,
            3 => entry.child_benefit += benamt,
            19 => entry.income_support += benamt,
            94 => entry.housing_benefit += benamt,
            12 => entry.attendance_allowance += benamt,
            1 => entry.dla_sc += benamt,
            2 => entry.dla_m += benamt,
            13 => entry.carers_allowance += benamt,
            4 => entry.pension_credit += benamt,
            91 => entry.child_tax_credit += benamt,
            90 => entry.working_tax_credit += benamt,
            95 => entry.universal_credit += benamt,
            97 => entry.pip_m += benamt,
            96 => entry.pip_dl += benamt,
            14 => {
                // JSA: var2 1,3 = contrib; 2,4 = income-based
                if var2 == 1 || var2 == 3 { entry.jsa_contrib += benamt; }
                if var2 == 2 || var2 == 4 { entry.jsa_income += benamt; }
            }
            16 => {
                // ESA: var2 1,3 = contrib; 2,4 = income-related
                if var2 == 1 || var2 == 3 { entry.esa_contrib += benamt; }
                if var2 == 2 || var2 == 4 { entry.esa_income += benamt; }
            }
            // Passthrough benefits
            6 => entry.bereavement += benamt,
            21 => entry.maternity_allowance += benamt,
            62 => entry.winter_fuel += benamt,
            15 => entry.industrial_injuries += benamt,
            10 => entry.sda += benamt,
            8 | 9 => entry.war_pension += benamt,
            30 => entry.other_ni_state += benamt,
            117 => entry.adp_dl += benamt,
            118 => entry.adp_m += benamt,
            121 => entry.cdp_care += benamt,
            122 => entry.cdp_mob += benamt,
            112 => entry.scp += benamt,
            _ => {}
        }
    }
    map
}

#[derive(Default)]
struct JobAgg {
    employee_pension_contributions_weekly: f64,
    #[allow(dead_code)]
    hours_worked_weekly: f64,
}

fn aggregate_jobs(table: &Table) -> HashMap<PersonKey, JobAgg> {
    let mut map: HashMap<PersonKey, JobAgg> = HashMap::new();
    for row in table {
        let sernum = get_i64(row, "sernum");
        let person = get_i64(row, "person");
        let deduc1 = get_positive_f64(row, "deduc1");

        let entry = map.entry(person_key(sernum, person)).or_default();
        entry.employee_pension_contributions_weekly += deduc1;
    }
    map
}

#[derive(Default)]
struct PensionAgg {
    private_pension_weekly: f64,
}

fn aggregate_pensions(table: &Table) -> HashMap<PersonKey, PensionAgg> {
    let mut map: HashMap<PersonKey, PensionAgg> = HashMap::new();
    for row in table {
        let sernum = get_i64(row, "sernum");
        let person = get_i64(row, "person");
        let penpay = get_positive_f64(row, "penpay");
        let ptamt = get_f64(row, "ptamt");
        let ptinc = get_i64(row, "ptinc");
        let poamt = get_f64(row, "poamt");
        let poinc = get_i64(row, "poinc");
        let penoth = get_i64(row, "penoth");

        let entry = map.entry(person_key(sernum, person)).or_default();
        entry.private_pension_weekly += penpay;
        if ptinc == 2 && ptamt > 0.0 { entry.private_pension_weekly += ptamt; }
        if (poinc == 2 || penoth == 1) && poamt > 0.0 { entry.private_pension_weekly += poamt; }
    }
    map
}

#[derive(Default)]
struct PenprovAgg {
    personal_pension_contributions_weekly: f64,
}

fn aggregate_penprov(table: &Table) -> HashMap<PersonKey, PenprovAgg> {
    let mut map: HashMap<PersonKey, PenprovAgg> = HashMap::new();
    for row in table {
        let sernum = get_i64(row, "sernum");
        let person = get_i64(row, "person");
        // stemppen (2006+): 5 or 6 = personal pension contribution
        // stemppay (2001–2005): 1 = personal pension contribution
        let stemppen = get_i64(row, "stemppen");
        let stemppay = get_i64(row, "stemppay");
        let is_personal = stemppen == 5 || stemppen == 6 || stemppay == 1;
        if !is_personal { continue; }

        let penamt_raw = get_positive_f64(row, "penamt");
        let penamtpd = get_i64(row, "penamtpd");
        // penamtpd=95 is an annual lump sum; divide by 52 to get weekly equivalent.
        // All other codes store amounts already expressed as weekly equivalents.
        let penamt = if penamtpd == 95 { penamt_raw / 52.0 } else { penamt_raw };

        let entry = map.entry(person_key(sernum, person)).or_default();
        entry.personal_pension_contributions_weekly += penamt;
    }
    map
}

// ── Person record parsing ────────────────────────────────────────────────

#[allow(dead_code)]
struct PersonRecord {
    sernum: i64,
    benunit: i64,
    person: i64,
    age: f64,
    gender: Gender,
    is_benunit_head: bool,
    is_household_head: bool,
    employment_income_weekly: f64,
    self_employment_income_weekly: f64,
    private_pension_income_weekly: f64,
    state_pension_weekly: f64,
    savings_interest_weekly: f64,
    dividend_income_weekly: f64,
    property_income_weekly: f64,
    maintenance_income_weekly: f64,
    miscellaneous_income_weekly: f64, // oddjob + sub-tenant rent
    hours_worked_weekly: f64,
    dla_care_low: bool,
    dla_care_mid: bool,
    dla_care_high: bool,
    dla_mob_low: bool,
    dla_mob_high: bool,
    pip_dl_std: bool,
    pip_dl_enh: bool,
    pip_mob_std: bool,
    pip_mob_enh: bool,
    aa_low: bool,
    aa_high: bool,
    is_disabled: bool,
    is_enhanced_disabled: bool,
    is_severely_disabled: bool,
    is_carer: bool,
    limitill: bool,
    esa_group: i64,
    emp_status: i64,
    looking_for_work: bool,
    is_self_identified_carer: bool,
    employee_pension_contributions_weekly: f64,
    personal_pension_contributions_weekly: f64,
    childcare_expenses_weekly: f64,
    // Benefits (weekly)
    child_benefit_weekly: f64,
    housing_benefit_weekly: f64,
    income_support_weekly: f64,
    pension_credit_weekly: f64,
    child_tax_credit_weekly: f64,
    working_tax_credit_weekly: f64,
    universal_credit_weekly: f64,
    dla_care_weekly: f64,
    dla_mobility_weekly: f64,
    pip_daily_living_weekly: f64,
    pip_mobility_weekly: f64,
    carers_allowance_weekly: f64,
    attendance_allowance_weekly: f64,
    esa_income_weekly: f64,
    esa_contributory_weekly: f64,
    jsa_income_weekly: f64,
    jsa_contributory_weekly: f64,
    // Aggregate of all unmodelled passthrough benefits (bereavement, maternity, winter fuel, etc.)
    other_benefits_weekly: f64,
    // Scottish disability payments (replace PIP/DLA in Scotland)
    adp_daily_living_weekly: f64,
    adp_mobility_weekly: f64,
    cdp_care_weekly: f64,
    cdp_mobility_weekly: f64,
    is_child: bool,
}

fn parse_adults(
    table: &Table,
    account_agg: &Option<HashMap<PersonKey, AccountAgg>>,
    benefit_agg: &Option<HashMap<PersonKey, BenefitAgg>>,
    job_agg: &Option<HashMap<PersonKey, JobAgg>>,
    pension_agg: &Option<HashMap<PersonKey, PensionAgg>>,
    penprov_agg: &Option<HashMap<PersonKey, PenprovAgg>>,
    oddjob_agg: &Option<HashMap<PersonKey, OddjobAgg>>,
    hh_property_map: &HashMap<i64, f64>,
    era: FrsEra,
) -> Vec<PersonRecord> {
    table.iter().map(|row| {
        let sernum = get_i64(row, "sernum");
        let person_id = get_i64(row, "person");
        let key = person_key(sernum, person_id);

        let acct = account_agg.as_ref().and_then(|m| m.get(&key));
        let bens = benefit_agg.as_ref().and_then(|m| m.get(&key));
        let jobs = job_agg.as_ref().and_then(|m| m.get(&key));
        let pens = pension_agg.as_ref().and_then(|m| m.get(&key));
        let pp = penprov_agg.as_ref().and_then(|m| m.get(&key));
        let oj = oddjob_agg.as_ref().and_then(|m| m.get(&key));

        let sex = get_i64(row, "sex");
        let hours = get_f64(row, "tothours").max(0.0);

        // Disability: classify PIP/DLA/AA into rate bands by comparing weekly amounts.
        // Rate-band midpoints for FRS 2023/24 (SI 2023/285 — Uprating Order 2023).
        // DLA care: low £26.90, mid £71.70, high £107.40
        // DLA mob:  low £26.90, high £75.75
        // PIP DL:   standard £68.10, enhanced £101.75
        // PIP mob:  standard £26.90, enhanced £75.75
        // AA:       lower £68.10, higher £101.75
        // We use midpoints between adjacent rates as split thresholds.
        let dla_sc = bens.map_or(0.0, |b| b.dla_sc);
        let dla_m = bens.map_or(0.0, |b| b.dla_m);
        let pip_dl = bens.map_or(0.0, |b| b.pip_dl);
        let pip_m = bens.map_or(0.0, |b| b.pip_m);
        let aa = bens.map_or(0.0, |b| b.attendance_allowance);

        // DLA care bands: low (<mid), mid (mid..high), high (>=high)
        let dla_care_low  = dla_sc > 0.0 && dla_sc < 49.30;  // midpoint(26.90, 71.70)
        let dla_care_mid  = dla_sc >= 49.30 && dla_sc < 89.55; // midpoint(71.70, 107.40)
        let dla_care_high = dla_sc >= 89.55;

        // DLA mobility bands: low (<high), high (>=high)
        let dla_mob_low  = dla_m > 0.0 && dla_m < 51.32; // midpoint(26.90, 75.75)
        let dla_mob_high = dla_m >= 51.32;

        // PIP DL bands: standard (<enhanced), enhanced (>=enhanced)
        let pip_dl_std = pip_dl > 0.0 && pip_dl < 84.93; // midpoint(68.10, 101.75)
        let pip_dl_enh = pip_dl >= 84.93;

        // PIP mobility bands
        let pip_mob_std = pip_m > 0.0 && pip_m < 51.32; // midpoint(26.90, 75.75)
        let pip_mob_enh = pip_m >= 51.32;

        // AA bands: lower (<higher), higher (>=higher)
        let aa_low  = aa > 0.0 && aa < 84.93; // midpoint(68.10, 101.75)
        let aa_high = aa >= 84.93;

        let is_disabled = (dla_sc + dla_m + pip_m + pip_dl + aa) > 0.0;
        // Enhanced disabled = highest DLA care or enhanced PIP DL (used for UC disabled child higher rate)
        let is_enhanced_disabled = dla_care_high || pip_dl_enh;
        // Severely disabled proxy for SDP: enhanced PIP DL or highest DLA care
        let is_severely_disabled = pip_dl_enh || dla_care_high;

        // Employment / health status from adult.tab
        // LIMITILL: only available from ~2004/05 onwards
        let limitill = match era {
            FrsEra::Early => false,
            _ => get_i64(row, "limitill") == 1,
        };
        // ESAGRP: only from 2008/09 (ESA introduced)
        let esa_group = match era {
            FrsEra::Early | FrsEra::Mid => 0,
            _ => get_i64(row, "esagrp"),
        };
        let emp_status = get_i64(row, "empstatb");
        // LOOKWK: available in early and mid eras, renamed/removed in some later years
        let looking_for_work = get_i64(row, "lookwk") == 1;
        // CARER1: only available from ~2009/10 onwards
        let is_self_identified_carer = match era {
            FrsEra::Early | FrsEra::Mid => false,
            _ => get_i64(row, "carer1") == 1,
        };

        // HRP flag: HRPID from 2002/03; UPERSON (benunit head) used as proxy before that
        let is_hrp = match era {
            FrsEra::Early => get_i64(row, "uperson") == 1 && get_i64(row, "benunit") == 1,
            _ => get_i64(row, "hrpid") == 1,
        };
        let royyr1 = get_positive_f64(row, "royyr1");
        let hh_prop = if is_hrp { hh_property_map.get(&sernum).copied().unwrap_or(0.0) } else { 0.0 };
        let property = royyr1 + hh_prop;

        // Maintenance received: MNTAMT1 (paid direct) + MNTAMT2 (via DWP).
        // If the usual amount differs (mntus1/2 == 2), use MNTUSAM1/2 instead.
        // Per UKMOD 07_Income.do logic. MRAMT from maint.tab is not used (mntamt already from adult.tab).
        let mntus1 = get_i64(row, "mntus1");
        let mntus2 = get_i64(row, "mntus2");
        let m1 = if mntus1 == 2 {
            get_positive_f64(row, "mntusam1")
        } else {
            get_positive_f64(row, "mntamt1")
        };
        let m2 = if mntus2 == 2 {
            get_positive_f64(row, "mntusam2")
        } else {
            get_positive_f64(row, "mntamt2")
        };
        let maintenance = m1 + m2;

        // Miscellaneous: odd job income + private transfers (UKMOD yptot components).
        // Per UKMOD 07_Income.do: allpay1/3/4 (friend/foster/adoption allowances),
        // apamt/apdamt (absent partner), pareamt (parental contrib), aliamt (maintenance for self).
        // These are all weekly amounts from adult.tab.
        let allow1 = get_i64(row, "allow1") == 1;
        let allow2 = get_i64(row, "allow2") == 1;
        let allow3 = get_i64(row, "allow3") == 1;
        let allow4 = get_i64(row, "allow4") == 1;
        let yptot = (if allow1 { get_positive_f64(row, "allpay1") } else { 0.0 })
            + (if allow2 { get_positive_f64(row, "allpay2") } else { 0.0 })
            + (if allow3 { get_positive_f64(row, "allpay3") } else { 0.0 })
            + (if allow4 { get_positive_f64(row, "allpay4") } else { 0.0 })
            + get_positive_f64(row, "apamt")
            + get_positive_f64(row, "apdamt")
            + get_positive_f64(row, "pareamt")
            + get_positive_f64(row, "aliamt");
        let misc = oj.map_or(0.0, |o| o.oddjob_weekly) + yptot;

        // Age: AGE80 (top-coded at 80) available from 2002/03; use AGE before that
        let age = match era {
            FrsEra::Early => get_f64(row, "age"),
            _ => get_f64(row, "age80"),
        };

        // Employment income: INEARNS is gross weekly earnings (all eras)
        let employment_income_weekly = get_positive_f64(row, "inearns");

        PersonRecord {
            sernum,
            benunit: get_i64(row, "benunit"),
            person: person_id,
            age,
            gender: if sex == 1 { Gender::Male } else { Gender::Female },
            is_benunit_head: get_i64(row, "uperson") == 1,
            is_household_head: is_hrp,
            employment_income_weekly,
            // seincam2 is the standard SE income column (1996+); inseinc is the pre-1996 name.
            self_employment_income_weekly: {
                let v = get_positive_f64(row, "seincam2");
                if v > 0.0 { v } else { get_positive_f64(row, "inseinc") }
            },
            private_pension_income_weekly: pens.map_or(
                get_positive_f64(row, "inpeninc"),
                |p| p.private_pension_weekly,
            ),
            state_pension_weekly: bens.map_or(0.0, |b| b.state_pension),
            savings_interest_weekly: acct.map_or(0.0, |a| a.savings_interest_weekly),
            // DIVIDGRO is director dividend income (weekly), taxed at dividend rates.
            // It is NOT in ININV (account-based investment) but is part of INRINC.
            dividend_income_weekly: acct.map_or(0.0, |a| a.dividend_income_weekly)
                + get_positive_f64(row, "dividgro"),
            property_income_weekly: property,
            maintenance_income_weekly: maintenance,
            miscellaneous_income_weekly: misc,
            hours_worked_weekly: hours,
            dla_care_low, dla_care_mid, dla_care_high,
            dla_mob_low, dla_mob_high,
            pip_dl_std, pip_dl_enh,
            pip_mob_std, pip_mob_enh,
            aa_low, aa_high,
            is_disabled,
            is_enhanced_disabled,
            is_severely_disabled,
            is_carer: bens.map_or(false, |b| b.carers_allowance > 0.0),
            limitill,
            esa_group,
            emp_status,
            looking_for_work,
            is_self_identified_carer,
            employee_pension_contributions_weekly: jobs.map_or(0.0, |j| j.employee_pension_contributions_weekly),
            personal_pension_contributions_weekly: pp.map_or(0.0, |p| p.personal_pension_contributions_weekly),
            childcare_expenses_weekly: 0.0, // Would need chldcare table
            child_benefit_weekly: bens.map_or(0.0, |b| b.child_benefit),
            housing_benefit_weekly: bens.map_or(0.0, |b| b.housing_benefit),
            income_support_weekly: bens.map_or(0.0, |b| b.income_support),
            pension_credit_weekly: bens.map_or(0.0, |b| b.pension_credit),
            child_tax_credit_weekly: bens.map_or(0.0, |b| b.child_tax_credit),
            working_tax_credit_weekly: bens.map_or(0.0, |b| b.working_tax_credit),
            universal_credit_weekly: bens.map_or(0.0, |b| b.universal_credit),
            dla_care_weekly: dla_sc,
            dla_mobility_weekly: dla_m,
            pip_daily_living_weekly: pip_dl,
            pip_mobility_weekly: pip_m,
            carers_allowance_weekly: bens.map_or(0.0, |b| b.carers_allowance),
            attendance_allowance_weekly: bens.map_or(0.0, |b| b.attendance_allowance),
            esa_income_weekly: bens.map_or(0.0, |b| b.esa_income),
            esa_contributory_weekly: bens.map_or(0.0, |b| b.esa_contrib),
            jsa_income_weekly: bens.map_or(0.0, |b| b.jsa_income),
            jsa_contributory_weekly: bens.map_or(0.0, |b| b.jsa_contrib),
            other_benefits_weekly: bens.map_or(0.0, |b| {
                b.bereavement + b.maternity_allowance + b.winter_fuel
                + b.industrial_injuries + b.sda + b.war_pension + b.other_ni_state
            }),
            adp_daily_living_weekly: bens.map_or(0.0, |b| b.adp_dl),
            adp_mobility_weekly: bens.map_or(0.0, |b| b.adp_m),
            cdp_care_weekly: bens.map_or(0.0, |b| b.cdp_care),
            cdp_mobility_weekly: bens.map_or(0.0, |b| b.cdp_mob),
            is_child: false,
        }
    }).collect()
}

fn parse_children(table: &Table) -> Vec<PersonRecord> {
    table.iter().map(|row| {
        let sernum = get_i64(row, "sernum");
        let person_id = get_i64(row, "person");
        let sex = get_i64(row, "sex");
        PersonRecord {
            sernum,
            benunit: get_i64(row, "benunit"),
            person: person_id,
            age: get_f64(row, "age"),
            gender: if sex == 1 { Gender::Male } else { Gender::Female },
            is_benunit_head: false,
            is_household_head: false,
            employment_income_weekly: get_f64(row, "chearns").max(0.0),
            self_employment_income_weekly: 0.0,
            private_pension_income_weekly: 0.0,
            state_pension_weekly: 0.0,
            savings_interest_weekly: 0.0,
            dividend_income_weekly: 0.0,
            property_income_weekly: 0.0,
            maintenance_income_weekly: 0.0,
            miscellaneous_income_weekly: get_f64(row, "chrinc").max(0.0),
            hours_worked_weekly: 0.0,
            dla_care_low: false, dla_care_mid: false, dla_care_high: false,
            dla_mob_low: false, dla_mob_high: false,
            pip_dl_std: false, pip_dl_enh: false,
            pip_mob_std: false, pip_mob_enh: false,
            aa_low: false, aa_high: false,
            is_disabled: false,
            is_enhanced_disabled: false,
            is_severely_disabled: false,
            is_carer: false,
            limitill: false,
            esa_group: 0,
            emp_status: 0,
            looking_for_work: false,
            is_self_identified_carer: false,
            employee_pension_contributions_weekly: 0.0,
            personal_pension_contributions_weekly: 0.0,
            childcare_expenses_weekly: 0.0,
            child_benefit_weekly: 0.0,
            housing_benefit_weekly: 0.0,
            income_support_weekly: 0.0,
            pension_credit_weekly: 0.0,
            child_tax_credit_weekly: 0.0,
            working_tax_credit_weekly: 0.0,
            universal_credit_weekly: 0.0,
            dla_care_weekly: 0.0,
            dla_mobility_weekly: 0.0,
            pip_daily_living_weekly: 0.0,
            pip_mobility_weekly: 0.0,
            carers_allowance_weekly: 0.0,
            attendance_allowance_weekly: 0.0,
            esa_income_weekly: 0.0,
            esa_contributory_weekly: 0.0,
            jsa_income_weekly: 0.0,
            jsa_contributory_weekly: 0.0,
            other_benefits_weekly: 0.0,
            adp_daily_living_weekly: 0.0,
            adp_mobility_weekly: 0.0,
            cdp_care_weekly: 0.0,
            cdp_mobility_weekly: 0.0,
            is_child: true,
        }
    }).collect()
}

// ── Dataset assembly ─────────────────────────────────────────────────────

fn assemble_dataset(
    hh_data: Vec<HouseholdRecord>,
    bu_data: Vec<BenUnitRecord>,
    adult_records: Vec<PersonRecord>,
    child_records: Vec<PersonRecord>,
) -> anyhow::Result<Dataset> {
    let mut hh_map: HashMap<i64, usize> = HashMap::new();
    let mut households: Vec<Household> = Vec::new();

    for (idx, hh) in hh_data.iter().enumerate() {
        hh_map.insert(hh.sernum, idx);
        households.push(Household {
            id: idx,
            benunit_ids: Vec::new(),
            person_ids: Vec::new(),
            weight: hh.weight,
            region: hh.region,
            rent: hh.rent_weekly * WEEKS_IN_YEAR,
            council_tax: hh.council_tax_annual,
            num_bedrooms: hh.num_bedrooms,
            tenure_type: hh.tenure_type,
            accommodation_type: hh.accommodation_type,
            ..Household::default()
        });
    }

    let mut bu_map: HashMap<(i64, i64), usize> = HashMap::new();
    let mut benunits: Vec<BenUnit> = Vec::new();

    for bu in &bu_data {
        if let Some(&hh_idx) = hh_map.get(&bu.sernum) {
            let bu_idx = benunits.len();
            bu_map.insert((bu.sernum, bu.benunit), bu_idx);
            benunits.push(BenUnit {
                id: bu_idx,
                household_id: hh_idx,
                person_ids: Vec::new(),
                on_uc: bu.claims_uc,
                rent_monthly: bu.rent_weekly * WEEKS_IN_YEAR / 12.0,
                is_lone_parent: false,
                free_school_meals: bu.free_school_meals_weekly * WEEKS_IN_YEAR,
                free_school_fruit_veg: bu.free_school_fruit_veg_weekly * WEEKS_IN_YEAR,
                free_school_milk: bu.free_school_milk_weekly * WEEKS_IN_YEAR,
                healthy_start_vouchers: bu.healthy_start_vouchers_weekly * WEEKS_IN_YEAR,
                free_tv_licence: bu.free_tv_licence_weekly * WEEKS_IN_YEAR,
                ..BenUnit::default()
            });
            households[hh_idx].benunit_ids.push(bu_idx);
        }
    }

    let mut people: Vec<Person> = Vec::new();

    let all_persons: Vec<&PersonRecord> = adult_records.iter()
        .chain(child_records.iter())
        .collect();

    for pr in all_persons {
        if let Some(&hh_idx) = hh_map.get(&pr.sernum) {
            let bu_key = (pr.sernum, pr.benunit);
            if let Some(&bu_idx) = bu_map.get(&bu_key) {
                let pid = people.len();
                let is_scotland = households[hh_idx].region.is_scotland();

                people.push(Person {
                    id: pid,
                    benunit_id: bu_idx,
                    household_id: hh_idx,
                    age: pr.age,
                    gender: pr.gender,
                    is_benunit_head: pr.is_benunit_head,
                    is_household_head: pr.is_household_head,
                    employment_income: pr.employment_income_weekly * WEEKS_IN_YEAR,
                    self_employment_income: (pr.self_employment_income_weekly * WEEKS_IN_YEAR).max(0.0),
                    pension_income: pr.private_pension_income_weekly * WEEKS_IN_YEAR,
                    state_pension: pr.state_pension_weekly * WEEKS_IN_YEAR,
                    savings_interest_income: pr.savings_interest_weekly * WEEKS_IN_YEAR,
                    dividend_income: pr.dividend_income_weekly * WEEKS_IN_YEAR,
                    capital_gains: 0.0,
                    capital_gains_residential_share: 0.0,
                    property_income: pr.property_income_weekly * WEEKS_IN_YEAR,
                    maintenance_income: pr.maintenance_income_weekly * WEEKS_IN_YEAR,
                    miscellaneous_income: pr.miscellaneous_income_weekly * WEEKS_IN_YEAR,
                    other_income: 0.0,
                    is_in_scotland: is_scotland,
                    hours_worked: pr.hours_worked_weekly * 52.0,
                    dla_care_low: pr.dla_care_low,
                    dla_care_mid: pr.dla_care_mid,
                    dla_care_high: pr.dla_care_high,
                    dla_mob_low: pr.dla_mob_low,
                    dla_mob_high: pr.dla_mob_high,
                    pip_dl_std: pr.pip_dl_std,
                    pip_dl_enh: pr.pip_dl_enh,
                    pip_mob_std: pr.pip_mob_std,
                    pip_mob_enh: pr.pip_mob_enh,
                    aa_low: pr.aa_low,
                    aa_high: pr.aa_high,
                    is_disabled: pr.is_disabled,
                    is_enhanced_disabled: pr.is_enhanced_disabled,
                    is_severely_disabled: pr.is_severely_disabled,
                    is_carer: pr.is_carer,
                    limitill: pr.limitill,
                    esa_group: pr.esa_group,
                    emp_status: pr.emp_status,
                    looking_for_work: pr.looking_for_work,
                    is_self_identified_carer: pr.is_self_identified_carer,
                    employee_pension_contributions: pr.employee_pension_contributions_weekly * WEEKS_IN_YEAR,
                    personal_pension_contributions: pr.personal_pension_contributions_weekly * WEEKS_IN_YEAR,
                    childcare_expenses: pr.childcare_expenses_weekly * WEEKS_IN_YEAR,
                    child_benefit: pr.child_benefit_weekly * WEEKS_IN_YEAR,
                    housing_benefit: pr.housing_benefit_weekly * WEEKS_IN_YEAR,
                    income_support: pr.income_support_weekly * WEEKS_IN_YEAR,
                    pension_credit: pr.pension_credit_weekly * WEEKS_IN_YEAR,
                    child_tax_credit: pr.child_tax_credit_weekly * WEEKS_IN_YEAR,
                    working_tax_credit: pr.working_tax_credit_weekly * WEEKS_IN_YEAR,
                    universal_credit: pr.universal_credit_weekly * WEEKS_IN_YEAR,
                    dla_care: pr.dla_care_weekly * WEEKS_IN_YEAR,
                    dla_mobility: pr.dla_mobility_weekly * WEEKS_IN_YEAR,
                    pip_daily_living: pr.pip_daily_living_weekly * WEEKS_IN_YEAR,
                    pip_mobility: pr.pip_mobility_weekly * WEEKS_IN_YEAR,
                    carers_allowance: pr.carers_allowance_weekly * WEEKS_IN_YEAR,
                    attendance_allowance: pr.attendance_allowance_weekly * WEEKS_IN_YEAR,
                    esa_income: pr.esa_income_weekly * WEEKS_IN_YEAR,
                    esa_contributory: pr.esa_contributory_weekly * WEEKS_IN_YEAR,
                    jsa_income: pr.jsa_income_weekly * WEEKS_IN_YEAR,
                    jsa_contributory: pr.jsa_contributory_weekly * WEEKS_IN_YEAR,
                    other_benefits: pr.other_benefits_weekly * WEEKS_IN_YEAR,
                    adp_daily_living: pr.adp_daily_living_weekly * WEEKS_IN_YEAR,
                    adp_mobility: pr.adp_mobility_weekly * WEEKS_IN_YEAR,
                    cdp_care: pr.cdp_care_weekly * WEEKS_IN_YEAR,
                    cdp_mobility: pr.cdp_mobility_weekly * WEEKS_IN_YEAR,
                });

                benunits[bu_idx].person_ids.push(pid);
                households[hh_idx].person_ids.push(pid);
            }
        }
    }

    // Derive lone parent status and UC receipt from person-level reported benefits.
    // All other take-up is derived at runtime from reported amounts (BenUnit::claims).
    for bu in &mut benunits {
        let num_adults = bu.person_ids.iter().filter(|&&pid| people[pid].is_adult()).count();
        let num_children = bu.person_ids.iter().filter(|&&pid| people[pid].is_child()).count();
        bu.is_lone_parent = num_adults == 1 && num_children > 0;

        if bu.person_ids.iter().any(|&pid| people[pid].universal_credit > 0.0) {
            bu.on_uc = true;
        }
        // Survey records gate take-up on reported receipt, not eligibility, so a
        // benunit only routes to the UC system if it reported UC. (The struct
        // default true is for hypothetical households without reported amounts.)
        bu.claims_uc_if_eligible = bu.on_uc;
    }

    Ok(Dataset {
        people,
        benunits,
        households,
        name: "Family Resources Survey 2023-24".to_string(),
        year: 2023,
    })
}
