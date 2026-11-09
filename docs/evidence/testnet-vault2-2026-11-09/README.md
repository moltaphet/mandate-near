# Testnet vault2 setup, 9 November 2026

A second, fresh testnet vault, `vault2.mandate-t1.testnet`, deployed with the testnet driver
([`contracts/vault/examples/testnet_run.rs`](../../../contracts/vault/examples/testnet_run.rs),
`MANDATE_VAULT=vault2`). It exists to show the withdrawal ceiling on a new deployment. Testnet
only. Mainnet remains NO-GO; see [MAINNET-GATES.md](../../MAINNET-GATES.md).

## Build

`build.log` records the no-ABI build of the vault from source. The vault WASM is 459,584
bytes with SHA256 `a7dec54d8f5687db29b903e2f12c91e0d5eb5b3294d9487d178be37b4cbe6fe9`, the same
hash as [`wasm-main-2026-11-09`](../wasm-main-2026-11-09/measurement.txt).

## Steps and transaction hashes

| Step | Result | Hash / evidence |
|---|---|---|
| Create, fund, deploy and initialise the vault in one transaction (`new_with_externals`, no key added) | ok | `AsmzwyXrx6abvNDHS5pJySoqDabQ4RmxvYe44nKiwBP4` (`vault2-deploy.log`) |
| Status after deploy | no access keys; no agent; key not registered; ledger 0 | `vault2-status.log` |
| `set_agent` through DAO proposal 3 | ok, agent `agent.mandate-t1.testnet` | `LCV3ZuM5uJZUEygNRfhwHParxQ9iSfhdnPH3BQPU1xz` (add), `Djkv8xPBAvhKf6hB6qtKmALwB8S2eLmYH5k2VNw1MVp7` (approve) |
| `register_intents_key` | ok | `A9BJn1HFsj6Cnazy4apYC27mN72QiHYHyL9xVen3MuRF` |
| `mint` 10 USDC to the operator | ok | `jBY7aUFa2EHHgSBEdrk9JD7egQeeXowUMDj3rYfnsGh` |
| `ft_transfer_call` to the Intents verifier for vault2 | ok | `8KkUtE6Fj4bpYA7CuydoyR3n1au37jiacZSpDLXJvJfV` |
| Status after funding | agent set; key registered; Intents ledger 10,000,000 | `vault2-status-after-funding.log` |

Each state-changing step printed its hash and reported zero receipt failures.

## What is on chain

- Owner: the Sputnik DAO `mandate-t1.sputnikv2.testnet`, with the operator as its single
  council member.
- Withdrawal ceiling: `withdrawal_limits` = 5,000,000 base units (5 USDC) per request, read from
  the stored mandate with `get_mandate`.
- Model allowlist: `manual-v1`.
- Externals: signer `v1.signer-prod.testnet`, Intents `intents.mandate-t1.testnet`, oracle
  `oracle.mandate-t1.testnet`. The Intents verifier and the oracle and token are the same
  labelled stand-ins described in [testnet-run-2026-11-07](../testnet-run-2026-11-07/README.md).

## Not done

No oracle report, proposal, execution, or withdrawal has been made on vault2. Each of those
needs its own approval and is not part of this record.
