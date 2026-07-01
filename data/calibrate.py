"""Reweight EFRS household survey data to match administrative targets.

Builds a matrix of household-level contributions to each calibration target by
running a baseline simulation through the Python engine interface
(`Simulation.run_microdata`), then optimises household weights with Adam in
log-space to minimise mean squared relative error. A hard max-weight clamp
(relative to the mean survey weight) is the sole regulariser. Years can be
warm-started from a neighbouring year's calibrated weights (fixed panel: rows
align positionally) so adjacent years share one solution through the
underdetermined null space instead of wandering independently.

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

# Survey-weight snapshot written by the build (efrs.py) into each year's clean
# dir right after imputation/uprating. It anchors calibration: the panel base
# year starts cold from it, warm-started years rescale their neighbour's
# weights to its population sum, and the max-weight clamp is always relative
# to its mean — so calibration can be re-run any number of times, separately
# from the build, with reproducible results. A build artifact (the engine
# reads `weight` from households.csv, never this file), so it doesn't
# reintroduce a survey-weight column into the CSV schema.
SURVEY_WEIGHTS_FILE = "survey_weights.npy"

console = Console()


def snapshot_survey_weights(data_dir: Path) -> None:
    """Persist households.csv's current weight column as the cold-start baseline.

    Called by the build after the survey weights are finalised (imputation for
    real years, population-index uprating for forecast years) so a later
    standalone calibration restores exactly these weights.
    """
    hh = pd.read_csv(data_dir / "households.csv")
    np.save(data_dir / SURVEY_WEIGHTS_FILE, hh["weight"].to_numpy(dtype=float))


# ── Config ───────────────────────────────────────────────────────────────────


class CalibrateConfig(BaseModel):
    epochs: int = 4096
    lr: float = 0.02
    beta1: float = 0.9
    beta2: float = 0.999
    eps: float = 1e-8
    log_interval: int = 16
    # Hard cap: no household may exceed max_weight_fraction × mean survey weight.
    # This is the sole control on weight concentration / ESS floor.
    # 0 disables. At 50× mean the top-1% weight share drops meaningfully with
    # ~0.1pp RMSRE cost vs unconstrained; below 20× RMSRE degrades substantially.
    max_weight_fraction: float = Field(default=50.0, ge=0.0)
    # Early stopping: halt once RMSRE improves by less than `tol` (%) over
    # `patience` consecutive log intervals.
    tol: float = Field(default=5e-4, ge=0.0)
    patience: int = Field(default=3, ge=1)
    # Drop targets below these magnitudes from the loss (RMSRE is magnitude-blind
    # so tiny targets dominate the gradient).
    min_target_value_sum: float = Field(default=5e7, ge=0.0)
    min_target_value_count: float = Field(default=1e4, ge=0.0)


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
        return _resolve_values(df, "child_tax_credit") + _resolve_values(
            df, "working_tax_credit"
        )
    # COICOP division 02 = alcohol + tobacco (imputed as separate columns).
    if variable == "alcohol_and_tobacco_consumption":
        return _resolve_values(df, "alcohol_consumption") + _resolve_values(
            df, "tobacco_consumption"
        )
    # Earned income = employment + self-employment (the UC in-work definition).
    if variable == "earned_income":
        return _resolve_values(df, "employment_income") + _resolve_values(
            df, "self_employment_income"
        )
    # DLA and PIP are each carried as care/daily-living + mobility components.
    if variable == "disability_living_allowance":
        return _resolve_values(df, "dla_care") + _resolve_values(df, "dla_mobility")
    if variable == "personal_independence_payment":
        return _resolve_values(df, "pip_daily_living") + _resolve_values(
            df, "pip_mobility"
        )
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

    entity_df = {
        "person": md_persons,
        "benunit": md_benunits,
        "household": md_households,
    }

    # Precompute, once per entity, each row's matrix-row index (-1 if its
    # household isn't in md_households). Replaces a per-target groupby + lambda
    # dict-lookup hot loop with a single vectorised gather per target.
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

        # Filter: restrict the rows that contribute to this target. Either a
        # numeric band on `filter.variable` in [min, max) (either bound may be
        # None), or a categorical equality via `filter.eq`.
        flt = t.get("filter")
        if flt:
            if flt.get("eq") is not None:
                mask = df[flt["variable"]].to_numpy() == flt["eq"]
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

    # Drop targets with no survey representation from the loss.
    train_mask = matrix.__abs__().sum(axis=0) > 1e-10
    n_skipped = int((~train_mask).sum())
    if n_skipped:
        console.print(
            f"  [yellow]Skipped {n_skipped} targets with no survey representation[/yellow]"
        )
    return matrix, y, train_mask


# ── Optimiser ────────────────────────────────────────────────────────────────


def calibrate(
    matrix: np.ndarray,
    y: np.ndarray,
    train_mask: np.ndarray,
    initial_weights: np.ndarray,
    config: CalibrateConfig,
    seed: int = 0,
) -> np.ndarray:
    """Adam in log-space minimising MSRE with a hard per-household weight cap.

    The cap (max_weight_fraction × mean of `initial_weights`) is the sole
    regulariser: no penalty term, no dropout. `initial_weights` is the survey
    snapshot for a cold start, or a neighbouring year's calibrated weights
    (rescaled to the survey population) for a warm-started chain; either way
    the result is a deterministic function of the inputs.
    """
    n_hh = matrix.shape[0]
    n_train = int(train_mask.sum())
    if n_hh == 0 or n_train == 0:
        return initial_weights.copy()

    w0 = np.where(initial_weights > 0.0, initial_weights, 1.0)
    u = np.log(w0).copy()

    if config.max_weight_fraction > 0.0:
        u_max = np.full(n_hh, np.log(config.max_weight_fraction * w0.mean()))
        np.clip(u, -np.inf, u_max, out=u)
    else:
        u_max = np.full(n_hh, np.inf)

    m = np.zeros(n_hh)
    v = np.zeros(n_hh)

    active = train_mask & (np.abs(y) > 1.0)
    y_safe = np.where(np.abs(y) > 1.0, y, 1.0)

    best_rmsre = np.inf
    stalls = 0

    for epoch in range(config.epochs):
        weights = np.exp(u)
        predictions = matrix.T @ weights
        residuals = np.where(active, predictions / y_safe - 1.0, 0.0)

        is_check = epoch % config.log_interval == 0 or epoch == config.epochs - 1
        if is_check:
            rmsre = np.sqrt(np.sum(residuals[active] ** 2) / n_train) * 100.0
            console.print(
                f"  epoch {epoch:>4}/{config.epochs}: training RMSRE {rmsre:.2f}%"
            )
            if config.tol > 0.0:
                if best_rmsre - rmsre < config.tol:
                    stalls += 1
                    if stalls >= config.patience:
                        console.print(
                            f"  [dim]converged at epoch {epoch} (Δ<{config.tol}% × {config.patience})[/dim]"
                        )
                        break
                else:
                    stalls = 0
                best_rmsre = min(best_rmsre, rmsre)

        grad = (2.0 / n_train) * weights * (matrix @ (residuals / y_safe))

        t = epoch + 1
        m = config.beta1 * m + (1.0 - config.beta1) * grad
        v = config.beta2 * v + (1.0 - config.beta2) * grad * grad
        m_hat = m / (1.0 - config.beta1**t)
        v_hat = v / (1.0 - config.beta2**t)
        u -= config.lr * m_hat / (np.sqrt(v_hat) + config.eps)
        np.clip(u, -np.inf, u_max, out=u)

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

    ess = weights.sum() ** 2 / (weights**2).sum()
    console.print("\n[bold green]Calibration complete[/bold green]")
    console.print(
        f"  households: {len(weights)}  "
        f"original weight sum: {initial_weights.sum():,.0f}  "
        f"calibrated weight sum: {weights.sum():,.0f}"
    )
    console.print(f"  training RMSRE: {rmsre:.2f}%")
    console.print(f"  ESS: {ess:,.0f}  ({ess / len(weights) * 100:.1f}% of n_hh)")

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
            targets[j]["name"],
            _fmt(preds[j]),
            _fmt(y[j]),
            f"[{colour}]{err:+.1f}%[/{colour}]",
        )
    console.print(table)


# ── Orchestration ────────────────────────────────────────────────────────────


def run(
    data_dir: Path,
    year: int,
    config: CalibrateConfig,
    sources: list[str] | None = None,
    warm_start_dir: Path | None = None,
) -> None:
    """Reweight one EFRS year against its calibration targets.

    Reads the already-imputed clean dir at `data_dir`, runs the baseline
    simulation and optimises weights — it never re-imputes, so swapping the
    target set (via `sources`) and re-running is cheap. `sources`, if given,
    restricts the loss to targets whose `source` is in the list (substring
    match), e.g. ["FRS grossed population"] for a demographics-only run.

    `warm_start_dir`, if given, points at a neighbouring year's already-
    calibrated clean dir (fixed panel: rows align positionally). Its calibrated
    weights — rescaled to this year's survey population — become the Adam
    starting point, so adjacent years share one weight solution rather than
    each wandering independently through the underdetermined null space. The
    max-weight clamp stays anchored to this year's survey snapshot (the rescale
    preserves the mean survey weight), and the chain stays reproducible: the
    base year starts cold from its snapshot and every other year is a
    deterministic function of its neighbour.
    """
    targets_all = json.loads(TARGETS_PATH.read_text())["targets"]
    targets = [t for t in targets_all if t["year"] == year]
    if not targets:
        raise SystemExit(f"No calibration targets for year {year}")
    if sources:
        targets = [t for t in targets if any(s in t["source"] for s in sources)]
        if not targets:
            raise SystemExit(f"No targets for year {year} matching sources {sources}")
        console.print(f"Restricted to {len(targets)} targets matching {sources}")
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
    consumption_cols = [
        c
        for c in input_hh.columns
        if c.endswith("_consumption") or c.endswith("_spending")
    ]
    households = md.households.reset_index(drop=True).copy()
    for c in consumption_cols:
        households[c] = input_hh[c].to_numpy()

    # Disability benefit amounts (DLA/PIP components, attendance allowance) are
    # carried in the FRS input but not re-emitted by the simulation's microdata
    # output, so join them onto the persons frame by row position (same contiguous
    # row-order guarantee as households above).
    input_persons = pd.read_csv(data_dir / "persons.csv")
    persons = md.persons.reset_index(drop=True).copy()
    disability_cols = [
        "dla_care",
        "dla_mobility",
        "pip_daily_living",
        "pip_mobility",
        "attendance_allowance",
    ]
    for c in disability_cols:
        if c in input_persons.columns:
            persons[c] = input_persons[c].to_numpy()

    matrix, y, train_mask = build_matrix(persons, md.benunits, households, targets)

    # Drop sub-threshold targets from the loss, floored by unit (£ for sum targets,
    # people for count targets). Keeps the magnitude-blind RMSRE from being hijacked
    # by micro-targets the survey can't represent (tiny SPI bands, transitional UC).
    too_small = np.array(
        [
            abs(t["value"])
            < (
                config.min_target_value_count
                if t["aggregation"] in ("count", "count_nonzero")
                else config.min_target_value_sum
            )
            for t in targets
        ]
    )
    n_dropped = int((train_mask & too_small).sum())
    if n_dropped:
        console.print(
            f"  [yellow]Dropped {n_dropped} sub-threshold targets from loss[/yellow]"
        )
    train_mask = train_mask & ~too_small

    # Held-out targets are excluded from the loss but still scored in the report,
    # so a structurally unfittable target (e.g. a count/total pair whose implied
    # per-recipient mean the survey can't match) stops dragging the global RMSRE
    # without vanishing from the diagnostics.
    holdout = np.array([t.get("holdout", False) for t in targets])
    n_holdout = int((train_mask & holdout).sum())
    if n_holdout:
        console.print(f"  [yellow]Held out {n_holdout} targets from loss[/yellow]")
    train_mask = train_mask & ~holdout

    snap_path = data_dir / SURVEY_WEIGHTS_FILE
    if snap_path.exists():
        survey_weights = np.load(snap_path)
        if len(survey_weights) != len(households):
            raise SystemExit(
                f"Survey-weight snapshot length {len(survey_weights)} != "
                f"household count {len(households)} in {data_dir}"
            )
    else:
        console.print(
            "  [yellow]No survey-weight snapshot; calibrating from current "
            "weight column (may be warm)[/yellow]"
        )
        survey_weights = households["weight"].to_numpy(dtype=float)

    start_weights = survey_weights
    if warm_start_dir is not None:
        warm = pd.read_csv(warm_start_dir / "households.csv")["weight"].to_numpy(
            dtype=float
        )
        if len(warm) != len(households):
            raise SystemExit(
                f"Warm-start weight count {len(warm)} ({warm_start_dir}) != "
                f"household count {len(households)} in {data_dir}"
            )
        start_weights = warm * (survey_weights.sum() / warm.sum())
        console.print(
            f"  warm-starting from {warm_start_dir.name} weights "
            f"(rescaled ×{survey_weights.sum() / warm.sum():.4f})"
        )

    weights = calibrate(matrix, y, train_mask, start_weights, config)
    print_report(targets, matrix, y, train_mask, weights, survey_weights)

    # Diagnostics: per-target predictions/errors before & after reweighting, plus
    # the calibrated weight distribution, consolidated into an HTML explorer by
    # data/calibration_report.py at the end of the build. Written to the
    # (gitignored) clean tree so a full rebuild leaves one file per year.
    preds_initial = matrix.T @ survey_weights
    preds_final = matrix.T @ weights

    def _rel_err(pred: float, actual: float) -> float | None:
        return float(pred / actual - 1.0) if abs(actual) > 1.0 else None

    diag_targets = [
        {
            "name": t["name"],
            "source": t["source"],
            "year": year,
            "actual": float(y[j]),
            "pred_initial": float(preds_initial[j]),
            "pred_final": float(preds_final[j]),
            "rel_err_initial": _rel_err(preds_initial[j], y[j]),
            "rel_err_final": _rel_err(preds_final[j], y[j]),
            "trained": bool(train_mask[j]),
        }
        for j, t in enumerate(targets)
    ]

    # Weight distribution: survey vs calibrated percentiles + summary stats. The
    # ratio of calibrated to survey weight shows how hard reweighting pushed each
    # record (the max-weight-ratio clamp bounds it to [1/r, r]).
    pcts = [1, 5, 10, 25, 50, 75, 90, 95, 99]
    ratio = weights / np.where(survey_weights > 0.0, survey_weights, np.nan)
    start_ratio = weights / np.where(start_weights > 0.0, start_weights, np.nan)
    weight_dist = {
        "percentiles": pcts,
        "survey": [float(np.percentile(survey_weights, p)) for p in pcts],
        "calibrated": [float(np.percentile(weights, p)) for p in pcts],
        "ratio": [float(np.nanpercentile(ratio, p)) for p in pcts],
        "n_households": int(len(weights)),
        "survey_sum": float(survey_weights.sum()),
        "calibrated_sum": float(weights.sum()),
        "ratio_min": float(np.nanmin(ratio)),
        "ratio_max": float(np.nanmax(ratio)),
        "mean_abs_log_drift": float(np.nanmean(np.abs(np.log(ratio)))),
        # Movement from the Adam starting point (== survey drift when cold);
        # under warm-start chaining this is the year-on-year weight churn.
        "warm_start": warm_start_dir is not None,
        "mean_abs_log_drift_vs_start": float(np.nanmean(np.abs(np.log(start_ratio)))),
    }

    diag = {"year": year, "targets": diag_targets, "weight_dist": weight_dist}
    diag_dir = REPO_ROOT / "data" / "clean" / "calib_diag"
    diag_dir.mkdir(parents=True, exist_ok=True)
    (diag_dir / f"{year}.json").write_text(json.dumps(diag))

    # Write calibrated weights back, aligned by row position.
    hh_path = data_dir / "households.csv"
    input_hh["weight"] = np.round(weights, 4)
    input_hh.to_csv(hh_path, index=False)
    console.print(f"[green]Wrote calibrated weights to {hh_path}[/green]")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--data",
        type=Path,
        required=True,
        help="EFRS clean dir (contains households.csv)",
    )
    parser.add_argument("--year", type=int, required=True)
    parser.add_argument(
        "--epochs", type=int, default=CalibrateConfig.model_fields["epochs"].default
    )
    parser.add_argument(
        "--max-weight-fraction",
        type=float,
        default=CalibrateConfig.model_fields["max_weight_fraction"].default,
    )
    parser.add_argument(
        "--sources",
        nargs="+",
        default=None,
        help="Restrict the loss to targets whose source matches "
        "(substring), e.g. --sources 'FRS grossed population'",
    )
    parser.add_argument(
        "--warm-start-dir",
        type=Path,
        default=None,
        help="Neighbouring year's calibrated clean dir to warm-start from "
        "(fixed panel: rows must align positionally)",
    )
    args = parser.parse_args()

    config = CalibrateConfig(
        epochs=args.epochs,
        max_weight_fraction=args.max_weight_fraction,
    )
    run(
        args.data,
        args.year,
        config,
        sources=args.sources,
        warm_start_dir=args.warm_start_dir,
    )


if __name__ == "__main__":
    main()
