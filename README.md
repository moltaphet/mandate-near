# Mandate

**An AI trading agent that cannot break its rules, because it never holds the keys.**

The strategy runs sealed inside a TEE. The treasury sits on NEAR Intents
under a smart contract's own account, and the **only** key that can move it
belongs to that contract (via NEAR Chain Signatures). The contract signs a
trade only after checking it on-chain against a mandate the owner wrote. The
agent can be wrong, jailbroken or fully compromised, and the funds still
cannot leave the mandate.

> **Status:** in active development for NEAR HACKATHON (2026-11-01 → 2026-11-14).
> Every claim below is marked ✅ **built and tested** or 🛠 **planned**.
> The architecture's critical assumption is already proven on mainnet; see
> [Proof so far](#proof-so-far). **Not audited.** Read [SECURITY.md](SECURITY.md)
> and [THREAT_MODEL.md](THREAT_MODEL.md) before trusting it with funds.

---

## Why this matters

Giving an AI agent money today means trusting everything that touches its key:

- **The model**, against prompt injection or a bad output.
- **The operator**, who can change the code or take the key.
- **Every machine** the key has passed through.

Spending limits enforced *off-chain* protect nothing against whoever runs
that off-chain code. Funds managers face the same problem from the other
side: depositors must trust a black box, or the manager must publish their
strategy.

Mandate splits the two concerns:

| | Who controls it | Where it lives |
|---|---|---|
| **Intelligence** (what to trade) | the agent | private: TEE (OutLayer), optional NEAR AI private inference |
| **Authority** (what *may* be traded) | the owner's mandate | public and deterministic: NEAR smart contract |

## How a trade works

```mermaid
sequenceDiagram
    autonumber
    participant K as Keeper (anyone)
    participant V as Vault contract
    participant A as Agent (script today; OutLayer TEE planned)
    participant P as price-oracle.near
    participant M as MPC network (v1.signer)
    participant I as intents.near
    participant S as Solver

    A->>V: propose(asset_in, asset_out, amount_in, min_out, reason_hash)
    K->>V: execute()
    V->>I: read balances
    V->>P: read prices and publish times (fail closed if stale)
    Note over V: Mandate check: assets, size, position,<br/>slippage vs oracle, turnover, cooldown, drawdown
    V->>M: sign(exact intent bytes)   [only if every rule passes]
    M-->>V: Ed25519 signature
    K->>S: publish signed intent
    S->>I: settle atomically (token_diff guarantees min_out)
```

The keeper can delay a trade but cannot change it. The signature covers the
exact bytes: amounts, deadline and nonce.

## Who can do what

This is the design's trust model. Owner controls are ✅ built and tested
today; the trading-path guarantees arrive with the 🛠 items in the
[roadmap](#roadmap-to-submission).

| Actor | Can | Cannot |
|---|---|---|
| **Owner** | ✅ set the mandate (full-access key + 1 yocto; tightening at once, loosening after a 24 h delay, cancellable), ✅ pause instantly, ✅ unpause · 🛠 withdraw | exceed the gates, or take a trade past them |
| **Agent** (script today, TEE planned) | ✅ propose trades | move funds, exceed any limit, change the mandate |
| **Keeper** | trigger steps, delay | alter a signed trade, sign anything |
| **Solver** | fill a trade at ≥ `min_out` | take more than the signed `token_diff` |
| **Us (developers)** | ship new code (owner opts in) | sign for the vault; the key belongs to the contract |
| **Attacker with agent/keeper access** | stop trading | steal funds |

## Proof so far

The design rests on one assumption: *a contract can obtain an MPC signature
that NEAR Intents accepts as the vault's own.* This was tested first, before
building anything on top of it
([full report](docs/spikes/R1.md)).

| Test | Network | Result |
|---|---|---|
| Intent signed over exact JSON bytes is accepted | mainnet `intents.near` | ✅ |
| Same intent modified after signing | mainnet | ✅ rejected: `invalid signature` |
| Wrong `verifying_contract` | mainnet | ✅ rejected |
| **Contract → MPC (`v1.signer`) → signature accepted by `intents.near`** | testnet MPC + mainnet verifier | ✅ |
| MPC-signed swap from an unfunded account | mainnet | ✅ fails on balance only; the signature itself was valid |
| **Swap payload built by the vault's own code**, V1 nonce bound to the live salt | mainnet | ✅ passes signature, nonce and salt checks; fails on balance only |
| Same payload with a salt `intents.near` never issued | mainnet | ✅ rejected: `invalid salt` (the V1 nonce is parsed, not treated as legacy) |

Why it holds by construction: MPC verifies its own output with
`env::ed25519_verify`, the same host function the Intents verifier uses
([source references](docs/spikes/R1.md#findings-from-source-commit-level-reading-2026-10-31)).

## Try it

Requirement: [rustup](https://rustup.rs). The pinned toolchain (Rust 1.95.0)
installs itself from `rust-toolchain.toml`. No keys are needed. Full steps,
prerequisites and what they do not cover: [docs/RUN-WITHOUT-KEYS.md](docs/RUN-WITHOUT-KEYS.md).

1. **Run `./scripts/verify.sh`.** You should see formatting, clippy (warnings
   as errors) and the unit and property tests pass, ending with
   `All checks passed.` The count is printed in the run; it is not fixed here.
2. **Run `cargo test -p mandate-vault engine::`.** You should see every
   per-trade rule exercised at its exact boundary: cooldown, signed-intent
   lock, price staleness, trade size, UTC-day turnover, slippage, drawdown
   breaker and position cap. A property test also re-derives every rule
   for random trades. Add `-- --nocapture property_generator` to see the
   outcome mix: roughly 27% approved, plus every rejection type.
3. **Run `./scripts/verify.sh --integration`.** You should see the real wasm
   deployed to a local NEAR sandbox, with `owner_controls_on_a_real_chain`
   passing. That test covers:
   - re-init blocked
   - stranger rejected
   - `set_mandate` without 1 yocto rejected
   - an invalid mandate rejected
   - pause and unpause
   - NEP-330 source metadata pointing to this repo

   It also runs `vault_registers_its_mpc_key_on_the_real_intents_contract`.
   That test deploys the **exact mainnet `intents.near` code**
   (`contracts/vault/tests/fixtures`, verifiable against the chain with
   `scripts/fetch-intents-fixture.sh`) next to the vault. The vault asks the
   MPC signer for its derived key and registers it on `intents.near` with
   predecessor auth. The test then reads the key back from the real
   contract, and refuses a second registration. The MPC signer in that test
   is a stand-in that enforces the call convention; the real MPC network is
   not used.

   The first run downloads the sandbox (needs `cargo-near`). The same run
   includes `tests/attacks.rs`, the eight attacks below.
4. **Run `./scripts/attack-demo.sh`.** It plays eight attacks against the
   contract on a local sandbox and writes what the vault answered to
   [docs/evidence/attacks.txt](docs/evidence/attacks.txt). Each line states the
   expected refusal and the observed one.
5. **Run `./scripts/gas-report.sh`.** It records the gas each vault call burns, in [docs/evidence/gas.txt](docs/evidence/gas.txt). A full `execute` with the MPC sign burnt about 30 TGas in the sandbox.
6. **Run `./scripts/check-claims.sh`.** It checks that each test name and path
   cited in these docs still exists. It does not run the tests; run the named test
   to check a claim.
7. **Run `cd spikes/r1-intents-signing && cargo run --release`.** You should
   see 6 PASS lines from live mainnet `simulate_intents`, a free view call
   that needs no account. The last two use payloads built by
   `mandate_vault::intent`, the contract's own code.

## Mandate rules

Enforced per trade by `contracts/vault/src/engine.rs`, a pure function
that is unit- and property-tested. Every USD value is exact 256-bit
integer math, rounded against the trade: exposure up, receipts down.
The vault calls the engine on-chain: it reads live balances and oracle
prices, runs the gates, and only then calls `sign`. That path is tested on
a sandbox with the real `intents.near` code and a stand-in MPC signer. It
has not run on mainnet.

| Rule | Purpose | Engine | On-chain wiring |
|---|---|---|---|
| Asset allowlist (max 32, each with decimals and price feed), stable subset | the agent trades only what the owner chose | ✅ | ✅ validated on `set_mandate` |
| Max notional per trade | bounds the damage of any single decision | ✅ | ✅ sandbox (attack 3) |
| Daily turnover, resets 00:00 UTC | bounds damage per day | ✅ | ✅ wired; engine tests |
| Min-out ≥ oracle value − slippage (hard cap 5%) | blocks bad or manipulated quotes | ✅ | ✅ sandbox (attack 4) |
| Every traded **or held** asset needs a fresh price (≤ max age, ≤ 5 s clock skew) | no trading or valuation on stale prices | ✅ | ✅ sandbox (attacks 5, 6) |
| Cooldown between trades | stops drain-by-many-small-trades | ✅ | ✅ wired; engine tests |
| Drawdown breaker vs high-water mark | after a loss, only trades into stable assets | ✅ | ✅ wired; engine tests |
| Max position weight, worst case (received value counted at max(min_out, notional)) | no all-in bets | ✅ | ✅ wired; engine tests |
| One signed intent at a time (120 s TTL) | no stacked exposure from unpublished signatures | ✅ | ✅ sandbox (`a_signed_intent_blocks_the_next_proposal`) |
| Exact intent bytes: `raw_ed25519` payload, V1 expirable nonce | the signature commits to amounts, deadline and nonce | ✅ proven on mainnet | ✅ sandbox (attack 7: the real verifier rejects an altered amount) |
| Kill switch: `pause` works with a function-call key; `unpause` needs a full-access key | stop fast, resume deliberately | — | ✅ unit + sandbox tested |

## Built on NEAR

| Component | Role in Mandate | Status |
|---|---|---|
| **NEAR Intents** (`intents.near`) | holds the treasury; atomic `token_diff` settlement | ✅ verified via `simulate_intents` · ✅ key registration and execute path tested on the exact mainnet code in a sandbox · 🛠 live trades |
| **Chain Signatures** (`v1.signer`, Ed25519) | gives the contract its own signing key | ✅ proven contract → MPC → Intents · ✅ sandbox execute path with a stand-in signer · 🛠 real MPC call from the vault on a live chain (testnet blocked: `intents.testnet` has no contract) |
| **OutLayer** (Intel TDX) | runs the agent with a pinned code hash; secrets sealed to that hash | 🛠 |
| **NEAR AI Cloud** (Private TEE models only) | optional private inference with verifiable signatures | 🛠 |
| **price-oracle.near** | fresh prices for slippage, position and drawdown checks | ✅ read on-chain with fail-closed sandbox tests · ⚠ on mainnet it returned no fresh price for wNEAR or USDC (2026-10-31) |
| **NEP-330** | reproducible build: anyone can verify the deployed wasm | ✅ |

## Roadmap to submission

- [x] Architecture, trust model, risk register ([docs/ARCHITECTURE.md](docs/ARCHITECTURE.md))
- [x] Vault contract: mandate config, owner controls, unit and sandbox tests, CI
- [x] **R1 proven**: the contract is sole signer via MPC, accepted by mainnet Intents
- [x] Per-trade mandate engine (exact math, property-tested) and intent builder (V1 expirable nonces), accepted by mainnet `intents.near`
- [x] Vault registers its MPC-derived key on the real `intents.near` code (sandbox, MPC stand-in)
- [x] Vault signs through the gates and the MPC call, with live balances and oracle prices (sandbox, stand-in signer)
- [x] Oracle integration with a staleness bound (sandbox)
- [x] Owner controls: 24 h delay on loosening, cancel, simulate (sandbox, and testnet)
- [x] Script agent and keeper binaries (sandbox-tested)
- [x] Threat model and self-review ([THREAT_MODEL.md](THREAT_MODEL.md), [SECURITY.md](SECURITY.md))
- [ ] Run the signing path on a live chain (mainnet, or testnet once `intents.testnet` exists)
- [ ] OutLayer agent (Rust → `wasm32-wasip2`) with yield/resume decision flow
- [ ] Solver-relay publishing
- [ ] Withdraw (owner, signed intent, own gates)
- [ ] Dashboard with live attack demos (each must fail on camera)
- [ ] First real mainnet trade with a few dollars
- [ ] Demo video, submission

## Not claimed

- That the strategy is profitable. Mandate bounds risk; it does not create alpha.
- That the contract verifies TEE quotes on-chain. OutLayer results are
  trusted via registered TEE worker keys; quotes are verifiable off-chain.
- That Incognito-tier NEAR AI models are private. Only Private TEE models are used.

## Repository layout

```
contracts/vault/        vault contract (Rust, near-sdk 5.30)
  src/mandate.rs        mandate config and validation
  src/engine.rs         per-trade enforcement (pure; unit + property tests)
  src/math.rs           exact USD valuation (256-bit, conservative rounding)
  src/intent.rs         raw_ed25519 token_diff payload + V1 nonce builder
  tests/sandbox.rs      owner controls on a local NEAR sandbox
  tests/attacks.rs      eight attacks, each refused on the sandbox
agent/                  script agent: proposes one trade, reports the decision
keeper/                 permissionless keeper: runs execute, backs off on failure
THREAT_MODEL.md         what is protected, from whom, and what is not
SECURITY.md             reporting, self-review findings, checklist before funds
docs/ARCHITECTURE.md    design, trust model, risk register
docs/TESTNET.md         testnet run: what worked, what did not, the cost
docs/evidence/          recorded output (attacks.txt)
docs/spikes/R1.md       R1 evidence with source and on-chain references
scripts/verify.sh       one-command check (add --integration for sandbox)
scripts/attack-demo.sh  plays the eight attacks and records the output
scripts/check-claims.sh checks that cited tests and paths exist
.claude/skills/         official NEAR agent skills, vendored unmodified
```

## Tracks

Primary **AI Security & Defense** and **Unstoppable DeFi**, also
**Agent Economy** and **Privacy & Private AI**.

## Hackathon disclosure

- All code was written during the NEAR HACKATHON window, starting
  2026-10-31. There is no pre-hackathon baseline.
- Libraries: `near-sdk`, `uint` (256-bit integers); dev-only `near-sandbox`,
  `near-api`, `cargo-near-build`, `tokio`, `testresult`, `proptest`, and
  `chrono` (used only as a test oracle). The spikes also use
  `ed25519-dalek`, `ureq`, `serde_json`, `bs58`, `base64`, `chrono`, `rand`
  and `hex`.
- Development is AI-assisted (Claude Code), using the official NEAR agent
  skills (`npx skills add near/agent-skills`), vendored unmodified in
  `.claude/skills/` and pinned in `skills-lock.json` (near/agent-skills@cb32b6a, MIT).

## License

[MIT](LICENSE)
