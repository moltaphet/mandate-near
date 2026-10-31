# Mandate — Architecture & Risk Register

> Status: design v1, 2026-10-31. Every external fact below is tagged
> **[V]** verified from official docs/source, or **[U]** unverified — must be
> proven in the Day 1–2 spike before we build on it.

## 1. One sentence

An autonomous trading agent whose strategy runs sealed inside a TEE, while a
NEAR smart contract is the **only** key that can move the treasury — so every
trade is checked on-chain against an owner-defined mandate, and nobody (agent,
keeper, or us) can move funds outside it.

## 2. Core security invariant

> **No signature that can move vault funds on NEAR Intents is ever produced
> unless the vault contract has checked the exact intent against the mandate.**

How we get there:

- Funds live in `intents.near` under the vault's own account `vault.near`.
- `execute_intents` (swaps / `token_diff`) **always** requires a signature from
  a public key registered to the signer account **[V]** (defuse
  `engine/mod.rs::execute_signed_intent`).
- The **only** key we register for `vault.near` on `intents.near` is an
  **MPC-derived ed25519 key** that belongs to the vault contract itself via
  Chain Signatures (`v1.signer`, domain 1 = Ed25519)
  **[V: proven end to end, see [spikes/R1.md](spikes/R1.md)]**.
- MPC signs only for its predecessor account, so only the vault contract
  can obtain signatures for that key, and it does so only after its own
  checks pass.
- Registering the key needs no signature: a contract can call
  `intents.near.add_public_key` itself, with 1 yocto attached, because
  `auth_by_predecessor_id` is on by default **[V]**.

Consequence: the agent, the keeper and the frontend are **untrusted for
safety**. Compromising any of them can at worst stop trading. It cannot steal
funds or break the mandate.

## 3. Components

| Component | Tech | Trust |
|---|---|---|
| `vault` contract | Rust, near-sdk 5.30.0 **[V]** | Root of safety |
| Mandate engine | pure Rust module inside `vault`, no I/O | Unit-tested exhaustively |
| Strategy agent | Rust → `wasm32-wasip2` on OutLayer (Intel TDX) **[V]** | Liveness only |
| Inference (optional) | NEAR AI Cloud, **Private TEE** models only **[V]** | Advisory, bounded |
| Price oracle | `price-oracle.near` (OutLayer TEE oracle) **[V docs] [U liveness/assets]** | Bounded by slippage + staleness rules |
| Settlement | NEAR Intents verifier `intents.near` + solver relay **[V]** | Atomic, `token_diff` enforces min-out |
| Keeper | TypeScript, permissionless | Liveness only |
| Dashboard | Next.js | Display only; verifies attestations client-side |

Pyth was dropped: Pyth Core no longer supports NEAR (since 2026-08-26 per Pyth
docs) **[V]**.

## 4. Trade lifecycle (two transactions, by design)

Splitting into two transactions is **required, not optional**. A single
transaction's 300 TGas would have to fund OutLayer (~120+ TGas per examples)
**[V]**, then balance and oracle reads, then the MPC sign.

```
TX-1  keeper → vault.request_decision()          (permissionless, rate-limited)
        vault → outlayer.near.request_execution(source = pinned wasm hash)   [V]
        (yield/resume, ≤200 blocks)                                          [V]
        agent (in TEE): reads market data, optional NEAR AI call,
                        gets solver quote, outputs ≤900 bytes:               [V limit]
                        {pair, amount_in, min_amount_out, quote_hash, reason_hash}
        vault.on_decision(): store Proposal{id, …, expires_at}

TX-2  keeper → vault.execute_proposal(id)
        vault → intents.near.mt_batch_balance_of(vault.near, assets)         [V]
        vault → price-oracle.near (fresh price, fail closed on staleness)    [V docs]
        vault: Mandate::check(proposal, balances, prices, state) → Ok | Err(reason)
        vault: build intent payload {signer_id: vault.near, verifying_contract:
               intents.near, deadline: now+120s, nonce: V1 nonce, intents:
               [token_diff{in: -amount_in, out: +min_amount_out}]}            [V shape]
        vault → v1.signer.sign(eddsa, hash(payload))                         [U]
        vault.on_signed(): store SignedIntent; emit event

OFF   keeper → solver-relay publish_intent(quote_hash, signed_data)          [V]
        solver's opposite intent settles atomically on intents.near
```

The keeper can withhold or delay, but cannot alter: the signature covers
amounts, deadline and nonce. `token_diff` guarantees we receive at least
`min_amount_out` or nothing executes.

## 5. Mandate (on-chain, owner-set)

| Rule | Check location |
|---|---|
| Asset allowlist (in and out) | `execute_proposal` |
| Max notional per trade (USD) | oracle price × amount |
| Max position weight after trade | balances + prices |
| Min-out ≥ oracle value × (1 − max_slippage_bps) | blocks bad quotes and oracle-vs-quote divergence |
| Daily turnover cap, min cooldown between trades | vault state |
| Drawdown circuit breaker: NAV < HWM × (1 − max_dd) ⇒ only de-risk trades (into stable) allowed | vault state |
| **Single outstanding signed intent**: no new signature until the previous deadline has passed | prevents stacked exposure from unpublished signatures |
| Oracle staleness bound; fail closed | `execute_proposal` |
| Owner kill switch; owner-only withdraw via `intents.near.ft_withdraw` (predecessor auth) **[V]** | owner methods |

v1 is **single owner**: an individual or a Sputnik DAO account. Pooled
depositor shares are deliberately out of scope. They add an inflation attack
and NAV-manipulation surface, and we do not ship that half-done.

## 6. What "private strategy, public guarantees" honestly means

Claims we make:

- The agent's code is pinned by sha256 (`version_key` / `WasmUrl.hash`). The
  worker refuses mismatched bytes **[V]**.
- The strategy's **parameters and signals** live in OutLayer `PROTECTED_`
  secrets bound to that wasm hash, invisible to us **[V]**.
- Every run has a TDX attestation (wasm hash, input/output hash) that can be
  verified off-chain **[V]**.
- Results reach the chain via the registered TEE worker key **[V]**.
- If NEAR AI is used, only **Private TEE** models are allowed (e.g.
  `Qwen/Qwen3.8-27B`, `deepseek-ai/DeepSeek-V4.1-Flash-1017`). Response
  signatures are verifiable (`/v1/signature/{id}`, EIP-191 or ed25519)
  **[V]**.

Claims we do **not** make:

- That the contract verifies the TDX quote per call. OutLayer deliberately
  doesn't **[V]**.
- That the strategy is profitable or "correct".
- That Incognito models are private.

## 7. Risk register

| # | Risk | Impact | Mitigation / fallback | Spike day |
|---|---|---|---|---|
| R1 | MPC eddsa sign from contract unusable for intents (args, payload size, derived key format) | **Critical**: core invariant | **Retired 2026-10-31: confirmed end to end, see [spikes/R1.md](spikes/R1.md).** Contract → MPC signature accepted by mainnet `simulate_intents` | 1 ✅ |
| R2 | Payload hash/serialization mismatch (NEP-413 vs `raw_ed25519`) | High | **Retired: `raw_ed25519` signs exact payload bytes; tamper test rejects** ([spikes/R1.md](spikes/R1.md)) | 1 ✅ |
| R3 | Solver relay access (partner key) / no quotes for tiny amounts | High | Get key on Day 1; test pairs USDC↔wNEAR first; 1Click as alternate quote source | 1 |
| R4 | OutLayer: HTTPS from WASM to relay/NEAR AI, cost, latency, 900-byte output | Medium | Hello-world project on testnet Day 2; output is a compact struct | 2 |
| R5 | `price-oracle.near` missing assets / stale | Medium | Restrict v1 universe to assets it serves; fail closed | 1 |
| R6 | Gas across callbacks | Medium | Already split into TX-1 / TX-2; measure in sandbox | 3 |
| R7 | Intents is mainnet-only **[V]** | Medium | Sandbox runs the **real defuse contract** with mock MPC/oracle/OutLayer; mainnet only for the final demo with a few dollars | — |
| R8 | Our own contract bugs | High | Mandate engine pure and property-tested; `#[private]` callbacks; no panics in callbacks; threat model doc | ongoing |

**Go / no-go: end of Day 2.** If R1 fails, switch to the fallback and change
the README claims accordingly. We do not hide it.

## 8. Reproducibility (for the AI screeners)

- `./scripts/verify.sh` runs:
  - mandate unit and property tests (seconds)
  - sandbox e2e with real defuse wasm and mocks: honest trade settles,
    each attack is rejected with a named reason
  - prints a pass/fail table
- No keys are needed for `verify.sh`. Mainnet scripts are separate and
  read-only by default (`simulate_intents`, dry quotes).
- README follows "do X → see Y". `THREAT_MODEL.md` lists what each component
  can and cannot do.

## 9. Demo attacks (each must fail, on camera)

1. A compromised agent proposes 100% into a non-allowlisted memecoin → `AssetNotAllowed`.
2. A malicious quote with 8% worse price than the oracle → `SlippageExceeded`.
3. A keeper alters the amount after signing → signature invalid at `intents.near`.
4. Rapid-fire small trades to drain via fees → `CooldownActive` / `TurnoverCapExceeded`.
5. A market crash triggers the drawdown breaker → only de-risk trades are accepted.

## 10. Tracks

Primary `defi`, plus `security`, `agents` and `privacy`. Each claim maps to
code a screener can run.

## 11. Repo layout

```
contracts/vault        Rust contract + mandate engine
contracts/mocks        mock MPC signer, oracle, OutLayer (sandbox only)
agent/                 OutLayer WASM strategy (wasm32-wasip2)
keeper/                TS: triggers TX-1/TX-2, publishes to solver relay
web/                   Next.js dashboard + attestation verification
scripts/verify.sh      one-command check
docs/                  ARCHITECTURE.md, THREAT_MODEL.md
```

## 12. Tooling versions (checked 2026-10-31) [V]

near-sdk 5.30.0 (Rust ≥1.95), cargo-near 0.22.0, near-workspaces 0.24.0,
near-cli-rs 0.30.1, near-api-js 7.4.0, @hot-labs/near-connect 0.12.0,
@defuse-protocol/one-click-sdk-typescript ≥0.1.24 (quote signature
verification), @nearai/inference-sdk.
