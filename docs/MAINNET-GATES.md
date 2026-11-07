# Mandate: mainnet gates checklist

**Current work is testnet only. Mainnet decision: NO-GO. Every evidence gate
below is PENDING.** This document authorizes no mainnet inspection, deployment,
funding, permission change, signing, transaction or custody operation. It
contains no executable actions, selected account IDs, approved budgets or
invented results.

See also [testnet decisions](design/testnet-decisions.md). Evaluate this planning checklist against the owner-approved source, configuration
and exact artifact before any later approval. It establishes no current deployed
identity or CI result. Changed source, external binaries or artifacts reopen the
affected gates.

## Required evidence gates

| Gate | Evidence and decision required to close it | Status |
|---|---|---|
| 0. Freeze the reviewed scope | Exact source revision, configuration, build inputs and public evidence manifest. Reconcile candidate changes with the reviewed main revision. Name the reviewer responsible for each gate. A merge, old test count or screenshot does not imply approval. | PENDING |
| 1. Finish the actual testnet core flow | Approved fresh testnet vault, distinct recoverable owner and correct testnet external contracts. Complete derived-authority registration, actual vault signing, a small funded matched settlement and replay refusal. Preserve final transaction/receipt outcomes and both parties' ledger changes. Local mocks, a signature, relay acknowledgement or simulation do not close this gate. | PARTIAL: the [7 Nov 2026 testnet run](evidence/testnet-run-2026-11-07/README.md) deployed a fresh keyless vault with a DAO owner, registered the key and obtained a verified testnet MPC signature. It used a fixture Intents contract and a stand-in oracle, and no matched settlement was run. |
| 2. Prove testnet delay and SR-1/SR-6 recovery | Observe a genuinely elapsed 86,400-second withdrawal queue, early refusal, mature dispatch and terminal receiver delivery or refund completion for the exact amount. Exercise failed, timed-out and late signing receipts with correct quota, execution identity and lock boundaries. Exercise withdrawal cancellation/recovery, ambiguous callbacks and authenticated voucher proof with no duplicate debit. Patched local time is not elapsed-chain evidence; an initial debit does not prove terminal delivery. | PARTIAL: one withdrawal is queued (maturity 2026-11-08 17:45:40 UTC) and an early apply was refused. Mature dispatch, receiver delivery and the failure and recovery cases are not yet shown. |
| 3. Select an oracle from verified current sources | Oracle is NOT SELECTED. Pyth Core is recorded as unsupported on NEAR; verify dated official support evidence rather than inferring availability from legacy source or mocks. Establish a supported mainnet receiver/account, reviewed code and administrators, every required USD feed and asset/decimal mapping, price/confidence schema, reliable funded updater, freshness under load and outage behavior. A null cache or generic documentation redirect establishes no usable feed. | PENDING |
| 4. Authenticate mainnet signer and Intents | Exact mainnet account/code hashes, source/artifact correspondence, administrative and upgrade powers, configuration, availability and applicable audit coverage. Verify the exact request/response scheme, domain, derivation path, payload bytes/size, deposits and gas, plus Intents registration, verifier, nonce and settlement semantics for this vault. Historical or testnet compatibility does not authenticate mainnet code. | PENDING |
| 5. Prove same-chain binding | Every vault, owner, signer, Intents and oracle call must resolve on the intended chain. NEAR cross-contract promises cannot call a testnet signer from a mainnet vault, or a mainnet signer from a testnet vault. Account state is network-specific; an account suffix or RPC URL does not route a promise to another network. Authenticate chain identity and all deployed targets; refuse mixed-network configuration. | PENDING |
| 6. Establish separate DAO/multisig ownership | Exact owner account distinct from the vault, reviewed multisig threshold, independent controllers, membership and execution/cancellation/pausing/delayed-control policy. Controllers use the approved hardware custody policy. Demonstrate loss/recovery procedures in a controlled rehearsal. Review permissions and upgrade/admin roles that can bypass governance. A DAO label does not establish recoverable multisig control. | PENDING |
| 7. Complete the external professional audit | Mandate is NOT independently professionally audited. Obtain an external audit of the exact final contract, source admission/migration, SR-1/SR-6, account-authority assumptions and external integration boundary. Close every finding with remediation and retest evidence before the audit gate closes. AI review and passing local tests do not satisfy this gate. | PENDING |
| 8. Re-review F26 for real funds | Prototype acceptance does not carry over. Independently assess guard retention after an ambiguous dispatched withdrawal, funds potentially already moved, blocked future authorization, refund/resolver overlap and safe recovery. Record the audited real-fund resolution. No timer, larger balance, blind resend or owner override may be invented to clear an unresolved transfer. | PENDING |
| 9. Prove fresh sealed-vault custody premises | Review and rehearse atomic creation/deployment/initialization with the distinct recoverable DAO owner and a vault name within the recovery-account naming limit. Establish no bypass-capable vault permission, absent sent/cancelled sinks without signing/proxy authority or a later creation route, no prior future-voucher issuance and no unresolved mint/burn/resolver/refund history. Current supply and permission counts do not prove lifetime uniqueness. Unknown old history or incompatible migration cannot be accepted silently. | PENDING |
| 10. Bind final artifact and gas evidence | Reproducible build and independently checked exact WASM hash; establish that deployment uses those bytes. Complete serial integration and gas/storage measurements for the final selected adapter and worst approved configuration: held/traded feeds, source-list limits, callbacks, registration and withdrawal/recovery. Establish adequate prepaid callback gas, storage and operating reserves. A historical summary or different artifact does not close this gate. | PENDING |
| 11. Choose economic caps and declared-source policy | Approve the asset universe, per-trade/daily authorization caps, position, slippage, drawdown, cooldown and price-age policy in correct units. Declared model IDs do not authenticate model execution. Choose admitted sources and caps on that basis, or disable the relevant model proposals. Optional pinned-signature verification proves neither TEE/model origin nor profitable decisions. | PENDING |
| 12. Establish monitoring and incident operations | Named operators and a tested runbook for oracle freshness/updater outages, agent and keeper health, proposal/signing deadlines, quota, unresolved withdrawal guards, relay failure and unfilled intents. Define alerts, response ownership, owner pause, public evidence retention and recovery escalation. Monitoring must not bypass contract checks or resend unknown withdrawals. | PENDING |
| 13. Decide and enforce the initial withdrawal ceiling | Approve the ceiling's amount, units, assets, per-request/aggregate scope, escalation process and enforcement mechanism. **The current vault does not implement a maximum withdrawal cap.** Any proposed ceiling is a DAO governance/operational control, not a claimed contract feature. Demonstrate the chosen enforcement and identify bypass/residual risks. Do not describe an alert or operator convention as on-chain prevention. | PENDING |
| 14. Approve the owner budget and first-trade cap | Explicit owner approval of NEAR storage/operating budget, maximum custody exposure, first-trade amount/cap, assets/base units, minimum output and settlement identities. Review the concrete deployment/funding/trade plan after preceding gates close. This draft supplies no default numeric budget, withdrawal ceiling, treasury amount or implied execution approval. | PENDING |

## Closure and approval checks

- [ ] Every gate has required public evidence and an accountable reviewer.
- [ ] Final source, configuration and exact artifact match that evidence.
- [ ] All external audit findings are closed and F26 has a real-fund resolution.
- [ ] The withdrawal ceiling's governance/operational enforcement and residual
  risks are explicit; no maximum-cap contract guarantee is claimed.
- [ ] The concrete action, budget and first-trade cap have separate approval.
- [ ] Changed code, policy, oracle, external binary/admin role or artifact
  reopens affected gates before another approval is considered.

All boxes remain unchecked. Gates 1 and 2 are only partly evidenced; every other gate is PENDING. Finished testnet work or a CI pass does not
automatically authorize mainnet work. Any future read-only mainnet evidence
gathering requires its scope and authorization to be settled separately; this
document performs and approves none of it.
