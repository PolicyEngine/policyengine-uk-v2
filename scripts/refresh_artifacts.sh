#!/usr/bin/env bash
# Regenerate the compiled axiom artifacts in src/axiom/artifacts/ from source.
#
# Each artifact is a compiled snapshot of statute-derived rules: a compose spec
# (five live in src/axiom/programs/, universal credit in axiom-programs) is
# composed against rulespec-uk by axiom-compose, then compiled by
# axiom-rules-engine. Upstream rule improvements only reach this repo when the
# artifacts are regenerated, so run this script (and the test suite) whenever
# rulespec-uk or the engine moves.
#
# Sibling checkouts are expected next to this repo; override with env vars:
#   RULESPEC_UK, AXIOM_COMPOSE, AXIOM_PROGRAMS, AXIOM_RULES_ENGINE
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SIBLINGS="$(dirname "$REPO_ROOT")"

RULESPEC_UK="${RULESPEC_UK:-$SIBLINGS/rulespec-uk}"
AXIOM_COMPOSE="${AXIOM_COMPOSE:-$SIBLINGS/axiom-compose}"
AXIOM_PROGRAMS="${AXIOM_PROGRAMS:-$SIBLINGS/axiom-programs}"
AXIOM_RULES_ENGINE="${AXIOM_RULES_ENGINE:-$SIBLINGS/axiom-rules-engine}"

for dir in "$RULESPEC_UK" "$AXIOM_COMPOSE" "$AXIOM_RULES_ENGINE"; do
    [ -d "$dir" ] || { echo "missing checkout: $dir" >&2; exit 1; }
done

ARTIFACTS="$REPO_ROOT/src/axiom/artifacts"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

refresh() {
    local spec="$1" artifact="$2"
    local name
    name="$(basename "$artifact" .json)"
    echo "==> $name"
    uv run --project "$AXIOM_COMPOSE" python -m axiom_compose.cli "$spec" \
        --rulespec-root "$RULESPEC_UK" -o "$WORK/$name.composed.yaml"
    AXIOM_RULESPEC_REPO_ROOTS="$(dirname "$RULESPEC_UK")" \
        cargo run --release --quiet \
        --manifest-path "$AXIOM_RULES_ENGINE/Cargo.toml" \
        --bin axiom-rules-engine -- \
        compile --program "$WORK/$name.composed.yaml" --output "$ARTIFACTS/$name.json"
}

for spec in "$REPO_ROOT"/src/axiom/programs/*.yaml; do
    refresh "$spec" "$ARTIFACTS/$(basename "$spec" .yaml).json"
done
refresh "$AXIOM_PROGRAMS/uk/universal-credit/fy-2026-27.yaml" \
    "$ARTIFACTS/uk-universal-credit-fy2026.json"

echo "==> running test suite"
cargo test --manifest-path "$REPO_ROOT/Cargo.toml" --quiet

echo "done — review 'git diff src/axiom/artifacts/' for changes"
