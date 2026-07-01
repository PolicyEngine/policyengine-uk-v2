"""Build clean EFRS microdata from a fixed pooled FRS panel + WAS/LCFS/SPI.

Fixed-panel design: pool the clean FRS years {2019, 2021-2024} (skipping the
covid-2020 collection) at 2024 prices, impute WAS wealth, LCFS consumption and
SPI top incomes ONCE, then shift the identical panel to every EFRS year
2016-2030 by de-uprating/uprating monetary columns and weights (data/uprate.py)
before calibrating each year's weights. Every year shares the same household
rows, so year-on-year differences come only from growth indices, calibrated
weights and policy — FRS sample rotation no longer enters the income-growth
series.

Usage:
    python data/efrs.py                           # build panel + all years
    python data/efrs.py --year 2023               # panel (cached) + one year
    python data/efrs.py --year 2023 --no-upload   # build only
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

# Imputation donors: the most recent available WAS + LCFS + SPI.
_WAS_DONOR = "was/round_8"
_LCFS_DONOR = "lcfs/2022"
_SPI_DONOR = "spi/2022"

# FRS years pooled into the fixed panel. 2020 is dropped (covid fieldwork);
# 2019 is included to keep pre-UC-rollout legacy-benefit claimants in the
# sample for the earliest EFRS years.
PANEL_FRS_YEARS = [2019, 2021, 2022, 2023, 2024]
# Price level the panel is built at (the most recent FRS year).
PANEL_BASE_YEAR = 2024
# EFRS years produced by shifting the panel. Capped at the last fiscal year
# the engine has policy parameters for: 2030/31 (parameters/2030_31.yaml).
EFRS_YEARS = list(range(2016, 2031))

console = Console()


def chain_order(years: list[int]) -> list[int]:
    """Order years for warm-start chaining: the panel base year first, then
    outwards (backwards to the earliest, then forwards), so each year's
    calibration can start from an already-calibrated neighbour."""
    below = sorted([y for y in years if y < PANEL_BASE_YEAR], reverse=True)
    above = sorted([y for y in years if y > PANEL_BASE_YEAR])
    base = [PANEL_BASE_YEAR] if PANEL_BASE_YEAR in years else []
    return base + below + above


def _warm_start_dir(year: int, work_dir: Path) -> Path | None:
    """Neighbouring year's clean dir to warm-start calibration from, if built.

    The panel base year always starts cold from its survey-weight snapshot;
    years below it chain from year+1, years above from year-1 (chain_order
    guarantees the neighbour is calibrated first in a full build). Returns
    None — a cold start — when the neighbour hasn't been built locally.
    """
    if year == PANEL_BASE_YEAR:
        return None
    neighbour = year + 1 if year < PANEL_BASE_YEAR else year - 1
    d = work_dir / "clean" / "efrs" / str(neighbour)
    return d if (d / "households.csv").exists() else None


def _has_targets(year: int) -> bool:
    import json

    targets = json.loads((REPO_ROOT / "data" / "calibration_targets.json").read_text())[
        "targets"
    ]
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
            [
                "gcloud",
                "storage",
                "cp",
                f"{BUCKET}/frs/{frs_year}/{fname}",
                str(frs_clean) + "/",
            ],
            check=True,
        )
    return frs_clean


def upload_clean(year: int, clean_dir: Path) -> None:
    csvs = sorted(clean_dir.glob("*.csv"))
    if not csvs:
        raise SystemExit(f"No CSVs in {clean_dir} — extraction must have failed")
    dest = f"{BUCKET}/efrs/{year}/"
    console.print(f"  uploading {len(csvs)} files → {dest}")
    subprocess.run(
        ["gcloud", "storage", "cp", *[str(f) for f in csvs], dest], check=True
    )


def _ensure_spi_block(work_dir: Path, panel_dir: Path, spi_raw: Path) -> Path:
    """Build (once) and cache the SPI high-earner block from the pooled panel
    frame. Injected verbatim into the panel by impute(), so each SPI earner
    keeps the same donor family wrapper and equivalised-income rank in every
    EFRS year."""
    block_dir = work_dir / "clean" / "spi_block"
    if (block_dir / "households.csv").exists():
        console.print(f"  [dim]SPI block cached at {block_dir}[/dim]")
        return block_dir

    import pandas as pd

    from build_targets import RAW_DIR, load_efo_earnings_index
    from spi_topup import build_spi_block
    from spi_unearned import impute_unearned_income

    console.print("[bold]Building SPI block from the pooled panel frame[/bold]")
    persons = pd.read_csv(panel_dir / "persons.csv")
    benunits = pd.read_csv(panel_dir / "benunits.csv")
    households = pd.read_csv(panel_dir / "households.csv")

    # Donor families carry imputed unearned income, matching the original flow.
    earnings_index = load_efo_earnings_index(RAW_DIR / "obr" / "efo_economy.xlsx")
    impute_unearned_income(persons, spi_raw, PANEL_BASE_YEAR, earnings_index)

    block_p, block_b, block_h = build_spi_block(persons, benunits, households, spi_raw)
    block_dir.mkdir(parents=True, exist_ok=True)
    block_p.to_csv(block_dir / "persons.csv", index=False)
    block_b.to_csv(block_dir / "benunits.csv", index=False)
    block_h.to_csv(block_dir / "households.csv", index=False)
    console.print(f"  cached {len(block_h)} SPI households → {block_dir}")
    return block_dir


def _panel_ready(panel_dir: Path) -> bool:
    """The panel is complete once imputation has run: imputed columns (e.g.
    net_financial_wealth) only appear in households.csv after impute()."""
    hh = panel_dir / "households.csv"
    if not hh.exists():
        return False
    with hh.open() as f:
        return "net_financial_wealth" in f.readline()


def build_panel(work_dir: Path) -> Path:
    """Pool the panel FRS years at PANEL_BASE_YEAR prices and impute once.

    Idempotent: skipped when the imputed panel already exists on disk.
    """
    panel_dir = work_dir / "clean" / "efrs" / "panel"
    if _panel_ready(panel_dir):
        console.print(f"[dim]panel cached at {panel_dir}[/dim]")
        return panel_dir

    console.rule(f"EFRS panel (FRS {PANEL_FRS_YEARS} @ {PANEL_BASE_YEAR} prices)")
    frs_base = work_dir / "clean" / "frs"
    for y in PANEL_FRS_YEARS:
        _ensure_frs_clean(y, work_dir)

    was_raw = work_dir / "raw" / _WAS_DONOR
    _download(_WAS_DONOR, was_raw)
    lcfs_raw = work_dir / "raw" / _LCFS_DONOR
    _download(_LCFS_DONOR, lcfs_raw)
    spi_raw = work_dir / "raw" / _SPI_DONOR
    _download(_SPI_DONOR, spi_raw)

    from pool import write_pooled

    used = write_pooled(frs_base, PANEL_BASE_YEAR, panel_dir, years=PANEL_FRS_YEARS)
    console.print(f"  pooled FRS years {used} → {panel_dir}")

    spi_block_dir = _ensure_spi_block(work_dir, panel_dir, spi_raw)

    from impute import impute

    impute(
        panel_dir,
        was_raw,
        lcfs_raw,
        year=PANEL_BASE_YEAR,
        spi_dir=spi_raw,
        spi_block_dir=spi_block_dir,
        spi_block_year=PANEL_BASE_YEAR,
    )
    return panel_dir


def build_year(
    year: int,
    work_dir: Path,
    panel_dir: Path,
    upload: bool = True,
    calibrate: bool = True,
) -> None:
    """Shift the imputed panel to `year` prices, snapshot weights, calibrate."""
    import pandas as pd

    from uprate import uprate_households, uprate_persons

    console.rule(f"EFRS {year} (panel shifted from {PANEL_BASE_YEAR})")
    out_dir = work_dir / "clean" / "efrs" / str(year)
    out_dir.mkdir(parents=True, exist_ok=True)
    console.print(f"  uprating panel {PANEL_BASE_YEAR} → {year} prices → {out_dir}")

    persons = pd.read_csv(panel_dir / "persons.csv")
    households = pd.read_csv(panel_dir / "households.csv")
    uprate_persons(persons, PANEL_BASE_YEAR, year).to_csv(
        out_dir / "persons.csv", index=False
    )
    uprate_households(households, PANEL_BASE_YEAR, year).to_csv(
        out_dir / "households.csv", index=False
    )
    # benunits carry no monetary fields — copy unchanged.
    pd.read_csv(panel_dir / "benunits.csv").to_csv(
        out_dir / "benunits.csv", index=False
    )

    # Snapshot the shifted survey weights as the cold-start baseline so
    # calibration can be re-run standalone (make calibrate) without rebuilding.
    from calibrate import snapshot_survey_weights

    snapshot_survey_weights(out_dir)

    if calibrate and _has_targets(year):
        console.print(f"  calibrating weights for EFRS {year}")
        from calibrate import CalibrateConfig
        from calibrate import run as run_calibration

        run_calibration(
            out_dir,
            year,
            CalibrateConfig(),
            warm_start_dir=_warm_start_dir(year, work_dir),
        )
    elif calibrate:
        console.print(
            f"  [yellow]no calibration targets for {year} — leaving shifted weights[/yellow]"
        )

    if upload:
        upload_clean(year, out_dir)


def calibrate_only(year: int, work_dir: Path) -> None:
    """Reweight an already-built clean dir without rebuilding.

    Skips pooling/imputation/uprating entirely: it reads the existing clean dir
    (built by `efrs.py --no-calibrate`), so calibration can be iterated on
    without rebuilding. The panel base year starts cold from the survey-weight
    snapshot written at build time; other years warm-start from their
    neighbour, so a full `make calibrate` pass (which runs in chain order) is
    reproducible no matter how many times it is re-run.
    """
    efrs_out = work_dir / "clean" / "efrs" / str(year)
    if not (efrs_out / "households.csv").exists():
        console.print(f"[yellow]skip {year}: no clean dir at {efrs_out}[/yellow]")
        return
    if not _has_targets(year):
        console.print(f"[yellow]skip {year}: no calibration targets[/yellow]")
        return
    console.rule(f"calibrate EFRS {year} (no rebuild)")
    from calibrate import CalibrateConfig
    from calibrate import run as run_calibration

    run_calibration(
        efrs_out,
        year,
        CalibrateConfig(),
        warm_start_dir=_warm_start_dir(year, work_dir),
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--year", type=int, choices=EFRS_YEARS, help="Single year to build"
    )
    parser.add_argument("--work-dir", type=Path, default=REPO_ROOT / "data")
    parser.add_argument("--no-upload", action="store_true")
    parser.add_argument("--no-calibrate", action="store_true")
    parser.add_argument(
        "--calibrate-only",
        action="store_true",
        help="Reweight existing clean dirs from their survey-weight snapshots "
        "without rebuilding (pool/impute/uprate skipped).",
    )
    args = parser.parse_args()

    years = chain_order([args.year] if args.year else EFRS_YEARS)

    if args.calibrate_only:
        for year in years:
            calibrate_only(year, args.work_dir)
        console.print("[green]Done.[/green]")
        return

    table = Table(title=f"Building EFRS for {len(years)} year(s)", show_header=True)
    table.add_column("Year", style="bold")
    table.add_column("Source")
    for y in years:
        table.add_row(str(y), f"panel (FRS {PANEL_FRS_YEARS}) shifted to {y}")
    console.print(table)

    try:
        panel_dir = build_panel(args.work_dir)
        for year in years:
            build_year(
                year,
                args.work_dir,
                panel_dir,
                upload=not args.no_upload,
                calibrate=not args.no_calibrate,
            )
    except subprocess.CalledProcessError as e:
        console.print(f"[red]Failed: {e}[/red]")
        sys.exit(1)

    console.print("[green]Done.[/green]")


if __name__ == "__main__":
    main()
