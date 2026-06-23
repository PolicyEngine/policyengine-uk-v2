"""Generate example outputs for the documentation site."""
import json
import pandas as pd
from policyengine_uk_compiled import (
    Simulation, Parameters, IncomeTaxParams, UniversalCreditParams,
    StructuralReform,
)
from policyengine_uk_compiled.engine import PERSON_DEFAULTS, BENUNIT_DEFAULTS, HOUSEHOLD_DEFAULTS

outputs = {}

# ── Single person: baseline ───────────────────────────────────────────────────
persons, benunits, households = Simulation.single_person(employment_income=50_000)
sim_single = Simulation(year=2025, persons=persons, benunits=benunits, households=households)

baseline = sim_single.run()
reform_pa = Parameters(income_tax=IncomeTaxParams(personal_allowance=15_000))
result_pa = sim_single.run(policy=reform_pa)

outputs["single_baseline_revenue"] = round(baseline.budgetary_impact.baseline_revenue, 2)
outputs["pa_reform_net_cost"] = round(result_pa.budgetary_impact.net_cost, 2)

# ── Microdata: single person ──────────────────────────────────────────────────
micro = sim_single.run_microdata(policy=reform_pa)
hh = micro.households.iloc[0]
p  = micro.persons.iloc[0]

outputs["microdata_hh"] = {
    "household_id": int(hh["household_id"]),
    "weight": float(hh["weight"]),
    "baseline_net_income": float(hh["baseline_net_income"]),
    "reform_net_income":   float(hh["reform_net_income"]),
}
outputs["microdata_person"] = {
    "person_id":          int(p["person_id"]),
    "employment_income":  float(p["employment_income"]),
    "baseline_income_tax": float(p["baseline_income_tax"]),
    "reform_income_tax":   float(p["reform_income_tax"]),
}

# ── Couple with children ──────────────────────────────────────────────────────
persons2, benunits2, households2 = Simulation.couple(
    ages=(38, 35), incomes=(55_000, 30_000), children=2, child_ages=[4.0, 8.0],
    region="London", rent_monthly=2_000,
)
sim_couple = Simulation(year=2025, persons=persons2, benunits=benunits2, households=households2)

baseline_couple = sim_couple.run()
outputs["couple_baseline_net_income"] = round(
    baseline_couple.budgetary_impact.baseline_revenue -
    baseline_couple.budgetary_impact.baseline_revenue +
    0.0, 2
)
micro_couple = sim_couple.run_microdata()
hh2 = micro_couple.households.iloc[0]
outputs["couple_hh"] = {
    "baseline_net_income": float(hh2["baseline_net_income"]),
    "baseline_total_tax":  float(hh2["baseline_total_tax"]),
    "baseline_total_benefits": float(hh2["baseline_total_benefits"]),
}

# ── Income sweep (MTR illustration) ──────────────────────────────────────────
incomes = list(range(10_000, 100_001, 10_000))
person_rows, bu_rows, hh_rows = [], [], []
for i, inc in enumerate(incomes):
    person_rows.append({**PERSON_DEFAULTS, "person_id": i, "benunit_id": i, "household_id": i,
                        "employment_income": float(inc)})
    bu_rows.append({**BENUNIT_DEFAULTS, "benunit_id": i, "household_id": i, "person_ids": str(i)})
    hh_rows.append({**HOUSEHOLD_DEFAULTS, "household_id": i, "benunit_ids": str(i), "person_ids": str(i)})

sim_sweep = Simulation(year=2025,
    persons=pd.DataFrame(person_rows),
    benunits=pd.DataFrame(bu_rows),
    households=pd.DataFrame(hh_rows))

micro_sweep = sim_sweep.run_microdata()
sweep_hh = micro_sweep.households[["household_id", "baseline_net_income"]].copy()
sweep_hh["employment_income"] = incomes

outputs["sweep_sample"] = [
    {"employment_income": int(row["employment_income"]),
     "baseline_net_income": round(row["baseline_net_income"], 2)}
    for _, row in sweep_hh.iterrows()
]

print(json.dumps(outputs, indent=2))
