"""ONS demographic calibration targets.

Population by age group, total households, tenure distribution, and regional
population. From ONS mid-year population estimates, household projections,
and English Housing Survey / census tenure breakdowns.

Sources:
- ONS mid-year population estimates 2023
- ONS household projections
- English Housing Survey / Census 2021 tenure distribution (UK-adjusted)
"""

from __future__ import annotations


# ONS mid-2023 population estimates (UK), rounded to nearest 1000.
# Source: ONS MYE 2023.
_POPULATION = {
    "children_0_15": 12_200_000,
    "working_age_16_64": 42_100_000,
    "pensioners_65_plus": 12_600_000,
    "total": 66_900_000,
}

# ONS household projections 2023 (England + Scotland + Wales + NI)
_TOTAL_HOUSEHOLDS = 28_200_000

# Regional population shares (2023 mid-year estimates)
# Regions match the FRS region codes.
_REGIONAL_POPULATION = {
    "north_east": 2_650_000,
    "north_west": 7_400_000,
    "yorkshire": 5_550_000,
    "east_midlands": 4_900_000,
    "west_midlands": 5_950_000,
    "east_of_england": 6_350_000,
    "london": 8_800_000,
    "south_east": 9_300_000,
    "south_west": 5_700_000,
    "wales": 3_100_000,
    "scotland": 5_450_000,
    "northern_ireland": 1_900_000,
}

# Household tenure distribution (UK, ~2023).
# Source: EHS 2022-23 headline report + census 2021 proportions for DA adjustment.
# tenure_type RF codes: 0=OwnedOutright, 1=OwnedWithMortgage, 2=RentFromCouncil,
# 3=RentFromHA, 4=RentPrivately, 5=Other.
# We combine social rent (council + HA) and use 3 broad categories.
_TENURE_HOUSEHOLDS = {
    "owned_outright": (0, 0, 8_800_000),       # ~31%
    "owned_mortgage": (1, 1, 6_600_000),        # ~23%
    "social_rent": (2, 3, 4_700_000),           # ~17% (council + HA)
    "private_rent": (4, 4, 4_900_000),          # ~17%
}

# Region RF codes matching the Rust enum.
_REGION_RF_CODE = {
    "north_east": 0,
    "north_west": 1,
    "yorkshire": 2,
    "east_midlands": 3,
    "west_midlands": 4,
    "east_of_england": 5,
    "london": 6,
    "south_east": 7,
    "south_west": 8,
    "wales": 9,
    "scotland": 10,
    "northern_ireland": 11,
}


def get_targets() -> list[dict]:
    """Generate ONS demographic targets for all calibration years.

    Population changes slowly year-to-year, so we emit the same targets for
    each year in the calibration range. This ensures they bind regardless of
    which --year is passed to calibration.
    """
    targets = []

    # Emit for all plausible calibration years
    for year in range(2023, 2031):
        # Age group population counts
        for group, count in _POPULATION.items():
            if group == "total":
                continue
            if group == "children_0_15":
                age_filter = {"variable": "age", "min": 0, "max": 16}
            elif group == "working_age_16_64":
                age_filter = {"variable": "age", "min": 16, "max": 65}
            else:  # pensioners
                age_filter = {"variable": "age", "min": 65, "max": 200}

            targets.append(
                {
                    "name": f"ons/population_{group}/{year}",
                    "variable": "age",
                    "entity": "person",
                    "aggregation": "count",
                    "filter": age_filter,
                    "value": float(count),
                    "source": "ons",
                    "year": year,
                    "holdout": False,
                }
            )

        # Total population
        targets.append(
            {
                "name": f"ons/total_population/{year}",
                "variable": "age",
                "entity": "person",
                "aggregation": "count",
                "filter": None,
                "value": float(_POPULATION["total"]),
                "source": "ons",
                "year": year,
                "holdout": False,
            }
        )

        # Total households
        targets.append(
            {
                "name": f"ons/total_households/{year}",
                "variable": "household_id",
                "entity": "household",
                "aggregation": "count",
                "filter": None,
                "value": float(_TOTAL_HOUSEHOLDS),
                "source": "ons",
                "year": year,
                "holdout": False,
            }
        )

        # Households by tenure
        for tenure_name, (code_lo, code_hi, count) in _TENURE_HOUSEHOLDS.items():
            targets.append(
                {
                    "name": f"ons/tenure_{tenure_name}/{year}",
                    "variable": "household_id",
                    "entity": "household",
                    "aggregation": "count",
                    "filter": {
                        "variable": "tenure_type",
                        "min": float(code_lo),
                        "max": float(code_hi) + 1.0,  # exclusive upper bound
                    },
                    "value": float(count),
                    "source": "ons",
                    "year": year,
                    "holdout": False,
                }
            )

        # Households by region
        for region_name, code in _REGION_RF_CODE.items():
            pop = _REGIONAL_POPULATION.get(region_name, 0)
            if pop == 0:
                continue
            # Approximate households from population using national ratio
            hh_count = pop * _TOTAL_HOUSEHOLDS / _POPULATION["total"]
            targets.append(
                {
                    "name": f"ons/region_{region_name}/{year}",
                    "variable": "household_id",
                    "entity": "household",
                    "aggregation": "count",
                    "filter": {
                        "variable": "region",
                        "min": float(code),
                        "max": float(code) + 1.0,
                    },
                    "value": round(hh_count),
                    "source": "ons",
                    "year": year,
                    "holdout": True,  # holdout — approximate conversion
                }
            )

    return targets
