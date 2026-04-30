"""Parse HMRC Survey of Personal Incomes into calibration targets.

Downloads the SPI collated ODS from gov.uk (Tables 3.6 and 3.7) and builds
income-by-band targets for employment, self-employment, pensions, property,
dividends, and savings interest — both amounts and taxpayer counts per band.

The 2022-23 SPI snapshot is then scaled to all calibration years (2024-2030)
using OBR income growth indexes from sheets 3.5 and 1.6.

Source: https://www.gov.uk/government/statistics/income-tax-summarised-accounts-statistics
"""

from __future__ import annotations

import io
import logging
from functools import lru_cache
from pathlib import Path

import pandas as pd
import requests

logger = logging.getLogger(__name__)

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
CACHE_DIR = REPO_ROOT / "data" / "cache"

# HMRC SPI 2022-23 collated tables (ODS)
SPI_URL = "https://assets.publishing.service.gov.uk/media/67cabb37ade26736dbf9ffe5/Collated_Tables_3_1_to_3_17_2223.ods"
SPI_YEAR = 2022  # FY 2022-23 → base year for growth indexing
CALIBRATION_YEARS = range(2023, 2031)

INCOME_BANDS_LOWER = [
    12_570,
    15_000,
    20_000,
    30_000,
    40_000,
    50_000,
    70_000,
    100_000,
    150_000,
    200_000,
    300_000,
    500_000,
    1_000_000,
]
INCOME_BANDS_UPPER = INCOME_BANDS_LOWER[1:] + [
    1e12
]  # Effectively infinity, but JSON-safe

INCOME_VARIABLES = [
    "employment_income",
    "self_employment_income",
    "state_pension",
    "private_pension_income",
    "property_income",
    "dividend_income",
]


def _to_float(val) -> float:
    if val is None:
        return 0.0
    if isinstance(val, (int, float)):
        return float(val)
    s = str(val).replace(",", "").replace("£", "").strip()
    if s in ("", "-", "..", ".."):
        return 0.0
    return float(s)


@lru_cache(maxsize=1)
def _download_ods() -> bytes:
    cache_file = CACHE_DIR / "hmrc_spi.ods"
    if cache_file.exists():
        logger.info("Using cached HMRC SPI ODS: %s", cache_file)
        return cache_file.read_bytes()

    logger.info("Downloading HMRC SPI ODS...")
    r = requests.get(SPI_URL, timeout=60, allow_redirects=True)
    r.raise_for_status()
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    cache_file.write_bytes(r.content)
    return r.content


def _parse_table_36(ods_bytes: bytes) -> pd.DataFrame:
    """Table 3.6: employment, self-employment, state/private pensions by income band."""
    df = pd.read_excel(
        io.BytesIO(ods_bytes), sheet_name="Table_3_6", engine="odf", header=None
    )
    rows = []
    for i in range(5, len(df)):
        lower = df.iloc[i, 0]
        if not isinstance(lower, (int, float)):
            break
        rows.append(
            {
                "lower_bound": int(lower),
                "self_employment_income_count": _to_float(df.iloc[i, 1]),
                "self_employment_income_amount": _to_float(df.iloc[i, 2]),
                "employment_income_count": _to_float(df.iloc[i, 4]),
                "employment_income_amount": _to_float(df.iloc[i, 5]),
                "state_pension_count": _to_float(df.iloc[i, 7]),
                "state_pension_amount": _to_float(df.iloc[i, 8]),
                "private_pension_income_count": _to_float(df.iloc[i, 10]),
                "private_pension_income_amount": _to_float(df.iloc[i, 11]),
            }
        )
    return pd.DataFrame(rows)


def _parse_table_37(ods_bytes: bytes) -> pd.DataFrame:
    """Table 3.7: property, savings interest, dividends by income band."""
    df = pd.read_excel(
        io.BytesIO(ods_bytes), sheet_name="Table_3_7", engine="odf", header=None
    )
    rows = []
    for i in range(5, len(df)):
        lower = df.iloc[i, 0]
        if not isinstance(lower, (int, float)):
            break
        rows.append(
            {
                "lower_bound": int(lower),
                "property_income_count": _to_float(df.iloc[i, 1]),
                "property_income_amount": _to_float(df.iloc[i, 2]),
                "savings_interest_count": _to_float(df.iloc[i, 4]),
                "savings_interest_amount": _to_float(df.iloc[i, 5]),
                "dividend_income_count": _to_float(df.iloc[i, 7]),
                "dividend_income_amount": _to_float(df.iloc[i, 8]),
            }
        )
    return pd.DataFrame(rows)


def get_targets() -> list[dict]:
    targets = []
    try:
        ods_bytes = _download_ods()
    except Exception as e:
        logger.error("Failed to download HMRC SPI ODS: %s", e)
        return targets

    # Get OBR growth indexes for scaling to future years
    from build_targets import obr

    growth_indexes = obr.get_income_growth_indexes()

    t36 = _parse_table_36(ods_bytes)
    t37 = _parse_table_37(ods_bytes)
    merged = t36.merge(t37, on="lower_bound", how="outer")

    # Build base-year targets, then scale to all calibration years
    for idx, row in merged.iterrows():
        lower = int(row["lower_bound"])
        upper = INCOME_BANDS_UPPER[idx] if idx < len(INCOME_BANDS_UPPER) else 1e12
        band_label = f"{lower}_to_{upper:.0f}" if upper < 1e12 else f"{lower}_plus"

        for variable in INCOME_VARIABLES:
            amount_col = f"{variable}_amount"
            count_col = f"{variable}_count"

            if amount_col in row.index and row[amount_col] > 0:
                base_amount = float(row[amount_col]) * 1e6  # £millions → £
                var_index = growth_indexes.get(variable, {})

                for year in CALIBRATION_YEARS:
                    # Scale amount by growth index relative to base year
                    scale = 1.0
                    if var_index:
                        base_idx = var_index.get(SPI_YEAR, 1.0)
                        year_idx = var_index.get(year, base_idx)
                        scale = year_idx / base_idx if base_idx > 0 else 1.0
                    scaled_amount = base_amount * scale

                    targets.append(
                        {
                            "name": f"hmrc/{variable}_amount_{band_label}/{year}",
                            "variable": variable,
                            "entity": "person",
                            "aggregation": "sum",
                            "filter": {
                                "variable": "total_income",
                                "min": float(lower),
                                "max": float(upper),
                            },
                            "value": scaled_amount,
                            "source": "hmrc_spi",
                            "year": year,
                            "holdout": False,
                        }
                    )

            if count_col in row.index and row[count_col] > 0:
                base_count = float(row[count_col]) * 1e3  # thousands → people

                for year in CALIBRATION_YEARS:
                    # Counts are held constant — income growth changes amounts
                    # not the number of taxpayers per band (the band boundaries
                    # are fixed in nominal terms)
                    targets.append(
                        {
                            "name": f"hmrc/{variable}_count_{band_label}/{year}",
                            "variable": variable,
                            "entity": "person",
                            "aggregation": "count_nonzero",
                            "filter": {
                                "variable": "total_income",
                                "min": float(lower),
                                "max": float(upper),
                            },
                            "value": base_count,
                            "source": "hmrc_spi",
                            "year": year,
                            "holdout": True,
                        }
                    )

    return targets
