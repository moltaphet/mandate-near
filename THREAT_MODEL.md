# Threat model

What Mandate protects, from whom, and what it does not. Each threat names the
mechanism that answers it and the test that shows it. Attack numbers refer to
[`docs/evidence/attacks.txt`](docs/evidence/attacks.txt), recorded by
[`scripts/attack-demo.sh`](scripts/attack-demo.sh). Open problems are listed in
[SECURITY.md](SECURITY.md#self-review-findings) and are not repeated as solved here.

Status words: **built and tested** applies to the stated environment and dated
run, not every later source revision. The recent client/voucher changes need
final combined release verification.
**planned** means it is not in the code. **not verified** means the claim rests
on reading, not on a run.
The 7 November baseline attempt before this documentation reconciliation exited
101 after protocol tests hit loopback-socket `PermissionDenied`. Formatting and
strict Clippy passed before that failure; later provenance targets were not
reached. Final combined fast and integration verification remain incomplete.

## Assets

| Asset | Where it lives | Who can move it today |
|---|---|---|
| Treasury balances | `intents.near`, credited to the vault account | Vault-authorized signed trades, delayed owner withdrawals, and any privileged Intents controls in the verified external deployment |
| Mandate (the rules) | vault state | Owner: tightening at once; loosening after a 24 h delay |
| Signing authority | MPC key derived for the vault account (`v1.signer`) | Vault contract under the intended derivation, after all bypass-capable vault permissions are sealed; external MPC trust remains required |
| Owner key | a NEAR full-access key held by the owner | Owner (see T3) |
| Agent key | an account the owner names with `set_agent`; a later change waits 24 h | Can propose only (see T2) |

Trades change the vault's Intents balances through signed settlement. Owner
withdrawal moves supported NEP141 assets out of that ledger, queued for 24 h
and sent to the authorized receiver. Native FT balances on the keyless vault
are outside its supported custody path. See T3 and SR-6.

## Actors and trust

| Actor | Trusted for | Limits / verification |
|---|---|---|
| Owner | writing the mandate, pausing, authorizing delayed withdrawals and safe retry rearming | Trade signatures remain gated; stolen owner authority can withdraw after the delay |
| Agent (manual, deterministic two-asset or optional model decision; TEE planned) | choosing a trade | On-chain gates remain authoritative; local previews do not replace them |
| Keeper, any stranger | running `execute` and `register_intents_key` | choosing or changing a trade |
| Solver / relay | filling a signed intent at or above `min_amount_out` | altering bytes; taking more than the signed diff |
| Configured immutable PriceOracle or Pyth adapter | market correctness and coherent publication times | Mainnet feed/updater, source and administrative identity UNVERIFIED; a fresh wrong price can pass (T6) |
| `v1.signer` (MPC) | signing only what the vault asks for, under its derivation rules | nothing on the vault's behalf beyond the call |
| `intents.near` | correct custody ledger, signature verification and atomic settlement | Current deployed code hash, administrator/upgrade controls and applicable audit coverage UNVERIFIED |

The selected owner is a DAO account distinct from the vault. Its governance and
recovery operations remain to be verified. The oracle is not yet selected: Pyth
Core is documented as unsupported on NEAR, and a feed source with fresh feeds,
correct units and a reliable updater remains to be verified.

## Threats

### T1. Compromised or jailbroken agent proposes harmful trades

The agent is the most exposed component: it reads untrusted market data and,
in the planned design, runs in a TEE with a pinned code hash.

The optional model client receives structured public-chain state, including
bounded owner-configured feed identifiers. It receives no news commentary,
previous rationale or arbitrary tool-output message. A hostile model response
or misleading structured metadata can still influence strategy. Its decision is strictly parsed and validated; the
rationale is untrusted text and is never executed or used to override trade
fields or owner rules. Paused/halted readiness prevents inference calls. Local
preview is advisory: a compromised agent can bypass the client, so the unchanged
on-chain gates remain the final boundary. A provider/model label alone proves
no model origin. A supplied invalid or unsupported proof stops a model proposal. Required NEAR
proof retrieval also fails closed. Signature recovery is checked against an
external public pin, whose source remains a trust assumption; hardware
attestation and live provider compatibility are unverified. Unsigned generic
responses remain UNVERIFIED. The offline verifier recomputes raw wire hashes,
the canonical commitment and exact decision binding; it authenticates no chain
or settlement.

The contract also enforces a declared source allowlist at proposal and execution.
Owner additions wait 24 hours; removals apply immediately. Migrated active and
queued mandates deny all sources until a delayed owner addition, including
manual/rebalance sources. The agent can lie about an allowed ID, so this policy
does not prove the origin of a model response. Canonical record verification
links the captured response and exact trade tuple; external pinned signature
verification has the separate, explicitly limited provenance meaning above.

**Mechanism.** Every proposal is checked on-chain at execution against the
mandate: asset allowlist, per-trade size, UTC-day turnover, slippage against
the oracle value, price freshness, cooldown, drawdown breaker (de-risk only),
position weight, and one signed intent at a time (`contracts/vault/src/engine.rs`).

**Tests.** Attacks 1 to 6 in `contracts/vault/tests/attacks.rs`; per-rule
boundary tests and a property test in `engine.rs`; three-strike halt in
`tests/execute_path.rs`.

**Residual.** A strategy inside the limits can still lose money.
The oracle-valued input/output shortfall authorized in a UTC day is bounded by
daily turnover times slippage bps divided by 10,000 (slippage is capped at 5%).
This requires correct coherent oracle values and is not a bound on realized
market losses, fees or a manipulated oracle.
Market moves inside the position cap are not bounded by the contract. The
mandate bounds risk; it does not create returns.

### T2. Stolen agent key

**Mechanism.** The agent account can call `propose` only. It cannot call
`set_agent`, `set_mandate`, `pause` or `resume_proposals`. A proposal still has
to pass every gate. `propose` is not gated by 1 yoctoNEAR, so a function-call
key for the agent works, which is the intent.

**Residual.** A stolen agent key can spend the agent's turns, which blocks real
proposals for up to the TTL (60 s) each time. It can also make the owner's
strike counter climb if gates close on its proposals (see T4).

### T3. Stolen owner full-access key

This is the most serious owner-side threat, because the owner key controls the rules.

**Mechanism.**
- Loosening a limit is queued for 24 h; the old limits hold meanwhile, and the
  owner can cancel (`MANDATE_DELAY_SEC`, `set_mandate`, `cancel_pending_mandate`).
  Anyone may apply a queued change after the delay, so it cannot be held back.
- Tightening applies at once and drops any queued loosening.
- `pause` blocks new execution and unsigned callback work. Applied control changes invalidate stale approvals through configuration revision checks. Already issued signatures remain valid until their deadline. `unpause` and `set_mandate` require 1 yoctoNEAR and owner authorization.
- A withdrawal is queued for 24 h (`request_withdrawal`, `cancel_pending_withdrawal`,
  `apply_pending_withdrawal`). It is refused while an execution holds the lock or a
  signed intent is open, and it is checked again when it is sent. An identified
  unresolved withdrawal guards competing operations until an identified terminal
  result or authenticated voucher debit/cancellation proof releases it. Initial
  debit proof is not terminal FT delivery; later resolver/refund receipts may
  overlap subsequent operations.

**Tests.** Attack 8 (a $150 cap queued, the $60 trade still refused, early apply
refused); unit tests in `lib.rs` for the queue rules.

**Mechanism for the agent.** A later change of agent is queued for 24 h
(`set_agent`, `apply_pending_agent`, `cancel_pending_agent`). The first agent
applies at once, since no agent can propose before it is set. The current agent
keeps its rights during the delay. Pause invalidates unsigned work; it cannot
revoke a signature already issued by MPC.
Tests: `a_new_agent_waits_24_hours_on_a_real_chain` in `tests/sandbox.rs`
checks first-agent authorization, replacement queuing, early-apply refusal and
cancellation. Despite its name, it does not advance 24 hours or test a matured
replacement. The `set_agent` unit tests in `lib.rs` cover delay boundaries with
controlled timestamps. SR-5 is implemented; live post-delay application remains
unverified.

**Residual.**
- A stolen owner key can still pause the vault, and can still queue a new
  agent. The agent then takes over after 24 h unless the owner cancels, so the
  owner has one day to notice. Gates still apply, so the damage is limited to
  mandate-approved trades.
- A stolen owner key can queue a withdrawal of any mandate asset. The owner has
  24 h to notice and cancel. If nobody cancels, anyone can send it after the
  delay, so the funds leave. This is the main residual risk for real funds.
- The owner can tighten a mandate to a point that stops all trading. That is a
  liveness loss, not a theft.
- **Accepted residual (F26): a stuck withdrawal guard.** A withdrawal may already
  have been dispatched or debited when its final callback is lost or malformed.
  Missing, failed or inconsistent proof retains the guard, blocking new
  withdrawals and trades. Elapsed time, a larger balance and owner authority
  cannot override it. Retaining the guard initiates no additional transfer;
  the original withdrawal may already have moved funds. Availability can be
  lost until authenticated proof resolves it. This receipt-loss fault has not
  been reproduced on a live chain. The project owner accepted this residual
  for the current testnet scope.
- The selected DAO owner's vote, execution, cancellation and recovery tests
  remain required before real funds.

### T4. Strike griefing by a stranger

**Mechanism.** `execute` is permissionless. A stranger who calls it while a
gate is closed turns the proposal into a strike. Three strikes halt proposals
until the owner calls `resume_proposals`.

**Residual.** Availability only. A stranger cannot choose a closed gate, but
can choose when to call `execute`. The owner recovers with one call. Accepted
as a design choice; see SR-4.

### T5. Solver or relay alters, delays or front-runs a signed trade

**Mechanism.** The signature covers the exact bytes: `raw_ed25519` payload with
`token_diff`, a V1 expirable nonce, and a 120 s deadline. `intents.near` checks
the signature and settles atomically, so the solver can fill only at or above
`min_amount_out`. Attack 7 shows the verifier refusing a changed amount.
The sandbox `matched_funded_settlement_is_atomic_and_replay_is_refused` test
also submits a funded matched bundle and checks ledger changes and atomic
refusal. It uses the supplied Intents fixture and stand-in signer/oracle; live
MPC and the currently deployed verifier still require separate verification.

**Residual.** A solver can decline to settle. The intent then expires after
120 s and the next proposal can run. Lost opportunity, not lost funds.

Turnover, cooldown and high-water accounting is conservatively reserved before
the signing Promise is dispatched, using the gate snapshot's time and state,
without observing settlement. Failed signing, lost answers and unfilled
intents still consume turnover quota and cooldown, with no automatic refund.
The high-water mark uses pre-trade snapshot NAV. An intent approved before
UTC midnight can settle on the next day within its deadline, so the daily
turnover cap does not bound actual settlement-day turnover.
The regression
`signing_dispatch_reserves_quota_even_if_the_result_arrives_after_lock_expiry`
checks that a late answer cannot discard the reservation.

The implemented keeper publishes only with explicit `--publish` configuration.
It requires matching public proposal/quote metadata and checks vault/verifier,
current signed result, exact input/output and deadlines before publication.
Acknowledgement is `acknowledged_not_settled`; even relay-reported `SETTLED`
keeps `settlement_verified: false`. This mode makes one initial status query,
not continuous settlement monitoring. Live relay interoperability and independent
final receipt/ledger proof remain unverified.

### T6. Oracle wrong, manipulated or stale

**Mechanism.** Provider choice is immutable at initialization. PriceOracle uses
aggregate and report reads and requires price/time coherence; it cannot stamp an earlier
aggregate with a newer unrelated report. The engine checks age at most
`max_price_age_sec` and future skew at most 5 s. Missing, stale or incoherent
data prevents signing. An incoherent oracle response can produce `MissingPrice`
before the engine's `FuturePrice` check is reached.

Pyth uses `list_prices_no_older_than` and lowercase 64-character hexadecimal
feed IDs. It requires positive signed price, nonnegative publication time,
exponent from -30 through 0, confidence below price, and confidence/price within
the mandate's slippage allowance. Equality at that allowance passes. Missing or
invalid held/traded entries become `MissingPrice`. Provider tests and
`contracts/vault/tests/pyth_path.rs` cover the documented response shape,
confidence/null refusal and twenty-asset sandbox wiring; final combined revision
verification remains required.

The execution records the oldest approved-price deadline. Before the salt
callback requests signing, all approved prices must still satisfy configured
age: exactly at the deadline is allowed; later is refused. There must be a
recorded approval. The checks are covered by
`salt_cannot_sign_after_the_approved_prices_become_stale` and
`salt_can_sign_at_the_exact_price_age_boundary`.

**Residual.** A manipulated price that is still fresh passes the age check.
Slippage, size and position checks use that same oracle and do not establish
an independent bound on loss in real market value. The oracle is trusted for
correctness; its mainnet provider, feed/source and administrator identity
remain UNVERIFIED.
An issued intent may settle later during its own lifetime; the contract does
not re-price or guarantee oracle freshness at settlement.

**Historical check.** On 2026-10-31, `price-oracle.near` returned no fresh price
for wNEAR or USDC. Current feed availability must be verified; the vault rejects
unusable data. A labelled sandbox feed is demonstration evidence, not a production oracle.

### T7. Replay of a signed trade

**Mechanism.** Single outstanding signed intent (the next proposal waits for the
deadline); V1 nonce bound to the live salt and deadline; `intents.near` rejects
reuse of a nonce. The vault refuses a proposal while a signed intent is
outstanding (`propose` and `execute` both check).

**Tests.** `a_signed_intent_blocks_the_next_proposal` exercises the local
outstanding-intent guard. The separate sandbox test
`matched_funded_settlement_is_atomic_and_replay_is_refused` submits a reused
nonce to the Intents fixture. Current live verifier identity and replay
behaviour remain a separate deployment verification task.

### T8. Concurrent executions or proposals corrupt state

**Mechanism.** `execute` sets an in-flight lock before its first cross-contract
call and refuses a second execution while the lock is set. Trading quota,
cooldown and high-water state are reserved before the signing request; the
signature itself is recorded by the identified final callback. Callbacks are
`#[private]` and must retain current identity/configuration/time authority.

**Found and fixed in this change.** `propose` did not check the lock, so an
agent proposal made while an execution was in flight could be dropped when that
execution finished (SR-2). Fixed by refusing `propose` while in flight.

**Execution recovery (SR-1).** Callback authority requires the current execution,
its proposal, unchanged configuration revision and valid time window. At
expiry an old callback must not clear a newly accepted proposal, even before
another execute replaces the ID. The 300 s build window plus the 120 s intent
lifetime bounds the execution lock. Any post-request signing error retains
that timed lock, including Failed Promise, malformed or oversized response;
a signature may already exist. Pre-sign read/salt/configuration/deadline
failures can release it. Live signer timeout and late-result behaviour remain
unverified.

**Withdrawal recovery (SR-6).** `unique_voucher_v1` persists a never-reused ID
and `Preparing` phase before issuing one voucher and reading the victim balance
in a single Intents receipt. Dispatch proceeds only after authenticated reads
show global `mt_supply` one, source
balance one and both sink balances zero; duplicate units preseeded anywhere are
refused. The `Dispatched` receipt transfers the voucher to
`s.<vault>` first and performs `ft_withdraw` last. Full terminal amount confirms
delivery; explicit zero/refund can requeue only the still-authorized generation.
Safe requeues are unarmed. `rearm_pending_withdrawal` requires owner predecessor
authorization and exactly 1 yoctoNEAR, preserves maturity, and authorizes only
one new-ID attempt. This bounds permissionless retry/storage grief per owner arm.

Apply requires **280 TGas**. After **300 s**, `Preparing` recovery invalidates
the ID before any delayed preparation callback can dispatch. `Dispatched`
recovery requires **140 TGas** and races the unique unit to `c.<vault>`.
Recovery requires global supply one and source zero.
Sent=0/cancelled=1 proves cancellation before debit and permits safe requeue;
sent=1/cancelled=0 consumes the authorization as `DebitObserved`. Both zero,
both positive, malformed or failed reads retain the guard. A late callback for
an old ID/phase cannot complete or restore a new attempt.

Sent proof establishes initial debit only. It releases the guard without proving
terminal FT delivery or absence of a later refund; those receipts can overlap
subsequent operations. Both sinks must be uncreated and without implicit,
registered or proxy signing authority, and the vault sealed against later
creation or bypass. An empty current signing list alone is insufficient.
The vault name must be at most 62 bytes. Intents administrative behavior remains
trusted, and its per-ID voucher journal consumes funded external storage.
Historical unknown sends from pre-voucher deployments cannot acquire this proof
retroactively.
All earlier voucher issuance/burn/resolver/refund receipts must have completed before funding
or enabling operations. A delayed pre-seal mint after preparation proof violates
the global uniqueness premise. Prefer fresh atomic account creation, deployment
and initialization without adding access permissions; current counts do not
establish receipt history.

The full vault withdrawal lifecycle is exercised in sandbox by
`vault_withdrawal_reads_sends_and_confirms_the_actual_transfer`. Its maturity
mode is explicitly logged; if fast-forward cannot reach the queued deadline,
only that maturity is patched in sandbox STATE. This is a send/receipt/callback
test, not proof that the 24-hour delay elapsed.

The 6 November 170 TGas lifecycle validation is historical pre-voucher evidence.
Native Intents voucher/batch mechanics have dedicated fixture tests in
`contracts/vault/tests/recovery_protocol.rs`; final production callback/race
tests and intended-chain elapsed-delay/recovery evidence remain required.

### T9. Mandate changes that are not in the mandate

**Mechanism.** The mandate is validated on every write, so an invalid mandate
cannot be stored (for example, a per-trade cap above the daily turnover cap).

**Tests.** `simulate_reports_the_effect_and_changes_nothing`, the sandbox owner-control test.

### T10. Key leaks outside the chain

**Intended operation.** Clients load credentials from outside product source.
Credential presence, file permissions, historical leak checks and local hooks
were not verified by this review; excluded files were not inspected.

**Residual.** Compromised client credentials can authorize permitted proposals
or owner actions. No claim about the existence or contents of local mainnet
credentials is established here.

### T11. Deployment mistakes

**Mechanism.** `new`, `new_with_externals` and `new_with_oracle` are `#[init]`,
so they run once.
The NEP-330 metadata names the source repo, so anyone can rebuild the wasm and
compare its hash.

**Residual (not verified).**
- If the vault account keeps a full-access key, its holder can deploy any code
  and sign through the MPC key. The vault account must have no full-access
  keys before it holds funds (SR-9).
- Deploying code and calling `new` in separate transactions lets anyone
  initialize the vault first. Deploy and init must be one transaction.
- Owner must differ from the vault account. Removing vault full-access keys
  from a self-owned vault would remove its external recovery authority.
- `new_with_externals` lets the deployer choose the signer, intents and oracle
  accounts. Users must check `get_externals` before trusting a deployment.
- Changed Borsh state requires private atomic migration or fresh deployment.
  Exactly five historical layouts are recognized: `77ebc0c` pre-sequence,
  `0de5c48` sequence-enabled, pre-provider remediation and provider-enabled
  pre-voucher, plus voucher-enabled pre-source-admission at `b43569d`.
  Migrated active and queued lists deny all declared sources until a new delayed
  owner addition. All require paused/quiescent state and a distinct owner; an
  active withdrawal guard is never discarded. The provider-enabled layout
  and voucher-enabled layouts preserve their adapter. Unresolved voucher stage
  or retry authorization also blocks migration. Old unrecorded sends require external final-receipt
  history; deploying new WASM alone is not migration or recovery.
- A voucher-capable vault name is limited to 62 bytes. Verify absence and inert
  authority for both `s.<vault>` and `c.<vault>`, then seal every bypass-capable
  vault permission. The contract does not itself prove this deployment history.
- NEAR Intents administrative permissions, registered signing authority and
  deployed hashes are part of the custody trust boundary and must be checked.

### T12. Gas exhaustion of `execute`

**Mechanism.** PriceOracle reads an aggregate and one report per held/traded
feed; Pyth uses one batch. Both use the engine's held/traded price checks. The
mandate caps assets at `MAX_ASSETS` (20).

**Historical measurement (4 November, PriceOracle path).** On the sandbox,
22 feeds passed and 24 and 32 failed with
`Exceeded the prepaid gas` ([docs/evidence/gas-limit.txt](docs/evidence/gas-limit.txt)).
The cap is set below the largest size that passed.

`gas_by_feed_count` covers the supported PriceOracle limit;
`pyth_twenty_asset_callback_chain_fits_gas_and_checks_every_held_feed` covers
the Pyth branch. Final-revision verification and intended-chain gas/storage
measurement are still release requirements.

**Residual.** A failing `execute` holds the lock until it runs out, about 7
minutes (SR-1). The cap keeps the chain inside the measured limit, but a future
change to a callback's gas or the oracle's answer size could move it. Re-measure
after any such change.

## Out of scope

- Compromise of NEAR consensus or configured signer/Intents infrastructure:
  the design assumes correct external behavior; current deployed source/audit/
  administrative correspondence remains unverified.
- The TEE (OutLayer): not built yet. Until it is, the agent is a script run by
  the developer, and the README says so.
- Market profitability and strategy quality.
- Dashboard: public read-only snapshots are implemented with strict schemas and
  block pinning. The RPC is trusted for the reported state; the page does not
  independently authenticate receipts, governance or oracle correctness.
