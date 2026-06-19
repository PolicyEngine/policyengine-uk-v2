"""Auto-download microdata from private GCS bucket using HMAC credentials."""

from __future__ import annotations

import base64
import hashlib
import hmac
import os
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

GCS_BUCKET = "policyengine-uk-microdata"
GCS_HOST = "storage.googleapis.com"
ENV_TOKEN = "POLICYENGINE_UK_DATA_TOKEN"
LOCAL_CACHE = Path.home() / ".policyengine-uk-data"


def _sign_request(method: str, path: str, access_key: str, secret_key: str) -> dict:
    """Sign a GCS XML API request using HMAC-SHA1."""
    date = datetime.now(timezone.utc).strftime("%a, %d %b %Y %H:%M:%S GMT")
    string_to_sign = f"{method}\n\n\n{date}\n/{GCS_BUCKET}{path}"
    signature = hmac.new(
        secret_key.encode(), string_to_sign.encode(), hashlib.sha1
    ).digest()
    sig_b64 = base64.b64encode(signature).decode()
    return {
        "Date": date,
        "Authorization": f"GOOG1 {access_key}:{sig_b64}",
    }


def _download_object(key: str, dest: Path, access_key: str, secret_key: str):
    """Download a single object from the bucket."""
    path = f"/{key}"
    headers = _sign_request("GET", path, access_key, secret_key)
    url = f"https://{GCS_HOST}/{GCS_BUCKET}{path}"
    req = urllib.request.Request(url, headers=headers)
    dest.parent.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(req, timeout=300) as resp:
        expected = int(resp.headers.get("Content-Length", 0))
        written = 0
        with open(dest, "wb") as f:
            while True:
                chunk = resp.read(1 << 20)
                if not chunk:
                    break
                f.write(chunk)
                written += len(chunk)
        if expected and written != expected:
            dest.unlink(missing_ok=True)
            raise IOError(
                f"Incomplete download for {key}: got {written} of {expected} bytes"
            )


def _get_credentials() -> tuple[str, str]:
    """Get HMAC credentials from the single token env var.

    Token format: {access_key}:{secret_key}
    """
    token = os.environ.get(ENV_TOKEN)
    if not token or ":" not in token:
        raise EnvironmentError(
            f"Set {ENV_TOKEN} to download data from gs://{GCS_BUCKET}. "
            f"Format: ACCESS_KEY:SECRET_KEY"
        )
    return token.split(":", 1)


DATASETS = ("frs", "efrs", "lcfs", "spi", "was")


def _list_available_years(dataset: str, access_key: str, secret_key: str) -> list[int]:
    """List years available on the bucket for a given dataset.

    Returns a sorted list of integer years found under the `<dataset>/` prefix.
    """
    import re
    keys = []
    marker = ""
    while True:
        path = f"/?prefix={dataset}/&marker={marker}"
        headers = _sign_request("GET", "/", access_key, secret_key)
        url = f"https://{GCS_HOST}/{GCS_BUCKET}{path}"
        req = urllib.request.Request(url, headers=headers)
        with urllib.request.urlopen(req) as resp:
            body = resp.read().decode()
        found = re.findall(r"<Key>([^<]+)</Key>", body)
        if not found:
            break
        keys.extend(found)
        if "<IsTruncated>true</IsTruncated>" not in body:
            break
        marker = found[-1]

    years = set()
    year_re = re.compile(rf"^{dataset}/(\d{{4}})/")
    for key in keys:
        m = year_re.match(key)
        if m:
            years.add(int(m.group(1)))
    return sorted(years)


def _pick_nearest_year(available: list[int], requested: int) -> int:
    """Pick the nearest year to requested from available.

    Prefers the latest year ≤ requested (so uprating moves forward), falling
    back to the earliest available year if none is ≤ requested.
    """
    if not available:
        raise FileNotFoundError("No years available on bucket")
    candidates = [y for y in available if y <= requested]
    if candidates:
        return max(candidates)
    return min(available)


def ensure_dataset_year(dataset: str, year: int) -> Path:
    """Ensure clean CSVs for a dataset/year are available locally, downloading if needed.

    If the requested year isn't on the bucket, downloads the nearest available
    year and returns its directory. The Rust engine handles uprating from the
    downloaded year to the requested year at run time.

    Returns the path to the year directory actually downloaded (may differ from
    the requested year).
    """
    year_dir = LOCAL_CACHE / dataset / str(year)
    expected_files = ["persons.csv", "benunits.csv", "households.csv"]
    if all((year_dir / f).exists() for f in expected_files):
        return year_dir

    access_key, secret_key = _get_credentials()

    # Determine which year to download. If the requested year isn't on the
    # bucket, fall back to the nearest available.
    available = _list_available_years(dataset, access_key, secret_key)
    # If we already cached the nearest year locally, use that.
    if available:
        download_year = _pick_nearest_year(available, year)
        if download_year != year:
            near_dir = LOCAL_CACHE / dataset / str(download_year)
            if all((near_dir / f).exists() for f in expected_files):
                return near_dir
            year_dir = near_dir
    else:
        download_year = year

    year_dir.mkdir(parents=True, exist_ok=True)
    for f in expected_files:
        key = f"{dataset}/{download_year}/{f}"
        dest = year_dir / f
        if dest.exists():
            continue
        print(f"  Downloading {key}...", end="", flush=True)
        _download_object(key, dest, access_key, secret_key)
        print(" done")

    return year_dir


# Keep old name for backwards compatibility
def ensure_year(year: int) -> Path:
    return ensure_dataset_year("frs", year)


def ensure_frs(year: int, clean_frs_base: str | None = None) -> str:
    """Return a path to FRS data base dir, downloading the needed year if missing."""
    if clean_frs_base:
        year_dir = Path(clean_frs_base) / str(year)
        if year_dir.is_dir():
            return clean_frs_base

    local_base = LOCAL_CACHE / "frs"
    year_dir = local_base / str(year)
    expected = ["persons.csv", "benunits.csv", "households.csv"]
    if all((year_dir / f).exists() for f in expected):
        return str(local_base)

    # Exact year missing locally — fall back to the nearest earlier local
    # year (the engine uprates forward at runtime), matching the bucket
    # fallback behaviour.
    if local_base.is_dir():
        local_years = sorted(
            (int(p.name) for p in local_base.iterdir() if p.name.isdigit()),
            reverse=True,
        )
        for y in local_years:
            if y <= year and all(
                (local_base / str(y) / f).exists() for f in expected
            ):
                return str(local_base)

    if not os.environ.get(ENV_TOKEN):
        raise FileNotFoundError(
            f"No FRS data found for {year}. Either pass clean_frs_base= pointing to "
            f"a directory with a {year}/ subdirectory, or set {ENV_TOKEN} to "
            f"auto-download from GCS."
        )
    ensure_dataset_year("frs", year)
    return str(local_base)


def ensure_dataset(dataset: str, year: int) -> str:
    """Return a path to a dataset base dir, downloading the needed year if missing.

    Supports: frs, lcfs, spi, was.
    """
    if dataset not in DATASETS:
        raise ValueError(f"Unknown dataset {dataset!r}. Choose from: {DATASETS}")

    local_base = LOCAL_CACHE / dataset
    year_dir = local_base / str(year)
    expected = ["persons.csv", "benunits.csv", "households.csv"]
    if all((year_dir / f).exists() for f in expected):
        return str(local_base)

    if not os.environ.get(ENV_TOKEN):
        raise FileNotFoundError(
            f"No {dataset.upper()} data found for {year}. Set {ENV_TOKEN} to auto-download."
        )
    ensure_dataset_year(dataset, year)
    return str(local_base)


def capabilities() -> dict:
    """Return a structured description of engine capabilities for LLM consumption.

    Does not require authentication — reports only what is locally cached
    plus static knowledge about the engine. Returns a plain dict suitable
    for JSON serialisation.
    """
    # Locally cached years per dataset
    dataset_years: dict[str, list[int]] = {}
    for ds in DATASETS:
        ds_dir = LOCAL_CACHE / ds
        if ds_dir.is_dir():
            years = sorted(
                int(p.name) for p in ds_dir.iterdir()
                if p.is_dir() and p.name.isdigit()
            )
            if years:
                dataset_years[ds] = years

    dataset_descriptions = {
        "efrs": (
            "Enhanced Family Resources Survey. Merges FRS household microdata with "
            "Wealth and Assets Survey (wealth) and Living Costs and Food Survey "
            "(expenditure). Full tax-benefit model. Available from 1994 to 2029. "
            "Use when wealth or expenditure data is needed (e.g. wealth tax, VAT)."
        ),
        "frs": (
            "Family Resources Survey. Full tax-benefit model, ~20,000 households. "
            "Available from 1994 to 2024. Default dataset for distributional and "
            "historical analysis."
        ),
        "spi": (
            "Survey of Personal Incomes (HMRC administrative data). Person-level only — "
            "no household or benefit calculations. Far better coverage of very high earners "
            "(top 1–5%). Use when the question is specifically about high-income taxpayers "
            "or income tax/NI only."
        ),
        "was": (
            "Wealth and Assets Survey. Authoritative source for wealth distribution. "
            "Use for wealth tax, inheritance, or asset-based analysis."
        ),
        "lcfs": (
            "Living Costs and Food Survey. Expenditure and consumption data. "
            "Use for VAT, duties, or consumption-based tax analysis."
        ),
    }

    return {
        "engine": "PolicyEngine UK compiled microsimulation engine",
        "fiscal_years_supported": "1994–2029 (year=2025 means 2025/26 fiscal year)",
        "multi_year_analysis": (
            "Fully supported. Call tools once per year and collate results. "
            "Never refuse a multi-year or trend question — just loop over years."
        ),
        "datasets": {
            ds: {
                "description": dataset_descriptions.get(ds, ""),
                "locally_cached_years": dataset_years.get(ds, []),
            }
            for ds in DATASETS
        },
        "default_dataset": "frs",
        "programmes_modelled": [
            "Income tax", "National Insurance (employee and employer)",
            "Universal Credit", "Child Benefit", "State Pension",
            "Pension Credit", "Housing Benefit", "Tax Credits (CTC/WTC)",
            "Scottish Child Payment", "Benefit Cap", "Stamp Duty",
            "Capital Gains Tax", "Wealth Tax (parametric)",
        ],
        "microdata_columns_available": {
            "persons": [
                "age", "gender", "employment_income", "self_employment_income",
                "pension_income", "capital_gains", "savings_interest",
                "baseline_income_tax", "reform_income_tax",
                "baseline_employee_ni", "reform_employee_ni",
                "baseline_total_income", "reform_total_income",
                "weight", "region", "is_household_head", "is_benunit_head",
                "household_id", "benunit_id",
            ],
            "benunits": [
                "baseline_universal_credit", "reform_universal_credit",
                "baseline_child_benefit", "reform_child_benefit",
                "baseline_housing_benefit", "reform_housing_benefit",
                "baseline_child_tax_credit", "reform_child_tax_credit",
                "baseline_working_tax_credit", "reform_working_tax_credit",
                "baseline_pension_credit", "reform_pension_credit",
                "baseline_total_benefits", "reform_total_benefits",
                "weight", "household_id",
            ],
            "households": [
                "baseline_net_income", "reform_net_income",
                "baseline_total_tax", "reform_total_tax",
                "baseline_total_benefits", "reform_total_benefits",
                "baseline_gross_income", "rent", "council_tax",
                "main_residence_value", "region", "weight",
                "household_id",
            ],
        },
        "notes": [
            "Rent is an input field on households (rent_monthly). "
            "The FRS records actual rent paid, so rent burden (rent/income) "
            "can be computed directly from microdata across any year 1994–2026.",
            "Poverty and HBAI fields (relative/absolute poverty rates, mean/median "
            "equivalised income) are only available from run_economy_simulation, "
            "not from analyse_microdata.",
            "EFRS is only available from 2023. For earlier years use FRS.",
        ],
    }


def download_all(force: bool = False, datasets: tuple = DATASETS) -> None:
    """Download all available years for the given datasets (default: all)."""
    import re
    access_key, secret_key = _get_credentials()

    for dataset in datasets:
        keys = []
        marker = ""
        while True:
            path = f"/?prefix={dataset}/&marker={marker}"
            headers = _sign_request("GET", "/", access_key, secret_key)
            url = f"https://{GCS_HOST}/{GCS_BUCKET}{path}"
            req = urllib.request.Request(url, headers=headers)
            with urllib.request.urlopen(req) as resp:
                body = resp.read().decode()
            found = re.findall(r"<Key>([^<]+)</Key>", body)
            if not found:
                break
            keys.extend(found)
            if "<IsTruncated>true</IsTruncated>" not in body:
                break
            marker = found[-1]

        total = len(keys)
        for i, key in enumerate(keys, 1):
            rel = key[len(f"{dataset}/"):]
            if not rel:
                continue
            dest = LOCAL_CACHE / dataset / rel
            if dest.exists() and not force:
                continue
            _download_object(key, dest, access_key, secret_key)
            print(f"\r  Downloading {dataset}: {i}/{total}", end="", flush=True)
        if keys:
            print()
