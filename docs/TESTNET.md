# Testnet run, 4 Nov 2026

## Read-only refresh, 6 Nov 2026

At final block 276508606, the historical account `mandate-vault2.testnet`
still returned itself as owner and had one full-access account permission.
The returned WASM was 293,825 bytes, SHA256
`ea6960b601f44a52723320253b3a330140b5d9714a2b1ef06cc74f59dbb7db10`.
The inspector reported custody blockers and incomplete control/external views;
it made no readiness claim. This confirms the historical binary and unsafe
owner/permission model remain, not that the current source is deployed.
No permission values, signing, deployment or funds were used. Current source
readiness requirements are in [OPERATIONAL-READINESS.md](OPERATIONAL-READINESS.md).

## Historical transaction record

Historical observations from 4 November 2026, not a statement about the current
source or deployed accounts. They have not been independently re-fetched by
this review. No gate evaluation, MPC signature, funded settlement or delayed
vault withdrawal was demonstrated in this run.

The self-owned vault below is a historical control experiment, not the intended
custody deployment. Use a distinct recoverable owner before removing vault
full-access keys. Current source changes its stored layout; an old deployment
requires a reviewed migration for that layout or a fresh deployment, not only
a WASM replacement.

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
