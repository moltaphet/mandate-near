# Mandate: submission summary

Mandate lets an AI agent propose treasury trades under an owner-defined mandate. The vault's on-chain gates
decide what may be signed. Testnet only. Mainnet is NO-GO.

## What is shown on testnet

All transactions are on `vault2.mandate-t1.testnet`, with its owner a Sputnik DAO. Full index:
[testnet-vault2-2026-11-09/README.md](evidence/testnet-vault2-2026-11-09/README.md).

| Scenario | What happened | Transaction |
|---|---|---|
| Owner delay is real | 1 USDC withdrawal queued, then delivered after the 24-hour delay (`Delivered`) | [`7mG76TGMSkGPPXG28fLRDKTygaGVoZUeVg5N1u8Y7wfZ`](https://testnet.nearblocks.io/txns/7mG76TGMSkGPPXG28fLRDKTygaGVoZUeVg5N1u8Y7wfZ) |
| Trade settles on both sides | Vault signed 2 USDC for 0.55 wNEAR. A matched intent from a counterparty we control settled in one `execute_intents`. Vault: −2 USDC, +0.55 wNEAR. Counterparty: −0.55 wNEAR, +2 USDC | [`7uTJpKFkEDfKEqz1PxzNePeXJN27LK1ank75E6TRonZE`](https://testnet.nearblocks.io/txns/7uTJpKFkEDfKEqz1PxzNePeXJN27LK1ank75E6TRonZE) |
| Per-trade cap refuses | 60 USDC proposal with 67 USDC held. Gate result `TradeTooLarge` (59.97 > 50 USDC cap). Nothing moved | [`9cdyxmaAW6Q79SNbfayzoYBn6JFAfA362eEzjfGSKQhA`](https://testnet.nearblocks.io/txns/9cdyxmaAW6Q79SNbfayzoYBn6JFAfA362eEzjfGSKQhA) |
| Balance check refuses | 60 USDC proposal with 7 USDC held. Gate result `InsufficientBalance`. Nothing moved | [`C17Yg3kqqA2jkxvzyJcYz5dRBE6HBZKCLt2MPskzF1dA`](https://testnet.nearblocks.io/txns/C17Yg3kqqA2jkxvzyJcYz5dRBE6HBZKCLt2MPskzF1dA) |
| Model admission waits 24 hours | `gemini-3.1-flash-lite` queued through the DAO on 11 Nov, effective 12 Nov 2026 07:37:49 UTC | [`Fg5CEsnvMbooJHtUobKh2i1wsN1pyzoqtA74pfeok4Xj`](https://testnet.nearblocks.io/txns/Fg5CEsnvMbooJHtUobKh2i1wsN1pyzoqtA74pfeok4Xj) |
| Model admission applied | Applied at 07:38:59 UTC on 12 Nov by a permissionless `apply_pending_mandate`. Allowlist is now `manual-v1`, `gemini-3.1-flash-lite`; nothing queued | [`Exen1r7y3un6fNq7xYTnvRKqoCnaNv1bqFA93XXzMvVs`](https://testnet.nearblocks.io/txns/Exen1r7y3un6fNq7xYTnvRKqoCnaNv1bqFA93XXzMvVs) |
| First model-decided proposal | Proposal 6 by `gemini-3.1-flash-lite`: 0.142450 wNEAR for at least 49.75 USDC, a wrong valuation by the model (about 100 times the market). Never executed: the RPC refused the execute (`NotEnoughBalance` on the sending account), and the proposal expired with no strike. The agent now refuses such a plan before sending | [`8Ek1SRJWUZGdUZbG7JNE2EWsPAdBDBaF7kKUYq6hyWQ8`](https://testnet.nearblocks.io/txns/8Ek1SRJWUZGdUZbG7JNE2EWsPAdBDBaF7kKUYq6hyWQ8) |
| Model-decided trade settles | Proposal 8: `gemini-3.1-flash-lite` chose one of the code-priced quotes, 0.290698 wNEAR for at least 0.995503 USDC. The vault checked every gate and the MPC signed (`Executed`); one `execute_intents` settled it. Vault +0.995503 USDC −0.290698 wNEAR; counterparty the mirror | [`5ZMSQdRgwkMoWVFy48tetrbPPJBMX5SgLKSbpJhKmzh7`](https://testnet.nearblocks.io/txns/5ZMSQdRgwkMoWVFy48tetrbPPJBMX5SgLKSbpJhKmzh7), [`BYWPTGwcw5nYkByRdTX9ReqKFvyYWxyJWiJCiCBRiASY`](https://testnet.nearblocks.io/txns/BYWPTGwcw5nYkByRdTX9ReqKFvyYWxyJWiJCiCBRiASY) |

## What is not shown yet

- **Model judgement.** The model chooses among quotes that code prices and checks; its recorded rationale for proposal 8 is not sound. The quotes and the vault's gates bound what it can do. Origin proof (a signed response or a TEE attestation) is not shown: the response is recorded as `UNVERIFIED`.
- **A real counterparty.** The counterparty is an account we control, not an independent solver.
- **A real oracle.** Prices are operator-fed testnet values from a public source (CoinGecko, cross-checked against Kraken), with source timestamps checked. Since 12 November the testnet oracle accepts reports only from its parent account; before that it accepted any caller.
- **A real Intents contract.** The verifier is the fixture build.
- **An independent audit.** None has been done. See [audit/AUDIT-PACKAGE.md](audit/AUDIT-PACKAGE.md).

## Three-minute demo script

1. **Problem (30 s).** An AI agent should be able to move treasury funds, but a compromised or wrong agent must not be able to drain the treasury.
2. **Mandate (30 s).** The owner sets limits: a per-trade cap, a token list, a slippage limit, a price-age limit, and an allowed model list. Each limit is enforced on-chain, not by the agent.
3. **Allowed: a model-decided trade settles (60 s).** Show proposal 8: the model chose a code-priced quote, the vault checked every gate, the MPC signed, and both ledgers changed. Stress that the counterparty is ours and the prices are testnet.
4. **Refused: dangerous requests (30 s).** Show the cap refusal (60 USDC against a 50 USDC cap, refused on chain) and proposal 6 (a model request at about 100 times the market, which the agent now refuses before sending).
5. **Delay and boundary (30 s).** Show the withdrawal row (24-hour delay, then `Delivered`) and the note on what is not shown.

## Verify without a block explorer

Every hash above is the real transaction hash and opens on the explorer. The run logs printed different values because of a near-api 0.8.7 bug; [TX-HASH-CORRECTIONS.md](evidence/TX-HASH-CORRECTIONS.md) maps each one to its real transaction. The results can also be checked from the contracts directly:

```bash
python3 -B scripts/demo-readback.py
```

This makes public view calls only. It sends no transaction and needs no key. It prints the last proposal outcome (`Executed` or `Rejected` with its reason), the strike count, the withdrawal outcome (`Delivered`), the allowed models, and the verifier ledgers for the vault and the counterparty. Recorded runs are in [readback-2026-11-11.txt](evidence/testnet-vault2-2026-11-09/readback-2026-11-11.txt) and, after the model admission applied, [readback-2026-11-12.txt](evidence/testnet-vault2-2026-11-09/readback-2026-11-12.txt).

## Reproduce

- Driver: `contracts/vault/examples/testnet_run.rs`. Every vault step needs `MANDATE_VAULT` set.
- Price feed: `scripts/live-price-feed.py`. It sends nothing and writes a JSON file.
- Tests: `cargo test -p mandate-vault --lib` (286 tests).
- Site checks: `scripts/deploy-site.sh --check` (local only; production needs explicit approval).

Mainnet remains NO-GO. [MAINNET-GATES.md](MAINNET-GATES.md) lists the gates still pending.
