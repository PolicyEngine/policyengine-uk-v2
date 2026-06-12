"""Build clean EFRS microdata from FRS (clean) + WAS + LCFS raw files on GCS.

EFRS (Enhanced FRS) merges FRS household microdata with WAS (wealth) and LCFS
(expenditure). The Rust binary handles the imputation; Python orchestrates.

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

# EFRS fiscal year → (frs_year, was_gcs_ref, lcfs_gcs_ref)
YEARS: dict[int, tuple[int, str, str]] = {
    2023: (2023, "was/round_7", "lcfs/2021"),
}

console = Console()


def _binary() -> Path:
    built = REPO_ROOT / "target" / "release" / "policyengine-uk-rust"
    if built.exists():
        return built
    console.print("[yellow]Binary not found at target/release/, building...[/yellow]")
    subprocess.run(["cargo", "build", "--release"], cwd=REPO_ROOT, check=True)
    return built


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


def build(year: int, work_dir: Path, upload: bool = True) -> None:
    console.rule(f"EFRS {year}")
    frs_year, was_ref, lcfs_ref = YEARS[year]

    frs_base = work_dir / "clean" / "frs"
    _ensure_frs_clean(frs_year, work_dir)

    was_raw = work_dir / "raw" / was_ref
    _download(was_ref, was_raw)

    lcfs_raw = work_dir / "raw" / lcfs_ref
    _download(lcfs_ref, lcfs_raw)

    efrs_out = work_dir / "clean" / "efrs" / str(year)
    efrs_out.mkdir(parents=True, exist_ok=True)
    console.print(f"  extracting EFRS {year} → {efrs_out}")
    subprocess.run(
        [
            str(_binary()),
            "--extract-efrs", str(efrs_out),
            "--data", str(frs_base),
            "--year", str(year),
            "--was-dir", str(was_raw),
            "--lcfs-dir", str(lcfs_raw),
        ],
        check=True,
    )

    if upload:
        upload_clean(year, efrs_out)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--year", type=int, choices=sorted(YEARS), help="Single year to build")
    parser.add_argument("--work-dir", type=Path, default=REPO_ROOT / "data")
    parser.add_argument("--no-upload", action="store_true")
    args = parser.parse_args()

    years = [args.year] if args.year else sorted(YEARS)

    table = Table(title=f"Building EFRS for {len(years)} year(s)", show_header=True)
    table.add_column("Year", style="bold")
    table.add_column("FRS base")
    table.add_column("WAS")
    table.add_column("LCFS")
    for y in years:
        frs_year, was_ref, lcfs_ref = YEARS[y]
        table.add_row(str(y), str(frs_year), was_ref, lcfs_ref)
    console.print(table)

    for year in years:
        try:
            build(year, args.work_dir, upload=not args.no_upload)
        except subprocess.CalledProcessError as e:
            console.print(f"[red]Failed on year {year}: {e}[/red]")
            sys.exit(1)

    console.print("[green]Done.[/green]")


if __name__ == "__main__":
    main()
