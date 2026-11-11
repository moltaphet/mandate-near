# Mandate demo walkthrough

This guide shows what Mandate refuses and what it allows, using evidence already in the repository.
It is for a reviewer or a new reader. It does not authorize any chain action. Testnet only.
Mainnet is NO-GO: see [MAINNET-GATES.md](MAINNET-GATES.md).

## What you can show without touching a chain

These require no keys and no network access. Each reads a recorded result.

1. **Refusals on a local sandbox.** [docs/evidence/attacks.txt](evidence/attacks.txt) lists eight attacks
   and what the vault answered. Each line gives the attack, the expected refusal and the observed one:
   a stranger proposing a trade, a token outside the mandate, a trade over the per-trade cap, a slippage
   breach, a stale price, a future-dated price, a relay that changes the signed amount, and a stolen owner
   key raising the cap. Attack 8 shows the 24-hour delay: the raised cap is queued, the old cap still
   refuses a $60 trade, and an early apply is refused.
2. **Gas per call.** [docs/evidence/gas.txt](evidence/gas.txt) records gas use per vault call on a local sandbox.
3. **Testnet run with transaction hashes.** The [7 November run](evidence/testnet-run-2026-11-07/README.md)
   deployed a keyless vault with a DAO owner and a verified testnet MPC signature. The
   [vault2 run](evidence/testnet-vault2-2026-11-09/README.md) set a 5 USDC per-request withdrawal ceiling,
   executed one trade, and delivered a 1 USDC withdrawal after its 24-hour delay. Its README also records
   an incident in which a trade ran on the wrong vault. Show the incident as well: it is part of the record.
4. **Read-only dashboard.** `web/dashboard/index.html` shows the state of a named vault from RPC, read-only.
   Its numbers come from the RPC node you choose and are not independently verified. See the
   [dashboard limits](../THREAT_MODEL.md) in the out-of-scope section.

## What each step proves and does not prove

| Step | Proves | Does not prove |
|---|---|---|
| Sandbox attacks | The refusals hold on the current vault code, with stand-in verifier, signer and oracle | Behaviour against the real Intents or MPC contracts |
| Testnet run | The flow works end to end on testnet with recorded hashes | A matched settlement with a real counterparty, or a real price feed |
| Withdrawal delay | A queued withdrawal waits the full delay and then delivers | Failure and recovery paths on chain |
| Dashboard | What the RPC node reports | That the RPC node or the vault is correct |

## Live steps (each needs explicit approval per step)

These send transactions to testnet. Each one needs a separate approval from the owner before it runs.
The driver refuses to run any vault step unless `MANDATE_VAULT` is set explicitly.

```bash
MANDATE_VAULT=<vault> cargo run -q -p mandate-vault --example testnet_run -- status
MANDATE_VAULT=<vault> cargo run -q -p mandate-vault --example testnet_run -- price
MANDATE_VAULT=<vault> cargo run -q -p mandate-vault --example testnet_run -- propose
MANDATE_VAULT=<vault> cargo run -q -p mandate-vault --example testnet_run -- execute
MANDATE_VAULT=<vault> cargo run -q -p mandate-vault --example testnet_run -- withdraw-queue
MANDATE_VAULT=<vault> cargo run -q -p mandate-vault --example testnet_run -- withdraw-apply
```

`status` only reads. The others change state. In the vault2 run, the withdrawal request went through a DAO
proposal, and `withdraw-apply` was called by the parent account once the delay had passed.

## Not yet shown

- A full refusal gallery on testnet. Two refusals are recorded on chain (TradeTooLarge and
  InsufficientBalance); stale price, slippage, cooldown and the other gates are shown only on the sandbox.
- A real oracle. The current testnet oracle is a stand-in; see [testnet decisions](design/testnet-decisions.md).
- A matched settlement with an independent counterparty. The recorded settlement used an account the team controls.
- A model-decided trade. The model admission applies on 12 November; until then every recorded trade uses `manual-v1`.
- A failure or recovery case on chain (lost callback, failed signing, cancellation).
