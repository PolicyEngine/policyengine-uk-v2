"""Generate parameter-YAML growth_factors from the OBR Economic and Fiscal Outlook.

Reads the OBR EFO "detailed forecast tables: economy" workbook (fiscal-year rows)
and writes the `growth_factors:` block of each `parameters/YYYY_YY.yaml`:

  cpi_rate         ← Table 1.7 "CPI" (year-on-year, fiscal year)
  gdp_deflator     ← Table 1.7 "GDP deflator"
  earnings_growth  ← Table 1.6 "Average weekly earnings growth"

The workbook is the canonical source; it lives in the microdata bucket at
`gs://policyengine-uk-microdata/obr/efo_2026_03_economy.xlsx` and is fetched on
run (cached locally). Only forecast years are rewritten by default — outturn
years carry values from their own historical releases and are left untouched
unless `--from-year` is lowered.

Usage::

    uv run python data/gen_growth_factors.py --dry-run     # show diff, write nothing
    uv run python data/gen_growth_factors.py               # rewrite forecast YAMLs
    uv run python data/gen_growth_factors.py --from-year 2025
"""

from __future__ import annotations

import argparse
import re
import subprocess
from pathlib import Path

import openpyxl
from rich.console import Console
from rich.table import Table

REPO_ROOT = Path(__file__).resolve().parent.parent
PARAMS_DIR = REPO_ROOT / "parameters"

GCS_OBJECT = "gs://policyengine-uk-microdata/obr/efo_2026_03_economy.xlsx"
LOCAL_CACHE = Path.home() / ".policyengine-uk-data" / "obr" / "efo_2026_03_economy.xlsx"
SOURCE_LABEL = "OBR Economic and Fiscal Outlook, March 2026"

# First fiscal year the OBR EFO treats as a forecast (data is outturn before this).
DEFAULT_FROM_YEAR = 2025

# Sheet / column locations, verified against the March 2026 workbook.
INFLATION_SHEET = "1.7"
INFLATION_CPI_COL = 4
INFLATION_GDP_DEFLATOR_COL = 11
LABOUR_SHEET = "1.6"
LABOUR_AWE_GROWTH_HEADER = "Average weekly earnings growth"

console = Console()

_FISCAL_RE = re.compile(r"^(\d{4})-(\d{2})$")


def ensure_workbook() -> Path:
    """Download the OBR workbook from GCS if not cached locally."""
    if LOCAL_CACHE.exists():
        return LOCAL_CACHE
    LOCAL_CACHE.parent.mkdir(parents=True, exist_ok=True)
    console.print(f"  downloading {GCS_OBJECT}")
    subprocess.run(["gcloud", "storage", "cp", GCS_OBJECT, str(LOCAL_CACHE)], check=True)
    return LOCAL_CACHE


def _fiscal_rows(ws) -> dict[int, tuple]:
    """Map the entered fiscal year (e.g. 2026 for '2026-27') to its row tuple."""
    out: dict[int, tuple] = {}
    for r in ws.iter_rows(values_only=True):
        label = r[1] if len(r) > 1 else None
        if isinstance(label, str):
            m = _FISCAL_RE.match(label.strip())
            if m:
                out[int(m.group(1))] = r
    return out


def _awe_growth_col(ws) -> int:
    header = next(ws.iter_rows(min_row=3, max_row=3, values_only=True))
    for j, h in enumerate(header):
        if isinstance(h, str) and LABOUR_AWE_GROWTH_HEADER in h:
            return j
    raise RuntimeError(f"Could not find '{LABOUR_AWE_GROWTH_HEADER}' column in sheet {LABOUR_SHEET}")


def read_obr_rates(xlsx: Path) -> dict[int, dict[str, float]]:
    """Return {fiscal_year: {cpi_rate, gdp_deflator, earnings_growth}} (as fractions)."""
    wb = openpyxl.load_workbook(xlsx, read_only=True, data_only=True)
    infl = _fiscal_rows(wb[INFLATION_SHEET])
    lab_ws = wb[LABOUR_SHEET]
    lab = _fiscal_rows(lab_ws)
    awe_col = _awe_growth_col(lab_ws)

    rates: dict[int, dict[str, float]] = {}
    for year in sorted(set(infl) & set(lab)):
        cpi = infl[year][INFLATION_CPI_COL]
        gdp = infl[year][INFLATION_GDP_DEFLATOR_COL]
        awe = lab[year][awe_col]
        if cpi is None or gdp is None or awe is None:
            continue
        rates[year] = {
            "cpi_rate": cpi / 100.0,
            "gdp_deflator": gdp / 100.0,
            "earnings_growth": awe / 100.0,
        }
    return rates


def _yaml_path(year: int) -> Path:
    return PARAMS_DIR / f"{year}_{(year + 1) % 100:02d}.yaml"


def _render_block(year: int, r: dict[str, float]) -> str:
    fy = f"{year}/{(year + 1) % 100:02d}"
    return (
        "growth_factors:\n"
        f"  # {SOURCE_LABEL}\n"
        "  # Table 1.7 (Inflation) and Table 1.6 (Labour Market), fiscal-year rows\n"
        f"  cpi_rate: {r['cpi_rate']:.5f}        # {r['cpi_rate'] * 100:.3f}% CPI inflation (FY {fy})\n"
        f"  gdp_deflator: {r['gdp_deflator']:.5f}    # {r['gdp_deflator'] * 100:.3f}% GDP deflator\n"
        f"  earnings_growth: {r['earnings_growth']:.5f}  # {r['earnings_growth'] * 100:.3f}% average weekly earnings growth\n"
    )


def _read_existing(year: int) -> dict[str, float]:
    path = _yaml_path(year)
    if not path.exists():
        return {}
    import yaml

    with path.open() as f:
        data = yaml.safe_load(f)
    return (data or {}).get("growth_factors") or {}


def _rewrite_block(year: int, block: str) -> bool:
    """Replace the trailing growth_factors block in the YAML. Returns True if written."""
    path = _yaml_path(year)
    if not path.exists():
        return False
    text = path.read_text()
    idx = text.find("\ngrowth_factors:")
    if idx == -1:
        # No existing block: append after a blank line.
        new_text = text.rstrip("\n") + "\n\n" + block
    else:
        new_text = text[: idx + 1] + block
    if not new_text.endswith("\n"):
        new_text += "\n"
    path.write_text(new_text)
    return True


def main() -> int:
    ap = argparse.ArgumentParser(description="Generate growth_factors YAML blocks from OBR EFO tables.")
    ap.add_argument("--dry-run", action="store_true", help="Show the diff; write nothing.")
    ap.add_argument("--from-year", type=int, default=DEFAULT_FROM_YEAR,
                    help=f"First fiscal year to (re)write (default {DEFAULT_FROM_YEAR}).")
    ap.add_argument("--to-year", type=int, default=None, help="Last fiscal year to write (default: all available).")
    args = ap.parse_args()

    xlsx = ensure_workbook()
    rates = read_obr_rates(xlsx)
    years = [y for y in sorted(rates) if y >= args.from_year and (args.to_year is None or y <= args.to_year)]

    table = Table(title=f"growth_factors from {SOURCE_LABEL}")
    table.add_column("FY"); table.add_column("field")
    table.add_column("old", justify="right"); table.add_column("new", justify="right")
    table.add_column("Δ", justify="right")

    written = 0
    for year in years:
        r = rates[year]
        old = _read_existing(year)
        for k in ("cpi_rate", "gdp_deflator", "earnings_growth"):
            ov = old.get(k)
            nv = r[k]
            delta = "" if ov is None else f"{(nv - ov) * 100:+.3f}pp"
            mark = "" if (ov is not None and abs(nv - ov) < 5e-6) else " *"
            table.add_row(
                f"{year}/{(year + 1) % 100:02d}", k,
                "—" if ov is None else f"{ov * 100:.3f}%",
                f"{nv * 100:.3f}%{mark}", delta,
            )
        if not args.dry_run and _yaml_path(year).exists():
            if _rewrite_block(year, _render_block(year, r)):
                written += 1

    console.print(table)
    if args.dry_run:
        console.print("[yellow]dry run — no files written. '*' marks a changed value.[/yellow]")
    else:
        console.print(f"[green]wrote growth_factors to {written} YAML files ({years[0]}–{years[-1]}).[/green]")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
