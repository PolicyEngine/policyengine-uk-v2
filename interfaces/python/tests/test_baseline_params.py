from types import SimpleNamespace

import pytest

import policyengine_uk_compiled.engine as engine
from policyengine_uk_compiled import Simulation, get_baseline_params


def test_get_baseline_params_does_not_require_data_source(monkeypatch):
    calls = {}

    monkeypatch.setattr(engine, "_find_binary", lambda: "/tmp/policyengine-uk-rust")
    monkeypatch.setattr(engine, "_find_cwd", lambda binary_path: "/tmp")

    def fake_run(cmd, capture_output, text, timeout, cwd):
        calls["cmd"] = cmd
        calls["capture_output"] = capture_output
        calls["text"] = text
        calls["timeout"] = timeout
        calls["cwd"] = cwd
        return SimpleNamespace(
            returncode=0,
            stdout='{"income_tax": {"personal_allowance": 12570}}',
            stderr="",
        )

    monkeypatch.setattr(engine.subprocess, "run", fake_run)

    assert get_baseline_params(year=2025) == {
        "income_tax": {"personal_allowance": 12570}
    }
    assert calls == {
        "cmd": [
            "/tmp/policyengine-uk-rust",
            "--year",
            "2025",
            "--export-params-json",
        ],
        "capture_output": True,
        "text": True,
        "timeout": 10,
        "cwd": "/tmp",
    }


def test_get_baseline_params_raises_when_export_fails(monkeypatch):
    monkeypatch.setattr(engine, "_find_cwd", lambda binary_path: "/tmp")

    def fake_run(cmd, capture_output, text, timeout, cwd):
        return SimpleNamespace(returncode=1, stdout="", stderr="bad year")

    monkeypatch.setattr(engine.subprocess, "run", fake_run)

    with pytest.raises(RuntimeError, match="bad year"):
        get_baseline_params(year=9999, binary_path="/tmp/policyengine-uk-rust")


def test_simulation_instance_get_baseline_params_delegates(monkeypatch):
    calls = {}

    def fake_get_baseline_params(year, timeout, binary_path):
        calls["year"] = year
        calls["timeout"] = timeout
        calls["binary_path"] = binary_path
        return {"ok": True}

    monkeypatch.setattr(engine, "get_baseline_params", fake_get_baseline_params)

    sim = engine.Simulation(
        year=2030,
        persons="person_id,benunit_id,household_id\n0,0,0\n",
        benunits="benunit_id,household_id,person_ids\n0,0,0\n",
        households="household_id,benunit_ids,person_ids\n0,0,0\n",
        binary_path="/tmp/policyengine-uk-rust",
    )

    assert sim.get_baseline_params(timeout=3) == {"ok": True}
    assert calls == {
        "year": 2030,
        "timeout": 3,
        "binary_path": "/tmp/policyengine-uk-rust",
    }


def test_simulation_still_requires_data_source():
    with pytest.raises(ValueError, match="No data source specified"):
        Simulation(year=2025, binary_path="/tmp/policyengine-uk-rust")
