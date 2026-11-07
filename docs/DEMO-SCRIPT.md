# Mandate — Phase A three-minute demo draft

Draft prepared on 7 November 2026. This document has not been published and is
not a recording or a completed rehearsal. Phase A prepares a local demonstration;
it authorizes no deployment, funding, live signing, provider inference, relay
publication or submission. Any later chain work is testnet only and needs the
separate approval recorded in the operation plan.

This draft reads committed public documentation/evidence at
`6c72705adda0151a8515043dafb4577f1b5a3df2`. That is its documentation baseline,
not a final reviewed release or a claim that this exact artifact passed every
check. The full local integration pass is reported for `aa32116`; the user also
reports PR 15 CI passed. Direct GitHub/CI verification is **UNVERIFIED**. No CI
run URL, job conclusion or remote revision was verified while preparing this
draft.

## Preparation and screen labels

- Keep the existing site/dashboard design. Start the local fixture with
  `python3 -B scripts/dashboard-fixture.py`, open `http://127.0.0.1:8124`, and
  select **Local test vault**. That screen uses fixed synthetic RPC data; it is
  an interface demonstration, not a deployed vault observation.
- Keep **LOCAL FIXTURE**, **LOCAL NEAR SANDBOX**, **SYNTHETIC MODEL RESPONSE**,
  and **HISTORICAL R1** visibly separate. No segment in this draft is a live
  funded vault demonstration. A sandbox owner stand-in is not a deployed DAO.
- Show the exact source revision and, when independently bound, the sandbox
  artifact hash beside actual command output. A host build or summarized gas
  record is not a Docker reproducible-build certificate.
- The intended owner is a recoverable DAO distinct from the vault. The fresh
  vault, DAO owner and agent public AccountIds are **PENDING**. Do not replace
  them with the historical R1 account or example mainnet names.
- The oracle is **NOT SELECTED**. Current project documentation and user
  steering exclude Pyth Core as a supported current NEAR choice. Existing
  legacy adapter/source and local Pyth mocks establish no supported receiver
  deployment. Independent verification of the current support/removal citation
  remains **UNVERIFIED**; the PriceOracle alternative is also not selected.
- Show only explicit public outputs. Keep authentication, account signing
  material and prohibited files out of the recording. A synthetic signature
  must retain its label even when it verifies against a public pin.
- A new demonstration segment remains **NOT RUN** until its exact command,
  result and public artifacts exist. The reported integration pass must not be
  used to invent a new recording, model capture or verifier output.

## Timed local walkthrough

| Time | Screen and action | Narration and required evidence |
|---|---|---|
| 00:00–00:25 | Architecture, environment and custody boundary | “The agent proposes an exact trade. The vault checks the owner's policy before requesting a contract-derived Chain Signatures signature. Balances live in the configured Intents ledger.” Identify the screen as a local fixture or sandbox. The real DAO and three public IDs remain pending. |
| 00:25–00:50 | Sandbox policy and controls | Show assets/decimals, trade and daily authorization limits, slippage, position, drawdown, timestamps and declared source IDs. “A source label is admission metadata; it does not prove model origin. The contract remains authoritative if local preview is bypassed.” Show observed values, not fabricated live prices. |
| 00:50–01:30 | Hostile oversized proposal | In the local adversarial scenario, show exact $60 input against a $50 cap, including the captured public rationale. Deliberately bypass the client preview. Require exact `TradeTooLarge`, the relevant proposal/execution outcome, no new signed result or signer dispatch, and unchanged ledger balances. Check the strike/lock assertions actually present in that case before narrating them. **New Phase A demonstration: NOT RUN.** |
| 01:30–02:05 | Permitted proposal and public commitment | Show the exact asset IDs, base-unit amounts, public mock response, canonical version, wire hashes and proposal `reason_hash`. Run the offline verifier only on a generated ModelRecord and compare the exact proposal tuple independently. State its actual `UNVERIFIED` or `VERIFIED_PINNED_SIGNER` result. A fake model response is not a live provider run. **New Phase A demonstration: NOT RUN.** |
| 02:05–02:30 | Decision inside the configured limits | Show which bounds permit the proposed decision, even when a human regards it as poor. “Mandate constrains authority; it does not certify strategy quality or predict profits.” Do not invent a realized loss, settlement or fill. **New Phase A demonstration: NOT RUN.** |
| 02:30–03:00 | Recorded checks, gas and remaining boundary | Show the named local evidence. Local integration is reported passed on `aa32116`; direct PR 15 CI verification is UNVERIFIED. The 7 November gas record reports 71.80 TGas for the 20-feed, maximum-source case. Explain that this uses sandbox stand-ins and is not a mainnet fee or a release certificate. End with oracle NOT SELECTED, DAO/IDs pending, live core flow unverified and the F26 residual. |

The declared-source segment should show the exact refused ID and its unchanged
proposal state, then the 24-hour delayed admission and immediate removal
behavior. Do not claim a real elapsed day from patched sandbox timestamps.
The relevant sandbox cases are in the configured integration scope, which is
reported as passed on `aa32116`; a newly rehearsed presentation still needs its
own saved outputs. An agent can lie about its declared source.

## Offline provenance segment

After a generated public ModelRecord and separately observed proposal hash
exist, replace these uppercase placeholders:

```sh
cargo run -p mandate-agent -- --verify-model PUBLIC_RECORD.json --reason-hash OBSERVED_REASON_HASH
```

For a stored signature, add `--signing-address TRUSTED_PUBLIC_SIGNER_ADDRESS`.
An additional proof can use `--signature PUBLIC_PROOF.json`; compare a public
plan using `--plan PUBLIC_PLAN.json`. The aggregate public recovery-vector
fixture is not itself a generated ModelRecord.

The model's assistant message content uses exactly seven fields: `asset_in`,
`asset_out`, `amount_in`, `min_amount_out`, `rationale`, `no_trade`, and required
string-or-null `no_trade_reason`. All remain required and valid for no-trade
responses. The completion envelope's model must exactly match configured
`MODEL_NAME`, and that declared source must be allowed.

The offline verifier proves no authenticated chain snapshot, proposal inclusion,
gate approval, model execution, TEE identity or settlement. A matching public
pin authenticates the supported signature statement, not those other claims.

## Evidence to display without enlarging its scope

| Source at the documentation baseline | Recorded fact | Limitation |
|---|---|---|
| `README.md:26–29` and `README.md:517` | Local full integration is recorded as passed on `aa32116`, including gas tests | Complete serial output and final source/WASM binding still need to accompany the final release; this draft did not rerun it |
| User report; local merge metadata for `53c806e` | PR 15 was merged locally; user reports its CI passed | Merge metadata does not prove CI. Direct current GitHub verification is UNVERIFIED; no run URL is supplied |
| `docs/evidence/gas.txt:1–7` and `:18–19` | 7 November, 14:03:41 UTC, exit 0; 71.80 TGas maximum-source case; grouped 3- and 4-case passes | Summary, local sandbox, stand-in signer/oracle; not current external gas or a final artifact certificate |
| `docs/evidence/phase0-9-2026-11-07/final-fast-outside-sandbox-after-model-name-fix.txt:647` | Saved fast output ends `All checks passed.` | That log lacks embedded revision/time metadata; old restricted-host failures remain historical records |
| `docs/spikes/R1.md:7–15` | Historical separate spike obtained a testnet MPC signature accepted by mainnet simulation | Not a current vault, named-vault registration or funded settlement |

The 25.11/32.56 TGas entries in the gas record are a separate local Pyth batch
callback test. Do not present them as a selected supported live oracle. The old
4 November numbers and the earlier approval/loopback caveats remain history;
they do not override the newer successful local gas record.

## Limits to state aloud

“Our oracle is not selected and our fresh testnet vault, DAO and agent identities
are pending. We have local tests and scoped recorded measurements. We have not
established the current vault's live signing, funded matched settlement, actual
elapsed 24-hour withdrawal or intended-chain failure/recovery behavior.”

F26 is an accepted hackathon availability residual. Missing or inconsistent
withdrawal proof retains the guard, blocking new trade authorization and
withdrawal dispatches. A successor withdrawal can still be queued. The original
withdrawal may already have moved funds. No elapsed-time, balance or owner
override exists, and a debit observation does not establish terminal delivery
or exclude a later refund.

Do not claim mainnet readiness, universal prompt-injection immunity, profitable
strategy, private inference or verified TEE/model execution. A signed intent and
relay acknowledgement remain separate from final receipt/ledger settlement.
