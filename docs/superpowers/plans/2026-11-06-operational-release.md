# Mandate operational release implementation plan

> **For agentic workers:** Execute this authorized plan in the existing working tree with separate file ownership and independent review gates. Do not commit, deploy, push, publish, or use live funds. Steps use checkbox tracking.

**Goal:** Deliver a quote-driven autonomous agent, a connected read-only audit dashboard, and a verified recovery/readiness package for the exact vault revision.

**Architecture:** Reuse the vault's pure mandate engine for off-chain preview; the contract remains the authorization boundary. Pin public RPC snapshots to one final block and distinguish signature acquisition, publication acknowledgement and settlement. Recovery changes require authenticated operation evidence; elapsed time or an increased balance never authorizes a duplicate withdrawal.

**Tech Stack:** Rust 2024, near-sdk 5.30.0, near-api 0.8.7, near-sandbox 0.4.0, cargo-near 0.22.0 metadata, vanilla HTML/JavaScript and Node direct tests.

## Global constraints

- Work in /Users/ehs4n/mandate-near on the existing fix/audit-round-1 branch; preserve prior edits.
- No secrets/credential files, prohibited filenames, dependency/build output or lockfile contents are reviewed.
- contracts/vault/tests/common/mod.rs:21: secret-like value, redacted. Do not reread or reproduce that fixture value.
- No live signing, deployment, funds, commit, push or external publication without separately authorized concrete execution.
- Keep MAX_ASSETS=20, sensitive delays=86400 seconds, proposal TTL=60 seconds, intent TTL=120 seconds, signing window=300 seconds.
- Never label a stored signature or relay acknowledgement as settlement.
- Retain unknown-outcome guards until a reviewed evidence mechanism proves safe progress.
- Source changes and claims must be backed by observed command results; no ranking or zero-defect guarantee.

---

## Task 1: Withdrawal recovery feasibility and exact external behavior

**Files:** Review contracts/vault/src/lib.rs, contracts/vault/src/remediation_tests.rs, contracts/vault/tests/withdrawal_lifecycle.rs, contracts/vault/tests/fixtures/README.md. Create docs/evidence/recovery-design.txt.

**Consumes:** Current ActiveWithdrawal fields id, request_seq, request, started_sec and balance_before; existing on_withdrawn and on_withdrawal_reconciled behavior.
**Produces:** A concrete source-backed recovery design or a documented external API blocker. Production mutation is a separate review gate.

- [ ] Inspect the current upstream Intents withdrawal, resolver, direct transfer, nonce and receipt APIs without reading prohibited filenames.
- [ ] Trace the reproduced case: baseline100, withdraw20, credit25, ambiguous terminal result, current105.
- [ ] Check whether any authenticated per-operation state exists; nonce consumption must prove the relevant ledger action, not merely signature acceptance.
- [ ] Evaluate atomic receipt markers only if unforgeability, batch rollback and returned-promise behavior are proven; do not introduce an unreviewed privileged release.
- [ ] Add a sandbox experiment for the chosen runtime behavior before altering the guard.
- [ ] Keep F26 explicitly open if no safe compatible mechanism is established. Do not use a timeout, operator assertion, balance increase or blind retry as proof.

Acceptance: no release before a genuine debit/refund/terminal proof; no second send under the old authorization; recovery works despite incoming same-asset credits. If unavailable, the missing protocol/evidence dependency is precisely identified.

## Task 2: Autonomous quote-driven agent and relay integration

**Files:** Modify agent/Cargo.toml, agent/src/lib.rs, agent/src/main.rs, keeper/Cargo.toml, keeper/src/lib.rs, keeper/src/main.rs. Create focused agent strategy/transport modules, public example configuration and protocol tests. Modify contracts/vault/tests/agent.rs only for new autonomous integration evidence.

**Consumes:** Mandate, TradingStateView, Intents ledger balances, coherent oracle reports, final chain time and an exact-input solver quote.
**Produces:** A deterministic rebalance decision, Plan with rationale hash and exact quoted amounts, proposal-associated public relay metadata, explicit publication acknowledgement/status.

Core behavior:
```text
read final block -> validate mandate/readiness -> value actual holdings
-> choose listed overweight input/underweight destination under configured target band
-> bound input by balance, per-trade cap and remaining daily allowance
-> request exact-input quote -> validate asset IDs/input/expiry/output
-> preview check_trade using the vault engine -> produce Plan or an explicit no-trade reason
-> dry-run output OR submit through existing agent account -> record proposal metadata
```

- [ ] Preserve manual plan mode; add an explicit autonomous configuration mode and read-only dry-run.
- [ ] Use exact integer base units/micro-USD; do not duplicate security gates using floating point.
- [ ] Use the actual documented relay quote schema; no invented public API. Bound response size/time and validate returned identifiers/amounts.
- [ ] Keep quote_hash/proposal/payload association explicit; exact signed output must match the selected quote.
- [ ] Add quote malformed/expired/mismatched tests and strategy cap/cooldown/stale-data/no-trade tests.
- [ ] Add keeper publication only as an explicit opt-in mode; local tests use a mock HTTP endpoint. Retain acknowledgement separately from confirmed settlement.
- [ ] Run agent/keeper unit tests and a sandbox agent-to-keeper-to-signed-intent integration; preserve no authority over withdrawals or owner methods.

Acceptance: an input configuration plus current ledger/quote data produces and submits a proposal without a human-authored trade file; transport/protocol failures spend no avoidable proposal gas and do not invent settlement.

## Task 3: Connected read-only audit dashboard

**Files:** Modify web/dashboard/index.html. Create web/dashboard/live.mjs and focused Node tests. Preserve the existing illustration as an explicitly separated sample mode.

**Consumes:** Public vault account and chosen RPC network. Read get_chain_time_sec at finality=final, then pin subsequent queries using its returned block_hash.
**Produces:** Real observed state, bounded latest-outcome timeline and correct raw-unit formatting; no wallet mutation.

Core rendering:
```javascript
const element = document.createElement("span");
element.textContent = untrustedRpcText;
```

- [ ] Reuse the existing visual identity; make observed network/account/block and freshness prominent.
- [ ] Read current rules, owner/agent, readiness, pending proposal, recorded outcome, signature expiry, withdrawal phase, queues and Intents balances.
- [ ] Pin all values to one final block; ignore stale/out-of-order requests and show RPC/schema errors.
- [ ] Format 6/24/30-decimal assets with BigInt; do not show sample balances as live results.
- [ ] Derive Signed only from recorded evidence; show settlement as unverified unless a genuine final execution receipt is supplied and checked.
- [ ] Render RPC/URL-derived data with textContent and validated links, not innerHTML.
- [ ] Test hostile strings, large amounts, deadline equality, unknown outcomes, pinned-block transport and wrong code identity.
- [ ] Inspect desktop/mobile layout in the browser and verify actual fetch/error behavior using an explicitly labelled local RPC test endpoint.

Acceptance: a judge can distinguish live observations, illustration, unknown history and verified receipts without reading implementation notes.

## Task 4: Readiness and evidence tooling

**Files:** Create a read-only deployment-inspection script, deployment-readiness documentation and docs/evidence/operational-verification.txt. Modify scripts/verify.sh and scripts/check-claims.sh to include new tests safely.

- [ ] Read public code hashes and account permissions; emit only counts/booleans, never access-key values.
- [ ] Compare a newly downloaded Intents binary separately from the pinned fixture; preserve old evidence.
- [ ] Establish current oracle availability and adapter compatibility through read-only calls. Do not label price freshness as correctness.
- [ ] Record missing official contest URL/track and owner model as unverified inputs, not invented decisions.
- [ ] Document atomic initialization, supported migration restrictions, bypass-authority removal and an actual live receipt checklist.
- [ ] Run complete fast verification and the full serial sandbox suite after integration.
- [ ] Produce an English operational report with implemented/passing/blocked distinctions, exact commands and remaining live blockers.

Acceptance: local claims reproduce from a clean supported environment; live readiness is never inferred from fixtures.

## Integration/review gate

- [ ] Review every agent diff and reconcile APIs/file ownership.
- [ ] Run cargo fmt, strict Clippy, Rust unit/property tests and new JavaScript tests.
- [ ] Run actual funded settlement, gas and withdrawal lifecycle; then the full serial suite.
- [ ] Check HTML/JS/runtime behavior, allowed documentation references and local links.
- [ ] Freeze the reviewed artifact manifest and show the report in Codex.
- [ ] Before any live execution, present the exact network/accounts/source hash/transactions and unresolved risks for the final authorization step.

