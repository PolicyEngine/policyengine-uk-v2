"""Pool adjacent FRS years onto a common price level for EFRS calibration.

For EFRS year Y we pool the clean FRS samples {Y-2, Y-1, Y}, uprating the two
donor years to Y's prices (data/uprate.py) before combining. Pooling triples the
effective sample, which tightens the weighted median and stops sparse-target
noise being amplified into year-on-year poverty swings. Weights are scaled by
1/n_years so the grossed population still matches Y (calibration fine-tunes).

IDs (person/benunit/household, including the ';'-joined cross-reference lists in
households.person_ids/benunit_ids and benunits.person_ids) are re-based per donor
so they stay unique and internally consistent across the pool.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pandas as pd

from uprate import uprate_households, uprate_persons


def _offset_id_list(s: pd.Series, offset: int) -> pd.Series:
    """Add `offset` to every id in a ';'-joined id-list column."""
    def shift(cell: str) -> str:
        if not isinstance(cell, str) or cell == "":
            return cell
        return ";".join(str(int(x) + offset) for x in cell.split(";"))
    return s.map(shift)


def _rebase(persons: pd.DataFrame, benunits: pd.DataFrame, households: pd.DataFrame,
            p_off: int, b_off: int, h_off: int) -> tuple[pd.DataFrame, pd.DataFrame, pd.DataFrame]:
    p, b, h = persons.copy(), benunits.copy(), households.copy()

    p["person_id"] = p["person_id"].to_numpy() + p_off
    p["benunit_id"] = p["benunit_id"].to_numpy() + b_off
    p["household_id"] = p["household_id"].to_numpy() + h_off

    b["benunit_id"] = b["benunit_id"].to_numpy() + b_off
    b["household_id"] = b["household_id"].to_numpy() + h_off
    b["person_ids"] = _offset_id_list(b["person_ids"], p_off)

    h["household_id"] = h["household_id"].to_numpy() + h_off
    h["benunit_ids"] = _offset_id_list(h["benunit_ids"], b_off)
    h["person_ids"] = _offset_id_list(h["person_ids"], p_off)
    return p, b, h


def pool_frs_years(frs_base: Path, target_year: int, n_years: int = 3) -> tuple[
        pd.DataFrame, pd.DataFrame, pd.DataFrame]:
    """Build a pooled, uprated, re-based clean FRS frame for `target_year`.

    Pools {target_year-n_years+1 .. target_year}, dropping any years whose clean
    dir is absent (so the window shrinks only if data genuinely doesn't exist).
    """
    years = [y for y in range(target_year - n_years + 1, target_year + 1)
             if (frs_base / str(y) / "households.csv").exists()]
    if not years:
        raise SystemExit(f"No FRS clean data for pooling around {target_year}")

    p_parts, b_parts, h_parts = [], [], []
    p_off = b_off = h_off = 0
    for y in years:
        d = frs_base / str(y)
        persons = pd.read_csv(d / "persons.csv")
        benunits = pd.read_csv(d / "benunits.csv")
        households = pd.read_csv(d / "households.csv")

        if y != target_year:
            persons = uprate_persons(persons, y, target_year)
            households = uprate_households(households, y, target_year)

        persons, benunits, households = _rebase(
            persons, benunits, households, p_off, b_off, h_off)

        p_parts.append(persons)
        b_parts.append(benunits)
        h_parts.append(households)

        p_off += len(persons)
        b_off += len(benunits)
        h_off += len(households)

    pooled_p = pd.concat(p_parts, ignore_index=True)
    pooled_b = pd.concat(b_parts, ignore_index=True)
    pooled_h = pd.concat(h_parts, ignore_index=True)

    # Scale weights by 1/n_years so the grossed population matches the target
    # year (each donor year was already population-uprated to target_year).
    pooled_h["weight"] = pooled_h["weight"].to_numpy(dtype=float) / len(years)

    return pooled_p, pooled_b, pooled_h


def write_pooled(frs_base: Path, target_year: int, out_dir: Path, n_years: int = 3) -> list[int]:
    """Pool and write persons/benunits/households CSVs into out_dir. Returns the years used."""
    years = [y for y in range(target_year - n_years + 1, target_year + 1)
             if (frs_base / str(y) / "households.csv").exists()]
    p, b, h = pool_frs_years(frs_base, target_year, n_years)
    out_dir.mkdir(parents=True, exist_ok=True)
    p.to_csv(out_dir / "persons.csv", index=False)
    b.to_csv(out_dir / "benunits.csv", index=False)
    h.to_csv(out_dir / "households.csv", index=False)
    return years
