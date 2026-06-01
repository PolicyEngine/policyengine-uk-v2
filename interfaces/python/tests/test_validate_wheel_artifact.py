"""Tests for release wheel artifact validation."""

import importlib.util
import zipfile
from pathlib import Path

import pytest


_REPO = Path(__file__).resolve().parents[3]
_SCRIPT = _REPO / "scripts" / "validate_wheel_artifact.py"
_SPEC = importlib.util.spec_from_file_location("validate_wheel_artifact", _SCRIPT)
validator = importlib.util.module_from_spec(_SPEC)
assert _SPEC.loader is not None
_SPEC.loader.exec_module(validator)


def _write_wheel(dist_dir: Path, filename: str, tag: str) -> Path:
    dist_dir.mkdir()
    wheel_path = dist_dir / filename
    with zipfile.ZipFile(wheel_path, "w") as wheel:
        wheel.writestr(
            "policyengine_uk_compiled-0.30.0.dist-info/WHEEL",
            f"Wheel-Version: 1.0\nRoot-Is-Purelib: false\nTag: {tag}\n",
        )
    return wheel_path


def _write_binary(package_dir: Path) -> Path:
    binary = package_dir / "bin" / "policyengine-uk-rust"
    binary.parent.mkdir(parents=True)
    binary.write_text("fake binary")
    return binary


def test_validates_matching_macos_x86_64_wheel(tmp_path, monkeypatch):
    dist_dir = tmp_path / "dist"
    package_dir = tmp_path / "pkg"
    _write_wheel(
        dist_dir,
        "policyengine_uk_compiled-0.30.0-py3-none-macosx_10_13_x86_64.whl",
        "py3-none-macosx_10_13_x86_64",
    )
    _write_binary(package_dir)
    monkeypatch.setattr(
        validator,
        "_binary_description",
        lambda path: "Mach-O 64-bit executable x86_64",
    )

    validator.validate_wheel_artifact(
        dist_dir=dist_dir,
        package_dir=package_dir,
        target="x86_64-apple-darwin",
        wheel_platform="macosx_10_13_x86_64",
    )


def test_validates_matching_macos_arm64_wheel(tmp_path, monkeypatch):
    dist_dir = tmp_path / "dist"
    package_dir = tmp_path / "pkg"
    _write_wheel(
        dist_dir,
        "policyengine_uk_compiled-0.30.0-py3-none-macosx_11_0_arm64.whl",
        "py3-none-macosx_11_0_arm64",
    )
    _write_binary(package_dir)
    monkeypatch.setattr(
        validator,
        "_binary_description",
        lambda path: "Mach-O 64-bit executable arm64",
    )

    validator.validate_wheel_artifact(
        dist_dir=dist_dir,
        package_dir=package_dir,
        target="aarch64-apple-darwin",
        wheel_platform="macosx_11_0_arm64",
    )


def test_rejects_universal2_tag_for_arm64_only_binary(tmp_path, monkeypatch):
    dist_dir = tmp_path / "dist"
    package_dir = tmp_path / "pkg"
    _write_wheel(
        dist_dir,
        "policyengine_uk_compiled-0.30.0-py3-none-macosx_10_13_universal2.whl",
        "py3-none-macosx_10_13_universal2",
    )
    _write_binary(package_dir)
    monkeypatch.setattr(
        validator,
        "_binary_description",
        lambda path: "Mach-O 64-bit executable arm64",
    )

    with pytest.raises(ValueError, match="expected tag"):
        validator.validate_wheel_artifact(
            dist_dir=dist_dir,
            package_dir=package_dir,
            target="aarch64-apple-darwin",
            wheel_platform="macosx_11_0_arm64",
        )


def test_rejects_wheel_filename_and_metadata_disagreement(tmp_path, monkeypatch):
    dist_dir = tmp_path / "dist"
    package_dir = tmp_path / "pkg"
    _write_wheel(
        dist_dir,
        "policyengine_uk_compiled-0.30.0-py3-none-macosx_10_13_x86_64.whl",
        "py3-none-macosx_11_0_arm64",
    )
    _write_binary(package_dir)
    monkeypatch.setattr(
        validator,
        "_binary_description",
        lambda path: "Mach-O 64-bit executable x86_64",
    )

    with pytest.raises(ValueError, match="metadata tags"):
        validator.validate_wheel_artifact(
            dist_dir=dist_dir,
            package_dir=package_dir,
            target="x86_64-apple-darwin",
            wheel_platform="macosx_10_13_x86_64",
        )


def test_rejects_binary_architecture_mismatch(tmp_path, monkeypatch):
    dist_dir = tmp_path / "dist"
    package_dir = tmp_path / "pkg"
    _write_wheel(
        dist_dir,
        "policyengine_uk_compiled-0.30.0-py3-none-macosx_10_13_x86_64.whl",
        "py3-none-macosx_10_13_x86_64",
    )
    _write_binary(package_dir)
    monkeypatch.setattr(
        validator,
        "_binary_description",
        lambda path: "Mach-O 64-bit executable arm64",
    )

    with pytest.raises(ValueError, match="Binary architecture"):
        validator.validate_wheel_artifact(
            dist_dir=dist_dir,
            package_dir=package_dir,
            target="x86_64-apple-darwin",
            wheel_platform="macosx_10_13_x86_64",
        )
