# F26 Durable Recovery Implementation Plan

> **For agentic workers:** Implement this plan task-by-task with delegated workers and independent review. Steps use checkbox tracking. The user has authorized implementation in the current dirty feature checkout; preserve all earlier hardening and user edits. Do not commit, publish or deploy as a workflow side effect.

**Goal:** Implement authenticated, idempotent withdrawal recovery across the custody executor, vault and keeper, without treating an unknown payment as a failed payment.

**Architecture:** A custody-executor journal must atomically bind the authenticated vault, never-reused withdrawal ID and exact request to debit/dispatch, and persist authoritative terminal results in the actual resolver. The vault reconciles that record through authenticated cross-contract reads. An ID-bound recovery interface and conservative keeper transaction handling are independently implementable compatibility foundations for the existing voucher protocol.

**Tech Stack:** Rust 1.95, NEAR SDK 5.30, NEAR API 0.8.7, current pinned Cargo.lock, native regression tests and serial actual-WASM sandbox tests.

## Global constraints

- Keep `unique_voucher_v1` and every existing migration, ceiling and custody guard compatible. No timer, balance-only reset, owner abandon or unknown-payment resend.
- `DebitObserved` spends an authorization but proves neither terminal delivery nor completed refund.
- The existing Intents WASM is an external fixture, not source-owned executor code. Preserve its reviewed bytes and fixture checks.
- The upstream source reference is `near/intents` commit `32cd1f9788a347908164fbcc56525dc6542f158f`. Its correspondence to the existing fixture is unverified.
- The user supplied `/private/tmp/f26-intents-32cd1f9.tar.gz` after automatic approval review failed before the public download. Its archive SHA256 is `0c9073d58953d06850b408800047a0f061194e925c9ea1c2259e3910f18c24d0` (1,324,342 bytes). The source and pinned public dependencies are available. Final official cargo-near artifact and actual-WASM asynchronous verification are tracked separately; host ABI packaging and localhost runtime limits remain open.
- A forwarding proxy or a mock journal cannot be reported as the real executor implementation. An executor resolver that never establishes terminal facts remains unresolved unless independently verified final-receipt proof establishes them.
- Public recovery may invalidate an old preparation or reconcile/cancel the same dispatched attempt. It never calls owner rearm or schedules a new payment automatically.
- Paid transaction ambiguity must stop new automatic signing until it is reconciled. Unknown/not-found status is not permission to resubmit.

## Task 1: Pin the protocol boundary

**Files:** Create `docs/design/f26-durable-executor.md`; preserve `contracts/vault/tests/fixtures/intents-near-mainnet.wasm`.

**Interfaces:** Source-overlay executor methods `withdraw_with_id(request)`, `cancel_withdrawal_with_id(request)`, `get_journal_withdrawal_status(vault_id, withdrawal_id)` and `get_withdrawal_journal_protocol()`. Their exact request, result binding and terminal definitions are specified in the design document; none is claimed to be an existing Intents export.

- [x] Record atomic request/debit scheduling, permanent deduplication and cancellation tombstones.
- [x] Define exact completion conditions, including completed refund mutations and late-receipt behavior.
- [x] Document actual-executor integration and final-receipt proof requirements, historical operation exclusion, source trust, migration, storage/gas and test cases.
- [x] Check local links and claims; an independent reviewer checks that no wrapper claim or time-based guard release slipped in.

## Task 2: Add ID-bound recovery without changing stored state

**Files:** Modify `contracts/vault/src/lib.rs`, `contracts/vault/src/withdrawal.rs`.

**Consumes:** Existing `ActiveWithdrawal`, `WithdrawalStage`, `WITHDRAW_RECOVERY_SEC`, `MIN_WITHDRAWAL_RECOVERY_GAS_TGAS` and `recover_withdrawal()`.

**Produces:** One atomic public snapshot and a stale-attempt-safe permissionless recovery method:

```rust
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithdrawalRecoveryState {
    pub protocol: String,
    pub withdrawal_id: U64,
    pub stage: WithdrawalStage,
    pub started_sec: u64,
    pub recover_after_sec: u64,
    pub required_gas_tgas: u64,
}

pub fn get_withdrawal_recovery_state(&self) -> Option<WithdrawalRecoveryState> {
    let active = self.active_withdrawal.as_ref()?;
    let stage = self.withdrawal_stage.expect("active withdrawal has no phase");
    Some(WithdrawalRecoveryState {
        protocol: self.get_withdrawal_protocol(),
        withdrawal_id: active.id,
        stage,
        started_sec: active.started_sec,
        recover_after_sec: active.started_sec.saturating_add(WITHDRAW_RECOVERY_SEC),
        required_gas_tgas: MIN_WITHDRAWAL_RECOVERY_GAS_TGAS,
    })
}

pub fn recover_withdrawal_attempt(&mut self, withdrawal_id: U64) -> PromiseOrValue<bool> {
    if !self.active_withdrawal.as_ref().is_some_and(|active| active.id == withdrawal_id) {
        return PromiseOrValue::Value(false);
    }
    self.recover_withdrawal()
}
```

- [x] Add native regressions using the existing `preparing()` and `dispatched()` fixtures. Assert absent snapshot, exact snapshot, stale-ID/no-active no-op, identical stored bytes and no receipts for a stale ID.
- [x] Assert early matching attempt still refuses, mature preparation invalidates only that ID and mature dispatched recovery enforces the existing 140-TGas minimum.
- [x] Implement the interfaces without adding fields to `Vault` or changing unbound legacy recovery.
- [x] Verify the vault library in the final locked workspace native run (285 vault tests passed); independently inspect that stale calls never produce a payment or affect newer IDs.

## Task 3: Recover with keeper, preserving the ambiguity boundary

**Files:** Modify `keeper/src/lib.rs`, `keeper/src/main.rs`; add focused tests under `keeper/tests/`; update `keeper/examples/README.md`.

**Consumes:** `get_withdrawal_recovery_state()` JSON exactly as Task 2 and existing `get_chain_time_sec`, network preflight and signed-intent publication.

**Produces:** Recovery observation/results distinct from trade results. Eligible recovery uses `recover_withdrawal_attempt({"withdrawal_id":"ID"})`, zero deposit and 140 TGas. Old versions or unknown protocol/snapshot shape are rejected, not silently guessed.

- [x] Strictly validate the atomic snapshot: supported protocol, U64 ID, known phase, integer times, exact supported gas and recovery delay.
- [x] Publish already-issued fresh signatures first; when an active withdrawal exists, do not submit `execute` or automatically apply/rearm a queued request.
- [x] Repeat network preflight immediately before a paid recovery call. Bind every call to the observed ID so a raced newer attempt is a no-op.
- [x] Introduce a per-ID cooldown for known unresolved recovery results; public transport failures use bounded backoff. A transport failure or malformed result after paid submission is an explicit fail-stop, not a retry of the payment.
- [x] Either retain an exact public transaction hash before broadcast in a durable operation journal with single-writer exclusion, or use the initial conservative operator-reconciliation fail-stop. Never persist private/signed transaction bytes. A restart must not be described as resolving an unknown transaction automatically unless a durable journal actually exists.
- [x] Mock-RPC/binary tests exercise eligibility, early waiting, wrong chain, stale IDs, false recovery, changed ID, malformed snapshot, ambiguous paid result and absence of apply/rearm/payment calls.
- [x] Verify keeper native regressions (8 passed), strict all-target workspace Clippy and independent paid-error/finality review.
- [ ] Execute the full keeper HTTP/binary recovery target on a host where localhost can bind; the current HTTP setup limit does not establish those runtime passes.

## Task 4: Implement in the custody executor after source becomes available

**Consumes:** The pinned source archive and the reviewed protocol specification. Existing public fixture remains a legacy compatibility artifact.

- [x] Inspect the real `contracts/defuse/src/contract/tokens/nep141/withdraw.rs`, resolver, state layout, public traits and build inputs before defining the executable patch.
- [x] Bind the first dispatch and permanent journal entry atomically inside custody. A same-ID changed request refuses; exact duplicates cannot debit or schedule again.
- [x] Persist terminal amount/result with the actual refund/resolver mutation. Cancellation must irreversibly fence all delayed originals before dispatch.
- [x] Build and pin the new executor artifact separately. Do not repoint the vault to an unimplemented method or silently replace the reviewed fixture.
- [x] Add a fresh, explicitly selected vault protocol with exact request/status validation and authenticated reads. Existing unresolved pre-journal sends cannot migrate into an invented result.
- [x] Author and no-run compile two additional maximum-input grouped cases. Current scope is 11 asynchronous cases plus one artifact-only native check; preserve the original nine. Executor/vault maximum tuples, 20 admissions, underfunded-account fixture and 100/30/280/140-TGas paths are authored, not measured runtime results. Final all-target Clippy and native artifact binding passed; production/shared source and artifacts are unchanged.
- [ ] Actual-WASM tests use the real modified custody executor for lost vault callback, duplicate/delayed sends, cancellation races, refund completion, masked credits, stale IDs, malformed/missing status, late resolver, source/account mismatch and storage/gas boundaries.
- [x] Record remaining build/runtime blockers without declaring full F26 closure. Source and dependencies are available; final asynchronous and host ABI verification remain separate gates.

## Task 5: Verify and reconcile evidence

- [ ] Run all tests appropriate to changed source and integration targets in serial; record actual counts and command exits.
- [x] Run `cargo fmt --all -- --check`, strict all-target Clippy, `bash scripts/check-claims.sh`, `python3 -B scripts/build-web.py --check`, and `git diff --check`.
- [x] Identify the tested source/lock/artifacts and state exactly which executor path was executed. Reopen affected tests/release gates when source changes.
- [x] Update F26 documents and user-facing status without claiming universal liveness, mainnet readiness, external audit, or zero defects.

## Initial review

The atomic executor journal is not implementable by editing this vault or its external WASM fixture alone. The user supplied the source, enabling actual custody-executor implementation rather than a forwarding proxy.

## Source-specific implementation decisions

- Shared types and canonical commitments live in `crates/withdrawal-journal`; executor and vault consume that exact crate.
- The executor's `nep141/journal.rs` is a source overlay on the pinned upstream revision, packaged under `integrations/intents-executor`. Domain, immutable admitted vaults and padded operation records use separate raw-storage namespaces; the upstream root Borsh layout is preserved.
- Executor initialization is fresh-only `new_with_withdrawal_journal(config, ledger_domain, authorized_vaults)`. The immutable admission list contains 1–20 distinct vaults; no arbitrary predecessor can allocate tombstones at executor expense. Normal debits/refunds use the actual upstream `withdraw`/`deposit` custody helpers.
- New withdrawal requires 100 TGas; cancellation requires 30 TGas. The ordinary token transfer receives 30 TGas and its identified private terminal resolver receives 40 TGas. Every pending record prepays a fixed terminal slot and a separate refund-storage reserve. Actual intended-token and target-chain costs remain measured release requirements.
- Vault selection is fresh-only `new_with_journal(owner, mandate, journal: JournalVaultConfig)`. The immutable descriptor binding uses separate raw storage, preserving the existing root Borsh layout and legacy migration behavior.
- Journal `Preparing` schedules only authenticated descriptor and admission reads. Matching descriptor/domain/admission, current attempt, deadline, authorization generation and ceiling must pass before setting `Dispatched` and scheduling `withdraw_with_id`. An old preparation callback cannot send after timeout or replacement.
- Recovery uses the same ID-bound API and 140 TGas. `Preparing` can be invalidated locally after 300 seconds; `Dispatched` calls idempotent cancellation and then reads authoritative status if necessary. Only a matching terminal record clears that guard; missing/malformed/Pending status does not.
- Initial callback reports actual delivery; recovery callbacks report guard resolution. A terminal refund/cancellation preserves the matching request unarmed for explicit owner retry under a fresh ID. Legacy voucher callbacks cannot affect journal-mode state.
- Keeper accepts the two known phases for both explicitly supported protocols, retains its paid-transaction hash before broadcast, and refuses new signing/restart after an ambiguous paid outcome.

## Current verification boundaries

The pre-journal hardening checkpoint's 479 tests and original vault artifact
are historical and cannot be counted as coverage of this implementation.
Focused native/shared/client/preparation checks have separate run evidence;
keeper transaction finality changes reopen their affected tests.

The executor package vendors original-byte near-plugins 0.5.3 (CC0-1.0), pins
its archive and every original/staged file hash, and changes only the staged
normalized manifest to disable SDK defaults. Source preparation and reuse
reject hash changes and edited source. The reviewed lock changes registry
near-plugins to the path override and removes its disabled allocator family
without upgrading external versions. Official cargo-near optimization produces
`target/intents-executor/artifact/defuse.wasm`; the raw release intermediate
is not the runtime test artifact.

On the current Apple SDK/linker, ABI dylib extraction/loading fails. The
sandbox helper has an explicit `MANDATE_SANDBOX_NO_ABI=1` option that preserves
optimization and does not change the default ABI build. ABI packaging needs
verification on a compatible compiler/container. Localhost sandbox/RPC binding
and approval-review infrastructure also block executed asynchronous evidence.
No artifact rewriting or automatic verification bypass is permitted. The
[reproduction commands](../../OPERATIONAL-READINESS.md#current-host-verification-limit)
are ready for a host where localhost is available. The official optimized artifact and build manifest are recorded in the
[current checkpoint](../../audit/F26-DURABLE-2026-11-08.md). Full affected checks,
asynchronous coverage and independent audit remain gates; Mainnet remains NO-GO.
