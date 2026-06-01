"""Validate that a built wheel's tag matches its bundled Rust binary."""

from __future__ import annotations

import argparse
import subprocess
import sys
import zipfile
from pathlib import Path


EXPECTED_BINARY_ARCH = {
    "x86_64-apple-darwin": ("x86_64",),
    "aarch64-apple-darwin": ("arm64",),
    "x86_64-unknown-linux-gnu": ("x86-64", "x86_64"),
    "aarch64-unknown-linux-gnu": ("aarch64", "arm aarch64"),
}


def _wheel_files(dist_dir: Path) -> list[Path]:
    return sorted(dist_dir.glob("*.whl"))


def _wheel_tags(wheel_path: Path) -> list[str]:
    with zipfile.ZipFile(wheel_path) as wheel:
        metadata_files = [
            name
            for name in wheel.namelist()
            if name.endswith(".dist-info/WHEEL")
        ]
        if len(metadata_files) != 1:
            raise ValueError(
                f"Expected exactly one WHEEL metadata file in {wheel_path}, "
                f"found {len(metadata_files)}"
            )
        metadata = wheel.read(metadata_files[0]).decode()
    return [
        line.split(":", 1)[1].strip()
        for line in metadata.splitlines()
        if line.startswith("Tag:")
    ]


def _binary_description(binary_path: Path) -> str:
    result = subprocess.run(
        ["file", str(binary_path)],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def validate_wheel_artifact(
    *,
    dist_dir: Path,
    package_dir: Path,
    target: str,
    wheel_platform: str,
) -> None:
    wheels = _wheel_files(dist_dir)
    if len(wheels) != 1:
        raise ValueError(f"Expected exactly one wheel in {dist_dir}, found {len(wheels)}")

    wheel = wheels[0]
    expected_tag = f"py3-none-{wheel_platform}"
    if expected_tag not in wheel.name:
        raise ValueError(f"{wheel.name} does not contain expected tag {expected_tag}")

    tags = _wheel_tags(wheel)
    if expected_tag not in tags:
        raise ValueError(f"{wheel.name} metadata tags {tags} do not include {expected_tag}")

    binary_path = package_dir / "bin" / "policyengine-uk-rust"
    if not binary_path.is_file():
        raise FileNotFoundError(f"Bundled Rust binary not found: {binary_path}")

    if target not in EXPECTED_BINARY_ARCH:
        raise ValueError(f"Unsupported Rust target for validation: {target}")
    description = _binary_description(binary_path).lower()
    expected_arches = EXPECTED_BINARY_ARCH[target]
    if not any(arch in description for arch in expected_arches):
        raise ValueError(
            f"Binary architecture does not match target {target}: "
            f"expected one of {expected_arches}, got {description!r}"
        )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist-dir", type=Path, default=Path("dist"))
    parser.add_argument(
        "--package-dir",
        type=Path,
        default=Path("interfaces/python/policyengine_uk_compiled"),
    )
    parser.add_argument("--target", required=True)
    parser.add_argument("--wheel-platform", required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        validate_wheel_artifact(
            dist_dir=args.dist_dir,
            package_dir=args.package_dir,
            target=args.target,
            wheel_platform=args.wheel_platform,
        )
    except Exception as exc:
        print(f"wheel artifact validation failed: {exc}", file=sys.stderr)
        return 1
    print("wheel artifact validation passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
