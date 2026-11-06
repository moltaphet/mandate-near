<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="web/assets/logo-light.svg">
    <img src="web/assets/logo.svg" alt="Mandate logo" width="360">
  </picture>
</p>

# Mandate

**On-chain trade authorization for agents that hold no treasury signing key.**

The Rust reference agent accepts a manual plan, runs a deterministic two-asset
rebalancer, or requests a strictly validated model decision using a public
final-block snapshot. Model output has no treasury authority. Response
provenance and live provider operation are unverified; OutLayer integration is
planned. The treasury
sits on NEAR Intents under the vault account. The vault requests a Chain
Signatures signature only after checking a proposed trade against the owner's
mandate. The owner also has a separate, delayed withdrawal path. A compromised
agent can still choose harmful trades inside the limits: this controls trading
authority, not profitability. Custody depends on a correctly deployed vault and
the trusted NEAR Intents and MPC contracts.

> **Status:** experimental prototype; submission target 2026-11-14 23:59 UTC.
> The vault execution path is implemented and sandbox-tested with a stand-in
> signer and oracle. R1 demonstrates signature interoperability using mainnet
> simulation; it is not a funded trade from the deployed vault.
> **Not professionally audited.** Read [SECURITY.md](SECURITY.md)
> and [THREAT_MODEL.md](THREAT_MODEL.md) before trusting it with funds.
> Recent autonomous-client and unique-voucher recovery changes are implemented;
> final combined release verification and live operation remain open.

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
| **Intelligence** (what to trade) | the agent | manual plan, deterministic two-asset strategy, or validated model output; OutLayer and private inference planned |
| **Authority** (what *may* be traded) | the owner's mandate | public and deterministic: NEAR smart contract |

## How a trade works

```mermaid
sequenceDiagram
    autonumber
    participant K as Keeper (anyone)
    participant V as Vault contract
    participant A as Agent (Rust today; OutLayer TEE planned)
    participant P as Configured PriceOracle or Pyth
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

This is the design's trust model. Owner controls and execution wiring have
unit and sandbox coverage. Live custody assumptions remain open in the
[roadmap](#roadmap-to-submission).

| Actor | Can | Cannot |
|---|---|---|
| **Owner** | ✅ set the mandate (owner authorization + 1 yocto; tightening at once, loosening after a 24 h delay, cancellable), ✅ pause unsigned work, ✅ unpause, ✅ withdraw (queued 24 h, cancellable; live post-delay send unverified) | exceed the gates, or take a trade past them |
| **Agent** (script today, TEE planned) | ✅ propose trades, including harmful trades that the mandate permits | directly sign, withdraw, exceed a gate, change the mandate |
| **Keeper** | trigger steps, write a public handoff, optionally publish with explicit `--publish`, delay | alter a signed trade, sign anything |
| **Solver** | fill a trade at ≥ `min_out` | take more than the signed `token_diff` |
| **Us (developers)** | publish source and builds; control deployment while vault full-access keys exist | bypass contract controls through vault-account access after all vault full-access and direct-sign keys are removed, assuming verified intended signer authority; those deployment assumptions remain UNVERIFIED |
| **Attacker with agent/keeper access** | propose harmful permitted trades; disrupt availability | change owner controls or substitute unsigned trade bytes |

## Proof so far

The interoperability question is whether a contract can obtain an MPC
signature whose exact payload NEAR Intents verifies. R1 tested this with a
separate testnet spike contract and mainnet simulation under an implicit
account. Actual vault registration, signing and funded settlement on a live
chain remain separate deployment milestones
([full report](docs/spikes/R1.md)).

The mainnet rows below are historical author-recorded simulation results.
They were not re-fetched by this review and do not establish funded settlement,
the current deployed code identity or current source/audit correspondence.

| Test | Network | Result |
|---|---|---|
| Intent signed over exact JSON bytes is accepted | historical mainnet simulation | Recorded accepted |
| Same intent modified after signing | historical mainnet simulation | Recorded rejected: `invalid signature` |
| Wrong `verifying_contract` | historical mainnet simulation | Recorded rejected |
| **Separate spike contract → testnet MPC → mainnet simulation** | testnet MPC + mainnet verifier | ✅ historical R1 evidence; not a funded vault trade |
| MPC-signed swap from an unfunded account | historical mainnet simulation | Recorded balance-only failure; signature verification passed |
| **Swap payload built by the vault's own code**, V1 nonce bound to the observed salt | historical mainnet simulation | Recorded signature, nonce and salt checks passed; balance-only failure |
| Same payload with a salt `intents.near` never issued | historical mainnet simulation | Recorded rejected: `invalid salt` (V1 nonce parsed, not treated as legacy) |
| Matched funded settlement, atomic refusal and nonce replay | local sandbox Intents fixture | Dedicated `matched_funded_settlement_is_atomic_and_replay_is_refused` test; stand-in signer/oracle, not live MPC |

The historical source reading reports that MPC and the referenced Intents
verifier use `env::ed25519_verify`
([source references](docs/spikes/R1.md#findings-from-source-commit-level-reading-2026-10-31)).
Correspondence with the currently deployed signer and verifier remains
UNVERIFIED.

## Try it

Requirements: [rustup](https://rustup.rs), Python 3 for reference checks and
Node.js 22 or later for dashboard logic tests. The pinned toolchain (Rust 1.95.0)
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
   That test deploys the supplied **Intents fixture**
   (`contracts/vault/tests/fixtures`) next to the vault. Current deployed-code,
   upstream-source and audit correspondence require separate verification;
   `scripts/fetch-intents-fixture.sh` can refresh a chain snapshot but does not
   establish source or audit identity. The vault asks the
   MPC signer for its derived key and registers it on `intents.near` with
   predecessor auth. The test then reads the key back from the fixture
   contract, and refuses a second registration. The MPC signer in that test
   is a stand-in that enforces the call convention; the real MPC network is
   not used.

   The first run downloads the sandbox (needs `cargo-near`). The same run
   includes `tests/attacks.rs`, the eight attacks below.
   It also includes `tests/settlement.rs`: a funded matched bundle changes both
   parties' ledger balances, an unbalanced bundle is refused atomically, and
   replay is refused. This establishes sandbox verifier behaviour, not the
   identity or behaviour of the currently deployed mainnet contracts.
4. **Run `./scripts/attack-demo.sh`.** It plays eight attacks against the
   contract on a local sandbox and writes what the vault answered to
   [docs/evidence/attacks.txt](docs/evidence/attacks.txt). Each line states the
   expected refusal and the observed one.
5. **Run `./scripts/gas-report.sh`.** It records sandbox gas. [docs/evidence/gas.txt](docs/evidence/gas.txt) is a historical 4 November run: a two-feed PriceOracle execution burned 29.82 TGas. Re-measure the final revision and chosen adapter before deployment.
6. **Run `./scripts/check-claims.sh`.** It checks that each test name and path
   cited in these docs still exists. It does not run the tests; run the named test
   to check a claim.
7. **Run `cd spikes/r1-intents-signing && cargo run --release`.** This reruns
   the six historical simulation cases against current mainnet
   `simulate_intents`, a free view call that needs no account. Verify its
   current output; the historical PASS record was not re-fetched in this
   review. The last two use payloads built by
   `mandate_vault::intent`, the contract's own code.

## Reference agent and explicit relay publication

The autonomous mode supports exactly two mandate assets. It reads holdings,
controls and the selected PriceOracle/Pyth adapter at one final block, chooses
an exact input toward a configured target outside its band, obtains a matching
quote, and previews every gate with the shared engine. A fresh final snapshot
repeats readiness, strategy and gate checks before proposing. On-chain execution
remains authoritative if state changes afterward. Set `expected_chain_id` to
the reviewed network in the public configuration.

```sh
cargo run -p mandate-agent -- --auto agent/examples/rebalance-public.json --dry-run
```

This preview performs public status/view/quote requests only. It does not load
signing or relay authentication settings, create an outbox, submit a proposal
or publish an intent. The template account and feeds must be replaced with a
reviewed deployment. The documented partner relay requires authentication, so
an unauthenticated quote request may fail. Executing `--auto <public-config.json>`
submits one proposal; `--loop` repeats after completed decisions with a 30-second
pause. Errors can terminate the process. Manual `<public-plan.json>` mode is retained.

The keeper defaults to execution and public file handoff. Only explicit
`--publish <public-relay-config.json>` enables the implemented Message Bus
publication path. It requires matching public proposal/quote metadata and
checks the current signed result, vault, verifier, exact amounts and deadlines.
Offers require more than the configured validity, whose minimum is 450 seconds.
An acknowledgement and an initial relay status are recorded separately; even
relay-reported `SETTLED` retains `settlement_verified: false`. Continuous status
monitoring and independent final receipt/ledger verification are not implemented.

See [agent usage](agent/examples/README.md), [keeper publication usage](keeper/examples/README.md)
and the [official Message Bus protocol](https://docs.near-intents.org/integration/market-makers/message-bus/rpc).
No live quote, publication, strategy or funded settlement result is claimed by
these implementations.

### Optional model decision source

`cargo run -p mandate-agent -- --model --dry-run` reads the existing final-block
snapshot and asks a configured OpenAI-compatible chat-completions endpoint for
one decision. Runtime settings are `NEAR_RPC_URL`, `VAULT_ID`, optional
`NEAR_CHAIN_ID`, and the required provider settings `MODEL_ENDPOINT_URL`,
`MODEL_NAME`, `MODEL_API_KEY`. The endpoint is the complete HTTPS completion
URL. No provider, endpoint or model is selected by a fallback. Tests inject a
fake client and never call a model endpoint.

The response must contain the exact seven-field decision schema. Unknown or
missing fields, malformed JSON, unsupported assets, invalid positive u128
amounts, rationale outside 1..800 characters, HTTP errors and timeouts produce
no trade. Closed vault controls prevent the model call. The agent previews the
unchanged vault engine, then refreshes public state before paid submission;
it never repairs model amounts or changes the mandate from rationale text.
`no_trade` makes no proposal. There is no automatic model retry loop.

Without `--dry-run`, existing agent signing and outbox settings are needed to
submit. The model never receives those settings or signs a transaction. At this
the model path commits to the **exact response bytes**: its on-chain reason hash
is the SHA-256 of the response body as received, and the request hash is the
SHA-256 of the request body as sent. Each proposal writes `model-<response>.json`
to the outbox. The verifier checks it offline:

```bash
cargo run -p mandate-agent -- --verify-model model-<hash>.json --reason-hash <on-chain hash>
```

With a signature document from NEAR AI Cloud and a pinned signing address, add
`--signature <file> --signing-address 0x…`. The verifier then checks the ECDSA
signature over the EIP-191 message, as the provider documents it. Fetching the
signature and checking the provider's attestation are not implemented, so a
record written by the agent stays **UNVERIFIED** until someone runs the verifier.
Neither this client nor a valid model response proves profitability, privacy or
TEE execution.

## Mandate rules

Enforced per trade by `contracts/vault/src/engine.rs`, a pure function
that is unit- and property-tested. Every USD value is exact 256-bit
integer math, rounded against the trade: exposure up, receipts down.
The vault calls the engine on-chain: it reads live balances and oracle
prices, runs the gates, and only then calls `sign`. That path is tested on
a sandbox with the supplied Intents fixture and a stand-in MPC signer. It
has not run on mainnet.

| Rule | Purpose | Engine | On-chain wiring |
|---|---|---|---|
| NEP141 asset allowlist (max 20, each with decimals and price feed), stable subset | the agent trades only supported owner-chosen assets | ✅ | ✅ validated on `set_mandate` |
| Max notional per trade | bounds the damage of any single decision | ✅ | ✅ sandbox (attack 3) |
| Signing-request turnover, approval-day accounting resets 00:00 UTC | bounds reserved oracle notional per approval day, not actual settlement-day turnover | ✅ | ✅ wired; engine and callback regressions |
| Min-out ≥ oracle value − slippage (hard cap 5%) | blocks bad or manipulated quotes | ✅ | ✅ sandbox (attack 4) |
| Every traded **or held** asset needs a coherent fresh price at approval and signing request (≤ max age, ≤ 5 s clock skew) | unsigned approval cannot age into a stale-price signature request | ✅ | ✅ freshness and exact signing-age boundary tests; settlement-time freshness is not guaranteed |
| Cooldown between trades | stops drain-by-many-small-trades | ✅ | ✅ wired; engine tests |
| Drawdown breaker vs high-water mark | after a loss, only trades into stable assets | ✅ | ✅ wired; engine tests |
| Position cap: conservative destination exposure; other over-cap holdings may not worsen | prevents increasing excessive concentration while allowing de-risking | ✅ | ✅ wired; engine tests |
| One signed intent at a time (120 s TTL) | no stacked exposure from unpublished signatures | ✅ | ✅ sandbox (`a_signed_intent_blocks_the_next_proposal`) |
| Exact intent bytes: `raw_ed25519` payload, V1 expirable nonce | the signature commits to amounts, deadline and nonce | Historical author-recorded mainnet simulation | ✅ sandbox (attack 7: supplied verifier fixture rejects an altered amount) |
| Kill switch: `pause` accepts owner function-call authorization; `unpause` requires owner predecessor and exactly 1 yoctoNEAR | stop fast, resume deliberately | — | ✅ unit + sandbox tested |

Turnover, cooldown and high-water state are reserved from the gate snapshot
before requesting MPC signing. Failed signing, lost answers and unfilled
intents do not automatically refund that reservation. Settlement remains
separate: an intent approved before midnight can settle on the next UTC day
within its deadline.

## Built on NEAR

| Component | Role in Mandate | Status |
|---|---|---|
| **NEAR Intents** (`intents.near`) | holds the treasury; atomic `token_diff` settlement | Historical author-recorded mainnet simulation; sandbox registration, execution and settlement with supplied fixture; current deployed/source/audit correspondence UNVERIFIED; live funded trade open |
| **Chain Signatures** (`v1.signer`, Ed25519) | gives the contract its own signing key | Historical separate spike → testnet MPC → mainnet simulation; sandbox vault execution with stand-in signer; live vault MPC signing UNVERIFIED |
| **OutLayer** (Intel TDX) | runs the agent with a pinned code hash; secrets sealed to that hash | 🛠 |
| **NEAR AI Cloud** (Private TEE models only) | optional private inference with verifiable signatures | 🛠 |
| **price-oracle.near** | fresh prices for slippage, position and drawdown checks | ✅ read on-chain with fail-closed sandbox tests · ⚠ on mainnet it returned no fresh price for wNEAR or USDC (2026-10-31) |
| **Pyth NEAR receiver** | immutable alternative adapter selected at fresh initialization | Adapter and confidence guards implemented; usable live feeds/updater still UNVERIFIED. Both queried NEAR caches were null on 6 November. |
| **Solver Message Bus client** | exact-input quotes and explicit keeper publication | Implemented reference protocol; matching metadata required; acknowledgement/status do not prove settlement; live operation UNVERIFIED |
| **NEP-330** | source metadata supports independent build/hash comparison | ✅ metadata; deployed reproducible-hash match still needs verification |

## Roadmap to submission

- [x] Architecture, trust model, risk register ([docs/ARCHITECTURE.md](docs/ARCHITECTURE.md))
- [x] Vault contract: mandate config, owner controls, unit and sandbox tests, CI
- [x] **R1 interoperability**: historical author-recorded spike-contract MPC signature accepted by mainnet simulation, not re-fetched in this review
- [x] Per-trade mandate engine (exact math, property-tested) and intent builder (V1 expirable nonces); historical author-recorded mainnet simulation
- [x] Vault registers its MPC-derived key on the supplied Intents fixture (sandbox, MPC stand-in)
- [x] Vault signs through the gates and the MPC call, with live balances and oracle prices (sandbox, stand-in signer)
- [x] Oracle integration with a staleness bound (sandbox)
- [x] Immutable Pyth adapter choice, strict feed/value/confidence guards and provider regression tests
- [x] Connected read-only dashboard with final-block snapshots and separately labelled sample mode
- [x] Owner controls: 24 h delay on loosening, cancel, simulate (sandbox, and testnet)
- [x] Reference agent and keeper binaries, with manual mode and existing sandbox coverage
- [x] Deterministic two-asset autonomous preparation and explicit relay-publication clients implemented; final combined verification and live operation remain open
- [x] Funded matched settlement, atomic refusal and replay test against the sandbox Intents fixture
- [x] Threat model and self-review ([THREAT_MODEL.md](THREAT_MODEL.md), [SECURITY.md](SECURITY.md))
- [ ] Run the signing path on a live chain (mainnet, or testnet once `intents.testnet` exists)
- [ ] OutLayer agent (Rust → `wasm32-wasip2`) with yield/resume decision flow
- [ ] Demonstrate the implemented explicit solver-relay publishing path on the intended live deployment
- [x] Withdraw: owner queue, 24 h boundary unit tests and full read/send/confirm sandbox lifecycle. The sandbox can inject only request maturity when clock advancement is insufficient; that mode does not demonstrate passage of 24 hours or live withdrawal.
- [ ] Dashboard with live attack demos (each must fail on camera). A recorded replay of the eight attacks is on the dashboard; a live run against a chain is not built.
- [ ] First real mainnet trade with a few dollars
- [ ] Demo video, submission

## Deployment and recovery boundaries

The ordered live release gates, public read-only inspection tool and oracle
availability observations are in [docs/OPERATIONAL-READINESS.md](docs/OPERATIONAL-READINESS.md).

Use a distinct recoverable owner and a vault account name of at most 62 bytes.
Removing full-access keys from a self-owned
vault would remove its external recovery authority. Supported custody is
NEP141 assets in the vault's Intents ledger; withdrawal receivers cannot be the
vault or Intents contract.

For an existing deployment, a new WASM alone cannot read a changed Borsh layout.
The private `migrate` entry point supports exactly four historical layouts:
the pre-sequence vault at `77ebc0c`, the sequence-enabled vault at `0de5c48`,
the pre-provider remediation layout, and the provider-enabled pre-voucher
layout. It preserves the selected provider for the fourth layout and
requires a paused, quiescent vault with a distinct owner. It must be called by
the vault account in an authorized deployment transaction. The legacy layout
does not prove that no withdrawal was already sent: require final-receipt
evidence, and use a fresh deployment when that history is unknown. Migration
does not replace the owner or provide an upgrade key for an already keyless vault.

`get_chain_time_sec`, `get_last_proposal_outcome`, `get_withdrawal_status` and
`get_last_withdrawal_outcome` expose bounded status. The withdrawal protocol is
`unique_voucher_v1`; clients can also read `get_withdrawal_stage`,
`is_withdrawal_retry_armed` and `get_withdrawal_recovery_accounts`.

After the owner's 24-hour queue matures, `apply_pending_withdrawal` requires at
least **280 TGas**. It spends one owner-armed attempt and persists a never-reused
ID in `Preparing` before issuing a one-unit Intents voucher and reading
the victim balance. Before dispatch, authenticated reads require global
`mt_supply` exactly one, source balance one and both sink balances zero.
This rejects units preseeded into any ledger by a former issuer authority.
Its `Dispatched` batch moves the voucher to `s.<vault>`
first and calls `ft_withdraw` last. Full-amount terminal response records
`Delivered`; explicit zero/refund can requeue only the still-authorized request.
Every safe requeue is unarmed: only the owner, with exactly 1 yoctoNEAR, may
call `rearm_pending_withdrawal`. Rearming preserves the original maturity; the
next apply allocates a new ID. Failed attempts do not create an unbounded
permissionless retry loop.

After **300 seconds**, recovery can invalidate `Preparing` before a batch
exists. For `Dispatched`, `recover_withdrawal` requires at least **140 TGas**
and races that voucher to `c.<vault>`. Recovery requires global supply one
and source balance zero before positive destination proof decides:
sent=0/cancelled=1 safely requeues; sent=1/cancelled=0 records `DebitObserved`
and consumes the authorization. Missing, failed or inconsistent proof retains
the guard. Recovery never blindly repeats an unknown amount.

`DebitObserved` proves the original initial debit, not terminal FT delivery or
absence of a later refund. It releases the guard, so subsequent operations can
overlap the original transfer's unresolved resolver/refund receipts. Setup must
prove that both named sinks are uncreated and have no implicit, registered or
proxy signing authority, then seal every bypass-capable vault permission.
Every earlier voucher issuance/burn/resolver/refund receipt must also have completed: a delayed
pre-seal mint after the preparation proof would violate uniqueness. Prefer a
fresh atomic account creation/deployment/initialization without adding access
permissions. Present permission counts alone do not prove receipt history.
`mt_supply` measures current supply, not lifetime issuance: an earlier burned
duplicate can return through a delayed refund. Historical quiescence includes
external-holder withdrawals and resolvers. Use a fresh atomic account with no
added access authority for real funds unless that complete history is authenticated.
Checking an empty signing list today alone does not prevent future account
creation. The vault never creates those sinks. External Intents behavior and
administrative permissions remain trusted; unique voucher journal entries also
consume external storage and need measured funding.

Every signing error after requesting MPC retains the timed execution lock,
even a failed Promise result or oversized response. A signature may have been
produced before that error. Read/salt and pre-sign validation failures can
release the lock safely; live MPC failure and late-response behaviour still
requires transaction evidence.

Approved oracle data must remain within its configured price age when the salt
callback requests signing. The execution records the oldest approved price's
allowed deadline; equality is allowed and a later callback is refused. A salt
callback cannot invent an unrecorded approval. This checks signing-request time,
not prices when an already issued intent eventually settles.

The 6 November **170 TGas** withdrawal validation belongs to the pre-voucher
implementation and is historical. The current lifecycle target
`vault_withdrawal_reads_sends_and_confirms_the_actual_transfer` must be rerun at
280 TGas with the final protocol revision. It logs its clock mode; a sandbox
patch of only request maturity is not evidence that 24 hours elapsed. Final
combined tests and intended-chain withdrawal/recovery evidence remain required.

## Not claimed

- That the strategy is profitable. Mandate bounds risk; it does not create alpha.
- That TEE attestation or private inference is delivered today. The optional
  model client has no verified hardware attestation; OutLayer delivery is planned.
- That a proposed inference provider/model is private or available without
  separate verification. The intended private-model policy remains planned.

## Repository layout

```
contracts/vault/        vault contract (Rust, near-sdk 5.30)
  src/mandate.rs        mandate config and validation
  src/engine.rs         per-trade enforcement (pure; unit + property tests)
  src/math.rs           exact USD valuation (256-bit, conservative rounding)
  src/intent.rs         raw_ed25519 token_diff payload + V1 nonce builder
  tests/sandbox.rs      owner controls on a local NEAR sandbox
  tests/attacks.rs      eight attacks, each refused on the sandbox
agent/                  manual and deterministic two-asset reference agent; quote client
keeper/                 permissionless execution/outbox; explicit relay publication
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

- Development started on 2026-10-31. Eligibility for the organizer's official
  build window must be confirmed; this date alone does not establish it.
- Libraries: `near-sdk`, `uint` (256-bit integers); dev-only `near-sandbox`,
  `near-api`, `cargo-near-build`, `tokio`, `testresult`, `proptest`, and
  `chrono` (used only as a test oracle). The spikes also use
  `ed25519-dalek`, `ureq`, `serde_json`, `bs58`, `base64`, `chrono`, `rand`
  and `hex`.
- Development is AI-assisted (including Claude Code and Codex), using the official NEAR agent
  skills (`npx skills add near/agent-skills`), vendored unmodified in
  `.claude/skills/` and pinned in `skills-lock.json` (near/agent-skills@cb32b6a, MIT).

## License

[MIT](LICENSE)
