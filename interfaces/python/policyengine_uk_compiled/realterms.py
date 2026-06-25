"""Nominal-to-real conversion using the CPI index.

The engine reports all monetary outputs (HBAI incomes, budgetary impacts,
program totals) in nominal prices for the simulation year. To compare across
years you must deflate to a common base year. This module is the single
source of truth for the CPI index used to do that.
"""

from __future__ import annotations

# CPI index by fiscal year, rebased to 2010/11 = 100 (the absolute-poverty
# reference year). Source: OBR EFO March 2026 table 1.7 CPI (2015=100), with
# pre-2010 fiscal years from ONS series D7BT financial-year averages. Mirrors
# cpi_index_for_year in src/data/clean.rs — keep the two in sync.
_CPI_INDEX = {
    1994: 72.916542, 1995: 74.863074, 1996: 76.532848, 1997: 77.879738,
    1998: 79.079023, 1999: 79.983099, 2000: 80.647319, 2001: 81.772802,
    2002: 82.787582, 2003: 83.876164, 2004: 85.084674, 2005: 86.874377,
    2006: 89.116118, 2007: 91.071875, 2008: 94.485225, 2009: 96.616263,
    2010: 100.000000, 2011: 104.300545, 2012: 107.068495, 2013: 109.535701,
    2014: 110.686646, 2015: 110.798825, 2016: 112.025879, 2017: 115.190516,
    2018: 117.802559, 2019: 119.851492, 2020: 120.557502, 2021: 125.368849,
    2022: 137.951381, 2023: 145.773765, 2024: 149.171329, 2025: 153.921493,
    2026: 156.889890, 2027: 160.021436, 2028: 163.222508, 2029: 166.486583,
    2030: 169.815982,  # 2029 chained by OBR Mar-2026 table 1.7 CPI 1.9998%
}

# Base year the index is rebased to (index == 100 here).
CPI_BASE_YEAR = 2010


def cpi_index_for_year(year: int) -> float:
    """CPI index for a fiscal year, rebased to 2010/11 = 100.

    Falls back to 100.0 for years outside the known range.
    """
    return _CPI_INDEX.get(year, 100.0)


def deflate(nominal: float, nominal_year: int, base_year: int = CPI_BASE_YEAR) -> float:
    """Convert a nominal value to real terms in ``base_year`` prices.

    ``nominal`` is a figure expressed in ``nominal_year`` prices; the result is
    the same quantity expressed in ``base_year`` prices using the CPI index.

    >>> deflate(33000, 2029, base_year=2025)  # 2029/30 nominal -> 2025/26 real
    """
    return nominal * cpi_index_for_year(base_year) / cpi_index_for_year(nominal_year)
