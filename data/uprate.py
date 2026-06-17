"""Uprate clean FRS microdata from its collection year to a target year.

Faithful Python port of the Rust `Dataset::uprate_to` (src/data/mod.rs): applies
OBR EFO year-on-year growth indices (matching policyengine-uk's
economic_assumptions) to monetary columns, and the population index to weights.

Used for pooling adjacent FRS years onto a common price level before building
the EFRS. Operates on the clean persons/households CSV columns; benunits carry no
monetary fields and are passed through unchanged.
"""

from __future__ import annotations

import numpy as np
import pandas as pd

# ── Year-on-year growth rates by index (OBR EFO Nov 2025). Each (year, rate)
# applies to the transition *into* that fiscal year. Outside the table, the
# default long-run rate is used. Mirrors src/data/mod.rs::yoy_rates. ──
_YOY: dict[str, list[tuple[int, float]]] = {
    "earnings": [
        (2022, 0.0614), (2023, 0.0622), (2024, 0.0493), (2025, 0.0517),
        (2026, 0.0333), (2027, 0.0225), (2028, 0.0210), (2029, 0.0221), (2030, 0.0232),
    ],
    "cpi": [
        (2022, 0.0907), (2023, 0.0730), (2024, 0.0253), (2025, 0.0345),
        (2026, 0.0248), (2027, 0.0202), (2028, 0.0204), (2029, 0.0204), (2030, 0.0200),
    ],
    "gdp_pc": [
        (2022, 0.1019), (2023, 0.0532), (2024, 0.0372), (2025, 0.0418),
        (2026, 0.0327), (2027, 0.0326), (2028, 0.0302), (2029, 0.0294), (2030, 0.0306),
    ],
    "mixed_pc": [
        (2022, 0.0296), (2023, -0.0060), (2024, 0.0273), (2025, 0.0024),
        (2026, 0.0362), (2027, 0.0374), (2028, 0.0351), (2029, 0.0358), (2030, 0.0364),
    ],
    "rent": [
        (2022, 0.0347), (2023, 0.0575), (2024, 0.0716), (2025, 0.0546),
        (2026, 0.0334), (2027, 0.0311), (2028, 0.0243), (2029, 0.0234), (2030, 0.0254),
    ],
    "council_tax": [
        (2023, 0.051), (2024, 0.051), (2025, 0.0781), (2026, 0.0530),
        (2027, 0.0579), (2028, 0.0565), (2029, 0.0547), (2030, 0.0542),
    ],
    "population": [
        (2022, 0.0093), (2023, 0.0131), (2024, 0.0107), (2025, 0.0072),
        (2026, 0.0038), (2027, 0.0037), (2028, 0.0040), (2029, 0.0044), (2030, 0.0045),
    ],
    "interest": [
        (2022, 1.210), (2023, 0.987), (2024, 0.142), (2025, 0.0519),
        (2026, 0.0565), (2027, 0.0474), (2028, 0.0364), (2029, 0.0302), (2030, 0.0292),
    ],
}

_DEFAULT_RATE: dict[str, float] = {
    "earnings": 0.0383,
    "cpi": 0.0200,
    "gdp_pc": 0.0306,
    "mixed_pc": 0.0364,
    "rent": 0.0254,
    "council_tax": 0.0542,
    "population": 0.0045,
    "interest": 0.0292,
}


def cumulative_factor(base_year: int, target_year: int, index: str) -> float:
    """Cumulative growth factor from base_year to target_year for an index.

    Mirrors src/data/mod.rs::cumulative_factor — compounds the per-year rates
    forward (or divides them backward) over the fiscal years crossed.
    """
    rates = dict(_YOY[index])
    default = _DEFAULT_RATE[index]

    def rate_for(y: int) -> float:
        return rates.get(y, default)

    if target_year == base_year:
        return 1.0
    factor = 1.0
    if target_year > base_year:
        for y in range(base_year + 1, target_year + 1):
            factor *= 1.0 + rate_for(y)
    else:
        for y in range(target_year + 1, base_year + 1):
            factor /= 1.0 + rate_for(y)
    return factor


# ── Column → index assignments (clean-CSV column names). Mirrors the field
# assignments in uprate_to; struct fields are mapped to their CSV column names
# (e.g. pension_income → private_pension_income). Columns not listed (counts,
# components, capital_gains, ids, ages) are left unchanged, matching Rust. ──
_PERSON_COLS: dict[str, list[str]] = {
    "earnings": [
        "employment_income", "employee_pension_contributions",
        "personal_pension_contributions",
    ],
    "mixed_pc": ["self_employment_income"],
    "gdp_pc": [
        "private_pension_income", "dividend_income", "property_income",
        "maintenance_income", "miscellaneous_income", "other_income",
    ],
    "interest": ["savings_interest"],
    "cpi": [
        "state_pension", "child_benefit", "housing_benefit", "income_support",
        "pension_credit", "child_tax_credit", "working_tax_credit",
        "universal_credit", "dla_care", "dla_mobility", "pip_daily_living",
        "pip_mobility", "carers_allowance", "attendance_allowance", "esa_income",
        "esa_contributory", "jsa_income", "jsa_contributory", "other_benefits",
        "adp_daily_living", "adp_mobility", "cdp_care", "cdp_mobility",
        "childcare_expenses",
    ],
}

_HOUSEHOLD_COLS: dict[str, list[str]] = {
    "earnings": [
        "owned_land", "property_wealth", "corporate_wealth",
        "gross_financial_wealth", "net_financial_wealth", "main_residence_value",
        "other_residential_property_value", "non_residential_property_value",
        "savings",
    ],
    "rent": ["rent_annual"],
    "council_tax": ["council_tax_annual"],
    "cpi": [
        "food_consumption", "alcohol_consumption", "tobacco_consumption",
        "clothing_consumption", "housing_water_electricity_consumption",
        "furnishings_consumption", "health_consumption", "transport_consumption",
        "communication_consumption", "recreation_consumption",
        "education_consumption", "restaurants_consumption",
        "miscellaneous_consumption", "petrol_spending", "diesel_spending",
        "domestic_energy_consumption", "electricity_consumption",
        "gas_consumption",
    ],
}


def _apply(df: pd.DataFrame, col_map: dict[str, list[str]],
           base_year: int, target_year: int) -> pd.DataFrame:
    out = df.copy()
    for index, cols in col_map.items():
        f = cumulative_factor(base_year, target_year, index)
        for c in cols:
            if c in out.columns:
                out[c] = out[c].to_numpy(dtype=float) * f
    return out


def uprate_persons(persons: pd.DataFrame, base_year: int, target_year: int) -> pd.DataFrame:
    if target_year == base_year:
        return persons.copy()
    return _apply(persons, _PERSON_COLS, base_year, target_year)


def uprate_households(households: pd.DataFrame, base_year: int, target_year: int) -> pd.DataFrame:
    """Uprate monetary household columns and scale weights by the population index."""
    if target_year == base_year:
        return households.copy()
    out = _apply(households, _HOUSEHOLD_COLS, base_year, target_year)
    if "weight" in out.columns:
        pop = cumulative_factor(base_year, target_year, "population")
        out["weight"] = out["weight"].to_numpy(dtype=float) * pop
    return out
