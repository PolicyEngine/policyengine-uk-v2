---
name: policyengine-uk-compiled
description: Run credible UK tax-benefit microsimulation analysis with the policyengine_uk_compiled package — reforms, budgetary impacts, and distributional statistics — including the methodological guardrails (medians over bin means, real-terms conversion, estimator decomposition, sanity checks) that separate a defensible estimate from a misleading one.
---

# Using policyengine_uk_compiled effectively

This package is a compiled Rust microsimulation of the UK tax-benefit system
(income tax, NI, UC, legacy benefits, child benefit, pensions, indirect taxes)
run over survey microdata or synthetic households. The full API reference is in
`CLAUDE.md` next to this file (`policyengine_uk_compiled.print_guide()` prints
it). This skill covers how to use it *well*: the recipes, conventions and
skepticism habits that make results credible.

## Ground rules

- **Fiscal years.** `year=2025` means FY 2025/26 (April–March). Growth factors
  in the parameter files are the year-on-year change *entering* that fiscal
  year. When comparing with external sources, always check which convention
  they use before claiming agreement or disagreement.
- **Datasets are explicit.** `Simulation(year=..., dataset=...)` requires one
  of `"frs"`, `"efrs"`, `"spi"`, `"lcfs"`, `"was"`. Use `"efrs"` (enhanced,
  calibrated FRS) for distributional and fiscal analysis across years;
  `"frs"` for raw-survey comparisons; `"spi"` for top-income tax questions;
  `"was"` for wealth; `"lcfs"` for consumption.
- **Everything is nominal.** All monetary outputs are in the simulation year's
  prices. Never compare levels or changes across years without deflating:
  `deflate(value, nominal_year, base_year)` or
  `result.real_hbai_incomes(base_year=...)`. Quoting a cross-year change in
  nominal terms is the single most common way to get the sign of a story wrong
  in high-inflation years.
- **Batch, don't loop.** One `Simulation` call with 10,000 households is ~100×
  faster than 10,000 calls. Build household DataFrames from the
  `*_DEFAULTS` templates and run once.

## Distributional statistics: how to not fool yourself

These lessons come from building HBAI-style income trend charts against DWP
outturn. They generalise.

1. **Use medians (or quantile points) within groups, not group means.**
   The mean of a decile/vigintile bin is dominated by whoever lands at the bin
   edges and by extreme values — especially in the bottom bin (negative and
   zero incomes from self-employment losses) and the top bin (unbounded).
   Median income within each group, or income at fixed quantile points
   (p5, p10, ... p95), is far more stable year-to-year and is what DWP HBAI
   publishes. If your group means show a spike the medians don't, the spike is
   an artefact.
2. **Follow the HBAI convention unless you have a reason not to:** equivalised
   household disposable income, counted per person (weight × household size),
   ranked within each year (re-ranked, not a fixed panel of households).
   State which convention you used; BHC and AHC rank households differently.
3. **When a distributional statistic looks implausible, decompose the
   estimator before blaming the data.** Split the change into parts: hold
   weights fixed vs reweight; hold group membership fixed vs re-rank; bin
   means vs quantile points. This localises whether the movement comes from
   the microdata, the calibration weights, or the statistic's own
   construction. Most "wild" swings turn out to be construction.
4. **Distrust the extremes.** The bottom 5% mixes genuine low income with
   under-reported benefits and transient self-employment losses; the top 1%
   is thin in the FRS (use SPI). Say less about v1 and v20 than about the
   middle, and never headline them without a robustness check.

## Skepticism checklist before quoting any number

- Have you actually computed it in this session? Never quote from memory.
- Does the aggregate pass an external sanity check? Before trusting a
  distributional detail, check the total (caseload, program cost, mean income
  growth) against an official source (OBR EFO, DWP benefit statistics, HBAI).
  If the aggregate is off, the distribution is off.
- Is it real or nominal, and did you say which base year?
- Are you sure about the year convention (fiscal vs calendar, "rate entering
  the year" vs "level in the year")? Off-by-one errors here are silent and
  produce plausible-looking wrong answers.
- If two layers disagree (e.g. your result vs a published figure), trace the
  discrepancy to the actual source layer — raw input → transform → engine →
  statistic — with evidence, before "fixing" the first plausible suspect.
- Direction-of-effect check: does the sign make mechanical sense? Raising the
  personal allowance can't cost non-taxpayers; a UC taper cut can't help
  households with no earnings; a benefit uprating lag hurts most in
  high-inflation years. If the sign surprises you, suspect the pipeline first
  and the economy second.

## Model boundaries to keep in mind

- Benefit receipt follows *reported* receipt in the survey (no take-up
  modelling for survey households; hypothetical households get full take-up).
  Reforms that would change take-up are outside the model.
- DWP cost of living payments are modelled for 2022/23 (£650 means-tested,
  £150 disability) and 2023/24 (£900, £150) via the `cost_of_living`
  parameter block; the pensioner payment alongside winter fuel payment is not.
- The final projection year carries the previous year's calibration weights
  (no targets exist for it) — treat it as an extrapolation, not an estimate.
- Consumption taxes (VAT, duties) are imputed from spending patterns;
  capital gains default to zero outside datasets that record them.
- Historical years before the survey base are constructed by de-uprating a
  fixed panel with OBR growth factors, then recalibrating — good for trends,
  not for point-in-time levels of small subgroups.

## Recipes

Budgetary impact of a reform:

```python
from policyengine_uk_compiled import Simulation, Parameters, IncomeTaxParams
sim = Simulation(year=2026, dataset="efrs")
result = sim.run(policy=Parameters(income_tax=IncomeTaxParams(personal_allowance=15_000)))
result.budgetary_impact.net_cost
```

Distributional change by vigintile (the robust way):

```python
md = sim.run_microdata(policy=reform)
hh = md.households
# The engine already provides equivalised incomes:
# baseline_equivalised_net_income, reform_equivalised_net_income
# (and _ahc variants). Person-weight by merging household size from persons:
size = md.persons.groupby("household_id").size().rename("people")
hh = hh.join(size, on="household_id")
w = hh["weight"] * hh["people"]
# Rank hh by baseline_equivalised_net_income into 20 groups on cumulative w,
# then report the *median* equivalised income per group in each scenario,
# not the group mean, and deflate both to a common base year.
```

Cross-year real trend: run one `Simulation` per year, take
`real_hbai_incomes(base_year=...)` or deflate quantile medians yourself, and
report YoY changes in quantile points — never nominal levels.

If a result will be published or presented, re-derive it once from scratch in
a fresh session before it goes on a slide.
