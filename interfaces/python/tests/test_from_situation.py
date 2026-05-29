"""Tests for ``Simulation.from_situation``.

These exercise the pure-Python conversion from a situation dict into the three
input DataFrames the wrapper passes to the Rust binary. They do not invoke the
binary itself, so they run quickly with no data dependencies.
"""

from __future__ import annotations

import pytest

pd = pytest.importorskip("pandas")

from policyengine_uk_compiled.engine import (
    _resolve_period_value,
    _situation_to_dataframes,
    Simulation,
    PERSON_DEFAULTS,
    BENUNIT_DEFAULTS,
    HOUSEHOLD_DEFAULTS,
)


# ── _resolve_period_value ─────────────────────────────────────────────────────

class TestResolvePeriodValue:
    def test_scalar_passes_through(self):
        assert _resolve_period_value(42, year=2025) == 42
        assert _resolve_period_value("LONDON", year=2025) == "LONDON"
        assert _resolve_period_value(None, year=2025) is None

    def test_exact_year_match(self):
        assert _resolve_period_value({"2024": 100, "2025": 200}, year=2025) == 200

    def test_falls_back_to_most_recent_earlier_year(self):
        # No 2025 entry — pick the latest period <= 2025.
        assert _resolve_period_value({"2020": 1, "2023": 2}, year=2025) == 2

    def test_falls_back_to_earliest_when_only_later_years_present(self):
        assert _resolve_period_value({"2030": 9, "2040": 10}, year=2025) == 9

    def test_handles_year_month_keys(self):
        assert _resolve_period_value({"2025-04": 50}, year=2025) == 50

    def test_handles_eternity_style_keys(self):
        assert _resolve_period_value({"ETERNITY": "x"}, year=2025) == "x"


# ── _situation_to_dataframes ──────────────────────────────────────────────────

class TestSituationToDataframes:
    def test_minimal_single_person(self):
        situation = {
            "people": {"alice": {"age": 30, "employment_income": {"2025": 50_000}}},
            "benunits": {"bu_1": {"members": ["alice"]}},
            "households": {"hh_1": {"members": ["alice"], "region": "LONDON"}},
        }
        persons, benunits, households = _situation_to_dataframes(situation, year=2025)

        assert len(persons) == 1
        assert persons.loc[0, "person_id"] == 0
        assert persons.loc[0, "benunit_id"] == 0
        assert persons.loc[0, "household_id"] == 0
        assert persons.loc[0, "age"] == 30
        assert persons.loc[0, "employment_income"] == 50_000
        assert bool(persons.loc[0, "is_benunit_head"]) is True
        assert bool(persons.loc[0, "is_household_head"]) is True
        assert bool(persons.loc[0, "is_in_scotland"]) is False

        assert len(benunits) == 1
        assert benunits.loc[0, "person_ids"] == "0"
        assert benunits.loc[0, "household_id"] == 0

        assert len(households) == 1
        assert households.loc[0, "person_ids"] == "0"
        assert households.loc[0, "benunit_ids"] == "0"
        assert households.loc[0, "region"] == "London"

    def test_matches_single_person_constructor(self):
        """`from_situation` produces the same input frames as `single_person`."""
        sp_persons, sp_benunits, sp_households = Simulation.single_person(
            age=40, employment_income=30_000, region="London"
        )
        situation = {
            "people": {"alice": {"age": 40, "employment_income": 30_000}},
            "benunits": {"bu": {"members": ["alice"]}},
            "households": {"hh": {"members": ["alice"], "region": "London"}},
        }
        s_persons, s_benunits, s_households = _situation_to_dataframes(
            situation, year=2025
        )
        # Compare on the columns single_person is expected to set / leave as defaults.
        for col in PERSON_DEFAULTS:
            assert sp_persons.loc[0, col] == s_persons.loc[0, col], col
        for col in BENUNIT_DEFAULTS:
            assert sp_benunits.loc[0, col] == s_benunits.loc[0, col], col
        for col in HOUSEHOLD_DEFAULTS:
            assert sp_households.loc[0, col] == s_households.loc[0, col], col

    def test_couple_with_children(self):
        situation = {
            "people": {
                "p1": {"age": 35, "employment_income": {"2025": 40_000}},
                "p2": {"age": 33, "employment_income": {"2025": 25_000}},
                "c1": {"age": 6},
                "c2": {"age": 3},
            },
            "benunits": {"bu": {"members": ["p1", "p2", "c1", "c2"]}},
            "households": {"hh": {"members": ["p1", "p2", "c1", "c2"], "region": "South East"}},
        }
        persons, benunits, households = _situation_to_dataframes(situation, year=2025)

        assert len(persons) == 4
        assert list(persons["person_id"]) == [0, 1, 2, 3]
        # p1 is the implicit head of both benunit and household.
        assert bool(persons.loc[0, "is_benunit_head"]) is True
        assert bool(persons.loc[0, "is_household_head"]) is True
        assert bool(persons.loc[1, "is_benunit_head"]) is False
        assert bool(persons.loc[1, "is_household_head"]) is False
        assert benunits.loc[0, "person_ids"] == "0;1;2;3"
        assert households.loc[0, "person_ids"] == "0;1;2;3"
        assert households.loc[0, "benunit_ids"] == "0"
        assert households.loc[0, "region"] == "South East"

    def test_region_normalisation_upper_snake(self):
        situation = {
            "people": {"p": {"age": 30}},
            "benunits": {"b": {"members": ["p"]}},
            "households": {"h": {"members": ["p"], "region": "NORTH_EAST"}},
        }
        _, _, households = _situation_to_dataframes(situation, year=2025)
        assert households.loc[0, "region"] == "North East"

    def test_region_normalisation_title_case_passthrough(self):
        situation = {
            "people": {"p": {"age": 30}},
            "benunits": {"b": {"members": ["p"]}},
            "households": {"h": {"members": ["p"], "region": "South West"}},
        }
        _, _, households = _situation_to_dataframes(situation, year=2025)
        assert households.loc[0, "region"] == "South West"

    def test_scotland_sets_is_in_scotland(self):
        situation = {
            "people": {"p": {"age": 40}},
            "benunits": {"b": {"members": ["p"]}},
            "households": {"h": {"members": ["p"], "region": "SCOTLAND"}},
        }
        persons, _, households = _situation_to_dataframes(situation, year=2025)
        assert households.loc[0, "region"] == "Scotland"
        assert bool(persons.loc[0, "is_in_scotland"]) is True

    def test_explicit_is_in_scotland_overrides_region(self):
        situation = {
            "people": {"p": {"age": 40, "is_in_scotland": True}},
            "benunits": {"b": {"members": ["p"]}},
            "households": {"h": {"members": ["p"], "region": "London"}},
        }
        persons, _, _ = _situation_to_dataframes(situation, year=2025)
        # Explicit value wins even though the region is London.
        assert bool(persons.loc[0, "is_in_scotland"]) is True

    def test_gender_lowercased(self):
        situation = {
            "people": {"p": {"age": 30, "gender": "FEMALE"}},
            "benunits": {"b": {"members": ["p"]}},
            "households": {"h": {"members": ["p"]}},
        }
        persons, _, _ = _situation_to_dataframes(situation, year=2025)
        assert persons.loc[0, "gender"] == "female"

    def test_period_keyed_value_picks_year(self):
        situation = {
            "people": {
                "p": {
                    "age": 30,
                    "employment_income": {"2024": 1000, "2025": 2000, "2026": 3000},
                }
            },
            "benunits": {"b": {"members": ["p"]}},
            "households": {"h": {"members": ["p"]}},
        }
        persons, _, _ = _situation_to_dataframes(situation, year=2025)
        assert persons.loc[0, "employment_income"] == 2000

    def test_implicit_benunit_when_omitted(self):
        situation = {
            "people": {"p": {"age": 30}},
            # benunits omitted entirely
            "households": {"h": {"members": ["p"]}},
        }
        persons, benunits, households = _situation_to_dataframes(situation, year=2025)
        assert len(benunits) == 1
        assert benunits.loc[0, "person_ids"] == "0"
        assert persons.loc[0, "benunit_id"] == 0

    def test_multiple_benunits_in_one_household(self):
        situation = {
            "people": {
                "lodger": {"age": 28, "employment_income": {"2025": 20_000}},
                "owner":  {"age": 45, "employment_income": {"2025": 60_000}},
            },
            "benunits": {
                "bu_lodger": {"members": ["lodger"]},
                "bu_owner":  {"members": ["owner"]},
            },
            "households": {
                "hh": {"members": ["lodger", "owner"], "region": "London"},
            },
        }
        persons, benunits, households = _situation_to_dataframes(situation, year=2025)
        assert len(benunits) == 2
        # Each benunit head is the first (and only) member of its benunit.
        assert bool(persons.loc[0, "is_benunit_head"]) is True
        assert bool(persons.loc[1, "is_benunit_head"]) is True
        # But only the first person in the household is the household head.
        assert bool(persons.loc[0, "is_household_head"]) is True
        assert bool(persons.loc[1, "is_household_head"]) is False
        assert households.loc[0, "person_ids"] == "0;1"
        assert households.loc[0, "benunit_ids"] == "0;1"

    def test_missing_person_membership_raises(self):
        situation = {
            "people": {"orphan": {"age": 30}},
            "benunits": {"b": {"members": []}},
            "households": {"h": {"members": []}},
        }
        with pytest.raises(ValueError, match="not a member of any benunit"):
            _situation_to_dataframes(situation, year=2025)

    def test_no_people_raises(self):
        with pytest.raises(ValueError, match="people"):
            _situation_to_dataframes(
                {"people": {}, "benunits": {}, "households": {"h": {"members": []}}},
                year=2025,
            )

    def test_no_households_raises(self):
        with pytest.raises(ValueError, match="households"):
            _situation_to_dataframes(
                {"people": {"p": {"age": 30}}, "benunits": {}, "households": {}},
                year=2025,
            )


# ── Simulation.from_situation ────────────────────────────────────────────────

class TestFromSituationClassmethod:
    def test_returns_simulation_with_year(self):
        sim = Simulation.from_situation(
            {
                "people": {"p": {"age": 30}},
                "benunits": {"b": {"members": ["p"]}},
                "households": {"h": {"members": ["p"], "region": "London"}},
            },
            year=2024,
        )
        assert isinstance(sim, Simulation)
        assert sim.year == 2024

    def test_passes_through_dataframe_to_constructor(self):
        sim = Simulation.from_situation(
            {
                "people": {"p": {"age": 30, "employment_income": 25_000}},
                "benunits": {"b": {"members": ["p"]}},
                "households": {"h": {"members": ["p"], "region": "LONDON"}},
            },
        )
        # Constructor stored the DataFrames so structural pre-hooks can see them.
        assert sim._persons_df is not None
        assert sim._benunits_df is not None
        assert sim._households_df is not None
        assert sim._stdin_payload is not None
        assert "===PERSONS===" in sim._stdin_payload
