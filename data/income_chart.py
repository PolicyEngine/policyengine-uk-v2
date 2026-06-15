"""Chart mean equivalised net income BHC (weekly, nominal) from three sources.

  - DWP Stat-Xplore: HBAI_SURVEY S_OE_BHC_INYR MEAN (already weekly).
  - FRS (model): PolicyEngine baseline on FRS, mean_equiv_bhc (annual → /52).
  - EFRS (calibrated): PolicyEngine baseline on calibrated EFRS (annual → /52).

Saves /tmp/mean_income_bhc.png and prints the aligned table.
"""

from __future__ import annotations

import json
from pathlib import Path

import matplotlib.pyplot as plt
import pandas as pd
from rich.console import Console
from rich.table import Table

console = Console()

DWP = Path("/tmp/hbai_survey_mean_bhc_nominal.json")
FRS = Path("/tmp/frs_model_mean_equiv_bhc.json")
EFRS = Path("/tmp/efrs_model_mean_equiv_bhc.json")


def _load(path: Path, weekly: bool) -> pd.Series:
    raw = json.loads(path.read_text())
    data = raw["values"] if "values" in raw else raw
    s = pd.Series({int(y): float(v) for y, v in data.items() if float(v) > 0.0})
    return s if weekly else s / 52.0


def main() -> None:
    dwp = _load(DWP, weekly=True)
    frs = _load(FRS, weekly=False)
    efrs = _load(EFRS, weekly=False)

    df = pd.DataFrame({"DWP Stat-Xplore": dwp, "FRS (model)": frs, "EFRS (calibrated)": efrs}).sort_index()
    df = df[df.index >= 2010]

    table = Table(title="Mean equivalised net income BHC (£/week, nominal)")
    table.add_column("Year")
    for c in df.columns:
        table.add_column(c, justify="right")
    for yr, row in df.iterrows():
        table.add_row(str(yr), *[f"{v:.0f}" if pd.notna(v) else "—" for v in row])
    console.print(table)

    fig, ax = plt.subplots(figsize=(11, 6))
    for col, colour in zip(df.columns, ["#1f77b4", "#ff7f0e", "#2ca02c"]):
        ax.plot(df.index, df[col], marker="o", ms=3, label=col, color=colour)
    ax.set_xlabel("FRS start year")
    ax.set_ylabel("Mean equivalised net income BHC (£/week, nominal)")
    ax.set_title("Mean household disposable income: DWP vs PolicyEngine FRS vs EFRS")
    ax.legend()
    ax.grid(True, alpha=0.3)
    fig.tight_layout()
    dest = Path("/tmp/mean_income_bhc.png")
    fig.savefig(dest, dpi=130)
    console.print(f"[green]Saved {dest}[/green]")


if __name__ == "__main__":
    main()
