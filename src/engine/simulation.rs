use rayon::prelude::*;
use crate::engine::entities::*;
use crate::parameters::Parameters;
use crate::variables;

/// Results for a single person
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct PersonResult {
    pub income_tax: f64,
    /// Sum of `ni_class1_employee + ni_class2 + ni_class4`. Kept for back-compat
    /// with consumers that don't care about the per-class split.
    pub national_insurance: f64,
    /// Class 1 primary (employee) — SSCBA 1992 s.6, on employment_income between
    /// the primary threshold and UEL at the main rate, above UEL at the additional rate.
    pub ni_class1_employee: f64,
    /// Class 2 — SSCBA 1992 s.11, flat-weekly self-employed contribution above the
    /// small-profits threshold. Abolished from 2024/25 (rate defaults to zero).
    pub ni_class2: f64,
    /// Class 4 — SSCBA 1992 s.15, profit-based self-employed contribution.
    pub ni_class4: f64,
    /// Class 1 secondary (employer).
    pub employer_ni: f64,
    pub total_income: f64,
    pub taxable_income: f64,
    pub personal_allowance: f64,
    pub adjusted_net_income: f64,
    pub unused_personal_allowance: f64,
    pub marriage_allowance_deduction: f64,
    /// High Income Child Benefit Charge — income tax charge on the highest
    /// earner in a benefit unit receiving child benefit.
    pub hicbc: f64,
    /// Capital gains tax (proxied from investment income).
    pub capital_gains_tax: f64,
}

/// Results for a benefit unit
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct BenUnitResult {
    pub universal_credit: f64,
    pub child_benefit: f64,
    pub state_pension: f64,
    pub pension_credit: f64,
    pub housing_benefit: f64,
    pub child_tax_credit: f64,
    pub working_tax_credit: f64,
    pub income_support: f64,
    pub esa_income_related: f64,
    pub jsa_income_based: f64,
    pub carers_allowance: f64,
    pub scottish_child_payment: f64,
    pub benefit_cap_reduction: f64,
    /// Passthrough reported benefits not modelled (PIP, DLA, AA, ESA-C, JSA-C)
    pub passthrough_benefits: f64,
    pub total_benefits: f64,
    pub uc_max_amount: f64,
    pub uc_income_reduction: f64,
}

/// Results for a household
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct HouseholdResult {
    pub net_income: f64,
    pub total_tax: f64,
    pub total_benefits: f64,
    pub gross_income: f64,
    /// VAT paid by the household (estimated from consumption or disposable income)
    pub vat: f64,
    /// Fuel duty on petrol and diesel
    pub fuel_duty: f64,
    /// Alcohol duty
    pub alcohol_duty: f64,
    /// Tobacco duty
    pub tobacco_duty: f64,
    /// Capital gains tax (aggregated from persons in this household)
    pub capital_gains_tax: f64,
    /// Stamp duty land tax (annualised)
    pub stamp_duty: f64,
    /// Annual wealth tax (hypothetical)
    pub wealth_tax: f64,
    /// Council tax (calculated from parameters, for reform modelling)
    pub council_tax_calculated: f64,
    /// Modified OECD equivalisation factor for the household
    pub equivalisation_factor: f64,
    /// HBAI net income BHC (before housing costs)
    pub equivalised_net_income: f64,
    /// HBAI net income AHC (after housing costs = BHC - rent - council tax)
    pub net_income_ahc: f64,
    /// HBAI equivalised net income AHC
    pub equivalised_net_income_ahc: f64,
    /// Extended net income: HBAI net income minus stamp duty and wealth tax.
    /// Used for decile impacts and winners/losers so that SDLT/wealth tax reforms show up.
    pub extended_net_income: f64,
}

/// Complete simulation result set
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SimulationResults {
    pub person_results: Vec<PersonResult>,
    pub benunit_results: Vec<BenUnitResult>,
    pub household_results: Vec<HouseholdResult>,
}

/// The microsimulation engine.
pub struct Simulation {
    pub people: Vec<Person>,
    pub benunits: Vec<BenUnit>,
    pub households: Vec<Household>,
    pub parameters: Parameters,
    /// Fiscal year (e.g. 2025 for 2025/26) — used for new/basic SP cutoff.
    pub fiscal_year: u32,
}

impl Simulation {
    pub fn new(
        people: Vec<Person>,
        benunits: Vec<BenUnit>,
        households: Vec<Household>,
        parameters: Parameters,
        fiscal_year: u32,
    ) -> Self {
        Simulation {
            people, benunits, households, parameters,
            fiscal_year,
        }
    }

    /// Run the full simulation. Calculates all tax-benefit variables for every entity.
    /// Uses Rayon for parallel computation across households.
    pub fn run(&self) -> SimulationResults {
        let mut person_results = vec![PersonResult::default(); self.people.len()];
        let mut benunit_results = vec![BenUnitResult::default(); self.benunits.len()];
        let mut household_results = vec![HouseholdResult::default(); self.households.len()];

        // Phase 1a: Calculate each person's state pension under the current policy.
        // State pension is taxable income so must be computed before income tax.
        let fiscal_year = self.fiscal_year;
        let person_sp: Vec<f64> = self.people.par_iter().map(|p| {
            variables::benefits::person_state_pension(p, &self.parameters, fiscal_year)
        }).collect();

        // Phase 1b: Person-level tax calculations (parallelised).
        // Income tax receives the calculated SP amount so reforms flow through correctly.
        let pr: Vec<PersonResult> = self.people.par_iter().enumerate().map(|(i, person)| {
            variables::income_tax::calculate(person, &self.parameters, person_sp[i])
        }).collect();
        person_results = pr;

        // Phase 1c: Marriage allowance (benunit-level adjustment to person tax)
        // Cannot be parallelised as it mutates person_results across benunits
        for bu in &self.benunits {
            variables::income_tax::apply_marriage_allowance(
                bu, &self.people, &mut person_results, &self.parameters,
            );
        }

        // Phase 2: BenUnit-level calculations (parallelised)
        let br: Vec<BenUnitResult> = self.benunits.par_iter().map(|bu| {
            let hh = &self.households[bu.household_id];
            variables::benefits::calculate_benunit(
                bu, &self.people, &person_results, hh, &self.parameters,
                fiscal_year,
            )
        }).collect();
        benunit_results = br;

        // Phase 2b: HICBC — the highest earner in each benunit pays back child
        // benefit as an income tax charge, tapered between hicbc_threshold and
        // hicbc_taper_end based on adjusted net income.
        for bu in &self.benunits {
            let cb = benunit_results[bu.id].child_benefit;
            if cb <= 0.0 { continue; }

            let threshold = self.parameters.child_benefit.hicbc_threshold;
            let taper_end = self.parameters.child_benefit.hicbc_taper_end;

            // Find the highest earner among adults
            let highest_pid = bu.person_ids.iter()
                .copied()
                .filter(|&pid| self.people[pid].is_adult())
                .max_by(|&a, &b| {
                    person_results[a].adjusted_net_income
                        .partial_cmp(&person_results[b].adjusted_net_income)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

            if let Some(pid) = highest_pid {
                let ani = person_results[pid].adjusted_net_income;
                let charge = if ani <= threshold {
                    0.0
                } else if ani >= taper_end {
                    cb
                } else {
                    let fraction = (ani - threshold) / (taper_end - threshold);
                    cb * fraction
                };
                if charge > 0.0 {
                    person_results[pid].hicbc = charge;
                    person_results[pid].income_tax += charge;
                }
            }
        }

        // Phase 2c: Capital gains tax (person-level, needs income tax band info)
        if let Some(ref cgt_params) = self.parameters.capital_gains_tax {
            for person in &self.people {
                // Higher/additional rate if taxable income exceeds basic rate limit
                let basic_rate_limit = self.parameters.income_tax.uk_brackets
                    .get(1)
                    .map(|b| b.threshold)
                    .unwrap_or(37700.0);
                let is_higher = person_results[person.id].taxable_income > basic_rate_limit;
                let cgt = variables::wealth_taxes::calculate_capital_gains_tax(
                    person, cgt_params, is_higher,
                );
                person_results[person.id].capital_gains_tax = cgt;
            }
        }

        // Phase 3: Household-level aggregation (parallelised)
        let hr: Vec<HouseholdResult> = self.households.par_iter().map(|hh| {
            // Gross income uses calculated SP (from Phase 1a) instead of reported amounts,
            // so SP reforms flow through to gross/net income correctly.
            let gross: f64 = hh.person_ids.iter()
                .map(|&pid| {
                    person_results[pid].total_income
                })
                .sum::<f64>();

            let calculated_sp: f64 = hh.person_ids.iter()
                .map(|&pid| person_sp[pid])
                .sum();

            let direct_tax: f64 = hh.person_ids.iter()
                .map(|&pid| person_results[pid].income_tax + person_results[pid].national_insurance)
                .sum();

            let total_benefits: f64 = hh.benunit_ids.iter()
                .map(|&bid| benunit_results[bid].total_benefits)
                .sum();

            // State pension is already in gross (adjusted above) so exclude
            // it from benefits when computing net income to avoid double-counting.
            let state_pension: f64 = calculated_sp;

            // Pension contributions are deducted from net income (as in FRS NINDINC/HBAI)
            let pension_contributions: f64 = hh.person_ids.iter()
                .map(|&pid| self.people[pid].employee_pension_contributions + self.people[pid].personal_pension_contributions)
                .sum();

            // In-kind benefits included in HBAI net income
            let in_kind_benefits: f64 = hh.benunit_ids.iter()
                .map(|&bid| {
                    let bu = &self.benunits[bid];
                    bu.free_school_meals + bu.free_school_fruit_veg + bu.free_school_milk
                        + bu.healthy_start_vouchers + bu.free_tv_licence
                })
                .sum();

            let net_income_before_vat = gross - direct_tax - pension_contributions
                + total_benefits - state_pension + in_kind_benefits;

            // VAT: computed from consumption data (EFRS) or estimated from disposable income
            let vat = variables::vat::calculate_household_vat(
                hh, net_income_before_vat, &self.parameters,
            );

            // Fuel duty
            let fuel_duty = self.parameters.fuel_duty.as_ref()
                .map(|p| variables::consumption_taxes::calculate_fuel_duty(hh, p))
                .unwrap_or(0.0);

            // Alcohol duty
            let alcohol_duty = self.parameters.alcohol_duty.as_ref()
                .map(|p| variables::consumption_taxes::calculate_alcohol_duty(hh, p))
                .unwrap_or(0.0);

            // Tobacco duty
            let tobacco_duty = self.parameters.tobacco_duty.as_ref()
                .map(|p| variables::consumption_taxes::calculate_tobacco_duty(hh, p))
                .unwrap_or(0.0);

            // Capital gains tax (aggregated from persons)
            let cgt: f64 = hh.person_ids.iter()
                .map(|&pid| person_results[pid].capital_gains_tax)
                .sum();

            // Property transaction tax (annualised): SDLT in England/NI, LBTT in
            // Scotland, LTT in Wales. Stored on the household result as
            // `stamp_duty` for backwards compatibility.
            let stamp_duty = variables::wealth_taxes::calculate_property_transaction_tax(
                hh,
                self.parameters.stamp_duty.as_ref(),
                self.parameters.lbtt.as_ref(),
                self.parameters.ltt.as_ref(),
            );

            // Wealth tax
            let wealth_tax = self.parameters.wealth_tax.as_ref()
                .map(|p| variables::wealth_taxes::calculate_wealth_tax(hh, p))
                .unwrap_or(0.0);

            // Council tax (calculated from parameters for reform modelling).
            // Applies the single-person discount when the household has exactly
            // one adult (18+) — Local Government Finance Act 1992 s.11(1)(a).
            let adult_count = hh.person_ids.iter()
                .filter(|&&pid| self.people[pid].is_adult())
                .count();
            let council_tax_calculated = self.parameters.council_tax.as_ref()
                .map(|p| variables::wealth_taxes::calculate_council_tax(hh, p, adult_count == 1))
                .unwrap_or(hh.council_tax);

            let total_tax = direct_tax + vat + fuel_duty + alcohol_duty + tobacco_duty
                + cgt + stamp_duty + wealth_tax;
            // HBAI net income: gross minus direct taxes and pension contributions, plus benefits.
            // Excludes indirect taxes (VAT, duties) and transaction/wealth taxes (SDLT, wealth tax)
            // to match the government HBAI definition used for poverty and distributional analysis.
            let net_income = net_income_before_vat;
            // Extended net income: subtracts indirect and wealth taxes for fiscal analysis.
            let extended_net_income = net_income_before_vat - vat - stamp_duty - wealth_tax;

            // Modified OECD equivalisation scale (used by HBAI):
            // First adult: 0.67, additional adults (14+): 0.33, children (<14): 0.20
            let mut adults = 0usize;
            let mut children = 0usize;
            for &pid in &hh.person_ids {
                if self.people[pid].age >= 14.0 {
                    adults += 1;
                } else {
                    children += 1;
                }
            }
            let eq_factor = if adults == 0 { 1.0 } else {
                0.67 + (adults.saturating_sub(1) as f64) * 0.33 + (children as f64) * 0.20
            };

            // AHC: subtract rent and council tax (housing costs), using HBAI net income
            let housing_costs = hh.rent + hh.council_tax;
            let net_income_ahc = net_income - housing_costs;

            HouseholdResult {
                gross_income: gross,
                total_tax,
                total_benefits,
                net_income,
                vat,
                fuel_duty,
                alcohol_duty,
                tobacco_duty,
                capital_gains_tax: cgt,
                stamp_duty,
                wealth_tax,
                council_tax_calculated,
                equivalisation_factor: eq_factor,
                equivalised_net_income: net_income / eq_factor,
                net_income_ahc,
                equivalised_net_income_ahc: net_income_ahc / eq_factor,
                extended_net_income,
            }
        }).collect();
        household_results = hr;

        SimulationResults {
            person_results,
            benunit_results,
            household_results,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::entities::{Person, BenUnit, Household, Region};
    use crate::variables::labour_supply::apply_labour_supply_responses;

    /// Build a minimal one-person, one-benunit, one-household dataset.
    fn make_worker(income: f64, age: f64, gender: crate::engine::entities::Gender) -> (Vec<Person>, Vec<BenUnit>, Vec<Household>) {
        let mut person = Person::default();
        person.id = 0;
        person.benunit_id = 0;
        person.household_id = 0;
        person.age = age;
        person.gender = gender;
        person.employment_income = income;
        person.hours_worked = 37.5 * 52.0;
        person.emp_status = 2; // full-time employee (FRS EMPSTATB=2)

        let bu = BenUnit {
            id: 0, household_id: 0, person_ids: vec![0],
            ..BenUnit::default()
        };
        let hh = Household {
            id: 0, benunit_ids: vec![0], person_ids: vec![0],
            weight: 1.0, region: Region::London,
            ..Household::default()
        };
        (vec![person], vec![bu], vec![hh])
    }

    /// A basic-rate income tax cut should raise the marginal net wage → positive
    /// substitution effect → employment income increases.
    #[test]
    fn labour_supply_responds_to_income_tax_cut() {
        let baseline_params = crate::parameters::Parameters::for_year(2025).unwrap();
        let mut policy_params = baseline_params.clone();
        // Cut basic rate from 20% to 15% — lowers marginal effective tax rate, raises net wage
        policy_params.income_tax.uk_brackets[0].rate = 0.15;

        let income = 30_000.0;
        let (people, benunits, households) = make_worker(
            income, 35.0, crate::engine::entities::Gender::Male,
        );

        // Baseline household net income
        let baseline_sim = Simulation::new(
            people.clone(), benunits.clone(), households.clone(),
            baseline_params.clone(), 2025,
        );
        let baseline_results = baseline_sim.run();
        let baseline_net: Vec<f64> = baseline_results.household_results.iter()
            .map(|hr| hr.net_income).collect();

        let adjusted_people = apply_labour_supply_responses(
            &people, &benunits, &households,
            &baseline_params, &policy_params,
            &baseline_net, 2025,
        );

        let delta_e = adjusted_people[0].employment_income - people[0].employment_income;

        // With a lower marginal rate, substitution effect is positive → more work
        assert!(
            delta_e > 0.0,
            "Expected positive labour supply response to basic rate cut, got ΔE = {:.2}",
            delta_e
        );
        // Should be small but meaningful: roughly 0.15 * 0.5% * 30k ≈ £22
        // (0.05pp rate reduction × substitution elasticity 0.15)
        assert!(
            delta_e < income * 0.05,
            "Labour supply response implausibly large: ΔE = {:.2} on income {:.2}",
            delta_e, income
        );
    }

    /// With labour_supply.enabled = false, employment income must be unchanged.
    #[test]
    fn labour_supply_disabled_is_static() {
        let baseline_params = crate::parameters::Parameters::for_year(2025).unwrap();
        let mut policy_params = baseline_params.clone();
        policy_params.income_tax.uk_brackets[0].rate = 0.15;
        policy_params.labour_supply.enabled = false;

        let income = 30_000.0;
        let (people, benunits, households) = make_worker(
            income, 35.0, crate::engine::entities::Gender::Male,
        );
        let baseline_sim = Simulation::new(
            people.clone(), benunits.clone(), households.clone(),
            baseline_params.clone(), 2025,
        );
        let baseline_net: Vec<f64> = baseline_sim.run().household_results.iter()
            .map(|hr| hr.net_income).collect();

        let adjusted = apply_labour_supply_responses(
            &people, &benunits, &households,
            &baseline_params, &policy_params,
            &baseline_net, 2025,
        );

        assert_eq!(
            adjusted[0].employment_income, people[0].employment_income,
            "Employment income should be unchanged when labour supply is disabled"
        );
    }

    /// Married women with young children have a higher substitution elasticity
    /// than single men — their employment income adjustment should be larger.
    #[test]
    fn married_women_young_children_higher_elasticity() {
        use crate::engine::entities::Gender;

        let baseline_params = crate::parameters::Parameters::for_year(2025).unwrap();
        let mut policy_params = baseline_params.clone();
        policy_params.income_tax.uk_brackets[0].rate = 0.15;

        let income = 30_000.0;

        // Married woman with a 3-year-old child (highest OBR elasticity: 0.439)
        let (mut people_f, mut benunits_f, households_f) = make_worker(income, 35.0, Gender::Female);
        // Add a partner to make it a couple
        let mut partner = Person::default();
        partner.id = 1; partner.benunit_id = 0; partner.household_id = 0;
        partner.age = 36.0; partner.gender = Gender::Male;
        partner.emp_status = 2; // full-time employee
        // Add a young child
        let mut child = Person::default();
        child.id = 2; child.benunit_id = 0; child.household_id = 0;
        child.age = 3.0;
        people_f.push(partner); people_f.push(child);
        benunits_f[0].person_ids = vec![0, 1, 2];
        let hh_f = crate::engine::entities::Household {
            id: 0, benunit_ids: vec![0], person_ids: vec![0, 1, 2],
            weight: 1.0, region: Region::London,
            ..crate::engine::entities::Household::default()
        };

        let baseline_sim_f = Simulation::new(
            people_f.clone(), benunits_f.clone(), vec![hh_f.clone()],
            baseline_params.clone(), 2025,
        );
        let baseline_net_f: Vec<f64> = baseline_sim_f.run().household_results.iter()
            .map(|hr| hr.net_income).collect();

        let adjusted_f = apply_labour_supply_responses(
            &people_f, &benunits_f, &[hh_f],
            &baseline_params, &policy_params, &baseline_net_f, 2025,
        );
        let delta_f = adjusted_f[0].employment_income - people_f[0].employment_income;

        // Single man (elasticity 0.15)
        let (people_m, benunits_m, households_m) = make_worker(income, 35.0, Gender::Male);
        let baseline_sim_m = Simulation::new(
            people_m.clone(), benunits_m.clone(), households_m.clone(),
            baseline_params.clone(), 2025,
        );
        let baseline_net_m: Vec<f64> = baseline_sim_m.run().household_results.iter()
            .map(|hr| hr.net_income).collect();
        let adjusted_m = apply_labour_supply_responses(
            &people_m, &benunits_m, &households_m,
            &baseline_params, &policy_params, &baseline_net_m, 2025,
        );
        let delta_m = adjusted_m[0].employment_income - people_m[0].employment_income;

        assert!(
            delta_f > delta_m,
            "Married woman (youngest child 3) ΔE ({:.2}) should exceed single man ΔE ({:.2})",
            delta_f, delta_m
        );
    }

    fn make_hicbc_sim(income: f64, params: Parameters) -> Simulation {
        let mut adult = Person::default();
        adult.id = 0;
        adult.age = 35.0;
        adult.employment_income = income;
        adult.hours_worked = 37.5 * 52.0;

        let mut child = Person::default();
        child.id = 1;
        child.age = 5.0;

        let bu = BenUnit {
            id: 0, household_id: 0, person_ids: vec![0, 1],
            full_take_up: true,
            ..BenUnit::default()
        };
        let hh = Household {
            id: 0, person_ids: vec![0, 1], benunit_ids: vec![0],
            weight: 1.0, region: Region::London, council_tax: 1500.0,
            ..Household::default()
        };

        Simulation::new(vec![adult, child], vec![bu], vec![hh], params, 2025)
    }

    #[test]
    fn hicbc_zero_below_threshold() {
        let params = Parameters::for_year(2025).unwrap();
        let sim = make_hicbc_sim(50000.0, params);
        let results = sim.run();
        assert!(results.person_results[0].hicbc < 0.01,
            "No HICBC below threshold, got {}", results.person_results[0].hicbc);
        assert!(results.benunit_results[0].child_benefit > 0.0,
            "Should receive full child benefit");
    }

    #[test]
    fn hicbc_full_above_taper_end() {
        let params = Parameters::for_year(2025).unwrap();
        let sim = make_hicbc_sim(90000.0, params);
        let results = sim.run();
        let cb = results.benunit_results[0].child_benefit;
        assert!(cb > 0.0, "Full child benefit should be paid");
        assert!((results.person_results[0].hicbc - cb).abs() < 1.0,
            "HICBC should equal full CB above taper end: hicbc={}, cb={}",
            results.person_results[0].hicbc, cb);
    }

    #[test]
    fn hicbc_partial_in_taper_zone() {
        let params = Parameters::for_year(2025).unwrap();
        // £70k is halfway between threshold (60k) and taper_end (80k)
        let sim = make_hicbc_sim(70000.0, params);
        let results = sim.run();
        let cb = results.benunit_results[0].child_benefit;
        let hicbc = results.person_results[0].hicbc;
        assert!(hicbc > 0.0, "HICBC should be positive in taper zone");
        assert!(hicbc < cb, "HICBC should be less than full CB in taper zone");
        // Roughly 50% clawback at midpoint (adjusted net income may differ slightly from gross)
        assert!(hicbc > cb * 0.3 && hicbc < cb * 0.7,
            "HICBC should be roughly 50% of CB at midpoint: hicbc={}, cb={}", hicbc, cb);
    }

    #[test]
    fn hicbc_threshold_param_responsive() {
        let mut params = Parameters::for_year(2025).unwrap();
        let sim_base = make_hicbc_sim(65000.0, params.clone());
        let base_hicbc = sim_base.run().person_results[0].hicbc;

        params.child_benefit.hicbc_threshold += 3000.0;
        let sim_reform = make_hicbc_sim(65000.0, params);
        let reform_hicbc = sim_reform.run().person_results[0].hicbc;

        assert!(reform_hicbc < base_hicbc,
            "Raising HICBC threshold should reduce charge: base={}, reform={}", base_hicbc, reform_hicbc);
    }

    #[test]
    fn hicbc_taper_end_param_responsive() {
        let mut params = Parameters::for_year(2025).unwrap();
        let sim_base = make_hicbc_sim(70000.0, params.clone());
        let base_hicbc = sim_base.run().person_results[0].hicbc;

        params.child_benefit.hicbc_taper_end += 10000.0;
        let sim_reform = make_hicbc_sim(70000.0, params);
        let reform_hicbc = sim_reform.run().person_results[0].hicbc;

        assert!(reform_hicbc < base_hicbc,
            "Raising HICBC taper end should reduce charge: base={}, reform={}", base_hicbc, reform_hicbc);
    }

    #[test]
    fn hicbc_included_in_income_tax() {
        let params = Parameters::for_year(2025).unwrap();
        let sim = make_hicbc_sim(90000.0, params);
        let results = sim.run();
        let hicbc = results.person_results[0].hicbc;
        let it = results.person_results[0].income_tax;
        assert!(hicbc > 0.0);
        // Income tax should include HICBC
        assert!(it > hicbc, "Income tax ({}) should be greater than HICBC ({}) alone", it, hicbc);
    }

    /// Integration test mirroring the canonical policyengine-uk PR #1296 example:
    /// a 2pp NI employee rate cut (12% → 10%) applied to a basic-rate earner.
    ///
    /// Verifies the full pipeline: baseline → labour supply adjustment → reform run.
    /// Checks:
    ///   1. Labour supply response is positive (lower NI → higher retention → more work)
    ///   2. Dynamic net income gain exceeds the static gain (behavioural response adds revenue)
    ///   3. Disabled labour supply produces exactly the static result
    #[test]
    fn ni_cut_2pp_labour_supply_integration() {
        let baseline_params = crate::parameters::Parameters::for_year(2025).unwrap();

        // Reform: cut employee NI main rate by 2pp (8% → 6%), matching OBR NI example
        let mut policy_params = baseline_params.clone();
        policy_params.national_insurance.main_rate -= 0.02;

        // Basic-rate male worker earning £35k — should see a positive substitution response
        let (people, benunits, households) = make_worker(35_000.0, 40.0, crate::engine::entities::Gender::Male);

        // Baseline static run
        let baseline_sim = Simulation::new(
            people.clone(), benunits.clone(), households.clone(),
            baseline_params.clone(), 2025,
        );
        let baseline_results = baseline_sim.run();
        let baseline_net = baseline_results.household_results[0].net_income;
        let baseline_net_vec: Vec<f64> = baseline_results.household_results.iter()
            .map(|hr| hr.net_income).collect();

        // Static reform (no behavioural response)
        let static_sim = Simulation::new(
            people.clone(), benunits.clone(), households.clone(),
            policy_params.clone(), 2025,
        );
        let static_results = static_sim.run();
        let static_net = static_results.household_results[0].net_income;
        let static_gain = static_net - baseline_net;

        // Dynamic reform: adjust employment incomes first, then run
        let adjusted_people = apply_labour_supply_responses(
            &people, &benunits, &households,
            &baseline_params, &policy_params,
            &baseline_net_vec, 2025,
        );
        let delta_e = adjusted_people[0].employment_income - people[0].employment_income;

        let dynamic_sim = Simulation::new(
            adjusted_people, benunits.clone(), households.clone(),
            policy_params.clone(), 2025,
        );
        let dynamic_results = dynamic_sim.run();
        let dynamic_net = dynamic_results.household_results[0].net_income;
        let dynamic_gain = dynamic_net - baseline_net;

        // 1. Static gain should be positive (NI cut raises take-home pay)
        assert!(static_gain > 0.0,
            "Static gain from NI cut should be positive, got {:.2}", static_gain);

        // 2. Labour supply response should be positive for a basic-rate earner
        assert!(delta_e > 0.0,
            "Expected positive ΔE from NI cut, got {:.2}", delta_e);

        // 3. Dynamic gain exceeds static gain (extra earnings partially offset revenue cost)
        assert!(dynamic_gain > static_gain,
            "Dynamic gain ({:.2}) should exceed static gain ({:.2})", dynamic_gain, static_gain);

        // Disabled: dynamic result must match static exactly
        let mut policy_static = policy_params.clone();
        policy_static.labour_supply.enabled = false;
        let adjusted_static = apply_labour_supply_responses(
            &people, &benunits, &households,
            &baseline_params, &policy_static,
            &baseline_net_vec, 2025,
        );
        assert_eq!(adjusted_static[0].employment_income, people[0].employment_income,
            "Disabled labour supply should not change employment income");
    }
}
