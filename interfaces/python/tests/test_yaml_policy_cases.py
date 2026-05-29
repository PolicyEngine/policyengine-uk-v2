"""Auto-discovers every YAML test case under ``tests/policy/`` and runs each
through the Rust engine as a parametrized pytest case.

Skips cleanly when the Rust binary isn't available.
"""

from __future__ import annotations

from pathlib import Path

import pytest

pytest.importorskip("yaml")

from policyengine_uk_compiled.yaml_tests import (
    YamlTestCase,
    _run_case,
    discover_cases,
)

_REPO = Path(__file__).resolve().parents[3]
_POLICY_DIR = _REPO / "tests" / "policy"


def _has_rust_binary() -> bool:
    candidates = [
        _REPO / "target" / "release" / "policyengine-uk-rust",
        _REPO / "target" / "debug"   / "policyengine-uk-rust",
    ]
    return any(c.is_file() for c in candidates)


CASES = discover_cases(_POLICY_DIR) if _POLICY_DIR.exists() else []


@pytest.mark.skipif(not _has_rust_binary(), reason="Rust binary not built")
@pytest.mark.skipif(not CASES, reason="No YAML test cases discovered")
@pytest.mark.parametrize("case", CASES, ids=[c.name for c in CASES])
def test_yaml_policy_case(case: YamlTestCase) -> None:
    result = _run_case(case)
    if not result.passed:
        msg = f"\n{case.name} ({Path(case.file).name if case.file else '<inline>'})"
        for f in result.failures:
            msg += f"\n  ✗ {f}"
        pytest.fail(msg)
