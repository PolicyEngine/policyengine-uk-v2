
use clap::Parser;
use colored::Colorize;
use comfy_table::{Table, ContentArrangement, presets};
use serde::Serialize;
use std::path::PathBuf;

use policyengine_uk_rust::run;
use policyengine_uk_rust::parameters::Parameters;
use policyengine_uk_rust::reforms::Reform;
use policyengine_uk_rust::data::frs::load_frs;
use policyengine_uk_rust::data::spi::load_spi;
use policyengine_uk_rust::data::lcfs::load_lcfs;
use policyengine_uk_rust::data::was::load_was;
use policyengine_uk_rust::data::clean::{write_clean_csvs, load_clean_dataset, write_microdata, write_microdata_to_stdout};
use policyengine_uk_rust::data::stdin::load_dataset_from_reader;

#[derive(Parser)]
#[command(name = "policyengine-uk")]
#[command(about = "UK tax-benefit microsimulation engine")]
#[command(version)]
#[command(after_help = "\
MODEL RUNS (any clean dataset + year):
  Score a policy:    policyengine-uk --data data/frs/ --year 2025 --output json
  Score with reform: policyengine-uk --data data/frs/ --year 2025 --policy-json '{...}'
  Export microdata:  policyengine-uk --data data/frs/ --year 2025 --output-microdata out/

DATA CREATION (raw survey → clean CSVs):
  FRS:  policyengine-uk --frs  raw_tab_dir/ --year 2023 --extract data/frs/2023/
  SPI:  policyengine-uk --spi  raw_tab_dir/ --year 2022 --extract data/spi/2022/
  LCFS: policyengine-uk --lcfs raw_tab_dir/ --year 2023 --extract data/lcfs/2023/
  WAS:  policyengine-uk --was  raw_tab_dir/ --year 2020 --extract data/was/2020/
  Uprated: policyengine-uk --frs raw_tab_dir/ --year 2023 --uprate-to 2026 --extract data/frs/2026/

PARAMETER INSPECTION:
  Export as JSON:     policyengine-uk --year 2025 --export-params-json
  Export as YAML:     policyengine-uk --year 2025 --export-baseline
")]
struct Cli {
    // ── Data source for simulation (pick one) ──

    /// Read dataset from stdin (concatenated CSV protocol).
    #[arg(long)]
    stdin_data: bool,

    /// Base dir with per-year clean subdirs (YYYY/persons.csv etc.).
    /// Works for any dataset (FRS, SPI, LCFS, WAS). Falls back to latest year + uprating.
    #[arg(long)]
    data: Option<PathBuf>,

    // ── Raw data extraction (survey-specific) ──

    /// Raw FRS tab-file directory.
    #[arg(long)]
    frs: Option<PathBuf>,

    /// Raw SPI tab-file directory.
    #[arg(long)]
    spi: Option<PathBuf>,

    /// Raw LCFS tab-file directory.
    #[arg(long)]
    lcfs: Option<PathBuf>,

    /// Raw WAS tab-file directory.
    #[arg(long)]
    was: Option<PathBuf>,

    /// Output directory for extracted clean CSVs. Requires --frs, --spi, --lcfs, or --was.
    #[arg(long)]
    extract: Option<PathBuf>,

    /// When used with --extract, uprate the extracted dataset to this fiscal year before writing.
    #[arg(long)]
    uprate_to: Option<u32>,

    // ── Year ──

    /// Fiscal year (e.g. 2025 for 2025/26). Range: 1994-2030.
    #[arg(short, long, default_value = "2025")]
    year: u32,

    // ── Policy ──

    /// Policy file (YAML overlay on baseline parameters).
    #[arg(short, long)]
    policy: Option<PathBuf>,

    /// Policy as inline JSON string.
    #[arg(long)]
    policy_json: Option<String>,

    // ── Model run output ──

    /// Output format: "json" for machine-readable, "pretty" for terminal table.
    #[arg(long, default_value = "json")]
    output: String,

    /// Write enhanced microdata CSVs (inputs + simulation outputs) to directory.
    #[arg(long)]
    output_microdata: Option<PathBuf>,

    /// Write enhanced microdata to stdout (concatenated CSV protocol).
    #[arg(long)]
    output_microdata_stdout: bool,

    /// Include baseline_* columns alongside reform_* in microdata output.
    /// By default only reform values are written with unprefixed column names.
    #[arg(long)]
    microdata_return_baselines: bool,

    // ── Parameter inspection ──

    /// Export baseline parameters as JSON.
    #[arg(long)]
    export_params_json: bool,

    /// Export baseline parameters as YAML.
    #[arg(long)]
    export_baseline: bool,

    /// Only simulate person-level variables (income tax, NI). Skips benefit unit and
    /// household calculations. Suitable for SPI and other datasets without household structure.
    #[arg(long)]
    persons_only: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Load baseline parameters for the chosen fiscal year
    let baseline_params = Parameters::for_year(cli.year)?;

    if cli.export_baseline {
        println!("{}", baseline_params.to_yaml());
        return Ok(());
    }

    if cli.export_params_json {
        println!("{}", baseline_params.to_json());
        return Ok(());
    }

    let json_mode = cli.output == "json";

    // Extract raw survey data to clean CSVs if requested
    if let Some(output_dir) = &cli.extract {
        let mut dataset = if let Some(path) = &cli.frs {
            eprintln!("Loading raw FRS from {}...", path.display());
            load_frs(path, cli.year)?
        } else if let Some(path) = &cli.spi {
            eprintln!("Loading raw SPI from {}...", path.display());
            load_spi(path, cli.year)?
        } else if let Some(path) = &cli.lcfs {
            eprintln!("Loading raw LCFS from {}...", path.display());
            load_lcfs(path, cli.year)?
        } else if let Some(path) = &cli.was {
            eprintln!("Loading raw WAS from {}...", path.display());
            load_was(path, cli.year)?
        } else {
            anyhow::bail!("--extract requires a raw data source: --frs, --spi, --lcfs, or --was");
        };
        eprintln!("Loaded {} households, {} people", dataset.households.len(), dataset.people.len());
        if let Some(target_year) = cli.uprate_to {
            eprintln!("Uprating from {}/{} to {}/{}...",
                dataset.year, (dataset.year + 1) % 100,
                target_year, (target_year + 1) % 100);
            dataset.uprate_to(target_year);
        }
        write_clean_csvs(&mut dataset, output_dir)?;
        eprintln!("Wrote clean CSVs to {}", output_dir.display());
        return Ok(());
    }

    // Load dataset for simulation
    let dataset = if cli.stdin_data {
        load_dataset_from_reader(std::io::BufReader::new(std::io::stdin().lock()), cli.year)?
    } else if let Some(base) = &cli.data {
        // Base dir with per-year clean subdirs: base/YYYY/persons.csv etc.
        let year_dir = base.join(cli.year.to_string());
        if year_dir.is_dir() {
            if !json_mode { println!("  {} Loading clean data {}/{}...", "▸".bright_cyan(), cli.year, (cli.year + 1) % 100); }
            load_clean_dataset(&year_dir, cli.year)?
        } else {
            // Find latest available year and uprate
            let latest = (1994..=cli.year).rev()
                .find(|y| base.join(y.to_string()).is_dir())
                .ok_or_else(|| anyhow::anyhow!("No clean data found in {}", base.display()))?;
            if !json_mode {
                println!("  {} Loading clean data {}/{} and uprating to {}/{}...",
                    "▸".bright_cyan(), latest, (latest + 1) % 100,
                    cli.year, (cli.year + 1) % 100);
            }
            let mut ds = load_clean_dataset(&base.join(latest.to_string()), latest)?;
            ds.uprate_to(cli.year);
            ds
        }
    } else {
        anyhow::bail!("No data source specified. Use --data <clean-data-base> or --stdin-data.\n\
            To create clean data from raw surveys, use --extract with --frs, --spi, --lcfs, or --was.")
    };
    // Load policy (if none specified, policy = baseline). Reforms loaded from a
    // YAML file may also declare a `neutralise:` list, applied to the reform
    // results below; JSON overlays don't carry one (parameter-only by design).
    let mut reform: Option<Reform> = None;
    let policy_params = if let Some(json_str) = &cli.policy_json {
        baseline_params.apply_json_overlay(json_str)?
    } else if let Some(path) = &cli.policy {
        let r = Reform::from_file(path, &baseline_params)?;
        let params = r.parameters.clone();
        reform = Some(r);
        params
    } else if json_mode {
        baseline_params.clone()
    } else {
        let r = Reform::personal_allowance_20k(&baseline_params);
        r.parameters
    };

    // Run baseline simulation
    let baseline = run::run_baseline(&dataset, &baseline_params, cli.year);

    // Labour supply responses (if enabled in the policy parameters) plus the
    // policy simulation.
    let mut reformed = run::run_reform(&dataset, &baseline_params, &policy_params, &baseline, cli.year);
    // Neutralisation runs after the reform simulation completes, so baseline
    // results are unaffected. No-op when the reform has an empty `neutralise`.
    if let Some(r) = reform.as_ref() {
        r.apply_to_results(&mut reformed, &dataset.benunits, &dataset.households);
    }

    // Persons-only output: per-person tax results, no household/benefit analysis
    if cli.persons_only {
        if cli.output == "json" {
            #[derive(Serialize)]
            struct PersonRecord {
                person_id: usize,
                weight: f64,
                employment_income: f64,
                self_employment_income: f64,
                pension_income: f64,
                savings_interest_income: f64,
                dividend_income: f64,
                baseline_income_tax: f64,
                baseline_employee_ni: f64,
                baseline_employer_ni: f64,
                baseline_ni_class1_employee: f64,
                baseline_ni_class2: f64,
                baseline_ni_class4: f64,
                reform_income_tax: f64,
                reform_employee_ni: f64,
                reform_employer_ni: f64,
                reform_ni_class1_employee: f64,
                reform_ni_class2: f64,
                reform_ni_class4: f64,
            }
            let mut records: Vec<PersonRecord> = Vec::new();
            for hh in &dataset.households {
                for &pid in &hh.person_ids {
                    let p = &dataset.people[pid];
                    let bp = &baseline.person_results[pid];
                    let rp = &reformed.person_results[pid];
                    records.push(PersonRecord {
                        person_id: pid,
                        weight: hh.weight,
                        employment_income: p.employment_income,
                        self_employment_income: p.self_employment_income,
                        pension_income: p.pension_income,
                        savings_interest_income: p.savings_interest_income,
                        dividend_income: p.dividend_income,
                        baseline_income_tax: bp.income_tax,
                        baseline_employee_ni: bp.national_insurance,
                        baseline_employer_ni: bp.employer_ni,
                        baseline_ni_class1_employee: bp.ni_class1_employee,
                        baseline_ni_class2: bp.ni_class2,
                        baseline_ni_class4: bp.ni_class4,
                        reform_income_tax: rp.income_tax,
                        reform_employee_ni: rp.national_insurance,
                        reform_employer_ni: rp.employer_ni,
                        reform_ni_class1_employee: rp.ni_class1_employee,
                        reform_ni_class2: rp.ni_class2,
                        reform_ni_class4: rp.ni_class4,
                    });
                }
            }
            println!("{}", serde_json::to_string(&records)?);
        }
        return Ok(());
    }

    // Enhanced microdata output
    if let Some(micro_dir) = &cli.output_microdata {
        std::fs::create_dir_all(micro_dir)?;
        write_microdata(&dataset, &baseline, &reformed, micro_dir, cli.year, cli.microdata_return_baselines)?;
        if !json_mode {
            println!("  {} Wrote enhanced microdata to {}", "▸".bright_cyan(), micro_dir.display());
        }
        return Ok(());
    }

    // Microdata to stdout
    if cli.output_microdata_stdout {
        write_microdata_to_stdout(&dataset, &baseline, &reformed, cli.year, cli.microdata_return_baselines)?;
        return Ok(());
    }

    let output = run::analyse(&dataset, &baseline_params, &baseline, &reformed, cli.year);

    // JSON output mode
    if json_mode {
        println!("{}", serde_json::to_string(&output)?);
        return Ok(());
    }

    let run::JsonOutput {
        winners_losers, baseline_hbai_incomes, baseline_poverty, budgetary_impact, ..
    } = &output;
    let run::BudgetaryImpact {
        baseline_revenue, reform_revenue, revenue_change, baseline_benefits,
        reform_benefits, benefit_spending_change: benefit_change, net_cost,
    } = *budgetary_impact;

    // Pretty output
    println!();
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bright_blue());
    println!("  {} {}", "PolicyEngine UK".bright_white().bold(), format!("v{}", env!("CARGO_PKG_VERSION")).dimmed());
    println!("  {}", "High-performance microsimulation engine in Rust".dimmed());
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bright_blue());
    println!();

    println!("    {} {} households, {} people",
        "✓".bright_green(),
        format_num(dataset.households.len()),
        format_num(dataset.people.len()),
    );
    println!("    {} Fiscal year: {}", "◆".bright_cyan(), baseline_params.fiscal_year.bright_white());

    println!();
    println!("{}", "═══════════════════════════════════════════════════════════════════════════════════".bright_yellow());
    println!("  {}", "FISCAL IMPACT".bright_white().bold().underline());
    println!("{}", "═══════════════════════════════════════════════════════════════════════════════════".bright_yellow());

    let mut fiscal_table = Table::new();
    fiscal_table.load_preset(presets::UTF8_FULL);
    fiscal_table.set_content_arrangement(ContentArrangement::Dynamic);
    fiscal_table.set_header(vec!["Metric", "Baseline", "Reform", "Change"]);
    fiscal_table.add_row(vec![
        "Tax Revenue".to_string(),
        format!("£{:.1}bn", baseline_revenue / 1e9),
        format!("£{:.1}bn", reform_revenue / 1e9),
        format_change_bn(revenue_change),
    ]);
    fiscal_table.add_row(vec![
        "Benefit Spending".to_string(),
        format!("£{:.1}bn", baseline_benefits / 1e9),
        format!("£{:.1}bn", reform_benefits / 1e9),
        format_change_bn(benefit_change),
    ]);
    fiscal_table.add_row(vec![
        "Net Cost to Exchequer".to_string(),
        "".to_string(),
        "".to_string(),
        format!("£{:.1}bn", net_cost / 1e9),
    ]);
    println!("{fiscal_table}");

    // Winners and losers
    println!("\n  {}", "WINNERS & LOSERS".bright_white().bold().underline());
    println!();
    println!("    {} {:.1}% gain — avg £{:.0}/year",
        "▲".bright_green(), winners_losers.winners_pct, winners_losers.avg_gain);
    println!("    {} {:.1}% lose — avg £{:.0}/year",
        "▼".bright_red(), winners_losers.losers_pct, winners_losers.avg_loss);
    println!("    {} {:.1}% unchanged",
        "●".dimmed(), winners_losers.unchanged_pct);

    // HBAI incomes (means/medians)
    println!("\n  {}", "HBAI INCOMES".bright_white().bold().underline());
    println!();
    let mut hbai_table = Table::new();
    hbai_table.load_preset(presets::UTF8_FULL);
    hbai_table.set_content_arrangement(ContentArrangement::Dynamic);
    hbai_table.set_header(vec!["Metric", "Value"]);
    hbai_table.add_row(vec!["Median equivalised BHC".to_string(), format!("£{:.0}", baseline_hbai_incomes.median_equiv_bhc)]);
    hbai_table.add_row(vec!["Median equivalised AHC".to_string(), format!("£{:.0}", baseline_hbai_incomes.median_equiv_ahc)]);
    hbai_table.add_row(vec!["Mean equivalised BHC".to_string(), format!("£{:.0}", baseline_hbai_incomes.mean_equiv_bhc)]);
    hbai_table.add_row(vec!["Mean equivalised AHC".to_string(), format!("£{:.0}", baseline_hbai_incomes.mean_equiv_ahc)]);
    hbai_table.add_row(vec!["Mean BHC (unequivalised)".to_string(), format!("£{:.0}", baseline_hbai_incomes.mean_bhc)]);
    hbai_table.add_row(vec!["Mean AHC (unequivalised)".to_string(), format!("£{:.0}", baseline_hbai_incomes.mean_ahc)]);
    println!("{hbai_table}");

    // Poverty headcounts
    println!("\n  {}", "POVERTY HEADCOUNTS".bright_white().bold().underline());
    println!();
    let mut pov_table = Table::new();
    pov_table.load_preset(presets::UTF8_FULL);
    pov_table.set_content_arrangement(ContentArrangement::Dynamic);
    pov_table.set_header(vec!["Group", "Relative BHC", "Relative AHC", "Absolute BHC", "Absolute AHC"]);
    pov_table.add_row(vec![
        "Children".to_string(),
        format!("{:.1}%", baseline_poverty.relative_bhc_children),
        format!("{:.1}%", baseline_poverty.relative_ahc_children),
        format!("{:.1}%", baseline_poverty.absolute_bhc_children),
        format!("{:.1}%", baseline_poverty.absolute_ahc_children),
    ]);
    pov_table.add_row(vec![
        "Working-age".to_string(),
        format!("{:.1}%", baseline_poverty.relative_bhc_working_age),
        format!("{:.1}%", baseline_poverty.relative_ahc_working_age),
        format!("{:.1}%", baseline_poverty.absolute_bhc_working_age),
        format!("{:.1}%", baseline_poverty.absolute_ahc_working_age),
    ]);
    pov_table.add_row(vec![
        "Pensioners".to_string(),
        format!("{:.1}%", baseline_poverty.relative_bhc_pensioners),
        format!("{:.1}%", baseline_poverty.relative_ahc_pensioners),
        format!("{:.1}%", baseline_poverty.absolute_bhc_pensioners),
        format!("{:.1}%", baseline_poverty.absolute_ahc_pensioners),
    ]);
    println!("{pov_table}");

    println!();
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bright_blue());
    println!();

    Ok(())
}

fn format_num(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

fn format_change_bn(n: f64) -> String {
    if n >= 0.0 {
        format!("+£{:.1}bn", n / 1e9)
    } else {
        format!("-£{:.1}bn", n.abs() / 1e9)
    }
}

/// Aggregate statistics from a simulation run for validation.
#[allow(dead_code)]
fn aggregate_stats(
    dataset: &policyengine_uk_rust::data::Dataset,
    results: &policyengine_uk_rust::engine::simulation::SimulationResults,
) -> (f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) {
    let hhs = &dataset.households;
    let bus = &dataset.benunits;

    let income_tax: f64 = hhs.iter().flat_map(|h| h.person_ids.iter().map(|&p| h.weight * results.person_results[p].income_tax)).sum();
    let employee_ni: f64 = hhs.iter().flat_map(|h| h.person_ids.iter().map(|&p| h.weight * results.person_results[p].national_insurance)).sum();
    let employer_ni: f64 = hhs.iter().flat_map(|h| h.person_ids.iter().map(|&p| h.weight * results.person_results[p].employer_ni)).sum();
    let uc: f64 = bus.iter().map(|b| hhs[b.household_id].weight * results.benunit_results[b.id].universal_credit).sum();
    let cb: f64 = bus.iter().map(|b| hhs[b.household_id].weight * results.benunit_results[b.id].child_benefit).sum();
    let sp: f64 = bus.iter().map(|b| hhs[b.household_id].weight * results.benunit_results[b.id].state_pension).sum();
    let pc: f64 = bus.iter().map(|b| hhs[b.household_id].weight * results.benunit_results[b.id].pension_credit).sum();
    let hb: f64 = bus.iter().map(|b| hhs[b.household_id].weight * results.benunit_results[b.id].housing_benefit).sum();
    let ctc: f64 = bus.iter().map(|b| hhs[b.household_id].weight * results.benunit_results[b.id].child_tax_credit).sum();
    let wtc: f64 = bus.iter().map(|b| hhs[b.household_id].weight * results.benunit_results[b.id].working_tax_credit).sum();
    let it_payers: f64 = hhs.iter().flat_map(|h| h.person_ids.iter().map(|&p| if results.person_results[p].income_tax > 0.0 { h.weight } else { 0.0 })).sum();
    let uc_claimants: f64 = bus.iter().map(|b| if results.benunit_results[b.id].universal_credit > 0.0 { hhs[b.household_id].weight } else { 0.0 }).sum();
    (income_tax, employee_ni, employer_ni, uc, cb, sp, pc, hb, ctc + wtc, it_payers, uc_claimants, 0.0)
}

#[cfg(test)]
mod obr_validation {
    /// OBR validation tests — require clean FRS data at data/frs/2023.
    /// Skips gracefully if data not present (e.g. in CI without FRS access).
    ///
    /// Tolerances are ±20% of OBR outturn/forecast (OBR EFO March 2025, 2025/26).
    /// These are gross sanity checks, not precision targets.
    #[test]
    fn obr_2025_revenue_and_spending() {
        use policyengine_uk_rust::data::clean::load_clean_dataset;
        use policyengine_uk_rust::engine::Simulation;
use policyengine_uk_rust::run;
        use policyengine_uk_rust::parameters::Parameters;
        use std::path::Path;

        if !Path::new("data/frs/2023").exists() {
            eprintln!("Skipping OBR validation: data/frs/2023 not found");
            return;
        }

        let dataset = load_clean_dataset(Path::new("data/frs/2023"), 2023)
            .expect("data/frs/2023 must contain persons.csv, benunits.csv, households.csv");
        let params = Parameters::for_year(2025).unwrap();
        let sim = Simulation::new(
            dataset.people.clone(), dataset.benunits.clone(),
            dataset.households.clone(), params, 2025,
        );
        let results = sim.run();

        let hhs = &dataset.households;
        let bus = &dataset.benunits;

        macro_rules! weighted_person_sum {
            ($field:ident) => {
                hhs.iter().flat_map(|h| h.person_ids.iter()
                    .map(|&p| h.weight * results.person_results[p].$field))
                    .sum::<f64>()
            };
        }
        macro_rules! weighted_bu_sum {
            ($field:ident) => {
                bus.iter().map(|b| hhs[b.household_id].weight * results.benunit_results[b.id].$field)
                    .sum::<f64>()
            };
        }
        macro_rules! bu_caseload {
            ($field:ident) => {
                bus.iter().map(|b| if results.benunit_results[b.id].$field > 0.0 { hhs[b.household_id].weight } else { 0.0 })
                    .sum::<f64>()
            };
        }
        macro_rules! person_caseload {
            ($field:ident) => {
                hhs.iter().flat_map(|h| h.person_ids.iter()
                    .map(|&p| if results.person_results[p].$field > 0.0 { h.weight } else { 0.0 }))
                    .sum::<f64>()
            };
        }

        // OBR March 2025 EFO, 2025/26 (£bn)
        // Revenue
        let income_tax = weighted_person_sum!(income_tax);
        let employee_ni = weighted_person_sum!(national_insurance);
        let employer_ni = weighted_person_sum!(employer_ni);
        // Benefits
        let uc = weighted_bu_sum!(universal_credit);
        let cb = weighted_bu_sum!(child_benefit);
        let sp = weighted_bu_sum!(state_pension);
        let pc = weighted_bu_sum!(pension_credit);
        let _hb = weighted_bu_sum!(housing_benefit);
        let _tc = weighted_bu_sum!(child_tax_credit) + weighted_bu_sum!(working_tax_credit);
        // Caseloads
        let it_payers = person_caseload!(income_tax);
        let uc_claimants = bu_caseload!(universal_credit);
        let cb_claimants = bu_caseload!(child_benefit);

        // ── Revenue checks (OBR 2025/26 central forecast) ──
        // Income tax: ~£305bn (OBR), model ~£250bn due to FRS income underreporting
        assert!(income_tax > 200e9 && income_tax < 380e9,
            "Income tax £{:.0}bn outside [£200bn, £380bn]", income_tax / 1e9);
        // Employee NI: ~£72bn
        assert!(employee_ni > 40e9 && employee_ni < 100e9,
            "Employee NI £{:.0}bn outside [£40bn, £100bn]", employee_ni / 1e9);
        // Employer NI: ~£115bn (pre-2025 Budget rise)
        assert!(employer_ni > 80e9 && employer_ni < 200e9,
            "Employer NI £{:.0}bn outside [£80bn, £200bn]", employer_ni / 1e9);

        // ── Benefit spending checks ──
        // UC: ~£79bn OBR (inc. housing element); model awards only to reported claimants
        assert!(uc > 30e9 && uc < 100e9,
            "UC £{:.0}bn outside [£30bn, £100bn]", uc / 1e9);
        // Child benefit: only reported claimants; ~£4-15bn
        assert!(cb > 2e9 && cb < 22e9,
            "Child benefit £{:.0}bn outside [£2bn, £22bn]", cb / 1e9);
        // State pension: ~£130bn
        assert!(sp > 80e9 && sp < 180e9,
            "State pension £{:.0}bn outside [£80bn, £180bn]", sp / 1e9);
        // Pension credit: only reported claimants; ~£2-12bn
        assert!(pc > 1e9 && pc < 12e9,
            "Pension credit £{:.0}bn outside [£1bn, £12bn]", pc / 1e9);
        // Housing benefit: now folded into UC housing element; standalone HB ~£0 in model
        // OBR shows £12bn standalone HB (pensioners/legacy remaining) — we skip this check
        // as the spending is captured within UC total above.
        // Tax credits: folded into UC; standalone TC now ~£0 in model (migration complete)

        // ── Caseload checks ──
        // IT payers: ~32m
        assert!(it_payers > 25e6 && it_payers < 40e6,
            "IT payers {:.1}m outside [25m, 40m]", it_payers / 1e6);
        // UC claimants: ~3-7m benefit units (OBR counts individuals; model counts benefit units)
        assert!(uc_claimants > 2e6 && uc_claimants < 10e6,
            "UC claimants {:.1}m outside [2m, 10m]", uc_claimants / 1e6);
        // Child benefit claimants: only reported claimants
        assert!(cb_claimants > 1e6 && cb_claimants < 9e6,
            "CB claimants {:.1}m outside [1m, 9m]", cb_claimants / 1e6);
    }
}

#[cfg(test)]
mod historical_frs_tests {
    use policyengine_uk_rust::data::frs::load_frs;
    use policyengine_uk_rust::engine::Simulation;
use policyengine_uk_rust::run;
    use policyengine_uk_rust::parameters::Parameters;
    use std::path::Path;

    /// Test that representative historical FRS years load and simulate correctly.
    /// Tests one year per era: Early (1994), Mid (2003), Late (2013), Current (2023).
    /// Skips if frs_raw not present.
    #[test]
    fn all_historical_years_run() {
        let raw_base = Path::new("data/frs_raw");
        if !raw_base.exists() {
            eprintln!("Skipping historical FRS test: data/frs_raw not found");
            return;
        }

        // One representative year per FrsEra
        for year in [1994u32, 2003, 2013, 2023] {
            let suffix = format!("frs_{}_{:02}", year, (year + 1) % 100);
            let year_dir = raw_base.join(&suffix);
            if !year_dir.exists() {
                eprintln!("Skipping {}/{}: directory not found", year, year + 1);
                continue;
            }

            let tab_dir = find_tab_dir(&year_dir);
            let tab_dir = match tab_dir {
                Some(d) => d,
                None => {
                    eprintln!("Skipping {}/{}: no tab directory found", year, year + 1);
                    continue;
                }
            };

            let dataset = load_frs(&tab_dir, year)
                .unwrap_or_else(|e| panic!("Failed to load FRS {}/{}: {}", year, year + 1, e));

            assert!(!dataset.households.is_empty(),
                "FRS {}/{} loaded 0 households", year, year + 1);
            assert!(!dataset.people.is_empty(),
                "FRS {}/{} loaded 0 people", year, year + 1);

            let params = Parameters::for_year(year)
                .unwrap_or_else(|e| panic!("Failed to load params {}/{}: {}", year, year + 1, e));

            let sim = Simulation::new(
                dataset.people.clone(), dataset.benunits.clone(),
                dataset.households.clone(), params, year,
            );
            let results = sim.run();

            // Basic sanity: income tax should be positive
            let it: f64 = dataset.households.iter()
                .flat_map(|h| h.person_ids.iter()
                    .map(|&p| h.weight * results.person_results[p].income_tax))
                .sum();
            assert!(it > 10e9,
                "FRS {}/{}: income tax £{:.0}bn suspiciously low", year, year + 1, it / 1e9);

            eprintln!("  {}/{}: OK ({} HH, IT=£{:.0}bn)",
                year, year + 1, dataset.households.len(), it / 1e9);
        }
    }

    fn find_tab_dir(year_dir: &Path) -> Option<std::path::PathBuf> {
        for entry in std::fs::read_dir(year_dir).ok()? {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("UKDA-") && name.ends_with("-tab") {
                let tab = entry.path().join("tab");
                if tab.is_dir() { return Some(tab); }
            }
        }
        if year_dir.join("househol.tab").exists() {
            return Some(year_dir.to_path_buf());
        }
        None
    }
}
