# Mandate — Phase A submission draft

Draft only, 7 November 2026. This text has not been submitted, published or
deployed. It prepares a local judge-facing presentation. Any later funded or
chain action is testnet only and requires separate approval. Public fresh-vault,
DAO-owner and agent identities remain **PENDING**.

Mandate is an on-chain authorization boundary for treasury trading on NEAR
Intents. Treasury balances live under the vault account in the configured
Intents ledger. An agent proposes exact assets and amounts; the vault checks
the owner's policy before requesting a contract-derived Ed25519 signature from
Chain Signatures. The agent has no owner withdrawal or treasury-signing
authority through the contract API. Trades inside the configured limits can
still be poor decisions or lose money.

The reference agent supports manual plans, deterministic two-asset rebalancing
and an optional model decision source. Model assistant-message content must
match a strict seven-field schema; the completion envelope's model must exactly
match configured `MODEL_NAME` and an admitted declared source. The vault checks
source admission again before execution. IDs are metadata, not proof that a
particular model executed. Unknown sources are refused; admission uses the
24-hour loosening delay, while removal applies immediately.

Public model records separate exact request/response wire hashes from
`SHA-256(canonical response bytes)` in `mandate-response-json-v1`. The record
binds the response and exact decision tuple. A supplied invalid or unsupported
signature blocks submission; unsigned provenance stays **UNVERIFIED**. Supported
pinned-signature verification establishes neither hardware attestation nor
authenticated model execution, chain state, gate approval or settlement.

The proposed local demonstration bypasses client preview for an oversized
proposal and requires the actual on-chain `TradeTooLarge` outcome and unchanged
balances. A permitted case shows the exact tuple and canonical proposal
commitment. A decision inside the limits demonstrates the remaining strategy
risk. **New Phase A demonstration and recording: NOT RUN.** The reported
integration pass does not create new demonstration assets or a live provider
capture; those need their own saved outputs before submission.

The intended owner is a recoverable DAO distinct from the vault. Its public
account, governance execution/cancellation and recovery operations remain
unverified. **The oracle is NOT SELECTED.** Current user steering and project
documentation say Pyth Core is unsupported on NEAR; a legacy adapter or mocked
batch test is not a supported live receiver. Independently verified current
support/removal citations remain **UNVERIFIED** in this preparation. No alternative
oracle is selected: required USD feeds, correct units, updater reliability,
freshness under load and deployed administrative/source identity must be proved.

Live signing by the fresh vault, funded matched settlement, a real elapsed
24-hour withdrawal, explicit keeper relay publication and SR-1/SR-6 failure or
late-callback recovery still require intended-testnet evidence. The historical
`mandate-r1-4715a0.testnet` spike is separate interoperability evidence, not the
current custody deployment. No mainnet-readiness or real-fund claim is made.

F26 remains an accepted hackathon availability residual. A lost callback can
leave a withdrawal already dispatched or debited. Missing or inconsistent proof
blocks new trade authorization and withdrawal dispatches with no time, balance
or owner override. A successor withdrawal may still be queued. Guard retention
starts no extra transfer, but the original withdrawal may have moved funds;
an initial debit proves no terminal receiver delivery or refund completion.

## Recorded progress and unresolved evidence

Documentation/evidence was read at
`6c72705adda0151a8515043dafb4577f1b5a3df2`. This identifies the draft's source
baseline, not a frozen, independently reviewed release artifact.

| Item | Current recorded status | What remains before a final claim |
|---|---|---|
| Local integration | Repository README and user report say the full suite passed locally on `aa32116`, including gas tests | Retain the complete serial command log, exit status, environment and exact source/WASM attribution. This preparation did not rerun it |
| PR 15 CI | User reports a passing result; local Git contains its merge at `53c806e` | Direct GitHub job/run/head verification is UNVERIFIED because access is unavailable. No verified CI URL or conclusion is claimed |
| Gas | `docs/evidence/gas.txt` records 7 November 14:03:41 UTC, exit 0, 71.80 TGas for 20 relevant feeds and 8 source IDs of 64 bytes with the last ID matched; grouped tests report 3 and 4 passes | Local stand-ins and summarized output; bind the final reviewed source/artifact and retain raw measurements. Not a mainnet fee or live-oracle measurement |
| Declared-source policy | Implemented; relevant cases are in the reported integration scope | New recorded demonstration of exact refusal, delayed admission, immediate removal and migration remains pending |
| Model provenance | Canonical record and offline verification implemented; public synthetic vectors are separate | Generated demonstration record and actual verifier output pending; live provider/model/TEE origin unverified |
| Fresh testnet deployment | Vault, separate DAO and agent public IDs pending | Reviewed artifact, atomic fresh account creation, sealed permissions and working DAO controls before custody |
| Oracle | NOT SELECTED | Supported receiver, deployed code/roles, feeds/units, updater and freshness evidence; no silent fallback |
| Live core flow | UNVERIFIED; no new live actions performed in Phase A preparation | Actual vault signing, small matched settlement, replay refusal, elapsed withdrawal and recovery receipts/ledger evidence |
| Demo and site release | Local draft; new recording/deployment not performed | Rehearse with honest labels, review public outputs, verify hosted-source correspondence and organizer requirements; publish only after approval |

Older restricted-host failures, earlier partial runs and historical
4 November gas values remain scoped history, including in Git when superseded
logs are pruned by an approved change. They must not be described as
the latest local gas result. Active evidence links must point to retained records. The
new record also does not retroactively bind every previous result to the final
release. No cumulative historical test count is advertised.

A signed intent, publication acknowledgement or relay-reported `SETTLED` status
is not independently verified final settlement. Screenshots, synthetic
signatures, patched sandbox time and local mocks cannot be presented as live
receipts or a real elapsed day.

## Evidence references for editorial review

| Repository source at `6c72705` | Relevant lines |
|---|---|
| `README.md` | 26–29: recorded integration pass; 455–461: owner/oracle decision; 517–521: scoped verification, gas and sandbox settlement |
| `docs/evidence/gas.txt` | 1–7: timestamp, exit and maximum-source cost; 16–19: separate callback-chain costs and grouped outcomes |
| `docs/evidence/gas-limit.txt` | 1–2: new record supersedes old measurements; 18–28: preserved 4 November history |
| `docs/evidence/phase0-9-2026-11-07/external-contract-public-assessment.txt` | 153–175: old Pyth links generic/unavailable, no deployed receiver/updater established |
| `docs/OPERATIONAL-READINESS.md` | 62–68: null cache observations and selection requirements; 72–74: distinct owner governance remains unverified |
| `docs/spikes/R1.md` | 7–15: explicitly historical simulation/interoperability scope |
| `SECURITY.md` | 49–51: oracle remains unselected; unsupported Pyth Core is a project documentation statement |

The local docs contain the corrected Pyth status, but this preparation found no
saved exact removal-notice URL/excerpt that independently establishes current
Pyth Core support status. Add that citation before a public claim of external
documentation verification. The prior legacy-source assessment establishes
source schema and unsuccessful address discovery, not current platform support.

## Related work and contribution

[Safe's allowance module](https://github.com/safe-global/safe-modules/blob/main/modules/allowances/README.md)
already provides delegated transfer budgets, recurring resets and replay nonces.
[Enzyme's PolicyManager](https://github.com/enzymefinance/protocol/blob/dev/contracts/release/extensions/policy-manager/PolicyManager.sol)
already enforces configured asset-management policies. These public references
are prior ideas, not deployment-safety reviews performed for this project.

Mandate's contribution combines Intents custody, pre-MPC trade authorization,
delayed policy loosening and an inspectable model decision commitment. No first,
unique, universal-injection-immunity or superior-security claim is made.
