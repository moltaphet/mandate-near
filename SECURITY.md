# Security

## Status

Mandate is an **experimental prototype, not professionally audited**. The
historical self-review below is supplemented by an adversarial code review
and regression work. Local tests and mainnet signature simulations do not
establish safe live custody, funded settlement or failure recovery.

The pre-journal 8 November hardening candidate passed its configured local components:
479 distinct tests, including all 14 sandbox targets, plus formatting, the
workspace all-target check and strict Clippy. The [hardening report](docs/audit/HARDENING-2026-11-08.md)
records exact artifact identity and the two stale test-expectation failures
followed by full affected-target reruns. The final verify.sh wrapper was blocked
by approval review infrastructure; its scope passed through direct commands.
Frozen-release CI, reproducible-build and live custody evidence remain required.

The subsequent fresh-only `durable_withdrawal_journal_v1` source implementation
adds a shared commitment crate, native Intents custody/resolver journal, vault
adapter and durable keeper ambiguity fence. Its final source-v4 artifact and
source/lock bindings are recorded in the current checkpoint; its 11
asynchronous cases remain unexecuted, including two newly authored/compiled
maximum-input groups. The earlier 479 tests do not cover it. A recorded executor terminal result can recover a lost vault notification.
An executor resolver that never completes can leave `Pending` indefinitely;
there is no implemented finalized-receipt proof verifier. See the
[protocol and gates](docs/design/f26-durable-executor.md) and
[current checkpoint](docs/audit/F26-DURABLE-2026-11-08.md).

The supported custody surface is NEP141 assets, at most 20 mandate entries.
A vault must have a distinct recoverable owner and a name of at most 62 bytes;
withdrawal to the vault or
Intents contract is unsafe and must be rejected. A signed intent means a
signature was obtained, not that a trade settled.

## Reporting a vulnerability

Do not open a public issue for a security problem.

- Once the repository is public, use GitHub's private vulnerability reporting
  on this repo (Security tab, "Report a vulnerability").
- Until then, contact the maintainer through their GitHub profile.

We aim to acknowledge a report within 3 days and to publish a fix or a
mitigation before any public disclosure.

## Scope

In scope for the current candidate: `contracts/vault`, the shared
`crates/withdrawal-journal`, `integrations/intents-executor` preparation and
native custody/resolver overlay, the keeper (`keeper/`) and the script agent
(`agent/`) as far as they can cause a wrong on-chain decision. Freeze the exact
candidate revision and both built artifacts for an independent audit.

Out of scope: `intents.near`, `v1.signer`, `price-oracle.near`, Pyth receiver
correctness and administrative roles, and NEAR consensus. These are external
and trusted as described in
[THREAT_MODEL.md](THREAT_MODEL.md#actors-and-trust).

## Self-review findings

The current adapters are immutable PriceOracle or Pyth choices. Pyth checks
signed values, feed format, freshness and confidence against the slippage
allowance; it remains trusted for market correctness. Current live cache/updater
and deployment readiness are recorded in [docs/OPERATIONAL-READINESS.md](docs/OPERATIONAL-READINESS.md).
The oracle is not yet selected. Pyth Core is documented as no longer supporting
NEAR, so the choice remains open: a verified feed source, units and updater must be
demonstrated before deployment or funds.

Each finding has an ID so the threat model and the code can refer to it.

| ID | Severity | Where | Finding | Status |
|---|---|---|---|---|
| SR-1 | High (state and availability) | `src/lib.rs`, `execute` and callbacks | Execution identity must expire with its authority: an old callback must not clear a new proposal or commit state approved under changed rules. A failed receipt can also leave a lock. | Execution IDs, proposal identity, configuration revision and time checks guard callbacks. The build window is `EXECUTION_WINDOW_SEC` (300 s); the outer lock includes `INTENT_TTL_SEC` (120 s). Every post-request signing error retains the timed lock, including a Failed Promise or oversized/malformed answer, because a signature may already exist. Turnover, cooldown and high-water state are reserved from the approval snapshot before signing dispatch, with no automatic refund for failed signing, lost answers or unfilled intents. Read/salt and pre-sign configuration/deadline failures can release the execution lock. Regression names include `a_failed_signer_call_keeps_the_timed_lock`, `oversized_successful_signer_answer_keeps_the_timed_lock` and `signing_dispatch_reserves_quota_even_if_the_result_arrives_after_lock_expiry`. Live failure/timeout recovery remains unverified. |
| SR-2 | Low | `src/lib.rs`, `propose` | `propose` did not check the in-flight lock. A proposal made while an execution was in flight could be dropped when that execution finished, because `on_signed` and `reject` clear `pending`. | **Fixed.** `propose` refuses while an execution is in flight. Unit test `a_proposal_waits_for_the_execution_in_flight`. |
| SR-3 | Medium (liveness) | `src/lib.rs`, `execute` and `on_price_data` | An underfunded permissionless execution could consume a proposal and leave a timed lock when a later callback exhausted gas. Historical PriceOracle measurements also motivated bounding feed count. | `execute` requires 300 TGas before proposal consumption or execution-state mutation. `MAX_ASSETS` is 20. [Historical gas record](docs/evidence/gas-limit.txt) explains the feed bound; the 8 November artifact passed supported-size PriceOracle/Pyth sandbox regressions and low-gas refusal/retry. Pyth uses a batch rather than one report call per feed. Intended-chain budgets still need verification. |
| SR-4 | Info | `src/lib.rs`, `execute` | `execute` is permissionless, so a stranger can choose the moment a closing gate becomes a strike. Three strikes halt proposals until the owner calls `resume_proposals`. | **Accepted.** Availability only. |
| SR-5 | Info | `src/lib.rs`, `set_agent` | A later `set_agent` took effect at once. A stolen owner key could point the vault at a new agent. Trades still pass every gate. | **Fixed in code.** A later agent is queued for `AGENT_DELAY_SEC` (24 h), like a loosened mandate. The current agent keeps its rights meanwhile, and `cancel_pending_agent` drops the change. The first agent applies at once, since no agent can propose before it is set. `a_new_agent_waits_24_hours_on_a_real_chain` checks queuing, early refusal and cancellation in sandbox; it does not advance 24 hours or apply a matured replacement. Unit tests cover controlled-time boundaries. Live post-delay application remains unverified. A DAO or multisig owner is still recommended before real funds. |
| SR-6 | High (custody and recovery) | `src/lib.rs`, `src/withdrawal.rs` | Owner withdrawal must retain authorization across send/refund callbacks; an unknown result must not be blindly retried. A successful zero return can mean an underlying FT transfer failed and Intents refunded the ledger. | `unique_voucher_v1` issues one unique unit per never-reused attempt ID, records `Preparing` before issuance, and uses one marker-first/withdraw-last Intents batch in `Dispatched`. Full terminal amount records delivery; zero/refund can requeue only the matching request generation. After 300 s, pre-dispatch preparation can be invalidated or a dispatched voucher raced to cancellation. Positive sent/cancelled proof releases the guard; missing or inconsistent proof retains it. Safe retries require explicit owner rearming, preserving maturity and using a new ID. Sent proof records initial `DebitObserved`, not terminal delivery; later FT resolver/refund receipts can overlap subsequent operations. Local protocol regressions passed on the 8 November artifact. Sink deployment trust and intended-chain recovery remain release blockers. |
| F26 | High (availability) | `src/withdrawal.rs`, `src/journal_withdrawal.rs`, native executor journal | Missing, failed or inconsistent proof retains the guard, blocking new withdrawal dispatches and trades. Journal mode resolves only exact authenticated Delivered/Refunded/Cancelled records; a missing executor resolver can leave Pending indefinitely. Time, a larger balance and owner authority cannot override it. The original transfer may already have moved funds. | Accepted for the current prototype/testnet scope only; real funds require separate review. Lost vault notification can be queried after terminal executor commit, but no finalized-receipt proof verifier exists and unknown legacy sends gain no retrospective truth. Final asynchronous and live evidence remain required. |
| SR-7 | Info | `src/oracle.rs`, `src/engine.rs` | The configured oracle is trusted for market correctness. Freshness, slippage and size checks use that oracle and establish no independent loss bound against a fresh wrong price. | Mainnet provider, feed/source and administrator identity remain UNVERIFIED. See [THREAT_MODEL.md](THREAT_MODEL.md#t6-oracle-wrong-manipulated-or-stale). |
| SR-8 | Info | `pause`, `propose` | `pause` and `propose` take no 1 yoctoNEAR, so a function-call key can call them. This is deliberate: a kill switch must work at once, and the agent's key should be a function-call key. | **Accepted by design.** |
| SR-9 | Critical if violated (deployment) | deployment procedure | The vault account must have no bypass-capable access permission, including direct signer-call authority. Initialization must be atomic; prior canonical voucher issuance can invalidate recovery. | **Open live prerequisite.** Prefer fresh atomic account creation/deployment/initialization without adding access permissions. Current permission absence does not establish prior issuance/refund history. Check exact artifact, distinct owner, sinks and complete history before funds. |

The oldest approved-price deadline is recorded in execution state and checked
again before requesting a signature. `salt_cannot_sign_after_the_approved_prices_become_stale`
and `salt_can_sign_at_the_exact_price_age_boundary` exercise strict-later
rejection and equality acceptance. An unrecorded approval cannot sign.
This establishes price age at the request, not at settlement.

**Open economic residual:** drawdown uses absolute pre-trade NAV, without
cash-flow adjustment. A confirmed owner withdrawal reduces ledger NAV without
reducing the stored high-water mark and can trigger de-risk-only trading despite
no market loss. Final economic policy must address this before real-fund use;
the withdrawal ceiling does not change this accounting.

`vault_withdrawal_reads_sends_and_confirms_the_actual_transfer` exercises the
actual read/send/confirm path against the sandbox fixture. The 6 November
170 TGas validation was for the pre-voucher implementation. The 8 November
artifact passed the 280 TGas path and all configured local protocol tests;
the hardening report records the commands and artifact identity.
Its logged maturity mode can be a sandbox-only STATE fixture when block/time
advancement is insufficient. This changes only the queued deadline and is not
proof that 24 real or chain hours elapsed. Live post-delay withdrawal and
unknown-result reconciliation remain required.

The voucher protocol requires `s.<vault>` and `c.<vault>` to be uncreated and
without implicit, registered or proxy signing authority before all bypass-capable
vault permissions are sealed. The vault does not create the sinks. An empty
Intents signing list alone does not prove no account can later be created;
a movable marker would invalidate cancellation proof. Unique voucher records
grow external Intents storage. One permissionless attempt is spent per owner
authorization; a safe retry requires owner predecessor authorization and exactly
1 yoctoNEAR through `rearm_pending_withdrawal`.

Before dispatch, `mt_supply` must report exactly one unit globally, source must
hold one, and both sinks must hold zero. Recovery additionally requires supply
one, source zero and an exclusive positive destination. These checks reject
preseeded duplicate vouchers held anywhere. They do not authenticate completion
of pre-seal voucher issuance/burn/resolver/refund receipts; a late prior mint can still break
uniqueness. Establish that history before funds, preferably through fresh atomic
account creation/deployment/initialization without adding access permissions.

The reference clients include a deterministic two-asset strategy, an optional
strict model decision source and explicit Message Bus publication. Local gate
preview does not replace on-chain checks.
Quote metadata is tied to the exact proposal/vault/verifier/input/output;
publication acknowledgement and even relay-reported `SETTLED` remain unverified
settlement observations. Autonomous dry-run loads no signing or relay
authentication settings. Model dry-run calls its configured provider with
provider authentication, but submits no NEAR transaction. Model output is
untrusted; a valid pinned response signature establishes no hardware attestation
or authenticated model origin. The vault's on-chain gates remain authoritative.

Declared decision-source admission is bounded to 8 unique nonblank IDs of at
most 64 UTF-8 bytes. Empty denies all, including reserved `manual-v1` and
`rebalance-v1`. Owner additions use the existing 24-hour mandate delay; removals
apply immediately. `propose` checks membership after predecessor-agent
authentication; `execute` checks the stored ID again before acquiring a lock.
Configuration revisions fence callbacks from an earlier mandate. Pre-model
migrations initialize empty active and queued source lists; pre-ceiling migration
preserves already-authorized sources. No historical proposal receives an invented
source ID. This enforces declarations, not cryptographic model origin. Native tests in
`mandate.rs`, `lib.rs` and `migration.rs` cover bounds, delays and refusal; final
combined verification is recorded separately.

Per-asset `withdrawal_limits` contain positive `U128` base-unit ceilings and
apply to each request at queue, mature apply and preparation before dispatch.
An omitted asset ceiling is uncapped. Adding/tightening applies immediately;
increasing/removing an existing ceiling waits 24 hours, even with asset removal.
There is no aggregate or daily withdrawal cap. Post-dispatch tightening does
not change terminal accounting or bypass unknown-result guards. Migrated old
mandates have no ceiling until one is configured.

### Dependency advisories

The 8 November 2026 OSV query flagged wee_alloc 0.4.5
(`RUSTSEC-2022-0054`, also `GHSA-rc23-xxgq-x27g`). NEAR SDK 5.30.0 enables
this allocator by default for Wasm contracts, so its dependency was applicable
to the production vault and sandbox mock binaries. The workspace now disables
the SDK's default features and uses Rust's standard Wasm allocator, without a
custom unsafe allocator or SDK patch. wee_alloc, memory_units, and their
otherwise-unused legacy dependency leaves are removed from the workspace lock.
The pre-journal locked graph and rebuilt artifact passed its configured local
scope, including both-provider maximum-policy gas and withdrawal/recovery
regressions. See [the pre-journal hardening report](docs/audit/HARDENING-2026-11-08.md)
for its source/WASM hashes. Intended-chain and reproducible-build evidence
remain separate requirements.

The source-owned executor needs a separate allocator check: its near-plugins
0.5.3 dependency is now vendored with original source bytes, and preparation
disables SDK defaults only in the staged normalized manifest. Original/staged
hashes and the archive pin are explicit; no registry/cache edit or external
version upgrade is part of this change. The final optimized executor WASM and
its asynchronous tests require separate recorded evidence.

The pre-journal user-run OSV query covered 491 public package/version pairs against
that checkpoint lock; [the saved result](docs/audit/DEPENDENCY-SCAN-2026-11-08.json)
has no Quinn/Zstd or removed allocator matches. Two native development findings
remain. `crypto-bigint` 0.5.5 (`RUSTSEC-2026-0387`) can take secret-dependent
branches; its upstream fix is 0.7.6, and production clients use 0.7.10. The older
test-VM path inspected here verifies public signatures. `unsafe-libyaml` 0.2.11
(`RUSTSEC-2024-0449`) is informationally unmaintained; serde_yaml →
`near-parameters` → the native NEAR SDK test stack parses trusted compile-time
embedded SDK YAML. Neither affected version appears in production client/WASM
normal-dependency trees. No project secret-timing or external-YAML exploit was
confirmed; maintenance risk remains open. No SDK fork or advisory ignore was added.

A `cargo audit` run on 12 November 2026 (RustSec database of 1,471 advisories, 498 locked
crates) reported the same two items and nothing else. Neither appears in the vault's
`wasm32-unknown-unknown` dependency tree; both reach only native agent/keeper builds through
the NEAR SDK's test VM, and the project signs with ed25519, not the affected P-256 code.

### Checklist results (`near-contract-audit`)

| Detector | Result |
|---|---|
| `non-private-callback` | Pass. Every `on_*` callback is `#[private]`. |
| `reentrancy` | Ordering reviewed: execution/send state precedes external calls. Identity, revision, expiry and withdrawal reconciliation need regression coverage; receipt-local rollback does not undo earlier receipts. |
| `incorrect-argument-or-return-types` | Pass. Token amounts cross the JSON boundary as `U128`; ids and nonces as `U64`. Timestamps are seconds, far below 2^53. |
| `unsaved-changes` | Not applicable. The contract uses no `near_sdk::store` collections. |
| `owner-check` | Owner methods, including withdrawal request/cancel/rearm, check predecessor through `assert_owner`. Applying queued changes, registering the Intents identity, executing and withdrawal recovery are permissionless by design. |
| `yocto-attach` | Pass with two deliberate exceptions, SR-8: `pause` and `propose`. |
| `storage-collision` | Journal binding, executor domain/admission and operation records use separate raw-storage namespaces; their prefixes and upgrade preservation require independent review. |
| `required-initialization-macro` | `new`, `new_with_externals`, `new_with_oracle` and fresh-only `new_with_journal` are `#[init]`; private `migrate` uses `#[init(ignore_state)]` and validates supported quiescent layouts. The source executor journal initializer is also fresh-only. |
| `gas-griefing` | Mandate asset iterations are bounded by `MAX_ASSETS` (20); account/feed lengths and response limits also matter. Supported-size gas regressions run with integration verification. |
| `insecure-random` | Pass. Randomness is used only to make the intent nonce unique, never for an outcome. |
| `prepaid-gas` | Callbacks use static gas. `execute` requires 300 TGas before consuming a proposal or mutating execution state. Voucher withdrawal apply requires at least 280 TGas; dispatched recovery requires at least 140 TGas. Preparing timeout invalidation after 300 s does not dispatch an external call. Supported-size and withdrawal/recovery regressions passed on the 8 November sandbox artifact; the selected intended-chain adapter still needs verification. |
| `cover-storage-cost` | Local input/entry bounds do not prove sufficient storage funding. Voucher entries and source-owned journal records/tombstones persist and grow with owner-authorized attempts. The source executor admits 1–20 immutable vaults and reserves padded terminal/refund storage before debit. Exact artifact/token costs and repeated owner rearming still need measured funding. |
| `unsafe-math` | Pass. The release profile sets `overflow-checks = true`. |
| `float-math` | Pass. No floating-point types. |

### Review of 12 November 2026

A line-by-line review after the testnet runs found these items. Each is listed with its state.

| Item | State |
|---|---|
| The testnet driver printed `transaction().get_hash()`, which near-api 0.8.7 computes over a rebuilt object, so recorded hashes were not transaction hashes | Fixed: the driver prints `outcome().transaction_hash`; all 65 recorded hashes mapped to real ones ([TX-HASH-CORRECTIONS.md](docs/evidence/TX-HASH-CORRECTIONS.md)) |
| A model plan could ask for far more than the market pays; approving it spends turnover and the cooldown and resets strikes | Fixed in the engine (`MinOutAboveMarket`, 2% margin) and in the agent. vault2 has no deployment keys and runs the earlier code; the gate applies to new deployments |
| The testnet mock oracle accepted price writes from any account | Fixed: `oracle.mandate-t1.testnet` runs the `parent-writes-only` build; a write from another account is refused on chain |
| Price feed had one source and no jump bound | Fixed: Kraken cross-check (2%) and a 10% jump limit against the on-chain report, both fail closed |
| CI actions were pinned by tag and cargo-near was installed with `curl \| sh` | Fixed: actions pinned by commit hash; cargo-near installed from its release binary with a SHA-256 recorded in the workflow |
| On testnet the agent account and the owner DAO's council account share one key | Open: needs a new, propose-only key for the agent and removal of the shared key. A leaked agent key would currently also control the DAO council vote |
| Removing a held asset from the mandate applies at once (a tightening), and that asset cannot be withdrawn until it is listed again, which waits 24 hours | Documented. Before removing an asset, withdraw it or confirm the vault holds none |
| `token_diff` signs an exact received amount; the real `intents.near` may take a protocol fee from it | Open for mainnet: gate 4 in [MAINNET-GATES.md](docs/MAINNET-GATES.md) |

## Before real funds

A short checklist, since the code alone does not enforce these:

1. Freeze and independently review the final vault/executor source, both WASM artifacts and the explicitly selected withdrawal protocol; bind completed local verification to that revision and rerun affected checks after changes. Prefer fresh deployment. Migration recognizes six layouts: `77ebc0c` pre-sequence, `0de5c48` sequence-enabled, pre-provider remediation, provider-enabled pre-voucher, pre-model `b43569d`, and pre-ceiling `a582d36`. All require paused quiescent state; an existing active withdrawal guard is never discarded. Migrated ceilings start empty and uncapped.
2. Demonstrate the selected DAO owner, distinct from the vault, including vote/execute/cancel/recovery. Verify intended signer/Intents/oracle deployments, administrative trust, a verified oracle with feeds, updater and asset units (the oracle choice is still open); an unselected oracle is not live readiness.
3. Use a vault name of at most 62 bytes; establish both recovery sinks are uncreated and have no signing authority or later creation path. Prepare atomic deployment/initialization and funded storage.
4. Compare deployed WASM with the reviewed artifact, register the intended Intents authority, demonstrate owner controls, and seal every bypass-capable vault permission. Reinspect final-block state before funds.
5. Demonstrate actual vault MPC signing and funded matched settlement with final receipts and ledger deltas. A relay acknowledgement or status is insufficient.
6. Demonstrate a genuinely elapsed 24-hour withdrawal, callback-loss races, confirmed refunds, owner-rearmed safe retries and exact receiver/ledger changes on the intended controlled deployment. Unknown pre-voucher sends cannot be retrofitted into this proof protocol. These requirements remain open; this is not a mainnet go recommendation.
