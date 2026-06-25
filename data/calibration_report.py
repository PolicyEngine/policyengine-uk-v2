"""Consolidate per-year calibration diagnostics into one JSON for the docs site.

data/calibrate.py writes data/clean/calib_diag/{year}.json after each year's
reweighting (per-target start/final relative errors plus the survey-vs-calibrated
weight distribution). This script gathers them into a single payload consumed by
the calibration section of the documentation site (site/src/sections/
CalibrationSection.jsx), which renders a d3 fit heatmap and a sortable target
table.

Usage:
    python data/calibration_report.py     # build → site/public/calibration.json
"""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path

from rich.console import Console

REPO_ROOT = Path(__file__).resolve().parent.parent
DIAG_DIR = REPO_ROOT / "data" / "clean" / "calib_diag"
TARGETS_PATH = REPO_ROOT / "data" / "calibration_targets.json"
OUT_PATH = REPO_ROOT / "site" / "public" / "calibration.json"

console = Console()


def _unit_by_name() -> dict[str, str]:
    """Map each target name to its display unit ('count' or 'gbp').

    The diagnostics JSON predates carrying the aggregation, so join on the
    targets file: count/count_nonzero targets are headcounts (people, benunits,
    households, claimants), everything else is a £ amount.
    """
    targets = json.loads(TARGETS_PATH.read_text())["targets"]
    return {
        t["name"]: ("count" if t["aggregation"] in ("count", "count_nonzero") else "gbp")
        for t in targets
    }


def _rmsre(targets: list[dict]) -> float:
    errs = [t["rel_err_final"] for t in targets if t["trained"] and t["rel_err_final"] is not None]
    if not errs:
        return 0.0
    return math.sqrt(sum(e * e for e in errs) / len(errs)) * 100.0


def load_years() -> dict[int, dict]:
    if not DIAG_DIR.exists():
        raise SystemExit(f"No diagnostics at {DIAG_DIR} — run a calibration build first.")
    out: dict[int, dict] = {}
    for f in sorted(DIAG_DIR.glob("*.json")):
        d = json.loads(f.read_text())
        out[int(d["year"])] = d
    if not out:
        raise SystemExit(f"No {DIAG_DIR}/*.json files found.")
    return out


def build_payload(years: dict[int, dict]) -> dict:
    units = _unit_by_name()
    for d in years.values():
        for t in d["targets"]:
            t["unit"] = units.get(t["name"], "gbp")
    summary = [
        {
            "year": yr,
            "rmsre": _rmsre(d["targets"]),
            "n_targets": sum(1 for t in d["targets"] if t["trained"]),
            "n_untrained": sum(1 for t in d["targets"] if not t["trained"]),
            "weight_dist": d["weight_dist"],
        }
        for yr, d in sorted(years.items())
    ]
    return {"summary": summary, "years": {str(yr): d for yr, d in sorted(years.items())}}


def main() -> None:
    argparse.ArgumentParser(description=__doc__).parse_args()
    years = load_years()
    payload = build_payload(years)
    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(payload, allow_nan=False, separators=(",", ":")))
    kb = OUT_PATH.stat().st_size / 1024
    console.print(
        f"[green]Wrote calibration payload for {len(years)} years "
        f"({kb:.0f} KB) → {OUT_PATH.relative_to(REPO_ROOT)}[/green]"
    )


if __name__ == "__main__":
    main()
