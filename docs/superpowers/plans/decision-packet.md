# Owner decision packet

Status: 7 November 2026. This lists what is decided and what is still open. A
recommendation below is advice made under the owner's delegation. It is not a
chain action and it authorizes none.

## Decided for the testnet run

These are final for testnet. They are recorded, with the evidence behind them, in
[testnet-decisions.md](../../design/testnet-decisions.md), and the run itself is in
[testnet-run-2026-11-07](../../evidence/testnet-run-2026-11-07/README.md).

| Item | Decision |
|---|---|
| Oracle (testnet) | Self-operated Burrow-compatible stand-in (`contracts/mock-oracle`) |
| Owner | Sputnik DAO v2, one council member |
| Intents | The supplied verifier fixture on a testnet account |
| Signer | `v1.signer-prod.testnet`, domain 1 |
| F26 | Accepted for the testnet run only |
| Accounts | Sub-accounts of `mandate-t1.testnet` |

## Still open, with a recommended default

| ID | Question | Options | Recommended default |
|---|---|---|---|
| O1 | Mainnet oracle | A supported NEAR price oracle, verified by source and feeds; a self-operated oracle with a published trust model; or no mainnet launch until one is verified | Do not pick yet. Pyth Core is unsupported on NEAR. Verify a candidate against every gate in [MAINNET-GATES](../../MAINNET-GATES.md) first. |
| O2 | Mainnet owner | One-member DAO (testnet only); multisig DAO with hardware keys; another recoverable account | Multisig DAO with at least three independent controllers on hardware keys. |
| O3 | F26 recovery | See [f26-recovery.md](../../design/f26-recovery.md) | Option A (owner-initiated abandon after a long extra delay), plus a withdrawal ceiling. Implement only after review. |
| O4 | Withdrawal ceiling | A per-request and per-day limit in the mandate; governance convention only; none | A per-request and per-day limit in the mandate, loosened only through the existing 24-hour delay. The vault has no such cap today. |
| O5 | Matched settlement | Run a testnet settlement against a second funded counterparty; skip until mainnet rehearsal | Run it on testnet before any mainnet step. It needs a counterparty account and a documented quote flow. |
| O6 | Model path | Disable model proposals for launch; allow with strict caps; allow with attested execution | Disable for launch. Declared model IDs are not authenticated. |
| O7 | Audit | Choose an external auditor and a budget | Required before real funds. Use the [audit package](../../audit/AUDIT-PACKAGE.md). |
| O8 | First-trade cap and budget | Owner-set amounts | Not set. No default exists, and none may be assumed. |

## What the owner has to do

Only the choices in the table marked "Do not pick yet" or "Not set" still need an
owner input. Everything else can proceed on the recommended default once the
owner agrees, and any chain step still needs its own written approval.
