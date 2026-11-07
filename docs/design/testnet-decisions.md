# Testnet decisions, 7 November 2026

Status: decided under the owner's delegation on 7 November 2026. Scope: testnet
only. Mainnet decisions are separate and stay open.

## Evidence

Read-only queries to `https://rpc.testnet.near.org`, 7 November 2026. No
transaction was sent.

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
| D1 oracle (testnet) | A self-operated Burrow-compatible oracle: `contracts/mock-oracle`, deployed to a testnet account we own and fed by the operator. It is a stand-in, not a production oracle. The mainnet oracle remains a separate, open decision. |
| D2 owner | A Sputnik DAO v2 created through `sputnikv2.testnet`, with a single-member council for the testnet run. Mainnet needs a multisig DAO with hardware keys. |
| D3 Intents on testnet | Deploy the supplied Intents verifier fixture (`contracts/vault/tests/fixtures/intents-near-mainnet.wasm`, SHA256 `a1afab98916d9f213746544dcf8e4e6bdb22c0be6bc5dfbd8ea8814c9922b547`) to a testnet account we own, with `contracts/mock-ft` tokens. It is a fixture, not the real NEAR Intents. |
| D4 signer | `v1.signer-prod.testnet`, domain 1. |
| D5 F26 | Accepted for the testnet run. No contract change before the run. The recovery design is written after the run and implemented only after review. |
| D6 withdrawal test | One withdrawal of a mock token after a real 86,400-second delay. |
| D7 model provider | Not part of the testnet run. |
| D8 accounts | One funded testnet operator account supplied by the owner. Every other account is a sub-account of it, so the vault, agent, Intents fixture, oracle and tokens share one namespace. The vault name must stay at most 62 bytes, and `s.<vault>` and `c.<vault>` must not exist. |

## What the run proves and does not prove

It proves the vault's gates, the real testnet MPC signature path, delayed
withdrawal timing and recovery behaviour on a real chain. It does not prove
behaviour against the real NEAR Intents contracts or a production oracle,
because those are replaced by labelled fixtures on testnet. Any claim made from
this run must say so.
