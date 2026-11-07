#!/usr/bin/env bash
# Measures the gas each vault call burns on a local NEAR sandbox and records it.
#
#   ./scripts/gas-report.sh    writes docs/evidence/gas.txt (a few minutes)
set -euo pipefail

cd "$(dirname "$0")/.."

mandate_gas_raw="$(mktemp)"
mandate_gas_out="docs/evidence/gas.txt"
mkdir -p docs/evidence

mandate_gas_status=0
cargo test -p mandate-vault --test gas --test pyth_path -- --test-threads=1 --nocapture >"$mandate_gas_raw" 2>&1 || mandate_gas_status=$?

{
  echo "# Gas per call, local NEAR sandbox, $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "# Protocol gas burnt by the receipts of each call. Not a mainnet fee."
  printf '# Full command output retained at: %s\n' "$mandate_gas_raw"
  printf '# Command exit code: %s\n' "$mandate_gas_status"
  rg -o '(GAS|PYTH) \| .*' "$mandate_gas_raw" || true
  rg 'test result|^error:|FAILED' "$mandate_gas_raw" || true
} >"$mandate_gas_out"

cat "$mandate_gas_out"
exit "$mandate_gas_status"
