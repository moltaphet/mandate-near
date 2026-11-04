#!/usr/bin/env bash
# Plays eight attacks against the deployed vault code on a local NEAR sandbox,
# one after another, and records what the vault answered.
#
#   ./scripts/attack-demo.sh    writes docs/evidence/attacks.txt (about 5 minutes)
#
# Each line is one attack: what was tried, what the vault must do, and what it did.
# The test fails if any attack gets through, so a green run is the evidence.
set -euo pipefail

cd "$(dirname "$0")/.."

raw="$(mktemp)"
out="docs/evidence/attacks.txt"
mkdir -p docs/evidence

status=0
cargo test -p mandate-vault --test attacks -- --nocapture --test-threads=1 >"$raw" 2>&1 || status=$?

{
  echo "# Attack demo on a local NEAR sandbox, $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "# Vault: contracts/vault, built from this checkout. Source: tests/attacks.rs"
  grep -o 'ATTACK [0-9]\+ .*' "$raw" || true
  grep 'test result' "$raw" || true
} >"$out"

rm -f "$raw"
cat "$out"
exit "$status"
