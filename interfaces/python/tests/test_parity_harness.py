"""Hermetic tests for the parity harness in ``scripts/parity.py``.

These do NOT load the FRS and do NOT hit the network. They inject small fake
stat dictionaries / Series-like values and assert the comparison logic:

* fails (over-tolerance diff is reported) on injected divergence,
* passes when everything is within tolerance,
* raises (does NOT silently skip) when an expected column / variable is missing.
"""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
import pytest

_REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(_REPO / "scripts"))
parity = pytest.importorskip("parity")


def _stats(mean, p10, p50, p90):
    return {"mean": mean, "p10": p10, "p50": p50, "p90": p90}


def _all_within():
    """A full python/rust stat pair with identical values for every variable."""
    py, ru = {}, {}
    for var in parity.VARIABLES:
        s = _stats(100.0, 10.0, 50.0, 90.0)
        py[var.label] = dict(s)
        ru[var.label] = dict(s)
    return py, ru


# ── Weighted-statistic helpers ────────────────────────────────────────────────

class TestWeightedStats:
    def test_weighted_mean_equal_weights(self):
        v = np.array([1.0, 2.0, 3.0])
        w = np.array([1.0, 1.0, 1.0])
        assert parity.weighted_mean(v, w) == pytest.approx(2.0)

    def test_weighted_mean_unequal_weights(self):
        v = np.array([0.0, 10.0])
        w = np.array([3.0, 1.0])
        assert parity.weighted_mean(v, w) == pytest.approx(2.5)

    def test_weighted_median(self):
        v = np.array([1.0, 2.0, 3.0, 4.0])
        w = np.array([1.0, 1.0, 1.0, 1.0])
        assert parity.weighted_quantile(v, w, 0.5) == pytest.approx(2.5)

    def test_zero_total_weight_raises(self):
        with pytest.raises(ValueError):
            parity.weighted_mean(np.array([1.0]), np.array([0.0]))


# ── compare(): passes within tolerance ────────────────────────────────────────

class TestComparePasses:
    def test_identical_stats_have_no_over_tolerance(self):
        py, ru = _all_within()
        all_diffs, over = parity.compare(py, ru, tolerance=0.01)
        assert over == []
        assert len(all_diffs) == len(parity.VARIABLES) * len(parity._STATS)

    def test_small_diff_within_tolerance(self):
        py, ru = _all_within()
        # Bump one stat by 0.5% with a 1% tolerance → still OK.
        first = parity.VARIABLES[0].label
        ru[first]["mean"] = 100.5
        _, over = parity.compare(py, ru, tolerance=0.01)
        assert over == []


# ── compare(): fails on divergence ────────────────────────────────────────────

class TestCompareFails:
    def test_divergence_beyond_tolerance_is_reported(self):
        py, ru = _all_within()
        first = parity.VARIABLES[0].label
        ru[first]["mean"] = 120.0  # +20% vs python 100 → over a 1% tolerance.
        _, over = parity.compare(py, ru, tolerance=0.01)
        assert len(over) == 1
        assert over[0].label == first
        assert over[0].stat == "mean"
        assert over[0].rel == pytest.approx(0.20)

    def test_python_zero_rust_nonzero_is_infinite_divergence(self):
        py, ru = _all_within()
        first = parity.VARIABLES[0].label
        py[first]["p10"] = 0.0
        ru[first]["p10"] = 5.0
        _, over = parity.compare(py, ru, tolerance=0.01)
        assert any(d.stat == "p10" and d.rel == float("inf") for d in over)

    def test_both_zero_is_not_a_divergence(self):
        py, ru = _all_within()
        first = parity.VARIABLES[0].label
        py[first]["p10"] = 0.0
        ru[first]["p10"] = 0.0
        _, over = parity.compare(py, ru, tolerance=0.01)
        assert over == []


# ── compare(): missing data is a HARD error (not skipped) ──────────────────────

class TestCompareMissingIsHardError:
    def test_missing_variable_on_rust_raises(self):
        py, ru = _all_within()
        del ru[parity.VARIABLES[0].label]
        with pytest.raises(RuntimeError):
            parity.compare(py, ru, tolerance=0.01)

    def test_missing_variable_on_python_raises(self):
        py, ru = _all_within()
        del py[parity.VARIABLES[0].label]
        with pytest.raises(RuntimeError):
            parity.compare(py, ru, tolerance=0.01)

    def test_missing_statistic_raises(self):
        py, ru = _all_within()
        del ru[parity.VARIABLES[0].label]["p90"]
        with pytest.raises(RuntimeError):
            parity.compare(py, ru, tolerance=0.01)


# ── run_rust(): a missing expected column raises (not NaN-filled / skipped) ────

class TestRunRustMissingColumnRaises:
    def test_missing_expected_column_raises(self, monkeypatch):
        import types

        class FakeDF:
            def __init__(self, cols):
                self.columns = list(cols)
                self._w = np.array([1.0, 2.0, 3.0])

            def __getitem__(self, key):
                class _Col:
                    def __init__(self, arr):
                        self._arr = arr

                    def to_numpy(self, dtype=float):
                        return np.asarray(self._arr, dtype=dtype)

                if key not in self.columns:
                    raise KeyError(key)
                if key == "weight":
                    return _Col(self._w)
                return _Col(np.array([100.0, 200.0, 300.0]))

        class FakeMicrodata:
            # Deliberately omit baseline_total_benefits.
            households = FakeDF(["weight", "baseline_net_income", "baseline_total_tax"])

        class FakeSim:
            def __init__(self, year, dataset=None):
                pass

            def run_microdata(self):
                return FakeMicrodata()

        fake_module = types.SimpleNamespace(Simulation=FakeSim)
        monkeypatch.setitem(sys.modules, "policyengine_uk_compiled", fake_module)

        with pytest.raises(RuntimeError, match="missing expected column"):
            parity.run_rust(2025)


# ── FRS-unavailable path: parity() exits 0 loudly, never silently swallows ─────

class TestDataUnavailableSkip:
    def test_parity_returns_zero_when_frs_unavailable(self, monkeypatch, capsys):
        def _raise(year):
            raise parity.FRSUnavailable("simulated missing FRS")

        monkeypatch.setattr(parity, "run_python", _raise)
        rc = parity.parity(year=2025, tolerance=0.01)
        assert rc == 0
        out = capsys.readouterr().out
        assert "FRS data unavailable" in out
        assert "NOT a pass" in out

    def test_parity_returns_one_on_injected_divergence(self, monkeypatch):
        py, ru = _all_within()
        ru[parity.VARIABLES[0].label]["mean"] = 200.0  # +100% over tolerance.
        monkeypatch.setattr(parity, "run_python", lambda year: py)
        monkeypatch.setattr(parity, "run_rust", lambda year: ru)
        rc = parity.parity(year=2025, tolerance=0.01)
        assert rc == 1

    def test_parity_returns_zero_when_all_within(self, monkeypatch):
        py, ru = _all_within()
        monkeypatch.setattr(parity, "run_python", lambda year: py)
        monkeypatch.setattr(parity, "run_rust", lambda year: ru)
        rc = parity.parity(year=2025, tolerance=0.01)
        assert rc == 0
