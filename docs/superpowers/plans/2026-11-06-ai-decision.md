# Model decision implementation plan

> **For agentic workers:** Execute this user-authorized plan task by task with separate file ownership and review gates. Steps use checkbox tracking. Do not push, use live funds, bypass hooks, or inspect prohibited files.

**Goal:** Add bounded model decisions, independently checkable response commitments, declared-source admission policy, and adversarial regression evidence.

**Architecture:** Keep the existing vault engine authoritative and unchanged. An injectable off-chain decision source validates a compact final-block model response; signed responses are verified over their original wire bytes before a separate canonical commitment. The vault restricts declared source IDs, without claiming authenticated model origin.

**Tech Stack:** Existing Rust workspace, near-sdk 5.30, near-api 0.8.7, reqwest, serde, SHA-256, Tokio. Step 2 adds exact secp256k1 0.27.0 and sha3 0.10.9 pins only for documented EIP-191 verification; both already exist in the dependency graph.

## Global constraints and accepted clarifications

- Feature branch: `feat/ai-decision`, initially based on `origin/main` at `bb0b30b64bc436eaadfc31e732971778a28f7725`.
- One passing, reviewed commit per requested step. No push. No hook bypass.
- Do not change `engine.rs` gate logic, limits, ordering, rounding or numeric constants.
- Never read prohibited filenames or credential/configuration directories. Exclude all contents of `contracts/vault/tests/common/mod.rs` from review and hashing; its previously identified sensitive line is never reproduced.
- No model test calls a network. Fake clients control responses/failures. Existing sandbox and local protocol tests retain their assertions and explicit fixture labels.
- Runtime provider endpoint, model name and API authentication come exclusively from environment configuration; no real environment values are inspected during development.
- User-approved policy: `allowed_model_ids` limits **declared source IDs**, not independently authenticated model origin. A compromised agent can still claim an approved ID.
- Reserved non-model source IDs: `manual-v1` and `rebalance-v1`. Their existing SHA-256(rationale UTF-8) semantics remain unchanged.
- Only model decisions use the canonical response-body commitment after Step 2.
- Migrated active and queued mandates have empty source lists. Empty is valid and denies every proposal; adding the first ID waits 86,400 seconds.
- Model rationale is untrusted text, never a command, query, trade parameter, policy instruction or authority.
- Stored signatures, publication acknowledgements and settlement remain separate.

## Task 1: Injectable decisions and strict model validation

**Files:** Create `agent/src/decision.rs`, `agent/src/model.rs`, `agent/tests/model.rs`; modify only agent exports, CLI and behavior documentation.

**Interfaces:**

```rust
pub trait DecisionSource {
    fn decide(&self, snapshot: &crate::autonomous::Snapshot)
        -> impl std::future::Future<Output = Decision> + Send;
}
pub enum Decision {
    Trade { plan: crate::Plan, response: Option<crate::model::ChatResponse> },
    NoTrade { reason: String },
}
```

`ChatClient::complete` returns a captured response or a sanitized typed failure. `ModelDecision<C>` accepts a fake or HTTP client. Legacy adapters delegate existing logic; the existing manual plan shape and rebalance quote tuple are retained.

- [x] Read the brief and claims; resolve source-label and compatibility contradictions directly with the user.
- [x] Confirm the requested branch and unchanged base; run baseline formatting, strict Clippy and workspace library tests.
- [x] Test fake valid, malformed, unknown-field, duplicate-field, non-mandate, same-asset, zero/negative/overflow amount, oversized rationale, timeout, HTTP-failure and explicit no-trade responses.
- [x] Assert paused/halted readiness produces zero client calls.
- [x] Enforce exactly seven decision fields, decimal u128 strings, positive amounts and rationale 1..800 Unicode characters. Validate before any proposal; never repair or loop-retry.
- [x] Build compact snapshot JSON from final block identity/time, asset configuration, balances, prices/publication times, mandate and trading state. Authentication never enters this body.
- [x] Implement HTTPS-only authenticated transport without redirects, bounded response bytes and a request deadline; zero automatic retries.
- [x] Wire `--model [--dry-run]`; read readiness before model construction; refresh the snapshot and rerun the unchanged engine before paid submission. No model loop.
- [x] Run new fake-client tests, existing agent libraries/autonomous/protocol tests, formatting and all-target strict Clippy. Review legacy Plan/hash and rebalance behavior.
- [x] Existing commit: `40c5ec1` — optional model decision source.

Step 1 evidence: `docs/evidence/ai-decision-step1-fast.txt` records fmt/strict Clippy/workspace libraries/autonomous/model successes and the initial five loopback permission failures. `docs/evidence/ai-decision-step1-local.txt` records the authorized local rerun: protocol 7/7 and sandbox agent 5/5 passed. Development compiler failure and repair are retained separately.

## Task 2: Response evidence and offline verifier

Existing commit `dd98a81` implemented raw response commitments and offline-only
signature checks. The user explicitly reaffirmed canonical commitments and
pre-proposal signature refusal on 2026-11-07. Preserve existing history and add a
focused corrective commit after fresh checks. `b43569d` contains separate F26
residual-risk documentation; preserve that disclosure. Old completion claims
are not verification evidence for the corrected snapshot.

**Files:** Add focused agent evidence/verifier modules and public-only fixtures/tests; update CLI and `docs/ARCHITECTURE.md`. Add only the two exact crypto dependency pins listed above.

**Consumes:** Captured original request and response UTF-8 bytes, the strictly validated decision, provider-returned response/model IDs, final block identity and configured external signer expectation.

**Produces:** A raw-response-SHA256-named evidence entry, model-specific canonical `reason_hash`, and offline verifier output separating commitment, signature, tuple comparison, chain authenticity and settlement.

- [x] Preserve the exact request body sent and response body received; exclude headers/authentication/environment dumps. Write evidence before paying for a proposal.
- [x] For required NEAR EIP-191 mode, retrieve the separate documented signature with a bounded request and no unsigned fallback. Unsupported or invalid proof means no proposal.
- [x] Require signed text to equal the exact original request/response SHA-256 pair and returned model where the documented shape includes it.
- [x] Verify against an independently configured public signer expectation; never trust the identity asserted by the response itself. Hardware attestation remains unimplemented and unclaimed.
- [x] Define `mandate-response-json-v1`: whole response envelope, recursively UTF-8-byte-sorted object keys, preserved arrays/string contents, compact deterministic JSON, no trailing newline, duplicate keys and float/exponent/unsupported numbers refused. It is not RFC8785.
- [x] Test an independently checked public EIP-191 fixture without storing any private signing material, plus request/response/model/signature/signer/tuple mutations, unsigned mode, required lookup failure, duplicate keys and canonical-equivalent/wire-different inputs.
- [x] Verify a supplied on-chain hash as a commitment value only; authenticate no chain or settlement merely from that argument. Compare exact proposal fields when supplied.
- [x] Preserve legacy rationale hashes and document generic unsigned responses as UNVERIFIED.
- [x] Run all Step 1 checks plus the new verifier fixture tests; review exact hash/signature semantics.
- [ ] Corrective commit: `fix(agent): bind and verify model decisions`.

Step 2 correction evidence: fmt and strict Clippy passed after recorded formatting/lint repairs; workspace libraries235, autonomous6, model35, provenance15, CLI7, protocol8 and sandboxagent5 all passed (311 distinct tests). Complete outputs are in `docs/evidence/ai-decision-step2-correction-fast.txt`, `ai-decision-step2-correction-protocol.txt` and `ai-decision-step2-correction-sandbox.txt`. Public vector verification is separate; actual provider/hardware/live-chain verification remains unperformed.

## Task 3: Declared-source policy and Borsh migration

**Files:** `contracts/vault/src/mandate.rs`, `lib.rs`, `migration.rs`; affected agent admission helpers, strict frontend schema compatibility, safe test fixture builders, and relevant sandbox/gas tests. Keep `engine.rs` and `intent.rs` unchanged.

**Consumes:** Declared model or reserved non-model source ID and the corresponding reason commitment.

**Produces:** Proposal model/source identity persisted on chain; strict bounded policy; fifth prior-layout migration with empty active/queued policy lists.

- [ ] Append `allowed_model_ids: Vec<String>`; permit empty deny-all, maximum eight distinct nonblank IDs, each at most 64 UTF-8 bytes.
- [ ] Extend `propose` with `model_id: String`; check membership before proposal ID/state mutation.
- [ ] Persist the ID in `Proposal`; recheck current policy before `execute` acquires an execution ID/lock. Retain revision fencing for in-flight unsigned work.
- [ ] Addition of any absent ID returns `allowed_model_ids` as a loosened field; removal/reordering/equality do not. Test replacement, mixed changes and exact 24-hour application boundary.
- [ ] Freeze pre-change nested Mandate/Proposal/PendingMandate representations for all four historical decoders. Add a decoder for the previous current voucher layout; preserve provider, monotonic IDs and control state.
- [ ] Preserve every quiescence/paused/distinct-owner/active-guard check and immutable historical fixture hash. A sealed deployment is not silently owner-upgradeable.
- [ ] Use empty lists for every migrated active/queued mandate; verify all proposals refuse until explicit owner addition has matured. Never grant reserved IDs automatically during migration.
- [ ] Fresh test deployments explicitly authorize reserved IDs; retain existing test assertions. Update only safe common-builder contexts without inspecting or reproducing its sensitive line.
- [ ] Measure supported-size gas; record observed values and exact artifact identity rather than assuming earlier figures still apply.
- [ ] Run native policy/migration tests, existing client regressions and affected serial sandbox targets; strict formatting/Clippy must pass.
- [ ] Commit: `feat(vault): restrict declared decision sources`.

## Task 4: Adversarial model integration and final verification

**Files:** Focused existing sandbox/agent/attack tests, `scripts/verify.sh`, updated threat-model/README/architecture sections, final English evidence report.

- [ ] A fake model's injected rationale changes no mandate, receiver or trade parameter; an over-limit tuple is refused by the actual vault with the exact gate code.
- [ ] A large harmful tuple within every configured limit is allowed; name the test explicitly as residual-risk documentation and assert the signed outcome.
- [ ] Unknown source ID is refused with unchanged pending/sequence state. Removing an admitted source before execution also blocks it.
- [ ] Paused and halted cases make zero fake model calls; no test accesses a real provider.
- [ ] Run, retain full outputs and report actual failures for:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --lib
./scripts/verify.sh --integration
cargo test -p mandate-agent --test model
```

- [ ] Run the added verifier fixture target and all existing manual/rebalance regression targets. Count distinct results, never reruns or historical logs.
- [ ] Independently review diffs; confirm unchanged engine/limits and no excluded paths staged. Keep live chain, external attestations and provider execution explicitly UNVERIFIED.
- [ ] Commit: `test(model): prove bounded adversarial decisions`.
- [ ] Deliver A–E summary with four actual commit hashes, complete test evidence, reviewed design decisions, remaining limitations and exact branch-review commands.
