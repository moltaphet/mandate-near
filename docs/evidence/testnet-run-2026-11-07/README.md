# Testnet run, 7 November 2026

A controlled run of the Mandate vault on NEAR testnet, driven by
[`contracts/vault/examples/testnet_run.rs`](../../../contracts/vault/examples/testnet_run.rs).
The decisions behind it are in [testnet-decisions.md](../../design/testnet-decisions.md).
Every state-changing step printed a hash; the raw output of each step is saved next to this file.
The printed values are not the real transaction hashes (a near-api 0.8.7 bug). This README uses the real
hashes; [TX-HASH-CORRECTIONS.md](../TX-HASH-CORRECTIONS.md) maps each printed value to its transaction.

## What is real and what is a stand-in

| Part | Status |
|---|---|
| Vault (`vault.mandate-t1.testnet`) | Real: built from this source, SHA256 `5266d033e30c6311d10f92d494b1cb456471daace03124342d24ededbff70f6f` (391,671 bytes) |
| MPC signer `v1.signer-prod.testnet` | Real testnet Chain Signatures, domain 1 (ed25519) |
| Owner | Real Sputnik DAO v2 (`mandate-t1.sputnikv2.testnet`), one council member |
| Intents verifier | **Fixture**: the supplied mainnet WASM (SHA256 `a1afab98…c9922b547`) on `intents.mandate-t1.testnet`. Testnet has no Intents contract. |
| Oracle | **Stand-in**: `mock-oracle` on `oracle.mandate-t1.testnet`, fed by the operator with fixed prices |
| Token | **Stand-in**: `mock-ft` on `usdc.mandate-t1.testnet` |
| Agent | `agent.mandate-t1.testnet`. On testnet it shares the operator's key; mainnet must not. |

The vault has no access keys, `s.<vault>` and `c.<vault>` do not exist, and its owner
is the DAO. The vault, oracle and token WASM hashes are in `00-wasm-sha256.txt`.

## Steps and transaction hashes

| Step | Result | Hash |
|---|---|---|
| Create `mandate-t1.testnet` | ok | `AH1ceSpBY13wDoBW15GEBy9iaV9vemWRvhpTg3h7vHRS` |
| Create oracle, usdc, intents, agent accounts | ok | `6x62Tgj2…`, `94Y9c9WY…`, `BimYP5kM…`, `GmvSNxPQ…` |
| Deploy oracle, usdc, intents | ok | `2dNdzhda…`, `AfJ7APfp…`, `CmnreRcZ…` |
| Create the DAO | ok | `8U3Kows3zeW1DU4Tbp8qYsBwqDR14jstfGSC8omZn7BX` |
| Create, fund, deploy and initialise the vault in one transaction | ok, no key added | `4khnfb5gjGxxghx5bkHjnbwLHvatHf3FemKj3oqUQEv6` |
| `set_agent` through DAO proposal 0 | ok | `6mdRKswm…` (add), `BKuy63Ux…` (approve) |
| `register_intents_key` | ok, key registered | `Co2GvcbFrpNUSJFdLr7cgFmZXvFk24BjgvP6MQAFx2mE` |
| Mint and deposit 10 USDC to the vault's Intents ledger | ledger shows 10,000,000 | `GLoxeoWJ…`, `C36pp2dn…` |
| Oracle reports (wNEAR $4.50, USDC $1.00) | ok | `EfQ6X7De…`, `93PJoxXi…` |
| `propose` by the agent | proposal 1 | `GeUV6vzdVtdeKZmD35PMxcBkqXfFUxV88S5ymcDosaEA` |
| `execute` | outcome `Executed`, signed intent retained | not captured, see below |
| `request_withdrawal` through DAO proposal 2 | queued, `effective_sec` 1794159940 | `DAhi8GQL…` (add), `GwV529dH…` (approve) |
| `apply_pending_withdrawal` before maturity | refused: "the withdrawal is still in its delay" | `09-withdraw-apply-early.log` |
| `apply_pending_withdrawal` after maturity (8 Nov 2026, 18:01 UTC) | ok, receipt failures 0 | `81JCWdkt3cMfhQgNLp6yJ36Np9HfF14g5NpRR7g3w26s` (logs `11`–`14`) |

## Independent check of the signature

The retained signed intent was read from `get_signed_intent` and checked outside the vault:

- its public key `ed25519:ADbT3FKzBG8QF2YFG7UsBF2A3ZnPxezzCjoDgqrwJK2a` equals the key
  `v1.signer-prod.testnet` derives for this vault (`derived_public_key`, path `intents`,
  domain 1), and equals the key the vault registered;
- its signature verifies over the exact payload bytes.

So the vault obtained a real testnet MPC signature, and only after the on-chain gates passed.

## Withdrawal

The withdrawal of 1 USDC (1,000,000 base units) to `mandate-t1.testnet` was queued and
matured at `effective_sec` 1794159940 (2026-11-08 17:45:40 UTC). Before maturity an
early apply was refused. The apply ran about 15 minutes after maturity:

- chain time at apply: 1794160858; elapsed since the queue: 87,318 seconds (requirement 86,400);
- the receiver's USDC balance went from 0 to 1,000,000;
- the vault's Intents ledger went from 10,000,000 to 9,000,000;
- the last withdrawal outcome is `Delivered { withdrawal_id: "1", amount: "1000000" }`.

This delayed, real-time withdrawal is shown on testnet. Logs are `11-withdraw-before.log`
to `14-status-after-withdrawal.log`.

## Problems met and how they were handled

1. The first `add_proposal` attached 0.1 NEAR; the DAO policy requires a 1 NEAR bond,
   so the call failed. The driver was fixed and the step repeated. The failed attempt
   moved nothing.
2. The first `request_withdrawal` (DAO proposal 1) failed in the vault. It was issued
   seconds after `execute`, while the signed intent was still open, and the vault refuses
   a withdrawal request in that window. The receipt failure reason could not be fetched
   (the RPC timed out), so this cause is inferred; the same request succeeded as proposal 2
   after the intent expired.
3. The client reported "error decoding response body" for `execute`. The chain state
   shows the call completed (`Executed` outcome and a retained signature), but the
   transaction hash was not captured.

## What this does and does not show

It shows, on a real chain: atomic keyless deployment, DAO-governed owner calls, key
registration against the real testnet signer, gates passing, and a verifiable MPC
signature. It does **not** show a matched settlement (no counterparty and no real Intents
contract), a production oracle, recovery after a
lost callback (not exercised), or a live model provider run.
