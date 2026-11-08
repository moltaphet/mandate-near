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
cargo clippy --workspace --all-targets --locked -- -D warnings

step "Documentation references (allowed documents only)"
bash scripts/check-claims.sh

step "Reviewed Intents verifier identity (no sandbox or network)"
python3 -B scripts/verify-intents-fixture.py
python3 -B scripts/intents_fixture_test.py
cargo test -p mandate-vault --test fixture_identity --locked

step "Read-only dashboard transport, schema and rendering tests"
node --test web/dashboard/live.test.mjs

step "Main-site navigation, share-link and clipboard safety tests"
node --test web/site/site.test.mjs

step "Existing site inspection asset matches the tested reader"
python3 -B scripts/build-web.py --check

step "Deployment inspection safety tests"
python3 -B scripts/inspect_deployment_test.py

step "Local preview routing safety tests"
python3 -B scripts/dashboard_fixture_test.py

step "Unit tests (cargo test --lib)"
cargo test --workspace --lib --locked

step "Shared withdrawal-journal commitment and schema tests"
cargo test -p mandate-withdrawal-journal --locked

step "Pinned source-owned executor preparation (offline, no build)"
python3 -B integrations/intents-executor/prepare_test.py
python3 -B integrations/intents-executor/build_test.py

step "Autonomous strategy, offline model decisions, model provenance and relay protocol tests"
cargo test -p mandate-agent --test autonomous --test protocol --test model --test provenance --test provenance_cli --locked

step "Keeper network pinning, ID-bound recovery and paid-transaction ambiguity"
cargo test -p mandate-keeper --test network --test recovery --locked

if $integration; then
  step "Sandbox integration and gas regressions (legacy fixture plus pinned source-owned journal executor)"
  cargo test -p mandate-vault --test sandbox --test intents_registration --test execute_path --test keeper --test agent --test model_path --test attacks --test withdraw --test gas --test settlement --test withdrawal_lifecycle --test recovery_protocol --test pyth_path --test sealed_deployment --test journal_withdrawal --locked -- --test-threads=1
fi

printf '\nAll checks passed.\n'
