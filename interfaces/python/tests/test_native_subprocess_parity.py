"""Parity between the in-process native ``run_microdata`` and the subprocess path.

``run_microdata`` has two implementations behind one API: the in-process PyO3
native engine (fast path) and the CSV-over-stdout subprocess (fallback). They
share the same column-table source in Rust, so their per-entity output must be
identical. This test runs both on the same FRS year and asserts equality.

Skips (does not fail) when FRS data is unavailable locally and no download
token is set — the same loud-skip contract as the python/rust parity harness.
"""

from __future__ import annotations

import numpy as np
import pytest

from policyengine_uk_compiled import Parameters, Simulation, engine
from policyengine_uk_compiled.models import IncomeTaxParams

YEAR = 2025


def _native_available() -> bool:
    return engine._native is not None


def _frs_available() -> bool:
    try:
        from policyengine_uk_compiled.data import ensure_frs

        ensure_frs(YEAR)
        return True
    except Exception:
        return False


pytestmark = [
    pytest.mark.skipif(not _native_available(), reason="native extension not built"),
    pytest.mark.skipif(not _frs_available(), reason="FRS data unavailable"),
]


def _run_native(policy):
    sim = Simulation(year=YEAR)
    assert sim._native_sim is None
    return sim.run_microdata(policy=policy)


def _run_subprocess(policy, monkeypatch):
    # Disable the native module so run_microdata falls through to the subprocess.
    monkeypatch.setattr(engine, "_native", None)
    sim = Simulation(year=YEAR)
    return sim.run_microdata(policy=policy)


def _infer_decimals(arr, max_dp=6):
    """Smallest dp (0..max_dp) at which rounding ``arr`` is a no-op.

    The subprocess values come from CSV with a fixed per-column precision, so
    this recovers that precision to compare the native (full-precision) column
    on equal terms.
    """
    finite = arr[np.isfinite(arr)]
    for dp in range(max_dp + 1):
        if np.allclose(np.round(finite, dp), finite, rtol=0, atol=0):
            return dp
    return max_dp


def _assert_frames_equal(native_df, sub_df, label):
    common = sorted(set(native_df.columns) & set(sub_df.columns))
    assert common, f"{label}: no shared columns"
    assert len(native_df) == len(sub_df), f"{label}: row count differs"
    for col in common:
        n = native_df[col].to_numpy()
        s = sub_df[col].to_numpy()
        # The native path types booleans as real bools; the CSV path yields
        # 1/0 integers for the same column. Normalise to int so the same value
        # compares equal across dtypes.
        if np.issubdtype(n.dtype, np.bool_):
            n = n.astype(np.int64)
        if np.issubdtype(s.dtype, np.bool_):
            s = s.astype(np.int64)
        if np.issubdtype(n.dtype, np.number) and np.issubdtype(s.dtype, np.number):
            # The subprocess path round-trips through CSV, where float columns
            # are written to a fixed number of decimal places; the native path
            # keeps full float64 precision. Compare at the subprocess's own
            # precision, tolerating one unit in the last place — numpy's
            # round-half-to-even and Rust's CSV formatting disagree at the
            # half-boundary (e.g. x.xx5), a display artefact, not a divergence.
            dp = _infer_decimals(s)
            np.testing.assert_allclose(
                n, s, rtol=0, atol=10 ** (-dp) + 1e-9,
                err_msg=f"{label}.{col}",
            )
        else:
            assert list(map(str, n)) == list(map(str, s)), f"{label}.{col}"


@pytest.mark.parametrize(
    "policy",
    [
        None,
        Parameters(income_tax=IncomeTaxParams(personal_allowance=20_000)),
    ],
    ids=["baseline", "reform"],
)
def test_native_matches_subprocess(policy, monkeypatch):
    native = _run_native(policy)
    sub = _run_subprocess(policy, monkeypatch)

    _assert_frames_equal(native.persons, sub.persons, "persons")
    _assert_frames_equal(native.benunits, sub.benunits, "benunits")
    _assert_frames_equal(native.households, sub.households, "households")
