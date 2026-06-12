#!/usr/bin/env bash
# Build the Rust binary and stage it + parameters into the Python package.
# Usage: ./interfaces/python/build_package.sh
set -euo pipefail

# Ensure cargo is on PATH (needed in non-interactive shells like Modal image builds)
# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$SCRIPT_DIR/../.."
PKG_DIR="$SCRIPT_DIR/policyengine_uk_compiled"

echo "Building Rust binary (release)..."
(cd "$REPO_ROOT" && cargo build --release)

echo "Building native extension (release)..."
(cd "$REPO_ROOT" && cargo build --release --features python --lib)

echo "Staging binary into package..."
mkdir -p "$PKG_DIR/bin"
cp "$REPO_ROOT/target/release/policyengine-uk-rust" "$PKG_DIR/bin/"
chmod +x "$PKG_DIR/bin/policyengine-uk-rust"

echo "Staging native extension into package..."
case "$(uname)" in
    Darwin) DYLIB="libpolicyengine_uk_rust.dylib" ;;
    *)      DYLIB="libpolicyengine_uk_rust.so" ;;
esac
cp "$REPO_ROOT/target/release/$DYLIB" "$PKG_DIR/_native.so"

echo "Staging parameters into package..."
rm -rf "$PKG_DIR/parameters"
cp -r "$REPO_ROOT/parameters" "$PKG_DIR/parameters"

echo "Done. Build the wheel with: python -m build"
