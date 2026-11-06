# Mandate — Architecture & Risk Register

> Status: implemented prototype and remaining integration risks, 2026-11-06.
> Sandbox tests use an Intents WASM fixture with stand-in signer and oracle.
> Historical R1 evidence uses mainnet simulation. Live funded vault signing,
> settlement, withdrawal and failure recovery remain unverified.

## 1. One sentence

A policy vault that checks proposed trades on-chain before requesting their
signatures. The Rust agent supports manual plans and deterministic two-asset
rebalancing from holdings/oracle/quotes; model inference and OutLayer TEE
operation are planned.
The owner can separately withdraw supported NEP141 assets after a delay.
Permitted trades can lose money. Custody also trusts the deployed NEAR Intents
and MPC contracts and their administrative permissions.

## 2. Core security invariant

> For a correctly configured, sealed vault and trusted external contracts,
> the vault requests a trade signature only after checking the exact intent
> against the owner's mandate.

How we get there:

- Funds live in `intents.near` under the vault's own account `vault.near`.
- `execute_intents` (swaps / `token_diff`) **always** requires a signature from
  a public key registered to the signer account **[V]** (defuse
  `engine/mod.rs::execute_signed_intent`).
- The **only** key we register for `vault.near` on `intents.near` is an
  **MPC-derived ed25519 key** that belongs to the vault contract itself via
  Chain Signatures (`v1.signer`, domain 1 = Ed25519)
  **[sandbox-tested; historical interoperability simulation in [spikes/R1.md](spikes/R1.md)]**.
- MPC signs only for its predecessor account, so only the vault contract
  can obtain signatures for that key, and it does so only after its own
  checks pass.
- Registering the key needs no signature: a contract can call
  `intents.near.add_public_key` itself, with 1 yocto attached, because
  `auth_by_predecessor_id` is on by default **[V]**.

The agent, keeper and frontend do not receive the vault's treasury signing
authority. A compromised agent can still choose economically harmful permitted
trades and disrupt availability. Owner-account compromise can authorize delayed
withdrawals. The guarantee depends on valid configuration, coherent oracle
inputs, asynchronous callback checks and a correctly deployed contract.

## 3. Components

| Component | Tech | Trust |
|---|---|---|
| `vault` contract | Rust, near-sdk 5.30.0 **[V]** | Root of safety |
| Mandate engine | pure Rust module inside `vault`, no I/O | Boundary and property tests; not exhaustive proof |
| Strategy agent | Rust manual-plan or deterministic two-asset rebalance client; TEE/inference planned | Final-block preview and quotes; authoritative on-chain gates; permitted trades can lose money |
| Inference (optional) | Private TEE inference planned | Advisory; no live integration established |
| Price oracle | immutable PriceOracle aggregate/report or Pyth batch adapter | Trusted for correctness; coherence/freshness and provider-specific input guards checked |
| Settlement | NEAR Intents verifier `intents.near` + solver relay **[V]** | Atomic, `token_diff` enforces min-out |
| Keeper | Rust, permissionless; explicit `--publish` option | Executes/outbox by default; quote-bound publication only on opt-in; acknowledgements/status are not final receipts |
| Dashboard | HTML/JavaScript public RPC reader with separate sample mode | One final-block snapshot; RPC trusted; no attestation or settlement authentication |

Pyth is implemented through `list_prices_no_older_than`, with strict feed format,
positive signed values and confidence bounds. The provider is chosen at init and
cannot be changed afterward. Current updater availability, asset units and
deployed source/admin correspondence remain UNVERIFIED; see
[OPERATIONAL-READINESS.md](OPERATIONAL-READINESS.md).

## 4. Trade lifecycle (two transactions, by design)

Proposal and execution are separate transactions by design. Planned TEE
decision work stays outside the balance/oracle/signing execution budget. The
necessity of this split for the current plain-script path has not been measured.

```
TX-1  agent → vault.propose(asset_in, asset_out, amount_in, min_amount_out, reason_hash)
        vault stores one proposal with a 60-second expiry

TX-2  anyone → vault.execute()
        record execution/proposal identity and configuration revision
        read vault balances using an asset-ID snapshot
        read coherent oracle price/report data
        check_trade(mandate, state, trade, market) → approval or named rejection
        read current Intents salt; build raw_ed25519 payload and V1 nonce
        reserve turnover, cooldown and high-water state from approval snapshot
        vault → signer.sign(payload_v2.Eddsa = exact payload bytes, domain_id = 1)
        on_signed stores a successful signature; failed/lost signing has no automatic quota refund

OFF   keeper reports signed intent to its public outbox by default
        explicit --publish validates current signed result and matching quote metadata
        Message Bus acknowledgement/status remain unverified settlement observations
        funded matched execute_intents ledger settlement is covered in sandbox
        actual live vault signing/settlement remains unverified
```

The keeper can withhold or delay, but cannot alter: the signature covers
amounts, deadline and nonce. `token_diff` guarantees we receive at least
`min_amount_out` or nothing executes.

The signature is an authorization artifact, not a settlement receipt. Client
views expose chain time, last proposal outcome and withdrawal status/outcome.
Execution callbacks retain proposal identity, configuration revision and time
authority; pause or changed configuration stops unsigned work.
All post-request signing errors retain the timed execution lock, including a
Failed Promise, malformed or oversized answer. A signature may already have
been produced. Read/salt and pre-sign validation failures can release the lock.
Before requesting signing, the salt callback requires recorded approval and
checks the oldest approved-price deadline. Exact price-age equality passes;
later signing is refused. Settlement itself does not re-price the intent.

Withdrawal uses the separate 24-hour owner queue and `unique_voucher_v1`.
Apply requires **280 TGas**, spends one owner-armed attempt, and records a
never-reused ID in `Preparing` before issuing a unique unit and reading the
victim balance in one Intents receipt. Dispatch proceeds only after authenticated
reads show global `mt_supply` one,
source balance one and both sink balances zero. This rejects preseeded duplicates
held anywhere. `Dispatched` moves the unit to
`s.<vault>` first and calls `ft_withdraw` last in one receipt. The last action
preserves the terminal amount response. Full amount records `Delivered`;
zero/refund can requeue only the matching generation, with its arm consumed.
Only owner predecessor authorization and exactly 1 yoctoNEAR may rearm one
safe attempt. Maturity is preserved; the next apply allocates a fresh ID.

After **300 s**, preparation can be invalidated before a batch exists.
Dispatched recovery requires **140 TGas** and races that unit to `c.<vault>`.
Recovery requires global supply one and source zero before accepting destinations.
Sent=0/cancelled=1 proves cancellation before debit and requeues safely;
sent=1/cancelled=0 consumes the request as `DebitObserved`. Failed/missing,
both-zero or inconsistent proof retains the guard; it never blindly resends
an unknown amount. Sent proof establishes initial debit, not terminal FT
delivery or absence of a later refund. It releases the guard, so those resolver/
refund receipts may overlap subsequent operations.

Setup must prove `s.<vault>` and `c.<vault>` are uncreated, without implicit,
registered or proxy signing authority, and cannot later be created through a
vault bypass permission. The vault never creates them and must be sealed after
setup; an empty current signing list alone is insufficient. Vault names are
limited to **62 bytes**. Intents state/admin behavior is trusted. Its voucher
journal grows with owner-authorized attempts and needs measured storage funding.
Every pre-seal voucher issuance/burn/resolver/refund receipt must have completed before funds
or operations. Fresh atomic account creation/deployment/initialization without
adding access permissions avoids a former account issuer authority. Present
permission counts cannot prove that prior receipt history is quiescent.
For release, require a fresh account never given issuer authority, or independent
authentication that no canonical future withdrawal ID was previously issued.
Completed known receipts alone cannot exclude a duplicate holder's later
burn/refund. Current supply is not lifetime issuance.

Existing state requires the private `migrate` entry point for its known Borsh
layout, or a fresh deployment. Exactly four historical layouts are recognized:
`77ebc0c` pre-sequence, `0de5c48` sequence-enabled, pre-provider remediation and
provider-enabled pre-voucher. The last preserves its selected provider.
Migration requires a distinct owner, paused and
quiescent state, and final-receipt evidence for legacy dispatched withdrawals.
It must be called by the vault account in an authorized deployment transaction;
it neither replaces the owner nor restores an upgrade key to a keyless vault.
An active withdrawal guard is refused, never discarded; historical unknown
sends cannot be retrofitted into voucher proof. The 6 November 170 TGas
withdrawal validation applies to the older pre-voucher path. Final combined
280/140 TGas protocol verification and intended-chain evidence remain required.

The autonomous reference mode supports exactly two mandate assets, uses final
hash/height-pinned balances and the chosen oracle adapter, and selects an
exact-input quote toward a configured target outside its band. It previews
the shared engine, then repeats readiness/strategy/gates at a fresh final block
before proposing. Quotes must exceed the configured validity (at least
450 seconds); on-chain races still face authoritative vault checks. Dry-run
reads public state/quotes without loading signing/authentication settings or
writing an outbox. There is no LLM/TEE in this strategy. Publication requires
matching public proposal/quote metadata and explicit keeper opt-in; an initial
relay status, including `SETTLED`, keeps `settlement_verified: false` and is
not continuously monitored. Live strategy, relay and funded settlement remain
unverified. See [agent usage](../agent/examples/README.md) and
[keeper usage](../keeper/examples/README.md).

## 5. Mandate (on-chain, owner-set)

| Rule | Check location |
|---|---|
| Asset allowlist (in and out) | `execute` |
| Max notional per trade (USD) | oracle price × amount |
| Max position weight after trade | balances + prices |
| Min-out ≥ oracle value × (1 − max_slippage_bps) | blocks bad quotes and oracle-vs-quote divergence |
| Daily turnover cap, min cooldown between trades | vault state |
| Drawdown circuit breaker: NAV < HWM × (1 − max_dd) ⇒ only de-risk trades (into stable) allowed | vault state |
| **Single outstanding signed intent**: no new signature until the previous deadline has passed | prevents stacked exposure from unpublished signatures |
| Oracle staleness bound; fail closed | `execute` |
| Owner kill switch; owner-only withdraw via `intents.near.ft_withdraw` (predecessor auth) **[V]** | owner methods |

**Implemented (2026-10-31)** as the pure function `engine::check_trade`. It
checks in a fixed order, and the first violation wins:

1. structure
2. signed-intent lock + cooldown
3. fresh prices for every traded or held asset
4. balance
5. size
6. turnover (UTC day)
7. slippage
8. drawdown
9. position

Precise choices:

- USD math is exact 256-bit integer arithmetic (`math::usd_micros`).
  Notional and positions round up; receipts and NAV round down, so every
  boundary errs toward rejection.
- The non-stable destination position uses conservative valuation. Received value is
  at max(value of `min_out`, notional), because a solver may over-deliver.
  NAV is valued with only `min_out` received.
- Other non-stable holdings cannot increase their share while above the cap;
  an already excessive holding may remain when its weight does not worsen.
  This preserves de-risking rather than promising every position is always below cap.
- Prices are accepted up to 5 s ahead of block time (clock skew). A zero
  price or an exponent > 0 is a valuation error, never a panic.
- NAV counts only mandate assets. Tokens airdropped into the account cannot
  inflate it.
- Covered by unit tests at every boundary and by a property test. The
  property test re-derives each rule for random trades and is guarded by a
  coverage test (≈27% approvals; every rejection type produced).

Intent bytes come from `intent::token_diff_payload`, with a V1 expirable
nonce from `intent::v1_nonce`. Legacy random nonces are marked for removal
upstream. Historical R1 evidence recorded mainnet simulation acceptance of a
vault-built payload; current deployed identity and funded live behavior are
separate verification requirements
([spikes/R1.md](spikes/R1.md#follow-up-payloads-built-by-the-vaults-own-code-passed)).

v1 is **single owner**: an individual or a Sputnik DAO account. Pooled
depositor shares are deliberately out of scope. They add an inflation attack
and NAV-manipulation surface, and we do not ship that half-done.

## 6. Planned private-agent integration

The following are intended integration mechanisms, not current product claims.
There is no supplied OutLayer agent run or attestation from this vault flow:

- The agent's code is pinned by sha256 (`version_key` / `WasmUrl.hash`). The
  worker refuses mismatched bytes **[V]**.
- The strategy's **parameters and signals** live in OutLayer `PROTECTED_`
  secrets bound to that wasm hash, invisible to us **[V]**.
- Every run has a TDX attestation (wasm hash, input/output hash) that can be
  verified off-chain **[V]**.
- Results reach the chain via the registered TEE worker key **[V]**.
- If NEAR AI is used, the intended policy allows only **Private TEE** models.
  Actual model availability and privacy must be verified before integration. Response
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
| R1 | Actual vault MPC signing/registration and funded settlement | **Critical**: custody integration | Signature interoperability simulated in R1; actual live vault flow remains required | open |
| R2 | Payload hash/serialization mismatch (NEP-413 vs `raw_ed25519`) | High | **Retired: `raw_ed25519` signs exact payload bytes; tamper test rejects** ([spikes/R1.md](spikes/R1.md)) | 1 ✅ |
| R3 | Live relay authentication or sufficiently long exact-input offers unavailable | High | Explicit Message Bus quote/publication client implemented; demonstrate live provider/auth access and offers exceeding the configured ≥450 s validity. No automatic alternate provider is implemented | open |
| R4 | OutLayer: HTTPS from WASM to relay/NEAR AI, cost, latency, 900-byte output | Medium | Hello-world project on testnet Day 2; output is a compact struct | 2 |
| R5 | `price-oracle.near` missing assets / stale | Medium | Restrict v1 universe to assets it serves; fail closed | 1 |
| R6 | Gas across callbacks | Medium | Already split into TX-1 / TX-2; measure in sandbox. MPC `sign`: on-chain minimum is 15 TGas and 30 TGas worked in R1. The NEAR agent skill suggests 250 TGas, which would not fit TX-2, so the attachment is set from measurement | 3 |
| R7 | Intended live Intents deployment and external trust not established | Medium | Sandbox runs the supplied Intents WASM fixture with stand-in MPC/oracle; current binary/source/audit/admin correspondence and funded live flow need separate evidence | open |
| R8 | Our own contract bugs | High | Engine regressions; private callbacks; execution revision/expiry guards; identified withdrawal reconciliation; fresh verification and independent review | ongoing |

**Go / no-go: end of Day 2.** If R1 fails, switch to the fallback and change
the README claims accordingly. We do not hide it.

## 8. Reproducibility (for the AI screeners)

- `./scripts/verify.sh` runs formatting, Clippy, allowed-document reference
  checks and workspace unit/property tests. Counts come from that run.
- Add `--integration` for the listed sandbox targets, including owner controls,
  registration, execution, keeper/agent, attacks, withdrawal and supported-size
  gas regressions. New client/voucher paths still require final combined verification.
- A dedicated `matched_funded_settlement_is_atomic_and_replay_is_refused`
  sandbox test checks funded ledger settlement, atomic bundle refusal and
  replay against the fixture. The signer and oracle are stand-ins. Actual
  live vault signing/settlement remains unverified.
- The actual vault withdrawal read/send/confirm lifecycle has a dedicated
  sandbox target. It logs whether clock advancement reached the real deadline
  or only maturity was patched in sandbox STATE. The latter mode verifies
  transfer/callback behaviour, not observed passage of the 24-hour delay.
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
contracts/mock-ft     test-only NEP141 token
contracts/mock-oracle test-only oracle
contracts/mock-externals test-only signer and external calls
agent/                 Rust manual/two-asset strategy and quote reference client
keeper/                Rust execution/outbox and explicit publication client
web/                   connected read-only dashboard, separate sample and evidence page
scripts/verify.sh      one-command check
docs/                  architecture, project definition, dated evidence
THREAT_MODEL.md        threat model at repository root
```

## 12. Tooling versions (checked 2026-10-31) [V]

near-sdk 5.30.0 (Rust ≥1.95), cargo-near 0.22.0, near-sandbox 0.4.0 +
near-api 0.8.7 for integration tests (per the NEAR agent skill
`near-smart-contracts/rules/testing-integration-tests.md`),
Client web SDKs, solver libraries and private inference integrations are not
implemented dependencies of the static dashboard. Their compatibility must be
checked if those integrations are added.
