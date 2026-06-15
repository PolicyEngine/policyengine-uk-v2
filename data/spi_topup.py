"""Replace top-decile FRS household incomes with real SPI income streams.

The FRS under-samples very high incomes, so weight calibration cannot reach the
SPI tail-band targets (the worst calibration errors are all spi_* bands). This
takes the richest 10% of pensioner and non-pensioner FRS households and assigns
each adult a randomly drawn top-decile income record from HMRC's Survey of
Personal Incomes (SPI) microdata, restoring realistic tail variation before
calibration.

Pensioner = receives state pension. Adults drawing from the SPI pool are matched
to the pool of the same type (an adult on state pension draws a pensioner SPI
record; otherwise a non-pensioner record). SPI amounts are uprated to the FRS
year by the EFO average-earnings index. SPI's own grossing factor (FACT) weights
both the top-decile cut and the donor draw, since the SPI over-samples the rich.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pandas as pd
from rich.console import Console

console = Console()

# FRS person income column ← SPI tape column. State pension (SRP) is deliberately
# excluded: it is essentially flat-rate, so it has no high-income tail to capture,
# and overwriting it manufactures implausible high-value state-pension recipients.
_INCOME_MAP = {
    "employment_income": "PAY",
    "self_employment_income": "PROFITS",
    "private_pension_income": "PENSION",
    "savings_interest": "INCBBS",
    "dividend_income": "DIVIDENDS",
    "property_income": "INCPROP",
}

_SPI_YEAR = 2022  # the put2223uk.tab tape is fiscal year 2022-23
_TOP_QUANTILE = 0.9
_ADULT_AGE = 18


def _weighted_quantile(values: np.ndarray, weights: np.ndarray, q: float) -> float:
    order = np.argsort(values)
    v, w = values[order], weights[order]
    cum = np.cumsum(w) - 0.5 * w
    cum /= w.sum()
    return float(np.interp(q, cum, v))


def _load_spi(spi_dir: Path) -> pd.DataFrame:
    matches = sorted(spi_dir.glob("*.tab"))
    if not matches:
        raise SystemExit(f"No SPI .tab file in {spi_dir}")
    cols = ["FACT", "SRP", "TI", *_INCOME_MAP.values()]
    return pd.read_csv(matches[0], sep="\t", usecols=cols)


def _earnings_factor(earnings_index: dict[int, float], year: int) -> float:
    if year in earnings_index and _SPI_YEAR in earnings_index:
        return earnings_index[year] / earnings_index[_SPI_YEAR]
    return 1.0


def assign_top_incomes(
    persons: pd.DataFrame,
    households: pd.DataFrame,
    spi_dir: Path,
    year: int,
    earnings_index: dict[int, float],
    seed: int = 42,
) -> int:
    """Overwrite market-income streams for adults in the richest 10% of pensioner
    and non-pensioner FRS households with uprated SPI top-decile records.

    Mutates `persons` in place and returns the number of adults reassigned.
    """
    rng = np.random.default_rng(seed)
    spi = _load_spi(spi_dir)
    factor = _earnings_factor(earnings_index, year)

    # SPI donor pools, split by pensioner status, restricted to the FACT-weighted
    # top decile of total income within each pool.
    pools: dict[bool, tuple[np.ndarray, np.ndarray]] = {}
    spi_is_pensioner = spi["SRP"].to_numpy() > 0
    for is_pen in (True, False):
        pool = spi[spi_is_pensioner == is_pen]
        ti = pool["TI"].to_numpy(float)
        fact = pool["FACT"].to_numpy(float)
        thr = _weighted_quantile(ti, fact, _TOP_QUANTILE)
        top = pool[ti >= thr]
        donor = np.column_stack([top[c].to_numpy(float) * factor for c in _INCOME_MAP.values()])
        w = top["FACT"].to_numpy(float)
        pools[is_pen] = (donor, w / w.sum())

    # Per-adult pensioner flag (own state pension).
    adult = persons["age"].to_numpy() >= _ADULT_AGE
    person_is_pen = persons["state_pension"].to_numpy(float) > 0

    # Household pensioner flag + total income; select richest 10% of each group.
    inc_cols = list(_INCOME_MAP.keys()) + [
        "maintenance_income", "miscellaneous_income", "other_income",
    ]
    inc_cols = [c for c in inc_cols if c in persons.columns]
    persons_inc = persons[inc_cols].to_numpy(float).sum(axis=1)
    hh_income = pd.Series(persons_inc).groupby(persons["household_id"].to_numpy()).sum()
    hh_pen = pd.Series(person_is_pen).groupby(persons["household_id"].to_numpy()).any()

    weight = households.set_index("household_id")["weight"]
    selected: set = set()
    for is_pen in (True, False):
        ids = hh_pen.index[hh_pen.to_numpy() == is_pen]
        inc = hh_income.reindex(ids).to_numpy()
        w = weight.reindex(ids).to_numpy(float)
        thr = _weighted_quantile(inc, w, _TOP_QUANTILE)
        selected.update(ids[inc >= thr].tolist())

    in_selected = persons["household_id"].isin(selected).to_numpy()
    target = adult & in_selected
    out_cols = list(_INCOME_MAP.keys())
    arr = persons[out_cols].to_numpy(float)
    n = 0
    for is_pen in (True, False):
        mask = target & (person_is_pen == is_pen)
        idx = np.flatnonzero(mask)
        if idx.size == 0:
            continue
        donor, p = pools[is_pen]
        pick = rng.choice(donor.shape[0], size=idx.size, p=p)
        arr[idx] = donor[pick]
        n += idx.size
    persons[out_cols] = arr
    return n
