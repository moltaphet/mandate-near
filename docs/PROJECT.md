# Mandate — Project Definition

Status legend used throughout:
**[PROVEN]** demonstrated on NEAR mainnet · **[TESTED]** covered by a test that reruns from this repo · **[PLANNED]** designed, not built · **[OPEN]** a decision not yet taken.

---

## 1. One sentence

Mandate is an on-chain policy layer for NEAR agents: a smart contract that holds the only key to an agent's treasury on NEAR Intents and signs a trade only when it satisfies an owner-defined mandate.

## 2. Problem

An AI agent that moves money is only as safe as the code that decides whether a trade is allowed. Today that decision usually lives inside the agent: a `if amount > cap` check in the agent's own process. Whoever controls that process controls the money. Prompt injection, a compromised host, a bug, or a malicious operator can each bypass it, and the owner has no independent way to know which rules were enforced.

Two existing approaches leave gaps:

- **Rules inside the agent's code.** Easy to write, but unverifiable and bypassable by anyone who controls the process.
- **Rules inside a TEE-hosted wallet** (for example OutLayer Agent Custody). The policy runs inside the TEE, and the result arrives as a signed message. The owner must trust the TEE, its operator and its attestation, and the chain does not see the decision itself.

Mandate moves the decision to a place the owner can read and the chain can enforce: the contract's own code.

## 3. Thesis

Enforcement point matters more than policy richness. If the contract is the only signer, then a trade that violates the mandate cannot be signed no matter what the agent, keeper or operator does. The contract's rules are public, versioned and testable, and the signature exists only after they pass.

## 4. Scope of version 1

In scope:
- One vault contract per treasury, with one owner account (an individual account or a Sputnik DAO account).
- A nine-gate trade check, executed on-chain before any MPC signature.
- Signing through NEAR Chain Signatures (`v1.signer`, Ed25519) for the vault's own NEAR Intents account.
- Settlement through NEAR Intents `token_diff` intents with V1 expirable nonces.
- An SDK and a stable interface so that any agent can propose trades.
- Two agent integrations: an OutLayer-hosted agent and a plain script agent.
- A keeper script that anyone can run.
- A `simulate` view for owners to test a proposal against the current mandate.
- A 24-hour delay on mandate changes that loosen limits.
- An optional external gate from SHIELD (NEAR Intents incident service), once the partner key is approved.

Out of scope for version 1 (stated, not hidden):
- Pooled deposits from several investors, shares and NAV accounting. This adds inflation and manipulation surfaces we do not want to ship half-built.
- Cross-chain withdrawals directly from the contract. Settlement stays inside NEAR Intents.
- Automatic strategy discovery. The agent proposes; the mandate decides.
- Any claim that the strategy is profitable.

## 5. Actors and trust

| Actor | Can do | Cannot do | What enforces the limit |
|---|---|---|---|
| Owner (account or DAO) | Set mandate, pause, unpause, withdraw | Bypass the gates on a trade; change a limit to looser without the delay | Contract: full-access key and 1 yoctoNEAR for `set_mandate` and `unpause`; 24-hour delay for loosening |
| Agent (OutLayer TEE or script) | Propose a trade with a minimum output | Sign, withdraw, change the mandate, skip a gate | Contract. The agent holds no key on the vault account. |
| Keeper (anyone) | Trigger the next step, delay a trade | Change a trade or signed bytes; sign anything | Contract. Signed bytes fix amount, deadline, nonce. |
| Solver | Fill the signed trade at or above the minimum | Take more than the signed `token_diff` | NEAR Intents verifier |
| MPC network | Sign a payload the vault asks for | Be asked by anyone other than the vault | Chain Signatures: the derived key is bound to the vault account |
| Oracle operator | Publish prices | Set prices the contract trusts without a freshness check | Contract: staleness bound, fail closed |

Residual trust, stated explicitly:
- The MPC network has a threshold of signers (currently about eight nodes). We depend on its threshold assumption.
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
- **Intent builder**: `raw_ed25519` `token_diff` payload and V1 nonce. Built and proven on mainnet `simulate_intents`.
- **Keeper script** (TypeScript): anyone can run it. [PLANNED]
- **Agent integrations**: OutLayer agent [PLANNED], script agent [PLANNED].
- **SDK**: typed client over the vault's public methods. [PLANNED]
- **Evidence site and dashboard**: static, with simulated data labelled. Built as prototypes.

## 7. Trade lifecycle

Two transactions by design, because one would exceed the gas budget once oracle reads, balance reads and the MPC call are combined. [PLANNED for the on-chain wiring; the gate engine and intent builder are built.]

**Transaction 1, decision.**
1. Agent runs in OutLayer (or as a script) and produces a proposal: pair, amount in, minimum out, a reason hash.
2. The proposal is stored with an expiry. A new proposal cannot replace an unexecuted one before its expiry.

**Transaction 2, execution.** Permissionless: any account can call it.
1. Call `request_price_data` for each asset the trade touches or the vault holds.
2. Read prices. Any missing or stale price is a `MissingPrice` or `StalePrice` rejection. The contract fails closed.
3. Read vault balances on `intents.near`.
4. Run the nine gates in fixed order (section 8). The first closed gate rejects with a named code.
5. If all pass, build the `token_diff` payload and V1 nonce with the live salt.
6. Call `v1.signer.sign` with the EdDSA payload, attaching the required deposit and gas.
7. Store the signed intent and update trading state: last trade time, daily turnover, high-water mark, intent expiry.
8. Emit an event with the intent hash, mandate version, and result.

**Off-chain publication.** A solver or keeper publishes the signed intent to the solver relay. The solver settles atomically; `token_diff` guarantees at least the signed minimum or nothing.

## 8. Mandate

### 8.1 Fields

- `assets`: up to 32 entries. Each has `asset_id` (NEAR Intents id), `decimals`, `price_feed_id`.
- `stable_assets`: subset of `assets`, used for de-risking.
- `max_trade_usd`, `max_daily_turnover_usd`: micro-dollars.
- `max_position_bps`: cap on any non-stable asset's weight after a trade.
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
| 6 | Daily turnover | `TurnoverCapExceeded` | Today's turnover plus notional ≤ cap; the day resets at 00:00 UTC |
| 7 | Slippage | `SlippageExceeded` | Value of `min_out` (rounded down) ≥ notional × (1 − slippage) |
| 8 | Drawdown lock | `DrawdownDeRiskOnly` | If NAV < high-water mark × (1 − drawdown), only trades into stable assets |
| 9 | Position limit | `PositionTooLarge` | Unless the destination is stable, worst-case position ≤ `max_position_bps` of worst-case NAV |

Design choices, stated:
- Exposure rounds up and receipts round down, so every boundary errs toward rejection.
- The received asset is valued at the larger of `min_out` and notional, because a solver may deliver more than the minimum.
- NAV counts only mandate assets, so airdropped tokens cannot inflate it.
- The high-water mark only rises, and only on approved trades.

Status: gates 1–9 are built and tested at the engine level. On-chain wiring [PLANNED].

## 9. Owner controls

| Method | Who | Deposit | Effect |
|---|---|---|---|
| `set_mandate` (tightening) | owner | 1 yocto | Applies immediately |
| `set_mandate` (loosening) | owner | 1 yocto | Queued; applies after 24 hours; can be cancelled by owner |
| `cancel_mandate_change` | owner | 1 yocto | Cancels a queued change |
| `pause` | owner | none | Stops all new executions immediately |
| `unpause` | owner | 1 yocto | Resumes execution |
| `simulate` | anyone (view) | none | Runs the gates on a proposal against current state and returns the first failing gate, or approval, without changing state |
| `withdraw` | owner | 1 yocto | Withdraws via `intents.near` predecessor auth |

**Definition of loosening.** A change is loosening if it increases `max_trade_usd`, `max_daily_turnover_usd`, `max_position_bps`, `max_slippage_bps`, or `max_drawdown_bps`, decreases `min_trade_interval_sec` or `max_price_age_sec`, adds an asset, or moves an asset from non-stable to stable (which would relax the drawdown rule). Any change that is not loosening applies immediately. A change that mixes both directions is treated as loosening.

**Why these rules.** Tightening should be instant, since it reduces risk. Loosening needs a delay, so a compromised owner key cannot raise limits and drain the vault in one step. The owner can cancel a queued change, and the pause switch always works.

Status: owner controls for set, pause and unpause are built and sandbox-tested. The delay, `simulate` and `withdraw` are [PLANNED].

## 10. Owner identity

The owner can be:
- an individual NEAR account, or
- a Sputnik DAO account, so that `set_mandate` and `unpause` need a multisig vote.

The contract must not assume the owner is an individual. [PLANNED: one sandbox test with a DAO owner.]

## 11. Oracle policy

- Prices are requested immediately before a trade with `request_price_data`, then read.
- The contract accepts a price only if its publish time is within `max_price_age_sec`.
- A zero price, an exponent above zero, or an exponent below −30 is a valuation error, never a panic.
- Current state, observed on mainnet: `price-oracle.near` returns `null` for `wrap.near` and for the USDC asset id in `get_price_data`. The feed must be requested and confirmed working before any trade. If it still returns no price, trading stays off and the evidence says so.
- Pyth no longer supports NEAR (per Pyth's own documentation), so it is not used.
- As a second check, the solver quote is compared with the oracle value. [PLANNED]

## 12. Agent integrations

### 12.1 OutLayer agent [PLANNED]
- Rust compiled to `wasm32-wasip2`, pinned by SHA-256.
- Runs in an Intel TDX TEE on OutLayer; results return to the vault through yield and resume.
- Secrets, if any, are bound to the wasm hash.
- Outputs a proposal within the 900-byte result limit.

### 12.2 Script agent [PLANNED]
- A plain TypeScript process using the SDK, with no TEE.
- Exists to prove the vault is agent-agnostic: any process can propose; only the contract decides.

Acceptance for both: each agent makes the same proposals through the same SDK, and the vault treats them identically.

## 13. SDK [PLANNED]

- Typed client over: `get_mandate`, `get_trading_state`, `simulate`, `propose`, `execute`, `cancel_proposal`, owner methods.
- Stable public interface. Breaking changes require a new contract version.
- Target: a new agent integrates in under 100 lines and under 4 hours for a developer familiar with NEAR. Measured, not assumed.

## 14. Keeper [PLANNED]

- A permissionless script that watches for expired or pending proposals and calls `execute`.
- Anyone can run it. The vault does not trust it.
- If no keeper runs, no trade happens. This is a fail-safe, and is documented as such.

## 15. SHIELD gate [OPEN, PARTNER KEY PENDING]

- NEAR Intents SHIELD exposes incident status for chains, tokens and bridges.
- When the key is approved, the agent checks SHIELD before proposing and refuses to propose a trade touching an incident scope.
- SHIELD is an extra caution layer. It can only make the system stricter, so a bypass of this check removes a precaution but does not allow an unapproved trade.
- Until the key arrives, the integration reports "SHIELD unavailable" and proceeds only with the rest of the gates.

## 16. Claims policy

Every claim on the README or evidence site carries one status: [PROVEN], [TESTED], [PLANNED] or [OPEN].
- A [PROVEN] or [TESTED] claim must name the test or script that demonstrates it.
- A CI job checks that each named test or script exists and runs. [PLANNED]
- No demo, screenshot or dashboard value may be presented as on-chain. Simulated data is labelled wherever it appears.

## 17. Threat model (summary)

| Threat | Mitigation | Residual |
|---|---|---|
| Agent is prompt-injected | Agent cannot sign; gates reject out-of-mandate proposals | A permitted trade can still be a bad trade within limits |
| Agent host compromised | No key on the agent; proposals are checked | Denial of service only |
| Keeper malicious | Keeper cannot alter signed bytes or skip gates | Can delay |
| Owner key stolen | Loosening delayed 24 h; pause instant; cancel available | Attacker can wait out the delay, then loosen |
| Owner key lost | Pause still works with a function-call key; DAO owner recommended | Funds locked if no one can act |
| Stale or manipulated price | Freshness bound; fail closed; solver cross-check | Oracle operator collusion |
| MPC signer collusion | Threshold of MPC nodes | Threshold assumption |
| Solver under-delivers | `token_diff` requires minimum or nothing | None within the trade |
| Mandate misconfigured | `simulate` before going live; validation on set | Owner error is still possible |
| Re-initialisation | `#[init]` guard | — |
| Stuck signed intent | Expiry; one outstanding intent at a time | Blocks new trades until expiry |

A full threat model document is planned as a separate deliverable.

## 18. Metrics we will publish

Each has a method and a target, and a result will be shown only if measured.

| Metric | Method | Target |
|---|---|---|
| Integration time for a new agent | Timed run by a developer who did not write the SDK | under 4 hours |
| Integration size | Lines of agent code to propose a trade | under 100 lines |
| Attacks rejected | Sandbox run of each attack class | all rejected with the expected code |
| Gas per trade | Sandbox and mainnet | recorded in USD |
| Latency per trade | Time from proposal to signed intent | recorded in seconds |
| Approval rate under a realistic mandate | Property test and replayed proposals | recorded, not targeted |

## 19. Plan and milestones

Thirteen days remain before submission on 2026-11-14. Each milestone ends with evidence that reruns from this repository.

| Days | Milestone | Evidence |
|---|---|---|
| 1–3 | Vault wiring in sandbox with the real defuse contract: oracle request, MPC key registration on `intents.near`, deposit path | sandbox tests pass |
| 3–5 | `execute` and `propose`: balances, prices, gates, MPC sign, intent storage. One test per gate code | tests per code |
| 5 | Governance: loosening delay, cancel, `simulate`, DAO-owner test | tests pass |
| 5–6 | Gas measurement on sandbox and mainnet, with a funded account | recorded table |
| 6–8 | SDK and script agent. OutLayer agent on testnet with attestation | agent runs; attestation linked |
| 8–9 | Keeper script. First real mainnet trade with a small amount, with the explorer link | explorer link |
| 9–11 | Dashboard reads real chain data; attack demo on mainnet | live data shown |
| 11–12 | Threat model, evidence site with real results, README, 90-second video | all links resolve |
| 13 | Freeze. Repository public. Branch protection on `main`. Clean-machine rerun | rerun passes |
| 14 | Submission, after the owner approves the payload | submission record |

Gates that must pass or the scope is cut:
- End of day 3: the vault wiring works in sandbox with the real defuse contract. If not, cut to a mock-verifier demo and say so.
- End of day 6: gas and latency measured. If the mainnet MPC call fails, the mainnet trade is replaced by a recorded failure with the cause.
- End of day 9: a real mainnet trade exists. If not, the claim "trade executed" is removed.

## 20. Risks to the project

| Risk | Likelihood | Impact | Response |
|---|---|---|---|
| Oracle returns no price for the traded assets | High today | Blocks the live trade | Try the feed first; otherwise scope to sandbox with a labelled mock feed |
| OutLayer limits or cost block the agent | Medium | Agent integration slips | Script agent is the required integration; OutLayer is additional |
| Solver relay key not approved | Medium | No solver quotes | 1Click as fallback |
| SHIELD key not approved | High (pending) | Gate absent | Reported as unavailable; not a claim |
| Scope too large for 13 days | High | Unfinished features | Scope cuts above, decided at gates |
| A competitor ships the same idea | Unknown | Less novelty | Differentiation in section 22 |
| Judges read the scope as overclaiming | Medium | Lower score | Claims policy in section 16 |

## 21. Hackathon fit

- **Track:** AI Security & Defense (primary). Also Agent Economy, Autonomous Business, and Unstoppable DeFi where the trade flow applies.
- **Chain use:** Chain Signatures, NEAR Intents, OutLayer, NEAR AI (optional, private models only), NEP-330.
- **Eligibility:** code written during the window, repository public before submission, a demo post on X, and a NEAR account for payout.
- **Disclosure:** the repository states that all code was written during the window and that development is AI-assisted.

## 22. Related work and differentiation

- **OutLayer Agent Custody:** TEE-held wallet with policies. Policy decisions run inside the TEE; results are signed by the TEE worker. Mandate is complementary: the vault can accept OutLayer as an agent, and the enforcement moves on-chain.
- **Shade Agents:** TEE agents with Chain Signatures and an on-chain gate that checks attestations. Mandate's gate is the trade rules themselves, not attestation alone.
- **RHEA AI Vault:** planned, not confirmed live.

Mandate's distinct claim is narrow and checkable: the signing key cannot be used for a trade that the vault's own code has not accepted, and the rules are readable on-chain.

## 23. Open decisions

- **Loosening delay:** 24 hours is proposed. Owner-approved.
- **Strategy:** a rebalancer toward target weights, with an optional regime input from a private NEAR AI model, or a rebalancer without AI. Owner-approved.
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
