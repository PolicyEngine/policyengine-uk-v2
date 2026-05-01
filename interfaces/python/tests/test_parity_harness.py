"""Tests for the parity harness in `scripts/parity.py`.

These cover the pure-Python pieces (period substitution, scenario builder,
diff computation) without invoking either engine, plus an end-to-end smoke
test that runs the harness through the Rust binary if it is available.
"""

from __future__ import annotations

import math
import subprocess
import sys
from pathlib import Path

import pytest

_REPO = Path(__file__).resolve().parents[3]
_SCRIPT = _REPO / "scripts" / "parity.py"

# Make `import parity` work — the script is a top-level module, not a package.
sys.path.insert(0, str(_REPO / "scripts"))
parity = pytest.importorskip("parity")


# ── _replace_period ──────────────────────────────────────────────────────────

class TestReplacePeriod:
    def test_replaces_year_keys(self):
        out = parity._replace_period({"YEAR": 50_000}, year=2025)
        assert out == {"2025": 50_000}

    def test_recurses_into_nested_dicts(self):
        out = parity._replace_period(
            {"people": {"p": {"age": {"YEAR": 30}}}}, year=2024
        )
        assert out == {"people": {"p": {"age": {"2024": 30}}}}

    def test_recurses_into_lists(self):
        out = parity._replace_period({"a": [{"YEAR": 1}, {"YEAR": 2}]}, year=2025)
        assert out == {"a": [{"2025": 1}, {"2025": 2}]}

    def test_leaves_other_keys_untouched(self):
        out = parity._replace_period({"members": ["p1"], "YEAR": 1}, year=2025)
        assert out == {"members": ["p1"], "2025": 1}


# ── Scenario list ────────────────────────────────────────────────────────────

class TestScenarios:
    def test_returns_non_empty_list(self):
        scenarios = parity._scenarios(2025)
        assert len(scenarios) > 0

    def test_each_scenario_has_required_entities(self):
        for name, situation in parity._scenarios(2025):
            assert "people" in situation, name
            assert "benunits" in situation, name
            assert "households" in situation, name
            assert situation["people"], f"{name}: empty people"

    def test_year_substituted_into_periods(self):
        scenarios = parity._scenarios(2024)
        # Pick one that has period-keyed values and check they're 2024.
        for name, situation in scenarios:
            for person_fields in situation["people"].values():
                for var, val in person_fields.items():
                    if isinstance(val, dict):
                        assert "YEAR" not in val, f"{name}/{var}: YEAR not substituted"
                        assert all(k == "2024" for k in val.keys()), f"{name}/{var}: wrong year"

    def test_scenarios_cover_diverse_household_types(self):
        names = [n for n, _ in parity._scenarios(2025)]
        joined = "|".join(names).lower()
        assert "single" in joined
        assert "couple" in joined
        assert "lone_parent" in joined
        assert "pensioner" in joined
        assert "scotland" in joined


# ── ScenarioResult diff computation ──────────────────────────────────────────

class TestScenarioResult:
    def test_no_python_means_no_diffs(self):
        sr = parity.ScenarioResult(
            name="test", rust={"income_tax": 100.0}, python=None
        )
        sr.compute_diffs()
        assert sr.diffs == {}
        assert sr.max_abs_diff == 0.0

    def test_diffs_populated_when_python_present(self):
        sr = parity.ScenarioResult(
            name="test",
            rust={"income_tax": 100.0, "child_benefit": 50.0},
            python={"income_tax": 95.0, "child_benefit": 50.0},
        )
        sr.compute_diffs()
        assert sr.diffs["income_tax"] == 5.0
        assert sr.diffs["child_benefit"] == 0.0
        assert sr.max_abs_diff == 5.0

    def test_max_abs_diff_handles_negative(self):
        sr = parity.ScenarioResult(
            name="test",
            rust={"a": 100.0, "b": 50.0},
            python={"a": 110.0, "b": 50.0},
        )
        sr.compute_diffs()
        assert sr.diffs["a"] == -10.0
        assert sr.max_abs_diff == 10.0

    def test_nan_values_dont_pollute_max(self):
        sr = parity.ScenarioResult(
            name="test",
            rust={"a": 100.0, "b": float("nan")},
            python={"a": 100.0, "b": float("nan")},
        )
        sr.compute_diffs()
        # NaN diff is NaN, but compute_diffs's max-abs guard skips it.
        assert sr.max_abs_diff == 0.0


# ── End-to-end smoke (requires the Rust binary) ──────────────────────────────

def _has_rust_binary() -> bool:
    candidates = [
        _REPO / "target" / "release" / "policyengine-uk-rust",
        _REPO / "target" / "debug"   / "policyengine-uk-rust",
    ]
    return any(c.is_file() for c in candidates)


@pytest.mark.skipif(not _has_rust_binary(), reason="Rust binary not built")
class TestEndToEnd:
    def test_run_rust_returns_expected_keys_for_one_scenario(self):
        # Use the simplest synthetic scenario manually.
        situation = {
            "people":     {"p": {"age": {"2025": 30}, "employment_income": {"2025": 50_000}}},
            "benunits":   {"b": {"members": ["p"]}},
            "households": {"h": {"members": ["p"], "region": {"2025": "LONDON"}}},
        }
        out = parity.run_rust(situation, year=2025)
        assert "income_tax" in out
        assert "hbai_household_net_income" in out
        # £50k single in 2025: income tax should land in the £7k–8k range.
        assert 7_000 < out["income_tax"] < 8_000

    def test_parity_runs_to_completion_in_no_fail_mode(self):
        # `parity()` returns 0 in --no-fail mode regardless of diffs.
        rc = parity.parity(year=2025, tolerance=1.0, fail_on_diff=False)
        assert rc == 0

    def test_cli_invocation(self):
        result = subprocess.run(
            [sys.executable, str(_SCRIPT), "--no-fail"],
            capture_output=True,
            text=True,
            cwd=str(_REPO),
            timeout=120,
        )
        assert result.returncode == 0
        assert "income_tax" in result.stdout
