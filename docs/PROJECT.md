# Mandate — Project Definition

Status legend used throughout:
**[SIMULATED]** accepted by mainnet `simulate_intents`, without settlement · **[TESTED]** covered by a rerunnable test (environment stated) · **[LIVE]** actual deployment transaction evidence · **[PLANNED]** designed, not built · **[OPEN]** unresolved.

6 November implementation refresh: the dashboard now reads selected public
vault state and listed Intents balances at one final block, while sample mode
remains separate. Immutable PriceOracle/Pyth adapters, deterministic two-asset
autonomous preparation, explicit Message Bus publication and the unique-voucher
withdrawal protocol are implemented. Final combined release verification is
pending. Usable
live feeds, owner governance, exact deployed MPC/settlement and withdrawal
recovery evidence remain OPEN. See [OPERATIONAL-READINESS.md](OPERATIONAL-READINESS.md).

---

## 1. One sentence

Mandate is an on-chain policy layer for NEAR agents: a vault that authorizes trade signatures against owner-defined rules, with a separate delayed owner withdrawal path. The Rust reference agent accepts manual plans, deterministically rebalances two assets, or validates a configured model decision against a public snapshot. TEE execution and live model provenance remain unverified. Permitted trades can still lose money, and custody depends on the sealed deployment, NEAR Intents and MPC trust assumptions.

## 2. Problem

An AI agent that moves money is only as safe as the code that decides whether a trade is allowed. Today that decision usually lives inside the agent: a `if amount > cap` check in the agent's own process. Whoever controls that process controls the money. Prompt injection, a compromised host, a bug, or a malicious operator can each bypass it, and the owner has no independent way to know which rules were enforced.

Two existing approaches leave gaps:

- **Rules inside the agent's code.** Easy to write, but unverifiable and bypassable by anyone who controls the process.
- **Rules inside a TEE-hosted wallet** (for example OutLayer Agent Custody). The policy runs inside the TEE, and the result arrives as a signed message. The owner must trust the TEE, its operator and its attestation, and the chain does not see the decision itself.

Mandate moves the decision to a place the owner can read and the chain can enforce: the contract's own code.

## 3. Thesis

Enforcement point matters more than policy richness. With the intended contract as the sole request path, every bypass-capable vault permission sealed, and correct trusted external behavior, the vault requests a trade signature only after its rules pass. The rules and source are public and testable. Owner withdrawals are separately authorized and delayed; permitted trades can still lose money.

## 4. Scope of version 1

In scope:
- One vault contract per treasury, with one owner account (an individual account or a Sputnik DAO account).
- A nine-gate trade check, executed on-chain before any MPC signature.
- Signing through NEAR Chain Signatures (`v1.signer`, Ed25519) for the vault's own NEAR Intents account.
- Settlement through NEAR Intents `token_diff` intents with V1 expirable nonces.
- A stable contract interface and reference Rust clients; a separate SDK is planned.
- Manual-plan, deterministic two-asset and optional strict model Rust decision sources; OutLayer execution and live provider provenance remain unverified.
- A permissionless Rust keeper with execution/public outbox by default and explicit quote-bound Message Bus publication.
- `simulate_mandate_change` for rule changes; the pure engine can evaluate trades locally. A general on-chain trade-simulation view is not implemented.
- A 24-hour delay on mandate changes that loosen limits.
- An optional SHIELD incident gate is planned; partner access and implementation remain open.

Out of scope for version 1 (stated, not hidden):
- Pooled deposits from several investors, shares and NAV accounting. This adds inflation and manipulation surfaces we do not want to ship half-built.
- Cross-chain withdrawals directly from the contract. Settlement stays inside NEAR Intents.
- Automatic strategy discovery. The agent proposes; the mandate decides.
- Any claim that the strategy is profitable.

## 5. Actors and trust

| Actor | Can do | Cannot do | What enforces the limit |
|---|---|---|---|
| Owner (account or DAO) | Set mandate, pause, unpause, withdraw | Bypass the gates on a trade; change a limit to looser without the delay | Contract: owner predecessor and exactly 1 yoctoNEAR for `set_mandate` and `unpause`; 24-hour delay for loosening |
| Agent (OutLayer TEE or script) | Propose a trade with a minimum output | Sign, withdraw, change the mandate, skip a gate | Contract. The agent holds no key on the vault account. |
| Keeper (anyone) | Trigger the next step, delay a trade | Change a trade or signed bytes; sign anything | Contract. Signed bytes fix amount, deadline, nonce. |
| Solver | Fill the signed trade at or above the minimum | Take more than the signed `token_diff` | NEAR Intents verifier |
| MPC network | Sign a payload the vault asks for | Be asked by anyone other than the vault | Chain Signatures: the derived key is bound to the vault account |
| Oracle operator | Publish prices | Set prices the contract trusts without a freshness check | Contract: staleness bound, fail closed |

Residual trust, stated explicitly:
- We depend on the MPC threshold and on the intended external contracts and their administrative permissions. Current deployed membership, roles and code hashes must be verified before funds.
- The price oracle is operated by a third party. We reject stale or missing prices, but we cannot independently verify a price's correctness.
- The owner's key is a single point of authority. Mitigations are in section 8.

## 6. Architecture

```
            ┌──────────────┐  proposal (min_out, reason_hash)
            │ Agent        │ ─────────────────────────────┐
            │ OutLayer TEE │                              │
            │ or script    │                              ▼
            └──────────────┘                    ┌─────────────────────┐
                                                │  Vault contract     │
  Keeper (anyone) ──triggers steps────────────▶ │  mandate, state,    │
                                                │  nine gates, MPC    │
  Owner (account / DAO) ──set mandate, pause──▶ │  call, intent store │
                                                └──┬───────────┬──────┘
                                                   │           │
                                  price-oracle.near│           │ v1.signer (MPC, Ed25519)
                                                   ▼           ▼
                                             fresh prices   signature over raw_ed25519 intent
                                                               │
                                                               ▼
                                                    intents.near (token_diff)
                                                               ▲
                                                               │ publish signed intent
                                                        Solver relay / solver
```

Components:
- **Vault contract** (Rust, near-sdk 5.30): mandate storage, trading state, the nine gates, the MPC call, intent storage, owner controls. Built, with tests.
- **Gate engine**: a pure function `check_trade`. Built, with unit and property tests.
- **Exact math**: 256-bit USD valuation with conservative rounding. Built, with tests.
- **Intent builder**: `raw_ed25519` `token_diff` payload and V1 nonce. Built; historical author-recorded mainnet simulation and sandbox fixture verification, not a live funded vault trade.
- **Keeper** (Rust): existing execution/outbox sandbox coverage; explicit publication is implemented with matching quote metadata. Final combined client verification and live binary/relay operation remain open.
- **Agent integrations**: manual script with existing sandbox coverage; deterministic two-asset strategy and final-snapshot gate preview implemented. OutLayer and model inference [PLANNED].
- **SDK**: typed client over the vault's public methods. [PLANNED]
- **Evidence site and dashboard**: selected public vault state and Intents balances can be read at one final block; fixed sample mode is separate. Public RPC data does not independently authenticate custody or settlement.

## 7. Trade lifecycle

Two phases by design: the agent submits a proposal, then a permissionless caller executes it. Keeping planned TEE work outside execution avoids combining its gas with balance/oracle/signing calls. The implemented path is sandbox-tested with a stand-in MPC signer; a requirement to split the plain-script path has not been measured. It has not run end to end with funds on a live chain.

**Transaction 1, decision.**
1. The Rust reference agent accepts a manual plan or builds an exact-input proposal from two-asset holdings, target band and a matching solver quote. It previews every gate and refreshes final state before proposing. OutLayer is planned.
2. The proposal is stored with an expiry. A new proposal cannot replace an unexecuted one before its expiry.

**Transaction 2, execution.** Permissionless: any account can call it.
1. Read the vault's balances on NEAR Intents.
2. Read oracle data for assets held or traded, with price/time coherence checks. Missing, stale or incoherent prices prevent signing.
3. Retain execution identity and configuration revision across callbacks; changed or expired work must not advance to signing.
4. Run the nine gates in fixed order (section 8). The first closed gate rejects with a named code.
5. If all pass, build the `token_diff` payload and V1 nonce with the live salt.
6. Reserve trading state from the approved snapshot before dispatching `v1.signer.sign` with the EdDSA payload, required deposit and gas.
7. Store a successful signed intent. Signing failure or a lost answer does not automatically refund the reserved turnover, cooldown or high-water accounting.
8. [PLANNED] Emit an event with the intent hash, mandate version, and result. The contract emits no events yet; the result is in the transaction's return value and the stored state.

**Off-chain publication.** Default keeper mode writes a public signed-intent handoff. Explicit `--publish <public-relay-config.json>` enables the implemented Message Bus path after matching quote/proposal metadata, current signed state, exact amounts, vault/verifier and deadlines. Acknowledgement and an initial relay status are recorded separately; even relay-reported `SETTLED` retains `settlement_verified: false`. Continuous status monitoring and independent final receipt/ledger verification are not implemented. Live publication/fill remains unverified; the Intents settlement trust assumption is separate.

## 8. Mandate

### 8.1 Fields

- `assets`: up to 20 supported NEP141 entries (`MAX_ASSETS`; see [SECURITY.md](../SECURITY.md) SR-3). Each has `asset_id`, `decimals`, `price_feed_id`. Traded/custodied NEP245 assets are unsupported; the protocol's internal NEP245 voucher is a control marker, not a mandate asset.
- `stable_assets`: subset of `assets`, used for de-risking.
- `max_trade_usd`, `max_daily_turnover_usd`: micro-dollars.
- `max_position_bps`: cap on the non-stable destination's conservative weight. Other non-stable holdings above the cap may remain only when their weight does not increase; this preserves de-risking from an already excessive position.
- `max_slippage_bps`: hard cap 500 (5%).
- `max_drawdown_bps`: drawdown threshold from the high-water mark.
- `min_trade_interval_sec`: cooldown.
- `max_price_age_sec`: hard cap 300.

All fields are validated on set. Built and tested.

### 8.2 The nine gates

Checked in this order. The first failure ends the check and returns its code.

| # | Gate | Code on failure | Rule |
|---|---|---|---|
| 1 | Structure | `SameAsset`, `AssetNotAllowed`, `ZeroAmount` | Different assets, both allowed, non-zero amounts |
| 2 | Lock and cooldown | `SignedIntentOutstanding`, `CooldownActive` | No unexpired signed intent; cooldown elapsed |
| 3 | Price freshness | `MissingPrice`, `StalePrice`, `FuturePrice` | Every traded or held asset has a fresh price; at most 5 seconds ahead of block time |
| 4 | Balance | `InsufficientBalance` | Vault holds `amount_in` |
| 5 | Trade size | `TradeTooLarge` | Notional, rounded up, ≤ `max_trade_usd` |
| 6 | Daily turnover | `TurnoverCapExceeded` | The gate snapshot's UTC-day turnover plus notional ≤ cap; approval-day accounting resets at 00:00 UTC |
| 7 | Slippage | `SlippageExceeded` | Value of `min_out` (rounded down) ≥ notional × (1 − slippage) |
| 8 | Drawdown lock | `DrawdownDeRiskOnly` | If NAV < high-water mark × (1 − drawdown), only trades into stable assets |
| 9 | Position limit | `PositionTooLarge` | Non-stable destination stays below the cap; another over-cap holding may not increase its share of conservative NAV |

Accounting is conservatively reserved at signing request, before the MPC
Promise is dispatched, using the gate snapshot's time and state. Daily
turnover and cooldown therefore count signing requests even when signing
fails, the answer is lost or the intent is never filled. There is no
automatic refund of that reservation. The
high-water mark uses the snapshot's pre-trade NAV; it is not updated from an
observed settlement result. A signature approved before midnight may settle
on the next UTC day within its deadline. The daily cap is not a strict cap
on actual settlement-day turnover.

Design choices, stated:
- Exposure rounds up and receipts round down, so every boundary errs toward rejection.
- The received asset is valued at the larger of `min_out` and notional, because a solver may deliver more than the minimum.
- NAV counts only mandate assets, so airdropped tokens cannot inflate it.
- The high-water mark only rises, when an approved snapshot is reserved before signing dispatch.

Status: gates 1–9 and the callback wiring are implemented and tested on a sandbox with the Intents fixture, stand-in signer and stand-in oracle. Live funded settlement remains unverified.

The sandbox `matched_funded_settlement_is_atomic_and_replay_is_refused` test
checks matched funded ledger settlement, atomic refusal and nonce replay. It
does not use live MPC or establish current mainnet deployed identity.

All post-request signing errors, including Failed Promise and malformed or
oversized answers, retain the timed execution lock because a signature may
already exist. Read/salt and pre-sign validation failures can release it.

The salt callback also requires a recorded approval whose oldest approved-price
deadline has not passed. Equality at the configured price-age boundary is
allowed; a later signing request is refused. Prices are not re-evaluated when
an issued intent settles.

## 9. Owner controls

| Method | Who | Deposit | Effect |
|---|---|---|---|
| `set_mandate` (tightening) | owner | 1 yocto | Applies immediately |
| `set_mandate` (loosening) | owner | 1 yocto | Queued; applies after 24 hours; can be cancelled by owner |
| `cancel_pending_mandate` | owner | 1 yocto | Cancels a queued change |
| `set_agent` (first agent) | owner | 1 yocto | Applies immediately. No agent can propose before it is set |
| `set_agent` (later agent) | owner | 1 yocto | Queued; applies after 24 hours; the current agent keeps its rights meanwhile; can be cancelled by owner |
| `apply_pending_agent` | anyone | none | Applies a queued agent once its delay has passed |
| `cancel_pending_agent` | owner | 1 yocto | Cancels a queued agent change |
| `pause` | owner | none | Stops all new executions immediately |
| `unpause` | owner | 1 yocto | Resumes execution |
| `simulate_mandate_change` | anyone (view) | none | Reports whether a mandate is valid, which fields it loosens, and whether `set_mandate` would apply it now or queue it. Changes nothing |
| `request_withdrawal` | owner | 1 yocto | Queues a withdrawal of one mandate asset to a receiver, 24 h delay, replaces an earlier queue. Refused while an execution holds the lock or a signed intent is open |
| `cancel_pending_withdrawal` | owner | 1 yocto | Cancels a queued withdrawal |
| `apply_pending_withdrawal` | anyone | none | After the delay and only while owner-armed, requires ≥280 TGas; issues a unique voucher in `Preparing`, then marker-first/withdraw-last batch in `Dispatched` |
| `rearm_pending_withdrawal` | owner | exactly 1 yocto | Arms one safe pending retry, preserving maturity; apply allocates a fresh ID |
| `recover_withdrawal` | anyone | none | After 300 s, invalidates preparation or races a dispatched voucher to cancellation; dispatched recovery requires ≥140 TGas |
| `get_withdrawal_status`, `get_last_withdrawal_outcome` | anyone (view) | none | Active withdrawal identity and bounded outcome; `DebitObserved` is initial debit, not terminal delivery |
| `get_withdrawal_stage`, `get_withdrawal_protocol`, `is_withdrawal_retry_armed`, `get_withdrawal_recovery_accounts` | anyone (view) | none | `Preparing`/`Dispatched`/null, `unique_voucher_v1`, one-attempt arm and named proof sinks |
| `get_chain_time_sec`, `get_last_proposal_outcome` | anyone (view) | none | Chain time and the last stored proposal/execution result, including a gate rejection |

`unique_voucher_v1` persists a never-reused withdrawal ID and `Preparing` before
issuing exactly one Intents voucher and reading the victim balance in one
receipt. Before dispatch, authenticated `mt_supply` must be one globally,
source balance one and both sink balances zero. A former issuer's preseeded
duplicate in any ledger is refused. The `Dispatched` batch moves that unit to `s.<vault>` first, then
calls `ft_withdraw` last. Full terminal amount records `Delivered`; explicit
zero/refund can requeue only the matching authorized generation, unarmed.
Only the owner with exactly 1 yoctoNEAR may rearm a safe retry. Maturity is
preserved and the next apply uses a new ID, bounding permissionless retry/storage
grief to one attempt per arm.

Apply requires **280 TGas**. After **300 s**, preparation can be invalidated
before any delayed callback dispatches. Dispatched recovery requires **140 TGas**
and races the voucher to `c.<vault>`. Recovery requires supply one and source
zero; sent=0/cancelled=1 proves cancellation
before debit and requeues safely; sent=1/cancelled=0 consumes authorization as
`DebitObserved`. Missing/failed or inconsistent proof retains the guard.
Initial debit proof does not prove terminal FT delivery or absence of a later
refund. Its guard release allows subsequent operations to overlap unresolved
resolver/refund receipts.

The vault name must be at most **62 bytes**. Setup must prove both sinks are
uncreated and without implicit, registered or proxy signing authority, then
seal all bypass-capable vault permissions so they cannot later be created through
the vault. An empty current signing list alone is insufficient. The vault never
creates them. Intents code/admin behavior remains trusted; per-attempt voucher
journal storage requires measured funding.
All prior voucher issuance/burn/resolver/refund receipts must be complete before funding or
enabling operations. Prefer fresh atomic account creation/deployment/initialization
without adding access permissions; current counts do not authenticate that history.
For release, require a fresh account never given issuer authority or authenticated
absence of every prior canonical future-voucher issuance. Completion of known
receipts alone cannot exclude a duplicate holder initiating a later burn/refund;
current `mt_supply` is not lifetime issuance.

Upgrading an existing Borsh state requires the private `migrate` method for a
known legacy layout, called by the vault account in an authorized deployment
transaction. Exactly five layouts are supported: `77ebc0c` pre-sequence,
`0de5c48` sequence-enabled, pre-provider remediation and provider-enabled
pre-voucher, and voucher-enabled pre-source-admission at `b43569d`. The last two
preserve their provider. Active and queued source lists migrate empty, including
reserved manual/rebalance IDs; first admission requires a new 24-hour delay.
Unresolved voucher stage or retry authorization also blocks migration. Owner must be distinct, the vault
paused, no execution/pending proposal/queued withdrawal/active withdrawal guard
present, and signed deadlines expired. Queued mandate/agent controls are preserved;
mandates are revalidated. An active guard is never discarded. Legacy state cannot prove no withdrawal was
dispatched; require final-receipt history or use a fresh deployment. Migration
does not change owner identity or bypass removal of vault full-access keys.

**Definition of loosening.** A change is loosening if it increases `max_trade_usd`, `max_daily_turnover_usd`, `max_position_bps`, `max_slippage_bps`, `max_drawdown_bps` or `max_price_age_sec`, decreases `min_trade_interval_sec`, adds or changes an asset configuration, or adds a stable classification. Other changes apply immediately. A change that mixes both directions is treated as loosening.

**Why these rules.** Tightening invalidates unsigned work immediately. Loosening needs a delay, so a compromised owner key cannot raise limits and drain the vault in one step. The owner can cancel queued changes and pause unsigned execution. Already issued signatures retain their original deadline.

Status: owner controls have unit and sandbox coverage. The 6 November 170 TGas lifecycle validation is historical pre-voucher evidence. The current `vault_withdrawal_reads_sends_and_confirms_the_actual_transfer` target must be run at 280 TGas with the final protocol. It probes clock advancement and may patch only request maturity in sandbox STATE; that mode does not prove 24 hours elapsed. Final combined protocol/client verification and actual live post-delay withdrawal remain required. Historical testnet evidence queued and cancelled a loosening (see [TESTNET.md](TESTNET.md)).

## 10. Owner identity

The owner can be:
- an individual NEAR account, or
- a Sputnik DAO account, so that `set_mandate` and `unpause` need a multisig vote.

The contract must not assume the owner is an individual. [PLANNED: one sandbox test with a DAO owner.]

## 11. Oracle policy

- `new` and `new_with_externals` choose PriceOracle. `new_with_oracle` selects immutable `price_oracle` or `pyth`; there is no runtime provider setter. PriceOracle reads aggregate and individual reports; Pyth uses `list_prices_no_older_than`.
- The contract accepts a price only with a coherent publish time within `max_price_age_sec`. Freshness is not independent verification of market correctness.
- A zero price, an exponent above zero, or an exponent below −30 is a valuation error, never a panic.
- Current state, observed on mainnet on 2026-10-31: `price-oracle.near` returns `null` for `wrap.near` and for the USDC asset id in `get_price_data`. The feed must be confirmed working before any trade. If it still returns no price, trading stays off and the evidence says so. The contract's own gate fails closed on a missing or stale price (sandbox attacks 5 and 6).
- Pyth requires lowercase 64-hex feed IDs, positive signed price, nonnegative publication time, exponent -30 through 0, confidence below price and confidence/price within the slippage allowance. Equality at that allowance passes. Missing/invalid held or traded entries are refused by the engine. Unit/provider tests and `tests/pyth_path.rs` exercise sandbox wiring; live feed/updater, units and source/admin correspondence remain unverified. Both queried NEAR caches were null on 6 November; see [OPERATIONAL-READINESS.md](OPERATIONAL-READINESS.md).
- The implemented autonomous quote preview and authoritative on-chain slippage gate compare the exact quoted min-out with oracle notional. This is no independent guard against a fresh incorrect oracle.

## 12. Agent integrations

### 12.1 OutLayer agent [PLANNED]
- Rust compiled to `wasm32-wasip2`, pinned by SHA-256.
- Runs in an Intel TDX TEE on OutLayer; results return to the vault through yield and resume.
- Secrets, if any, are bound to the wasm hash.
- Outputs a proposal within the 900-byte result limit.

### 12.2 Rust reference agent [IMPLEMENTED; final combined verification pending]
- Manual `<public-plan.json>` mode is retained. `--auto <public-config.json>` deterministically rebalances exactly two listed assets toward a configured target outside its band, using actual ledger/oracle values and bounded exact-input quotes. No model inference or TEE is involved.
- `--dry-run` performs public status/view/quote requests only: no signing/authentication settings, outbox, proposal or publication. The documented partner relay requires a JWT, so an unauthenticated quote request may fail. Set optional `expected_chain_id` to the reviewed network; all views are pinned by final hash and height.
- Quotes must exceed configured validity, whose minimum is 450 seconds. Every gate is previewed with the shared engine; a fresh snapshot repeats readiness/strategy/gates before proposal. On-chain state races remain subject to authoritative vault checks.
- Executing mode writes public quote/proposal/vault/verifier/exact-amount metadata for later explicit keeper publication. `--loop` repeats after completed decisions with a 30-second pause; errors can terminate the process. Live strategy and relay operation remain unverified.
- Existing manual-client tests: `contracts/vault/tests/agent.rs`. Historical testnet: proposal reached the vault, execution refused without a registered identity ([TESTNET.md](TESTNET.md)). See current [agent usage](../agent/examples/README.md).

### 12.3 Optional model decision source [IMPLEMENTED LOCALLY; live provider unverified]
- `--model` reads a bounded final-block public snapshot and accepts the strict seven-field output. Its dry-run calls the configured model provider with authentication but sends no NEAR transaction; the autonomous dry-run rule above applies to `--auto`.
- The model path commits to `mandate-response-json-v1` canonical response bytes while retaining distinct raw request/response hashes for provenance. Manual/rebalance paths preserve `SHA256(rationale UTF-8)`.
- The contract stores the declared `model_id` in the proposal and enforces `allowed_model_ids` at proposal and execution. The list allows at most 8 unique nonblank IDs of at most 64 UTF-8 bytes; empty denies all. Additions wait 24 hours, removals apply immediately. Reserved non-model IDs are `manual-v1` and `rebalance-v1`.
- Invalid or unsupported supplied signatures refuse a model trade before proposal. Unsigned records remain UNVERIFIED. Declared IDs and pinned signer verification do not prove authenticated model origin or TEE identity. Native checks pass; final sandbox/gas and live provider evidence remain required.

Acceptance for both: each agent makes the same proposals through the same SDK, and the vault treats them identically.

## 13. SDK [PLANNED]

- Today the agent and keeper crates are the reference clients. A separate SDK is not built.
- Target surface: `get_mandate`, `get_trading_state`, `simulate_mandate_change`, `propose`, `execute`, owner methods.
- Stable public interface. Breaking changes require a new contract version.
- Target: a new agent integrates in under 100 lines and under 4 hours for a developer familiar with NEAR. Measured, not assumed.

## 14. Keeper [BUILT, sandbox-tested; the binary has not yet run against a live chain]

- A permissionless process (`keeper/`) that executes eligible proposals or writes a public signed-intent handoff by default. After failed work it uses bounded backoff, 5 s doubling up to 600 s.
- Only explicit `--publish <public-relay-config.json>` enables Message Bus publication. Matching quote/proposal metadata, current signed result, vault/verifier, amounts, signature format and deadlines are checked before submission. Acknowledgement and one initial relay status are separate, always retaining `settlement_verified: false`; no continuous settlement monitor is implemented. See [keeper usage](../keeper/examples/README.md) and the [official protocol](https://docs.near-intents.org/integration/market-makers/message-bus/rpc).
- Anyone can run it. The vault does not trust it.
- If no keeper runs, no trade happens. This is a fail-safe, and is documented as such.

## 15. SHIELD gate [PLANNED; partner access OPEN]

- NEAR Intents SHIELD exposes incident status for chains, tokens and bridges.
- When the key is approved, the agent checks SHIELD before proposing and refuses to propose a trade touching an incident scope.
- SHIELD is an extra caution layer. It can only make the system stricter, so a bypass of this check removes a precaution but does not allow an unapproved trade.
- This gate and an unavailable-status integration are not implemented. The intended optional behavior does not establish a current contract or client guarantee.

## 16. Claims policy

Claims distinguish historical [SIMULATED], stated-environment [TESTED], transaction-evidenced [LIVE], [PLANNED] and [OPEN]. Implemented code without a final run is described as implemented, with verification pending.
- A simulation/test claim names the command and environment; it does not establish funded live settlement or final-revision verification.
- `scripts/check-claims.sh` checks allowed-file references and test-name existence. It does not execute those tests. Verification scripts and fresh outputs establish runs separately.
- Fixed samples are labelled and kept separate from public RPC observations. An observed snapshot or explorer reference does not independently authenticate custody, governance or settlement.

## 17. Threat model (summary)

| Threat | Mitigation | Residual |
|---|---|---|
| Agent is prompt-injected | Agent cannot sign; gates reject out-of-mandate proposals | A permitted trade can still be a bad trade within limits |
| Agent host compromised | No treasury signing key on the agent; proposals are checked | Availability attacks and harmful permitted trades |
| Keeper malicious | Keeper cannot alter signed bytes or skip gates | Can delay |
| Owner key stolen | Loosening delayed 24 h; pause instant; cancel available | Attacker can wait out the delay, then loosen |
| Owner key lost | Pause still works with a function-call key; DAO owner recommended | Funds locked if no one can act |
| Stale or manipulated price | Coherent price/time and freshness checks; slippage floor against oracle | A fresh incorrect oracle remains trusted |
| MPC signer collusion | Threshold of MPC nodes | Threshold assumption |
| Solver under-delivers | `token_diff` requires minimum or nothing | None within the trade |
| Mandate misconfigured | `simulate_mandate_change` and validation on set; local engine evaluation | Owner error is still possible |
| Re-initialisation | `#[init]` guard | — |
| Stuck signed intent | Expiry; one outstanding intent at a time | Blocks new trades until expiry |
| Stuck execution (failed callback) | The lock runs out after a 300 s window plus the 120 s intent lifetime; a late callback from an earlier execution changes nothing | Pauses trading for up to 7 minutes ([SECURITY.md](SECURITY.md) SR-1) |

The full threat model is in [THREAT_MODEL.md](../THREAT_MODEL.md). The self-review findings are in [SECURITY.md](../SECURITY.md).

## 18. Metrics we will publish

Each has a method and a target; these targets are not completed measurements.

| Metric | Method | Target |
|---|---|---|
| Integration time for a new agent | Timed run by a developer who did not write the SDK | under 4 hours |
| Integration size | Lines of agent code to propose a trade | under 100 lines |
| Attacks rejected | Sandbox run of each attack class | all rejected with the expected code |
| Gas per trade | Sandbox and mainnet | recorded in USD |
| Latency per trade | Time from proposal to signed intent | recorded in seconds |
| Approval rate under a realistic mandate | Property test and replayed proposals | recorded, not targeted |

## 19. Plan and milestones

The numbered milestones below are planning checkpoints, not current implementation status. Each completed milestone needs dated, revision-bound evidence.

| Days | Milestone | Evidence |
|---|---|---|
| 1–3 | Vault wiring in sandbox with the real defuse contract: oracle request, MPC key registration on `intents.near`, deposit path | sandbox tests pass |
| 3–5 | `execute` and `propose`: balances, prices, gates, MPC sign, intent storage. One test per gate code | tests per code |
| 5 | Governance: loosening delay, cancel, `simulate`, DAO-owner test | tests pass |
| 5–6 | Gas measurement on sandbox and mainnet, with a funded account | recorded table |
| 6–8 | SDK and script agent. OutLayer agent on testnet with attestation | agent runs; attestation linked |
| 8–9 | Keeper script. First real mainnet trade with a small amount, with the explorer link | explorer link |
| 9–11 | Dashboard reads real chain data; attack scenarios on testnet | live data shown |
| 11–12 | Threat model, evidence site with real results, README | all links resolve |
| 13 | Freeze. Repository public. Branch protection on `main`. Clean-machine rerun | rerun passes |

Gates that must pass or the scope is cut:
- End of day 3: the vault wiring works in sandbox with the real defuse contract. If not, cut to a mock-verifier scope and say so.
- End of day 6: gas and latency measured. If the mainnet MPC call fails, the mainnet trade is replaced by a recorded failure with the cause.
- End of day 9: a real mainnet trade exists. If not, the claim "trade executed" is removed.

## 20. Risks to the project

| Risk | Likelihood | Impact | Response |
|---|---|---|---|
| Oracle returns no price for the traded assets | High today | Blocks the live trade | Try the feed first; otherwise scope to sandbox with a labelled mock feed |
| OutLayer limits or cost block the agent | Medium | Agent integration slips | Script agent is the required integration; OutLayer is additional |
| Solver relay access not approved | Medium | No solver quotes | Stop autonomous publication; evaluate a separately verified manual 1Click path later. No automatic fallback is implemented. |
| SHIELD key not approved | High (pending) | Gate absent | Reported as unavailable; not a claim |
| Scope too large for 13 days | High | Unfinished features | Scope cuts above, decided at gates |
| A competitor ships the same idea | Unknown | Less novelty | Differentiation in section 22 |
| Reviewers read the scope as overclaiming | Medium | Lower score | Claims policy in section 16 |

## 22. Related work and differentiation

- **OutLayer Agent Custody:** TEE-held wallet with policies. Policy decisions run inside the TEE; results are signed by the TEE worker. Mandate is complementary: the vault can accept OutLayer as an agent, and the enforcement moves on-chain.
- **Shade Agents:** TEE agents with Chain Signatures and an on-chain gate that checks attestations. Mandate's gate is the trade rules themselves, not attestation alone.
- **RHEA AI Vault:** planned, not confirmed live.

Mandate's distinct claim is narrow and checkable: the signing key cannot be used for a trade that the vault's own code has not accepted, and the rules are readable on-chain.

This depends on the sealed vault and external trust assumptions. Current
competitor feature/audit comparisons are unverified; no exclusive novelty or
superior security claim follows from these short descriptions.

## 23. Open decisions

- **Loosening delay:** 24 hours is implemented (`MANDATE_DELAY_SEC`); changing it requires a contract change.
- **Strategy:** deterministic two-asset target/band rebalancing and optional strict model decisions are implemented locally. TEE execution, provider provenance, selected strategy parameters and live operation still need evidence.
- **Funding:** mainnet account and a small amount, to be funded by the owner.
- **Solver key:** partner approval pending.
- **SHIELD key:** partner approval pending.

## 24. Glossary

- **Chain Signatures / MPC:** NEAR's network that signs for a contract-derived key on NEAR and other chains.
- **NEAR Intents:** intent-based settlement; `token_diff` is a signed balance change.
- **raw_ed25519:** the signature standard that signs the exact bytes of a JSON payload.
- **V1 nonce:** a versioned, expiring nonce that the verifier checks against a salt.
- **TEE:** trusted execution environment, a hardware-isolated runtime.
- **OutLayer:** a platform that runs WASM in TEEs and returns results to NEAR contracts.
- **SHIELD:** NEAR Intents' incident service for chain, token and bridge scopes.
