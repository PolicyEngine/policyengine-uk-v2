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

console = Console()


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
        console.print(f"  calibrating weights for EFRS {year}")
        from calibrate import CalibrateConfig
        from calibrate import run as run_calibration

        run_calibration(out_dir, year, CalibrateConfig())
    elif calibrate:
        console.print(f"  [yellow]no calibration targets for {year} — leaving uprated weights[/yellow]")

    if upload:
        upload_clean(year, out_dir)


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

    impute(efrs_out, was_raw, lcfs_raw, year=year, spi_dir=spi_raw)

    if calibrate and _has_targets(year):
        console.print(f"  calibrating weights for EFRS {year}")
        from calibrate import CalibrateConfig
        from calibrate import run as run_calibration

        run_calibration(efrs_out, year, CalibrateConfig())
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
