# Mandate — submission draft

Draft only, 7 November 2026. This text has not been submitted or published.
Replace evidence placeholders after the final revision is frozen and verified.

Mandate is a NEAR vault for constrained treasury trading. Treasury balances live
in NEAR Intents under the vault account. An agent proposes exact assets and
amounts; the vault checks the owner's rules before asking Chain Signatures to
sign the Intents payload. The agent has no owner withdrawal or treasury signing
authority through the contract API. Permitted trades can still lose money.

The reference agent supports manual plans, deterministic two-asset rebalancing
and an optional model decision source. The model receives bounded public state
from a final-block snapshot and must return one strict seven-field decision.
Captured request and response wire hashes remain separate from
`SHA-256(canonical response bytes)`, using `mandate-response-json-v1`. The record
binds response identity and the exact trade tuple. Invalid or unsupported
supplied signatures refuse a trade before proposal submission; an unsigned
record remains **UNVERIFIED**. A verified pinned signer establishes neither
hardware attestation nor authenticated model origin.

The model/source allowlist demonstration is a **pending evidence claim** until
Phase3's final contract, migration and gas checks pass. It checks **declared
source IDs**, not cryptographically authenticated model execution. An agent can
mislabel its source; every resulting trade must still pass the vault's economic
gates. Manual and rebalancer paths use the reserved `manual-v1` and `rebalance-v1`
IDs. After final verification, demonstrate refusal of an undeclared ID and the
existing delayed-addition/immediate-removal policy.

Our proposed adversarial demo deliberately bypasses local preview and sends a
captured oversized model proposal through the sandbox agent. The expected vault
result is `TradeTooLarge`, with no signature or balance movement. A permitted
proposal demonstrates exact assets, amounts and canonical commitment matching
the on-chain record. A large poor decision inside the owner's limits is allowed:
the contract constrains authority rather than grading strategy quality.
**Phase4 status: NOT RUN.**

The selected owner is a DAO account distinct from the vault. Its governance and
recovery operations remain unverified. Pyth is selected conditionally on usable
fresh feeds, correct units and a reliable updater. Live vault MPC signing,
funded matched settlement, a genuinely elapsed 24-hour withdrawal and SR-1/SR-6
recovery still need intended-chain evidence. The historical
`mandate-r1-4715a0.testnet` MPC spike is not evidence of a current deployed vault.

F26 remains an accepted hackathon availability residual. A withdrawal may
already be dispatched or debited when a callback is lost. Missing or inconsistent
proof blocks new trades and withdrawal dispatches, with no time, balance or owner
override. Guard retention starts no additional transfer; the original withdrawal
may already have moved funds. Real-fund readiness is not claimed.
The owner may still queue a successor withdrawal; the guard prevents sending it.

## Evidence to fill before submission

| Item | Required final evidence | Current status |
|---|---|---|
| Source | Reviewed commit and exact built artifact hash | Pending final freeze |
| Combined verification | Complete command output, exit code, separate test/case totals | Pending; latest default baseline exited101 on loopback permissions |
| Model provenance | Public record, canonical commitment, actual verifier result | Live provider run not performed; public synthetic recovery vectors are separate |
| Declared source policy | Allowlist, delay/removal, legacy migration and gas results | Pending final Phase3 verification |
| Adversarial demo | Exact rejection, unchanged balances and allowed residual-risk case | Phase4 not run |
| Live testnet path | Approved target accounts and final transaction/ledger evidence | Unverified |
| Site | Approved production deployment, alias and browser checks | New deployment not performed |

Do not replace these placeholders with cumulative historical counts, partial
runs, synthetic signatures or screenshots presented as live receipts. The
offline verifier authenticates no chain inclusion or settlement. A signed intent
and a relay acknowledgement are separate from final settlement evidence.

## Related work and the contribution

[Safe's allowance module](https://github.com/safe-global/safe-modules/blob/main/modules/allowances/README.md)
already provides delegated transfer budgets, recurring resets and replay nonces.
[Enzyme's PolicyManager](https://github.com/enzymefinance/protocol/blob/dev/contracts/release/extensions/policy-manager/PolicyManager.sol)
already enforces configured on-chain asset-management policies. These are prior
published ideas, not systems reviewed for deployment safety in this project.
Mandate's particular contribution is combining NEAR Intents custody, pre-MPC
trade checks, delayed policy loosening and an inspectable model decision
commitment. No first, unique or superior-security claim is made.
