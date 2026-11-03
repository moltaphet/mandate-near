#!/usr/bin/env bash
# One-command check for humans, CI and AI screeners. Needs no keys.
#
#   ./scripts/verify.sh                 fmt, clippy, unit tests (fast, offline after crates)
#   ./scripts/verify.sh --integration   also deploys the contract to a local NEAR
#                                       sandbox (downloads the sandbox binary once)
set -euo pipefail

cd "$(dirname "$0")/.."

integration=false
for arg in "$@"; do
  case "$arg" in
    --integration) integration=true ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

step() { printf '\n==> %s\n' "$1"; }

step "Formatting (cargo fmt --check)"
cargo fmt --all -- --check

step "Lints (cargo clippy, warnings are errors)"
cargo clippy --workspace --all-targets -- -D warnings

step "Unit tests (cargo test --lib)"
cargo test --workspace --lib

if $integration; then
  step "Integration tests on a local NEAR sandbox (owner controls; key registration on the real intents.near)"
  cargo test -p mandate-vault --test sandbox --test intents_registration
fi

printf '\nAll checks passed.\n'
