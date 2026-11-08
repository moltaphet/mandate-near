# Public rebalance configuration

`rebalance-public.json` is a template. Replace the vault and asset account names with the actual reviewed vault's two mandate assets. Those names are examples, not evidence of a deployment or supported mainnet feed.

Run a credential-free preview:

```sh
cargo run -p mandate-agent -- --auto agent/examples/rebalance-public.json --dry-run
```

Dry-run performs only public status/view/quote requests. It does not read account signing or relay authentication settings, create an outbox, propose a trade, or publish an intent. The documented solver relay currently requires a partner JWT in `X-API-Key`; an unauthenticated preview can therefore fail at the quote request. Stale or missing oracle prices stop before quoting. Local HTTP tests establish the protocol, not live provider availability.

Executing `--auto <public-config.json>` requires the existing agent account configuration and `AGENT_OUTBOX`; `--loop` repeats every 30 seconds after the previous decision completes. The manual `<public-plan.json>` mode remains available. Optional `RELAY_AUTH` is read only in executing autonomous mode and explicit keeper publication mode. Do not put authentication material into the public JSON file.

In `--loop`, failed public snapshot/quote reads and changed quotes at refresh are retried with a bounded 10–600 second backoff; a successful preparation resets it. Logs omit provider error details. One-shot mode still returns preparation errors. Once the submission path starts, errors stop the process: a missing transaction response can mean the proposal reached the chain. Reconcile the transaction, current proposal and retained outcome before restarting, rather than submitting another proposal blindly.

The public `NEAR_CHAIN_ID` or `EXPECTED_CHAIN_ID` setting pins the RPC network before signer settings are loaded in manual, autonomous and model execution. All specified environment pins and an autonomous config pin must agree. Blank pins are refused. Without a pin, public network identity and final-anchor checks still run before signing configuration is loaded.

The strategy supports exactly two listed assets, values actual ledger holdings in micro-USD, trades toward `target_a_bps` outside `band_bps`, and caps exact input using balance, the per-trade rule and remaining daily authorization quota. It selects a matching exact-input quote and uses the exact quoted output as min-out. Every vault gate is previewed through the shared engine. A fresh final snapshot repeats readiness, target and engine checks immediately before proposal submission; races remain subject to the authoritative on-chain checks.

Offers must remain valid longer than the configured `minimum_quote_validity_sec` (450 seconds minimum, covering the 300-second build window, 120-second intent lifetime and a margin). Shorter or expired offers are refused, including before publication. The config's optional `expected_chain_id` is checked against public RPC status. Block hash and height must match the final anchor on every view.

The agent writes public `proposal-<id>.json` metadata with the chosen quote, exact plan, vault/verifier identity and final snapshot. The keeper requires this matching file before explicitly opted-in relay publication. A signature, publication acknowledgement, or relay-reported `SETTLED` status does not independently prove final settlement.

Protocol source: https://docs.near-intents.org/integration/market-makers/message-bus/rpc
