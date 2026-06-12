"""Calibration audit: compare engine program totals against OBR/DWP targets.

Runs a full population simulation for the requested year, extracts per-program
annual totals, and compares them against targets loaded from
data/targets/YYYY_YY.yaml. Programs whose ratio drifts beyond a tolerance
(default 20%) are flagged.

Usage:
    python data/calibrate.py                     # audit 2025
    python data/calibrate.py --year 2024
    python data/calibrate.py --tolerance 0.15

Targets file format (data/targets/2025_26.yaml):
    income_tax: 320.0       # £bn
    employee_ni: 180.0
    employer_ni: 100.0
    universal_credit: 55.0
    child_benefit: 14.0
    state_pension: 130.0
    # ... etc; omit a program to skip it in the audit
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

import yaml
from rich.console import Console
from rich.table import Table
from rich import box

REPO_ROOT = Path(__file__).resolve().parent.parent
TARGETS_DIR = Path(__file__).resolve().parent / "targets"

console = Console()

# Targets that map to a sum of engine fields rather than a single one.
DERIVED = {"tax_credits": ("child_tax_credit", "working_tax_credit")}


def load_targets(year: int) -> dict[str, float]:
    path = TARGETS_DIR / f"{year}_{str(year + 1)[-2:]}.yaml"
    if not path.exists():
        console.print(f"[red]No targets file at {path}[/red]")
        console.print("Create it with OBR/DWP annual totals in £bn. See the docstring for format.")
        sys.exit(1)
    with open(path) as f:
        return yaml.safe_load(f)


def run_simulation(year: int) -> dict[str, float]:
    from policyengine_uk_compiled import Simulation

    console.print(f"Running baseline simulation for {year}...")
    sim = Simulation(year=year)
    result = sim.run()
    pb = result.program_breakdown
    return {k: v / 1e9 for k, v in pb.model_dump().items()}  # convert £ → £bn


def audit(year: int, tolerance: float) -> bool:
    targets = load_targets(year)
    engine = run_simulation(year)

    table = Table(
        title=f"Calibration audit — {year}/{str(year + 1)[-2:]}",
        box=box.SIMPLE_HEAD,
        show_header=True,
    )
    table.add_column("Program", style="bold")
    table.add_column("Engine (£bn)", justify="right")
    table.add_column("Target (£bn)", justify="right")
    table.add_column("Ratio", justify="right")
    table.add_column("Status", justify="center")

    failures = []
    for program, target in sorted(targets.items()):
        modelled = engine.get(program)
        if modelled is None and program in DERIVED:
            modelled = sum(engine[k] for k in DERIVED[program])
        if modelled is None:
            console.print(f"[yellow]Warning: '{program}' not in engine output, skipping[/yellow]")
            continue
        ratio = modelled / target if target != 0 else float("inf")
        ok = abs(ratio - 1.0) <= tolerance
        status = "[green]✓[/green]" if ok else "[red]✗[/red]"
        table.add_row(
            program.replace("_", " "),
            f"{modelled:.1f}",
            f"{target:.1f}",
            f"{ratio:.2f}",
            status,
        )
        if not ok:
            failures.append(program)

    console.print(table)

    if failures:
        console.print(f"[red]{len(failures)} program(s) outside ±{tolerance:.0%} tolerance: {', '.join(failures)}[/red]")
        return False

    console.print(f"[green]All programs within ±{tolerance:.0%} tolerance.[/green]")
    return True


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--year", type=int, default=2025)
    parser.add_argument("--tolerance", type=float, default=0.20, help="Acceptable ratio deviation (default 0.20 = ±20%%)")
    args = parser.parse_args()

    ok = audit(args.year, args.tolerance)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
