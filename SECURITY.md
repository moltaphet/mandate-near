# Security

## Status

Mandate is an **experimental prototype, not professionally audited**. The
historical self-review below is supplemented by an adversarial code review
and regression work. Local tests and mainnet signature simulations do not
establish safe live custody, funded settlement or failure recovery.

The 7 November baseline attempt before this documentation reconciliation exited
101: protocol tests could not bind loopback sockets (`PermissionDenied`).
Formatting and strict Clippy passed before the failure; later provenance targets
were not reached. Separate focused passes do not establish a complete fast or
integration pass.

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

In scope: `contracts/vault` on the `main` branch, the keeper (`keeper/`) and
the script agent (`agent/`) as far as they can cause a wrong on-chain decision.

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
| SR-3 | Medium (liveness) | `src/lib.rs`, `on_price_data` | Historical 4 November PriceOracle measurement: each held/traded report call reserved 8 TGas; 22 assets passed (72.51 TGas), while 24 and 32 failed with `Exceeded the prepaid gas` and retained the execution lock. | `MAX_ASSETS` is now 20. [Historical gas record](docs/evidence/gas-limit.txt) explains the limit; current PriceOracle and Pyth callback chains require supported-size regression runs. Pyth uses a batch rather than one report call per feed. Final revision and intended-chain budgets still need verification. |
| SR-4 | Info | `src/lib.rs`, `execute` | `execute` is permissionless, so a stranger can choose the moment a closing gate becomes a strike. Three strikes halt proposals until the owner calls `resume_proposals`. | **Accepted.** Availability only. |
| SR-5 | Info | `src/lib.rs`, `set_agent` | A later `set_agent` took effect at once. A stolen owner key could point the vault at a new agent. Trades still pass every gate. | **Fixed in code.** A later agent is queued for `AGENT_DELAY_SEC` (24 h), like a loosened mandate. The current agent keeps its rights meanwhile, and `cancel_pending_agent` drops the change. The first agent applies at once, since no agent can propose before it is set. `a_new_agent_waits_24_hours_on_a_real_chain` checks queuing, early refusal and cancellation in sandbox; it does not advance 24 hours or apply a matured replacement. Unit tests cover controlled-time boundaries. Live post-delay application remains unverified. A DAO or multisig owner is still recommended before real funds. |
| SR-6 | High (custody and recovery) | `src/lib.rs`, `src/withdrawal.rs` | Owner withdrawal must retain authorization across send/refund callbacks; an unknown result must not be blindly retried. A successful zero return can mean an underlying FT transfer failed and Intents refunded the ledger. | `unique_voucher_v1` issues one unique unit per never-reused attempt ID, records `Preparing` before issuance, and uses one marker-first/withdraw-last Intents batch in `Dispatched`. Full terminal amount records delivery; zero/refund can requeue only the matching request generation. After 300 s, pre-dispatch preparation can be invalidated or a dispatched voucher raced to cancellation. Positive sent/cancelled proof releases the guard; missing or inconsistent proof retains it. Safe retries require explicit owner rearming, preserving maturity and using a new ID. Sent proof records initial `DebitObserved`, not terminal delivery; later FT resolver/refund receipts can overlap subsequent operations. Sink deployment trust, final combined regressions and intended-chain recovery remain release blockers. |
| F26 | High (availability) | `src/withdrawal.rs`, `recover_withdrawal` | A withdrawal may already have been dispatched or debited when its final callback is lost or malformed. Missing, failed or inconsistent proof retains the guard, blocking new withdrawals and trades. Elapsed time, a larger balance and owner authority cannot override it. Guard retention initiates no additional transfer; the original withdrawal may already have moved funds. | Accepted by the project owner for the hackathon scope. Availability can be lost until authenticated proof resolves it. Receipt-loss fault not reproduced on a live chain; not a mainnet-readiness claim. |
| SR-7 | Info | `src/oracle.rs`, `src/engine.rs` | The configured oracle is trusted for market correctness. Freshness, slippage and size checks use that oracle and establish no independent loss bound against a fresh wrong price. | Mainnet provider, feed/source and administrator identity remain UNVERIFIED. See [THREAT_MODEL.md](THREAT_MODEL.md#t6-oracle-wrong-manipulated-or-stale). |
| SR-8 | Info | `pause`, `propose` | `pause` and `propose` take no 1 yoctoNEAR, so a function-call key can call them. This is deliberate: a kill switch must work at once, and the agent's key should be a function-call key. | **Accepted by design.** |
| SR-9 | Critical if violated (deployment) | deployment procedure | The vault account must have no bypass-capable access permission, including direct signer-call authority. Initialization must be atomic; prior canonical voucher issuance can invalidate recovery. | **Open live prerequisite.** Prefer fresh atomic account creation/deployment/initialization without adding access permissions. Current permission absence does not establish prior issuance/refund history. Check exact artifact, distinct owner, sinks and complete history before funds. |

The oldest approved-price deadline is recorded in execution state and checked
again before requesting a signature. `salt_cannot_sign_after_the_approved_prices_become_stale`
and `salt_can_sign_at_the_exact_price_age_boundary` exercise strict-later
rejection and equality acceptance. An unrecorded approval cannot sign.
This establishes price age at the request, not at settlement.

`vault_withdrawal_reads_sends_and_confirms_the_actual_transfer` exercises the
actual read/send/confirm path against the sandbox fixture. The 6 November
170 TGas validation was for the pre-voucher implementation; it does not verify
the new 280 TGas path. Final combined protocol tests must be rerun.
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
Configuration revisions fence callbacks from an earlier mandate. Migrated active
and queued lists are empty; no historical proposal receives an invented source
ID. This enforces declarations, not cryptographic model origin. Native tests in
`mandate.rs`, `lib.rs` and `migration.rs` cover bounds, delays and refusal; final
combined verification is recorded separately.

### Checklist results (`near-contract-audit`)

| Detector | Result |
|---|---|
| `non-private-callback` | Pass. Every `on_*` callback is `#[private]`. |
| `reentrancy` | Ordering reviewed: execution/send state precedes external calls. Identity, revision, expiry and withdrawal reconciliation need regression coverage; receipt-local rollback does not undo earlier receipts. |
| `incorrect-argument-or-return-types` | Pass. Token amounts cross the JSON boundary as `U128`; ids and nonces as `U64`. Timestamps are seconds, far below 2^53. |
| `unsaved-changes` | Not applicable. The contract uses no `near_sdk::store` collections. |
| `owner-check` | Owner methods, including withdrawal request/cancel/rearm, check predecessor through `assert_owner`. Applying queued changes, registering the Intents identity, executing and withdrawal recovery are permissionless by design. |
| `yocto-attach` | Pass with two deliberate exceptions, SR-8: `pause` and `propose`. |
| `storage-collision` | Not applicable. |
| `required-initialization-macro` | `new`, `new_with_externals` and `new_with_oracle` are `#[init]`; private `migrate` uses `#[init(ignore_state)]` and validates supported quiescent layouts. |
| `gas-griefing` | Mandate asset iterations are bounded by `MAX_ASSETS` (20); account/feed lengths and response limits also matter. Supported-size gas regressions run with integration verification. |
| `insecure-random` | Pass. Randomness is used only to make the intent nonce unique, never for an outcome. |
| `prepaid-gas` | Callbacks use static gas. Voucher withdrawal apply requires at least 280 TGas; dispatched recovery requires at least 140 TGas. Preparing timeout invalidation after 300 s does not dispatch an external call. `execute` has no entry minimum; supported-size callback regressions and the selected adapter still need final-revision verification. |
| `cover-storage-cost` | Local input/entry bounds do not prove sufficient storage funding. Voucher journal entries persist on external Intents storage and grow with owner-authorized attempts. The single armed attempt prevents unrestricted permissionless retries; repeated owner rearming still needs measured funding. |
| `unsafe-math` | Pass. The release profile sets `overflow-checks = true`. |
| `float-math` | Pass. No floating-point types. |

## Before real funds

A short checklist, since the code alone does not enforce these:

1. Freeze and independently review the final source/WASM and voucher protocol; run final combined tests. Prefer fresh deployment. Migration recognizes exactly `77ebc0c` pre-sequence, `0de5c48` sequence-enabled, pre-provider remediation and provider-enabled pre-voucher layouts; all require paused quiescent state, and an existing active withdrawal guard is never discarded.
2. Demonstrate the selected DAO owner, distinct from the vault, including vote/execute/cancel/recovery. Verify intended signer/Intents/oracle deployments, administrative trust, a verified oracle with feeds, updater and asset units (the oracle choice is still open); an unselected oracle is not live readiness.
3. Use a vault name of at most 62 bytes; establish both recovery sinks are uncreated and have no signing authority or later creation path. Prepare atomic deployment/initialization and funded storage.
4. Compare deployed WASM with the reviewed artifact, register the intended Intents authority, demonstrate owner controls, and seal every bypass-capable vault permission. Reinspect final-block state before funds.
5. Demonstrate actual vault MPC signing and funded matched settlement with final receipts and ledger deltas. A relay acknowledgement or status is insufficient.
6. Demonstrate a genuinely elapsed 24-hour withdrawal, callback-loss races, confirmed refunds, owner-rearmed safe retries and exact receiver/ledger changes on the intended controlled deployment. Unknown pre-voucher sends cannot be retrofitted into this proof protocol. These requirements remain open; this is not a mainnet go recommendation.
