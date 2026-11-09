# Mandate hardening implementation plan

> **For agentic workers:** Execute these tasks with collaboration subagents and review each diff before final verification. Steps use checkbox syntax.

**Goal:** Fix every actionable defect confirmed by the 8 November review and finish the existing withdrawal-ceiling feature without weakening custody guards.

**Architecture:** Keep the pure policy/schema boundary, contract-owned authorization and identified callback/recovery protocol. Reject underfunded execution before state changes, validate keeper chain identity before writes, and treat routine pre-submission agent failures as recoverable loop events. Preserve unknown-result guards and distinguish release evidence from live readiness.

**Tech Stack:** Rust 1.95.0, near-sdk 5.30.0, cargo-near 0.22.0, Node 22, Python 3.

## Global constraints

- Work on the existing feat/withdrawal-ceiling checkout; preserve the user's pre-existing withdrawal tests and three fixture/exporter files.
- No credentials, deployments, public-chain transactions, commits, pushes or unverified readiness claims.
- Unknown withdrawals never gain a timer or owner bypass; unknown submitted client transactions must be reconciled before another proposal.
- Withdrawal ceilings are per-request positive token-base-unit amounts; an omitted asset ceiling preserves the legacy uncapped behavior and is explicitly displayed.
- Adding/tightening a ceiling applies immediately; increasing/removing an existing ceiling waits 86,400 seconds, including combined asset removal.
- Preserve existing supported migrations, provider choice, pending deadlines and quiescence guards; add actual pre-ceiling state fixtures.
- Final checks must run after all edits stabilize, with serial sandbox targets and maximum-size gas tests.

## Task 1: Complete withdrawal ceilings and migration

**Files:** contracts/vault/src/mandate.rs, lib.rs (schema exports and withdrawal entry points only), withdrawal.rs, migration.rs, contract examples/tests/fixtures and test builders.

- [x] Reuse the currently failing ceiling regression tests and add schema tests for zero, duplicate, unknown-asset and oversized ceiling lists.
- [x] Add the exact public schema:

```rust
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithdrawalLimit {
    pub asset_id: String,
    pub max_per_request: U128,
}
// Mandate field:
#[serde(default)]
pub withdrawal_limits: Vec<WithdrawalLimit>,
```

- [x] Validate ceilings as a unique positive subset of mandate assets, bounded by MAX_ASSETS. Add a lookup returning Option<u128> and refuse an amount strictly above its ceiling.
- [x] Call the ceiling check before mutation in request/apply and before dispatch in on_withdrawal_prepared; preparation refusal uses withdrawal_failed and preserves replacement generation/arm rules.
- [x] In loosened_fields, emit withdrawal_limits when any old ceiling is absent or increased; retain this condition when its asset is removed.
- [x] Add a distinct pre-ceiling Borsh layout containing allowed_model_ids, convert its active and queued policies with empty withdrawal_limits, and preserve all existing paused/quiescent migration checks.
- [x] Verify boundary, tightening, cancellation, refund, stale-callback and migration tests, including final native and actual-Wasm callback/gas coverage.

## Task 2: Reject gas grief before execution state

**Files:** contracts/vault/src/lib.rs (execute and constant), contracts/vault/tests/execute_path.rs, SECURITY.md.

- [x] Preserve the failing sandbox reproduction at /private/tmp/mandate-audit-gas-repro-20261108.rs as the red evidence.
- [x] Add:

```rust
pub const MIN_EXECUTION_GAS_TGAS: u64 = 300;
// In execute, before next_execution_id and in_flight mutation:
require!(env::prepaid_gas() >= Gas::from_tgas(MIN_EXECUTION_GAS_TGAS),
         "execution requires at least 300 TGas");
```

- [x] Add regression: stranger calls valid execute with 30 TGas; rejection leaves pending unchanged, no lock or signature; owner's 300-TGas retry executes successfully. Add a one-below/equality native boundary check and both-provider maximum-size verification.
- [x] Run the execute-path target (including the low-gas attack refusal) and supported maximum-assets gas/Pyth targets on the final allocator.

## Task 3: Enforce keeper network and resilient agent iteration

**Files:** keeper/src/lib.rs, main.rs, publication.rs; agent/src/main.rs and focused client tests/builders.

- [x] Add a keeper expected-chain preflight using the shared PublicReader status/final anchor validation before the first tick and before transaction execution; carry configured expected_chain_id out of publication configuration.
- [x] Add a fake-RPC regression where configured chain differs: no transaction broadcast occurs. Preserve existing sandbox constructors or update their explicit optional chain field.
- [x] Refactor one autonomous iteration into a result with a clear pre-send boundary. Routine snapshot/quote/transport errors in --loop log sanitized failure and sleep with bounded backoff; one-shot mode still returns its error.
- [x] Preserve fail-stop/reconciliation for ambiguous submission rather than blindly generating a new proposal. Test initial RPC outage followed by recovery, quote changed between prepare/refresh, and one-shot failure.
- [x] Update client Mandate literals with withdrawal_limits: vec![]; run client unit and five protocol/model/provenance targets.

## Task 4: Bind fixture identity and fix preview/public schema

**Files:** scripts/verify.sh, fetch-intents-fixture.sh, dashboard-fixture.py; web/dashboard/live.mjs, live.test.mjs, site/audit.mjs, site/index.html; contract tests/common/mod.rs; documentation.

- [x] Verify fixture SHA256 a1afab98916d9f213746544dcf8e4e6bdb22c0be6bc5dfbd8ea8814c9922b547 and size 1,297,551 before sandbox deployment; valid replacement WASM must be refused. Keep the validation test fast and independent of a sandbox.
- [x] Make default fixture fetching use a separate comparison output instead of overwriting pinned evidence; replacement remains explicit.
- [x] Parse GET pathname with urlsplit(self.path).path before the preview allowlist; verify shared root URL returns 200 while forbidden paths remain 404.
- [x] Validate/render withdrawal_limits and legacy omission in the dashboard; show exact base units and per-request scope, never imply an aggregate daily cap. Regenerate site/audit.mjs through build-web.py.
- [x] Align website/README/security evidence with revision-bound historical passes and current results, without claiming funded live readiness.
- [x] Run 33+ browser logic tests, inspector tests, new fixture/preview checks, reader parity and shell syntax.

## Task 5: Review, verify and report

- [x] Review every delegated diff against authorization, unknown-result safety and migration compatibility.
- [x] Complete every verify.sh component and the full configured serial integration scope on the stabilized production source. Direct commands passed; the final wrapper invocation was rejected by approval infrastructure. Record the actual failed fixture-expectation attempts and successful targeted reruns.
- [x] Update the audit report to mark closed bugs with fresh evidence and list actual residuals: HWM cashflow behavior, F26, live oracle/governance/settlement/withdrawal/recovery/audit gates. Do not invent on-chain escape hatches to make those risks disappear.
- [x] Record current OSV matches and dependency-path applicability; preserve any advisory-detail lookup limits and outstanding transitive advisories.

## Dependency advisory remediation (8 November follow-up)

**Files:** Cargo.toml, Cargo.lock, SECURITY.md; rebuilt contract evidence and current dependency scan.

- [x] Confirm wee_alloc 0.4.5 is the NEAR SDK default global allocator in the production Wasm graph, with OSV identifiers RUSTSEC-2022-0054 / GHSA-rc23-xxgq-x27g.
- [x] Disable near-sdk default features at the workspace dependency, retaining version 5.30.0 and explicit native testing/client features. Use Rust's standard Wasm allocator; add no custom allocator or SDK patch.
- [x] Run cargo check --offline --workspace --all-targets (exit 0) and confirm Cargo.lock prunes wee_alloc, memory_units, cfg-if 0.1.10 and unused legacy winapi leaves.
- [x] Rebuild all affected contract Wasm and complete configured integration: PriceOracle with20assets/20ceilings/8longsourceIDs, Pyth with20assets/20ceilings, and actual withdrawal/recovery budgets. Record exact tested artifact identity; intended-chain storage/gas evidence remains a release gate.
- [x] Rescan the stabilized lock after compatible updates. The user-run OSV491-package scan matches the current lock; only inherited crypto-bigint0.5.5 and unsafe-libyaml0.2.11 remain. Runtime clients use fixed crypto-bigint0.7.10; the older SDK test-VM path verifies public data only.
- [x] Keep unsafe-libyaml 0.2.11 / RUSTSEC-2024-0449 as an outstanding native unit-test/runtime transitive dependency until its advisory can be fully assessed or upstream removes it; production Wasm and agent/keeper normal graphs exclude it.

## Final execution state

- Final production graph: 360 Rust fast tests, 64 sandbox tests across all14 configured targets, 36 JavaScript tests and19 Python tests passed (479 unique tests).
- Strict all-target Clippy, formatting, fixture identity, shell syntax, references, reader parity and diff checks passed.
- Sandbox stale JSON expectations (two cases) and withdrawal stale fixture JSON (one case) were repaired; the entire affected targets passed on rerun. Production source/lock/Wasm remained unchanged. The three serial command attempts exited101,101,0; no failed attempt is represented as a full pass.
- The final verify.sh wrapper was not executed because automatic approval review failed. Every required component was executed directly with the locked cached dependencies.
- User-run final OSV data is preserved in docs/audit/DEPENDENCY-SCAN-2026-11-08.json and verified against Cargo.lock SHA256 e53712759c0f89ce98a86aa7569b0b0ab6dcb55c752e9465635844c51ca7e287. Quinn/Zstd known matches cleared; two inherited SDK-native flags remain disclosed without suppression.
- Tested vault: 411312bytes, SHA256 4c2e2301a3688397edb1cd1499e415e1d7cd277433aaab24f1fadee15908fde2.
- Final report: docs/audit/HARDENING-2026-11-08.md. HWM/F26 decisions, final release freeze/CI/reproducibility, live oracle/governance/funded settlement/elapsed withdrawal/recovery/monitoring and independent audit remain separate release work.
- No commit, push, deployment or live funds action performed.
