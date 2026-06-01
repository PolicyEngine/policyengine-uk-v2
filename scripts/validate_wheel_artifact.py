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

EXPECTED_PYTHON_TAG = "py3"
EXPECTED_ABI_TAG = "none"


def _wheel_files(dist_dir: Path) -> list[Path]:
    return sorted(dist_dir.glob("*.whl"))


def _split_platform_tags(platform_tag: str) -> set[str]:
    return set(platform_tag.split("."))


def _parse_wheel_tag(tag: str) -> set[str]:
    parts = tag.split("-", 2)
    if len(parts) != 3:
        raise ValueError(f"Invalid wheel tag: {tag}")

    python_tag, abi_tag, platform_tag = parts
    if python_tag != EXPECTED_PYTHON_TAG or abi_tag != EXPECTED_ABI_TAG:
        raise ValueError(
            f"Expected wheel tag prefix {EXPECTED_PYTHON_TAG}-{EXPECTED_ABI_TAG}, "
            f"got {python_tag}-{abi_tag}"
        )
    return _split_platform_tags(platform_tag)


def _filename_platform_tags(wheel_path: Path) -> set[str]:
    if wheel_path.suffix != ".whl":
        raise ValueError(f"Expected a .whl file, got {wheel_path.name}")

    parts = wheel_path.name[:-4].rsplit("-", 3)
    if len(parts) != 4:
        raise ValueError(f"Invalid wheel filename: {wheel_path.name}")

    _, python_tag, abi_tag, platform_tag = parts
    return _parse_wheel_tag(f"{python_tag}-{abi_tag}-{platform_tag}")


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


def _metadata_platform_tags(wheel_path: Path) -> set[str]:
    platform_tags: set[str] = set()
    tags = _wheel_tags(wheel_path)
    if not tags:
        raise ValueError(f"{wheel_path.name} metadata has no Tag entries")

    for tag in tags:
        platform_tags.update(_parse_wheel_tag(tag))
    return platform_tags


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
    expected_platform_tags = _split_platform_tags(wheel_platform)
    filename_platform_tags = _filename_platform_tags(wheel)
    if filename_platform_tags != expected_platform_tags:
        raise ValueError(
            f"{wheel.name} filename platform tags {sorted(filename_platform_tags)} "
            f"do not match expected {sorted(expected_platform_tags)}"
        )

    metadata_platform_tags = _metadata_platform_tags(wheel)
    if metadata_platform_tags != expected_platform_tags:
        raise ValueError(
            f"{wheel.name} metadata platform tags {sorted(metadata_platform_tags)} "
            f"do not match expected {sorted(expected_platform_tags)}"
        )

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
