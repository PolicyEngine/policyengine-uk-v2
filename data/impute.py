"""Impute wealth (WAS) and consumption (LCFS) onto clean FRS households.

Ports the Rust EFRS pipeline (`src/data/efrs/`) to Python. Trains a per-target
random forest on each donor survey using predictors shared with the FRS, then
predicts onto FRS recipient households. Finally rakes electricity/gas spending to
NEED income-band targets.

Reads/writes the clean CSV schema in place: takes an FRS clean year directory
(persons.csv, benunits.csv, households.csv) and fills the wealth/consumption
columns on households.csv.

Usage:
    python data/impute.py --frs-clean data/clean/frs/2023 \\
        --was-dir data/raw/was/round_8 --lcfs-dir data/raw/lcfs/2022
"""

from __future__ import annotations

import argparse
from pathlib import Path

import numpy as np
import pandas as pd
from rich.console import Console
from sklearn.ensemble import RandomForestRegressor

console = Console()


# ── Categorical encodings (mirror src/engine/entities.rs to_rf_code) ──────────

# FRS clean CSV already stores tenure/accommodation as rf_codes, and region as a
# name string. WAS/LCFS use survey-specific codes mapped to the same rf_code space.

_REGION_RF = {
    "North East": 0, "North West": 1, "Yorkshire": 2, "East Midlands": 3,
    "West Midlands": 4, "East of England": 5, "London": 6, "South East": 7,
    "South West": 8, "Wales": 9, "Scotland": 10, "Northern Ireland": 11,
}

# WAS gorr8 → rf_code (NI not sampled by WAS; Rust maps unknown→Wales=9).
_WAS_REGION_RF = {1: 0, 2: 1, 4: 2, 5: 3, 6: 4, 7: 5, 8: 6, 9: 7, 10: 8, 11: 9, 12: 10}

# LCFS Gorx → rf_code.
_LCFS_REGION_RF = {1: 0, 2: 1, 3: 2, 4: 3, 5: 4, 6: 5, 7: 6, 8: 7, 9: 8, 10: 9, 11: 10, 12: 11}

# LCFS tenure A122 → rf_code (matches lcfs_tenure in Rust).
_LCFS_TENURE_RF = {1: 2, 2: 3, 3: 4, 4: 4, 5: 1, 6: 1, 7: 0}
# LCFS accommodation A121 → rf_code (matches lcfs_accommodation in Rust).
_LCFS_ACCOM_RF = {1: 0, 2: 1, 3: 2, 4: 3, 5: 3, 6: 4}


# ── Wealth imputation (WAS round 8) ───────────────────────────────────────────

WEALTH_TARGETS = [
    "owned_land", "property_wealth", "corporate_wealth",
    "gross_financial_wealth", "net_financial_wealth", "main_residence_value",
    "other_residential_property_value", "non_residential_property_value",
    "savings", "num_vehicles",
]

# WAS feature order (must match FRS predictor order below):
# [hh_income, num_adults, num_children, pension_income, emp_income,
#  se_income, capital_income, bedrooms, council_tax, is_renting, region]


def _load_was_table(was_dir: Path) -> pd.DataFrame:
    matches = sorted(was_dir.glob("*hhold*.tab")) or sorted(was_dir.glob("*.tab"))
    if not matches:
        raise SystemExit(f"No WAS household .tab file in {was_dir}")
    df = pd.read_csv(matches[0], sep="\t", low_memory=False)
    df.columns = [c.lower() for c in df.columns]
    return df


def _col(df: pd.DataFrame, name: str) -> np.ndarray:
    """Numeric column by lowercased name; raises if absent."""
    if name not in df.columns:
        raise KeyError(f"WAS/LCFS column '{name}' missing — got {list(df.columns)[:8]}…")
    return pd.to_numeric(df[name], errors="coerce").fillna(0.0).to_numpy(dtype=float)


def _col_opt(df: pd.DataFrame, name: str) -> np.ndarray:
    """Numeric column; zeros if absent (mirrors Rust get_f64 fallback)."""
    if name not in df.columns:
        return np.zeros(len(df), dtype=float)
    return pd.to_numeric(df[name], errors="coerce").fillna(0.0).to_numpy(dtype=float)


def build_was_training(was_dir: Path) -> tuple[np.ndarray, dict[str, np.ndarray]]:
    df = _load_was_table(was_dir)
    console.print(f"  loaded {len(df)} WAS households")

    region = np.array([_WAS_REGION_RF.get(int(c), 9) for c in _col(df, "gorr8")], dtype=float)
    features = np.column_stack([
        _col(df, "dvtotinc_bhcr8"),         # hh_income
        _col(df, "numadultr8"),             # num_adults
        _col(df, "numch18r8"),              # num_children
        _col(df, "dvgippenr8_aggr"),        # pension_income
        _col(df, "dvgiempr8_aggr"),         # emp_income
        _col(df, "dvgiser8_aggr"),          # se_income
        _col(df, "dvgiinvr8_aggr"),         # capital_income
        _col(df, "hbedrmr8"),               # bedrooms
        _col(df, "dvctaxamtannualr8"),      # council_tax
        (_col(df, "dvprirntr8") == 1).astype(float),  # is_renting
        region,
    ])

    owned_land = np.maximum(_col(df, "dvlukvalr8_sum"), 0.0)
    main_res = np.maximum(_col(df, "dvhvaluer8"), 0.0)
    other_res = np.maximum(_col(df, "dvhsevalr8_sum"), 0.0)
    non_res = np.maximum(_col(df, "dvbldvalr8_sum"), 0.0)
    property_wealth = main_res + other_res + non_res + owned_land

    shares = np.maximum(_col(df, "dvfesharesr8_aggr"), 0.0)
    isas = np.maximum(_col(df, "dvisavalr8_aggr"), 0.0)
    total_pensions = np.maximum(_col(df, "totpen_oldr8_aggr"), 0.0)
    corporate_wealth = shares + isas + total_pensions

    net_financial = _col(df, "dvfnsvalr8_aggr")  # can be negative
    gross_financial = np.maximum(net_financial, 0.0) + corporate_wealth
    savings = np.maximum(_col(df, "dvsavalr8_aggr"), 0.0)
    num_vehicles = np.maximum(_col(df, "vcarnr8"), 0.0)

    targets = {
        "owned_land": owned_land,
        "property_wealth": property_wealth,
        "corporate_wealth": corporate_wealth,
        "gross_financial_wealth": gross_financial,
        "net_financial_wealth": net_financial,
        "main_residence_value": main_res,
        "other_residential_property_value": other_res,
        "non_residential_property_value": non_res,
        "savings": savings,
        "num_vehicles": num_vehicles,
    }
    return features, targets


def build_frs_wealth_features(persons: pd.DataFrame, households: pd.DataFrame) -> np.ndarray:
    p = persons
    is_adult = p["age"] >= 18
    aux = pd.DataFrame({
        "household_id": p["household_id"].to_numpy(),
        "is_adult": (p["age"] >= 18).astype(float).to_numpy(),
        "is_child": (p["age"] < 18).astype(float).to_numpy(),
        "emp_income": p["employment_income"].to_numpy(float),
        "se_income": p["self_employment_income"].to_numpy(float),
        "pension_income": p["private_pension_income"].to_numpy(float),
        "capital_income": (p["savings_interest"] + p["dividend_income"]).to_numpy(float),
        "hh_income": p[_INCOME_COLS].to_numpy(float).sum(axis=1),
    })
    g = aux.groupby("household_id").sum()
    agg = pd.DataFrame({
        "num_adults": g["is_adult"],
        "num_children": g["is_child"],
        "emp_income": g["emp_income"],
        "se_income": g["se_income"],
        "pension_income": g["pension_income"],
        "capital_income": g["capital_income"],
        "hh_income": g["hh_income"],
    })
    h = households.set_index("household_id").join(agg).reset_index()
    h[agg.columns] = h[agg.columns].fillna(0.0)

    is_renting = h["tenure_type"].isin([2, 3, 4]).astype(float)
    region = h["region"].map(_REGION_RF).fillna(6).to_numpy(dtype=float)
    _ = is_adult  # silence unused

    return np.column_stack([
        h["hh_income"].to_numpy(float),
        h["num_adults"].to_numpy(float),
        h["num_children"].to_numpy(float),
        h["pension_income"].to_numpy(float),
        h["emp_income"].to_numpy(float),
        h["se_income"].to_numpy(float),
        h["capital_income"].to_numpy(float),
        h["num_bedrooms"].to_numpy(float),
        h["council_tax_annual"].to_numpy(float),
        is_renting.to_numpy(float),
        region,
    ])


_INCOME_COLS = [
    "employment_income", "self_employment_income", "private_pension_income",
    "state_pension", "savings_interest", "dividend_income", "property_income",
    "maintenance_income", "miscellaneous_income", "other_income",
]


# ── Consumption imputation (LCFS 2022) ─────────────────────────────────────────

CONSUMPTION_TARGETS = [
    "food_consumption", "alcohol_consumption", "tobacco_consumption",
    "clothing_consumption", "housing_water_electricity_consumption",
    "furnishings_consumption", "health_consumption", "transport_consumption",
    "communication_consumption", "recreation_consumption", "education_consumption",
    "restaurants_consumption", "miscellaneous_consumption", "petrol_spending",
    "diesel_spending", "domestic_energy_consumption", "electricity_consumption",
    "gas_consumption",
]

# LCFS feature order:
# [num_adults, num_children, region, emp_income, se_income, private_pension,
#  hbai_net_income, tenure, accommodation, has_fuel_consumption]


def _derive_energy(b226, b489, b490, p537, mean_elec_share):
    """Per-row electricity/gas split (ports derive_energy in lcfs.rs)."""
    elec = np.empty_like(b226)
    gas = np.empty_like(b226)
    case1 = b226 > 0.0
    case2 = (~case1) & (b489 > 0.0) & (b490 > 0.0)
    case3 = (~case1) & (~case2) & (b489 > 0.0)
    case4 = ~(case1 | case2 | case3)
    elec[case1] = b226[case1]
    gas[case1] = np.maximum(p537[case1] - b226[case1], 0.0)
    elec[case2] = np.maximum(b489[case2] - b490[case2], 0.0)
    gas[case2] = b490[case2]
    elec[case3] = b489[case3] * mean_elec_share
    gas[case3] = b489[case3] * (1.0 - mean_elec_share)
    elec[case4] = p537[case4] * mean_elec_share
    gas[case4] = p537[case4] * (1.0 - mean_elec_share)
    return elec, gas


def build_lcfs_training(lcfs_dir: Path) -> tuple[np.ndarray, dict[str, np.ndarray], np.ndarray]:
    hh_matches = sorted(lcfs_dir.glob("dvhh_*.tab")) or sorted(lcfs_dir.glob("*dvhh*.tab"))
    per_matches = sorted(lcfs_dir.glob("dvper_*.tab")) or sorted(lcfs_dir.glob("*dvper*.tab"))
    if not hh_matches or not per_matches:
        raise SystemExit(f"No LCFS dvhh/dvper .tab files in {lcfs_dir}")
    hh = pd.read_csv(hh_matches[0], sep="\t", low_memory=False)
    per = pd.read_csv(per_matches[0], sep="\t", low_memory=False)
    hh.columns = [c.lower() for c in hh.columns]
    per.columns = [c.lower() for c in per.columns]
    console.print(f"  loaded {len(hh)} LCFS households, {len(per)} persons")

    # Aggregate person incomes to household by case.
    per_emp = _col(per, "b303p")
    per_se = _col(per, "b3262p")
    per_pp = _col(per, "p049p")
    per_age = _col(per, "a005p")
    pcase = per["case"].to_numpy()
    pdf = pd.DataFrame({
        "case": pcase, "emp": per_emp, "se": per_se, "pp": per_pp,
        "adult": (per_age >= 18).astype(int), "child": (per_age < 18).astype(int),
    })
    pagg = pdf.groupby("case").sum()

    case = hh["case"].to_numpy()
    inc = pagg.reindex(case).fillna(0.0)

    b226 = _col(hh, "b226")
    b489 = _col(hh, "b489")
    b490 = _col(hh, "b490")
    p537 = _col(hh, "p537")
    dd_mask = (b226 > 0.0) & (p537 > 0.0)
    mean_elec_share = float((b226[dd_mask] / p537[dd_mask]).mean()) if dd_mask.any() else 0.52

    elec_weekly, gas_weekly = _derive_energy(b226, b489, b490, p537, mean_elec_share)

    region = np.array([_LCFS_REGION_RF.get(int(c), 6) for c in _col(hh, "gorx")], dtype=float)
    tenure = np.array([_LCFS_TENURE_RF.get(int(c), 5) for c in _col(hh, "a122")], dtype=float)
    accom = np.array([_LCFS_ACCOM_RF.get(int(c), 5) for c in _col(hh, "a121")], dtype=float)
    hbai_income = _col(hh, "p389p") * 52.0

    features = np.column_stack([
        inc["adult"].to_numpy(float),
        inc["child"].to_numpy(float),
        region,
        inc["emp"].to_numpy(float) * 52.0,
        inc["se"].to_numpy(float) * 52.0,
        inc["pp"].to_numpy(float) * 52.0,
        hbai_income,
        tenure,
        accom,
        np.zeros(len(hh)),  # has_fuel_consumption — filled below
    ])

    def wk(col):  # weekly → annual, floored at zero
        return np.maximum(_col(hh, col), 0.0) * 52.0

    c021 = _col_opt(hh, "c021")
    c022 = _col_opt(hh, "c022")
    p602 = np.maximum(_col(hh, "p602"), 0.0)
    alcohol = np.where(c021 > 0.0, np.maximum(c021, 0.0), p602 * 0.70) * 52.0
    tobacco = np.where(c022 > 0.0, np.maximum(c022, 0.0), p602 * 0.30) * 52.0

    targets = {
        "food_consumption": wk("p601"),
        "alcohol_consumption": alcohol,
        "tobacco_consumption": tobacco,
        "clothing_consumption": wk("p603"),
        "housing_water_electricity_consumption": wk("p604"),
        "furnishings_consumption": wk("p605"),
        "health_consumption": wk("p606"),
        "transport_consumption": wk("p607"),
        "communication_consumption": wk("p608"),
        "recreation_consumption": wk("p609"),
        "education_consumption": wk("p610"),
        "restaurants_consumption": wk("p611"),
        "miscellaneous_consumption": wk("p612"),
        "petrol_spending": wk("c72211"),
        "diesel_spending": wk("c72212"),
        "domestic_energy_consumption": np.maximum(p537, 0.0) * 52.0,
        "electricity_consumption": np.maximum(elec_weekly, 0.0) * 52.0,
        "gas_consumption": np.maximum(gas_weekly, 0.0) * 52.0,
    }
    transport_annual = targets["transport_consumption"]
    return features, targets, transport_annual


def build_frs_consumption_features(
    persons: pd.DataFrame, households: pd.DataFrame
) -> np.ndarray:
    aux = pd.DataFrame({
        "household_id": persons["household_id"].to_numpy(),
        "num_adults": (persons["age"] >= 18).astype(float).to_numpy(),
        "num_children": (persons["age"] < 18).astype(float).to_numpy(),
        "emp_income": persons["employment_income"].to_numpy(float),
        "se_income": persons["self_employment_income"].to_numpy(float),
        "pension_income": persons["private_pension_income"].to_numpy(float),
        "hbai": persons[_INCOME_COLS].to_numpy(float).sum(axis=1),
    })
    agg = aux.groupby("household_id").sum()
    h = households.set_index("household_id").join(agg).reset_index()
    h[agg.columns] = h[agg.columns].fillna(0.0)
    region = h["region"].map(_REGION_RF).fillna(6).to_numpy(dtype=float)

    return np.column_stack([
        h["num_adults"].to_numpy(float),
        h["num_children"].to_numpy(float),
        region,
        h["emp_income"].to_numpy(float),
        h["se_income"].to_numpy(float),
        h["pension_income"].to_numpy(float),
        h["hbai"].to_numpy(float),
        h["tenure_type"].to_numpy(float),
        h["accommodation_type"].to_numpy(float),
        np.zeros(len(h)),  # has_fuel_consumption — filled below
    ])


# ── has_fuel_consumption (vehicle-ownership proxy) ─────────────────────────────

def fill_has_fuel(
    frs_feat: np.ndarray, num_vehicles: np.ndarray,
    lcfs_feat: np.ndarray, lcfs_transport: np.ndarray, seed: int = 42,
) -> None:
    """Set feature column 9 (has_fuel) in place for FRS and LCFS rows.

    FRS: derive from imputed num_vehicles (90% ICE share). LCFS: transport-spend
    proxy (>£500 and 78% ownership × 90% ICE). Mirrors impute_has_fuel in lcfs.rs.
    """
    rng = np.random.default_rng(seed)
    has_vehicle = num_vehicles >= 0.5
    frs_feat[:, 9] = (has_vehicle & (rng.random(len(frs_feat)) < 0.90)).astype(float)

    has_vehicle_lcfs = (lcfs_transport > 500.0) & (rng.random(len(lcfs_feat)) < 0.78)
    lcfs_feat[:, 9] = (has_vehicle_lcfs & (rng.random(len(lcfs_feat)) < 0.90)).astype(float)


# ── NEED energy calibration ────────────────────────────────────────────────────

_ELEC_RATE = 0.2735
_GAS_RATE = 0.0689
_NEED_GAS_KWH = np.array([7755, 9325, 10426, 11231, 11870, 12576, 13367, 14792, 16477, 18850], float)
_NEED_ELEC_KWH = np.array([2412, 2689, 2857, 3029, 3148, 3303, 3458, 3793, 4258, 5100], float)
_INCOME_BANDS = np.array([15000, 20000, 25000, 30000, 35000, 40000, 50000, 75000, 100000, np.inf], float)


def _income_band(gross: np.ndarray) -> np.ndarray:
    return np.searchsorted(_INCOME_BANDS, gross, side="right").clip(0, 9)


def calibrate_energy_to_need(
    households: pd.DataFrame, gross_income: np.ndarray, weight: np.ndarray
) -> None:
    """Rake electricity/gas to NEED income-band means in place (ports calibrate.rs)."""
    band = _income_band(gross_income)
    elec = households["electricity_consumption"].to_numpy(float).copy()
    gas = households["gas_consumption"].to_numpy(float).copy()
    target_elec = _NEED_ELEC_KWH * _ELEC_RATE
    target_gas = _NEED_GAS_KWH * _GAS_RATE

    for _ in range(20):
        for b in range(10):
            sel = band == b
            tw = weight[sel].sum()
            if tw < 1.0:
                continue
            mean_elec = (weight[sel] * elec[sel]).sum() / tw
            mean_gas = (weight[sel] * gas[sel]).sum() / tw
            ef = target_elec[b] / mean_elec if mean_elec > 1.0 else 1.0
            gf = target_gas[b] / mean_gas if mean_gas > 1.0 else 1.0
            elec[sel] *= 1.0 + 0.5 * (ef - 1.0)
            gas[sel] *= 1.0 + 0.5 * (gf - 1.0)

        # Tenure adjustment (3 NEED categories), reference = median band 4.
        ten_cat = households["tenure_type"].map(
            {0: 0, 1: 0, 4: 1, 2: 2, 3: 2, 5: 0}).fillna(0).to_numpy()
        for tc in range(3):
            sel = ten_cat == tc
            tw = weight[sel].sum()
            if tw < 1.0:
                continue
            mean_elec = (weight[sel] * elec[sel]).sum() / tw
            mean_gas = (weight[sel] * gas[sel]).sum() / tw
            if abs(mean_elec / target_elec[4] - 1.0) > 0.1 and mean_elec > 0:
                elec[sel] *= 1.0 + 0.3 * (target_elec[4] / mean_elec - 1.0)
            if abs(mean_gas / target_gas[4] - 1.0) > 0.1 and mean_gas > 0:
                gas[sel] *= 1.0 + 0.3 * (target_gas[4] / mean_gas - 1.0)

    households["electricity_consumption"] = elec
    households["gas_consumption"] = gas
    households["domestic_energy_consumption"] = elec + gas


# ── RF training / prediction ────────────────────────────────────────────────────

def _subsample(features: np.ndarray, targets: dict[str, np.ndarray], max_train: int):
    n = len(features)
    if n <= max_train:
        return features, targets
    stride = n // max_train
    idx = np.arange(0, n, stride)
    return features[idx], {k: v[idx] for k, v in targets.items()}


def train_predict(
    train_feat: np.ndarray, train_targets: dict[str, np.ndarray],
    predict_feat: np.ndarray, n_trees: int, seed: int,
) -> dict[str, np.ndarray]:
    # One multi-output forest over all targets: the trees are built once on the
    # shared feature matrix and predict every target jointly, instead of fitting
    # a separate forest per target. Same trees/features — just amortised.
    names = list(train_targets)
    y = np.column_stack([train_targets[n] for n in names])
    model = RandomForestRegressor(n_estimators=n_trees, random_state=seed, n_jobs=-1)
    model.fit(train_feat, y)
    pred = model.predict(predict_feat)
    return {name: pred[:, i] for i, name in enumerate(names)}


# ── HHFCE level calibration ────────────────────────────────────────────────────

# COICOP target variable → household columns it aggregates. LCFS survey spend
# systematically under-records vs national-accounts HHFCE (worst for imputed
# rentals in housing, financial services in misc, and alcohol/tobacco), so we
# scale each category's imputed total to the HHFCE control for the year.
_HHFCE_COLS = {
    "food_consumption": ["food_consumption"],
    "alcohol_and_tobacco_consumption": ["alcohol_consumption", "tobacco_consumption"],
    "clothing_consumption": ["clothing_consumption"],
    "housing_water_electricity_consumption": ["housing_water_electricity_consumption"],
    "furnishings_consumption": ["furnishings_consumption"],
    "health_consumption": ["health_consumption"],
    "transport_consumption": ["transport_consumption"],
    "communication_consumption": ["communication_consumption"],
    "recreation_consumption": ["recreation_consumption"],
    "education_consumption": ["education_consumption"],
    "restaurants_consumption": ["restaurants_consumption"],
    "miscellaneous_consumption": ["miscellaneous_consumption"],
}

_TARGETS_PATH = Path(__file__).resolve().parent / "calibration_targets.json"


def scale_consumption_to_hhfce(households: pd.DataFrame, year: int) -> list[tuple[str, float]]:
    """Scale each COICOP category so its weighted total matches the HHFCE control."""
    import json

    targets = json.loads(_TARGETS_PATH.read_text())["targets"]
    controls = {
        t["variable"]: t["value"]
        for t in targets
        if t.get("source") == "Eurostat/ONS HHFCE COICOP" and t["year"] == year
    }
    w = households["weight"].to_numpy(float)
    factors = []
    for var, cols in _HHFCE_COLS.items():
        if var not in controls:
            continue
        imputed = sum((w * households[c].to_numpy(float)).sum() for c in cols)
        if imputed <= 0:
            continue
        f = controls[var] / imputed
        for c in cols:
            households[c] = households[c].to_numpy(float) * f
        factors.append((var, f))
    return factors


# ── Orchestration ────────────────────────────────────────────────────────────

def impute(
    frs_clean: Path, was_dir: Path, lcfs_dir: Path,
    year: int | None = None, seed: int = 42, spi_dir: Path | None = None,
    spi_block_dir: Path | None = None, spi_block_year: int | None = None,
) -> None:
    persons = pd.read_csv(frs_clean / "persons.csv")
    benunits = pd.read_csv(frs_clean / "benunits.csv")
    households = pd.read_csv(frs_clean / "households.csv")
    console.print(f"[bold]Imputing onto {len(households)} FRS households[/bold]")
    if year is None:
        year = int(frs_clean.name)

    # ── SPI high-income injection — runs first so the injected single-adult
    # households get wealth/consumption imputed alongside the FRS records. Drops
    # FRS households with a high earner and appends a prebuilt SPI tail block
    # (built once from the most recent survey year, reused verbatim every year so
    # each SPI earner keeps the same donor family — see spi_topup). ──
    if spi_dir is not None:
        from build_targets import RAW_DIR, load_efo_earnings_index
        from spi_topup import inject_spi_block
        from spi_unearned import impute_unearned_income

        earnings_index = load_efo_earnings_index(RAW_DIR / "obr" / "efo_economy.xlsx")

        # Impute unearned income onto FRS persons first (the FRS under-records it),
        # then inject SPI high earners — the injected singles keep their tape values.
        console.print("Imputing SPI unearned income (interest/dividends/property)…")
        n_ad = impute_unearned_income(persons, spi_dir, year, earnings_index, seed=seed)
        console.print(f"  imputed unearned income for {n_ad} FRS adults")

        console.print("Injecting SPI high-income earners…")
        block = (
            pd.read_csv(spi_block_dir / "persons.csv"),
            pd.read_csv(spi_block_dir / "benunits.csv"),
            pd.read_csv(spi_block_dir / "households.csv"),
        )
        persons, benunits, households, n_inj, n_drop = inject_spi_block(
            persons, benunits, households, block, spi_block_year, year, earnings_index,
        )
        console.print(
            f"  dropped {n_drop} FRS high-income households, injected {n_inj} SPI singles"
        )

    # ── Wealth (must run first — provides num_vehicles for fuel proxy) ──
    console.print("Training wealth models from WAS round 8…")
    was_feat, was_targets = build_was_training(was_dir)
    was_feat, was_targets = _subsample(was_feat, was_targets, 1500)
    frs_wealth_feat = build_frs_wealth_features(persons, households)
    wealth = train_predict(was_feat, was_targets, frs_wealth_feat, n_trees=100, seed=seed)

    is_renter = households["tenure_type"].isin([2, 3, 4]).to_numpy()
    for name in WEALTH_TARGETS:
        v = wealth[name]
        if name == "net_financial_wealth":
            households[name] = v  # can be negative
        elif name == "num_vehicles":
            households[name] = np.maximum(v, 0.0).round()
        elif name in ("owned_land", "property_wealth", "main_residence_value",
                      "other_residential_property_value", "non_residential_property_value"):
            households[name] = np.where(is_renter, 0.0, np.maximum(v, 0.0))
        else:
            households[name] = np.maximum(v, 0.0)

    # ── Consumption ──
    console.print("Training consumption models from LCFS 2022…")
    lcfs_feat, lcfs_targets, lcfs_transport = build_lcfs_training(lcfs_dir)
    frs_cons_feat = build_frs_consumption_features(persons, households)

    fill_has_fuel(
        frs_cons_feat, households["num_vehicles"].to_numpy(float),
        lcfs_feat, lcfs_transport, seed=seed,
    )

    lcfs_feat, lcfs_targets = _subsample(lcfs_feat, lcfs_targets, 1500)
    cons = train_predict(lcfs_feat, lcfs_targets, frs_cons_feat, n_trees=50, seed=seed)
    for name in CONSUMPTION_TARGETS:
        households[name] = np.maximum(cons[name], 0.0)

    # Zero fuel spend for non-fuel households.
    no_fuel = frs_cons_feat[:, 9] < 0.5
    households.loc[no_fuel, "petrol_spending"] = 0.0
    households.loc[no_fuel, "diesel_spending"] = 0.0

    # ── HHFCE level calibration ──
    factors = scale_consumption_to_hhfce(households, year)
    if factors:
        console.print("  HHFCE scaling factors:")
        for var, f in sorted(factors, key=lambda r: -r[1]):
            console.print(f"    {var:<44} {f:.2f}x")

    # ── NEED energy calibration ──
    gross_by_hh = pd.Series(
        persons[_INCOME_COLS].to_numpy(float).sum(axis=1),
    ).groupby(persons["household_id"].to_numpy()).sum()
    gross = households["household_id"].map(gross_by_hh).to_numpy(dtype=float)
    gross = np.nan_to_num(gross)
    calibrate_energy_to_need(households, gross, households["weight"].to_numpy(float))

    households.to_csv(frs_clean / "households.csv", index=False)
    if spi_dir is not None:
        persons.to_csv(frs_clean / "persons.csv", index=False)
        benunits.to_csv(frs_clean / "benunits.csv", index=False)
    w = households["weight"].to_numpy(float)
    mean_food = (w * households["food_consumption"].to_numpy(float)).sum() / w.sum()
    mean_prop = (w * households["property_wealth"].to_numpy(float)).sum() / w.sum()
    console.print(
        f"[green]Imputation complete.[/green] mean food £{mean_food:,.0f}/yr, "
        f"mean property wealth £{mean_prop:,.0f}"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--frs-clean", type=Path, required=True,
                        help="FRS clean year dir (persons.csv, households.csv)")
    parser.add_argument("--was-dir", type=Path, required=True)
    parser.add_argument("--lcfs-dir", type=Path, required=True)
    parser.add_argument("--spi-dir", type=Path, default=None)
    parser.add_argument("--year", type=int, default=None,
                        help="Defaults to the frs-clean dir name")
    parser.add_argument("--seed", type=int, default=42)
    args = parser.parse_args()
    impute(args.frs_clean, args.was_dir, args.lcfs_dir, year=args.year,
           seed=args.seed, spi_dir=args.spi_dir)


if __name__ == "__main__":
    main()
