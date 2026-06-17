"""Reweight EFRS household survey data to match administrative targets.

Builds a matrix of household-level contributions to each calibration target by
running a baseline simulation through the Python engine interface
(`Simulation.run_microdata`), then optimises household weights with Adam in
log-space to minimise mean squared relative error. A log-space weight-deviation
penalty plus a hard max-weight-ratio clamp keep calibrated weights close to the
survey originals.

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
    # Calibration is a quick nudge, not a fit: RMSRE plateaus by ~64 epochs and
    # the Adam loop runs in well under a second. More epochs just overfit target
    # noise while drifting weights away from the survey.
    epochs: int = 64
    lr: float = 0.1
    beta1: float = 0.9
    beta2: float = 0.999
    eps: float = 1e-8
    dropout: float = 0.05
    log_interval: int = 16
    # No household may exceed max_weight_ratio× its original weight (or fall
    # below 1/ratio×). Hard backstop applied after each step. 0 disables.
    max_weight_ratio: float = 10.0
    # Weight on the log-space deviation penalty: mean_i (log w_i - log w0_i)^2.
    # 0.05 keeps mean abs log-weight drift ~0.07 (vs ~0.23 at 0.01) for ~1pp
    # more RMSRE — favouring fidelity to the survey over chasing every target.
    weight_deviation_penalty: float = Field(default=5e-2, ge=0.0)


# ── Variable resolution ──────────────────────────────────────────────────────

# Variables whose microdata column name differs from the target variable name.
_COLUMN_ALIASES = {"stamp_duty": "property_transaction_tax"}


def _resolve_values(df: pd.DataFrame, variable: str) -> np.ndarray:
    """Return per-row values for `variable`, preferring baseline sim outputs.

    Mirrors the Rust resolver: simulation outputs (``baseline_*``) take priority
    over raw input columns of the same name.
    """
    # tax_credits has no single field — it is child + working tax credit.
    if variable == "tax_credits":
        return (
            _resolve_values(df, "child_tax_credit")
            + _resolve_values(df, "working_tax_credit")
        )
    # COICOP division 02 = alcohol + tobacco (imputed as separate columns).
    if variable == "alcohol_and_tobacco_consumption":
        return (
            _resolve_values(df, "alcohol_consumption")
            + _resolve_values(df, "tobacco_consumption")
        )
    # DLA and PIP are each carried as care/daily-living + mobility components.
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
    hh_index = {h: i for i, h in enumerate(hh_ids)}
    n_hh = len(hh_ids)
    n_t = len(targets)

    matrix = np.zeros((n_hh, n_t), dtype=float)
    y = np.zeros(n_t, dtype=float)

    entity_df = {"person": md_persons, "benunit": md_benunits, "household": md_households}

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

        # Filter: restrict the rows that contribute to this target. Either a
        # numeric band on `filter.variable` in [min, max) (either bound may be
        # None), or a categorical equality via `filter.eq`.
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

        grouped = pd.Series(contrib).groupby(df["household_id"].to_numpy()).sum()
        idx = grouped.index.map(lambda h: hh_index.get(h, -1)).to_numpy()
        keep = idx >= 0
        matrix[idx[keep], j] = grouped.to_numpy()[keep]

    # Drop targets with no survey representation from the loss.
    train_mask = matrix.__abs__().sum(axis=0) > 1e-10
    n_skipped = int((~train_mask).sum())
    if n_skipped:
        console.print(f"  [yellow]Skipped {n_skipped} targets with no survey representation[/yellow]")
    return matrix, y, train_mask


# ── Optimiser ────────────────────────────────────────────────────────────────

def calibrate(
    matrix: np.ndarray,
    y: np.ndarray,
    train_mask: np.ndarray,
    initial_weights: np.ndarray,
    config: CalibrateConfig,
) -> np.ndarray:
    """Adam optimisation of log-weights minimising MSRE + log-deviation penalty."""
    n_hh = matrix.shape[0]
    n_train = int(train_mask.sum())
    if n_hh == 0 or n_train == 0:
        return initial_weights.copy()

    w0 = np.where(initial_weights > 0.0, initial_weights, 1.0)
    u0 = np.log(w0)
    u = u0.copy()

    if config.max_weight_ratio > 0.0:
        u_max = u0 + np.log(config.max_weight_ratio)
        u_min = u0 - np.log(config.max_weight_ratio)
    else:
        u_max = np.full(n_hh, np.inf)
        u_min = np.full(n_hh, -np.inf)

    m = np.zeros(n_hh)
    v = np.zeros(n_hh)
    rng = np.random.default_rng(0)

    # Only training targets with non-trivial magnitude contribute to the loss.
    active = train_mask & (np.abs(y) > 1.0)
    y_safe = np.where(np.abs(y) > 1.0, y, 1.0)
    lam = config.weight_deviation_penalty

    for epoch in range(config.epochs):
        if config.dropout > 0.0:
            keep = rng.random(n_hh) >= config.dropout
            weights = np.where(keep, np.exp(u) / (1.0 - config.dropout), 0.0)
        else:
            weights = np.exp(u)

        predictions = matrix.T @ weights
        residuals = np.where(active, predictions / y_safe - 1.0, 0.0)

        if epoch % config.log_interval == 0 or epoch == config.epochs - 1:
            rmsre = np.sqrt(np.sum(residuals[active] ** 2) / n_train) * 100.0
            console.print(f"  epoch {epoch:>4}/{config.epochs}: training RMSRE {rmsre:.2f}%")

        # Gradient of MSRE wrt u_i = exp-weighted projection of residuals.
        g_msre = (2.0 / n_train) * weights * (matrix @ (residuals / y_safe))
        g_pen = lam * (2.0 / n_hh) * (u - u0)
        grad = g_msre + g_pen

        t = epoch + 1
        m = config.beta1 * m + (1.0 - config.beta1) * grad
        v = config.beta2 * v + (1.0 - config.beta2) * grad * grad
        m_hat = m / (1.0 - config.beta1 ** t)
        v_hat = v / (1.0 - config.beta2 ** t)
        u -= config.lr * m_hat / (np.sqrt(v_hat) + config.eps)
        np.clip(u, u_min, u_max, out=u)

    return np.exp(u)


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

    console.print("\n[bold green]Calibration complete[/bold green]")
    console.print(
        f"  households: {len(weights)}  "
        f"original weight sum: {initial_weights.sum():,.0f}  "
        f"calibrated weight sum: {weights.sum():,.0f}"
    )
    console.print(f"  training RMSRE: {rmsre:.2f}%")

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

    # `data_dir` points at the year-specific clean dir (contains households.csv);
    # the binary expects the base dir whose child is the year subdir. Use an
    # absolute path since the engine runs the binary from its own cwd.
    data_dir = data_dir.resolve()
    base_dir = data_dir.parent if data_dir.name == str(year) else data_dir

    console.print(f"Running baseline simulation on {base_dir} (year {year})")
    md = Simulation(year=year, data_dir=str(base_dir)).run_microdata()

    # The engine remaps entity IDs to contiguous 0..N but preserves row order,
    # so microdata rows align positionally with the input CSV — join by position,
    # not by household_id (which differs between the two).
    input_hh = pd.read_csv(data_dir / "households.csv")
    if len(input_hh) != len(md.households):
        raise SystemExit(
            f"Household count mismatch: input {len(input_hh)} vs microdata {len(md.households)}"
        )
    consumption_cols = [c for c in input_hh.columns if c.endswith("_consumption") or c.endswith("_spending")]
    households = md.households.reset_index(drop=True).copy()
    for c in consumption_cols:
        households[c] = input_hh[c].to_numpy()

    # Disability benefit amounts (DLA/PIP components, attendance allowance) are
    # carried in the FRS input but not re-emitted by the simulation's microdata
    # output, so join them onto the persons frame by row position (same contiguous
    # row-order guarantee as households above).
    input_persons = pd.read_csv(data_dir / "persons.csv")
    persons = md.persons.reset_index(drop=True).copy()
    disability_cols = ["dla_care", "dla_mobility", "pip_daily_living",
                       "pip_mobility", "attendance_allowance"]
    for c in disability_cols:
        if c in input_persons.columns:
            persons[c] = input_persons[c].to_numpy()

    matrix, y, train_mask = build_matrix(persons, md.benunits, households, targets)
    initial_weights = households["weight"].to_numpy(dtype=float)

    weights = calibrate(matrix, y, train_mask, initial_weights, config)
    print_report(targets, matrix, y, train_mask, weights, initial_weights)

    # Diagnostics: per-target predictions before/after reweighting, for charting.
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

    # Write calibrated weights back, aligned by row position.
    hh_path = data_dir / "households.csv"
    input_hh["weight"] = np.round(weights, 4)
    input_hh.to_csv(hh_path, index=False)
    console.print(f"[green]Wrote calibrated weights to {hh_path}[/green]")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data", type=Path, required=True, help="EFRS clean dir (contains households.csv)")
    parser.add_argument("--year", type=int, required=True)
    parser.add_argument("--epochs", type=int, default=CalibrateConfig.model_fields["epochs"].default)
    parser.add_argument("--weight-deviation-penalty", type=float,
                        default=CalibrateConfig.model_fields["weight_deviation_penalty"].default)
    parser.add_argument("--max-weight-ratio", type=float,
                        default=CalibrateConfig.model_fields["max_weight_ratio"].default)
    args = parser.parse_args()

    config = CalibrateConfig(
        epochs=args.epochs,
        weight_deviation_penalty=args.weight_deviation_penalty,
        max_weight_ratio=args.max_weight_ratio,
    )
    run(args.data, args.year, config)


if __name__ == "__main__":
    main()
