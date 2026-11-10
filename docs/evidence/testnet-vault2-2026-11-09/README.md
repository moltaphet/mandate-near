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

## Trade and withdrawal on vault2

| Step | Result | Hash / evidence |
|---|---|---|
| `set_report` (wNEAR $4.50, USDC $1.00) | ok | `BTuFayd1AwXpBo816a5ydVCQfgaXq4x4SAFSN7638Kms`, `EXDr9xUkwUAHC8z5asErnDyYFJdmJ7tRNYJwfJddj5Wr` |
| `propose` by the agent (2 USDC for wNEAR, `manual-v1`) | proposal 1 | `7Z79APfDNeHFnB8hM7JpMUBC8Upy2YRVZVUTjnjgh5jE` |
| `execute` | outcome `Executed`, signed intent retained | `69rmz6bKWFBdE5CQdxui7vU65qNU9pvXSYg81xzMU96W` |
| `request_withdrawal` of 1 USDC through DAO proposal 4 | queued, `effective_sec` 1794313426 | `FhkaxboXGSqCkG85yU2KjVjyN1PyCoRqdR74vKDrcQxf` (add), `BRSRDKmnAsbg3Di1N4TAzinGE6eR8UHpJe4BJk3Kp8ec` (approve) |

The withdrawal was queued until `2026-11-10 12:23:46 UTC`. It was applied on 10 November 2026,
after that time, with `withdraw-apply` (log: [vault2-withdraw-apply.log](vault2-withdraw-apply.log)).

| Step | Result | Hash / evidence |
|---|---|---|
| `apply_pending_withdrawal` on vault2 | receipt failures 0; outcome `Delivered`, withdrawal id 1, 1,000,000 base units (1 USDC) | `3W3xycheutTk9xixMubbPLTP9JFEhzhYDug4BPXG9JBE` |

After the apply, `get_pending_withdrawal` and `get_withdrawal_status` return null, and
`get_last_withdrawal_outcome` returns `Delivered`. The receiver `mandate-t1.testnet` holds
2,000,000 base units of the testnet USDC stand-in (`usdc.mandate-t1.testnet`). The balance before
the withdrawal is not recorded in this folder.

## Operator-fed prices on the testnet oracle, 10 November 2026

The oracle is the testnet stand-in `oracle.mandate-t1.testnet` (`contracts/mock-oracle`). Its
`set_report` accepts a report from any caller, so these values are operator-fed testnet prices.
They are not a verified oracle, and [gate 3 in MAINNET-GATES.md](../../MAINNET-GATES.md) stays PENDING.

The prices came from the public CoinGecko `simple/price` endpoint through
[scripts/live-price-feed.py](../../../scripts/live-price-feed.py), then were written by the driver's
`price` step with `MANDATE_PRICE_JSON`. The driver refuses a file older than 60 seconds, which matches the
vault's `max_price_age_sec` of 60.

| Step | Result | Hash / evidence |
|---|---|---|
| `set_report` for `wrap.testnet`, 360,000,000 at 8 decimals (wNEAR $3.60) | receipt failures 0 | `AAo7JxME8NnrZnPY711kmSnv7pzhmCZJL4U2V5kykYnt` |
| `set_report` for `usdc`, 99,956,500 at 8 decimals (USDC $0.999565) | receipt failures 0 | `8fvCSWv5kBV6qGiSmGwoMS5fktKVzJGdTBEzmTvxUQq2` |

The full log is in [vault2-live-price.log](vault2-live-price.log). A read-only `get_price_data` on the
oracle returned both values. The `recency_duration_sec` the oracle reports is 600 seconds, but the vault
accepts a price only if it is 60 seconds old or less. A new `set_report` is needed before each trade.

The stand-in price of $4.50 for wNEAR, used by the earlier trade in this folder, was about 27% above the
CoinGecko price on that day. That trade therefore priced against the stand-in, not the market.

## Incident: a trade ran on the wrong vault

The first `propose` and `execute` on 9 November 2026 ran on `vault.mandate-t1.testnet`, the
first testnet vault, not on `vault2`. The cause was the driver checkout: the working tree was on
a branch based on `origin/main`, whose driver had no `MANDATE_VAULT` support. The steps then used
the default vault name without any warning. The operator approved those steps for `vault2` only.

- `propose` `6ab8SKsvyoXdub7EBCqnqMDriVeVMy9fhYTw4dc4tv2N` (by the agent) and `execute`
  `J4kcEuidCLcGCUSqqLYst25UHEEKfkfCmYCAsufkkPvs` ran on the first vault and succeeded with no
  receipt failures.
- Proposal 2 on the first vault was executed. Its signed intent, moving 2 USDC for about 0.45
  wNEAR, expired at `12:21:20 UTC`. The intent was never sent to any solver or verifier, so it
  had no effect on any other account.
- The price reports used by that run were on the shared oracle and were not wrong.
- The first vault has no further actions from this record. Its earlier evidence in
  `testnet-run-2026-11-07` is unchanged.

Fix: `contracts/vault/examples/testnet_run.rs` now refuses to run any vault step unless
`MANDATE_VAULT` is set explicitly. It has no default, and each run prints the target
(`TARGET | vault <name>`).
