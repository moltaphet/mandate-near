# Testnet vault2 setup, 9 November 2026

A second, fresh testnet vault, `vault2.mandate-t1.testnet`, deployed with the testnet driver
([`contracts/vault/examples/testnet_run.rs`](../../../contracts/vault/examples/testnet_run.rs),
`MANDATE_VAULT=vault2`). It exists to show the withdrawal ceiling on a new deployment. Testnet
only. Mainnet remains NO-GO; see [MAINNET-GATES.md](../../MAINNET-GATES.md).

## Transaction index

Every transaction on vault2 in this folder, in order. Each one has a receipt with 0 failures unless noted. The
detailed tables and logs are in the sections below.

| # | Date (UTC) | Action | Hash | Result |
|---|---|---|---|---|
| 1 | 9 Nov | Deploy and initialise vault2 | `AsmzwyXrx6abvNDHS5pJySoqDabQ4RmxvYe44nKiwBP4` | ok |
| 2 | 9 Nov | `set_agent` via DAO (proposal 3) | `LCV3ZuM5…`, `Djkv8xPB…` | ok |
| 3 | 9 Nov | `register_intents_key` | `A9BJn1HF…` | ok |
| 4 | 9 Nov | `mint` 10 USDC, deposit to verifier | `jBY7aUFa…`, `8KkUtE6F…` | ok |
| 5 | 9 Nov | `set_report` ×2 (stand-in $4.50 / $1.00) | `BTuFayd1…`, `EXDr9xUk…` | ok |
| 6 | 9 Nov | `propose` 2 USDC for 0.45 wNEAR (proposal 1) | `7Z79APfD…` | ok |
| 7 | 9 Nov | `execute` proposal 1 | `69rmz6bK…` | Executed |
| 8 | 9 Nov | `request_withdrawal` 1 USDC via DAO (proposal 4) | `FhkaxboX…`, `BRSRDKmn…` | queued |
| 9 | 10 Nov | `set_report` ×2 (CoinGecko, live) | `AAo7JxME…`, `8fvCSWv5…` | ok |
| 10 | 10 Nov | `apply_pending_withdrawal` 1 USDC | `3W3xycheutTk9xixMubbPLTP9JFEhzhYDug4BPXG9JBE` | Delivered |
| 11 | 10 Nov | `set_report` ×2 (live, $3.60) | `6ZpBUv2C…`, `8aqjmmse…` | ok |
| 12 | 10 Nov | `propose` 2 USDC for 0.55 wNEAR (proposal 2) | `V6rQUkKZ…` | ok |
| 13 | 10 Nov | `execute` proposal 2 | `GX3H8soz…` | Executed |
| 14 | 11 Nov | DAO `set_mandate`: admit `gemini-3.1-flash-lite` (proposal 5 of the DAO) | `4sFspKrx…`, `Gd6WPZus…` | queued to 12 Nov 07:37 UTC; applied in row 24 |
| 15 | 11 Nov | Solver account created, key registered on verifier | `9jUQk8uq…`, `A1UQ2QYN…` | ok |
| 16 | 11 Nov | Solver: wrap 1 NEAR, storage, verifier storage, deposit 0.99 wNEAR | `7z4LM6BU…`, `99KkXHo4…`, `Fv2YpJTR…`, `D6y8Noj9…` | ok (two earlier attempts failed, not counted) |
| 17 | 11 Nov | `set_report` ×2, `propose` 2 USDC for 0.55 wNEAR (proposal 3), `execute` | `BWoaDazW…`, `H5QEezqw…`, `5ptybytM…`, `4a6qSdVN…` | Executed |
| 18 | 11 Nov | `execute_intents`: vault and solver intents settled | `B8nYgKKY…` | ledgers changed as listed |
| 19 | 11 Nov | `set_report` ×2, `propose` 60 USDC (proposal 4), `execute` | `HVq5STkP…`, `HND6fizC…`, `ALsNp7Nu…`, `GoUU2JWX…` | Rejected: InsufficientBalance |
| 20 | 11 Nov | `mint` 60 USDC, deposit to verifier | `3XMkRBUE…`, `8HxEpoX6…` | ok |
| 21 | 11 Nov | `set_report` ×2, `propose` 60 USDC (proposal 5), `execute` | `5YRs2UHE…`, `4sGusYnL…`, `Fgv7ad46…`, `J4RvGERY…` | Rejected: TradeTooLarge |
| check | 11 Nov | Read-only readback, no transaction (`scripts/demo-readback.py`) | [readback-2026-11-11.txt](readback-2026-11-11.txt) | Last outcome, strikes, withdrawal and ledgers match rows 1–21 |
| 22 | 11 Nov | Solver top-up: mint 20 USDC and deposit it for the solver; wrap 0.3 NEAR (`solver-topup`) | `Q4tUjTaj…`, `DGgVUpW1…`, `Bho28s95…` | ok |
| 23 | 11 Nov | Deposit the wrapped 0.3 wNEAR for the solver (`solver-topup-wnear`, 100 TGas) | `3nc47Muh…` | ok |
| 24 | 12 Nov | `apply_pending_mandate` after the delay (`mandate-apply`), 07:38 UTC | `DzUFptM81MGMUBzaCpFWv4UiEjQXwu147LPJP8SK2fF9` | Applied: `gemini-3.1-flash-lite` admitted |
| check | 12 Nov | Read-only readback after row 24, no transaction | [readback-2026-11-12.txt](readback-2026-11-12.txt) | Allowlist has both model ids; no change queued; ledgers unchanged |

**Net state at the end:** after step 18, vault2 holds 7 USDC and 0.55 wNEAR on the verifier, and the solver holds
2 USDC and 0.44 wNEAR. Step 20 adds 60 USDC, so vault2 holds 67 USDC. Steps 19 and 21 are refused and move nothing.
The strike count is 2, no proposal is pending, and the vault is not paused.
Steps 22 and 23 top up the solver for the model-decided trade: it then holds 22 USDC and 0.74 wNEAR, and vault2
is unchanged. Two earlier submissions of the wNEAR deposit at 200 TGas were rejected by the RPC before execution
(InvalidTransaction) and moved nothing; the log is [solver-topup.log](solver-topup.log).
Step 24 applies the queued model admission. The allowlist is then `["manual-v1", "gemini-3.1-flash-lite"]`,
no mandate change is queued, and balances, strikes (2) and the other mandate fields are unchanged.

Abbreviations: a hash ending in `…` is shortened here. The full hash is in the section named in the table.

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
`price` step with `MANDATE_PRICE_JSON`.

**Correction for the two writes above and the trade below.** Those writes stamped each report with the
write time, not the source's own time, so the vault's 60-second age check could not see how old the
quote was. The quotes were a few seconds old at write time, so the values are not wrong, but the stamp
hid the age. From the next write on, the driver stamps each feed with its source timestamp and refuses
any quote older than 60 seconds or dated in the future.

| Step | Result | Hash / evidence |
|---|---|---|
| `set_report` for `wrap.testnet`, 360,000,000 at 8 decimals (wNEAR $3.60) | receipt failures 0 | `AAo7JxME8NnrZnPY711kmSnv7pzhmCZJL4U2V5kykYnt` |
| `set_report` for `usdc`, 99,956,500 at 8 decimals (USDC $0.999565) | receipt failures 0 | `8fvCSWv5kBV6qGiSmGwoMS5fktKVzJGdTBEzmTvxUQq2` |

The full log is in [vault2-live-price.log](vault2-live-price.log). A read-only `get_price_data` on the
oracle returned both values. The `recency_duration_sec` the oracle reports is 600 seconds, but the vault
accepts a price only if it is 60 seconds old or less. A new `set_report` is needed before each trade.

The stand-in price of $4.50 for wNEAR, used by the earlier trade in this folder, was about 27% above the
CoinGecko price on that day. That trade therefore priced against the stand-in, not the market.

## Trade on vault2 at the live price, 10 November 2026

Written in one run, [vault2-live-trade.log](vault2-live-trade.log): prices written, then `propose`, then `execute`.

| Step | Result | Hash / evidence |
|---|---|---|
| `set_report` for `wrap.testnet` at $3.60 | receipt failures 0 | `6ZpBUv2Cod5nX5tCpL6cj3eHvjKKBRiPmcUu4rLLevfG` |
| `set_report` for `usdc` at $0.999587 | receipt failures 0 | `8aqjmmseMMyY25Knr1ngKRuU64xEfybGnNxNseQ8jr8` |
| `propose` by the agent: 2 USDC for at least 0.55 wNEAR, `manual-v1` | proposal 2 | `V6rQUkKZg9Xm5Svfy7V5jGuyZkYcho5EdBsgR4nquWt` |
| `execute` | outcome `Executed`, proposal 2 | `GX3H8soziLDsfb9F71zM6LunC11Ff48GL5rTS68ic4tQ` |

The minimum was set to 0.55 wNEAR, not the 0.45 used in proposal 1. The 0.45 minimum was for the $4.50
stand-in. At $3.60, 0.45 wNEAR is worth $1.62, below the 1% slippage floor of about $1.98 on $2 of USDC,
so the vault would refuse it. The vault checks slippage against the oracle before it signs, so an
attempted trade at 0.45 was not made.

The signed intent for proposal 2 is `token_diff` with USDC −2,000,000 and wNEAR +550,000,000,000,000,000,000,000.
After the run, `get_pending_proposal` is null, strikes are 0, the vault is not paused, and nothing is in flight.

What this run does not show: `execute` records the vault's signature. As in proposal 1, no solver filled
the intent in this run, so the balances on the Intents stand-in were not changed by a counterparty.
The prices are operator-fed testnet values, as described above.

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

## Model admission queued on vault2, 11 November 2026

The owner proposed a mandate change through DAO proposal 5 (`set_mandate`). Its only loosened field is
`allowed_model_ids`, which now also lists `gemini-3.1-flash-lite`. Everything else in the mandate is unchanged,
which `simulate_mandate_change` confirmed read-only. The change is queued for the mandate delay.

| Step | Result | Hash / evidence |
|---|---|---|
| `add_proposal` (set_mandate, DAO proposal 5) | receipt failures 0 | `4sFspKrxZbiwqTsstfPQCyAEKGnnaAtZnM9hU1ERe4nK` |
| `act_proposal` VoteApprove | receipt failures 0 | `Gd6WPZusSK958K6E6BRtzSUiRabgTnsvCnQN7wtb4mJw` |

The queued change takes effect at `effective_sec` 1794469069. The log is in [vault2-admit-model.log](vault2-admit-model.log).
The allowlist stays `["manual-v1"]` until `mandate-apply` runs after that time.

## Model admission applied on vault2, 12 November 2026

The queued change became effective at 07:37:49 UTC on 12 November (`effective_sec` 1794469069), 24 hours
after the DAO vote queued it. The script that applied it first read the vault and would have sent nothing
before that time. At 07:38:59 UTC it sent one transaction, `apply_pending_mandate`. Anyone may call it once
the delay has passed. Log: [vault2-mandate-apply.log](vault2-mandate-apply.log).

| Step | Result | Hash / evidence |
|---|---|---|
| Read before: `get_mandate`, `get_pending_mandate` | model not in the allowlist; change queued and due | log line `STATE` |
| `apply_pending_mandate` (`mandate-apply`) | receipt failures 0 | `DzUFptM81MGMUBzaCpFWv4UiEjQXwu147LPJP8SK2fF9` |
| Read after: `get_mandate`, `get_pending_mandate` | `gemini-3.1-flash-lite` in the allowlist; nothing queued | log line `VERIFY` |
| Readback, no transaction | allowlist `["manual-v1", "gemini-3.1-flash-lite"]`, cap 50 USD, strikes 2, ledgers unchanged | [readback-2026-11-12.txt](readback-2026-11-12.txt) |

The run then made a dry run of the model path (`mandate-agent --model --dry-run`). It reads the final-block
snapshot, sends it to the model and submits nothing. The model returned no trade, with this reason:
"Oracle price data is currently unavailable for both assets, preventing safe trade execution." That is the
expected answer. The last prices were written on 11 November, older than the 60-second limit, so the snapshot
had no usable price. A model trade needs fresh prices written first, in the same run.

What this shows: a loosened mandate field took effect only after the full 24-hour delay, through the DAO
and a permissionless apply. The admitted model can now be named in a proposal. What it does not show: a
trade decided by the model. That is a separate step and needs its own approval.

The transaction was looked up on three RPC endpoints (`rpc.testnet.near.org`, `archival-rpc.testnet.near.org`
and `test.rpc.fastnear.com`). All three returned `UNKNOWN_TRANSACTION`, and the nearblocks API returned HTTP
410. The state change is visible in the vault's own views, read in the log and in the readback above. Use
`python3 -B scripts/demo-readback.py` to read it yourself.

## Settled trade on vault2 with a matched counterparty, 11 November 2026

This is the first trade in this folder whose ledgers changed on both sides. The vault's signed intent
(proposal 3) and a matching intent from a counterparty we control were submitted together to the Intents
verifier in one `execute_intents` call. Its log is in [vault2-settlement.log](vault2-settlement.log).

The counterparty is `solver.mandate-t1.testnet`. Its key was generated locally, kept outside the repository,
and registered on the verifier. Setup logs: [solver-setup.log](solver-setup.log), [solver-fund.log](solver-fund.log),
[solver-deposit.log](solver-deposit.log), and [solver-deposit2.log](solver-deposit2.log). The first two
deposit attempts did not succeed. They are kept as a record.

| Step | Result | Hash / evidence |
|---|---|---|
| `set_report` wNEAR at $3.60 (quote 8 s old) | receipt failures 0 | `BWoaDazWtSbyJfpaf7pyVNzcw3jSDDMWAjkf3NFqp8G7` |
| `set_report` USDC at $0.999472 (quote 16 s old) | receipt failures 0 | `H5QEezqw8TpgZCEmGYrh2rFSSkfH16MJHf6YupF5mxXA` |
| `propose` by the agent: 2 USDC for at least 0.55 wNEAR, `manual-v1` | proposal 3 | `5ptybytM9ikj2ibkM4J1yYxvP8ZoaPdZLRGVBdZEDKVu` |
| `execute` | outcome `Executed`, proposal 3 | `4a6qSdVNhitAEVttjUdDUHHrPLSwMJFzEazwvdFrYY4R` |
| `execute_intents` with both signed intents | receipt failures 0 | `B8nYgKKYcNCrYEuG9QZ92KUJe3vj641ZhvZTzy9VzXo5` |

Verifier ledgers, before and after the settlement:

| Account | Token | Before | After | Change |
|---|---|---|---|---|
| vault2 | USDC | 9.000000 | 7.000000 | −2 USDC |
| vault2 | wNEAR | 0 | 0.55 | +0.55 wNEAR |
| solver | USDC | 0 | 2.000000 | +2 USDC |
| solver | wNEAR | 0.99 | 0.44 | −0.55 wNEAR |

The two sides match exactly. After the settlement, `get_pending_proposal` is null, the last outcome is
`Executed`, and strikes are 0.

What this shows: the vault signed a trade, a counterparty took the other side, and both ledgers changed by
the amounts the vault's mandate allowed. What it does not show: a real counterparty or solver. The solver is
an account we control. The prices are operator-fed testnet values, and the verifier is the fixture build,
as described above.

## Refused trade on vault2, 11 November 2026

Proposal 4 asked for 60 USDC, above the 50 USDC per-trade cap. The vault did not refuse it on the cap. It
refused it on balance, because the vault held only 7 USDC. The engine checks the balance before the cap.
Log: [vault2-overcap.log](vault2-overcap.log).

| Step | Result | Hash / evidence |
|---|---|---|
| `set_report` wNEAR at $3.61 | receipt failures 0 | `HVq5STkPKYyWy1jS2AZSyQgP45SakQnGvLoB449gWiTd` |
| `set_report` USDC at $0.999479 | receipt failures 0 | `HND6fizCzDaQ47tRLNsMToX2pjh4Z3YQtkbD1Byenafm` |
| `propose` by the agent: 60 USDC for at least 16.5 wNEAR | proposal 4 | `ALsNp7NuLAA5cJWdRxSYVsbnPNinqrBMrCsmtytG7mYN` |
| `execute` | outcome `Rejected`: `InsufficientBalance` | `GoUU2JWXjPSWdbf5tZU2F8ypFQvxrYkPBGnuWSSNyXbc` |

The rejection counts as one strike (`get_strikes` = 1). The next approved trade resets it to zero. Three strikes
halt new proposals. The proposal is cleared, and nothing moved.

This is not yet a demonstration of the cap. A cap refusal needs the vault to hold at least 60 USDC, so the cap
check is what refuses. Funding that is a separate step.

## Cap refusal on vault2 with enough balance, 11 November 2026

To make the per-trade cap the check that refuses, the vault was first given 60 USDC more on the verifier. The
vault then held 67 USDC. Log: [vault2-cap.log](vault2-cap.log).

| Step | Result | Hash / evidence |
|---|---|---|
| `mint` 60 USDC to the operator | receipt failures 0 | `3XMkRBUE71rwnbCJrkHK6XeV4Lg9dQzS3vRkygEVvwcR` |
| `ft_transfer_call` to the verifier for vault2 | receipt failures 0 | `8HxEpoX6ADRr7bkZHKiNw6op2vR5PAukxR3UZEwzXvih` |
| `set_report` wNEAR at $3.62 | receipt failures 0 | `5YRs2UHEK15TrPk4WMEmHa84pvnd1UdaXs9knEEifSNe` |
| `set_report` USDC at $0.999474 | receipt failures 0 | `4sGusYnLeuG7tgNMGEvZhqvrQt3WEC75rKQybL18i4TW` |
| `propose` by the agent: 60 USDC for at least 16.5 wNEAR | proposal 5 | `Fgv7ad46RhNGddAJsVoC4s6S3Qk9teAWPxzsBKiLvp6X` |
| `execute` | outcome `Rejected`: `TradeTooLarge`, notional 59,968,440 micro-USD above the 50,000,000 cap | `J4RvGERYiuFv2hfninTCFWxqjw4HikG6rXH7VRSsDjBM` |

This is the cap refusal: the balance check passed, and the per-trade cap refused the trade. Nothing moved.
After it, `get_strikes` is 2, and there is no pending proposal. A third strike halts new proposals. The
next approved trade resets the count to zero, so the next trade should be a successful one.
