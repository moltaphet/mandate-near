# Mandate controlled testnet run implementation plan

> **For agentic workers:** PLAN ONLY. No account creation, deployment, funding,
> signing, transaction submission, publication, commit, push or deletion is authorized.
> Execute no step below until its separate approval and prerequisites are satisfied.

**Goal:** Prepare one reproducible gated-vault testnet run, including signing,
settlement, one genuinely delayed withdrawal, agent/keeper operation and SR-1 recovery.

**Architecture:** Create a fresh vault atomically without adding account access
authority. Its distinct DAO/multisig owner governs changes and withdrawals; the
agent proposes trades and the permissionless keeper executes them. Select Pyth
and explicitly reviewed testnet external contracts at initialization.

**Tech stack:** Existing Rust workspace, near-sdk 5.30, cargo-near 0.22,
near-api 0.8.7 and near-sandbox 0.4.0. The standalone `near` CLI is not installed
in the inspected environment; its creation/transaction syntax is UNVERIFIED.

## Global constraints and public inputs

The user's latest instruction explicitly requires placeholders and overrides
the planning skill's usual no-placeholder and frequent-commit suggestions.

```text
VAULT_ACCOUNT_ID = <pending>
OWNER_ACCOUNT_ID = <pending>
AGENT_ACCOUNT_ID = <pending>
```

The fresh-vault choice is confirmed. `mandate-r1-4715a0.testnet` remains a
historical MPC-spike reference. No credential file supplies any of these identities.
Do not inspect, enumerate or use credential files to resolve an AccountId.

Additional public operation inputs remain pending: approved existing parent/
sponsor account, DAO implementation/factory and governance policy, permissionless
operator account, testnet Intents account, Pyth receiver, asset/feed mapping,
withdrawal asset/amount/receiver, and separate funding/gas budgets. AccountIds
must come from the user; external deployment identity must be independently checked.

Every mutation below is **NEEDS APPROVAL / NOT RUN**. A placeholder is not a
valid AccountId, amount, policy or executable command. No default real account,
factory, funding amount or signer credential is selected. Never put a signing
value in a command line, public file or output; approved runners read it only
from their process environment. Never run a command that prints signing material.

### Files and responsibility

- This file: operation sequence, transaction blueprints and approval gates.
- `docs/TESTNET-RUNBOOK.md`: external-source evidence and deployment limitations.
- `docs/evidence/phase0-9-2026-11-07/`: current local checks and public inspection.
- Existing `contracts/vault/src/lib.rs`: authoritative public methods.
- Existing `contracts/vault/tests/sealed_deployment.rs`: sandbox atomic-action example.
- Existing `agent/src/main.rs` and `keeper/src/main.rs`: actual runner interfaces.
- No deployment driver, public policy/config file or signing artifact is created now.

## Task 1 — Review and freeze the operation

- [ ] Receive the three public AccountIds above and the approved owner implementation.
- [ ] Finish local verification in an environment permitting loopback fixtures.
- [ ] Record the reviewed source checkpoint, full output and exact artifact SHA256.

Local commands; they do not send a live transaction:

```sh
./scripts/verify.sh
./scripts/verify.sh --integration
```

The second command deploys only into a local sandbox. Neither passing compilation
nor a partial script run proves runtime success. The current integration command
stops on host loopback restrictions; its new model/gas cases remain NOT RUN.

The inspected host build is 391671 bytes, SHA256
`5266d033e30c6311d10f92d494b1cb456471daace03124342d24ededbff70f6f`.
It is an uncommitted host build, not a Docker reproducible-build proof or an
approved deployment artifact. Rebuild and pin the approved source before deployment.

## Task 2 — Create owner, agent and fresh vault accounts

**NEEDS APPROVAL / NOT RUN — all three creation operations.**

Executable creation commands are **BLOCKED**, not invented. No DAO factory/
schema is selected, no three AccountIds are supplied, and standalone NEAR CLI
syntax was unavailable for verification. The following transaction blueprints
define what the eventual approved creation command must do. Before execution,
review the exact tool/driver command and emitted actions against these blueprints.
Do not use a development-account helper merely because it is installed: its
automatic permission additions have not been established as suitable for this vault.

### Owner creation command — blocked on reviewed governance

```text
NEEDS APPROVAL
Sender: approved existing wallet account
Receiver: <pending: inspected DAO/multisig factory>
Method: <pending: verified factory creation method>
Arguments: <pending: approved factory-schema object, public members and threshold>
Result: OWNER_ACCOUNT_ID = <pending>
Deposit and gas: <pending: verified requirements and approved budget>
```

If the selected DAO already exists, creation is skipped after verifying that
account. Do not create a personal wallet and label it a DAO. Establish governed
function calls, one-yocto deposits, vote/execute/cancel/recovery, administrative
permissions and upgrade policy. Owner calls must originate from the DAO contract;
a member signing directly as their wallet is not owner predecessor authorization.

### Agent creation command — blocked on identity and permission review

```text
NEEDS APPROVAL
Sender: approved existing parent/sponsor
Receiver: AGENT_ACCOUNT_ID = <pending>, in an authorized creation namespace
Actions: CreateAccount; approved operating-balance Transfer;
         provision the reviewed proposal-only agent access permission
Permission receiver: VAULT_ACCOUNT_ID = <pending>
Permitted contract method: propose
Allowance and secure provisioning route: <pending: reviewed and approved>
```

No key generation/import command is provided that would print or save signing
material. Secure provisioning is a separate approved operator step. The agent
is not the owner and receives no vault-account authority.

### Fresh vault creation/deployment command — one atomic action batch

```text
NEEDS APPROVAL
Sender: <pending: approved existing parent or reviewed DAO execution mechanism>
Receiver: VAULT_ACCOUNT_ID = <pending>, a fresh child in that sender's namespace
1. CreateAccount
2. Transfer(<pending: approved storage and operating funding>)
3. DeployContract(<reviewed, hash-pinned WASM>)
4. FunctionCall(new_with_oracle, reviewed initialization object, approved gas, 0 deposit)
No AddKey action at any stage.
```

`sealed_deployment.rs` demonstrates the CreateAccount/Transfer/DeployContract/
FunctionCall batch in sandbox using near-api `Transaction::construct` and
`Action` types. Its six-NEAR and 30-TGas values are fixture inputs, not approved
live requirements. A DAO cannot simply substitute for a wallet signer; prove
its execution mechanism supports the entire batch or use an approved sponsoring
parent wallet while initialization names the distinct DAO owner. The latter
owner-versus-creator case still needs a focused sandbox test before live use.

Initialization interface, not a ready-to-send JSON document:

```text
new_with_oracle {
  owner: OWNER_ACCOUNT_ID,
  mandate: complete approved Mandate OBJECT,
  signer: inspected approved testnet Chain Signatures AccountId,
  intents: inspected approved testnet Intents AccountId,
  oracle: inspected approved testnet Pyth receiver AccountId,
  oracle_kind: "pyth"
}
```

The mandate requires valid asset/decimals/lowercase-64-hex USD feed mappings and
`allowed_model_ids` (at most eight, at most 64 UTF-8 bytes each). Empty denies
all proposals. Initial source choices need explicit review; later additions
wait 86400 seconds. Manual and rebalancer source IDs are `manual-v1` and
`rebalance-v1`. Declared IDs do not authenticate model origin.

After approved creation, inspect final receipts, exact artifact, distinct owner,
zero vault permissions, immutable external/provider views, absent inert sinks
and DAO controls. Do not split deploy and initialize. Do not fund custody or
start trading until those checks pass.

## Task 3 — Pyth and Chain Signatures setup

Read-only inspection command template; no signing or account mutation:

```sh
python3 -B scripts/inspect-deployment.py --network testnet --rpc https://rpc.testnet.near.org --vault '<pending>' --expected-wasm-sha256 '<pending>'
```

Official docs list `v1.signer` on mainnet and `v1.signer-prod.testnet` on testnet.
Testnet cannot cross-call the mainnet `v1.signer` account. Domain 1 is Ed25519;
the reviewed wire shape is `payload_v2.Eddsa` as hex, at most 1232 decoded bytes.
No default signer account is selected until its deployed code/config is inspected.
The Pyth receiver, fresh USD feed caches and reliable payable update process
also remain unverified; passive reads do not update caches.

**NEEDS APPROVAL / NOT RUN — exact public call blueprint:**

```text
FunctionCall receiver: VAULT_ACCOUNT_ID
method: set_agent
args: { agent: AGENT_ACCOUNT_ID }
sender/predecessor: OWNER_ACCOUNT_ID through reviewed DAO governance
deposit: exactly 1 yoctoNEAR; gas: approved measured budget

FunctionCall receiver: VAULT_ACCOUNT_ID
method: register_intents_key
args: {}
sender: approved permissionless operator; attached deposit: 0
gas: reviewed budget (sandbox test uses 200 TGas)
```

Registration triggers vault-funded cross-contract work, including a one-yocto
Intents call; it is not a free read. Record derived authority registration
without printing its values. Inspect deployed signer domain/gas requirements;
the historical MPC spike does not establish this gated vault path.

## Task 4 — Funding and one matched settlement

**NEEDS APPROVAL / NOT RUN — separate funding transaction.**

```text
FunctionCall receiver: <pending: approved NEP141 asset contract>
method: ft_transfer_call
args: {
  receiver_id: <pending: approved testnet Intents account>,
  amount: <pending: approved decimal base-unit string>,
  msg: <pending: exact reviewed Intents deposit-message encoding for VAULT_ACCOUNT_ID>
}
sender: approved funding account
deposit/storage registration/gas: verified receiver requirements and separate budget
```

Do not guess Intents deposit-message encoding or asset registration requirements.
Verify them on the selected deployed version before preparing the executable
funding command. Verify vault ledger credit and external storage funding before
trading. Fund any counterparty only after separate approval. Test liquidity
must be labelled test liquidity; a signature or relay acknowledgement is not a fill.

## Task 5 — Agent and keeper run commands

The commands below are existing interfaces but **NEEDS APPROVAL / NOT RUN**.
Their public configuration/plan files must first be reviewed and created; they
do not exist as ready-to-use operation inputs in this plan.

```sh
cargo run -p mandate-agent -- '<approved public trade-plan file>'
cargo run -p mandate-keeper
```

Manual agent mode needs public `NEAR_RPC_URL`, `VAULT_ID`, `AGENT_ACCOUNT`,
`AGENT_OUTBOX` and signing configuration supplied only through its environment.
Keeper needs corresponding public RPC/vault/account/outbox settings and
environment-only signing configuration. Neither process is started now.
Keeper loops and has no dry-run/once CLI; use a supervised, explicitly approved
run with a stop condition after the target proposal outcome. It can execute
transactions. Do not use `--publish` without a separate publication/funding approval.

An optional rebalance command, also **NEEDS APPROVAL / NOT RUN**, is:

```sh
cargo run -p mandate-agent -- --auto '<approved public strategy-config file>'
```

No automatic model-provider request is authorized. `--model --dry-run` still
calls its provider and is excluded now. Manual/rebalance rationale hashing stays
unchanged; a model proposal uses the separately versioned canonical response hash.

Evidence: exact proposal tuple/source/hash, authoritative gate result, signature
status, final receipts and both parties' ledger deltas for approved settlement.
Use `get_pending_proposal` and status views; avoid displaying signing material.

## Task 6 — Exactly one genuinely elapsed 24-hour withdrawal test

**NEEDS APPROVAL / NOT RUN — withdrawal authorization and send.**

```text
Owner-governed FunctionCall:
receiver: VAULT_ACCOUNT_ID
method: request_withdrawal
args: { asset_id: <pending>, receiver: <pending>, amount: <pending decimal string> }
deposit: exactly 1 yoctoNEAR; gas: approved measured budget

Permissionless FunctionCall after the exact queue maturity:
receiver: VAULT_ACCOUNT_ID
method: apply_pending_withdrawal
args: {}; deposit: 0; prepaid gas: at least 280 TGas
```

Record chain time, returned `effective_sec` and the exact authorized tuple.
Observe an approved early apply refusal; each attempted transaction also needs
approval and consumes gas. At chain time equal to `effective_sec`, application
may proceed only if all other guards pass. `WITHDRAW_DELAY_SEC` is 86400.
Observe real elapsed time; no sandbox maturity patch, timer edit or fast-forward
counts as live delay evidence. Run only one withdrawal authorization/test amount.
Verify final receiver and ledger deltas and retained outcome; a debit proof alone
does not prove final delivery or absence of a later refund.

If the authorized send is ambiguous, do not repeat it. `recover_withdrawal {}`
is a separate, **NEEDS APPROVAL** permissionless operation after its 300-second
window; dispatched recovery requires at least 140 TGas and authenticated voucher
proof. Safe rearming requires owner plus exactly one yocto and preserves maturity.
F26 has no time/balance/owner override.

## Task 7 — SR-1 execution recovery test

**NEEDS APPROVAL / NOT RUN — separate fault-injection trade and any retry.**

There is no public SR-1 `recover` or `reset` method. Do not invent one or use
`recover_withdrawal` for a trade execution. Arrange a reviewed testnet fault
that causes an ambiguous/late signer response without substituting an unreviewed
production signer, changing live storage or blind retransmission.

```text
approved agent FunctionCall: propose(exact reviewed tuple/hash/allowed model_id), deposit 0
approved keeper/operator FunctionCall: execute({}), deposit 0, 300 TGas
read-only monitoring: get_chain_time_sec, get_pending_proposal, is_in_flight,
                     get_trading_state; final execution/signing receipts
after authenticated expiry and separate retry approval:
approved agent FunctionCall: propose(new reviewed proposal), deposit 0
approved keeper/operator FunctionCall: execute({}), deposit 0, 300 TGas
```

Signing window ends at `started_sec + 300`: equality closes it. The execution
lock ends at `started_sec + 300 + INTENT_TTL_SEC`, currently 420 seconds;
equality releases it. A stale callback must not change newer state or publish
an expired authorization. Assert exact execution/proposal identity and no
duplicate ledger movement. An outstanding already-issued signature is not
revoked by pause. If a controlled live fault cannot be reproduced, report
SR-1 live recovery UNVERIFIED rather than simulate it in a screenshot.

## Approval and exit checklist

- [ ] All three AccountIds and public governance/external contracts confirmed.
- [ ] Full local verification and final gas output reviewed.
- [ ] Owner creation/provisioning independently approved.
- [ ] Agent creation/provisioning independently approved.
- [ ] Exact fresh atomic vault creation/deployment batch and budget approved.
- [ ] Configuration and signing registration independently approved.
- [ ] Custody/storage/counterparty funding independently approved.
- [ ] Agent/keeper trade and any publication independently approved.
- [ ] One withdrawal, early test, mature send and any recovery separately approved.
- [ ] SR-1 fault scenario and any replacement trade separately approved.

At this checkpoint every chain action is NOT RUN. No prior approval is inferred
from an AccountId, local build, sandbox result, historical credential path or
approval of a different step. No repository cleanup, commit or deletion is
part of this operation plan.
