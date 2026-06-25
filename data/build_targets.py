"""Build calibration targets JSON covering 2010–2024.

Sources:
  - HMRC receipts ODS (data/raw/hmrc_receipts.ods): income tax, NI, CGT, VAT, stamp duty
  - DWP/OBR expenditure (data/raw/obr/dwp.xlsx): all major benefits
  - Eurostat nama_10_co3_p3 + ONS QNA: COICOP household consumption at current prices
  - DWP/OBR expenditure (data/raw/obr/dwp.xlsx): benefit claimant caseloads

Usage:
    python data/build_targets.py
    python data/build_targets.py --years 2020 2023
    python data/build_targets.py --no-download   # skip re-downloading raw files
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.request
from pathlib import Path

import openpyxl
import pandas as pd
import requests

from uprate import cumulative_factor

REPO_ROOT = Path(__file__).resolve().parent.parent
RAW_DIR = REPO_ROOT / "data" / "raw"
OUT_PATH = REPO_ROOT / "data" / "calibration_targets.json"

HMRC_ODS_URL = (
    "https://assets.publishing.service.gov.uk/media/"
    "6a0c6b6efcae986635db91f0/NS_Table.ods"
)
ONS_QNA_URL = (
    "https://www.ons.gov.uk/file?uri=/economy/grossdomesticproductgdp/datasets/"
    "uksecondestimateofgdpdatatables/quarter1jantomar2026firstestimate/"
    "firstquarterlyestimatedatatablescorrected.xlsx"
)

TARGET_YEARS = list(range(2010, 2025))

# Forecast horizon: the last year with real source data, and the OBR EFO years we
# project onto by uprating the latest real targets (data/uprate.py indices).
LATEST_REAL_YEAR = 2024
FORECAST_YEARS = list(range(2025, 2030))


# ── helpers ─────────────────────────────────────────────────────────────────

def _download(url: str, dest: Path) -> None:
    if dest.exists():
        return
    dest.parent.mkdir(parents=True, exist_ok=True)
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req, timeout=60) as r:
        dest.write_bytes(r.read())


def _ods_rows(path: Path, sheet_name: str) -> list[list]:
    import odf.opendocument
    import odf.table

    def cell_text(cell):
        parts = []
        for el in cell.childNodes:
            if hasattr(el, "data"):
                parts.append(el.data)
            elif el.qname[1] == "p":
                for sub in el.childNodes:
                    if hasattr(sub, "data"):
                        parts.append(sub.data)
        return "".join(parts)

    doc = odf.opendocument.load(str(path))
    for s in doc.spreadsheet.getElementsByType(odf.table.Table):
        if s.getAttribute("name") == sheet_name:
            rows = []
            for row in s.getElementsByType(odf.table.TableRow):
                cells = []
                for cell in row.getElementsByType(odf.table.TableCell):
                    repeat = int(cell.getAttribute("numbercolumnsrepeated") or 1)
                    cells.extend([cell_text(cell)] * repeat)
                rows.append(cells)
            return rows
    raise KeyError(sheet_name)


def _xls_col(ws, row: int, col: int):
    v = ws.cell(row=row, column=col).value
    return v


# ── HMRC receipts ────────────────────────────────────────────────────────────

def load_hmrc_receipts(path: Path) -> dict[int, dict[str, float]]:
    """Return {fiscal_year_start: {field: £bn}}."""
    rows = _ods_rows(path, "Receipts_Annually")

    # Row index 5 (0-based) is the header
    header = rows[5]
    col = {
        "income_tax": next(i for i, h in enumerate(header) if h == "Income Tax"),
        "employee_ni": next(i for i, h in enumerate(header) if "EMP'ee" in h),
        "employer_ni": next(i for i, h in enumerate(header) if "EMP'er" in h),
        "total_nic": next(i for i, h in enumerate(header) if h == "National Insurance Contributions"),
        "capital_gains_tax": next(i for i, h in enumerate(header) if h == "Capital Gains Tax"),
        "vat": next(i for i, h in enumerate(header) if h == "Value Added Tax"),
        "stamp_duty": next(i for i, h in enumerate(header) if h == "Stamp Duty Land Tax"),
    }

    out: dict[int, dict[str, float]] = {}
    for row in rows[6:]:
        m = re.match(r"(\d{4}) to (\d{4})", row[0]) if row else None
        if not m:
            continue
        fy = int(m.group(1))
        if fy < 2010 or fy > 2024:
            continue

        def get(c: int) -> float | None:
            v = row[c].replace(",", "").replace(" ", "").strip()
            try:
                return float(v) / 1000.0  # £m -> £bn
            except ValueError:
                return None

        # NI: use employer+employee if available, else split total proportionally
        emp_ee = get(col["employee_ni"])
        emp_er = get(col["employer_ni"])
        total_nic = get(col["total_nic"])

        if emp_ee is not None and emp_er is not None:
            employee_ni = emp_ee
            employer_ni = emp_er
        elif total_nic is not None:
            # 2010-2014: split not available; use 2015 ratio of employee/(employee+employer)
            # 2015 ratio from the data: emp_ee=44.792, emp_er=61.969, total=~107
            # employee share ≈ 0.420
            employee_ni = total_nic * 0.420
            employer_ni = total_nic * 0.580
        else:
            employee_ni = employer_ni = None

        out[fy] = {
            "income_tax": get(col["income_tax"]),
            "employee_ni": employee_ni,
            "employer_ni": employer_ni,
            "capital_gains_tax": get(col["capital_gains_tax"]),
            "vat": get(col["vat"]),
            "stamp_duty": get(col["stamp_duty"]),
        }

    return out


def load_obr_databank_receipts(path: Path) -> dict[int, dict[str, float]]:
    """Tax receipts (£bn) from the OBR public finances databank 'Receipts (£bn)' sheet.

    Covers 1999-00 onwards — used to backfill years the HMRC monthly bulletin
    (which starts 2005-06) lacks. NICs are a single total here, so split with the
    same 0.420 employee share the HMRC loader uses for years without a breakdown.
    Values agree with HMRC to within a few per cent at the overlap.
    """
    wb = openpyxl.load_workbook(str(path), data_only=True)
    ws = wb["Receipts (£bn)"]

    # Column layout fixed by the databank header (row 4):
    cols = {"vat": 3, "stamp_duty": 6, "cgt": 20, "nics": 28,
            "paye": 17, "sa": 18, "other_it": 19}

    out: dict[int, dict[str, float]] = {}
    for r in range(7, ws.max_row + 1):
        y = ws.cell(row=r, column=2).value
        if not (isinstance(y, str) and re.match(r"\d{4}-\d{2}", y)):
            continue
        fy = int(y[:4])

        def g(key: str) -> float:
            v = ws.cell(row=r, column=cols[key]).value
            return float(v) if isinstance(v, (int, float)) else 0.0

        nics = g("nics")
        out[fy] = {
            "income_tax": g("paye") + g("sa") + g("other_it"),
            "employee_ni": nics * 0.420,
            "employer_ni": nics * 0.580,
            "capital_gains_tax": g("cgt"),
            "vat": g("vat"),  # net of refunds, matching HMRC's VAT line
            "stamp_duty": g("stamp_duty"),
        }
    return out


def load_efo_labour_market(path: Path) -> dict[int, dict[str, float]]:
    """Employment and ILO unemployment levels (counts) by fiscal year from the
    OBR EFO economy table 1.6.

    The table is quarterly (rows "YYYYQn"); column 2 is employment (16+, millions)
    and column 5 is ILO unemployment (16+, millions). Each fiscal year is the mean
    of Q2..Q1 (e.g. 2010 = 2010Q2..2011Q1), matching the FRS year convention. Levels
    returned as raw person counts (millions x 1e6).
    """
    wb = openpyxl.load_workbook(str(path), data_only=True)
    ws = wb["1.6"]

    quarters: dict[tuple[int, int], dict[str, float]] = {}
    for row in ws.iter_rows(values_only=True):
        label = row[1]
        m = re.match(r"(\d{4})Q([1-4])", str(label)) if label else None
        if not m:
            continue
        emp, unemp = row[2], row[5]
        if isinstance(emp, (int, float)) and isinstance(unemp, (int, float)):
            quarters[(int(m.group(1)), int(m.group(2)))] = {
                "employed": float(emp) * 1e6,
                "unemployed": float(unemp) * 1e6,
            }

    out: dict[int, dict[str, float]] = {}
    for fy in TARGET_YEARS:
        fy_quarters = [(fy, 2), (fy, 3), (fy, 4), (fy + 1, 1)]
        vals = [quarters[q] for q in fy_quarters if q in quarters]
        if len(vals) == 4:
            out[fy] = {
                "employed": sum(v["employed"] for v in vals) / 4.0,
                "unemployed": sum(v["unemployed"] for v in vals) / 4.0,
            }
    return out


# ── SPI income distribution (Table 3.6) ─────────────────────────────────────

# HMRC SPI Table 3.6 by total-income band, per income source: number of
# individuals (thousands) and amount (£m). Two column layouts across the file
# vintages: xls/xlsx (wide, with spacer columns) and ods (compact). Each source
# maps to an engine person variable.
_SPI_SOURCES = [  # (key, person_variable)
    ("self_employment", "self_employment_income"),
    ("employment", "employment_income"),
    ("state_pension", "state_pension"),
    ("private_pension", "private_pension_income"),
]
_SPI_COLS_WIDE = {"band": 0, "self_employment": (2, 3), "employment": (6, 7),
                  "state_pension": (10, 11), "private_pension": (14, 15)}
_SPI_COLS_ODS = {"band": 0, "self_employment": (1, 2), "employment": (4, 5),
                 "state_pension": (7, 8), "private_pension": (10, 11)}


def load_spi_table_3_6(path: Path) -> list[dict]:
    """Return banded distribution rows from one SPI Table 3.6 file.

    Each row: {lo, hi, <src>_n (count), <src>_a (£m amount)} for each source in
    _SPI_SOURCES, where the band runs [lo, hi) over total income. The top band's
    hi is +inf. Rows whose band cell is not a positive number are skipped.
    """
    if path.suffix == ".xls":
        engine, cols, sheet = "xlrd", _SPI_COLS_WIDE, 0
    elif path.suffix == ".xlsx":
        engine, cols, sheet = "openpyxl", _SPI_COLS_WIDE, 0
    else:
        xl = pd.ExcelFile(path, engine="odf")
        sheet = next(s for s in ("Table_3_6", "T3_6") if s in xl.sheet_names)
        engine, cols = "odf", _SPI_COLS_ODS

    df = pd.read_excel(path, sheet_name=sheet, header=None, engine=engine)

    def num(v: object) -> float | None:
        return float(v) if isinstance(v, (int, float)) and pd.notna(v) else None

    rows: list[dict] = []
    for i in range(len(df)):
        lo = num(df.iloc[i, cols["band"]])
        if lo is None or lo <= 0:
            continue
        rec: dict[str, float] = {"lo": lo}
        for key, _ in _SPI_SOURCES:
            cc, ca = cols[key]
            rec[f"{key}_n"] = num(df.iloc[i, cc]) or 0.0
            rec[f"{key}_a"] = num(df.iloc[i, ca]) or 0.0
        rows.append(rec)
    # The very first numeric row in the xls/xlsx vintages is a stray header echo;
    # drop any leading row with no source counts at all.
    rows = [r for r in rows if any(r[f"{k}_n"] > 0 for k, _ in _SPI_SOURCES)]
    for k in range(len(rows)):
        rows[k]["hi"] = rows[k + 1]["lo"] if k + 1 < len(rows) else float("inf")
    return rows


def load_spi_distributions(spi_dir: Path) -> dict[int, list[dict]]:
    """Load all SPI Table 3.6 files keyed by fiscal-year start (2010 = 2010-11)."""
    out: dict[int, list[dict]] = {}
    for path in sorted(spi_dir.glob("Table_3_6_*")):
        m = re.search(r"_(\d{4})\.", path.name)
        if m:
            out[int(m.group(1))] = load_spi_table_3_6(path)
    return out


# ── SPI investment income (Table 3.7) ───────────────────────────────────────

# HMRC SPI Table 3.7 by total-income band: number of individuals (thousands) and
# amount (£m) of property, interest and dividend income — the unearned-income
# sources Table 3.6 omits. Two layouts: compact ods (number/amount in adjacent
# columns) and wide xlsx (a spacer column splits each source's number/amount).
# "Other income" is deliberately skipped: it has no clean engine counterpart.
_SPI37_SOURCES = [  # (key, person_variable)
    ("property", "property_income"),
    ("interest", "savings_interest"),
    ("dividends", "dividend_income"),
]
_SPI37_COLS_WIDE = {"band": 0, "property": (2, 3), "interest": (6, 7), "dividends": (10, 11)}
_SPI37_COLS_ODS = {"band": 0, "property": (1, 2), "interest": (4, 5), "dividends": (7, 8)}


def load_spi_table_3_7(path: Path) -> list[dict]:
    """Return banded investment-income rows from one SPI Table 3.7 file.

    Each row: {lo, hi, <src>_n (count, thousands), <src>_a (£m)} for each source
    in _SPI37_SOURCES, band [lo, hi) over total income; the top band's hi is +inf.
    """
    if path.suffix in (".xlsx", ".xls"):
        engine = "openpyxl" if path.suffix == ".xlsx" else "xlrd"
        cols = _SPI37_COLS_WIDE
        names = pd.ExcelFile(path, engine=engine).sheet_names
        sheet = next((s for s in names if "3.7" in s or "3_7" in s), names[0])
    else:
        xl = pd.ExcelFile(path, engine="odf")
        sheet = next(s for s in ("Table_3_7", "T3_7") if s in xl.sheet_names)
        engine, cols = "odf", _SPI37_COLS_ODS

    df = pd.read_excel(path, sheet_name=sheet, header=None, engine=engine)

    def num(v: object) -> float | None:
        return float(v) if isinstance(v, (int, float)) and pd.notna(v) else None

    rows: list[dict] = []
    for i in range(len(df)):
        lo = num(df.iloc[i, cols["band"]])
        if lo is None or lo <= 0:
            continue
        rec: dict[str, float] = {"lo": lo}
        for key, _ in _SPI37_SOURCES:
            cc, ca = cols[key]
            rec[f"{key}_n"] = num(df.iloc[i, cc]) or 0.0
            rec[f"{key}_a"] = num(df.iloc[i, ca]) or 0.0
        rows.append(rec)
    rows = [r for r in rows if any(r[f"{k}_n"] > 0 for k, _ in _SPI37_SOURCES)]
    for k in range(len(rows)):
        rows[k]["hi"] = rows[k + 1]["lo"] if k + 1 < len(rows) else float("inf")
    return rows


def load_spi_investment(spi_dir: Path) -> dict[int, list[dict]]:
    """Load all SPI Table 3.7 files keyed by fiscal-year start (2016 = 2016-17)."""
    out: dict[int, list[dict]] = {}
    for path in sorted(spi_dir.glob("Table_3_7_*")):
        m = re.search(r"_(\d{4})\.", path.name)
        if m:
            out[int(m.group(1))] = load_spi_table_3_7(path)
    return out


def build_spi_investment_targets(
    spi37: dict[int, list[dict]], earnings_index: dict[int, float], years: list[int]
) -> list[dict]:
    """Banded SPI investment-income targets (Table 3.7): per band per source, a
    count (count_nonzero) and an amount (sum), filtered on baseline_total_income.

    Mirrors build_spi_targets: years beyond the latest vintage reuse its bands
    with counts held and amounts grown by the EFO average-earnings ratio.
    """
    label = "HMRC SPI Table 3.7 (investment income distribution)"
    avail = sorted(spi37)
    out: list[dict] = []
    for yr in years:
        if yr in spi37:
            rows, amt_factor = spi37[yr], 1.0
        elif avail and yr > avail[-1]:
            base = avail[-1]
            rows = spi37[base]
            amt_factor = earnings_index[yr] / earnings_index[base]
        else:
            continue
        for r in rows:
            # Beyond the latest vintage the band edges uprate with earnings too,
            # not just the amounts (see build_spi_targets).
            lo = r["lo"] * amt_factor
            hi = r["hi"] if r["hi"] == float("inf") else r["hi"] * amt_factor
            band = {
                "variable": "total_income",
                "min": round(lo, 0),
                "max": None if hi == float("inf") else round(hi, 0),
            }
            tag = int(round(lo))
            for key, variable in _SPI37_SOURCES:
                count, amount = r[f"{key}_n"], r[f"{key}_a"]
                if count > 0:
                    out.append({
                        "name": f"spi37_{key}_count_{tag}_{yr}",
                        "variable": variable, "entity": "person",
                        "aggregation": "count_nonzero", "filter": band,
                        "benunit_filter": None, "value": round(count * 1000.0, 0),
                        "source": label, "year": yr, "holdout": False,
                    })
                if amount > 0:
                    out.append({
                        "name": f"spi37_{key}_amount_{tag}_{yr}",
                        "variable": variable, "entity": "person",
                        "aggregation": "sum", "filter": band,
                        "benunit_filter": None,
                        "value": round(amount * 1e6 * amt_factor, 0),
                        "source": label, "year": yr, "holdout": False,
                    })
    return out


def load_efo_earnings_index(path: Path) -> dict[int, float]:
    """Average-earnings index (2008Q1=100) by fiscal year from EFO table 1.6 col 16.

    Used to project SPI income amounts into years the SPI does not yet cover
    (the latest SPI is 2023-24); 2024 amounts grow by index[2024]/index[2023].
    """
    wb = openpyxl.load_workbook(str(path), data_only=True)
    ws = wb["1.6"]
    quarters: dict[tuple[int, int], float] = {}
    for row in ws.iter_rows(values_only=True):
        m = re.match(r"(\d{4})Q([1-4])", str(row[1])) if row[1] else None
        if m and len(row) > 16 and isinstance(row[16], (int, float)):
            quarters[(int(m.group(1)), int(m.group(2)))] = float(row[16])
    out: dict[int, float] = {}
    for fy in TARGET_YEARS:
        vals = [quarters[q] for q in [(fy, 2), (fy, 3), (fy, 4), (fy + 1, 1)] if q in quarters]
        if len(vals) == 4:
            out[fy] = sum(vals) / 4.0
    return out


def build_spi_targets(
    spi: dict[int, list[dict]], earnings_index: dict[int, float], years: list[int]
) -> list[dict]:
    """Banded SPI income-distribution targets: per band per source, a count
    (count_nonzero of the source variable) and an amount (sum). Each target
    carries a person-level band filter on baseline_total_income [lo, hi).

    Years beyond the latest SPI vintage reuse that year's bands with counts held
    and amounts grown by the EFO average-earnings ratio.
    """
    label = "HMRC SPI Table 3.6 (income distribution)"
    avail = sorted(spi)
    out: list[dict] = []
    for yr in years:
        if yr in spi:
            rows, amt_factor = spi[yr], 1.0
        elif avail and yr > avail[-1]:
            base = avail[-1]
            rows = spi[base]
            amt_factor = earnings_index[yr] / earnings_index[base]
        else:
            continue
        for r in rows:
            # Beyond the latest vintage the band edges uprate with earnings too,
            # not just the amounts — otherwise the £-thresholds stay frozen while
            # the amounts grow, mis-aligning the band a record's income falls in.
            lo = r["lo"] * amt_factor
            hi = r["hi"] if r["hi"] == float("inf") else r["hi"] * amt_factor
            band = {
                "variable": "total_income",
                "min": round(lo, 0),
                "max": None if hi == float("inf") else round(hi, 0),
            }
            tag = int(round(lo))
            for key, variable in _SPI_SOURCES:
                count, amount = r[f"{key}_n"], r[f"{key}_a"]
                # The public SPI tape is disclosure-controlled in the top bands:
                # FACT jumps ~10→~32 per record above £500k, so its grossed
                # pension headcount (~20k in the £1m+ band) is ~10× the published
                # count (~2k) while the amount agrees. Injected records carry the
                # tape's many-small-pensions shape, so a count target there is
                # unreachable — keep only the amount for pension ≥£500k bands.
                if key in ("state_pension", "private_pension") and r["lo"] >= 500_000:
                    count = 0
                if count > 0:
                    out.append({
                        "name": f"spi_{key}_count_{tag}_{yr}",
                        "variable": variable, "entity": "person",
                        "aggregation": "count_nonzero", "filter": band,
                        "benunit_filter": None, "value": round(count * 1000.0, 0),
                        "source": label, "year": yr, "holdout": False,
                    })
                if amount > 0:
                    out.append({
                        "name": f"spi_{key}_amount_{tag}_{yr}",
                        "variable": variable, "entity": "person",
                        "aggregation": "sum", "filter": band,
                        "benunit_filter": None,
                        "value": round(amount * 1e6 * amt_factor, 0),
                        "source": label, "year": yr, "holdout": False,
                    })
    return out


# ── DWP benefits ─────────────────────────────────────────────────────────────

def load_dwp_benefits(path: Path) -> dict[int, dict[str, float]]:
    """Return {fiscal_year_start: {field: £bn}} from Table 1a (£m nominal)."""
    wb = openpyxl.load_workbook(str(path), data_only=True)

    # Table 1a = nominal £m
    ws = wb["Table 1a"]
    row2 = [ws.cell(row=2, column=c).value for c in range(1, ws.max_column + 1)]
    yr_cols: dict[int, int] = {}
    for c, v in enumerate(row2, 1):
        if isinstance(v, str) and re.match(r"\d{4}/\d{2}", v.strip()):
            yr_cols[int(v.strip()[:4])] = c

    prog_rows = {
        "attendance_allowance": 7,
        "carers_allowance": 9,
        "disability_living_allowance": 18,
        "esa_income_related": 24,
        "housing_benefit": 29,
        "income_support": 35,
        "jsa_income_based": 43,
        "personal_independence_payment": 55,
        "pension_credit": 54,
        "state_pension": 63,
        "universal_credit": 72,
    }

    out: dict[int, dict[str, float]] = {}
    for yr in TARGET_YEARS:
        if yr not in yr_cols:
            continue
        row: dict[str, float] = {}
        for prog, row_idx in prog_rows.items():
            v = ws.cell(row=row_idx, column=yr_cols[yr]).value
            if isinstance(v, (int, float)):
                row[prog] = v / 1000.0  # £m -> £bn

        # Child benefit is in "UK welfare " sheet
        ws_uk = wb["UK welfare "]
        row2_uk = [ws_uk.cell(row=2, column=c).value for c in range(1, ws_uk.max_column + 1)]
        yr_cols_uk: dict[int, int] = {}
        for c, v in enumerate(row2_uk, 1):
            if isinstance(v, str) and re.match(r"\d{4}/\d{2}", v.strip()):
                yr_cols_uk[int(v.strip()[:4])] = c

        if yr in yr_cols_uk:
            cb = ws_uk.cell(row=10, column=yr_cols_uk[yr]).value
            tc = ws_uk.cell(row=13, column=yr_cols_uk[yr]).value
            if isinstance(cb, (int, float)):
                row["child_benefit"] = cb
            if isinstance(tc, (int, float)):
                row["tax_credits"] = tc

        out[yr] = row

    return out


# ── COICOP consumption ───────────────────────────────────────────────────────

def load_coicop(ons_qna_path: Path) -> dict[int, dict[str, float]]:
    """Return {calendar_year: {field: £bn}} using Eurostat + ONS QNA."""

    # --- ONS QNA: total HHFCE at current prices (£m) and CVM breakdown ---
    wb = openpyxl.load_workbook(str(ons_qna_path), data_only=True)

    ws_c1 = wb["C1 EXPENDITURE"]
    c1: dict[int, float] = {}
    in_t1 = False
    for row in range(1, ws_c1.max_row + 1):
        v = ws_c1.cell(row=row, column=1).value
        if v == "Table 1: Annual":
            in_t1 = True
            continue
        if in_t1 and isinstance(v, int) and v > 1900:
            val = ws_c1.cell(row=row, column=2).value
            if isinstance(val, (int, float)):
                c1[v] = float(val)
        if in_t1 and v is None and len(c1) > 3:
            break

    ws_e3 = wb["E3 EXPENDITURE"]
    e3: dict[int, list] = {}
    in_t1 = False
    for row in range(1, ws_e3.max_row + 1):
        v = ws_e3.cell(row=row, column=1).value
        if v == "Table 1: Annual":
            in_t1 = True
            continue
        if in_t1 and isinstance(v, int) and v > 1900:
            vals = [ws_e3.cell(row=row, column=c).value for c in range(1, 17)]
            e3[v] = vals
        if in_t1 and v is None and len(e3) > 3:
            break

    # --- Eurostat: UK nominal COICOP £m 2010-2019 ---
    r = requests.get(
        "https://ec.europa.eu/eurostat/api/dissemination/statistics/1.0/data/nama_10_co3_p3",
        params={"geo": "UK", "unit": "CP_MNAC", "freq": "A", "format": "JSON"},
        timeout=30,
    )
    estat = r.json()
    coicop_idx = estat["dimension"]["coicop"]["category"]["index"]
    time_idx = estat["dimension"]["time"]["category"]["index"]
    n_time = estat["size"][4]
    estat_vals = estat["value"]

    def estat_get(code: str, year: int) -> float | None:
        cp = coicop_idx.get(code)
        tp = time_idx.get(str(year))
        if cp is None or tp is None:
            return None
        return estat_vals.get(str(cp * n_time + tp))

    # col indices in E3 row (0-based from row list): 0=yr,1=national,2=net_tourism,3=domestic,
    # 4=food,5=alc+tob,6=clothing,7=housing,8=furnishings,9=health,10=transport,
    # 11=comms,12=recreation,13=education,14=restaurants,15=misc
    e3_mapping = [
        ("food_consumption",                       "CP01",  4),
        ("clothing_consumption",                   "CP03",  6),
        ("furnishings_consumption",                "CP05",  8),
        ("health_consumption",                     "CP06",  9),
        ("transport_consumption",                  "CP07",  10),
        ("communication_consumption",              "CP08",  11),
        ("recreation_consumption",                 "CP09",  12),
        ("education_consumption",                  "CP10",  13),
        ("restaurants_consumption",                "CP11",  14),
        ("miscellaneous_consumption",              "CP12",  15),
    ]

    out: dict[int, dict[str, float]] = {}
    for yr in TARGET_YEARS:
        row: dict[str, float] = {}
        total_cvm = e3[yr][1] if yr in e3 and isinstance(e3[yr][1], (int, float)) else None
        total_cp = c1.get(yr)

        # The CVM-share fallback only holds near the chained-volume reference year,
        # so it is used solely for 2020+ (post-Eurostat). For 1995-2019 we use the
        # direct Eurostat nominal reading; years Eurostat lacks (pre-1995) get no
        # consumption target rather than a bad CVM-derived one.
        for field, euro_code, e3_col in e3_mapping:
            if yr <= 2019:
                v = estat_get(euro_code, yr)
                if v is not None:
                    row[field] = v / 1000.0  # £m -> £bn
            elif total_cvm and total_cp and yr in e3:
                cvm_val = e3[yr][e3_col]
                if isinstance(cvm_val, (int, float)):
                    row[field] = (float(cvm_val) / total_cvm) * total_cp / 1000.0  # £m -> £bn

        # Alcohol and tobacco are one COICOP level-1 division (CP02); target the
        # combined line, not the sub-split. Eurostat nominal for 1995-2019,
        # CVM-share of CP02 × current-prices total for 2020+.
        if yr <= 2019:
            cp02 = estat_get("CP02", yr)
            if cp02:
                row["alcohol_and_tobacco_consumption"] = cp02 / 1000.0
        elif total_cvm and total_cp and yr in e3:
            cp02_cvm = e3[yr][5]
            if isinstance(cp02_cvm, (int, float)):
                row["alcohol_and_tobacco_consumption"] = (
                    float(cp02_cvm) / total_cvm
                ) * total_cp / 1000.0  # £bn

        # Housing (CP04) excludes owner-occupier imputed rentals (CP042), which are
        # a national-accounts construct, not spending households actually report.
        # Target CP04 - CP042. For 2020+ the E3 CVM sheet has no imputed-rentals
        # sub-line, so scale the CVM housing total by the latest Eurostat actual
        # (non-imputed) share of CP04.
        if yr <= 2019:
            cp04 = estat_get("CP04", yr)
            cp042 = estat_get("CP042", yr)
            if cp04 is not None and cp042 is not None:
                row["housing_water_electricity_consumption"] = (cp04 - cp042) / 1000.0
        elif total_cvm and total_cp and yr in e3:
            cp04_cvm = e3[yr][7]
            cp04_19 = estat_get("CP04", 2019)
            cp042_19 = estat_get("CP042", 2019)
            if isinstance(cp04_cvm, (int, float)) and cp04_19 and cp042_19 is not None:
                actual_share = (cp04_19 - cp042_19) / cp04_19
                row["housing_water_electricity_consumption"] = (
                    float(cp04_cvm) / total_cvm
                ) * total_cp * actual_share / 1000.0  # £bn

        out[yr] = row

    return out


# ── DWP caseloads ──────────────────────────────────────────────────────────

# Each entry: (variable, sheet, header_row, data_row).
# header_row carries the fiscal-year column labels ("YYYY/YY Outturn");
# data_row holds the caseload total (in thousands) for that benefit.
_CASELOAD_SPECS = [
    ("attendance_allowance",          "Disability benefits",              50, 74),
    ("carers_allowance",              "Carers Allowance",                 14, 15),
    ("disability_living_allowance",   "Disability benefits",              50, 55),
    ("esa_income_related",            "Incapacity benefits",             119, 129),
    ("housing_benefit",               "Housing benefits",                122, 123),
    ("income_support",                "Income Support",                   90, 91),
    ("jsa_income_based",              "Unemployment benefits",            24, 33),
    ("personal_independence_payment", "Disability benefits",              50, 65),
    ("pension_credit",                "Pension Credit",                   18, 19),
    ("state_pension",                 "State Pension",                    28, 29),
    ("universal_credit",              "Universal Credit and equivalent",  46, 53),
]


def _caseload_year_cols(ws, header_row: int) -> dict[int, int]:
    """Map fiscal-year start -> column index from a caseload header row."""
    out: dict[int, int] = {}
    for c in range(3, ws.max_column + 1):
        v = ws.cell(row=header_row, column=c).value
        m = re.search(r"(\d{4})/\d{2}", str(v)) if v else None
        if m:
            out[int(m.group(1))] = c
    return out


def load_dwp_caseloads(path: Path) -> dict[int, dict[str, float]]:
    """Return {fiscal_year_start: {variable: claimant_count}} from the DWP xlsx.

    Caseload totals are stored in thousands; multiply by 1000 for raw counts.
    """
    wb = openpyxl.load_workbook(str(path), data_only=True)

    out: dict[int, dict[str, float]] = {}
    for variable, sheet, header_row, data_row in _CASELOAD_SPECS:
        ws = wb[sheet]
        yr_cols = _caseload_year_cols(ws, header_row)
        for yr, col in yr_cols.items():
            v = ws.cell(row=data_row, column=col).value
            if isinstance(v, (int, float)) and v != 0:
                out.setdefault(yr, {})[variable] = float(v) * 1000.0

    return out


# ── FRS-grossed population targets ────────────────────────────────────────────

_AGE_BANDS = [(0, 16), (16, 25), (25, 35), (35, 45), (45, 55), (55, 65), (65, 75), (75, None)]


def build_population_targets(years: list[int]) -> list[dict]:
    """Population control totals grossed up from the FRS by its own household
    weights: total counts of households/people/benunits, plus household count by
    region, person count by sex, and person count by age band. These pin the
    survey universe so reweighting can't drift the totals away from the FRS.
    """
    frs_base = REPO_ROOT / "data" / "clean" / "frs"
    label = "FRS grossed population"
    out: list[dict] = []
    for yr in years:
        ydir = frs_base / str(yr)
        if not (ydir / "households.csv").exists():
            continue
        hh = pd.read_csv(ydir / "households.csv")
        persons = pd.read_csv(ydir / "persons.csv")
        benunits = pd.read_csv(ydir / "benunits.csv")
        w = hh.set_index("household_id")["weight"].astype(float)

        person_w = persons["household_id"].map(w).to_numpy()
        benunit_w = benunits["household_id"].map(w).to_numpy()

        def add(name, entity, value, flt=None):
            out.append({
                "name": f"{name}_{yr}", "variable": "household_id",
                "entity": entity, "aggregation": "count", "filter": flt,
                "benunit_filter": None, "value": round(float(value), 0),
                "source": label, "year": yr, "holdout": False,
            })

        add("population_households", "household", w.sum())
        add("population_people", "person", person_w.sum())
        add("population_benunits", "benunit", benunit_w.sum())

        for region in sorted(hh["region"].dropna().unique()):
            val = w[hh.set_index("household_id")["region"] == region].sum()
            slug = re.sub(r"[^a-z0-9]+", "_", str(region).lower()).strip("_")
            add(f"population_households_region_{slug}", "household", val,
                {"variable": "region", "eq": region})

        for sex in sorted(persons["gender"].dropna().unique()):
            val = person_w[(persons["gender"] == sex).to_numpy()].sum()
            add(f"population_people_sex_{sex}", "person", val,
                {"variable": "gender", "eq": sex})

        age = persons["age"].to_numpy()
        for lo, hi in _AGE_BANDS:
            mask = age >= lo
            if hi is not None:
                mask &= age < hi
            val = person_w[mask].sum()
            tag = f"{lo}_{hi}" if hi is not None else f"{lo}_plus"
            add(f"population_people_age_{tag}", "person", val,
                {"variable": "age", "min": lo, "max": hi})
    return out


# ── UC award-amount distribution targets ──────────────────────────────────────

# Households on UC by Monthly Award Amount band (Stat-Xplore UC_Households,
# V_F_UC_HOUSEHOLDS count), November snapshot each year. Index 0 = no payment;
# 1..15 = £0.01-100 ... £1400.01-1500 per month; 16 = £1500.01+ catch-all (used
# to Nov 2021); 17..27 = £1500.01-1600 ... £2500.01+ fine split (from Nov 2022,
# with index 16 then zero). We collapse everything ≥£1500/mo into one band, so
# the top band = index 16 + sum(17..27), robust to that mid-series split.
_UC_AWARD_BAND_COUNTS = {
    2016: [129503, 17858, 18321, 73888, 37739, 15760, 25830, 22105, 12086, 8390, 7527, 6396, 5015, 4111, 3371, 2629, 8108, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    2017: [158166, 25659, 29870, 89860, 58388, 31274, 45619, 42391, 26930, 22242, 20567, 16863, 13474, 11351, 8581, 6105, 16538, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    2018: [182935, 40293, 60361, 164478, 101211, 82030, 115629, 103657, 69159, 68587, 64331, 51867, 41439, 32368, 23491, 15958, 47424, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    2019: [169100, 66150, 99431, 287662, 166426, 146491, 225351, 201465, 136582, 146177, 149247, 121624, 101502, 82896, 68476, 44291, 130682, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    2020: [802377, 124189, 136197, 246176, 600631, 362331, 234686, 366594, 336284, 237386, 238638, 234041, 203015, 182719, 153674, 105579, 321623, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    2021: [761122, 149309, 189910, 423090, 345652, 208515, 297043, 440411, 242251, 240214, 274975, 243595, 200684, 179938, 145898, 107103, 346169, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    2022: [614296, 155013, 184636, 379951, 341840, 204799, 295290, 514875, 238277, 252998, 286350, 293905, 230732, 205770, 168462, 124252, 0, 95899, 74971, 61011, 47863, 37281, 26497, 20811, 16219, 12020, 9589, 28987],
    2023: [575921, 142285, 182890, 383919, 383034, 190141, 261780, 415570, 451402, 240548, 260864, 314996, 302320, 239787, 203790, 168543, 0, 138022, 105428, 84402, 69173, 56655, 46039, 34263, 25894, 20748, 15902, 56892],
}


def build_uc_award_band_targets(years: list[int]) -> list[dict]:
    """Count of UC benefit units by monthly award-amount band, calibrated to the
    DWP Households-on-UC distribution. The simulation's annual UC entitlement is
    binned into monthly £100 bands (annual edge = monthly × 12), pinning the
    *shape* of the award distribution rather than just the total caseload. The
    nil-award band is dropped (a £0 filter on UC alone can't distinguish non-
    recipients) and everything ≥£1500/mo is collapsed into one top band.
    """
    label = "DWP Stat-Xplore UC_Households award bands (Nov snapshot)"
    out: list[dict] = []
    for yr in years:
        counts = _UC_AWARD_BAND_COUNTS.get(yr)
        if counts is None:
            continue

        def add(name, value, lo_monthly, hi_monthly):
            flt = {"variable": "universal_credit",
                   "min": max(lo_monthly * 12.0, 0.01),
                   "max": None if hi_monthly is None else hi_monthly * 12.0}
            out.append({
                "name": f"{name}_{yr}", "variable": "universal_credit",
                "entity": "benunit", "aggregation": "count", "filter": flt,
                "benunit_filter": None, "value": round(float(value), 0),
                "source": label, "year": yr, "holdout": False,
            })

        for b in range(1, 16):
            lo, hi = (b - 1) * 100, b * 100
            add(f"uc_award_band_{lo}_{hi}", counts[b], lo, hi)
        add("uc_award_band_1500_plus", counts[16] + sum(counts[17:28]), 1500, None)
    return out


# ── Forecast-year targets (uprated from the latest real year) ─────────────────

# Per-source uprating index for forecast years. Each target grows at the rate of
# the microdata variable that generates it, so forecast-year calibration stays a
# nudge rather than fighting the uprating. Counts (population/caseloads/labour)
# track the population index; expenditure and consumption track CPI; tax receipts
# track the base of the tax (earnings for income tax/NI/SDLT, CPI for VAT, GDP per
# capita for CGT). SPI distribution targets are handled separately.
_FORECAST_INDEX = {
    "FRS grossed population": "population",
    "OBR EFO economy table 1.6 (labour market)": "population",
    "DWP Stat-Xplore UC_Households award bands (Nov snapshot)": "population",
    "Eurostat/ONS HHFCE COICOP": "cpi",
}
# DWP benefit expenditure carries both £ sums (CPI) and claimant counts
# (population), distinguished by aggregation. HMRC receipts vary by tax.
_HMRC_RECEIPT_INDEX = {
    "income_tax": "earnings", "employee_ni": "earnings", "employer_ni": "earnings",
    "stamp_duty": "earnings", "vat": "cpi", "capital_gains_tax": "gdp_pc",
}


def _forecast_index_for(t: dict) -> str:
    src = t["source"]
    if src == "HMRC receipts":
        return _HMRC_RECEIPT_INDEX[t["variable"]]
    if src == "DWP benefit expenditure":
        return "population" if t["aggregation"] == "count_nonzero" else "cpi"
    return _FORECAST_INDEX[src]


def _retag_name(name: str, base_year: int, forecast_year: int) -> str:
    suffix = f"_{base_year}"
    return name[: -len(suffix)] + f"_{forecast_year}" if name.endswith(suffix) else name


def build_forecast_targets(real_targets: list[dict], forecast_years: list[int]) -> list[dict]:
    """Project the latest real year's targets onto OBR forecast years.

    Non-SPI targets are scaled by the cumulative growth of their source index
    (counts by population, expenditure/consumption by CPI, receipts by their tax
    base). SPI distribution targets uprate their income thresholds and amounts by
    earnings growth while holding counts fixed (the band shape is assumed stable).
    """
    base = LATEST_REAL_YEAR
    spi_labels = (
        "HMRC SPI Table 3.6 (income distribution)",
        "HMRC SPI Table 3.7 (investment income distribution)",
    )
    base_targets = [t for t in real_targets if t["year"] == base]
    out: list[dict] = []

    for yr in forecast_years:
        for t in base_targets:
            if t["source"] in spi_labels:
                ef = cumulative_factor(base, yr, "earnings")
                flt = t["filter"]
                new_flt = {
                    "variable": flt["variable"],
                    "min": None if flt["min"] is None else round(flt["min"] * ef, 0),
                    "max": None if flt["max"] is None else round(flt["max"] * ef, 0),
                }
                # Counts held fixed; amounts grow with earnings. Re-tag the band
                # in the name to the uprated lower threshold.
                tag = 0 if new_flt["min"] is None else int(new_flt["min"])
                stem = t["name"].rsplit("_", 2)[0]  # spi_<key>_<count|amount>
                value = t["value"] if t["aggregation"] == "count_nonzero" else round(t["value"] * ef, 0)
                nt = dict(t)
                nt.update(name=f"{stem}_{tag}_{yr}", filter=new_flt, value=value, year=yr)
                out.append(nt)
            else:
                f = cumulative_factor(base, yr, _forecast_index_for(t))
                nt = dict(t)
                nt.update(name=_retag_name(t["name"], base, yr),
                          value=round(t["value"] * f, 0), year=yr)
                out.append(nt)
    return out


# ── Assemble targets ─────────────────────────────────────────────────────────

def build_targets(years: list[int]) -> list[dict]:
    hmrc_path = RAW_DIR / "hmrc" / "NS_Table.ods"
    dwp_path = RAW_DIR / "obr" / "dwp.xlsx"
    qna_path = RAW_DIR / "ons" / "qna_hhfce.xlsx"

    _download(HMRC_ODS_URL, hmrc_path)
    _download(ONS_QNA_URL, qna_path)

    hmrc = load_hmrc_receipts(hmrc_path)
    # Backfill pre-2006 tax (HMRC bulletin starts 2005-06) from the OBR databank,
    # which covers 1999-00 onwards. HMRC takes priority where both exist.
    databank = load_obr_databank_receipts(RAW_DIR / "obr" / "pf_databank.xlsx")
    for fy, vals in databank.items():
        hmrc.setdefault(fy, vals)
    dwp = load_dwp_benefits(dwp_path)
    coicop = load_coicop(qna_path)
    caseloads = load_dwp_caseloads(dwp_path)
    labour = load_efo_labour_market(RAW_DIR / "obr" / "efo_economy.xlsx")
    spi = load_spi_distributions(RAW_DIR / "hmrc" / "spi")
    spi37 = load_spi_investment(RAW_DIR / "hmrc" / "spi")
    earnings_index = load_efo_earnings_index(RAW_DIR / "obr" / "efo_economy.xlsx")

    # Variable specs: (name_prefix, entity, variable, aggregation, source_key_in_dict, scale)
    # Scale: all final values should be in £ (the raw survey unit is weekly £ per hh,
    # so calibrate target = aggregate_£_per_year × 52  — handled by the matrix builder
    # summing over households. But since benefit/tax values on Person/BenUnit are annual £,
    # the target should be the national annual total in £.)
    INCOME_TAX_SPECS = [
        ("income_tax_total",          "person", "income_tax",         "sum", "hmrc", "income_tax"),
        ("employee_ni_total",         "person", "employee_ni",        "sum", "hmrc", "employee_ni"),
        # Employer NI is intentionally not a calibration target: the engine
        # computes gross statutory liability (15% above the secondary threshold)
        # with no reliefs, while the HMRC receipts figure is net of the
        # Employment Allowance, under-21/apprentice exemptions and the per-job
        # (not per-person) threshold. The ~28% gap is structural, so reweighting
        # cannot close it and only distorts the weight vector chasing it.
        ("capital_gains_tax_total",   "person", "capital_gains_tax",  "sum", "hmrc", "capital_gains_tax"),
        ("vat_total",                 "household", "vat",             "sum", "hmrc", "vat"),
        # Stamp duty is intentionally not a calibration target: the engine
        # under-predicts receipts by ~15-19% every year (missing higher-rate
        # surcharges on additional/non-resident dwellings, plus survey under-
        # capture of high-value transactions). The gap is structural and stable,
        # so reweighting only distorts the weight vector chasing it.
    ]
    BENEFIT_SPECS = [
        ("child_benefit_total",         "benunit", "child_benefit",         "sum", "dwp", "child_benefit"),
        ("attendance_allowance_total",  "person",  "attendance_allowance",  "sum", "dwp", "attendance_allowance"),
        ("carers_allowance_total",      "person",  "carers_allowance",      "sum", "dwp", "carers_allowance"),
        ("disability_living_allowance_total",       "person", "disability_living_allowance",   "sum", "dwp", "disability_living_allowance"),
        ("personal_independence_payment_total",     "person", "personal_independence_payment", "sum", "dwp", "personal_independence_payment"),
        ("esa_income_related_total",    "benunit", "esa_income_related",    "sum", "dwp", "esa_income_related"),
        ("housing_benefit_total",       "benunit", "housing_benefit",       "sum", "dwp", "housing_benefit"),
        ("income_support_total",        "benunit", "income_support",        "sum", "dwp", "income_support"),
        ("jsa_income_based_total",      "benunit", "jsa_income_based",      "sum", "dwp", "jsa_income_based"),
        ("pension_credit_total",        "benunit", "pension_credit",        "sum", "dwp", "pension_credit"),
        ("state_pension_total",         "person",  "state_pension",         "sum", "dwp", "state_pension"),
        ("universal_credit_total",      "benunit", "universal_credit",      "sum", "dwp", "universal_credit"),
        ("tax_credits_total",           "benunit", "tax_credits",           "sum", "dwp", "tax_credits"),
    ]
    COICOP_SPECS = [
        ("food_consumption_total",                        "household", "food_consumption",                        "sum", "coicop", "food_consumption"),
        ("alcohol_and_tobacco_consumption_total",         "household", "alcohol_and_tobacco_consumption",         "sum", "coicop", "alcohol_and_tobacco_consumption"),
        ("clothing_consumption_total",                    "household", "clothing_consumption",                    "sum", "coicop", "clothing_consumption"),
        ("housing_water_electricity_consumption_total",   "household", "housing_water_electricity_consumption",   "sum", "coicop", "housing_water_electricity_consumption"),
        ("furnishings_consumption_total",                 "household", "furnishings_consumption",                 "sum", "coicop", "furnishings_consumption"),
        ("health_consumption_total",                      "household", "health_consumption",                      "sum", "coicop", "health_consumption"),
        ("transport_consumption_total",                   "household", "transport_consumption",                   "sum", "coicop", "transport_consumption"),
        ("communication_consumption_total",               "household", "communication_consumption",               "sum", "coicop", "communication_consumption"),
        ("recreation_consumption_total",                  "household", "recreation_consumption",                  "sum", "coicop", "recreation_consumption"),
        ("education_consumption_total",                   "household", "education_consumption",                   "sum", "coicop", "education_consumption"),
        ("restaurants_consumption_total",                 "household", "restaurants_consumption",                 "sum", "coicop", "restaurants_consumption"),
        ("miscellaneous_consumption_total",               "household", "miscellaneous_consumption",               "sum", "coicop", "miscellaneous_consumption"),
    ]

    CASELOAD_SPECS = [
        ("attendance_allowance_claimants", "person", "attendance_allowance", "count_nonzero", "caseloads", "attendance_allowance"),
        ("carers_allowance_claimants",   "person",  "carers_allowance",   "count_nonzero", "caseloads", "carers_allowance"),
        ("disability_living_allowance_claimants",   "person", "disability_living_allowance",   "count_nonzero", "caseloads", "disability_living_allowance"),
        ("personal_independence_payment_claimants", "person", "personal_independence_payment", "count_nonzero", "caseloads", "personal_independence_payment"),
        ("esa_income_related_claimants", "benunit", "esa_income_related", "count_nonzero", "caseloads", "esa_income_related"),
        ("housing_benefit_claimants",    "benunit", "housing_benefit",    "count_nonzero", "caseloads", "housing_benefit"),
        ("income_support_claimants",     "benunit", "income_support",     "count_nonzero", "caseloads", "income_support"),
        ("jsa_income_based_claimants",   "benunit", "jsa_income_based",   "count_nonzero", "caseloads", "jsa_income_based"),
        ("pension_credit_claimants",     "benunit", "pension_credit",     "count_nonzero", "caseloads", "pension_credit"),
        ("state_pension_claimants",      "person",  "state_pension",      "count_nonzero", "caseloads", "state_pension"),
        ("universal_credit_claimants",   "benunit", "universal_credit",   "count_nonzero", "caseloads", "universal_credit"),
    ]

    LABOUR_SPECS = [
        ("employed_count",   "person", "is_employed",   "sum", "labour", "employed"),
        ("unemployed_count", "person", "is_unemployed", "sum", "labour", "unemployed"),
    ]

    all_specs = INCOME_TAX_SPECS + BENEFIT_SPECS + COICOP_SPECS + CASELOAD_SPECS + LABOUR_SPECS
    source_map = {"hmrc": hmrc, "dwp": dwp, "coicop": coicop, "caseloads": caseloads, "labour": labour}
    source_label = {"hmrc": "HMRC receipts", "dwp": "DWP benefit expenditure",
                    "coicop": "Eurostat/ONS HHFCE COICOP",
                    "caseloads": "DWP benefit expenditure",
                    "labour": "OBR EFO economy table 1.6 (labour market)"}

    real_years = [y for y in years if y <= LATEST_REAL_YEAR]
    targets = []
    for yr in real_years:
        for name, entity, variable, aggregation, source_key, data_key in all_specs:
            data = source_map[source_key].get(yr, {})
            raw = data.get(data_key)
            if raw is None:
                continue
            # caseloads and labour levels are raw counts; everything else is £bn -> £
            val = raw if source_key in ("caseloads", "labour") else raw * 1e9
            targets.append({
                "name": f"{name}_{yr}",
                "variable": variable,
                "entity": entity,
                "aggregation": aggregation,
                "filter": None,
                "benunit_filter": None,
                "value": round(val, 0),
                "source": source_label[source_key],
                "year": yr,
                "holdout": False,
            })

    targets += build_spi_targets(spi, earnings_index, real_years)
    targets += build_spi_investment_targets(spi37, earnings_index, real_years)
    targets += build_population_targets(real_years)
    targets += build_uc_award_band_targets(real_years)

    forecast_years = [y for y in years if y > LATEST_REAL_YEAR]
    if forecast_years:
        # Forecast targets project the latest real year forward; build that base
        # even if the caller didn't request it.
        base = targets if LATEST_REAL_YEAR in real_years else build_targets([LATEST_REAL_YEAR])
        targets += build_forecast_targets(base, forecast_years)
    return targets


# ── CLI ──────────────────────────────────────────────────────────────────────

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--years", nargs="+", type=int, default=TARGET_YEARS + FORECAST_YEARS)
    args = parser.parse_args()

    print(f"Building calibration targets for {len(args.years)} years...")
    targets = build_targets(args.years)

    from rich.console import Console
    from rich.table import Table as RichTable
    console = Console()

    by_year: dict[int, int] = {}
    for t in targets:
        by_year[t["year"]] = by_year.get(t["year"], 0) + 1

    rt = RichTable(title="Calibration targets", show_header=True)
    rt.add_column("Year", style="bold")
    rt.add_column("Targets", justify="right")
    for yr in sorted(by_year):
        rt.add_row(str(yr), str(by_year[yr]))
    rt.add_row("Total", str(len(targets)), style="bold")
    console.print(rt)

    out = {"targets": targets}
    OUT_PATH.write_text(json.dumps(out, indent=2))
    console.print(f"[green]Wrote {len(targets)} targets to {OUT_PATH.relative_to(REPO_ROOT)}[/green]")


if __name__ == "__main__":
    main()
