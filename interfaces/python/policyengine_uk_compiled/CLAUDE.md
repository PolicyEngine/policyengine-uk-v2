# policyengine_uk_compiled — UK tax-benefit microsimulation engine

Compiled Rust binary wrapped in Python. Simulates UK income tax, National Insurance, Universal Credit, Child Benefit, and 10+ other programs for any household or the full FRS population. ~0.1ms per household.

## Quick start

```python
from policyengine_uk_compiled import (
    Simulation,
    Parameters,
    IncomeTaxParams,
    get_baseline_params,
)

# Single person earning £50k
persons, benunits, households = Simulation.single_person(employment_income=50_000)
sim = Simulation(year=2025, persons=persons, benunits=benunits, households=households)
result = sim.run()  # → SimulationResult
result.budgetary_impact.net_cost  # fiscal impact

# With a reform
reform = Parameters(income_tax=IncomeTaxParams(personal_allowance=20_000))
result = sim.run(policy=reform)

# Parameter inspection does not require microdata
params = get_baseline_params(year=2025)
```

## Two output modes

### `sim.run(policy=None) → SimulationResult`
Aggregate stats: budgetary impact, program breakdown, decile impacts, winners/losers.

### `sim.run_microdata(policy=None) → MicrodataResult`
Per-entity DataFrames. Use this when you need individual household results (e.g. marginal tax rates, budget constraints). Returns:
- `.persons` — DataFrame with input fields + `baseline_income_tax`, `reform_income_tax`, etc.
- `.benunits` — DataFrame with `baseline_universal_credit`, `baseline_child_benefit`, `baseline_total_benefits`, etc.
- `.households` — DataFrame with `baseline_net_income`, `baseline_total_tax`, `baseline_total_benefits`, `reform_net_income`, etc.

## Household constructors

### `Simulation.single_person(**kwargs) → (persons_df, benunits_df, households_df)`
```python
Simulation.single_person(
    age=30, employment_income=0.0, self_employment_income=0.0,
    pension_income=0.0, region="London", rent_monthly=0.0,
    council_tax_annual=0.0, **person_kwargs
)
```

### `Simulation.couple(**kwargs) → (persons_df, benunits_df, households_df)`
```python
Simulation.couple(
    ages=(30, 30), incomes=(0.0, 0.0), children=0,
    child_ages=None, region="London", rent_monthly=0.0,
    council_tax_annual=0.0
)
```

### Custom / batched households
Build DataFrames directly using `PERSON_DEFAULTS`, `BENUNIT_DEFAULTS`, `HOUSEHOLD_DEFAULTS` as templates. Link entities via IDs: each person has `benunit_id` and `household_id`; each benunit/household has `person_ids` as semicolon-separated string (e.g. `"0;1;2"`).

```python
from policyengine_uk_compiled.engine import PERSON_DEFAULTS, BENUNIT_DEFAULTS, HOUSEHOLD_DEFAULTS
import pandas as pd

persons = []
benunits = []
households = []
for i, income in enumerate(range(20_000, 100_001, 1_000)):
    persons.append({**PERSON_DEFAULTS, "person_id": i, "benunit_id": i,
                    "household_id": i, "employment_income": float(income)})
    benunits.append({**BENUNIT_DEFAULTS, "benunit_id": i, "household_id": i,
                     "person_ids": str(i)})
    households.append({**HOUSEHOLD_DEFAULTS, "household_id": i,
                       "benunit_ids": str(i), "person_ids": str(i)})

sim = Simulation(year=2025, persons=pd.DataFrame(persons),
                 benunits=pd.DataFrame(benunits), households=pd.DataFrame(households))
result = sim.run_microdata()
# result.households has one row per household with net_income, total_tax, etc.
```

Batching is critical for performance: 1 call with 100 households ≈ 15ms; 100 separate calls ≈ 1000ms.

## Reform parameters

Only set fields you want to change — everything else keeps baseline values.

```python
Parameters(
    income_tax=IncomeTaxParams(personal_allowance=..., pa_taper_threshold=..., ...),
    national_insurance=NationalInsuranceParams(primary_threshold_annual=..., main_rate=..., ...),
    universal_credit=UniversalCreditParams(taper_rate=..., work_allowance_higher=..., child_element_first=..., ...),
    child_benefit=ChildBenefitParams(eldest_weekly=..., hicbc_threshold=..., hicbc_taper_end=..., ...),
    state_pension=StatePensionParams(new_state_pension_weekly=..., ...),
    pension_credit=PensionCreditParams(standard_minimum_single=..., ...),
    benefit_cap=BenefitCapParams(single_london=..., non_single_london=..., ...),
    housing_benefit=HousingBenefitParams(withdrawal_rate=..., ...),
    tax_credits=TaxCreditsParams(ctc_child_element=..., taper_rate=..., ...),
    scottish_child_payment=ScottishChildPaymentParams(weekly_amount=..., ...),
    capital_gains_tax=CapitalGainsTaxParams(annual_exempt_amount=3000, basic_rate=0.18, higher_rate=0.24),
    stamp_duty=StampDutyParams(bands=[StampDutyBand(rate=0.0, threshold=0), StampDutyBand(rate=0.05, threshold=125000), ...]),
    wealth_tax=WealthTaxParams(enabled=True, threshold=1_000_000, rate=0.01),
)
```

`capital_gains_tax` uses `person.capital_gains` (default zero in FRS/WAS — zero by default unless the dataset records capital gains). `stamp_duty` applies to owner-occupiers using the household's `main_residence_value` and an annualised purchase probability. `wealth_tax` requires wealth data — use EFRS or WAS datasets.

## Marginal tax rate calculation pattern

```python
DELTA = 100  # £100 increment
# Create paired households: (income, income+DELTA) for each income level
# Run once with run_microdata()
# MTR = 1 - (net_income_at_higher - net_income_at_lower) / DELTA
# Use "reform_net_income" column when policy is passed, "baseline_net_income" otherwise
```

## Full population runs (FRS, SPI, LCFS, WAS)

If `POLICYENGINE_UK_DATA_TOKEN` is set, data auto-downloads on demand to `~/.policyengine-uk-data/<dataset>/`.

Available datasets: `"frs"`, `"efrs"`, `"spi"`, `"lcfs"`, `"was"`. A dataset
must be named explicitly — there is no default. Constructing a `Simulation`
with neither DataFrames nor `dataset=` (nor `data_dir=`) raises `ValueError`.

```python
# FRS — the standard household survey
sim = Simulation(year=2025, dataset="frs")
result = sim.run()

# SPI, LCFS, or WAS — pass dataset=
sim = Simulation(year=2025, dataset="spi")
result = sim.run()

sim = Simulation(year=2025, dataset="lcfs")
result = sim.run()

# Download all datasets/years explicitly
from policyengine_uk_compiled import download_all
download_all()                        # all datasets
download_all(datasets=("spi", "was")) # specific datasets
```

Or with an explicit local path:
```python
sim = Simulation(year=2025, data_dir="data/frs")
result = sim.run()
```

No extra dependencies needed — uses stdlib only.

## Available years
1994–2030 (fiscal years, so year=2025 means 2025/26).

## Nominal vs real terms

All monetary outputs (HBAI incomes, budgetary impact, program totals) are
**nominal** — expressed in the simulation year's prices. To compare figures
across years, deflate them to a common base year using the CPI index.

`SimulationResult.cpi_index` is the year's CPI rebased to 2010/11 = 100.

```python
from policyengine_uk_compiled import Simulation, deflate

# Real growth in mean household income, 2025/26 prices
base = 2025
reals = {}
for year in (2025, 2026, 2027, 2028, 2029):
    res = Simulation(year=year, dataset="efrs").run()
    real_baseline, _ = res.real_hbai_incomes(base_year=base)
    reals[year] = real_baseline.mean_bhc

# Or deflate a raw figure directly:
real_2029 = deflate(res.baseline_hbai_incomes.mean_bhc, nominal_year=2029, base_year=2025)
```

`SimulationResult.real_factor(base_year)` returns the nominal→real multiplier;
`real_hbai_incomes(base_year)` returns `(baseline, reform)` HBAI incomes already
deflated. `deflate(nominal, nominal_year, base_year)` is a standalone helper for
any figure.

## Key model classes (all from `policyengine_uk_compiled`)

| Class | Description |
|---|---|
| `Simulation` | Main entry point |
| `Parameters` | Reform overlay (all fields Optional) |
| `SimulationResult` | Aggregate output from `run()` |
| `MicrodataResult` | Per-entity DataFrames from `run_microdata()` |
| `BudgetaryImpact` | `.baseline_revenue`, `.reform_revenue`, `.net_cost`, etc. |
| `ProgramBreakdown` | Per-program tax/benefit totals |
| `DecileImpact` | Per-decile average income changes |
| `WinnersLosers` | Share gaining/losing, average gain/loss |
