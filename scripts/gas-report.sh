#!/usr/bin/env bash
# Measures the gas each vault call burns on a local NEAR sandbox and records it.
#
#   ./scripts/gas-report.sh    writes docs/evidence/gas.txt (a few minutes)
set -euo pipefail

cd "$(dirname "$0")/.."

raw="$(mktemp)"
out="docs/evidence/gas.txt"
mkdir -p docs/evidence

status=0
cargo test -p mandate-vault --test gas -- --nocapture >"$raw" 2>&1 || status=$?

{
  echo "# Gas per call, local NEAR sandbox, $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "# Protocol gas burnt by the receipts of each call. Not a mainnet fee."
  grep -o 'GAS | .*' "$raw" || true
  grep 'test result' "$raw" || true
} >"$out"

rm -f "$raw"
cat "$out"
exit "$status"
