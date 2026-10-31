#!/usr/bin/env bash
# One-command check for humans, CI and AI screeners. Needs no keys or network
# access beyond fetching crates.
set -euo pipefail

cd "$(dirname "$0")/.."

step() { printf '\n==> %s\n' "$1"; }

step "Formatting (cargo fmt --check)"
cargo fmt --all -- --check

step "Lints (cargo clippy, warnings are errors)"
cargo clippy --workspace --all-targets -- -D warnings

step "Unit tests (cargo test)"
cargo test --workspace

printf '\nAll checks passed.\n'
