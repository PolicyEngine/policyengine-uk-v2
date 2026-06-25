"""Extract clean FRS microdata (persons / benunits / households CSVs) from raw
UKDS tab files, in pure Python.

This is a faithful port of the Rust extractor (src/data/frs.rs + src/data/clean.rs)
so that all FRS data processing lives in /data. Output schema, column order and
numeric formatting match the Rust CSVs exactly.

Usage:
    python data/frs_extract.py --raw-dir data/raw/frs/2010 --year 2010 --out data/clean/frs/2010
"""

from __future__ import annotations

import argparse
from pathlib import Path

import numpy as np
import pandas as pd

WEEKS_IN_YEAR = 365.25 / 7.0

# ── Era ───────────────────────────────────────────────────────────────────
# Early  1994-2001: stdregn region, gross3 weight, age (not top-coded), no hrpid/esa/uc
# Mid    2002-2007: gvtregn region, gross4 weight, age80, hrpid, no esa
# Late   2008-2021: as Mid plus esagrp, limitill, carer1
# Current 2022+:    as Late plus buuc, gvtregno


def era_for_year(year: int) -> str:
    if year <= 2001:
        return "early"
    if year <= 2007:
        return "mid"
    if year <= 2021:
        return "late"
    return "current"


# ── Table loading ──────────────────────────────────────────────────────────


def load_table(raw_dir: Path, name: str, cols: list[str]) -> pd.DataFrame | None:
    """Load selected lower-cased columns from a .tab (or .csv) file. Returns None if absent."""
    tab = raw_dir / f"{name}.tab"
    csv = raw_dir / f"{name}.csv"
    if tab.exists():
        path, sep = tab, "\t"
    elif csv.exists():
        path, sep = csv, ","
    else:
        return None
    # Read full header to know which requested cols actually exist.
    header = pd.read_csv(path, sep=sep, nrows=0)
    header.columns = [c.lower() for c in header.columns]
    present = [c for c in cols if c in header.columns]
    df = pd.read_csv(path, sep=sep, usecols=lambda c: c.lower() in present, low_memory=False)
    df.columns = [c.lower() for c in df.columns]
    # Add any missing requested cols as zero so downstream getters are uniform.
    for c in cols:
        if c not in df.columns:
            df[c] = 0
    return df


def num(series: pd.Series) -> pd.Series:
    return pd.to_numeric(series, errors="coerce").fillna(0.0)


def pos(series: pd.Series) -> pd.Series:
    return num(series).clip(lower=0.0)


# ── Region mappings ──────────────────────────────────────────────────────────

REGION_NAMES = {
    0: "North East", 1: "North West", 2: "Yorkshire", 3: "East Midlands",
    4: "West Midlands", 5: "East of England", 6: "London", 7: "South East",
    8: "South West", 9: "Wales", 10: "Scotland", 11: "Northern Ireland",
}


def region_idx_from_stdregn(code: int) -> int:
    # 1994-2001 STDREGN coding → region index (matches Rust region_from_stdregn)
    return {
        1: 0, 2: 2, 3: 1, 4: 3, 5: 4, 6: 5, 7: 6, 8: 7, 9: 8, 10: 9, 11: 10, 12: 11,
    }.get(code, 6)


def region_idx_from_gvtregno(code: int) -> int:
    # 2002+ GVTREGN / GVTREGNO coding → region index
    return {
        1: 0, 2: 1, 4: 2, 5: 3, 6: 4, 7: 5, 8: 6, 9: 7, 10: 8, 11: 9, 12: 10, 13: 11,
    }.get(code, 6)


def tenure_rf_code(tentyp2: int) -> int:
    # from_frs_code → to_rf_code, composed
    m = {1: 2, 2: 3, 3: 4, 4: 4, 5: 1, 6: 1, 7: 0}
    return m.get(tentyp2, 5)


def accom_rf_code(typeacc: int) -> int:
    m = {1: 0, 2: 1, 3: 2, 4: 3, 5: 3, 6: 4}
    return m.get(typeacc, 5)


# ── Person-key helpers ───────────────────────────────────────────────────────


def add_key(df: pd.DataFrame) -> pd.DataFrame:
    df = df.copy()
    df["_key"] = df["sernum"].astype("int64").astype(str) + "_" + df["person"].astype("int64").astype(str)
    return df


# ── Sub-table aggregations (person-level, weekly) ────────────────────────────


def agg_accounts(t: pd.DataFrame) -> pd.DataFrame:
    t = add_key(t)
    accint = num(t["accint"])
    acct = num(t["account"]).astype("int64")
    acctax = num(t["acctax"]).astype("int64")
    invtax = num(t["invtax"]).astype("int64")

    si = pd.Series(0.0, index=t.index)
    grossed = np.where(acctax == 1, accint * 1.25, accint).clip(min=0.0)
    m1 = acct.isin([1, 3, 5, 27, 28])
    si = si.where(~m1, grossed)
    m2 = acct == 2
    si = si.where(~m2, (accint - 70.0 / 52.0).clip(lower=0.0))
    m9 = acct.isin([9, 21, 24])
    si = si.where(~m9, accint.clip(lower=0.0))
    m6 = acct == 6
    grossed6 = np.where(invtax == 1, accint * 1.25, accint).clip(min=0.0)
    si = si.where(~m6, grossed6)
    m78 = acct.isin([7, 8])
    si = si.where(~m78, accint.clip(lower=0.0))

    t = t.assign(savings_interest_weekly=si)
    out = t.groupby("_key")["savings_interest_weekly"].sum().to_frame()
    out["dividend_income_weekly"] = 0.0  # dividends come from adult.dividgro
    return out


def agg_oddjobs(t: pd.DataFrame) -> pd.DataFrame:
    t = add_key(t)
    t = t.assign(oddjob_weekly=pos(t["ojamt"]))
    return t.groupby("_key")["oddjob_weekly"].sum().to_frame()


def agg_jobs(t: pd.DataFrame) -> pd.DataFrame:
    t = add_key(t)
    t = t.assign(epc=pos(t["deduc1"]))
    out = t.groupby("_key")["epc"].sum().to_frame()
    out.columns = ["employee_pension_contributions_weekly"]
    return out


def agg_pensions(t: pd.DataFrame) -> pd.DataFrame:
    t = add_key(t)
    penpay = pos(t["penpay"])
    ptamt = num(t["ptamt"])
    ptinc = num(t["ptinc"]).astype("int64")
    poamt = num(t["poamt"])
    poinc = num(t["poinc"]).astype("int64")
    penoth = num(t["penoth"]).astype("int64")
    val = penpay.copy()
    val = val + ptamt.where((ptinc == 2) & (ptamt > 0.0), 0.0)
    val = val + poamt.where(((poinc == 2) | (penoth == 1)) & (poamt > 0.0), 0.0)
    t = t.assign(private_pension_weekly=val)
    out = t.groupby("_key")["private_pension_weekly"].sum().to_frame()
    return out


def agg_penprov(t: pd.DataFrame) -> pd.DataFrame:
    t = add_key(t)
    stemppen = num(t["stemppen"]).astype("int64")
    stemppay = num(t["stemppay"]).astype("int64")
    is_personal = stemppen.isin([5, 6]) | (stemppay == 1)
    penamt_raw = pos(t["penamt"])
    penamtpd = num(t["penamtpd"]).astype("int64")
    penamt = penamt_raw.where(penamtpd != 95, penamt_raw / 52.0)
    penamt = penamt.where(is_personal, 0.0)
    t = t.assign(ppc=penamt)
    out = t.groupby("_key")["ppc"].sum().to_frame()
    out.columns = ["personal_pension_contributions_weekly"]
    return out


# Benefit code → (column, optional var2-conditional handling)
BENEFIT_SIMPLE = {
    5: "state_pension", 3: "child_benefit", 19: "income_support",
    94: "housing_benefit", 12: "attendance_allowance", 1: "dla_sc", 2: "dla_m",
    13: "carers_allowance", 4: "pension_credit", 91: "child_tax_credit",
    90: "working_tax_credit", 95: "universal_credit", 97: "pip_m", 96: "pip_dl",
    6: "bereavement", 21: "maternity_allowance", 62: "winter_fuel",
    15: "industrial_injuries", 10: "sda", 30: "other_ni_state",
    117: "adp_dl", 118: "adp_m", 121: "cdp_care", 122: "cdp_mob", 112: "scp",
    8: "war_pension", 9: "war_pension",
}

BENEFIT_COLS = [
    "state_pension", "child_benefit", "income_support", "housing_benefit",
    "attendance_allowance", "dla_sc", "dla_m", "carers_allowance", "pension_credit",
    "child_tax_credit", "working_tax_credit", "universal_credit", "pip_m", "pip_dl",
    "esa_income", "esa_contrib", "jsa_income", "jsa_contrib",
    "bereavement", "maternity_allowance", "winter_fuel", "industrial_injuries",
    "sda", "war_pension", "other_ni_state", "adp_dl", "adp_m", "cdp_care", "cdp_mob", "scp",
]


def agg_benefits(t: pd.DataFrame) -> pd.DataFrame:
    t = add_key(t)
    benefit = num(t["benefit"]).astype("int64")
    benpd = num(t["benpd"]).astype("int64")
    benamt_raw = pos(t["benamt"])
    var2 = num(t["var2"]).astype("int64")
    # benpd 0/90/95/97 → annual lump, /52; benpd -1 for code 62 → /52; else weekly
    benamt = benamt_raw.copy()
    lump = benpd.isin([0, 90, 95, 97])
    benamt = benamt.where(~lump, benamt_raw / 52.0)
    wf_lump = (benpd == -1) & (benefit == 62)
    benamt = benamt.where(~wf_lump, benamt_raw / 52.0)

    t = t.assign(_benefit=benefit, _benamt=benamt, _var2=var2)
    for col in BENEFIT_COLS:
        t[col] = 0.0
    for code, col in BENEFIT_SIMPLE.items():
        mask = benefit == code
        t[col] = t[col] + benamt.where(mask, 0.0)
    # JSA (14): var2 1,3 contrib; 2,4 income
    jsa = benefit == 14
    t["jsa_contrib"] = t["jsa_contrib"] + benamt.where(jsa & var2.isin([1, 3]), 0.0)
    t["jsa_income"] = t["jsa_income"] + benamt.where(jsa & var2.isin([2, 4]), 0.0)
    # ESA (16)
    esa = benefit == 16
    t["esa_contrib"] = t["esa_contrib"] + benamt.where(esa & var2.isin([1, 3]), 0.0)
    t["esa_income"] = t["esa_income"] + benamt.where(esa & var2.isin([2, 4]), 0.0)

    return t.groupby("_key")[BENEFIT_COLS].sum()


# ── Disability band thresholds (midpoints, FRS 2023/24 rates) ────────────────


def disability_flags(b: pd.DataFrame) -> pd.DataFrame:
    dla_sc, dla_m = b["dla_sc"], b["dla_m"]
    pip_dl, pip_m, aa = b["pip_dl"], b["pip_m"], b["attendance_allowance"]
    out = pd.DataFrame(index=b.index)
    out["dla_care_low"] = (dla_sc > 0.0) & (dla_sc < 49.30)
    out["dla_care_mid"] = (dla_sc >= 49.30) & (dla_sc < 89.55)
    out["dla_care_high"] = dla_sc >= 89.55
    out["dla_mob_low"] = (dla_m > 0.0) & (dla_m < 51.32)
    out["dla_mob_high"] = dla_m >= 51.32
    out["pip_dl_std"] = (pip_dl > 0.0) & (pip_dl < 84.93)
    out["pip_dl_enh"] = pip_dl >= 84.93
    out["pip_mob_std"] = (pip_m > 0.0) & (pip_m < 51.32)
    out["pip_mob_enh"] = pip_m >= 51.32
    out["aa_low"] = (aa > 0.0) & (aa < 84.93)
    out["aa_high"] = aa >= 84.93
    out["is_disabled"] = (dla_sc + dla_m + pip_m + pip_dl + aa) > 0.0
    out["is_enhanced_disabled"] = out["dla_care_high"] | out["pip_dl_enh"]
    out["is_severely_disabled"] = out["pip_dl_enh"] | out["dla_care_high"]
    return out


# ── Boolean / float formatting to match Rust CSV ─────────────────────────────


def b2s(series: pd.Series) -> pd.Series:
    return series.astype(bool).map({True: "true", False: "false"})


def f2(series: pd.Series) -> pd.Series:
    return num(series).map(lambda v: f"{v:.2f}")


# ── Main extraction ──────────────────────────────────────────────────────────


def extract(raw_dir: Path, year: int, out_dir: Path) -> None:
    era = era_for_year(year)
    out_dir.mkdir(parents=True, exist_ok=True)

    household = load_table(raw_dir, "househol", [
        "sernum", "gross3", "gross4", "stdregn", "gvtregn", "gvtregno",
        "ctannual", "hhrent", "subrent", "cvpay", "bedroom6", "tentyp2", "typeacc",
    ])
    benunit = load_table(raw_dir, "benunit", [
        "sernum", "benunit", "buuc", "burent",
        "fsmbu", "fsfvbu", "fsmlkbu", "heartbu", "butvlic",
    ])
    adult = load_table(raw_dir, "adult", [
        "sernum", "benunit", "person", "sex", "age", "age80", "tothours",
        "uperson", "hrpid", "limitill", "esagrp", "empstatb", "lookwk", "carer1",
        "inearns", "seincam2", "inseinc", "inpeninc", "royyr1", "dividgro",
        "mntus1", "mntus2", "mntusam1", "mntusam2", "mntamt1", "mntamt2",
        "allow1", "allow2", "allow3", "allow4",
        "allpay1", "allpay2", "allpay3", "allpay4",
        "apamt", "apdamt", "pareamt", "aliamt",
    ])
    child = load_table(raw_dir, "child", [
        "sernum", "benunit", "person", "sex", "age", "chearns", "chrinc",
    ])

    # Pre-2013 main-scheme housing benefit is recorded on the renter record
    # (HBENAMT, weekly equivalent, one row per household), not in the benefits
    # table — benefit code 94 only appears from FRS 2013. Assigned to the HRP.
    renter = load_table(raw_dir, "renter", ["sernum", "hbenamt"]) if year < 2013 else None

    accounts = load_table(raw_dir, "accounts", ["sernum", "person", "accint", "account", "acctax", "invtax"])
    benefits = load_table(raw_dir, "benefits", ["sernum", "person", "benefit", "benamt", "benpd", "var2"])
    job = load_table(raw_dir, "job", ["sernum", "person", "deduc1"])
    pension = load_table(raw_dir, "pension", ["sernum", "person", "penpay", "ptamt", "ptinc", "poamt", "poinc", "penoth"])
    penprov = load_table(raw_dir, "penprov", ["sernum", "person", "stemppen", "stemppay", "penamt", "penamtpd"])
    oddjob = load_table(raw_dir, "oddjob", ["sernum", "person", "ojamt"])

    acc_agg = agg_accounts(accounts) if accounts is not None else None
    ben_agg = agg_benefits(benefits) if benefits is not None else None
    job_agg = agg_jobs(job) if job is not None else None
    pen_agg = agg_pensions(pension) if pension is not None else None
    pp_agg = agg_penprov(penprov) if penprov is not None else None
    oj_agg = agg_oddjobs(oddjob) if oddjob is not None else None

    # ── Households ──
    hh = household.copy()
    hh["sernum"] = num(hh["sernum"]).astype("int64")
    weight = num(hh["gross3"]) if era == "early" else num(hh["gross4"])
    if era == "early":
        ridx = num(hh["stdregn"]).astype("int64").map(region_idx_from_stdregn)
    elif era == "mid":
        ridx = num(hh["gvtregn"]).astype("int64").map(region_idx_from_gvtregno)
    else:
        # late (2008-2021) and current (2022+): GVTREGN here holds GSS area codes
        # (e.g. 112000008), not the 1-13 region index — that lives in GVTREGNO.
        ridx = num(hh["gvtregno"]).astype("int64").map(region_idx_from_gvtregno)
    ct = num(hh["ctannual"])
    hh = hh.assign(
        _weight=weight,
        _region_idx=ridx,
        _region=ridx.map(REGION_NAMES),
        _rent_weekly=pos(hh["hhrent"]),
        _ct_annual=ct.where(ct > 0.0, 1800.0),
        _subrent_weekly=pos(hh["subrent"]),
        _cvpay_weekly=pos(hh["cvpay"]),
        _num_bedrooms=num(hh["bedroom6"]).clip(lower=0).astype("int64"),
        _tenure=num(hh["tentyp2"]).astype("int64").map(tenure_rf_code),
        _accom=num(hh["typeacc"]).astype("int64").map(accom_rf_code),
    )
    hh = hh.reset_index(drop=True)
    hh["_hh_idx"] = hh.index
    hh_idx_by_sernum = dict(zip(hh["sernum"], hh["_hh_idx"]))
    hh_property = dict(zip(hh["sernum"], hh["_subrent_weekly"] + hh["_cvpay_weekly"]))
    hh_region_scotland = dict(zip(hh["sernum"], hh["_region_idx"] == 10))

    # Per-sernum weekly housing benefit from renter table (pre-2013 only).
    if renter is not None:
        rr = renter.copy()
        rr["sernum"] = num(rr["sernum"]).astype("int64")
        hh_housing_benefit = rr.groupby("sernum")["hbenamt"].apply(lambda s: pos(s).sum()).to_dict()
    else:
        hh_housing_benefit = {}

    # ── Benunits (table order, only those with valid sernum) ──
    bu = benunit.copy()
    bu["sernum"] = num(bu["sernum"]).astype("int64")
    bu["benunit"] = num(bu["benunit"]).astype("int64")
    bu = bu[bu["sernum"].isin(hh_idx_by_sernum)].reset_index(drop=True)
    bu["_bu_idx"] = bu.index
    claims_uc = (pos(bu["buuc"]) > 0.0) if era == "current" else pd.Series(False, index=bu.index)
    bu = bu.assign(
        _hh_idx=bu["sernum"].map(hh_idx_by_sernum),
        _on_uc=claims_uc,
        _rent_monthly=pos(bu["burent"]) * WEEKS_IN_YEAR / 12.0,
        _fsm=num(bu["fsmbu"]).clip(lower=0.0) * WEEKS_IN_YEAR,
        _fsfv=num(bu["fsfvbu"]).clip(lower=0.0) * WEEKS_IN_YEAR,
        _fsmlk=num(bu["fsmlkbu"]).clip(lower=0.0) * WEEKS_IN_YEAR,
        _heart=num(bu["heartbu"]).clip(lower=0.0) * WEEKS_IN_YEAR,
        _tvlic=num(bu["butvlic"]).clip(lower=0.0) * WEEKS_IN_YEAR,
    )
    bu_idx_by_key = {(s, b): i for s, b, i in zip(bu["sernum"], bu["benunit"], bu["_bu_idx"])}

    # ── Persons: adults then children, assembled in that order ──
    persons = build_persons(adult, child, era, acc_agg, ben_agg, job_agg, pen_agg, pp_agg, oj_agg,
                            hh_property, hh_region_scotland, hh_idx_by_sernum, bu_idx_by_key,
                            hh_housing_benefit)

    write_csvs(out_dir, hh, bu, persons)


def _getcol(agg: pd.DataFrame | None, key: pd.Series, col: str) -> pd.Series:
    if agg is None or col not in agg.columns:
        return pd.Series(0.0, index=key.index)
    return key.map(agg[col]).fillna(0.0)


def build_persons(adult, child, era, acc_agg, ben_agg, job_agg, pen_agg, pp_agg, oj_agg,
                  hh_property, hh_region_scotland, hh_idx_by_sernum, bu_idx_by_key,
                  hh_housing_benefit):
    # ----- adults -----
    a = adult.copy()
    a["sernum"] = num(a["sernum"]).astype("int64")
    a["person"] = num(a["person"]).astype("int64")
    a["benunit"] = num(a["benunit"]).astype("int64")
    a = add_key(a)
    key = a["_key"]

    # benefit-derived series
    b = pd.DataFrame(index=a.index)
    for col in BENEFIT_COLS:
        b[col] = _getcol(ben_agg, key, col)
    dis = disability_flags(b)

    sex = num(a["sex"]).astype("int64")
    hours = num(a["tothours"]).clip(lower=0.0)

    limitill = pd.Series(False, index=a.index) if era == "early" else (num(a["limitill"]).astype("int64") == 1)
    esa_group = pd.Series(0, index=a.index) if era in ("early", "mid") else num(a["esagrp"]).astype("int64")
    emp_status = num(a["empstatb"]).astype("int64")
    looking = num(a["lookwk"]).astype("int64") == 1
    self_carer = pd.Series(False, index=a.index) if era in ("early", "mid") else (num(a["carer1"]).astype("int64") == 1)

    if era == "early":
        is_hrp = (num(a["uperson"]).astype("int64") == 1) & (a["benunit"] == 1)
    else:
        is_hrp = num(a["hrpid"]).astype("int64") == 1

    royyr1 = pos(a["royyr1"])
    hh_prop = a["sernum"].map(hh_property).fillna(0.0)
    property_income = royyr1 + hh_prop.where(is_hrp, 0.0)

    # Pre-2013 household-level housing benefit, assigned to the HRP.
    hh_hb = a["sernum"].map(hh_housing_benefit).fillna(0.0)
    renter_hb = hh_hb.where(is_hrp, 0.0)

    mntus1 = num(a["mntus1"]).astype("int64")
    mntus2 = num(a["mntus2"]).astype("int64")
    m1 = pos(a["mntusam1"]).where(mntus1 == 2, pos(a["mntamt1"]))
    m2 = pos(a["mntusam2"]).where(mntus2 == 2, pos(a["mntamt2"]))
    maintenance = m1 + m2

    allow1 = num(a["allow1"]).astype("int64") == 1
    allow2 = num(a["allow2"]).astype("int64") == 1
    allow3 = num(a["allow3"]).astype("int64") == 1
    allow4 = num(a["allow4"]).astype("int64") == 1
    yptot = (
        pos(a["allpay1"]).where(allow1, 0.0)
        + pos(a["allpay2"]).where(allow2, 0.0)
        + pos(a["allpay3"]).where(allow3, 0.0)
        + pos(a["allpay4"]).where(allow4, 0.0)
        + pos(a["apamt"]) + pos(a["apdamt"]) + pos(a["pareamt"]) + pos(a["aliamt"])
    )
    misc = _getcol(oj_agg, key, "oddjob_weekly") + yptot

    age = num(a["age"]) if era == "early" else num(a["age80"])
    se = pos(a["seincam2"])
    se = se.where(se > 0.0, pos(a["inseinc"]))
    priv_pen = _getcol(pen_agg, key, "private_pension_weekly")
    priv_pen = priv_pen.where((key.isin(pen_agg.index) if pen_agg is not None else False), pos(a["inpeninc"]))

    adf = pd.DataFrame({
        "sernum": a["sernum"], "benunit": a["benunit"], "person": a["person"],
        "age": age,
        "gender": np.where(sex == 1, "male", "female"),
        "is_benunit_head": num(a["uperson"]).astype("int64") == 1,
        "is_household_head": is_hrp,
        "employment_income_w": pos(a["inearns"]),
        "self_employment_income_w": se,
        "private_pension_income_w": priv_pen,
        "state_pension_w": b["state_pension"],
        "savings_interest_w": _getcol(acc_agg, key, "savings_interest_weekly"),
        "dividend_income_w": _getcol(acc_agg, key, "dividend_income_weekly") + pos(a["dividgro"]),
        "property_income_w": property_income,
        "maintenance_income_w": maintenance,
        "miscellaneous_income_w": misc,
        "hours_worked_w": hours,
        "is_carer": b["carers_allowance"] > 0.0,
        "limitill": limitill, "esa_group": esa_group, "emp_status": emp_status,
        "looking_for_work": looking, "is_self_identified_carer": self_carer,
        "employee_pension_contributions_w": _getcol(job_agg, key, "employee_pension_contributions_weekly"),
        "personal_pension_contributions_w": _getcol(pp_agg, key, "personal_pension_contributions_weekly"),
        "childcare_expenses_w": 0.0,
        "child_benefit_w": b["child_benefit"], "housing_benefit_w": b["housing_benefit"] + renter_hb,
        "income_support_w": b["income_support"], "pension_credit_w": b["pension_credit"],
        "child_tax_credit_w": b["child_tax_credit"], "working_tax_credit_w": b["working_tax_credit"],
        "universal_credit_w": b["universal_credit"],
        "dla_care_w": b["dla_sc"], "dla_mobility_w": b["dla_m"],
        "pip_daily_living_w": b["pip_dl"], "pip_mobility_w": b["pip_m"],
        "carers_allowance_w": b["carers_allowance"], "attendance_allowance_w": b["attendance_allowance"],
        "esa_income_w": b["esa_income"], "esa_contributory_w": b["esa_contrib"],
        "jsa_income_w": b["jsa_income"], "jsa_contributory_w": b["jsa_contrib"],
        "other_benefits_w": (b["bereavement"] + b["maternity_allowance"] + b["winter_fuel"]
                             + b["industrial_injuries"] + b["sda"] + b["war_pension"] + b["other_ni_state"]),
        "adp_daily_living_w": b["adp_dl"], "adp_mobility_w": b["adp_m"],
        "cdp_care_w": b["cdp_care"], "cdp_mobility_w": b["cdp_mob"],
        "is_child": False,
    })
    for c in dis.columns:
        adf[c] = dis[c].values

    # ----- children -----
    c = child.copy()
    c["sernum"] = num(c["sernum"]).astype("int64")
    c["person"] = num(c["person"]).astype("int64")
    c["benunit"] = num(c["benunit"]).astype("int64")
    csex = num(c["sex"]).astype("int64")
    cdf = pd.DataFrame({
        "sernum": c["sernum"], "benunit": c["benunit"], "person": c["person"],
        "age": num(c["age"]),
        "gender": np.where(csex == 1, "male", "female"),
        "is_benunit_head": False, "is_household_head": False,
        "employment_income_w": num(c["chearns"]).clip(lower=0.0),
        "self_employment_income_w": 0.0, "private_pension_income_w": 0.0,
        "state_pension_w": 0.0, "savings_interest_w": 0.0, "dividend_income_w": 0.0,
        "property_income_w": 0.0, "maintenance_income_w": 0.0,
        "miscellaneous_income_w": num(c["chrinc"]).clip(lower=0.0),
        "hours_worked_w": 0.0, "is_carer": False,
        "limitill": False, "esa_group": 0, "emp_status": 0,
        "looking_for_work": False, "is_self_identified_carer": False,
        "employee_pension_contributions_w": 0.0, "personal_pension_contributions_w": 0.0,
        "childcare_expenses_w": 0.0,
        "child_benefit_w": 0.0, "housing_benefit_w": 0.0, "income_support_w": 0.0,
        "pension_credit_w": 0.0, "child_tax_credit_w": 0.0, "working_tax_credit_w": 0.0,
        "universal_credit_w": 0.0, "dla_care_w": 0.0, "dla_mobility_w": 0.0,
        "pip_daily_living_w": 0.0, "pip_mobility_w": 0.0, "carers_allowance_w": 0.0,
        "attendance_allowance_w": 0.0, "esa_income_w": 0.0, "esa_contributory_w": 0.0,
        "jsa_income_w": 0.0, "jsa_contributory_w": 0.0, "other_benefits_w": 0.0,
        "adp_daily_living_w": 0.0, "adp_mobility_w": 0.0, "cdp_care_w": 0.0, "cdp_mobility_w": 0.0,
        "is_child": True,
    })
    for col in dis.columns:
        cdf[col] = False

    allp = pd.concat([adf, cdf], ignore_index=True)
    # Keep only persons whose (sernum) is a known household and (sernum,benunit) a known benunit.
    allp = allp[allp["sernum"].isin(hh_idx_by_sernum)].copy()
    allp["_bukey"] = list(zip(allp["sernum"], allp["benunit"]))
    allp = allp[allp["_bukey"].isin(bu_idx_by_key)].reset_index(drop=True)
    allp["person_id"] = allp.index
    allp["household_id"] = allp["sernum"].map(hh_idx_by_sernum)
    allp["benunit_id"] = allp["_bukey"].map(bu_idx_by_key)
    allp["is_in_scotland"] = allp["sernum"].map(hh_region_scotland).fillna(False)
    return allp


# ── CSV writers (match Rust column order & formatting) ───────────────────────

PERSON_COLS = [
    "person_id", "benunit_id", "household_id",
    "age", "gender", "is_benunit_head", "is_household_head",
    "employment_income", "self_employment_income",
    "private_pension_income", "state_pension",
    "savings_interest", "dividend_income", "capital_gains",
    "capital_gains_residential_share",
    "property_income", "maintenance_income",
    "miscellaneous_income", "other_income",
    "is_in_scotland", "hours_worked_annual",
    "dla_care_low", "dla_care_mid", "dla_care_high",
    "dla_mob_low", "dla_mob_high",
    "pip_dl_std", "pip_dl_enh",
    "pip_mob_std", "pip_mob_enh",
    "aa_low", "aa_high",
    "is_disabled", "is_enhanced_disabled", "is_severely_disabled", "is_carer",
    "limitill", "esa_group", "emp_status", "looking_for_work",
    "is_self_identified_carer",
    "employee_pension_contributions", "personal_pension_contributions",
    "childcare_expenses",
    "child_benefit", "housing_benefit",
    "income_support", "pension_credit",
    "child_tax_credit", "working_tax_credit",
    "universal_credit",
    "dla_care", "dla_mobility",
    "pip_daily_living", "pip_mobility",
    "carers_allowance", "attendance_allowance",
    "esa_income", "esa_contributory",
    "jsa_income", "jsa_contributory",
    "other_benefits",
    "adp_daily_living", "adp_mobility",
    "cdp_care", "cdp_mobility",
]

HH_EXTRA_ZERO_COLS = [
    "owned_land", "property_wealth", "corporate_wealth",
    "gross_financial_wealth", "net_financial_wealth",
    "main_residence_value", "other_residential_property_value",
    "non_residential_property_value", "savings", "num_vehicles",
    "food_consumption", "alcohol_consumption", "tobacco_consumption", "clothing_consumption",
    "housing_water_electricity_consumption", "furnishings_consumption",
    "health_consumption", "transport_consumption", "communication_consumption",
    "recreation_consumption", "education_consumption", "restaurants_consumption",
    "miscellaneous_consumption", "petrol_spending", "diesel_spending",
    "domestic_energy_consumption", "electricity_consumption", "gas_consumption",
]


def write_csvs(out_dir: Path, hh: pd.DataFrame, bu: pd.DataFrame, p: pd.DataFrame) -> None:
    # ----- persons.csv -----
    out = pd.DataFrame()
    out["person_id"] = p["person_id"]
    out["benunit_id"] = p["benunit_id"]
    out["household_id"] = p["household_id"]
    out["age"] = num(p["age"]).map(lambda v: f"{v:.0f}")
    out["gender"] = p["gender"]
    out["is_benunit_head"] = b2s(p["is_benunit_head"])
    out["is_household_head"] = b2s(p["is_household_head"])
    out["employment_income"] = f2(p["employment_income_w"] * WEEKS_IN_YEAR)
    out["self_employment_income"] = f2((p["self_employment_income_w"] * WEEKS_IN_YEAR).clip(lower=0.0))
    out["private_pension_income"] = f2(p["private_pension_income_w"] * WEEKS_IN_YEAR)
    out["state_pension"] = f2(p["state_pension_w"] * WEEKS_IN_YEAR)
    out["savings_interest"] = f2(p["savings_interest_w"] * WEEKS_IN_YEAR)
    out["dividend_income"] = f2(p["dividend_income_w"] * WEEKS_IN_YEAR)
    out["capital_gains"] = f2(pd.Series(0.0, index=p.index))
    out["capital_gains_residential_share"] = num(pd.Series(0.0, index=p.index)).map(lambda v: f"{v:.4f}")
    out["property_income"] = f2(p["property_income_w"] * WEEKS_IN_YEAR)
    out["maintenance_income"] = f2(p["maintenance_income_w"] * WEEKS_IN_YEAR)
    out["miscellaneous_income"] = f2(p["miscellaneous_income_w"] * WEEKS_IN_YEAR)
    out["other_income"] = f2(pd.Series(0.0, index=p.index))
    out["is_in_scotland"] = b2s(p["is_in_scotland"])
    out["hours_worked_annual"] = num(p["hours_worked_w"] * 52.0).map(lambda v: f"{v:.1f}")
    for c in ["dla_care_low", "dla_care_mid", "dla_care_high", "dla_mob_low", "dla_mob_high",
              "pip_dl_std", "pip_dl_enh", "pip_mob_std", "pip_mob_enh", "aa_low", "aa_high",
              "is_disabled", "is_enhanced_disabled", "is_severely_disabled", "is_carer"]:
        out[c] = b2s(p[c])
    out["limitill"] = b2s(p["limitill"])
    out["esa_group"] = num(p["esa_group"]).astype("int64").astype(str)
    out["emp_status"] = num(p["emp_status"]).astype("int64").astype(str)
    out["looking_for_work"] = b2s(p["looking_for_work"])
    out["is_self_identified_carer"] = b2s(p["is_self_identified_carer"])
    out["employee_pension_contributions"] = f2(p["employee_pension_contributions_w"] * WEEKS_IN_YEAR)
    out["personal_pension_contributions"] = f2(p["personal_pension_contributions_w"] * WEEKS_IN_YEAR)
    out["childcare_expenses"] = f2(p["childcare_expenses_w"] * WEEKS_IN_YEAR)
    for src, dst in [
        ("child_benefit_w", "child_benefit"), ("housing_benefit_w", "housing_benefit"),
        ("income_support_w", "income_support"), ("pension_credit_w", "pension_credit"),
        ("child_tax_credit_w", "child_tax_credit"), ("working_tax_credit_w", "working_tax_credit"),
        ("universal_credit_w", "universal_credit"),
        ("dla_care_w", "dla_care"), ("dla_mobility_w", "dla_mobility"),
        ("pip_daily_living_w", "pip_daily_living"), ("pip_mobility_w", "pip_mobility"),
        ("carers_allowance_w", "carers_allowance"), ("attendance_allowance_w", "attendance_allowance"),
        ("esa_income_w", "esa_income"), ("esa_contributory_w", "esa_contributory"),
        ("jsa_income_w", "jsa_income"), ("jsa_contributory_w", "jsa_contributory"),
        ("other_benefits_w", "other_benefits"),
        ("adp_daily_living_w", "adp_daily_living"), ("adp_mobility_w", "adp_mobility"),
        ("cdp_care_w", "cdp_care"), ("cdp_mobility_w", "cdp_mobility"),
    ]:
        out[dst] = f2(p[src] * WEEKS_IN_YEAR)
    out = out[PERSON_COLS]
    out.to_csv(out_dir / "persons.csv", index=False)

    # ----- benunits.csv -----
    pid_by_bu = p.groupby("benunit_id")["person_id"].apply(lambda s: ";".join(map(str, s)))
    bdf = pd.DataFrame()
    bdf["benunit_id"] = bu["_bu_idx"]
    bdf["household_id"] = bu["_hh_idx"]
    bdf["person_ids"] = bu["_bu_idx"].map(pid_by_bu).fillna("")
    # on_uc: from benunit flag OR any member with universal_credit > 0
    uc_members = p.groupby("benunit_id")["universal_credit_w"].max()
    on_uc = bu["_on_uc"] | (bu["_bu_idx"].map(uc_members).fillna(0.0) > 0.0)
    bdf["on_uc"] = b2s(on_uc)
    bdf["rent_monthly"] = f2(bu["_rent_monthly"])
    # is_lone_parent: 1 adult (age>=18) + >=1 child (age<18) in the benunit,
    # matching Rust Person::is_adult/is_child (age-based, not source-table split)
    p_age = num(p["age"])
    adults_by_bu = p[p_age >= 18.0].groupby("benunit_id")["person_id"].count()
    children_by_bu = p[p_age < 18.0].groupby("benunit_id")["person_id"].count()
    na = bu["_bu_idx"].map(adults_by_bu).fillna(0)
    nc = bu["_bu_idx"].map(children_by_bu).fillna(0)
    bdf["is_lone_parent"] = b2s((na == 1) & (nc > 0))
    bdf.to_csv(out_dir / "benunits.csv", index=False)

    # ----- households.csv -----
    bu_by_hh = bu.groupby("_hh_idx")["_bu_idx"].apply(lambda s: ";".join(map(str, s)))
    pid_by_hh = p.groupby("household_id")["person_id"].apply(lambda s: ";".join(map(str, s)))
    hdf = pd.DataFrame()
    hdf["household_id"] = hh["_hh_idx"]
    hdf["benunit_ids"] = hh["_hh_idx"].map(bu_by_hh).fillna("")
    hdf["person_ids"] = hh["_hh_idx"].map(pid_by_hh).fillna("")
    hdf["weight"] = num(hh["_weight"]).map(lambda v: f"{v:.4f}")
    hdf["region"] = hh["_region"]
    hdf["rent_annual"] = f2(hh["_rent_weekly"] * WEEKS_IN_YEAR)
    hdf["council_tax_annual"] = f2(hh["_ct_annual"])
    hdf["num_bedrooms"] = num(hh["_num_bedrooms"]).astype("int64").astype(str)
    hdf["tenure_type"] = num(hh["_tenure"]).astype("int64").astype(str)
    hdf["accommodation_type"] = num(hh["_accom"]).astype("int64").astype(str)
    for c in HH_EXTRA_ZERO_COLS:
        hdf[c] = "0.00"
    hdf.to_csv(out_dir / "households.csv", index=False)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--raw-dir", type=Path, required=True)
    ap.add_argument("--year", type=int, required=True)
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args()
    extract(args.raw_dir, args.year, args.out)
    print(f"wrote clean CSVs to {args.out}")


if __name__ == "__main__":
    main()
