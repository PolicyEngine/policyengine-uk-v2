"""Check which engine HBAI income measure replicates an external baseline
forecast of real income growth (before housing costs).

Runs the engine over 2025/26-2029/30 on EFRS, deflates each nominal HBAI mean
to real terms via the CPI index, and compares cumulative real growth against
the external reference column.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "interfaces" / "python"))

from policyengine_uk_compiled import Simulation

from rich.console import Console
from rich.table import Table

YEARS = [2025, 2026, 2027, 2028, 2029]
BASE_YEAR = 2025  # express everything in 2025/26 prices

# External model's "overall real income (before housing costs)", cumulative
# growth vs 2025/26 (the right-hand column of the reference table).
REFERENCE_CUM = {2025: 0.0, 2026: 0.9, 2027: 0.8, 2028: 0.3, 2029: 0.3}

MEASURES = ["mean_bhc", "mean_equiv_bhc", "mean_ahc", "mean_equiv_ahc"]

console = Console()


def real_hbai(year: int) -> dict:
    sim = Simulation(year=year, dataset="efrs")
    res = sim.run()
    real_baseline, _ = res.real_hbai_incomes(base_year=BASE_YEAR)
    return {m: getattr(real_baseline, m) for m in MEASURES}


def main():
    reals = {y: real_hbai(y) for y in YEARS}

    table = Table(title="Cumulative real growth vs 2025/26 by income measure (CPI-deflated)")
    table.add_column("Year")
    table.add_column("Reference (BHC)", justify="right")
    for m in MEASURES:
        table.add_column(m, justify="right")

    for y in YEARS:
        fy = f"{y}/{(y + 1) % 100:02d}"
        row = [fy, f"{REFERENCE_CUM[y]:+.1f}%"]
        for m in MEASURES:
            cum = (reals[y][m] / reals[YEARS[0]][m] - 1) * 100
            gap = cum - REFERENCE_CUM[y]
            row.append(f"{cum:+.1f}% ({gap:+.1f})")
        table.add_row(*row)

    console.print(table)
    console.print("Each cell: cumulative real growth (gap vs reference in pp).")


if __name__ == "__main__":
    main()
