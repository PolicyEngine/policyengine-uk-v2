"""Impute unearned income (interest, dividends, property) onto FRS persons.

The FRS badly under-records investment income, especially dividends across the
£15-100k earned-income mass (HMRC SPI Table 3.7 has ~3m dividend recipients the
FRS misses). Weight calibration cannot fix this — it can only upweight records
that exist, and the FRS has almost no dividend recipients there to upweight.

So we learn the conditional P(unearned | earned, demographics) from the SPI tape,
which observes both sides per person, and predict the three unearned streams onto
every FRS adult. Predictors (employment, self-employment, private pension, state
pension, age, sex) are present on both surveys; the SPI fit is FACT-weighted via
weighted bootstrap so the rich (over-sampled by SPI) don't dominate. SPI amounts
are uprated to the FRS year by the EFO average-earnings index.

This runs before the SPI high-income injection, so it only touches FRS-origin
persons; the injected SPI singles keep their own tape-reported unearned income.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pandas as pd
from rich.console import Console
from sklearn.ensemble import RandomForestClassifier, RandomForestRegressor

console = Console()

# FRS person column ← SPI tape column for the three Table 3.7 unearned streams.
_TARGET_MAP = {
    "savings_interest": "INCBBS",
    "dividend_income": "DIVIDENDS",
    "property_income": "INCPROP",
}

# Earned-income predictors (FRS column ← SPI tape column), shared by both surveys.
_PREDICTOR_MAP = {
    "employment_income": "PAY",
    "self_employment_income": "PROFITS",
    "private_pension_income": "PENSION",
    "state_pension": "SRP",
}

_SPI_YEAR = 2022
_MAX_TRAIN = 30_000  # FACT-weighted bootstrap sample size for the RF fit
_ADULT_AGE = 18

# AGERANGE band → representative age, to give the RF a demographic predictor.
_AGERANGE_AGE = {1: 21, 2: 30, 3: 40, 4: 50, 5: 60, 6: 70, 7: 80, -1: 50}


def _load_spi(spi_dir: Path) -> pd.DataFrame:
    matches = sorted(spi_dir.glob("*.tab"))
    if not matches:
        raise SystemExit(f"No SPI .tab file in {spi_dir}")
    cols = ["FACT", "SEX", "AGERANGE", *_PREDICTOR_MAP.values(), *_TARGET_MAP.values()]
    return pd.read_csv(matches[0], sep="\t", usecols=cols)


def _earnings_factor(earnings_index: dict[int, float], year: int) -> float:
    if year in earnings_index and _SPI_YEAR in earnings_index:
        return earnings_index[year] / earnings_index[_SPI_YEAR]
    return 1.0


def impute_unearned_income(
    persons: pd.DataFrame,
    spi_dir: Path,
    year: int,
    earnings_index: dict[int, float],
    n_trees: int = 100,
    seed: int = 42,
) -> int:
    """Overwrite the three unearned-income streams on every FRS adult with a
    two-part (hurdle) random-forest prediction conditioned on earned income and
    demographics, trained on the (FACT-weighted) SPI tape.

    A mean regressor smears a tiny amount onto everyone, which over-counts
    recipients ~9×, so each stream uses a classifier for incidence (drawn
    stochastically against its probability, preserving the zero mass) and a
    regressor — trained on recipients only — for the amount. Mutates `persons`;
    returns the FRS adult count.
    """
    rng = np.random.default_rng(seed)
    spi = _load_spi(spi_dir)
    factor = _earnings_factor(earnings_index, year)

    # SPI training features: earned streams (uprated) + age + sex.
    spi_age = np.array([_AGERANGE_AGE.get(int(a), 50) for a in spi["AGERANGE"]], dtype=float)
    spi_female = (spi["SEX"].to_numpy() == 2).astype(float)
    spi_earned = np.column_stack(
        [spi[c].to_numpy(float) * factor for c in _PREDICTOR_MAP.values()]
    )
    spi_feat = np.column_stack([spi_earned, spi_age, spi_female])

    # FACT-weighted bootstrap so SPI's over-sampling of the rich doesn't bias the fit.
    fact = spi["FACT"].to_numpy(float)
    draw = rng.choice(len(spi), size=min(_MAX_TRAIN, len(spi)), p=fact / fact.sum())
    train_feat = spi_feat[draw]

    # FRS prediction features in the same order.
    age = persons["age"].to_numpy(float)
    female = (persons["gender"].to_numpy() == "female").astype(float)
    frs_earned = np.column_stack(
        [persons[c].to_numpy(float) for c in _PREDICTOR_MAP.keys()]
    )
    predict_feat = np.column_stack([frs_earned, age, female])

    adult = age >= _ADULT_AGE
    for i, (frs_col, spi_col) in enumerate(_TARGET_MAP.items()):
        y_full = spi[spi_col].to_numpy(float) * factor
        y = y_full[draw]
        has = y > 0

        # Part 1: incidence — predicted P(receipt), realised by a uniform draw.
        clf = RandomForestClassifier(n_estimators=n_trees, random_state=seed + i, n_jobs=-1)
        clf.fit(train_feat, has.astype(int))
        prob = clf.predict_proba(predict_feat)[:, list(clf.classes_).index(1)]
        recipient = rng.random(len(persons)) < prob

        # Part 2: amount — regressor trained on recipients only.
        reg = RandomForestRegressor(n_estimators=n_trees, random_state=seed + i, n_jobs=-1)
        reg.fit(train_feat[has], y[has])
        amount = np.maximum(reg.predict(predict_feat), 0.0)

        out = np.where(adult & recipient, amount, 0.0)
        persons[frs_col] = out

    return int(adult.sum())
