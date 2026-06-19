"""Build clean EFRS microdata from FRS (clean) + WAS + LCFS raw files on GCS.

EFRS (Enhanced FRS) merges FRS household microdata with WAS (wealth) and LCFS
(expenditure). Imputation runs in Python (data/impute.py); the Rust binary is
only used downstream for the baseline simulation during weight calibration.

Usage:
    python data/efrs.py                           # build all years
    python data/efrs.py --year 2023               # single year
    python data/efrs.py --year 2023 --no-upload   # extract only
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

from rich.console import Console
from rich.table import Table

BUCKET = "gs://policyengine-uk-microdata"
REPO_ROOT = Path(__file__).resolve().parent.parent

# EFRS fiscal year → (frs_year, was_gcs_ref, lcfs_gcs_ref). Every FRS year is a
# recipient; the donor pool is the most recent available WAS + LCFS for all years.
_WAS_DONOR = "was/round_8"
_LCFS_DONOR = "lcfs/2022"
_SPI_DONOR = "spi/2022"
# Number of consecutive FRS years pooled per EFRS year (target + preceding).
POOL_N_YEARS = 3
YEARS: dict[int, tuple[int, str, str, str]] = {
    y: (y, _WAS_DONOR, _LCFS_DONOR, _SPI_DONOR) for y in range(2010, 2025)
}

# Forecast years have no FRS/WAS/LCFS data: they uprate the latest real EFRS year
# (2024) to OBR EFO price levels and calibrate to uprated forecast targets.
# Capped at the last fiscal year the engine has policy parameters for (2029/30);
# 2030 has no parameters/2030_31.yaml so the baseline simulation can't run.
FORECAST_BASE_YEAR = 2024
FORECAST_YEARS = list(range(2025, 2030))

# The SPI high-earner block is built ONCE from the most recent survey year's
# pooled FRS frame, then reused verbatim every year (only amounts/weights are
# rescaled). Building it once fixes the donor family wrapping each SPI earner, so
# its equivalised-income rank is stable year to year (avoids top-decile churn).
SPI_BLOCK_YEAR = max(YEARS)

console = Console()


def warm_start_weights(out_dir: Path, year: int, work_dir: Path) -> None:
    """Write a `start_weight` column seeded from the previous year's calibrated
    solution, matched by the cross-year `provenance` key.

    Each EFRS year pools a 3-year FRS window, so ~2/3 of households recur from
    year to year (same provenance key); the SPI block is identical every year.
    Calibration is underdetermined in the decile dimension — deciles aren't
    targets, so the solver is free to wander the null space, and solving each
    year cold from the survey weights lands on a different null-space point,
    churning decile composition into a spurious year-on-year sawtooth. Starting
    the optimiser from last year's solution makes it converge to a nearby point,
    so unchanged records keep their weight and only genuine target moves shift it.

    This only sets the optimiser's STARTING POINT (calibrate's `start_weights`).
    The survey `weight` column is left untouched so it still anchors the
    deviation penalty and the max-weight-ratio clamp — otherwise a record needing
    a big shift would be boxed to within 10× of last year's value instead of its
    survey value, which blows up the fit. Records with no prior match (the freshly
    rotated-in FRS year, plus dropped high earners) fall back to the survey weight.
    """
    import pandas as pd

    from uprate import cumulative_factor

    prev_path = work_dir / "clean" / "efrs" / str(year - 1) / "households.csv"
    if not prev_path.exists():
        return
    cur = pd.read_csv(out_dir / "households.csv")
    prev = pd.read_csv(prev_path)
    if "provenance" not in cur.columns or "provenance" not in prev.columns:
        return

    # Scale prior calibrated weights to this year's population so the warm start
    # starts at the right grossed total (calibration then fine-tunes).
    pop = cumulative_factor(year - 1, year, "population")
    prev_w = dict(zip(prev["provenance"], prev["weight"].to_numpy(float) * pop))
    matched = cur["provenance"].map(prev_w)
    n_matched = int(matched.notna().sum())
    cur["start_weight"] = matched.fillna(cur["weight"]).to_numpy(float)
    cur.to_csv(out_dir / "households.csv", index=False)
    console.print(
        f"  warm-started {n_matched}/{len(cur)} weights from EFRS {year - 1}"
    )


def _has_targets(year: int) -> bool:
    import json

    targets = json.loads((REPO_ROOT / "data" / "calibration_targets.json").read_text())["targets"]
    return any(t["year"] == year for t in targets)


def _download(gcs_ref: str, dest: Path) -> None:
    if dest.exists() and any(dest.iterdir()):
        console.print(f"  [dim]cached at {dest}[/dim]")
        return
    dest.mkdir(parents=True, exist_ok=True)
    src = f"{BUCKET}/ukds/{gcs_ref}/*"
    console.print(f"  downloading {src}")
    subprocess.run(["gcloud", "storage", "cp", "-r", src, str(dest)], check=True)


def _ensure_frs_clean(frs_year: int, work_dir: Path) -> Path:
    frs_clean = work_dir / "clean" / "frs" / str(frs_year)
    if frs_clean.exists() and (frs_clean / "households.csv").exists():
        console.print(f"  [dim]FRS {frs_year} clean cached at {frs_clean}[/dim]")
        return frs_clean
    frs_clean.mkdir(parents=True, exist_ok=True)
    console.print(f"  downloading FRS {frs_year} clean from GCS")
    for fname in ("persons.csv", "benunits.csv", "households.csv"):
        subprocess.run(
            ["gcloud", "storage", "cp", f"{BUCKET}/frs/{frs_year}/{fname}", str(frs_clean) + "/"],
            check=True,
        )
    return frs_clean


def upload_clean(year: int, clean_dir: Path) -> None:
    csvs = sorted(clean_dir.glob("*.csv"))
    if not csvs:
        raise SystemExit(f"No CSVs in {clean_dir} — extraction must have failed")
    dest = f"{BUCKET}/efrs/{year}/"
    console.print(f"  uploading {len(csvs)} files → {dest}")
    subprocess.run(["gcloud", "storage", "cp", *[str(f) for f in csvs], dest], check=True)


def _ensure_efrs_base(base_year: int, work_dir: Path) -> Path:
    """Return the latest real EFRS clean dir, downloading from GCS if absent."""
    base_dir = work_dir / "clean" / "efrs" / str(base_year)
    if base_dir.exists() and (base_dir / "households.csv").exists():
        console.print(f"  [dim]EFRS {base_year} base cached at {base_dir}[/dim]")
        return base_dir
    base_dir.mkdir(parents=True, exist_ok=True)
    console.print(f"  downloading EFRS {base_year} base from GCS")
    for fname in ("persons.csv", "benunits.csv", "households.csv"):
        subprocess.run(
            ["gcloud", "storage", "cp", f"{BUCKET}/efrs/{base_year}/{fname}", str(base_dir) + "/"],
            check=True,
        )
    return base_dir


def build_forecast(year: int, work_dir: Path, upload: bool = True, calibrate: bool = True) -> None:
    """Build a forecast-year EFRS by uprating the latest real EFRS to OBR prices.

    No new survey data exists past FORECAST_BASE_YEAR, so we take that year's
    pooled+imputed clean dir, uprate every monetary column (data/uprate.py) and
    the weights (population index) to `year`, then calibrate to the uprated
    forecast targets (built by data/build_targets.py).
    """
    import pandas as pd

    from uprate import uprate_households, uprate_persons

    console.rule(f"EFRS {year} (forecast)")
    base_dir = _ensure_efrs_base(FORECAST_BASE_YEAR, work_dir)

    out_dir = work_dir / "clean" / "efrs" / str(year)
    out_dir.mkdir(parents=True, exist_ok=True)
    console.print(f"  uprating EFRS {FORECAST_BASE_YEAR} → {year} prices → {out_dir}")

    persons = pd.read_csv(base_dir / "persons.csv")
    households = pd.read_csv(base_dir / "households.csv")
    uprate_persons(persons, FORECAST_BASE_YEAR, year).to_csv(out_dir / "persons.csv", index=False)
    uprate_households(households, FORECAST_BASE_YEAR, year).to_csv(out_dir / "households.csv", index=False)
    # benunits carry no monetary fields — copy unchanged.
    pd.read_csv(base_dir / "benunits.csv").to_csv(out_dir / "benunits.csv", index=False)

    if calibrate and _has_targets(year):
        warm_start_weights(out_dir, year, work_dir)
        console.print(f"  calibrating weights for EFRS {year}")
        from calibrate import CalibrateConfig
        from calibrate import run as run_calibration

        run_calibration(out_dir, year, CalibrateConfig(weight_deviation_penalty=0.0))
    elif calibrate:
        console.print(f"  [yellow]no calibration targets for {year} — leaving uprated weights[/yellow]")

    if upload:
        upload_clean(year, out_dir)


def _ensure_spi_block(work_dir: Path) -> Path:
    """Build (once) and cache the SPI high-earner block from SPI_BLOCK_YEAR's
    pooled FRS frame. Reused verbatim by every year's impute() call so each SPI
    earner keeps the same donor family wrapper and equivalised-income rank.
    """
    block_dir = work_dir / "clean" / "spi_block"
    if (block_dir / "households.csv").exists():
        console.print(f"  [dim]SPI block cached at {block_dir}[/dim]")
        return block_dir

    import pandas as pd

    from build_targets import RAW_DIR, load_efo_earnings_index
    from pool import pool_frs_years
    from spi_topup import build_spi_block
    from spi_unearned import impute_unearned_income

    frs_year = YEARS[SPI_BLOCK_YEAR][0]
    spi_raw = work_dir / "raw" / YEARS[SPI_BLOCK_YEAR][3]
    _download(YEARS[SPI_BLOCK_YEAR][3], spi_raw)
    frs_base = work_dir / "clean" / "frs"
    for y in range(frs_year - POOL_N_YEARS + 1, frs_year + 1):
        if y >= 1994:
            _ensure_frs_clean(y, work_dir)

    console.print(f"[bold]Building SPI block from {SPI_BLOCK_YEAR} pooled FRS[/bold]")
    persons, benunits, households = pool_frs_years(frs_base, frs_year, n_years=POOL_N_YEARS)

    # Donor families carry imputed unearned income, matching the original flow.
    earnings_index = load_efo_earnings_index(RAW_DIR / "obr" / "efo_economy.xlsx")
    impute_unearned_income(persons, spi_raw, SPI_BLOCK_YEAR, earnings_index)

    block_p, block_b, block_h = build_spi_block(persons, benunits, households, spi_raw)
    block_dir.mkdir(parents=True, exist_ok=True)
    block_p.to_csv(block_dir / "persons.csv", index=False)
    block_b.to_csv(block_dir / "benunits.csv", index=False)
    block_h.to_csv(block_dir / "households.csv", index=False)
    console.print(f"  cached {len(block_h)} SPI households → {block_dir}")
    return block_dir


def build(year: int, work_dir: Path, upload: bool = True, calibrate: bool = True) -> None:
    if year in FORECAST_YEARS:
        build_forecast(year, work_dir, upload=upload, calibrate=calibrate)
        return
    console.rule(f"EFRS {year}")
    frs_year, was_ref, lcfs_ref, spi_ref = YEARS[year]

    frs_base = work_dir / "clean" / "frs"
    # Pool the target FRS year with the two preceding years (uprated to the
    # target's price level) to triple the sample and smooth the calibrated
    # poverty series. The window shrinks only if earlier years are unavailable.
    pool_years = [y for y in range(frs_year - POOL_N_YEARS + 1, frs_year + 1) if y >= 1994]
    for y in pool_years:
        _ensure_frs_clean(y, work_dir)

    was_raw = work_dir / "raw" / was_ref
    _download(was_ref, was_raw)

    lcfs_raw = work_dir / "raw" / lcfs_ref
    _download(lcfs_ref, lcfs_raw)

    spi_raw = work_dir / "raw" / spi_ref
    _download(spi_ref, spi_raw)

    efrs_out = work_dir / "clean" / "efrs" / str(year)
    efrs_out.mkdir(parents=True, exist_ok=True)
    console.print(f"  building EFRS {year} → {efrs_out}")

    # Write the pooled, uprated, re-based clean FRS frame, then impute on it.
    from pool import write_pooled

    used = write_pooled(frs_base, frs_year, efrs_out, n_years=POOL_N_YEARS)
    console.print(f"  pooled FRS years {used} → {efrs_out}")

    from impute import impute

    spi_block_dir = _ensure_spi_block(work_dir)
    impute(efrs_out, was_raw, lcfs_raw, year=year, spi_dir=spi_raw,
           spi_block_dir=spi_block_dir, spi_block_year=SPI_BLOCK_YEAR)

    if calibrate and _has_targets(year):
        warm_start_weights(efrs_out, year, work_dir)
        console.print(f"  calibrating weights for EFRS {year}")
        from calibrate import CalibrateConfig
        from calibrate import run as run_calibration

        run_calibration(efrs_out, year, CalibrateConfig(weight_deviation_penalty=0.0))
    elif calibrate:
        console.print(f"  [yellow]no calibration targets for {year} — leaving uprated weights[/yellow]")

    if upload:
        upload_clean(year, efrs_out)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    all_years = sorted(YEARS) + FORECAST_YEARS
    parser.add_argument("--year", type=int, choices=all_years, help="Single year to build")
    parser.add_argument("--work-dir", type=Path, default=REPO_ROOT / "data")
    parser.add_argument("--no-upload", action="store_true")
    parser.add_argument("--no-calibrate", action="store_true")
    args = parser.parse_args()

    years = [args.year] if args.year else all_years

    table = Table(title=f"Building EFRS for {len(years)} year(s)", show_header=True)
    table.add_column("Year", style="bold")
    table.add_column("FRS base")
    table.add_column("WAS")
    table.add_column("LCFS")
    table.add_column("SPI")
    for y in years:
        if y in FORECAST_YEARS:
            table.add_row(str(y), f"uprated {FORECAST_BASE_YEAR}", "—", "—", "—")
        else:
            frs_year, was_ref, lcfs_ref, spi_ref = YEARS[y]
            table.add_row(str(y), str(frs_year), was_ref, lcfs_ref, spi_ref)
    console.print(table)

    for year in years:
        try:
            build(year, args.work_dir, upload=not args.no_upload, calibrate=not args.no_calibrate)
        except subprocess.CalledProcessError as e:
            console.print(f"[red]Failed on year {year}: {e}[/red]")
            sys.exit(1)

    console.print("[green]Done.[/green]")


if __name__ == "__main__":
    main()
