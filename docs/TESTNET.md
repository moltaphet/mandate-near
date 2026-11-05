# Testnet run, 4 Nov 2026

What ran on NEAR testnet, what the chain said, and what did not run. Every line below is an observed result, not a target.

## Accounts and contracts

| | |
| --- | --- |
| Vault | `mandate-vault2.testnet` (owner = the same account; agent = `1dc936a53f908babc717e8f0a6efe2409373a8799e645ed94a368babba4e2e8c`) |
| Signer | `v1.signer-prod.testnet` (exists on testnet) |
| Intents | `intents.testnet` (**no contract deployed**: code hash is all ones) |
| Oracle | `price-oracle.testnet` (exists; no fresh price for the test feeds) |

The vault build is from `feat/owner-controls` (`contracts/vault`, wasm sha256 prefix `ea6960b601f44a52`).

## What worked on the chain

1. **Deploy and init** with `new_with_externals`. `get_owner`, `get_agent`, `get_externals` return the values set at init and by `set_agent`.
2. **Loosening waits.** `set_mandate` with a higher slippage cap returned `Queued` and the old mandate stayed in force. `get_pending_mandate` showed the queued change. `cancel_pending_mandate` removed it.
3. **Proposal.** The agent script's library proposed a trade. The vault stored it as pending.
4. **Fail closed.** `execute` panicked with `the intents key is not registered`. The proposal expired with no strike counted, which matches the design: a failed read or signature leaves the proposal pending until expiry.

## What did not run, and why

- **Gate evaluation and the MPC signature never ran on testnet.** `execute` stops at the intents key check, which runs before the gates. The key cannot be registered because `intents.testnet` has no contract.
- **Full trade on testnet is not possible** until someone deploys `intents.testnet` or the test moves to mainnet.

## Cost observed

- A failed `execute` burnt about 9.17 TGas in total, of which the vault's own receipt burnt about 1.33 TGas. The receipt's fee was about 0.13 NEAR. Each retry costs that again, which is why the keeper now backs off.
- These are single observations from failed transactions. They are not a gas budget for a successful trade.

## Reproduce

Needs a funded testnet account and the vault deployed as above. The steps are the owner controls (`set_mandate`, `get_pending_mandate`, `cancel_pending_mandate`), then `propose` as the agent, then `execute` as anyone. The sandbox tests reproduce the same logic without funds: `./scripts/verify.sh --integration`.

## Run 2, 5 Nov 2026: the current vault on testnet

The vault as it stands after SR-1, SR-3, SR-5 and SR-6. The owner is `mandate-r1-4715a0.testnet`. The vault is `vault.mandate-r1-4715a0.testnet` and the agent is `agent.mandate-r1-4715a0.testnet`, both subaccounts created by the owner's key. The externals are the testnet ones: `v1.signer-prod.testnet`, `intents.testnet` and `price-oracle.testnet`.

The run is `contracts/vault/tests/testnet_run.rs`. It is ignored by default and needs `MANDATE_TESTNET_RUN=1`.

| Step | Result |
| --- | --- |
| Create vault (5 NEAR) and agent (0.5 NEAR) subaccounts | ok |
| `new_with_externals` deployed and initialised in one step | ok; `get_externals` shows the three testnet externals |
| `set_agent` (first agent) | applied at once; `get_agent` returns the agent |
| `set_mandate` with a higher `max_trade_usd` | `Queued`, effective 24 h later |
| `cancel_pending_mandate` | ok; `get_pending_mandate` is then empty |
| `request_withdrawal` (1000 units of wNEAR to the owner) | queued |
| `apply_pending_withdrawal` before the delay | refused: `the withdrawal is still in its delay` |
| `cancel_pending_withdrawal` | ok |
| `propose` as the agent | stored as proposal 1; then proposal 2 in the recheck |
| `execute` as the owner | refused: `the intents key is not registered` |
| Delete the vault's full-access key (SR-9) | ok; 0 access keys left on the vault |

Failed transactions burnt about 1.89 TGas each (the early apply and the execute). The key deletion burnt about 0.63 TGas.

### What this run did not reach

- **The signing path.** `intents.testnet` has no contract, so the key cannot be registered and nothing is signed. `execute` stops at the key check, before the gates and the MPC call.
- **The send after the 24 h delay.** The sandbox clock cannot reach it, and this run did not wait a day. The request, the early refusal and the cancel ran on the chain.
- **The script agent and the keeper against a live chain.** Not run in this step.
