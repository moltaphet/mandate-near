# Security

## Status

Mandate has **not had a third-party audit**. The review below is one
self-review, run against the `near-contract-audit` checklist in
`.claude/skills/near-contract-audit`. Treat the contract as experimental until
an audit is done. Do not deposit funds you cannot afford to lose.

## Reporting a vulnerability

Do not open a public issue for a security problem.

- Once the repository is public, use GitHub's private vulnerability reporting
  on this repo (Security tab, "Report a vulnerability").
- Until then, contact the maintainer through their GitHub profile.

We aim to acknowledge a report within 3 days and to publish a fix or a
mitigation before any public disclosure.

## Scope

In scope: `contracts/vault` on the `main` branch, the keeper (`keeper/`) and
the script agent (`agent/`) as far as they can cause a wrong on-chain decision.

Out of scope: `intents.near`, `v1.signer`, `price-oracle.near` and NEAR
consensus. These are external and trusted as described in
[THREAT_MODEL.md](THREAT_MODEL.md#actors-and-trust).

## Self-review findings

Each finding has an ID so the threat model and the code can refer to it.

| ID | Severity | Where | Finding | Status |
|---|---|---|---|---|
| SR-1 | Medium (availability) | `src/lib.rs`, `execute` and its callbacks | The in-flight lock has no recovery path. If a callback ends without reaching `failed()` or `on_signed` (a panic, or running out of gas), `execute` refuses every later call. With no withdraw path (SR-6), funds stay where they are. | **Open.** A recovery function is not safe to add on its own: a late callback from the old chain could then sign a second intent for the same proposal. The fix is an execution id, checked by each callback, and it needs its own review. |
| SR-2 | Low | `src/lib.rs`, `propose` | `propose` did not check the in-flight lock. A proposal made while an execution was in flight could be dropped when that execution finished, because `on_signed` and `reject` clear `pending`. | **Fixed.** `propose` refuses while an execution is in flight. Unit test `a_proposal_waits_for_the_execution_in_flight`. |
| SR-3 | Medium (liveness, not verified) | `src/lib.rs`, `on_price_data` | The execute chain reads one price feed per held or traded asset, and the mandate allows up to 32 assets. The gas needed at that size has not been measured. If it is more than one transaction can carry, `execute` fails inside a callback, and SR-1 applies. | **Open.** Measure with 16 and 32 feeds on the sandbox. If needed, lower `MAX_ASSETS`. |
| SR-4 | Info | `src/lib.rs`, `execute` | `execute` is permissionless, so a stranger can choose the moment a closing gate becomes a strike. Three strikes halt proposals until the owner calls `resume_proposals`. | **Accepted.** Availability only. |
| SR-5 | Info | `src/lib.rs`, `set_agent` | A later `set_agent` took effect at once. A stolen owner key could point the vault at a new agent. Trades still pass every gate. | **Fixed.** A later agent is queued for `AGENT_DELAY_SEC` (24 h), like a loosened mandate. The current agent keeps its rights meanwhile, and `cancel_pending_agent` drops the change. The first agent applies at once, since no agent can propose before it is set. Tests: `a_new_agent_waits_24_hours_on_a_real_chain` (sandbox) and the `set_agent` unit tests in `lib.rs`. A DAO or multisig owner is still recommended before real funds. |
| SR-6 | Info (blocker for real funds) | whole contract | There is no withdraw path. Owner funds can be traded, not recovered. | **Planned.** Withdraw needs its own signed intent and its own gates. |
| SR-7 | Info | `src/oracle.rs`, `src/engine.rs` | The vault trusts `price-oracle.near` for correctness. Freshness, slippage and size limits bound the damage from a wrong price. | **Accepted.** See [THREAT_MODEL.md](THREAT_MODEL.md#t6-oracle-wrong-manipulated-or-stale). |
| SR-8 | Info | `pause`, `propose` | `pause` and `propose` take no 1 yoctoNEAR, so a function-call key can call them. This is deliberate: a kill switch must work at once, and the agent's key should be a function-call key. | **Accepted by design.** |
| SR-9 | Info (deployment) | deployment procedure | The vault account must have no full-access key, and deploy and `new` must run in one transaction. | **Open.** Check on the chain before any funds are sent. |

### Checklist results (`near-contract-audit`)

| Detector | Result |
|---|---|
| `non-private-callback` | Pass. Every `on_*` callback is `#[private]`. |
| `reentrancy` | Pass. `execute` sets the in-flight lock before its first cross-contract call. The trading state changes only in the final callback. |
| `incorrect-argument-or-return-types` | Pass. Token amounts cross the JSON boundary as `U128`; ids and nonces as `U64`. Timestamps are seconds, far below 2^53. |
| `unsaved-changes` | Not applicable. The contract uses no `near_sdk::store` collections. |
| `owner-check` | Pass. `set_mandate`, `cancel_pending_mandate`, `set_agent`, `cancel_pending_agent`, `resume_proposals`, `pause` and `unpause` call `assert_owner`. `apply_pending_mandate` and `register_intents_key` are permissionless by design. |
| `yocto-attach` | Pass with two deliberate exceptions, SR-8: `pause` and `propose`. |
| `storage-collision` | Not applicable. |
| `required-initialization-macro` | Pass. `new` and `new_with_externals` are `#[init]`. |
| `gas-griefing` | Pass for loops: every loop is bounded by `MAX_ASSETS`. Gas for the execute chain is SR-3. |
| `insecure-random` | Pass. Randomness is used only to make the intent nonce unique, never for an outcome. |
| `prepaid-gas` | Partly. Callbacks use static gas. There is no check of the prepaid gas, which is SR-3. |
| `cover-storage-cost` | Pass. Storage grows only by fixed-size entries: one pending proposal, one signed intent and one queued mandate. None of these grows with user input. |
| `unsafe-math` | Pass. The release profile sets `overflow-checks = true`. |
| `float-math` | Pass. No floating-point types. |

## Before real funds

A short checklist, since the code alone does not enforce these:

1. Deploy the wasm and call `new` in one transaction.
2. Delete every full-access key on the vault account (SR-9).
3. Compare the deployed code hash with a local build of the tagged commit
   (NEP-330 metadata names the repo).
4. Check `get_externals` shows the mainnet signer, intents and oracle.
5. Make the owner a multisig or DAO, or accept the owner-key risk in writing.
6. Resolve or accept SR-1, SR-3 and SR-6.
