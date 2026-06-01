"""Unit tests for the YAML test runner itself (pure Python, no engine).

The end-to-end execution of YAML cases against the Rust binary is handled by
``test_yaml_policy_cases.py``.
"""

from __future__ import annotations

import textwrap
from pathlib import Path

import pytest

pytest.importorskip("yaml")

from policyengine_uk_compiled.yaml_tests import (
    YamlTestCase,
    _flat_input_to_situation,
    _is_full_situation,
    _within_tolerance,
    load_yaml_file,
)


class TestIsFullSituation:
    def test_flat_input(self):
        assert _is_full_situation({"employment_income": 50_000}) is False

    def test_situation_with_people(self):
        assert _is_full_situation({"people": {}}) is True

    def test_situation_with_only_households(self):
        assert _is_full_situation({"households": {}}) is True


class TestFlatInputToSituation:
    def test_person_field_lands_on_person(self):
        out = _flat_input_to_situation({"employment_income": 50_000, "age": 30})
        assert out["people"]["you"]["employment_income"] == 50_000
        assert out["people"]["you"]["age"] == 30

    def test_region_lands_on_household(self):
        out = _flat_input_to_situation({"region": "London"})
        assert out["households"]["h"]["region"] == "London"
        assert "region" not in out["people"]["you"]

    def test_is_lone_parent_lands_on_benunit(self):
        out = _flat_input_to_situation({"is_lone_parent": True})
        assert out["benunits"]["b"]["is_lone_parent"] is True
        assert "is_lone_parent" not in out["people"]["you"]

    def test_rent_monthly_lands_on_benunit(self):
        out = _flat_input_to_situation({"rent_monthly": 1200})
        assert out["benunits"]["b"]["rent_monthly"] == 1200

    def test_council_tax_lands_on_household(self):
        out = _flat_input_to_situation({"council_tax_annual": 1500})
        assert out["households"]["h"]["council_tax_annual"] == 1500

    def test_members_always_set(self):
        out = _flat_input_to_situation({"employment_income": 0})
        assert out["benunits"]["b"]["members"] == ["you"]
        assert out["households"]["h"]["members"] == ["you"]


class TestWithinTolerance:
    def _case(self, abs_margin: float = 1.0, rel_margin: float | None = None):
        return YamlTestCase(
            name="t", period=2025, input={}, output={},
            absolute_error_margin=abs_margin, relative_error_margin=rel_margin,
        )

    def test_exact_match_within_zero_margin(self):
        assert _within_tolerance(100.0, 100, self._case(abs_margin=0)) is True

    def test_within_absolute_margin(self):
        assert _within_tolerance(100.5, 100, self._case(abs_margin=1.0)) is True

    def test_outside_absolute_margin(self):
        assert _within_tolerance(102.0, 100, self._case(abs_margin=1.0)) is False

    def test_relative_margin_kicks_in_for_large_values(self):
        # 5 over 1000 is 0.5%, within rel=1% even though abs > 1
        assert _within_tolerance(1005.0, 1000, self._case(abs_margin=0, rel_margin=0.01)) is True

    def test_relative_margin_ignored_when_expected_zero(self):
        assert _within_tolerance(2.0, 0, self._case(abs_margin=1.0, rel_margin=0.5)) is False

    def test_boolean_values_compared_exactly(self):
        assert _within_tolerance(True, True, self._case()) is True
        assert _within_tolerance(False, True, self._case()) is False


class TestLoadYamlFile:
    def test_loads_multiple_cases(self, tmp_path: Path):
        f = tmp_path / "x.yaml"
        f.write_text(textwrap.dedent("""\
            - name: a
              period: 2025
              input: {employment_income: 0}
              output: {baseline_income_tax: 0}
            - name: b
              period: 2024
              absolute_error_margin: 5
              input: {employment_income: 50000}
              output: {baseline_income_tax: 7486}
        """))
        cases = load_yaml_file(f)
        assert len(cases) == 2
        assert cases[0].name == "a"
        assert cases[0].period == 2025
        assert cases[0].absolute_error_margin == 1.0
        assert cases[1].absolute_error_margin == 5.0

    def test_empty_file_yields_empty_list(self, tmp_path: Path):
        f = tmp_path / "empty.yaml"
        f.write_text("")
        assert load_yaml_file(f) == []

    def test_missing_name_raises(self, tmp_path: Path):
        f = tmp_path / "bad.yaml"
        f.write_text("- period: 2025\n  input: {}\n  output: {}\n")
        with pytest.raises(ValueError, match="missing 'name'"):
            load_yaml_file(f)

    def test_missing_period_raises(self, tmp_path: Path):
        f = tmp_path / "bad.yaml"
        f.write_text("- name: x\n  input: {}\n  output: {}\n")
        with pytest.raises(ValueError, match="period"):
            load_yaml_file(f)

    def test_missing_input_or_output_raises(self, tmp_path: Path):
        f = tmp_path / "bad.yaml"
        f.write_text("- name: x\n  period: 2025\n  input: {}\n")
        with pytest.raises(ValueError, match="input.*output"):
            load_yaml_file(f)

    def test_top_level_must_be_a_list(self, tmp_path: Path):
        f = tmp_path / "bad.yaml"
        f.write_text("name: not-a-list\n")
        with pytest.raises(ValueError, match="list"):
            load_yaml_file(f)
