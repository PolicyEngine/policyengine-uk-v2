"""Build region-level LHA rates from DWP's published UC LHA rates by BRMA.

Sources (data/raw/dwp/lha/):
- <nation>_2014_2019.ods — UC LHA calendar-monthly rates by BRMA, sheets
  April_2014..April_2019 (England/Scotland/Wales).
- <nation>_<year>.{ods,csv} — annual UC LHA rates from April 2020 to April
  2026. April 2025 and April 2026 rates are frozen at April 2024 levels
  (SI 2026/5).
- lha_list_of_rents.csv.gz — VOA lists of rents 2019 & 2020 (weekly rents by
  BRMA and category), used for (a) the BRMA-to-region lookup, (b) rent
  observation counts as aggregation weights (a proxy for the size of the
  private rented sector in each BRMA), and (c) Northern Ireland rates, which
  DWP does not publish (NIHE administers LHA separately).

Output: an `lha:` block written into parameters/<fy>.yaml for 2016/17 to
2030/31. Rates are calendar-monthly, aggregated to the 12 regions in the
engine's Region order. Forecast years (2027/28 onwards) carry the April 2024
cash rates forward, matching the government's nominal freeze (and the RF LSO
2026 baseline).

Northern Ireland assumption: rates are the observation-weighted 30th
percentile of the 2019+2020 NI lists of rents (weekly x 52/12), scaled across
years by the GB-average cash path per category. NI is ~3% of UK households.

Usage: uv run python data/lha.py
"""

from __future__ import annotations

import re
from pathlib import Path

import numpy as np
import pandas as pd

REPO_ROOT = Path(__file__).resolve().parent.parent
RAW = REPO_ROOT / "data" / "raw" / "dwp" / "lha"
PARAMS_DIR = REPO_ROOT / "parameters"

REGION_ORDER = [
    "NORTH_EAST",
    "NORTH_WEST",
    "YORKSHIRE",
    "EAST_MIDLANDS",
    "WEST_MIDLANDS",
    "EAST_OF_ENGLAND",
    "LONDON",
    "SOUTH_EAST",
    "SOUTH_WEST",
    "WALES",
    "SCOTLAND",
    "NORTHERN_IRELAND",
]
REGION_LABELS = [
    "North East", "North West", "Yorkshire", "East Midlands",
    "West Midlands", "East of England", "London", "South East",
    "South West", "Wales", "Scotland", "Northern Ireland",
]
CATS = ["A", "B", "C", "D", "E"]
NATIONS = ["england", "scotland", "wales"]
YEARS = list(range(2016, 2027))


def norm_brma(name: str) -> str:
    return re.sub(r"[^A-Z0-9]+", "_", str(name).upper()).strip("_")


def parse_sheet(df: pd.DataFrame) -> pd.DataFrame:
    """Find the BRMA header row and return [brma, A..E] monthly rates."""
    header_idx = None
    header_col = None
    for i, row in df.iterrows():
        for j, v in enumerate(row):
            if isinstance(v, str) and v.strip().upper() == "BRMA":
                header_idx, header_col = i, j
                break
        if header_idx is not None:
            break
    if header_idx is None:
        raise ValueError("no BRMA header row found")
    # Welsh files carry the name in an "Area" column with a numeric BRMA
    # code under the "BRMA" header — take the name from the left if so.
    name_col = header_col
    body = df.iloc[header_idx + 1 :]
    col_vals = body.iloc[:, header_col].dropna().astype(str)
    if header_col > 0 and (col_vals.str.fullmatch(r"\d+").mean() > 0.5):
        name_col = header_col - 1
    out = []
    for _, row in body.iterrows():
        name = row.iloc[name_col]
        if not isinstance(name, str) or not name.strip():
            continue
        vals = []
        for v in row.iloc[header_col + 1 : header_col + 6]:
            if isinstance(v, str):
                v = re.sub(r"[^0-9.]", "", v)
                if not v:
                    break
                v = float(v)
            if v is None or (isinstance(v, float) and np.isnan(v)):
                break
            vals.append(float(v))
        if len(vals) == 5:
            out.append([norm_brma(name)] + vals)
    return pd.DataFrame(out, columns=["brma"] + CATS)


def load_year(year: int) -> pd.DataFrame:
    """UC LHA monthly rates by BRMA for GB, April of `year`."""
    frames = []
    for nation in NATIONS:
        if year <= 2019:
            path = RAW / f"{nation}_2014_2019.ods"
            raw = pd.read_excel(
                path, engine="odf", sheet_name=f"April_{year}", header=None
            )
        elif year <= 2021:
            path = RAW / f"{nation}_{year}.ods"
            sheets = pd.read_excel(path, engine="odf", sheet_name=None, header=None)
            best = pd.DataFrame(columns=["brma"] + CATS)
            for df in sheets.values():
                try:
                    parsed = parse_sheet(df)
                except ValueError:
                    continue
                if len(parsed) > len(best):
                    best = parsed
            frames.append(best)
            continue
        else:
            path = RAW / f"{nation}_{year}.csv"
            raw = pd.read_csv(path, encoding="latin-1", header=None)
        frames.append(parse_sheet(raw))
    return pd.concat(frames, ignore_index=True)


def main() -> None:
    lor = pd.read_csv(RAW / "lha_list_of_rents.csv.gz")
    lookup = lor.groupby("brma").region.first()
    # Aggregation weights: rent observations per BRMA x category.
    weights = (
        lor.groupby(["brma", "lha_category"]).size().rename("w").reset_index()
    )

    # GB rates per region x category per year.
    gb_rows = {}
    unmatched = set()
    import difflib

    alias: dict[str, str] = {"FLINT": "FLINTSHIRE"}

    def resolve(name: str) -> str | None:
        if name in lookup.index:
            return name
        if name not in alias:
            close = difflib.get_close_matches(name, lookup.index, n=1, cutoff=0.75)
            alias[name] = close[0] if close else None
        return alias[name]

    for year in YEARS:
        rates = load_year(year)
        resolved = rates.brma.map(resolve)
        unmatched |= set(rates.brma[resolved.isna()])
        rates["brma"] = resolved
        rates = rates.dropna(subset=["brma"])
        rates["region"] = rates.brma.map(lookup)
        long = rates.melt(
            id_vars=["brma", "region"], value_vars=CATS,
            var_name="lha_category", value_name="rate",
        ).merge(weights, on=["brma", "lha_category"], how="left")
        long["w"] = long.w.fillna(1.0)
        agg = long.groupby(["region", "lha_category"]).apply(
            lambda g: np.average(g.rate, weights=g.w), include_groups=False
        ).unstack()
        gb_rows[year] = agg
    fuzzy = {k: v for k, v in alias.items() if v is not None}
    if fuzzy:
        print("fuzzy BRMA matches:")
        for k, v in sorted(fuzzy.items()):
            print(f"  {k} -> {v}")
    if unmatched:
        print(f"warning: unmatched BRMAs (dropped): {sorted(unmatched)}")

    # Northern Ireland: weighted 30th percentile of the 2019+2020 lists of
    # rents (weekly -> calendar monthly), scaled by the GB-average cash path.
    ni = lor[lor.region == "NORTHERN_IRELAND"]
    ni_base = ni.groupby("lha_category").weekly_rent.quantile(0.3) * 52 / 12
    gb_mean = {
        year: gb_rows[year].mean(axis=0) for year in YEARS
    }  # region-mean per category
    for year in YEARS:
        scale = gb_mean[year] / gb_mean[2020]
        gb_rows[year].loc["NORTHERN_IRELAND"] = ni_base * scale

    # Emit YAML blocks. Forecast years carry April 2026 (= April 2024) cash
    # rates forward — the nominal freeze in current policy.
    for fy_start in range(2016, 2031):
        source_year = min(fy_start, 2026)
        table = gb_rows[source_year].loc[
            [r for r in REGION_ORDER if r in gb_rows[source_year].index]
        ]
        lines = [
            "lha:",
            f"  # Local Housing Allowance rates for {fy_start}/{str(fy_start + 1)[2:]} (calendar monthly, £).",
            f"  # DWP UC LHA rates by BRMA (April {source_year}), aggregated to region",
            "  # weighted by VOA list-of-rents observation counts. NI approximated from",
            "  # the 2019-20 NI list of rents scaled by the GB cash path.",
        ]
        if fy_start > 2026:
            lines.append(
                "  # Frozen in cash terms at April 2024 rates (SI 2026/5; OBR/RF baseline)."
            )
        lines += [
            "  # Categories: [A=shared, B=1-bed, C=2-bed, D=3-bed, E=4+bed].",
            "  enabled: true",
            "  private_rent_index: 1.0",
            "  rates_monthly:",
        ]
        for region, label in zip(REGION_ORDER, REGION_LABELS):
            vals = ", ".join(f"{v:8.2f}" for v in table.loc[region, CATS])
            lines.append(f"    - [{vals}]   # {label}")
        block = "\n".join(lines) + "\n"

        fy = f"{fy_start}_{str(fy_start + 1)[2:]}"
        path = PARAMS_DIR / f"{fy}.yaml"
        text = path.read_text()
        # Replace an existing top-level lha block, else append.
        pattern = re.compile(r"^lha:\n(?:^(?:[ \t].*)?\n)*", re.MULTILINE)
        if pattern.search(text):
            text = pattern.sub(block, text, count=1)
        else:
            if not text.endswith("\n"):
                text += "\n"
            text += "\n" + block
        path.write_text(text)
        print(f"{fy}: wrote LHA block (rates from April {source_year})")


if __name__ == "__main__":
    main()
