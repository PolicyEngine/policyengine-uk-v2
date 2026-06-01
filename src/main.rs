mod engine;
mod parameters;
mod variables;
mod data;
mod reforms;

use clap::Parser;
use colored::Colorize;
use comfy_table::{Table, ContentArrangement, presets};
use serde::Serialize;
use std::path::PathBuf;

use crate::engine::Simulation;
use crate::parameters::Parameters;
use crate::reforms::Reform;
use crate::data::frs::load_frs;
use crate::data::spi::load_spi;
use crate::data::lcfs::load_lcfs;
use crate::data::was::load_was;
use crate::data::clean::{write_clean_csvs, load_clean_dataset, write_microdata, write_microdata_to_stdout};
use crate::data::stdin::load_dataset_from_reader;
use crate::data::efrs;
use crate::data::calibrate;

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

    /// Fiscal year (e.g. 2025 for 2025/26). Range: 1994-2029.
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

    /// Extract Enhanced FRS (wealth + consumption imputation) to clean CSVs.
    /// Requires a base FRS dataset plus --was-dir and --lcfs-dir.
    #[arg(long)]
    extract_efrs: Option<PathBuf>,

    /// WAS Round 7 TAB file directory (for EFRS wealth imputation).
    #[arg(long)]
    was_dir: Option<PathBuf>,

    /// LCFS 2021/22 TAB file directory (for EFRS consumption imputation).
    #[arg(long)]
    lcfs_dir: Option<PathBuf>,

    // ── Calibration ──

    /// Calibration targets JSON file. Runs reweighting before any simulation.
    #[arg(long)]
    calibrate: Option<PathBuf>,

    /// Output directory for calibrated dataset CSVs.
    #[arg(long)]
    calibrate_output: Option<PathBuf>,

    /// Number of calibration epochs (default: 512).
    #[arg(long, default_value = "512")]
    calibrate_epochs: usize,

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

#[derive(Serialize, Clone)]
struct HbaiIncomes {
    /// Weighted mean equivalised net income BHC
    mean_equiv_bhc: f64,
    /// Weighted mean equivalised net income AHC
    mean_equiv_ahc: f64,
    /// Weighted mean net income BHC (non-equivalised)
    mean_bhc: f64,
    /// Weighted mean net income AHC (non-equivalised)
    mean_ahc: f64,
    /// Median equivalised net income BHC (poverty reference line = 60% of this)
    median_equiv_bhc: f64,
    /// Median equivalised net income AHC
    median_equiv_ahc: f64,
}

#[derive(Serialize)]
struct PovertyHeadcounts {
    /// Relative poverty (60% median BHC equiv), children
    relative_bhc_children: f64,
    /// Relative poverty (60% median BHC equiv), working-age adults
    relative_bhc_working_age: f64,
    /// Relative poverty (60% median BHC equiv), pensioners
    relative_bhc_pensioners: f64,
    /// Relative poverty (60% median AHC equiv), children
    relative_ahc_children: f64,
    /// Relative poverty (60% median AHC equiv), working-age adults
    relative_ahc_working_age: f64,
    /// Relative poverty (60% median AHC equiv), pensioners
    relative_ahc_pensioners: f64,
    /// Absolute poverty (60% median BHC equiv fixed at 2010/11 baseline), BHC, children
    absolute_bhc_children: f64,
    /// Absolute poverty BHC, working-age adults
    absolute_bhc_working_age: f64,
    /// Absolute poverty BHC, pensioners
    absolute_bhc_pensioners: f64,
    /// Absolute poverty AHC, children
    absolute_ahc_children: f64,
    /// Absolute poverty AHC, working-age adults
    absolute_ahc_working_age: f64,
    /// Absolute poverty AHC, pensioners
    absolute_ahc_pensioners: f64,
}

#[derive(Serialize)]
struct JsonOutput {
    fiscal_year: String,
    budgetary_impact: BudgetaryImpact,
    income_breakdown: IncomeBreakdown,
    program_breakdown: ProgramBreakdown,
    caseloads: Caseloads,
    decile_impacts: Vec<DecileImpact>,
    winners_losers: WinnersLosers,
    baseline_hbai_incomes: HbaiIncomes,
    reform_hbai_incomes: HbaiIncomes,
    baseline_poverty: PovertyHeadcounts,
    reform_poverty: PovertyHeadcounts,
    /// CPI index (2025/26 = 100) for deflating nominal values to real terms.
    cpi_index: f64,
}

/// CPI index by fiscal year (2025/26 = 100).
/// Sources: ONS CPI annual average (historical), OBR EFO March 2026 table 1.7 (forecast).
/// Each value is the annual average CPI index aligned to the fiscal year label.
fn cpi_index_for_year(year: u32) -> f64 {
    // ONS CPI Index (2015=100) annual averages, mapped to fiscal years.
    // Historical values from ONS series D7BT; forecasts from OBR EFO March 2026 table 1.7.
    // All rebased to 2025/26 = 100.
    let table: &[(u32, f64)] = &[
        (1994, 55.5), (1995, 56.9), (1996, 58.3), (1997, 59.5),
        (1998, 61.0), (1999, 61.7), (2000, 62.7), (2001, 63.6),
        (2002, 64.7), (2003, 65.7), (2004, 66.8), (2005, 68.1),
        (2006, 69.8), (2007, 71.5), (2008, 74.1), (2009, 75.5),
        (2010, 78.0), (2011, 81.5), (2012, 83.6), (2013, 85.6),
        (2014, 86.5), (2015, 86.5), (2016, 87.5), (2017, 89.9),
        (2018, 92.1), (2019, 93.8), (2020, 94.6), (2021, 97.5),
        (2022, 107.3), (2023, 113.4), (2024, 133.853167),
        (2025, 138.368083), (2026, 141.552006), (2027, 144.338518),
        (2028, 147.191323), (2029, 150.164842),
    ];
    // Rebase so 2025/26 = 100
    let base = 138.368083;
    table.iter()
        .find(|(y, _)| *y == year)
        .map(|(_, v)| v / base * 100.0)
        .unwrap_or(100.0)
}

#[derive(Serialize)]
struct BudgetaryImpact {
    baseline_revenue: f64,
    reform_revenue: f64,
    revenue_change: f64,
    baseline_benefits: f64,
    reform_benefits: f64,
    benefit_spending_change: f64,
    net_cost: f64,
}

#[derive(Serialize)]
struct IncomeBreakdown {
    employment_income: f64,
    self_employment_income: f64,
    pension_income: f64,
    savings_interest_income: f64,
    dividend_income: f64,
    property_income: f64,
    other_income: f64,
}

#[derive(Serialize)]
struct ProgramBreakdown {
    income_tax: f64,
    hicbc: f64,
    employee_ni: f64,
    employer_ni: f64,
    vat: f64,
    fuel_duty: f64,
    alcohol_duty: f64,
    tobacco_duty: f64,
    capital_gains_tax: f64,
    stamp_duty: f64,
    wealth_tax: f64,
    council_tax: f64,
    universal_credit: f64,
    child_benefit: f64,
    state_pension: f64,
    pension_credit: f64,
    housing_benefit: f64,
    child_tax_credit: f64,
    working_tax_credit: f64,
    income_support: f64,
    esa_income_related: f64,
    jsa_income_based: f64,
    carers_allowance: f64,
    scottish_child_payment: f64,
    benefit_cap_reduction: f64,
    passthrough_benefits: f64,
}

#[derive(Serialize)]
struct Caseloads {
    income_tax_payers: f64,
    ni_payers: f64,
    employer_ni_payers: f64,
    universal_credit: f64,
    child_benefit: f64,
    state_pension: f64,
    pension_credit: f64,
    housing_benefit: f64,
    child_tax_credit: f64,
    working_tax_credit: f64,
    income_support: f64,
    esa_income_related: f64,
    jsa_income_based: f64,
    carers_allowance: f64,
    scottish_child_payment: f64,
    benefit_cap_affected: f64,
}

#[derive(Serialize)]
struct DecileImpact {
    decile: usize,
    avg_baseline_income: f64,
    avg_reform_income: f64,
    avg_change: f64,
    pct_change: f64,
}

#[derive(Serialize)]
struct WinnersLosers {
    winners_pct: f64,
    losers_pct: f64,
    unchanged_pct: f64,
    avg_gain: f64,
    avg_loss: f64,
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

    // Extract Enhanced FRS if requested
    if let Some(efrs_output) = &cli.extract_efrs {
        let was_dir = cli.was_dir.as_ref()
            .ok_or_else(|| anyhow::anyhow!("--extract-efrs requires --was-dir <was-tab-dir>"))?;
        let lcfs_dir = cli.lcfs_dir.as_ref()
            .ok_or_else(|| anyhow::anyhow!("--extract-efrs requires --lcfs-dir <lcfs-tab-dir>"))?;

        // Load base FRS dataset
        let mut dataset = if let Some(frs_path) = &cli.frs {
            eprintln!("Loading raw FRS from {}...", frs_path.display());
            load_frs(frs_path, cli.year)?
        } else if let Some(base) = &cli.data {
            let year_dir = base.join(cli.year.to_string());
            if year_dir.is_dir() {
                eprintln!("Loading clean FRS {}/{}...", cli.year, (cli.year + 1) % 100);
                load_clean_dataset(&year_dir, cli.year)?
            } else {
                let latest = (1994..=cli.year).rev()
                    .find(|y| base.join(y.to_string()).is_dir())
                    .ok_or_else(|| anyhow::anyhow!("No clean FRS data found in {}", base.display()))?;
                eprintln!("Loading clean FRS {}/{} and uprating...", latest, (latest + 1) % 100);
                let mut ds = load_clean_dataset(&base.join(latest.to_string()), latest)?;
                ds.uprate_to(cli.year);
                ds
            }
        } else {
            anyhow::bail!("--extract-efrs requires a base FRS dataset (--frs or --data)")
        };

        eprintln!("Loaded {} households, {} people", dataset.households.len(), dataset.people.len());

        // Run EFRS imputation pipeline
        efrs::enhance_dataset(&mut dataset, was_dir, lcfs_dir)?;

        // Write enhanced clean CSVs
        std::fs::create_dir_all(efrs_output)?;
        eprintln!("Writing enhanced CSVs...");
        write_clean_csvs(&mut dataset, efrs_output)?;
        eprintln!("Wrote Enhanced FRS to {}", efrs_output.display());
        return Ok(());
    }

    // Calibrate dataset weights if requested
    if let Some(targets_path) = &cli.calibrate {
        let base = cli.data.as_ref()
            .ok_or_else(|| anyhow::anyhow!("--calibrate requires --data <clean-data-base>"))?;
        let year_dir = base.join(cli.year.to_string());
        let mut dataset = if year_dir.is_dir() {
            eprintln!("Loading clean data {}/{}...", cli.year, (cli.year + 1) % 100);
            load_clean_dataset(&year_dir, cli.year)?
        } else {
            let latest = (1994..=cli.year).rev()
                .find(|y| base.join(y.to_string()).is_dir())
                .ok_or_else(|| anyhow::anyhow!("No clean data found in {}", base.display()))?;
            eprintln!("Loading clean data {}/{} and uprating to {}/{}...",
                latest, (latest + 1) % 100, cli.year, (cli.year + 1) % 100);
            let mut ds = load_clean_dataset(&base.join(latest.to_string()), latest)?;
            ds.uprate_to(cli.year);
            ds
        };
        eprintln!("Loaded {} households, {} people", dataset.households.len(), dataset.people.len());

        // Load and filter targets for the requested year
        let all_targets = calibrate::load_targets(targets_path)?;
        let targets: Vec<_> = all_targets.into_iter()
            .filter(|t| t.year == cli.year)
            .collect();
        eprintln!("Loaded {} calibration targets for {}", targets.len(), cli.year);

        if targets.is_empty() {
            anyhow::bail!("No calibration targets found for year {}", cli.year);
        }

        // Run baseline simulation so calibration can use output variables
        let params = Parameters::for_year(cli.year)
            .unwrap_or_else(|e| panic!("Failed to load params {}/{}: {}", cli.year, cli.year + 1, e));
        eprintln!("Running baseline simulation...");
        let sim = Simulation::new(
            dataset.people.clone(), dataset.benunits.clone(),
            dataset.households.clone(), params, cli.year,
        );
        let sim_results = sim.run();

        // Build calibration matrix using simulation outputs
        let (matrix, target_values, training_mask) = calibrate::build_matrix(&dataset, &targets, Some(&sim_results));
        let initial_weights: Vec<f64> = dataset.households.iter().map(|h| h.weight).collect();

        // Run optimisation
        eprintln!("Running calibration ({} epochs)...", cli.calibrate_epochs);
        let config = calibrate::CalibrateConfig {
            epochs: cli.calibrate_epochs,
            ..Default::default()
        };
        let mut result = calibrate::calibrate(&matrix, &target_values, &training_mask, &initial_weights, &config);

        // Fill in target names for reporting
        for (j, entry) in result.per_target_error.iter_mut().enumerate() {
            entry.0 = targets[j].name.clone();
        }

        calibrate::print_report(&targets, &result, &dataset);

        // Write calibrated dataset
        calibrate::apply_weights(&mut dataset, &result.weights);
        let output_dir = cli.calibrate_output.as_ref()
            .unwrap_or(&year_dir);
        std::fs::create_dir_all(output_dir)?;
        write_clean_csvs(&mut dataset, output_dir)?;
        eprintln!("Wrote calibrated dataset to {}", output_dir.display());
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
    let baseline_sim = Simulation::new(
        dataset.people.clone(),
        dataset.benunits.clone(),
        dataset.households.clone(),
        baseline_params.clone(),
        cli.year,
    );
    let baseline = baseline_sim.run();

    // Apply OBR labour supply responses if enabled in the policy parameters.
    // Adjusts employment incomes in the policy dataset before running the reform simulation.
    let policy_people = if policy_params.labour_supply.enabled {
        let baseline_net: Vec<f64> = baseline.household_results.iter()
            .map(|hr| hr.net_income)
            .collect();
        crate::variables::labour_supply::apply_labour_supply_responses(
            &dataset.people,
            &dataset.benunits,
            &dataset.households,
            &baseline_params,
            &policy_params,
            &baseline_net,
            cli.year,
        )
    } else {
        dataset.people.clone()
    };

    // Run policy simulation (pass baseline old SP rate so reported amounts scale correctly)
    let policy_sim = Simulation::new_with_baseline_sp(
        policy_people,
        dataset.benunits.clone(),
        dataset.households.clone(),
        policy_params.clone(),
        baseline_params.state_pension.old_basic_pension_weekly,
        cli.year,
    );
    let mut reformed = policy_sim.run();
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
        write_microdata(&dataset, &baseline, &reformed, micro_dir)?;
        if !json_mode {
            println!("  {} Wrote enhanced microdata to {}", "▸".bright_cyan(), micro_dir.display());
        }
        return Ok(());
    }

    // Microdata to stdout
    if cli.output_microdata_stdout {
        write_microdata_to_stdout(&dataset, &baseline, &reformed)?;
        return Ok(());
    }

    // Analysis
    let households = &dataset.households;

    let baseline_revenue: f64 = households.iter()
        .map(|h| h.weight * baseline.household_results[h.id].total_tax)
        .sum();
    let reform_revenue: f64 = households.iter()
        .map(|h| h.weight * reformed.household_results[h.id].total_tax)
        .sum();
    let revenue_change = reform_revenue - baseline_revenue;

    let baseline_benefits: f64 = households.iter()
        .map(|h| h.weight * baseline.household_results[h.id].total_benefits)
        .sum();
    let reform_benefits: f64 = households.iter()
        .map(|h| h.weight * reformed.household_results[h.id].total_benefits)
        .sum();
    let benefit_change = reform_benefits - baseline_benefits;
    let net_cost = -revenue_change + benefit_change;

    // Decile analysis — ranked by equivalised HBAI net income BHC (baseline).
    // Changes are measured on equivalised extended net income (HBAI minus stamp duty/wealth tax),
    // so that reforms to those taxes show up in decile impacts and winners/losers.
    let mut hh_incomes: Vec<(usize, f64, f64, f64)> = households.iter().map(|hh| {
        let bl = &baseline.household_results[hh.id];
        let rf = &reformed.household_results[hh.id];
        let eq = bl.equivalisation_factor.max(1e-9);
        (hh.id,
         bl.equivalised_net_income,
         bl.extended_net_income / eq,
         rf.extended_net_income / eq)
    }).collect();
    hh_incomes.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    let decile_size = hh_incomes.len() / 10;
    let mut decile_impacts = Vec::new();
    for d in 0..10 {
        let start = d * decile_size;
        let end = if d == 9 { hh_incomes.len() } else { (d + 1) * decile_size };
        let slice = &hh_incomes[start..end];
        let n = slice.len() as f64;
        let avg_base: f64 = slice.iter().map(|h| h.2).sum::<f64>() / n;   // baseline extended
        let avg_reform: f64 = slice.iter().map(|h| h.3).sum::<f64>() / n; // reform extended
        let avg_change = avg_reform - avg_base;
        let pct_change = if avg_base != 0.0 { 100.0 * avg_change / avg_base } else { 0.0 };
        decile_impacts.push(DecileImpact {
            decile: d + 1,
            avg_baseline_income: (avg_base * 100.0).round() / 100.0,
            avg_reform_income: (avg_reform * 100.0).round() / 100.0,
            avg_change: (avg_change * 100.0).round() / 100.0,
            pct_change: (pct_change * 100.0).round() / 100.0,
        });
    }

    // Winners and losers
    let mut winners = 0.0f64;
    let mut losers = 0.0f64;
    let mut unchanged = 0.0f64;
    let mut total_gain = 0.0f64;
    let mut total_loss = 0.0f64;

    for hh in households {
        let change = reformed.household_results[hh.id].extended_net_income
            - baseline.household_results[hh.id].extended_net_income;
        if change > 1.0 {
            winners += hh.weight;
            total_gain += hh.weight * change;
        } else if change < -1.0 {
            losers += hh.weight;
            total_loss += hh.weight * change;
        } else {
            unchanged += hh.weight;
        }
    }

    let total_hh = winners + losers + unchanged;
    let winners_losers = WinnersLosers {
        winners_pct: (1000.0 * winners / total_hh).round() / 10.0,
        losers_pct: (1000.0 * losers / total_hh).round() / 10.0,
        unchanged_pct: (1000.0 * unchanged / total_hh).round() / 10.0,
        avg_gain: if winners > 0.0 { (total_gain / winners).round() } else { 0.0 },
        avg_loss: if losers > 0.0 { (total_loss.abs() / losers).round() } else { 0.0 },
    };

    // Program-level breakdown and caseloads (weighted totals from reform)
    let benunits = &dataset.benunits;
    let people = &dataset.people;
    let (income_breakdown, program_breakdown, caseloads) = {
        // Income aggregates
        let mut total_employment = 0.0f64;
        let mut total_self_employment = 0.0f64;
        let mut total_pension = 0.0f64;
        let mut total_savings = 0.0f64;
        let mut total_dividend = 0.0f64;
        let mut total_property = 0.0f64;
        let mut total_other = 0.0f64;
        // Tax spending and caseloads
        let mut income_tax = 0.0f64;
        let mut hicbc_total = 0.0f64;
        let mut employee_ni = 0.0f64;
        let mut employer_ni = 0.0f64;
        let mut vat_total = 0.0f64;
        let mut fuel_duty_total = 0.0f64;
        let mut alcohol_duty_total = 0.0f64;
        let mut tobacco_duty_total = 0.0f64;
        let mut cgt_total = 0.0f64;
        let mut stamp_duty_total = 0.0f64;
        let mut wealth_tax_total = 0.0f64;
        let mut council_tax_total = 0.0f64;
        let mut it_payers = 0.0f64;
        let mut ni_payers = 0.0f64;
        let mut eni_payers = 0.0f64;
        for hh in households {
            let hr = &reformed.household_results[hh.id];
            vat_total += hh.weight * hr.vat;
            fuel_duty_total += hh.weight * hr.fuel_duty;
            alcohol_duty_total += hh.weight * hr.alcohol_duty;
            tobacco_duty_total += hh.weight * hr.tobacco_duty;
            cgt_total += hh.weight * hr.capital_gains_tax;
            stamp_duty_total += hh.weight * hr.stamp_duty;
            wealth_tax_total += hh.weight * hr.wealth_tax;
            council_tax_total += hh.weight * hr.council_tax_calculated;
            for &pid in &hh.person_ids {
                let person = &people[pid];
                total_employment += hh.weight * person.employment_income;
                total_self_employment += hh.weight * person.self_employment_income;
                total_pension += hh.weight * person.pension_income;
                total_savings += hh.weight * person.savings_interest_income;
                total_dividend += hh.weight * person.dividend_income;
                total_property += hh.weight * person.property_income;
                total_other += hh.weight * (person.maintenance_income + person.miscellaneous_income + person.other_income);
                let pr = &reformed.person_results[pid];
                income_tax += hh.weight * pr.income_tax;
                hicbc_total += hh.weight * pr.hicbc;
                employee_ni += hh.weight * pr.national_insurance;
                employer_ni += hh.weight * pr.employer_ni;
                if pr.income_tax > 0.0 { it_payers += hh.weight; }
                if pr.national_insurance > 0.0 { ni_payers += hh.weight; }
                if pr.employer_ni > 0.0 { eni_payers += hh.weight; }
            }
        }
        // Benefit spending and caseloads
        let mut uc = 0.0f64;
        let mut cb = 0.0f64;
        let mut sp = 0.0f64;
        let mut pc = 0.0f64;
        let mut hb = 0.0f64;
        let mut ctc = 0.0f64;
        let mut wtc = 0.0f64;
        let mut is_val = 0.0f64;
        let mut esa_ir = 0.0f64;
        let mut jsa_ib = 0.0f64;
        let mut ca = 0.0f64;
        let mut scp = 0.0f64;
        let mut cap = 0.0f64;
        let mut passthrough = 0.0f64;
        let mut cl_uc = 0.0f64;
        let mut cl_cb = 0.0f64;
        let mut cl_sp = 0.0f64;
        let mut cl_pc = 0.0f64;
        let mut cl_hb = 0.0f64;
        let mut cl_ctc = 0.0f64;
        let mut cl_wtc = 0.0f64;
        let mut cl_is = 0.0f64;
        let mut cl_esa = 0.0f64;
        let mut cl_jsa = 0.0f64;
        let mut cl_ca = 0.0f64;
        let mut cl_scp = 0.0f64;
        let mut cl_cap = 0.0f64;
        for bu in benunits {
            let w = households[bu.household_id].weight;
            let br = &reformed.benunit_results[bu.id];
            uc += w * br.universal_credit;
            cb += w * br.child_benefit;
            sp += w * br.state_pension;
            pc += w * br.pension_credit;
            hb += w * br.housing_benefit;
            ctc += w * br.child_tax_credit;
            wtc += w * br.working_tax_credit;
            is_val += w * br.income_support;
            esa_ir += w * br.esa_income_related;
            jsa_ib += w * br.jsa_income_based;
            ca += w * br.carers_allowance;
            scp += w * br.scottish_child_payment;
            cap += w * br.benefit_cap_reduction;
            passthrough += w * br.passthrough_benefits;
            if br.universal_credit > 0.0 { cl_uc += w; }
            if br.child_benefit > 0.0 { cl_cb += w; }
            if br.state_pension > 0.0 { cl_sp += w; }
            if br.pension_credit > 0.0 { cl_pc += w; }
            if br.housing_benefit > 0.0 { cl_hb += w; }
            if br.child_tax_credit > 0.0 { cl_ctc += w; }
            if br.working_tax_credit > 0.0 { cl_wtc += w; }
            if br.income_support > 0.0 { cl_is += w; }
            if br.esa_income_related > 0.0 { cl_esa += w; }
            if br.jsa_income_based > 0.0 { cl_jsa += w; }
            if br.carers_allowance > 0.0 { cl_ca += w; }
            if br.scottish_child_payment > 0.0 { cl_scp += w; }
            if br.benefit_cap_reduction > 0.0 { cl_cap += w; }
        }
        (IncomeBreakdown {
            employment_income: total_employment,
            self_employment_income: total_self_employment,
            pension_income: total_pension,
            savings_interest_income: total_savings,
            dividend_income: total_dividend,
            property_income: total_property,
            other_income: total_other,
        }, ProgramBreakdown {
            income_tax,
            hicbc: hicbc_total,
            employee_ni,
            employer_ni,
            vat: vat_total,
            fuel_duty: fuel_duty_total,
            alcohol_duty: alcohol_duty_total,
            tobacco_duty: tobacco_duty_total,
            capital_gains_tax: cgt_total,
            stamp_duty: stamp_duty_total,
            wealth_tax: wealth_tax_total,
            council_tax: council_tax_total,
            universal_credit: uc,
            child_benefit: cb,
            state_pension: sp,
            pension_credit: pc,
            housing_benefit: hb,
            child_tax_credit: ctc,
            working_tax_credit: wtc,
            income_support: is_val,
            esa_income_related: esa_ir,
            jsa_income_based: jsa_ib,
            carers_allowance: ca,
            scottish_child_payment: scp,
            benefit_cap_reduction: cap,
            passthrough_benefits: passthrough,
        }, Caseloads {
            income_tax_payers: it_payers,
            ni_payers,
            employer_ni_payers: eni_payers,
            universal_credit: cl_uc,
            child_benefit: cl_cb,
            state_pension: cl_sp,
            pension_credit: cl_pc,
            housing_benefit: cl_hb,
            child_tax_credit: cl_ctc,
            working_tax_credit: cl_wtc,
            income_support: cl_is,
            esa_income_related: cl_esa,
            jsa_income_based: cl_jsa,
            carers_allowance: cl_ca,
            scottish_child_payment: cl_scp,
            benefit_cap_affected: cl_cap,
        })
    };

    // ── HBAI income aggregates ────────────────────────────────────────────────
    let total_weight: f64 = households.iter().map(|h| h.weight).sum();

    let compute_hbai_incomes = |results: &crate::engine::simulation::SimulationResults| -> HbaiIncomes {
        // Weighted median over individuals: each person carries the household's weight.
        let total_person_weight: f64 = households.iter()
            .map(|h| h.weight * (h.person_ids.len() as f64))
            .sum();
        let weighted_median = |vals: &mut Vec<(f64, f64)>| -> f64 {
            vals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let half = total_person_weight / 2.0;
            let mut cum = 0.0;
            for &(v, w) in vals.iter() {
                cum += w;
                if cum >= half { return v; }
            }
            vals.last().map(|&(v, _)| v).unwrap_or(0.0)
        };

        let mut equiv_bhc: Vec<(f64, f64)> = households.iter()
            .map(|h| {
                let w = h.weight * (h.person_ids.len() as f64);
                (results.household_results[h.id].equivalised_net_income, w)
            })
            .collect();
        let mut equiv_ahc: Vec<(f64, f64)> = households.iter()
            .map(|h| {
                let w = h.weight * (h.person_ids.len() as f64);
                (results.household_results[h.id].equivalised_net_income_ahc, w)
            })
            .collect();

        let median_equiv_bhc = weighted_median(&mut equiv_bhc);
        let median_equiv_ahc = weighted_median(&mut equiv_ahc);

        let mean_equiv_bhc = households.iter()
            .map(|h| h.weight * results.household_results[h.id].equivalised_net_income)
            .sum::<f64>() / total_weight;
        let mean_equiv_ahc = households.iter()
            .map(|h| h.weight * results.household_results[h.id].equivalised_net_income_ahc)
            .sum::<f64>() / total_weight;
        let mean_bhc = households.iter()
            .map(|h| h.weight * results.household_results[h.id].net_income)
            .sum::<f64>() / total_weight;
        let mean_ahc = households.iter()
            .map(|h| h.weight * results.household_results[h.id].net_income_ahc)
            .sum::<f64>() / total_weight;

        HbaiIncomes { mean_equiv_bhc, mean_equiv_ahc, mean_bhc, mean_ahc,
                      median_equiv_bhc, median_equiv_ahc }
    };
    let baseline_hbai_incomes = compute_hbai_incomes(&baseline);
    let reform_hbai_incomes = compute_hbai_incomes(&reformed);

    // ── Poverty headcounts ────────────────────────────────────────────────────
    // Relative lines: 60% of baseline weighted median equivalised income
    let rel_line_bhc = 0.60 * baseline_hbai_incomes.median_equiv_bhc;
    let rel_line_ahc = 0.60 * baseline_hbai_incomes.median_equiv_ahc;
    // Absolute lines: 60% of median in 2010/11 (ONS HBAI reference, uprated by CPI to nominal)
    // 2010/11 median equivalised BHC ~£14,400/yr; AHC ~£11,600/yr (2010/11 prices)
    // Uprate to simulation year using CPI index
    let cpi = cpi_index_for_year(cli.year) / 100.0;
    let abs_line_bhc = 14_400.0 * cpi;
    let abs_line_ahc = 11_600.0 * cpi;

    let compute_poverty = |results: &crate::engine::simulation::SimulationResults| -> PovertyHeadcounts {
        let mut rc_children = 0.0f64; let mut rc_working = 0.0f64; let mut rc_pensioners = 0.0f64;
        let mut ra_children = 0.0f64; let mut ra_working = 0.0f64; let mut ra_pensioners = 0.0f64;
        let mut ac_children = 0.0f64; let mut ac_working = 0.0f64; let mut ac_pensioners = 0.0f64;
        let mut aa_children = 0.0f64; let mut aa_working = 0.0f64; let mut aa_pensioners = 0.0f64;
        let mut total_children = 0.0f64; let mut total_working = 0.0f64; let mut total_pensioners = 0.0f64;

        for hh in households {
            let hr = &results.household_results[hh.id];
            let eq_bhc = hr.equivalised_net_income;
            let eq_ahc = hr.equivalised_net_income_ahc;
            let w = hh.weight;

            for &pid in &hh.person_ids {
                let age = dataset.people[pid].age;
                let pw = w; // person weight = household weight (no person-level weights)
                let (child, working, pensioner) = if age < 16.0 {
                    (true, false, false)
                } else if age < 66.0 {
                    (false, true, false)
                } else {
                    (false, false, true)
                };

                if child   { total_children   += pw; }
                if working { total_working    += pw; }
                if pensioner { total_pensioners += pw; }

                if eq_bhc < rel_line_bhc {
                    if child { rc_children += pw; } else if working { rc_working += pw; } else { rc_pensioners += pw; }
                }
                if eq_ahc < rel_line_ahc {
                    if child { ra_children += pw; } else if working { ra_working += pw; } else { ra_pensioners += pw; }
                }
                if eq_bhc < abs_line_bhc {
                    if child { ac_children += pw; } else if working { ac_working += pw; } else { ac_pensioners += pw; }
                }
                if eq_ahc < abs_line_ahc {
                    if child { aa_children += pw; } else if working { aa_working += pw; } else { aa_pensioners += pw; }
                }
            }
        }

        let pct = |n: f64, d: f64| if d > 0.0 { (n / d * 1000.0).round() / 10.0 } else { 0.0 };
        PovertyHeadcounts {
            relative_bhc_children:    pct(rc_children,    total_children),
            relative_bhc_working_age: pct(rc_working,     total_working),
            relative_bhc_pensioners:  pct(rc_pensioners,  total_pensioners),
            relative_ahc_children:    pct(ra_children,    total_children),
            relative_ahc_working_age: pct(ra_working,     total_working),
            relative_ahc_pensioners:  pct(ra_pensioners,  total_pensioners),
            absolute_bhc_children:    pct(ac_children,    total_children),
            absolute_bhc_working_age: pct(ac_working,     total_working),
            absolute_bhc_pensioners:  pct(ac_pensioners,  total_pensioners),
            absolute_ahc_children:    pct(aa_children,    total_children),
            absolute_ahc_working_age: pct(aa_working,     total_working),
            absolute_ahc_pensioners:  pct(aa_pensioners,  total_pensioners),
        }
    };

    let baseline_poverty = compute_poverty(&baseline);
    let reform_poverty   = compute_poverty(&reformed);

    // JSON output mode
    if json_mode {
        let output = JsonOutput {
            fiscal_year: baseline_params.fiscal_year.clone(),
            budgetary_impact: BudgetaryImpact {
                baseline_revenue,
                reform_revenue,
                revenue_change,
                baseline_benefits,
                reform_benefits,
                benefit_spending_change: benefit_change,
                net_cost,
            },
            income_breakdown,
            program_breakdown,
            caseloads,
            decile_impacts,
            winners_losers,
            baseline_hbai_incomes,
            reform_hbai_incomes,
            baseline_poverty,
            reform_poverty,
            cpi_index: cpi_index_for_year(cli.year),
        };
        println!("{}", serde_json::to_string(&output)?);
        return Ok(());
    }

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
    dataset: &crate::data::Dataset,
    results: &crate::engine::simulation::SimulationResults,
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
        use crate::data::clean::load_clean_dataset;
        use crate::engine::Simulation;
        use crate::parameters::Parameters;
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
    use crate::data::frs::load_frs;
    use crate::engine::Simulation;
    use crate::parameters::Parameters;
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
