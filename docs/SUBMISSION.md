# Mandate: submission summary

Mandate lets an AI agent propose treasury trades under an owner-defined mandate. The vault's on-chain gates
decide what may be signed. Testnet only. Mainnet is NO-GO.

## What is shown on testnet

All transactions are on `vault2.mandate-t1.testnet`, with its owner a Sputnik DAO. Full index:
[testnet-vault2-2026-11-09/README.md](evidence/testnet-vault2-2026-11-09/README.md).

| Scenario | What happened | Transaction |
|---|---|---|
| Owner delay is real | 1 USDC withdrawal queued, then delivered after the 24-hour delay (`Delivered`) | [`3W3xycheutTk9xixMubbPLTP9JFEhzhYDug4BPXG9JBE`](https://testnet.nearblocks.io/txns/3W3xycheutTk9xixMubbPLTP9JFEhzhYDug4BPXG9JBE) |
| Trade settles on both sides | Vault signed 2 USDC for 0.55 wNEAR. A matched intent from a counterparty we control settled in one `execute_intents`. Vault: −2 USDC, +0.55 wNEAR. Counterparty: −0.55 wNEAR, +2 USDC | [`B8nYgKKYcNCrYEuG9QZ92KUJe3vj641ZhvZTzy9VzXo5`](https://testnet.nearblocks.io/txns/B8nYgKKYcNCrYEuG9QZ92KUJe3vj641ZhvZTzy9VzXo5) |
| Per-trade cap refuses | 60 USDC proposal with 67 USDC held. Gate result `TradeTooLarge` (59.97 > 50 USDC cap). Nothing moved | [`J4RvGERYiuFv2hfninTCFWxqjw4HikG6rXH7VRSsDjBM`](https://testnet.nearblocks.io/txns/J4RvGERYiuFv2hfninTCFWxqjw4HikG6rXH7VRSsDjBM) |
| Balance check refuses | 60 USDC proposal with 7 USDC held. Gate result `InsufficientBalance`. Nothing moved | [`GoUU2JWXjPSWdbf5tZU2F8ypFQvxrYkPBGnuWSSNyXbc`](https://testnet.nearblocks.io/txns/GoUU2JWXjPSWdbf5tZU2F8ypFQvxrYkPBGnuWSSNyXbc) |
| Model admission queued | `gemini-3.1-flash-lite` queued through the DAO. Effective 12 Nov 2026 07:37:49 UTC | [`4sFspKrxZbiwqTsstfPQCyAEKGnnaAtZnM9hU1ERe4nK`](https://testnet.nearblocks.io/txns/4sFspKrxZbiwqTsstfPQCyAEKGnnaAtZnM9hU1ERe4nK) |

## What is not shown yet

- **A model-decided trade.** The model is queued, not admitted, until 12 Nov 07:37 UTC. Until then the trades above use the manual agent path (`manual-v1`).
- **A real counterparty.** The counterparty is an account we control, not an independent solver.
- **A real oracle.** Prices are operator-fed testnet values from a public source, with source timestamps checked. The testnet oracle accepts reports from any caller.
- **A real Intents contract.** The verifier is the fixture build.
- **An independent audit.** None has been done. See [audit/AUDIT-PACKAGE.md](audit/AUDIT-PACKAGE.md).

## Three-minute demo script

1. **Problem (30 s).** An AI agent should be able to move treasury funds, but a compromised or wrong agent must not be able to drain the treasury.
2. **Mandate (30 s).** The owner sets limits: a per-trade cap, a token list, a slippage limit, a price-age limit, and an allowed model list. Each limit is enforced on-chain, not by the agent.
3. **Settled trade (60 s).** Show the settlement row above. Point at the two ledgers before and after. Stress that the counterparty is ours and the prices are testnet.
4. **Refusal (30 s).** Show the cap refusal. The agent asked for 60 USDC against a 50 USDC cap, and the contract refused it.
5. **Delay and boundary (30 s).** Show the withdrawal row (24-hour delay, then `Delivered`) and the note on what is not shown.

## Verify without a block explorer

Block explorers may show empty pages for older transactions. The results can be checked from the contracts directly:

```bash
python3 -B scripts/demo-readback.py
```

This makes public view calls only. It sends no transaction and needs no key. It prints the last proposal outcome (`Executed` or `Rejected` with its reason), the strike count, the withdrawal outcome (`Delivered`), the allowed models, and the verifier ledgers for the vault and the counterparty. A recorded run is in [readback-2026-11-11.txt](evidence/testnet-vault2-2026-11-09/readback-2026-11-11.txt).

## Reproduce

- Driver: `contracts/vault/examples/testnet_run.rs`. Every vault step needs `MANDATE_VAULT` set.
- Price feed: `scripts/live-price-feed.py`. It sends nothing and writes a JSON file.
- Tests: `cargo test -p mandate-vault --lib` (286 tests).
- Site checks: `scripts/deploy-site.sh --check` (local only; production needs explicit approval).

Mainnet remains NO-GO. [MAINNET-GATES.md](MAINNET-GATES.md) lists the gates still pending.
