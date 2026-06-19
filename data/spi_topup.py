"""Inject real SPI high-income earners onto FRS-shaped families.

The FRS under-samples very high incomes, so weight calibration cannot reach the
SPI tail-band targets (the worst calibration errors are all spi_* bands).
Reweighting changes how many copies of a record exist but never the income a
record carries, so the FRS simply has no records in the high bands to upweight.

The calibration targets (build_targets.build_spi_targets) are aggregates of the
SPI Table 3.6 tape — a per-band count_nonzero and sum of each income source over
people whose total income falls in the band. So the way to hit both the count
and the amount in every band is to put the SPI tape's own high-income records
into the dataset at the tape's own FACT grossing weight: the aggregate then
matches by construction.

Each SPI record is an individual (a tax tape), but treating each high earner as
a childless single badly distorts household-equivalised income and poverty —
high earners disproportionately have partners and children. So instead of
fabricating a single, we clone a real FRS household whose head matches the SPI
record's age band and sex, overwrite the head's income streams with the SPI
amounts, and keep the donor's partner/children (carrying their own FRS incomes)
and housing shell. The person-level band targets still reproduce by construction:
the head's total income lands in the SPI band exactly. To avoid double-counting
we first drop every FRS household containing a ≥threshold earner, then append the
clones at the SPI record's FACT weight. SPI amounts are uprated to the FRS year
by the EFO average-earnings index.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pandas as pd
from rich.console import Console

console = Console()

# FRS person income column ← SPI tape column. State pension (SRP) is mapped
# separately onto state_pension so the record's total income (and the SPI
# state-pension band targets) reproduce; the six below are the market-income tail.
_INCOME_MAP = {
    "employment_income": "PAY",
    "self_employment_income": "PROFITS",
    "private_pension_income": "PENSION",
    "savings_interest": "INCBBS",
    "dividend_income": "DIVIDENDS",
    "property_income": "INCPROP",
}

# Every income-bearing person column zeroed on the overwritten head before the
# SPI streams are written, so the head's total income equals the SPI record's.
_HEAD_ZERO_COLS = [
    "employment_income", "self_employment_income", "private_pension_income",
    "state_pension", "savings_interest", "dividend_income", "capital_gains",
    "capital_gains_residential_share", "property_income", "maintenance_income",
    "miscellaneous_income", "other_income",
]

_SPI_YEAR = 2022  # the put2223uk.tab tape is fiscal year 2022-23
_DEFAULT_THRESHOLD = 100_000.0  # year-nominal total income above which to inject

# AGERANGE band → representative age, and the FRS-age → band inverse used to
# pool donor households by the head's age band. SRP recipients are forced old
# enough that the engine passes their state pension through (person_state_pension
# returns the carried amount only for age ≥ 66 + year − 2016), via _SP_AGE.
_AGERANGE_AGE = {1: 21, 2: 30, 3: 40, 4: 50, 5: 60, 6: 70, 7: 80, -1: 50}
_AGE_BAND_EDGES = (25, 35, 45, 55, 65, 75)  # → bands 1..7
_SP_AGE = 80
_SEX_GENDER = {1: "male", 2: "female", 0: "male"}


def _load_spi(spi_dir: Path) -> pd.DataFrame:
    matches = sorted(spi_dir.glob("*.tab"))
    if not matches:
        raise SystemExit(f"No SPI .tab file in {spi_dir}")
    cols = ["FACT", "SRP", "SEX", "AGERANGE", "TI", *_INCOME_MAP.values()]
    return pd.read_csv(matches[0], sep="\t", usecols=cols)


def _earnings_factor(earnings_index: dict[int, float], year: int) -> float:
    if year in earnings_index and _SPI_YEAR in earnings_index:
        return earnings_index[year] / earnings_index[_SPI_YEAR]
    return 1.0


def _age_to_band(age: np.ndarray) -> np.ndarray:
    """Map FRS head age to the SPI AGERANGE band scheme (1..7)."""
    return np.digitize(age, _AGE_BAND_EDGES) + 1


def _drop_high_earners(
    persons: pd.DataFrame,
    benunits: pd.DataFrame,
    households: pd.DataFrame,
    threshold: float,
) -> tuple[pd.DataFrame, pd.DataFrame, pd.DataFrame, int]:
    """Drop every household containing an adult whose own market income clears
    `threshold`, so the injected SPI tail does not double-count FRS high earners.
    Returns the filtered frames and the number of households removed.
    """
    market = persons[list(_INCOME_MAP.keys())].to_numpy(float).sum(axis=1)
    is_high = (persons["age"].to_numpy() >= 18) & (market >= threshold)
    drop_hh = set(persons.loc[is_high, "household_id"].unique())
    persons = persons[~persons["household_id"].isin(drop_hh)].reset_index(drop=True)
    benunits = benunits[~benunits["household_id"].isin(drop_hh)].reset_index(drop=True)
    households = households[~households["household_id"].isin(drop_hh)].reset_index(drop=True)
    return persons, benunits, households, len(drop_hh)


def build_spi_block(
    persons: pd.DataFrame,
    benunits: pd.DataFrame,
    households: pd.DataFrame,
    spi_dir: Path,
    threshold: float = _DEFAULT_THRESHOLD,
    seed: int = 42,
) -> tuple[pd.DataFrame, pd.DataFrame, pd.DataFrame]:
    """Build the SPI high-earner household block ONCE, at TAPE-NOMINAL incomes.

    Clones a demographically-matched FRS family per ≥threshold SPI tape record and
    overwrites its head's income with the record's (un-uprated) streams, at the
    record's FACT weight. The block is built a single time from the most recent
    survey year and reused verbatim across all years (see inject_spi_block), so
    the donor family wrapping each SPI earner — hence its equivalisation factor
    and equivalised-income rank — is constant year to year. Re-drawing the donor
    each year churned the top-decile composition and injected spurious D10 jitter.

    Returns the injected (persons, benunits, households) block; head incomes are
    tape-nominal and rescaled to each target year by inject_spi_block.
    """
    rng = np.random.default_rng(seed)
    spi = _load_spi(spi_dir)

    # SPI tail: records whose TAPE-NOMINAL total income clears the threshold.
    # Thresholding on the tape value (not an uprated value) keeps the injected
    # record set identical across all years — only the amounts are rescaled.
    ti = spi["TI"].to_numpy(float)
    spi = spi[ti >= threshold].reset_index(drop=True)
    n_inject = len(spi)
    if n_inject == 0:
        empty = persons.iloc[0:0].copy(), benunits.iloc[0:0].copy(), households.iloc[0:0].copy()
        return empty

    # Donor pool = the source frame minus its own high earners (avoids cloning a
    # family that is itself being dropped as a double-count elsewhere).
    persons, benunits, households, _ = _drop_high_earners(
        persons, benunits, households, threshold
    )

    # Donor pool: surviving households, bucketed by their head's (age band, sex).
    heads = persons[persons["is_household_head"]]
    head_hh = heads["household_id"].to_numpy()
    head_band = _age_to_band(heads["age"].to_numpy(float))
    head_female = (heads["gender"].to_numpy() == "female")
    hh_pos = {h: i for i, h in enumerate(households["household_id"].to_numpy())}
    head_pos = np.array([hh_pos[h] for h in head_hh])

    buckets: dict[tuple[int, bool], np.ndarray] = {}
    for band in range(1, 8):
        for female in (False, True):
            sel = (head_band == band) & (head_female == female)
            if sel.any():
                buckets[(band, female)] = head_pos[sel]
    any_male = head_pos[~head_female]
    any_female = head_pos[head_female]

    # Match each SPI record to a donor household position.
    spi_band = np.array([int(a) if a in _AGERANGE_AGE else 4 for a in spi["AGERANGE"]])
    spi_band = np.where(spi_band == -1, 4, spi_band)
    spi_female = spi["SEX"].to_numpy() == 2
    picks = np.empty(n_inject, dtype=int)
    for i in range(n_inject):
        b, f = int(spi_band[i]), bool(spi_female[i])
        pool = buckets.get((b, f))
        if pool is None:
            pool = any_female if f else any_male
        picks[i] = pool[rng.integers(len(pool))]

    # Explode chosen donor households into clone person/benunit rows.
    persons_by_hh = persons.groupby("household_id").indices
    bu_by_hh = benunits.groupby("household_id").indices
    donor_hid = households["household_id"].to_numpy()[picks]

    p_lists = [persons_by_hh[h] for h in donor_hid]
    p_clone = np.repeat(np.arange(n_inject), [len(r) for r in p_lists])
    p_src = np.concatenate(p_lists)
    b_lists = [bu_by_hh[h] for h in donor_hid]
    b_clone = np.repeat(np.arange(n_inject), [len(r) for r in b_lists])
    b_src = np.concatenate(b_lists)

    # Fresh IDs that cannot collide with any surviving record.
    pid0 = int(persons["person_id"].max()) + 1
    buid0 = int(benunits["benunit_id"].max()) + 1
    hid0 = int(households["household_id"].max()) + 1
    new_hid = np.arange(hid0, hid0 + n_inject)

    new_p = persons.iloc[p_src].reset_index(drop=True)
    new_b = benunits.iloc[b_src].reset_index(drop=True)
    new_h = households.iloc[picks].reset_index(drop=True)

    new_p["person_id"] = np.arange(pid0, pid0 + len(new_p))
    new_b["benunit_id"] = np.arange(buid0, buid0 + len(new_b))
    new_p["household_id"] = new_hid[p_clone]
    new_b["household_id"] = new_hid[b_clone]
    new_h["household_id"] = new_hid

    # Remap each clone's donor benunit_id → its fresh benunit_id, keyed per clone.
    old_bu_b = benunits["benunit_id"].to_numpy()[b_src]
    bu_map = {(c, o): n for c, o, n in zip(b_clone, old_bu_b, new_b["benunit_id"].to_numpy())}
    old_bu_p = persons["benunit_id"].to_numpy()[p_src]
    new_p["benunit_id"] = [bu_map[(c, o)] for c, o in zip(p_clone, old_bu_p)]

    # Overwrite each clone head's income with the SPI record's TAPE-NOMINAL
    # streams. inject_spi_block rescales these to each target year by the EFO
    # earnings factor — matching how build_spi_targets uprates the band amounts —
    # so the band aggregates reproduce by construction in every year.
    head_mask = new_p["is_household_head"].to_numpy()
    head_clone = p_clone[head_mask]
    for col in _HEAD_ZERO_COLS:
        new_p.loc[head_mask, col] = 0.0
    for frs_col, spi_col in _INCOME_MAP.items():
        new_p.loc[head_mask, frs_col] = spi[spi_col].to_numpy(float)[head_clone]
    srp = spi["SRP"].to_numpy(float)[head_clone]
    new_p.loc[head_mask, "state_pension"] = srp
    # SRP recipients must be old enough for the engine to pass state pension through.
    head_idx = new_p.index[head_mask]
    new_p.loc[head_idx[srp > 0], "age"] = _SP_AGE

    # Household weight = the SPI record's FACT, so the band aggregates reproduce.
    new_h["weight"] = spi["FACT"].to_numpy(float)

    # Stable provenance keys for the SPI block. The block is built once and reused
    # verbatim every year, so a positional key identifies the same injected earner
    # across years — overwriting any donor-inherited key (which would collide when
    # two SPI records clone the same donor). Lets calibration warm-start SPI weights.
    new_h["provenance"] = [f"spi:{i}" for i in range(n_inject)]

    # Regenerate the ';'-joined membership strings from the fresh IDs.
    pids_by_bu = new_p.groupby("benunit_id")["person_id"].apply(
        lambda s: ";".join(map(str, s))
    )
    pids_by_hh = new_p.groupby("household_id")["person_id"].apply(
        lambda s: ";".join(map(str, s))
    )
    buids_by_hh = new_b.groupby("household_id")["benunit_id"].apply(
        lambda s: ";".join(map(str, s))
    )
    new_b["person_ids"] = new_b["benunit_id"].map(pids_by_bu)
    new_h["person_ids"] = new_h["household_id"].map(pids_by_hh)
    new_h["benunit_ids"] = new_h["household_id"].map(buids_by_hh)

    return new_p, new_b, new_h


# SPI head income columns rescaled by the earnings factor each year (the six
# market streams plus state pension). All other head streams are zeroed at build.
_HEAD_INCOME_COLS = list(_INCOME_MAP.keys()) + ["state_pension"]


def inject_spi_block(
    persons: pd.DataFrame,
    benunits: pd.DataFrame,
    households: pd.DataFrame,
    block: tuple[pd.DataFrame, pd.DataFrame, pd.DataFrame],
    block_year: int,
    year: int,
    earnings_index: dict[int, float],
    threshold: float = _DEFAULT_THRESHOLD,
) -> tuple[pd.DataFrame, pd.DataFrame, pd.DataFrame, int, int]:
    """Drop FRS high earners and append a prebuilt SPI block, rescaled to `year`.

    The block (built once by build_spi_block at `block_year`, tape-nominal heads)
    is reused verbatim every year so each SPI earner keeps the same donor family
    and equivalisation factor. Per year: the donor wrapper is uprated generically
    block_year→year; the SPI head's income is set to its tape value × the EFO
    earnings factor; household weights stay at the SPI FACT (band counts are held
    fixed across years). IDs are re-based above the current frame to avoid clashes.

    Returns (persons, benunits, households, n_injected, n_removed_households).
    """
    from pool import _offset_id_list
    from uprate import uprate_households, uprate_persons

    block_p, block_b, block_h = block
    n_inject = len(block_h)

    persons, benunits, households, n_drop = _drop_high_earners(
        persons, benunits, households, threshold
    )
    if n_inject == 0:
        return persons, benunits, households, 0, n_drop

    # Uprate the donor wrapper generically (block_year→year), then overwrite the
    # SPI heads' income with tape × earnings factor (tape values are preserved in
    # the block, so re-derive them rather than reading post-uprating columns).
    head_mask = block_p["is_household_head"].to_numpy()
    tape = {c: block_p.loc[head_mask, c].to_numpy(float).copy() for c in _HEAD_INCOME_COLS}

    new_p = uprate_persons(block_p, block_year, year).reset_index(drop=True)
    f = _earnings_factor(earnings_index, year)
    for c in _HEAD_INCOME_COLS:
        new_p.loc[head_mask, c] = tape[c] * f

    new_h = uprate_households(block_h, block_year, year).reset_index(drop=True)
    new_h["weight"] = block_h["weight"].to_numpy(float)  # FACT fixed across years
    new_b = block_b.copy().reset_index(drop=True)

    # Re-base block IDs above the surviving frame's max so nothing collides.
    p_off = int(persons["person_id"].max()) + 1
    b_off = int(benunits["benunit_id"].max()) + 1
    h_off = int(households["household_id"].max()) + 1
    new_p["person_id"] = new_p["person_id"].to_numpy() + p_off
    new_p["benunit_id"] = new_p["benunit_id"].to_numpy() + b_off
    new_p["household_id"] = new_p["household_id"].to_numpy() + h_off
    new_b["benunit_id"] = new_b["benunit_id"].to_numpy() + b_off
    new_b["household_id"] = new_b["household_id"].to_numpy() + h_off
    new_b["person_ids"] = _offset_id_list(new_b["person_ids"], p_off)
    new_h["household_id"] = new_h["household_id"].to_numpy() + h_off
    new_h["person_ids"] = _offset_id_list(new_h["person_ids"], p_off)
    new_h["benunit_ids"] = _offset_id_list(new_h["benunit_ids"], b_off)

    persons = pd.concat([persons, new_p], ignore_index=True)
    benunits = pd.concat([benunits, new_b], ignore_index=True)
    households = pd.concat([households, new_h], ignore_index=True)
    return persons, benunits, households, n_inject, n_drop
