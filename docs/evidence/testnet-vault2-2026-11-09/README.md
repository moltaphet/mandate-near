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
| 1 | 9 Nov | Deploy and initialise vault2 | `7jiV95FtoscuRA77mcfM1gbjNVFHk2JKiu5LPzLjnTwz` | ok |
| 2 | 9 Nov | `set_agent` via DAO (proposal 3) | `6oq26jMU…`, `HLMZgk7X…` | ok |
| 3 | 9 Nov | `register_intents_key` | `3FhMDAZ4…` | ok |
| 4 | 9 Nov | `mint` 10 USDC, deposit to verifier | `5KkS5tbB…`, `BbsUHn9p…` | ok |
| 5 | 9 Nov | `set_report` ×2 (stand-in $4.50 / $1.00) | `BTuFayd1…`, `EXDr9xUk…` | ok |
| 6 | 9 Nov | `propose` 2 USDC for 0.45 wNEAR (proposal 1) | `7Z79APfD…` | ok |
| 7 | 9 Nov | `execute` proposal 1 | `69rmz6bK…` | Executed |
| 8 | 9 Nov | `request_withdrawal` 1 USDC via DAO (proposal 4) | `FhkaxboX…`, `BRSRDKmn…` | queued |
| 9 | 10 Nov | `set_report` ×2 (CoinGecko, live) | `2K2nvkdJ…`, `4wxCKUwm…` | ok |
| 10 | 10 Nov | `apply_pending_withdrawal` 1 USDC | `7mG76TGMSkGPPXG28fLRDKTygaGVoZUeVg5N1u8Y7wfZ` | Delivered |
| 11 | 10 Nov | `set_report` ×2 (live, $3.60) | `6PjdtyGB…`, `4WaCHUD1…` | ok |
| 12 | 10 Nov | `propose` 2 USDC for 0.55 wNEAR (proposal 2) | `Aba1XnF3…` | ok |
| 13 | 10 Nov | `execute` proposal 2 | `Ah4QhhG2…` | Executed |
| 14 | 11 Nov | DAO `set_mandate`: admit `gemini-3.1-flash-lite` (proposal 5 of the DAO) | `Fg5CEsnv…`, `CXoaBSYB…` | queued to 12 Nov 07:37 UTC; applied in row 24 |
| 15 | 11 Nov | Solver account created, key registered on verifier | `5tPucrgT…`, `Fur2kTTE…` | ok |
| 16 | 11 Nov | Solver: wrap 1 NEAR, storage, verifier storage, deposit 0.99 wNEAR | `ERHb9JY5…`, `FhBuakXh…`, `6MSP2SWy…`, `DtiVHrcd…` | ok (two earlier attempts failed, not counted) |
| 17 | 11 Nov | `set_report` ×2, `propose` 2 USDC for 0.55 wNEAR (proposal 3), `execute` | `2WjtSRLP…`, `7cNQVCod…`, `98i1jNVh…`, `GGyde4LH…` | Executed |
| 18 | 11 Nov | `execute_intents`: vault and solver intents settled | `7uTJpKFk…` | ledgers changed as listed |
| 19 | 11 Nov | `set_report` ×2, `propose` 60 USDC (proposal 4), `execute` | `5G8Mw727…`, `5Bh3w6LL…`, `CteU546z…`, `C17Yg3kq…` | Rejected: InsufficientBalance |
| 20 | 11 Nov | `mint` 60 USDC, deposit to verifier | `6qvFroaW…`, `4nFw7Tjj…` | ok |
| 21 | 11 Nov | `set_report` ×2, `propose` 60 USDC (proposal 5), `execute` | `9poFVzUm…`, `FuqRTZFZ…`, `G7j4CCrE…`, `9cdyxmaA…` | Rejected: TradeTooLarge |
| check | 11 Nov | Read-only readback, no transaction (`scripts/demo-readback.py`) | [readback-2026-11-11.txt](readback-2026-11-11.txt) | Last outcome, strikes, withdrawal and ledgers match rows 1–21 |
| 22 | 11 Nov | Solver top-up: mint 20 USDC and deposit it for the solver; wrap 0.3 NEAR (`solver-topup`) | `GAhztL12…`, `CDNUDt6G…`, `BT1s6qnS…` | ok |
| 23 | 11 Nov | Deposit the wrapped 0.3 wNEAR for the solver (`solver-topup-wnear`, 100 TGas) | `HT3er3bD…` | ok |
| 24 | 12 Nov | `apply_pending_mandate` after the delay (`mandate-apply`), 07:38 UTC | `Exen1r7y3un6fNq7xYTnvRKqoCnaNv1bqFA93XXzMvVs` | Applied: `gemini-3.1-flash-lite` admitted |
| check | 12 Nov | Read-only readback after row 24, no transaction | [readback-2026-11-12.txt](readback-2026-11-12.txt) | Allowlist has both model ids; no change queued; ledgers unchanged |
| 25 | 12 Nov | `set_report` ×2 (CoinGecko, live: NEAR $3.51, USDC $0.99948) | `EEWvren5…`, `CgFV6kML…` | ok |
| 26 | 12 Nov | `propose` by the agent, decided by `gemini-3.1-flash-lite` (proposal 6) | `8Ek1SRJW…` | proposed, 07:47:44 UTC |
| 27 | 12 Nov | `execute` proposal 6 | not sent: rejected by the RPC (`NotEnoughBalance`, see the section below) | not executed; proposal 6 expired at 07:48:44 UTC, no strike |
| check | 12 Nov | Read-only readback after row 27, no transaction | [readback-2026-11-12-after-attempt.txt](readback-2026-11-12-after-attempt.txt) | Proposal 6 still stored but expired; strikes 2; ledgers unchanged |
| 28 | 12 Nov | Operator funded with 0.55 NEAR from the agent account (the cause of row 27) | `9KfpREw4…` | ok |
| 29 | 12 Nov | `set_report` ×2, `propose` by the model (proposal 7, a $2 quote); execute skipped by the price guard | `GsBqwWxv…`, `F66W4HS2…`, `5eC7kpvA…` | expired, no strike |
| 30 | 12 Nov | `set_report` ×2 for the model's decision (CoinGecko, quote 8 s old) | `3S4eVcGk…`, `DXtBuSmf…` | ok |
| 31 | 12 Nov | `propose` by the agent: the quote `gemini-3.1-flash-lite` chose (proposal 8) | `4zycxw3c…` | proposed |
| 32 | 12 Nov | `set_report` ×2, refreshed before execute | `Bu3nXciR…`, `DFuneyv9…` | ok |
| 33 | 12 Nov | `execute` proposal 8 | `5ZMSQdRg…` | Executed |
| 34 | 12 Nov | `execute_intents`: the vault's and the counterparty's intents settled | `BYWPTGwc…` | ledgers changed as listed |
| check | 12 Nov | Read-only readback after row 34, no transaction | [readback-2026-11-12-after-model-trade.txt](readback-2026-11-12-after-model-trade.txt) | Last outcome Executed (proposal 8); strikes 0; ledgers match row 34 |
| 35 | 12 Nov | Oracle redeployed with `parent-writes-only`: only `mandate-t1.testnet` may write prices | `FCRXa3rU…` | ok; a write from the agent was refused (`7gvxw9Yy…`) |
| 36 | 12 Nov | Agent given its own key, limited to `propose` on vault2 | `E1erA7Lg…` | ok; tested with a refused `propose` (`5oXyyntq…`, model id not allowed, no state change) |
| 37 | 12 Nov | Shared operator key removed from the agent account | `GDAv2iui…` | ok; the agent account now holds only the propose-only key |

**Net state at the end:** after step 18, vault2 holds 7 USDC and 0.55 wNEAR on the verifier, and the solver holds
2 USDC and 0.44 wNEAR. Step 20 adds 60 USDC, so vault2 holds 67 USDC. Steps 19 and 21 are refused and move nothing.
The strike count is 2, no proposal is pending, and the vault is not paused.
Steps 22 and 23 top up the solver for the model-decided trade: it then holds 22 USDC and 0.74 wNEAR, and vault2
is unchanged. Two earlier submissions of the wNEAR deposit at 200 TGas were rejected by the RPC before execution
(InvalidTransaction) and moved nothing; the log is [solver-topup.log](solver-topup.log).
Step 24 applies the queued model admission. The allowlist is then `["manual-v1", "gemini-3.1-flash-lite"]`,
no mandate change is queued, and balances, strikes (2) and the other mandate fields are unchanged.
Steps 25 to 27 are the first proposal decided by the admitted model. It was proposed but not executed, so
nothing was signed and nothing moved. Proposal 6 expired without a strike; balances and strikes are unchanged.
Steps 28 to 34 are the first model-decided trade that executed and settled (proposal 8). After it, vault2 holds
67.995503 USDC and 0.259302 wNEAR, the counterparty holds 21.004497 USDC and 1.030698 wNEAR, and strikes are 0.

Abbreviations: a hash ending in `…` is shortened here. The full hash is in the section named in the table.

**Hashes.** The hashes in this README are the real transaction hashes. The `hash` values printed in the `.log`
files are not: the driver used a near-api 0.8.7 call that hashes a rebuilt object. Each printed value is mapped
to its real transaction in [TX-HASH-CORRECTIONS.md](../TX-HASH-CORRECTIONS.md).

## Build

`build.log` records the no-ABI build of the vault from source. The vault WASM is 459,584
bytes with SHA256 `a7dec54d8f5687db29b903e2f12c91e0d5eb5b3294d9487d178be37b4cbe6fe9`, the same
hash as [`wasm-main-2026-11-09`](../wasm-main-2026-11-09/measurement.txt).

## Steps and transaction hashes

| Step | Result | Hash / evidence |
|---|---|---|
| Create, fund, deploy and initialise the vault in one transaction (`new_with_externals`, no key added) | ok | `7jiV95FtoscuRA77mcfM1gbjNVFHk2JKiu5LPzLjnTwz` (`vault2-deploy.log`) |
| Status after deploy | no access keys; no agent; key not registered; ledger 0 | `vault2-status.log` |
| `set_agent` through DAO proposal 3 | ok, agent `agent.mandate-t1.testnet` | `6oq26jMUcUAy9Hp2NrLVPtSs1PouGZdg5pVoTYmi5da6` (add), `HLMZgk7XVioNkSLWsR93gDZGZJf3mdAUyxH9myTmAPJL` (approve) |
| `register_intents_key` | ok | `3FhMDAZ4aueZroB17wsayYuMpVNEzQJEEbK81n8fymfk` |
| `mint` 10 USDC to the operator | ok | `5KkS5tbB9DVUvEbJC9po8mSvRGGq41JeQSDKAkbN7ixh` |
| `ft_transfer_call` to the Intents verifier for vault2 | ok | `BbsUHn9pmtpKo4AXM2ZHuCMam4F4Mw65kfBEXrFJxrJ3` |
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
| `apply_pending_withdrawal` on vault2 | receipt failures 0; outcome `Delivered`, withdrawal id 1, 1,000,000 base units (1 USDC) | `7mG76TGMSkGPPXG28fLRDKTygaGVoZUeVg5N1u8Y7wfZ` |

After the apply, `get_pending_withdrawal` and `get_withdrawal_status` return null, and
`get_last_withdrawal_outcome` returns `Delivered`. The receiver `mandate-t1.testnet` holds
2,000,000 base units of the testnet USDC stand-in (`usdc.mandate-t1.testnet`). The balance before
the withdrawal is not recorded in this folder.

## Operator-fed prices on the testnet oracle, 10 November 2026

The oracle is the testnet stand-in `oracle.mandate-t1.testnet` (`contracts/mock-oracle`). Its
`set_report` accepted a report from any caller at the time, so these values are operator-fed testnet prices. (Since 12 November the oracle accepts reports only from its parent account: redeploy `FCRXa3rUgx9QzWcxUf4J4j6qNCqz6vGA3jot73yjayUr`; a write from the agent account was then refused on chain, `7gvxw9YygQZ2GttALAmzg7yysLTa4TANeTwhwAEbq59N`.)
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
| `set_report` for `wrap.testnet`, 360,000,000 at 8 decimals (wNEAR $3.60) | receipt failures 0 | `2K2nvkdJvbxPNCAP8KbVq7zRGuvgwURVKTDgTBFrJWeR` |
| `set_report` for `usdc`, 99,956,500 at 8 decimals (USDC $0.999565) | receipt failures 0 | `4wxCKUwmLZ1kYQEJyHHecdh9UxcrbZjVFo6U1jp5Y8mq` |

The full log is in [vault2-live-price.log](vault2-live-price.log). A read-only `get_price_data` on the
oracle returned both values. The `recency_duration_sec` the oracle reports is 600 seconds, but the vault
accepts a price only if it is 60 seconds old or less. A new `set_report` is needed before each trade.

The stand-in price of $4.50 for wNEAR, used by the earlier trade in this folder, was about 27% above the
CoinGecko price on that day. That trade therefore priced against the stand-in, not the market.

## Trade on vault2 at the live price, 10 November 2026

Written in one run, [vault2-live-trade.log](vault2-live-trade.log): prices written, then `propose`, then `execute`.

| Step | Result | Hash / evidence |
|---|---|---|
| `set_report` for `wrap.testnet` at $3.60 | receipt failures 0 | `6PjdtyGBi6HKNa58QhfbbTdgmDWN1e1xHPHXRXrEZUk6` |
| `set_report` for `usdc` at $0.999587 | receipt failures 0 | `4WaCHUD1MVbesuT6DiLVAUKa3FJZA7JGh3sbYDT5cc7H` |
| `propose` by the agent: 2 USDC for at least 0.55 wNEAR, `manual-v1` | proposal 2 | `Aba1XnF3Bw3tbgbzvi2eanNsBxTikJqfpFU82tHddum9` |
| `execute` | outcome `Executed`, proposal 2 | `Ah4QhhG2W8yTH9ZoG7XC6dVKBHTcKUvZeHDP2yzye8s4` |

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
| `add_proposal` (set_mandate, DAO proposal 5) | receipt failures 0 | `Fg5CEsnvMbooJHtUobKh2i1wsN1pyzoqtA74pfeok4Xj` |
| `act_proposal` VoteApprove | receipt failures 0 | `CXoaBSYBDqQenay3Mc2kcuZdFV1z7pJhvuUeWzXesLku` |

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
| `apply_pending_mandate` (`mandate-apply`) | receipt failures 0 | `Exen1r7y3un6fNq7xYTnvRKqoCnaNv1bqFA93XXzMvVs` |
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

When this section was first written, the printed hash could not be found on any RPC node. The cause was later
found: the driver printed a value that is not the transaction hash (a near-api 0.8.7 bug, see
[TX-HASH-CORRECTIONS.md](../TX-HASH-CORRECTIONS.md)). The table above now shows the real hash, which the archival
RPC confirms as `apply_pending_mandate` with status `SuccessValue`.

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
| `set_report` wNEAR at $3.60 (quote 8 s old) | receipt failures 0 | `2WjtSRLPU8SaNWi8iZLAq9ig5WeRwZuH9vCkZ4VfU8Vd` |
| `set_report` USDC at $0.999472 (quote 16 s old) | receipt failures 0 | `7cNQVCodjxJj6DqMsGVCBYWCoVxJj1tiAxegXkjCdAJp` |
| `propose` by the agent: 2 USDC for at least 0.55 wNEAR, `manual-v1` | proposal 3 | `98i1jNVhYQqozeJtcUdwZSHGcu8vBynrsLK3QfeZewDd` |
| `execute` | outcome `Executed`, proposal 3 | `GGyde4LHvEDMtjaEmKoMCLJhvEcYRM3RQ8MYe8Tnsc36` |
| `execute_intents` with both signed intents | receipt failures 0 | `7uTJpKFkEDfKEqz1PxzNePeXJN27LK1ank75E6TRonZE` |

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
| `set_report` wNEAR at $3.61 | receipt failures 0 | `5G8Mw727fqoMNHTzy9QZe3CfnAAJczPQgV9wX9hEvewV` |
| `set_report` USDC at $0.999479 | receipt failures 0 | `5Bh3w6LL22DnEbLYS2pWVHjctc9xVG2kvSfamxV2XBih` |
| `propose` by the agent: 60 USDC for at least 16.5 wNEAR | proposal 4 | `CteU546z8RMj3BP3FiqFTEMuEjoyN7Ro3RLjK4UFkCkQ` |
| `execute` | outcome `Rejected`: `InsufficientBalance` | `C17Yg3kqqA2jkxvzyJcYz5dRBE6HBZKCLt2MPskzF1dA` |

The rejection counts as one strike (`get_strikes` = 1). The next approved trade resets it to zero. Three strikes
halt new proposals. The proposal is cleared, and nothing moved.

This is not yet a demonstration of the cap. A cap refusal needs the vault to hold at least 60 USDC, so the cap
check is what refuses. Funding that is a separate step.

## Cap refusal on vault2 with enough balance, 11 November 2026

To make the per-trade cap the check that refuses, the vault was first given 60 USDC more on the verifier. The
vault then held 67 USDC. Log: [vault2-cap.log](vault2-cap.log).

| Step | Result | Hash / evidence |
|---|---|---|
| `mint` 60 USDC to the operator | receipt failures 0 | `6qvFroaWMhKNb61DNZQGoEEJ7ZbgrDUs5usETxBN4yHQ` |
| `ft_transfer_call` to the verifier for vault2 | receipt failures 0 | `4nFw7TjjywyR4zuiPqduDtG6AC5Mf2Kxtv86gFbNZrYf` |
| `set_report` wNEAR at $3.62 | receipt failures 0 | `9poFVzUm6ygEj4LmG8ydBA9UY27vjipWicjk13wxTysD` |
| `set_report` USDC at $0.999474 | receipt failures 0 | `FuqRTZFZHvUmWAaaS5FCX8FRVAmGnmdNSN5JMJxCaD2q` |
| `propose` by the agent: 60 USDC for at least 16.5 wNEAR | proposal 5 | `G7j4CCrE6mpEiHF8xpzUDfQdYqFb7Nq6GtYPXHGSrY8K` |
| `execute` | outcome `Rejected`: `TradeTooLarge`, notional 59,968,440 micro-USD above the 50,000,000 cap | `9cdyxmaAW6Q79SNbfayzoYBn6JFAfA362eEzjfGSKQhA` |

This is the cap refusal: the balance check passed, and the per-trade cap refused the trade. Nothing moved.
After it, `get_strikes` is 2, and there is no pending proposal. A third strike halts new proposals. The
next approved trade resets the count to zero, so the next trade should be a successful one.

## First model-decided proposal on vault2, 12 November 2026 (not executed)

After the admission applied, one model-decided trade was attempted with the owner's approval. The run was
filmed. Log: [vault2-model-trade-attempt.log](vault2-model-trade-attempt.log). The model's full response
record, with the request and response hashes, is
[model-response-proposal-6.json](model-response-proposal-6.json).

| Step | Result | Hash / evidence |
|---|---|---|
| Read-only preflight | model admitted, no pending proposal, not paused, strikes 2 | log |
| Model dry run on the old prices | no trade: "Oracle prices are unavailable for both assets" | log |
| `set_report` wNEAR at $3.51 (quote 33 s old) | receipt failures 0 | `EEWvren5Gt69R1kFheqKU4PWA7Evn89HXZJPkoN5wi6q` |
| `set_report` USDC at $0.99948 (quote 21 s old) | receipt failures 0 | `CgFV6kML8XHMqLXDB4FeErtwzaNrRKWTS977dYA4iNqR` |
| Model decision (`gemini-3.1-flash-lite`, response `C2_1ar-PDM7m7M8PsJKV2Qs`) | sell 0.142450 wNEAR for at least 49.75 USDC | [model-response-proposal-6.json](model-response-proposal-6.json) |
| Agent re-checks the plan against the vault's gates on a fresh snapshot | passed | log |
| `propose` by `agent.mandate-t1.testnet` | proposal 6, `model_id` `gemini-3.1-flash-lite`, created 07:47:44 UTC, expires 07:48:44 UTC | `8Ek1SRJWUZGdUZbG7JNE2EWsPAdBDBaF7kKUYq6hyWQ8` |
| `execute` | not sent: the RPC answered `InvalidTransaction` with no detail | log |

**The model's decision was wrong.** It valued 0.142450 wNEAR (about $0.50 at $3.51) as if it were $50, and asked
for at least 49.75 USDC. Its rationale says the vault holds "a significant amount of wrapped NEAR". In fact
the vault held 67 USDC and 0.55 wNEAR. It also misread the mandate's micro-USD units as a $500,000 limit. The
response is recorded unedited.

**Why this is still safe for the vault.** The vault's gates bound what the vault may give and the least it
may accept. They do not stop a model from asking for more than the market pays. If proposal 6 had executed,
the vault would have signed: give at most 0.142450 wNEAR, receive at least 49.75 USDC. No honest counterparty
fills that, so the worst case for the vault is no fill. The team's counterparty held 22 USDC, so the settle
step would have refused before sending.

**Why execute did not run.** The RPC rejected the `execute` transaction before it reached the chain. near-api
reported only `InvalidTransaction` with no detail. A later raw diagnostic call from the same account with the
same 300 TGas returned the full reason: `NotEnoughBalance`. The operator held 0.175 NEAR, and a 300 TGas
transaction needed 0.301 NEAR available at validation. The 0.3 NEAR wrapped for the counterparty on 11 November
had taken the account below that line. That diagnostic transaction was rejected the same way and never reached
the chain. Proposal 6 stayed stored until it expired at 07:48:44 UTC. An expired proposal
cannot be executed (`the pending proposal has expired`), counts no strike, and is replaced by the agent's
next `propose`. The agent recorded the outcome as `expired`, with settlement unverified.

What this shows: an admitted model's decision reached the chain as a proposal tagged with its model ID,
after the agent checked it against the vault's gates, and a wrong decision could not cost the vault anything.
What it does not show: a model-decided trade that executed and settled. That came later, with proposal 8 (last section).

## Model-decided trade executed and settled on vault2, 12 November 2026

After proposal 6, the model path was changed in two ways (agent code, this commit):

- **Readable input.** The prompt keeps every raw field and adds the same values in whole tokens and dollars,
  the limits in dollars, and a note on units. Proposal 6's error came from reading micro-dollars and base
  units without units.
- **Priced quotes.** Code prices a few small trades ($1 and $2 here) in both directions, at the oracle price less
  half the allowed slippage, and keeps only those that pass the vault's own gates (`check_trade`). The model
  is told to copy one of them unchanged or choose no trade. A hard check before sending refuses any plan whose
  minimum output is worth more than its input plus 2% (a margin for oracle lag): proposal 6's request, about 100 times the market,
  would now stop as no trade. Tests cover both.

The run was filmed. Log: [vault2-model-trade.log](vault2-model-trade.log). The model's full response record is
[model-response-proposal-8.json](model-response-proposal-8.json).

| Step | Result | Transaction |
|---|---|---|
| Read-only preflight | model admitted, strikes 2, not paused | log |
| `set_report` wNEAR at $3.44 and USDC at $0.999496 (quotes 8 s old) | receipt failures 0 | `3S4eVcGk7wn1m19LAg63h5EiD4pVzv6FVtqSAGXxqZpM`, `DXtBuSmfUeicoekosG3sSEgmtpPQywVHShhqYFqa9Ush` |
| Model decision (`gemini-3.1-flash-lite`, response `k6b1auqVDcXqxN8PoK2C0Ac`) | chose a priced quote: give 0.290698 wNEAR (about $1), receive at least 0.995503 USDC | [model-response-proposal-8.json](model-response-proposal-8.json) |
| `propose` by `agent.mandate-t1.testnet` | proposal 8, `model_id` `gemini-3.1-flash-lite` | `4zycxw3chugFFB6fcjWxHVqWSjU4U9efMTt9t1BANx5S` |
| `set_report` refreshed before execute (wNEAR $3.45, USDC $0.999471) | receipt failures 0 | `Bu3nXciRpH2cRNqZZRWT3FnzXQasyywG3AcnwYVaCEG7`, `DFuneyv9CSZdavQX9R9ngr8BPGxukBKjDZqVZKLtX6gM` |
| Guard, read-only | price 33 s old (vault limit 60 s); asks $0.9950 for $1.0029, vault floor $0.9929 | log |
| `execute` | outcome `Executed`, proposal 8: every gate passed and the MPC signed | `5ZMSQdRgwkMoWVFy48tetrbPPJBMX5SgLKSbpJhKmzh7` |
| `execute_intents` with both signed intents | receipt failures 0 | `BYWPTGwcw5nYkByRdTX9ReqKFvyYWxyJWiJCiCBRiASY` |

Verifier ledgers, before and after the settlement:

| Account | Token | Before | After | Change |
|---|---|---|---|---|
| vault2 | USDC | 67.000000 | 67.995503 | +0.995503 USDC |
| vault2 | wNEAR | 0.55 | 0.259302 | −0.290698 wNEAR |
| solver | USDC | 22.000000 | 21.004497 | −0.995503 USDC |
| solver | wNEAR | 0.74 | 1.030698 | +0.290698 wNEAR |

The two sides match exactly. After it, the last outcome is `Executed` for proposal 8, no proposal is pending,
and strikes are 0 ([readback-2026-11-12-after-model-trade.txt](readback-2026-11-12-after-model-trade.txt)).

**Why prices were written twice.** The vault refuses a price older than 60 seconds, and the oracle stores the
source's own timestamp. The quotes arrive already 8 to 25 seconds old, and the decision and the proposal take
about half a minute. So the run writes prices for the decision, then again right before execute. A read-only
guard then checks the price age and that the proposal's minimum output still clears the vault's slippage floor
at the new prices, and skips execute otherwise. A refused execute would have been the third strike and would
have halted proposals.

**The first attempt (proposal 7).** The model chose a $2 quote (2.000908 USDC for at least 0.575145 wNEAR). By the
time it was proposed the prices were 62 seconds old, so the guard skipped execute and the proposal expired
with no strike. Log: [vault2-model-trade-guard-stop.log](vault2-model-trade-guard-stop.log). Transactions:
`GsBqwWxvE5Sf5SGQcnpDTDNgasKu7ovS4CQDDZsAD9hX`, `F66W4HS2wmouVYzaNKEcCiqQDEYhxgva9qawejj9XAXh` (prices) and
`5eC7kpvAdAkBSJwQpVRFvRhmNEmGuUQkzEbKJau4iEhj` (propose).

**The model's rationale is recorded unedited and is not sound.** It says the vault "holds a high concentration in
USDC" and then sells wNEAR into USDC, which raises that concentration. The trade itself is one of the code's
priced quotes and passed every gate. The quotes and the vault's gates bound what the model can do; the
rationale is not evidence of good judgement.

**Funding note.** The operator account was given 0.55 NEAR from the agent account first
(`9KfpREw4mLSnRdNu7eLoTsX7uYJufs4xypGjo65Nn5qS`), so 300 TGas calls pass the balance check that stopped row 27.

What this shows: a model-decided trade on testnet, from a priced quote through the vault's on-chain gates and the
MPC signature to a matched settlement on both ledgers. What it does not show: an independent counterparty (the
solver is an account we control), a real oracle, the real Intents contract, or mainnet.
