"""Reweight EFRS household survey data to match administrative targets.

Builds a matrix of household-level contributions to each calibration target by
running a baseline simulation through the Python engine interface
(`Simulation.run_microdata`), then calibrates household weights using the CALMAR
logit method: Newton-Raphson on the Lagrange multipliers of a logit-bounded
distance function. Weights are bounded to [L, U] × initial weight (CALMAR
defaults L=0.05, U=10), satisfying the margin constraints exactly where feasible.

Usage:
    python data/calibrate.py --data data/clean/efrs/2023 --year 2023
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np
import pandas as pd
from pydantic import BaseModel, Field
from rich.console import Console
from rich.table import Table

from policyengine_uk_compiled import Simulation

REPO_ROOT = Path(__file__).resolve().parent.parent
TARGETS_PATH = REPO_ROOT / "data" / "calibration_targets.json"

console = Console()


# ── Config ───────────────────────────────────────────────────────────────────

class CalibrateConfig(BaseModel):
    # CALMAR logit bounds: calibrated weight stays within [logit_l, logit_u] × w0.
    # Defaults match the DWP PSM CALMAR documentation (L=0.05, U=10).
    logit_l: float = Field(default=0.05, gt=0.0)
    logit_u: float = Field(default=10.0, gt=1.0)
    # Newton-Raphson iteration limit. Convergence is typically reached well
    # before this with the damped step; 200 is a safe ceiling.
    max_iter: int = 200
    # Drop targets below these magnitudes from the constraint system.
    # A tiny target (e.g. a £11m dividend micro-band) the survey can't represent
    # makes the NR Jacobian ill-conditioned and inflates weight ratios for no gain.
    min_target_value_sum: float = Field(default=5e7, ge=0.0)
    min_target_value_count: float = Field(default=1e4, ge=0.0)


# ── Variable resolution ──────────────────────────────────────────────────────

_COLUMN_ALIASES = {"stamp_duty": "property_transaction_tax"}


def _resolve_values(df: pd.DataFrame, variable: str) -> np.ndarray:
    """Return per-row values for `variable`, preferring baseline sim outputs."""
    if variable == "tax_credits":
        return (
            _resolve_values(df, "child_tax_credit")
            + _resolve_values(df, "working_tax_credit")
        )
    if variable == "alcohol_and_tobacco_consumption":
        return (
            _resolve_values(df, "alcohol_consumption")
            + _resolve_values(df, "tobacco_consumption")
        )
    if variable == "disability_living_allowance":
        return _resolve_values(df, "dla_care") + _resolve_values(df, "dla_mobility")
    if variable == "personal_independence_payment":
        return _resolve_values(df, "pip_daily_living") + _resolve_values(df, "pip_mobility")
    alias = _COLUMN_ALIASES.get(variable, variable)
    for col in (f"baseline_{alias}", f"baseline_{variable}", variable, alias):
        if col in df.columns:
            return df[col].to_numpy(dtype=float)
    return np.zeros(len(df), dtype=float)


def build_matrix(
    md_persons: pd.DataFrame,
    md_benunits: pd.DataFrame,
    md_households: pd.DataFrame,
    targets: list[dict],
) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Build M[i][j] (household i's contribution to target j), y[j], train_mask[j]."""
    hh_ids = md_households["household_id"].to_numpy()
    n_hh = len(hh_ids)
    n_t = len(targets)

    matrix = np.zeros((n_hh, n_t), dtype=float)
    y = np.zeros(n_t, dtype=float)

    entity_df = {"person": md_persons, "benunit": md_benunits, "household": md_households}

    hh_index_obj = pd.Index(hh_ids)
    entity_pos = {
        name: hh_index_obj.get_indexer(edf["household_id"].to_numpy())
        for name, edf in entity_df.items()
    }

    for j, t in enumerate(targets):
        y[j] = t["value"]
        df = entity_df[t["entity"]]
        vals = _resolve_values(df, t["variable"])

        agg = t["aggregation"]
        if agg == "sum":
            contrib = vals
        elif agg == "count_nonzero":
            contrib = (vals > 0.0).astype(float)
        elif agg == "count":
            contrib = np.ones_like(vals)
        else:
            continue

        flt = t.get("filter")
        if flt:
            if flt.get("eq") is not None:
                mask = (df[flt["variable"]].to_numpy() == flt["eq"])
            else:
                fvals = _resolve_values(df, flt["variable"])
                mask = np.ones(len(fvals), dtype=bool)
                if flt.get("min") is not None:
                    mask &= fvals >= flt["min"]
                if flt.get("max") is not None:
                    mask &= fvals < flt["max"]
            contrib = contrib * mask

        pos = entity_pos[t["entity"]]
        keep = pos >= 0
        matrix[:, j] = np.bincount(pos[keep], weights=contrib[keep], minlength=n_hh)

    train_mask = matrix.__abs__().sum(axis=0) > 1e-10
    n_skipped = int((~train_mask).sum())
    if n_skipped:
        console.print(f"  [yellow]Skipped {n_skipped} targets with no survey representation[/yellow]")
    return matrix, y, train_mask


# ── CALMAR logit calibration ─────────────────────────────────────────────────

def _logit_w(lam: np.ndarray, A: np.ndarray, w0: np.ndarray, L: float, U: float) -> np.ndarray:
    """Calibrated weights given Lagrange multipliers.

    w_i = w0_i * g(A_i · lam)  where g(x) = (L + U·exp(x)) / (1 + exp(x)).
    """
    x = A @ lam
    ex = np.exp(np.clip(x, -500, 500))
    return w0 * (L + U * ex) / (1.0 + ex)


def _logit_dg(lam: np.ndarray, A: np.ndarray, L: float, U: float) -> np.ndarray:
    """Elementwise derivative dg/dx, shape (n_hh,)."""
    x = A @ lam
    ex = np.exp(np.clip(x, -500, 500))
    return (U - L) * ex / (1.0 + ex) ** 2


def calibrate(
    matrix: np.ndarray,
    y: np.ndarray,
    train_mask: np.ndarray,
    initial_weights: np.ndarray,
    config: CalibrateConfig,
    start_weights: np.ndarray | None = None,
) -> np.ndarray:
    """CALMAR logit calibration via Newton-Raphson on Lagrange multipliers.

    Minimises sum_i w0_i · F(w_i/w0_i) subject to A[:, train].T @ w = t[train],
    where F is the logit distance function. Solved by NR on the residual
    r(lam) = A.T @ w(lam) - t, using the Jacobian J_kl = sum_i A_ik A_il w0_i dg_i.

    `initial_weights` anchor the bounds and the logit penalty (w0).
    `start_weights`, if given, warm-starts the NR by back-solving to an initial lam.
    """
    L, U = config.logit_l, config.logit_u
    A = matrix[:, train_mask]  # (n_hh, n_constraints)
    t = y[train_mask]

    w0 = np.where(initial_weights > 0.0, initial_weights, 1.0)

    if start_weights is not None:
        ws = np.where(start_weights > 0.0, start_weights, 1.0)
        g = np.clip(ws / w0, L + 1e-6, U - 1e-6)
        x0 = np.log((g - L) / (U - g))
        lam, _, _, _ = np.linalg.lstsq(A, x0, rcond=None)
        lam = np.clip(lam, -2.0, 2.0)
    else:
        lam = np.zeros(t.shape[0])

    best_lam = lam.copy()
    best_rel = np.inf

    with np.errstate(over="ignore", invalid="ignore", divide="ignore"):
        for _ in range(config.max_iter):
            w = _logit_w(lam, A, w0, L, U)
            r = A.T @ w - t
            rel = float(np.nanmax(np.abs(r / np.where(np.abs(t) > 0, t, 1.0))))
            if rel < best_rel:
                best_rel = rel
                best_lam = lam.copy()
            if rel < 1e-4:
                break
            dg = _logit_dg(lam, A, L, U)
            D = w0 * dg
            J = (A * D[:, None]).T @ A
            try:
                delta = np.linalg.solve(J, r)
            except np.linalg.LinAlgError:
                delta = np.linalg.lstsq(J, r, rcond=None)[0]

            # Damped step: halve until residual norm decreases.
            r_norm = float(np.nanmax(np.abs(r)))
            step = 1.0
            for _ in range(10):
                lam_try = lam - step * delta
                r_try = A.T @ _logit_w(lam_try, A, w0, L, U) - t
                if np.isfinite(r_try).all() and float(np.nanmax(np.abs(r_try))) < r_norm:
                    break
                step *= 0.5
            lam = lam - step * delta

    # Reconstruct full weight vector (unconstrained targets keep initial weight).
    w_final = _logit_w(best_lam, A, w0, L, U)
    return w_final


# ── Reporting ────────────────────────────────────────────────────────────────

def _fmt(v: float) -> str:
    a = abs(v)
    if a >= 1e9:
        return f"£{v / 1e9:.1f}bn"
    if a >= 1e6:
        return f"£{v / 1e6:.1f}m"
    if a >= 1e3:
        return f"{v / 1e3:.0f}k"
    return f"{v:.0f}"


def print_report(
    targets: list[dict],
    matrix: np.ndarray,
    y: np.ndarray,
    train_mask: np.ndarray,
    weights: np.ndarray,
    initial_weights: np.ndarray,
    show: int = 30,
) -> None:
    preds = matrix.T @ weights
    active = train_mask & (np.abs(y) > 1.0)
    rel_err = np.where(active, preds / np.where(active, y, 1.0) - 1.0, 0.0)
    n_train = int(active.sum())
    rmsre = np.sqrt(np.sum(rel_err[active] ** 2) / n_train) * 100.0 if n_train else 0.0

    w_ratio = weights / np.where(initial_weights > 0.0, initial_weights, 1.0)

    console.print("\n[bold green]Calibration complete[/bold green]")
    console.print(
        f"  households: {len(weights)}  "
        f"original weight sum: {initial_weights.sum():,.0f}  "
        f"calibrated weight sum: {weights.sum():,.0f}"
    )
    console.print(f"  training RMSRE: {rmsre:.2f}%")
    console.print(
        f"  weight ratio min: {w_ratio.min():.3f}  max: {w_ratio.max():.3f}"
    )

    order = np.argsort(-np.abs(rel_err))
    table = Table(show_header=True)
    table.add_column("Target")
    table.add_column("Predicted", justify="right")
    table.add_column("Actual", justify="right")
    table.add_column("Rel error", justify="right")
    for j in order[:show]:
        err = rel_err[j] * 100.0
        colour = "green" if abs(err) < 5 else "yellow" if abs(err) < 15 else "red"
        table.add_row(
            targets[j]["name"], _fmt(preds[j]), _fmt(y[j]),
            f"[{colour}]{err:+.1f}%[/{colour}]",
        )
    console.print(table)


# ── Orchestration ────────────────────────────────────────────────────────────

def run(data_dir: Path, year: int, config: CalibrateConfig) -> None:
    targets_all = json.loads(TARGETS_PATH.read_text())["targets"]
    targets = [t for t in targets_all if t["year"] == year]
    if not targets:
        raise SystemExit(f"No calibration targets for year {year}")
    console.print(f"Loaded {len(targets)} targets for {year}")

    data_dir = data_dir.resolve()
    base_dir = data_dir.parent if data_dir.name == str(year) else data_dir

    console.print(f"Running baseline simulation on {base_dir} (year {year})")
    md = Simulation(year=year, data_dir=str(base_dir)).run_microdata()

    input_hh = pd.read_csv(data_dir / "households.csv")
    if len(input_hh) != len(md.households):
        raise SystemExit(
            f"Household count mismatch: input {len(input_hh)} vs microdata {len(md.households)}"
        )
    consumption_cols = [c for c in input_hh.columns if c.endswith("_consumption") or c.endswith("_spending")]
    households = md.households.reset_index(drop=True).copy()
    for c in consumption_cols:
        households[c] = input_hh[c].to_numpy()

    input_persons = pd.read_csv(data_dir / "persons.csv")
    persons = md.persons.reset_index(drop=True).copy()
    disability_cols = ["dla_care", "dla_mobility", "pip_daily_living",
                       "pip_mobility", "attendance_allowance"]
    for c in disability_cols:
        if c in input_persons.columns:
            persons[c] = input_persons[c].to_numpy()

    matrix, y, train_mask = build_matrix(persons, md.benunits, households, targets)

    # Drop sub-threshold targets from the constraint system.
    too_small = np.array([
        abs(t["value"]) < (config.min_target_value_count
                           if t["aggregation"] in ("count", "count_nonzero")
                           else config.min_target_value_sum)
        for t in targets
    ])
    n_dropped = int((train_mask & too_small).sum())
    if n_dropped:
        console.print(f"  [yellow]Dropped {n_dropped} sub-threshold targets from constraint system[/yellow]")
    train_mask = train_mask & ~too_small

    initial_weights = households["weight"].to_numpy(dtype=float)

    start_weights = None
    if "start_weight" in input_hh.columns:
        start_weights = input_hh["start_weight"].to_numpy(dtype=float)

    console.print(
        f"  CALMAR logit NR  L={config.logit_l}  U={config.logit_u}  "
        f"constraints={int(train_mask.sum())}"
    )
    weights = calibrate(matrix, y, train_mask, initial_weights, config, start_weights)
    print_report(targets, matrix, y, train_mask, weights, initial_weights)

    preds_initial = matrix.T @ initial_weights
    preds_final = matrix.T @ weights
    diag = [
        {
            "name": t["name"], "source": t["source"], "year": year,
            "actual": float(y[j]), "pred_initial": float(preds_initial[j]),
            "pred_final": float(preds_final[j]), "trained": bool(train_mask[j]),
        }
        for j, t in enumerate(targets)
    ]
    (Path("/tmp") / f"calib_diag_{year}.json").write_text(json.dumps(diag))

    hh_path = data_dir / "households.csv"
    input_hh["weight"] = np.round(weights, 4)
    input_hh = input_hh.drop(columns=["start_weight"], errors="ignore")
    input_hh.to_csv(hh_path, index=False)
    console.print(f"[green]Wrote calibrated weights to {hh_path}[/green]")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data", type=Path, required=True, help="EFRS clean dir (contains households.csv)")
    parser.add_argument("--year", type=int, required=True)
    parser.add_argument("--logit-l", type=float, default=CalibrateConfig.model_fields["logit_l"].default)
    parser.add_argument("--logit-u", type=float, default=CalibrateConfig.model_fields["logit_u"].default)
    parser.add_argument("--max-iter", type=int, default=CalibrateConfig.model_fields["max_iter"].default)
    args = parser.parse_args()

    config = CalibrateConfig(
        logit_l=args.logit_l,
        logit_u=args.logit_u,
        max_iter=args.max_iter,
    )
    run(args.data, args.year, config)


if __name__ == "__main__":
    main()
