# Threat model

What Mandate protects, from whom, and what it does not. Each threat names the
mechanism that answers it and the test that shows it. Attack numbers refer to
[`docs/evidence/attacks.txt`](docs/evidence/attacks.txt), recorded by
[`scripts/attack-demo.sh`](scripts/attack-demo.sh). Open problems are listed in
[SECURITY.md](SECURITY.md#self-review-findings) and are not repeated as solved here.

Status words: **built and tested** means a test in this repo runs it.
**planned** means it is not in the code. **not verified** means the claim rests
on reading, not on a run.

## Assets

| Asset | Where it lives | Who can move it today |
|---|---|---|
| Treasury balances | `intents.near`, credited to the vault account | Only a signed `token_diff` the vault produced after every gate passed |
| Mandate (the rules) | vault state | Owner: tightening at once; loosening after a 24 h delay |
| Signing authority | MPC key derived for the vault account (`v1.signer`) | Only the vault contract, through `sign` |
| Owner key | a NEAR full-access key held by the owner | Owner (see T3) |
| Agent key | an account the owner names with `set_agent`; a later change waits 24 h | Can propose only (see T2) |

There is **no withdraw path yet** (planned). Funds in the vault can change
form through mandate-approved trades but cannot leave by an owner call. See SR-6.

## Actors and trust

| Actor | Trusted for | Not trusted for |
|---|---|---|
| Owner | writing the mandate, pausing | nothing else: cannot bypass gates |
| Agent (the strategy, in a TEE once OutLayer is wired) | choosing a trade | anything the gates check |
| Keeper, any stranger | running `execute` and `register_intents_key` | choosing or changing a trade |
| Solver / relay | filling a signed intent at or above `min_amount_out` | altering bytes; taking more than the signed diff |
| `price-oracle.near` | returning a price and its publish time | being right; the vault bounds the damage (T6) |
| `v1.signer` (MPC) | signing only what the vault asks for, under its derivation rules | nothing on the vault's behalf beyond the call |
| `intents.near` | balances, salts and the signature check | assumed correct: it is the audited mainnet code |

## Threats

### T1. Compromised or jailbroken agent proposes harmful trades

The agent is the most exposed component: it reads untrusted market data and,
in the planned design, runs in a TEE with a pinned code hash.

**Mechanism.** Every proposal is checked on-chain at execution against the
mandate: asset allowlist, per-trade size, UTC-day turnover, slippage against
the oracle value, price freshness, cooldown, drawdown breaker (de-risk only),
position weight, and one signed intent at a time (`contracts/vault/src/engine.rs`).

**Tests.** Attacks 1 to 6 in `contracts/vault/tests/attacks.rs`; per-rule
boundary tests and a property test in `engine.rs`; three-strike halt in
`tests/execute_path.rs`.

**Residual.** A strategy inside the limits can still lose money.
The worst loss from slippage in one day is bounded by
`max_daily_turnover_usd × max_slippage_bps` (the engine caps slippage at 5%).
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
- `pause` stops execution at once. `unpause` and `set_mandate` need a full-access key.

**Tests.** Attack 8 (a $150 cap queued, the $60 trade still refused, early apply
refused); unit tests in `lib.rs` for the queue rules.

**Mechanism for the agent.** A later change of agent is queued for 24 h
(`set_agent`, `apply_pending_agent`, `cancel_pending_agent`). The first agent
applies at once, since no agent can propose before it is set. The current agent
keeps its rights during the delay, and `pause` stops execution at once.
Tests: `a_new_agent_waits_24_hours_on_a_real_chain` in `tests/sandbox.rs`, and
the `set_agent` unit tests in `lib.rs`. SR-5 is fixed.

**Residual.**
- A stolen owner key can still pause the vault, and can still queue a new
  agent. The agent then takes over after 24 h unless the owner cancels, so the
  owner has one day to notice. Gates still apply, so the damage is limited to
  mandate-approved trades.
- The owner can tighten a mandate to a point that stops all trading. That is a
  liveness loss, not a theft.
- Recommended before real funds: a DAO or multisig as owner. The DAO test is
  planned, not built.

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

**Residual.** A solver can decline to settle. The intent then expires after
120 s and the next proposal can run. Lost opportunity, not lost funds.

### T6. Oracle wrong, manipulated or stale

**Mechanism.** Prices come from `price-oracle.near`, read as `get_price_data`
and `get_asset`. The vault checks the publish time of each feed, not only the
oracle's own answer: age at most `max_price_age_sec`, future skew at most 5 s.
A missing or stale price rejects the trade (attacks 5 and 6), so a broken
oracle stops trading instead of mispricing it.

**Residual.** A manipulated price that is still fresh passes the age check. The
vault then limits the damage with the slippage floor (`min_out` against the
oracle value), the size cap and the position cap. Slippage protection is only
as good as the oracle, so the oracle is trusted for correctness. Accepted.

**Current state.** On mainnet, `price-oracle.near` returns no fresh price for
wNEAR or USDC (checked 2026-10-31). The vault fails closed there, so a mainnet
trade is blocked until a fresh feed exists or a labelled sandbox feed is used.

### T7. Replay of a signed trade

**Mechanism.** Single outstanding signed intent (the next proposal waits for the
deadline); V1 nonce bound to the live salt and deadline; `intents.near` rejects
reuse of a nonce. The vault refuses a proposal while a signed intent is
outstanding (`propose` and `execute` both check).

**Tests.** `a_signed_intent_blocks_the_next_proposal` in `tests/execute_path.rs`.

### T8. Concurrent executions or proposals corrupt state

**Mechanism.** `execute` sets an in-flight lock before its first cross-contract
call and refuses a second execution while the lock is set. State that changes
the outcome (trading state, signed intent) is written only in the final
callback. Callbacks are `#[private]`.

**Found and fixed in this change.** `propose` did not check the lock, so an
agent proposal made while an execution was in flight could be dropped when that
execution finished (SR-2). Fixed by refusing `propose` while in flight.

**Open.** The lock has no recovery path (SR-1). If a callback never finishes,
the vault stops executing for good, and with no withdraw path (SR-6) the funds
stay where they are.

### T9. Mandate changes that are not in the mandate

**Mechanism.** The mandate is validated on every write, so an invalid mandate
cannot be stored (for example, a per-trade cap above the daily turnover cap).

**Tests.** `simulate_reports_the_effect_and_changes_nothing`, the sandbox owner-control test.

### T10. Key leaks outside the chain

**Mechanism.** Keys live in environment variables or a file outside the repo
with mode 600. The repo has a gitleaks pre-commit hook. The keeper and agent
read keys from the environment only.

**Residual.** The testnet key file exists on the developer machine. It holds
testnet funds only. No mainnet key exists yet.

### T11. Deployment mistakes

**Mechanism.** `new` and `new_with_externals` are `#[init]`, so they run once.
The NEP-330 metadata names the source repo, so anyone can rebuild the wasm and
compare its hash.

**Residual (not verified).**
- If the vault account keeps a full-access key, its holder can deploy any code
  and sign through the MPC key. The vault account must have no full-access
  keys before it holds funds (SR-9).
- Deploying code and calling `new` in separate transactions lets anyone
  initialize the vault first. Deploy and init must be one transaction.
- `new_with_externals` lets the deployer choose the signer, intents and oracle
  accounts. Users must check `get_externals` before trusting a deployment.

### T12. Gas exhaustion of `execute`

**Mechanism.** The execute chain makes one read per held or traded feed. The
mandate caps assets at `MAX_ASSETS` (32).

**Residual (not verified).** The gas needed for 32 feeds has not been measured.
If it exceeds what one transaction can carry, `execute` fails in a callback,
which leaves the lock set (SR-1, SR-3). Measurement is planned.

## Out of scope

- Compromise of NEAR consensus, `v1.signer` or `intents.near`: the design
  assumes the audited mainnet code and the MPC network behave as documented.
- The TEE (OutLayer): not built yet. Until it is, the agent is a script run by
  the developer, and the README says so.
- Market profitability and strategy quality.
- Front-end and dashboard data: not built.
