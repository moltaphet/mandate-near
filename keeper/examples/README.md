# Optional relay publication

The keeper's default mode executes eligible proposals and writes signed intent files to `KEEPER_OUTBOX`. It makes no solver-relay publication requests.

Explicit publication mode:

```sh
cargo run -p mandate-keeper -- --publish keeper/examples/relay-public.json
```

Set the public config's `metadata_dir` to the agent's public outbox and `expected_chain_id` to the actual reviewed network. The existing keeper account configuration pays only for `execute`. The documented partner JWT can be supplied at runtime through `RELAY_AUTH`; never store it in this public file. No credentials are needed by the local mock HTTP tests.

Before publication, the keeper reads a final block, requires the same current signed result, compares its proposal ID, vault, verifier, exact input/output deltas, signature format and deadlines with the selected quote metadata, then sends the documented `publish_intent` request. A failed or unknown publication is retried with bounded backoff. The same signed nonce remains fixed.

The public `relay-<id>.json` file records acknowledgement only. `relay-observation-<id>.json` records an initial relay status and always keeps `settlement_verified: false`. Status is not continuously monitored by this mode. Independent final transaction/receipt and ledger evidence is still required before describing a trade as settled.
