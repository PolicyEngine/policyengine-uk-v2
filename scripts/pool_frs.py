"""Pool multiple FRS survey years into a single dataset for EFRS construction.

Concatenates persons, benunits, and households CSVs from multiple years,
reindexing all IDs to avoid collisions and dividing weights by the number
of years pooled (since each year independently represents the full population).

Usage:
    python scripts/pool_frs.py --years 2021 2022 2023 --data-dir ~/.policyengine-uk-data/frs --output data/frs_pooled/2023
"""

from __future__ import annotations

import argparse
import csv
from pathlib import Path


def pool(years: list[int], data_dir: Path, output_dir: Path) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    n_years = len(years)

    # First pass: determine ID offsets per year
    offsets = []  # (person_offset, benunit_offset, household_offset)
    p_off, b_off, h_off = 0, 0, 0
    for year in years:
        year_dir = data_dir / str(year)
        with open(year_dir / "persons.csv") as f:
            n_persons = sum(1 for _ in f) - 1
        with open(year_dir / "benunits.csv") as f:
            n_benunits = sum(1 for _ in f) - 1
        with open(year_dir / "households.csv") as f:
            n_households = sum(1 for _ in f) - 1
        offsets.append((p_off, b_off, h_off))
        p_off += n_persons
        b_off += n_benunits
        h_off += n_households

    # Pool persons — use union of all columns across years
    all_persons = []
    all_person_fields: list[str] = []
    for year, (po, _, _) in zip(years, offsets):
        with open(data_dir / str(year) / "persons.csv") as f:
            reader = csv.DictReader(f)
            for col in reader.fieldnames:
                if col not in all_person_fields:
                    all_person_fields.append(col)
            for row in reader:
                row["person_id"] = str(int(row["person_id"]) + po)
                all_persons.append(row)

    with open(output_dir / "persons.csv", "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=all_person_fields, restval="0")
        writer.writeheader()
        writer.writerows(all_persons)
    print(f"  Pooled {len(all_persons):,} persons")

    # Pool benunits — harmonise column names across FRS format changes.
    # FRS 2021/2022 uses: migration_seed, would_claim_uc, would_claim_cb, ...
    # FRS 2023 uses: take_up_seed, reported_uc, reported_cb, ..., is_enr_uc, ...
    # The EFRS writer and loader expect the would_claim_* format.
    _BU_RENAME = {
        "take_up_seed": "migration_seed",
        "reported_uc": "would_claim_uc",
        "reported_cb": "would_claim_cb",
        "reported_hb": "would_claim_hb",
        "reported_pc": "would_claim_pc",
        "reported_ctc": "would_claim_ctc",
        "reported_wtc": "would_claim_wtc",
        "reported_is": "would_claim_is",
    }
    # Columns to drop (is_enr_* are not used in the would_claim format)
    _BU_DROP = {
        "is_enr_uc", "is_enr_hb", "is_enr_pc", "is_enr_cb",
        "is_enr_ctc", "is_enr_wtc", "is_enr_is", "is_enr_esa", "is_enr_jsa",
    }

    all_benunits = []
    all_bu_fields: list[str] = []
    for year, (po, bo, ho) in zip(years, offsets):
        with open(data_dir / str(year) / "benunits.csv") as f:
            reader = csv.DictReader(f)
            for col in reader.fieldnames:
                mapped = _BU_RENAME.get(col, col)
                if mapped not in _BU_DROP and mapped not in all_bu_fields:
                    all_bu_fields.append(mapped)
            for row in reader:
                # Rename columns
                renamed = {}
                for k, v in row.items():
                    mapped = _BU_RENAME.get(k, k)
                    if mapped not in _BU_DROP:
                        renamed[mapped] = v
                renamed["benunit_id"] = str(int(renamed["benunit_id"]) + bo)
                renamed["household_id"] = str(int(renamed["household_id"]) + ho)
                pids = renamed["person_ids"].split(";")
                renamed["person_ids"] = ";".join(str(int(p) + po) for p in pids)
                all_benunits.append(renamed)

    with open(output_dir / "benunits.csv", "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=all_bu_fields, restval="false")
        writer.writeheader()
        writer.writerows(all_benunits)
    print(f"  Pooled {len(all_benunits):,} benunits")

    # Pool households
    all_households = []
    all_hh_fields: list[str] = []
    for year, (po, bo, ho) in zip(years, offsets):
        with open(data_dir / str(year) / "households.csv") as f:
            reader = csv.DictReader(f)
            for col in reader.fieldnames:
                if col not in all_hh_fields:
                    all_hh_fields.append(col)
            for row in reader:
                row["household_id"] = str(int(row["household_id"]) + ho)
                bids = row["benunit_ids"].split(";")
                row["benunit_ids"] = ";".join(str(int(b) + bo) for b in bids)
                pids = row["person_ids"].split(";")
                row["person_ids"] = ";".join(str(int(p) + po) for p in pids)
                row["weight"] = str(float(row["weight"]) / n_years)
                all_households.append(row)

    with open(output_dir / "households.csv", "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=all_hh_fields, restval="0")
        writer.writeheader()
        writer.writerows(all_households)
    print(f"  Pooled {len(all_households):,} households (weights / {n_years})")

    total_w = sum(float(r["weight"]) for r in all_households)
    n_uc = sum(1 for b in all_benunits if b.get("on_uc") == "true")
    print(f"  Total weight: {total_w:,.0f}, UC benunits: {n_uc:,}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__ or "")
    parser.add_argument("--years", type=int, nargs="+", required=True)
    parser.add_argument("--data-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    print(f"Pooling FRS years {args.years} from {args.data_dir}")
    pool(args.years, args.data_dir, args.output)
    print("Done.")


if __name__ == "__main__":
    main()
