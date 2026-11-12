# Testnet decisions, 7 November 2026

Status: decided under the owner's delegation on 7 November 2026. Scope: testnet
only. Mainnet decisions are separate and stay open.

## Evidence

The table below records read-only prerequisite queries to
`https://rpc.testnet.near.org` on 7 November 2026; those queries sent no
transactions. The later deployment and owner calls have separate
[transaction evidence](../evidence/testnet-run-2026-11-07/README.md).

| Account | Result |
|---|---|
| `v1.signer-prod.testnet` | Contract present, code hash `CpGX8ywXJwvWPgZFDDCTCuPJBHF7BATT51gSfc3APERJ`, version `3.18.0`. `derived_public_key` answers for domain 1 (ed25519) and domain 0 (secp256k1). Real MPC signing is possible on testnet. |
| `priceoracle.testnet` | Contract present and `get_price_data` answers with a current timestamp, but none of the 13 listed assets had a price at query time. Not usable as the only price source. |
| `pyth-oracle.testnet` | Code present, but Pyth Core is documented as unsupported on NEAR. Not used. |
| `intents.testnet` | No contract (code hash is all ones). There is no Intents verifier on testnet. |
| `sputnikv2.testnet` | Sputnik DAO v2 factory present. |

## Decisions

| ID | Decision |
|---|---|
| D1 oracle (testnet) | A self-operated Burrow-compatible oracle: `contracts/mock-oracle`, deployed to a testnet account we own and fed by the operator. It is a stand-in, not a production oracle. Since 12 November 2026 the testnet build uses the `parent-writes-only` feature, so only the parent account can write prices. The mainnet oracle remains a separate, open decision. |
| D2 owner | A Sputnik DAO v2 created through `sputnikv2.testnet`, with a single-member council for the testnet run. Mainnet needs a multisig DAO with hardware keys. |
| D3 Intents on testnet | Deploy the supplied Intents verifier fixture (`contracts/vault/tests/fixtures/intents-near-mainnet.wasm`, SHA256 `a1afab98916d9f213746544dcf8e4e6bdb22c0be6bc5dfbd8ea8814c9922b547`) to a testnet account we own, with `contracts/mock-ft` tokens. It is a fixture, not the real NEAR Intents. |
| D4 signer | `v1.signer-prod.testnet`, domain 1. |
| D5 F26 | Accepted for the current prototype/testnet scope only. The implementation retains the unknown-result guard until authenticated proof resolves it. Historical abandon proposals are unapproved; any protocol change needs separate review. |
| D6 withdrawal test | Planned: one withdrawal of a mock token after a real 86,400-second delay. Queueing and early refusal were observed; delivery and recovery remain unverified in the recorded run. |
| D7 model provider | Not part of the testnet run. |
| D8 accounts | One funded testnet operator account supplied by the owner. Every other account is a sub-account of it, so the vault, agent, Intents fixture, oracle and tokens share one namespace. The vault name must stay at most 62 bytes, and `s.<vault>` and `c.<vault>` must not exist. |

## What the recorded run shows

The [7 November run](../evidence/testnet-run-2026-11-07/README.md) records keyless
deployment, DAO owner calls, key registration and a verifiable real testnet MPC
signature. It also records a withdrawal queued with a 24-hour maturity and an
early-apply refusal. The execute transaction hash was not captured; its retained
outcome and exact signature were checked separately.

The recorded withdrawal remained pending. Elapsed-delay delivery, receiver/ledger
changes and recovery after a lost callback were not demonstrated. Matched funded
settlement and production-oracle behavior remain unverified because the run used
labelled Intents, token and oracle fixtures. These results apply to that historical
testnet artifact, not the 8 November hardening candidate or a mainnet release.
