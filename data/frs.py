"""Build clean FRS microdata from raw UKDS tab files on GCS.

For each year: download raw files → run Rust extraction → upload clean CSVs.

Usage:
    python data/frs.py                           # build all years
    python data/frs.py --year 2023               # single year
    python data/frs.py --year 2023 --no-upload   # extract only, skip GCS upload
    python data/frs.py --raw-dir /tmp/frs/2023 --year 2023  # skip download
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

# FRS fiscal year → raw GCS path under {BUCKET}/ukds/
YEARS: dict[int, str] = {year: f"frs/{year}" for year in range(1994, 2024)}

console = Console()


def _binary() -> Path:
    built = REPO_ROOT / "target" / "release" / "policyengine-uk-rust"
    if built.exists():
        return built
    console.print("[yellow]Binary not found at target/release/, building...[/yellow]")
    subprocess.run(["cargo", "build", "--release"], cwd=REPO_ROOT, check=True)
    return built


def download_raw(year: int, dest: Path) -> None:
    if dest.exists() and any(dest.iterdir()):
        console.print(f"  [dim]raw cached at {dest}[/dim]")
        return
    dest.mkdir(parents=True, exist_ok=True)
    src = f"{BUCKET}/ukds/{YEARS[year]}/*"
    console.print(f"  downloading {src}")
    subprocess.run(["gcloud", "storage", "cp", "-r", src, str(dest)], check=True)


def extract(raw_dir: Path, year: int, output_dir: Path) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    console.print(f"  extracting year {year} → {output_dir}")
    subprocess.run(
        [str(_binary()), "--frs", str(raw_dir), "--year", str(year), "--extract", str(output_dir)],
        check=True,
    )


def upload_clean(year: int, clean_dir: Path) -> None:
    csvs = sorted(clean_dir.glob("*.csv"))
    if not csvs:
        raise SystemExit(f"No CSVs in {clean_dir} — extraction must have failed")
    dest = f"{BUCKET}/frs/{year}/"
    console.print(f"  uploading {len(csvs)} files → {dest}")
    subprocess.run(["gcloud", "storage", "cp", *[str(f) for f in csvs], dest], check=True)


def build(year: int, work_dir: Path, raw_dir: Path | None = None, upload: bool = True) -> None:
    console.rule(f"FRS {year}")
    raw = raw_dir or work_dir / "raw" / "frs" / str(year)
    clean = work_dir / "clean" / "frs" / str(year)
    if raw_dir is None:
        download_raw(year, raw)
    extract(raw, year, clean)
    if upload:
        upload_clean(year, clean)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--year", type=int, choices=sorted(YEARS), help="Single year to build")
    parser.add_argument("--work-dir", type=Path, default=REPO_ROOT / "data", help="Working directory")
    parser.add_argument("--raw-dir", type=Path, help="Pre-downloaded raw FRS directory (skips GCS download)")
    parser.add_argument("--no-upload", action="store_true", help="Skip GCS upload")
    args = parser.parse_args()

    if args.raw_dir and not args.year:
        parser.error("--raw-dir requires --year (one raw directory maps to one year)")

    years = [args.year] if args.year else sorted(YEARS)

    table = Table(title=f"Building FRS for {len(years)} year(s)", show_header=True)
    table.add_column("Year", style="bold")
    table.add_column("Raw ref")
    for y in years:
        table.add_row(str(y), YEARS[y])
    console.print(table)

    for year in years:
        try:
            build(year, args.work_dir, raw_dir=args.raw_dir, upload=not args.no_upload)
        except subprocess.CalledProcessError as e:
            console.print(f"[red]Failed on year {year}: {e}[/red]")
            sys.exit(1)

    console.print("[green]Done.[/green]")


if __name__ == "__main__":
    main()
