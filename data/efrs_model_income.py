"""Extract calibrated EFRS mean equivalised net income BHC (weekly, nominal).

Runs the baseline simulation for each built EFRS clean year and pulls
baseline_hbai_incomes.mean_equiv_bhc — the measure matching DWP HBAI
S_OE_BHC_INYR MEAN.
"""

from __future__ import annotations

import json
from pathlib import Path

from rich.console import Console

from policyengine_uk_compiled import Simulation

REPO_ROOT = Path(__file__).resolve().parent.parent
EFRS_BASE = REPO_ROOT / "data" / "clean" / "efrs"
console = Console()


def main() -> None:
    years = sorted(int(p.name) for p in EFRS_BASE.iterdir() if p.name.isdigit())
    out: dict[int, float] = {}
    for year in years:
        try:
            res = Simulation(year=year, data_dir=str(EFRS_BASE)).run()
            out[year] = res.baseline_hbai_incomes.mean_equiv_bhc
            console.print(f"  {year}: {out[year]:.2f}")
        except Exception as e:  # noqa: BLE001
            console.print(f"  [red]{year}: failed — {e}[/red]")
    dest = Path("/tmp/efrs_model_mean_equiv_bhc.json")
    dest.write_text(json.dumps(out, indent=2))
    console.print(f"[green]Wrote {dest}[/green]")


if __name__ == "__main__":
    main()
