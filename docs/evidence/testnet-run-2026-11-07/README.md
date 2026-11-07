# Testnet run, 7 November 2026

A controlled run of the Mandate vault on NEAR testnet, driven by
[`contracts/vault/examples/testnet_run.rs`](../../../contracts/vault/examples/testnet_run.rs).
The decisions behind it are in [testnet-decisions.md](../../design/testnet-decisions.md).
Every state-changing step printed its transaction hash; the raw output of each step
is saved next to this file.

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
| Create `mandate-t1.testnet` | ok | `4c8z2FqUjCCXSJpNn3cVZJph8Wi5a9R5MKvZ8JdG5kWZ` |
| Create oracle, usdc, intents, agent accounts | ok | `2kRYmidk…`, `CSvkLMTf…`, `DCCLvtTN…`, `3arx6Suy…` |
| Deploy oracle, usdc, intents | ok | `FG6NioUM…`, `Hsf5JwUY…`, `8eFNCeSy…` |
| Create the DAO | ok | `7WveiGGEQ3uGheHTEFyQqNf8ZsLtuuCEZYJc8s72aqNW` |
| Create, fund, deploy and initialise the vault in one transaction | ok, no key added | `6GgwhTpBR9DK2CdjCfZTP5s77mnyDeDvrPo7jrZvyib4` |
| `set_agent` through DAO proposal 0 | ok | `9LXg2JEU…` (add), `GfXGFe2z…` (approve) |
| `register_intents_key` | ok, key registered | `2cWpV124QfHdTHRpEzpwjpLXmGv9ac8tnNhhYvDYDcq6` |
| Mint and deposit 10 USDC to the vault's Intents ledger | ledger shows 10,000,000 | `9mnnZoXd…`, `4SfvvCrq…` |
| Oracle reports (wNEAR $4.50, USDC $1.00) | ok | `EUfWPdAG…`, `499kGdtR…` |
| `propose` by the agent | proposal 1 | `7Q3oSFu65rS5XjnBPpuNsm6MtYy6inPSkYreKFj5CyR1` |
| `execute` | outcome `Executed`, signed intent retained | not captured, see below |
| `request_withdrawal` through DAO proposal 2 | queued, `effective_sec` 1794159940 | `F5s1ps27…` (add), `2N3GxKHg…` (approve) |
| `apply_pending_withdrawal` before maturity | refused: "the withdrawal is still in its delay" | `09-withdraw-apply-early.log` |

## Independent check of the signature

The retained signed intent was read from `get_signed_intent` and checked outside the vault:

- its public key `ed25519:ADbT3FKzBG8QF2YFG7UsBF2A3ZnPxezzCjoDgqrwJK2a` equals the key
  `v1.signer-prod.testnet` derives for this vault (`derived_public_key`, path `intents`,
  domain 1), and equals the key the vault registered;
- its signature verifies over the exact payload bytes.

So the vault obtained a real testnet MPC signature, and only after the on-chain gates passed.

## Withdrawal

The withdrawal of 1 USDC to `mandate-t1.testnet` is queued with maturity
**2026-11-08 17:45:40 UTC** (`effective_sec` 1794159940). Applying it after that time
is the remaining step: `cargo run -p mandate-vault --example testnet_run -- withdraw-apply`.
Until it is applied and its receiver and ledger changes are recorded, the delayed
withdrawal is **not** demonstrated.

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
contract), a production oracle, an elapsed delayed withdrawal (pending), recovery after a
lost callback (not exercised), or a live model provider run.
