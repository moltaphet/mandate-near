# Mandate controlled testnet execution checklist

Draft for review, 7 November 2026. Intended repository destination: `docs/superpowers/plans/testnet-checklist.md`. This scratch draft executes nothing. Every chain mutation below is **NEEDS APPROVAL / NOT RUN**. Expected checks describe future acceptance criteria; they are not reported results.

The latest task instruction governs this checklist: **testnet only; fresh vault; DAO owner distinct from the vault; oracle NOT SELECTED**. Three public account identities, DAO implementation/factory, operation driver, and oracle/adapter source remain pending. Earlier conditional Pyth selection is superseded. Historical R1 accounts and receipts are references only.

## Execution rules and pending inputs

- Do not run commands containing `<PENDING>`. Quoted placeholders are literal arguments, not valid account IDs, amounts, paths, or approval. Never substitute an example account or discover identities through credential files.
- An exact public contract/RPC call can be specified before a CLI transport exists. Where the driver, DAO factory, or deployed ABI is unknown, the executable command remains **BLOCKED**. Review its emitted receiver/actions/arguments/deposits/gas before seeking approval; do not invent command syntax.
- Approval must name the concrete testnet target, action list, public arguments, amount/deposit, prepaid gas, total budget, operator, and stop condition. Approving documentation, a local test, an account ID, or a previous action does not approve a chain mutation. Failed transaction tests, storage registration, oracle updates, registration, rearming, relay publication, and retries also need approval.
- Keep signing/authentication material outside command arguments, public configurations, and evidence. Use the separately approved secure provisioning route. Never print environment configuration, signing values, account permission values, or credential contents. A public evidence sanitizer must report counts/booleans, IDs, sizes, hashes and outcomes rather than permission/signing values. No protected file is an input to this checklist.
- Run local verification serially. Preserve current changes; do not reset, clean, delete, commit, merge, push or publish as part of a testnet step. A source change invalidates artifact/gas evidence until the required checks are repeated.
- All public RPC observations must use approved HTTPS testnet endpoints and one final block/hash-height anchor per snapshot. RPC-reported identity and matching bytes are not independent authentication of the RPC, external governance, or source correspondence.

| Required public input | Value / decision |
|---|---|
| `VAULT_ACCOUNT_ID` | `<PENDING>`; fresh valid account, at most 62 bytes |
| `OWNER_ACCOUNT_ID` | `<PENDING>`; recoverable DAO distinct from vault and agent |
| `AGENT_ACCOUNT_ID` | `<PENDING>`; separate proposal account |
| Existing sponsor/parent and creation namespace | `<PENDING>` |
| Permissionless operator account | `<PENDING>`; separate transaction budget |
| DAO implementation, factory, immutable source/artifact, ABI and governance policy | `<PENDING>` |
| DAO execution adapter/driver source and emitted-action proof | `<PENDING>` |
| Testnet RPC, signer, Intents, asset contracts, relay/counterparty | `<PENDING>` |
| Oracle selection, deployment, adapter source, feed universe and updater | `<PENDING>`; no default oracle |
| Reviewed source revision, allowed-file manifest, artifact path/SHA256 | `<PENDING>` |
| Complete initial mandate and admitted source IDs | `<PENDING>` |
| Core trade mode, exact tuple, reason commitment and test-liquidity amount | `<PENDING>` |
| Public runner/config/evidence locations and supervised stop condition | `<PENDING>` |
| One withdrawal asset, receiver and positive base-unit amount | `<PENDING>` |
| Recovery/fault scenarios, public receipt reader and expected failure codes | `<PENDING>` |
| Per-action gas, storage, operating-balance and total testnet budgets | `<PENDING>` |
| Concrete approval record for each mutation | `<PENDING>` |

The root reported current `main` at `6c72705adda0151a8515043dafb4577f1b5a3df2` during drafting; that is background, not an approved release pin. Recheck the actual reviewed revision/artifact at execution time. GitHub CI result is not established by a workflow definition.

## 1. Resolve the operation inputs and owner model

- **Exact command/call:** none; review the pending-input table, user-supplied public account IDs, reviewed DAO/adapter documents, and concrete operation specification.
- **Required inputs:** every identity, governance/adapter/source choice, asset/feed and budget above remains `<PENDING>`.
- **Expected success checks:** all public accounts are explicit and testnet-bound; owner is a DAO, distinct from vault and agent; operators and namespaces are authorized; no example/historical identity is silently selected.
- **Approval needed:** selecting public inputs is review work. Every later provisioning, creation or governance transaction requires separate approval.
- **Rollback / stop:** stop on a missing identity, an unreviewed DAO, a self-owned vault, a namespace mismatch, or an attempted credential-based identity lookup. Return to input review; create nothing.

## 2. Freeze the reviewed source and evidence format

- **Exact local command:** `git rev-parse --verify HEAD` (read-only revision identifier); review the approved allowed-file manifest and existing diffs through the responsible reviewer. No broad file inventory or protected-content hash is required here.
- **Required inputs:** approved source checkpoint `<PENDING>`, allowed-file manifest `<PENDING>`, evidence directory `<PENDING>`, artifact build recipe `<PENDING>`, sanitizer/schema `<PENDING>`.
- **Expected success checks:** revision and actual allowed source contents are bound together; uncommitted changes are disclosed; each future log identifies UTC time, command/arguments, exit status, environment, revision, artifact digest and scenario. Synthetic, sandbox and live evidence are labelled separately.
- **Approval needed:** no chain approval for revision/evidence review. Any Git mutation remains outside this step and needs its own authorization.
- **Rollback / stop:** any source change or unknown build input returns the operation to this gate. Do not treat HEAD alone as a manifest of uncommitted source or a passing release.

## 3. Run the exact fast verification scope

- **Exact local command:** `./scripts/verify.sh`
- **Required inputs:** reviewed working tree `<PENDING>`, pinned toolchain/dependencies already available or separately approved installation, loopback-capable host `<PENDING>`, safe evidence capture `<PENDING>`.
- **Expected success checks:** exit 0 and complete terminal `All checks passed.` after formatting, strict Clippy, references, browser logic/parity, inspector, workspace library and agent protocol/model/provenance suites. Record actual test output, not an invented total.
- **Approval needed:** local verification under its authorized scope; no chain approval. Dependency installation/network changes, if needed, require their own operator review. No signer/provider configuration is needed for the fixture tests.
- **Rollback / stop:** save exact failure and exit status; stop release work on partial execution, loopback denial, stale generated reader, failed lint/test or unavailable toolchain. Do not skip failing stages or claim full integration success.

## 4. Run integration, migration and supported-size gas verification

- **Exact local command:** `./scripts/verify.sh --integration`
- **Required inputs:** same reviewed source/artifact as step 3 `<PENDING>`, sandbox/loopback-capable host `<PENDING>`, current adapter/source policy `<PENDING>`.
- **Expected success checks:** complete fast scope plus all configured sandbox targets, including registration, execution, agent/model/keeper, attacks, settlement, withdrawal lifecycle/recovery, both implemented adapters and sealed deployment. Required measurements cover 20 assets and maximum admitted-source storage/payload overhead; record actual gas and outcome. Verify the creator-versus-distinct-DAO-owner atomic deployment case before relying on it live. Five migration decoders must have appropriate actual-old-state coverage; a native golden test is not automatically a Wasm upgrade test.
- **Approval needed:** local sandbox verification, not a live chain transaction. Root owns serial test execution. Do not inspect/hash the protected common helper source.
- **Rollback / stop:** any failure, incomplete target, insufficient callback gas, ambiguous fixture identity or missing focused case blocks live deployment. A host restriction is a limitation, not a passing test. Do not refresh tracked attack/gas logs implicitly.

## 5. Establish the public operation tooling

- **Exact help-only command:** `near --help`, if an approved standalone NEAR CLI is installed. Follow only its actually returned help to verify creation, action-batch, contract-call, wallet/hardware and send/approval interfaces. The inspected host previously had no `near` executable.
- **Help-only interfaces recorded by the source plan, not rerun for this checklist:** `cargo-near near --help`; `cargo-near near deploy --help`; `cargo-near near deploy build-non-reproducible-wasm with-init-call --help`; `cargo-near near create-dev-account use-specific-account-id use-ledger --help`.
- **Required inputs:** CLI version/source/install decision `<PENDING>`, secure signing route `<PENDING>`, DAO/action driver source and exact command `<PENDING>`.
- **Expected success checks:** help establishes exact syntax without making account/provider/hardware calls; the reviewed driver emits the intended actions and never logs signing values. Existing cargo-near help proves an initialization interface and a development-account Ledger option, not a keyless DAO atomic deployment.
- **Approval needed:** help is local read-only work. Installation, hardware access, provisioning, signing and any send require separate approval. Do not start an interactive creation/deploy command while gathering help.
- **Rollback / stop:** if no suitable driver exists, creation/governance commands remain BLOCKED. Never substitute a development-account helper that adds vault access authority or invent a runnable CLI string.

## 6. Review DAO governance and exact execution capability

- **Exact call:** factory creation method, proposal/vote/execute/cancel/recovery methods, argument schemas, deposits and gas are `<PENDING:VERIFY_FROM_SELECTED_DAO_SOURCE_AND_DEPLOYED_ABI>`; no factory or function name is assumed.
- **Required inputs:** DAO implementation/factory/source/artifact/admin permissions `<PENDING>`, members/threshold/voting deadlines/recovery policy `<PENDING>`, action adapter `<PENDING>`.
- **Expected success checks:** an executed vault call has `predecessor_account_id == OWNER_ACCOUNT_ID`; a member wallet is not substituted as owner. DAO can emit exactly one-yocto sensitive calls, zero-deposit pause calls, the required gas, and complete supported action batches. Vote, execute, cancellation and recovery paths are tested locally; upgrade/admin trust and owner recoverability are explicit.
- **Approval needed:** read-only source/ABI review is not creation approval. Every DAO creation, vote, proposal, execution, cancellation, recovery and permission mutation needs concrete approval.
- **Rollback / stop:** missing execution/deposit/batch capability blocks deployment. Revise the plan before creation; do not create a personal account and label it a DAO. A governance transaction already executed cannot be undone by deleting its local file.

## 7. Select a usable oracle and compatible adapter

- **Exact read calls, conditional only:** current PriceOracle adapter uses `get_price_data({"asset_ids": <PENDING:FEED_ID_ARRAY>})` and `get_asset({"asset_id": <PENDING:FEED_ID>})`; current Pyth adapter uses `list_prices_no_older_than({"price_ids": <PENDING:FEED_ID_ARRAY>, "age": <PENDING:MANDATE_MAX_PRICE_AGE_SECONDS>})`.
- **Required inputs:** oracle selection `<PENDING>`, testnet account/code/source/admin proof `<PENDING>`, adapter source and supported kind `<PENDING>`, USD quote units/decimals/feed identities `<PENDING>`, update mechanism/SLA/funding `<PENDING>`.
- **Expected success checks:** every held/traded asset has correctly scaled usable prices under actual load; future/stale/null/malformed prices fail closed; input timestamp/aggregate/confidence semantics match the reviewed adapter. Current constructor accepts only `price_oracle` or `pyth`. A third provider requires a separately authorized adapter change, review and verification before initialization.
- **Approval needed:** choosing/reviewing a feed source is read-only planning. Deploying a receiver/adapter, posting reports, paying updates or changing code needs separate approval.
- **Rollback / stop:** oracle remains NOT SELECTED until deployment/feed/updater facts are proved. Current runbook records Pyth Core as unsupported on NEAR; historical receiver code or matching JSON is insufficient. Do not silently choose Pyth or treat any example receiver as supported. PriceOracle is not automatically usable either. Missing feed/units/updater proof blocks deployment and trading.

## 8. Prepare the selected oracle updater, if one is needed

- **Exact conditional call:** a reviewed Pyth receiver source uses `update_price_feeds({"data": <PENDING:SOURCE_VERIFIED_UPDATE_PAYLOAD>})`, not singular `update_price_feed`. This is not selected or executable. For any other oracle, method/ABI/driver is `<PENDING>`.
- **Required inputs:** selected receiver/updater source and payload verifier `<PENDING>`, update inputs and authenticity `<PENDING>`, fee/storage/gas requirement and approved payer/budget `<PENDING>`.
- **Expected success checks:** authenticated update completes through the selected verifier, cache publication times/units match feeds, freshness stays within the mandate during quote/signing latency, and alarms identify stalled updates. Passive reads do not refresh caches.
- **Approval needed:** every payable update, updater deployment/start and funding is NEEDS APPROVAL. No update or provider request runs in Phase A.
- **Rollback / stop:** do not pay or forward unreviewed payloads. Stop trading when cache freshness is inadequate; stopping an updater or pause does not revoke existing signed intents.

## 9. Verify signer, Intents and asset deployments

- **Exact read calls:** final-block `view_account`/`view_code` and source/admin correspondence review through `<PENDING:REVIEWED_SANITIZING_INSPECTION_DRIVER>`; public deployment identity/status, not permission values, is retained. No raw account permission/signing list is printed.
- **Required inputs:** signer/Intents/asset accounts `<PENDING>`, source/artifact hashes/admin roles `<PENDING>`, signer domain/config/fees/gas `<PENDING>`, Intents ledger/voucher/withdrawal behavior `<PENDING>`.
- **Expected success checks:** RPC chain identity is testnet; all required accounts have the expected code, storage and source correspondence. Official docs name `v1.signer-prod.testnet`, but that is a candidate requiring current inspection. Domain 1, `payload_v2.Eddsa`, 1232-byte decoded payload limit and actual callback shape match the deployed signer. Intents supports the exact custody, signing registration, NEP245 voucher and terminal NEP141 withdrawal methods used by this source.
- **Approval needed:** read-only inspection only; deploying externals, modifying their configuration or funding storage needs separate approval.
- **Rollback / stop:** no mainnet account/RPC may substitute; empty Intents code, missing voucher methods, wrong signer domain/gas or unknown admin trust blocks the run. Historical R1 interoperability is not this vault's live evidence.

## 10. Verify the fresh namespace and finalize the complete mandate

- **Exact local call:** `Mandate::validate()` and `simulate_mandate_change({"mandate": <PENDING:COMPLETE_POLICY_OBJECT>})` when a vault exists; the simulation is a read-only view, not an owner transaction.
- **Required inputs:** approved fresh child namespace/account `<PENDING>`, complete asset/feed/stable/source list and limits `<PENDING>`, vault name at most 62 bytes; no prior canonical voucher issuance.
- **Expected success checks:** 1–20 unique NEP141 assets; decimals 0–30; at least one stable asset; valid selected-adapter feeds; positive per-trade USD limit no greater than daily limit; slippage 1–500 bps; price age 1–300 seconds; position/drawdown 1–10000 bps; 0–8 unique nonblank source IDs of at most 64 UTF-8 bytes. USD limits are U64 decimal strings in microUSD; trade/withdrawal amounts are U128 decimal strings in base units. Pyth alone requires lowercase 64-hex feed IDs; do not impose that requirement on every adapter.
- **Approval needed:** policy review is planning. Initializing or changing policy is a chain mutation needing approval; source-ID additions after initialization require 86400 seconds.
- **Rollback / stop:** do not mutate a live policy to repair wrong feed units during an experiment. A changed adapter/policy invalidates the approval and affected evidence. Initial source selection must not silently admit every provider.

## 11. Create or verify the DAO owner

- **Exact creation call:** receiver `<PENDING:REVIEWED_FACTORY>`; method `<PENDING:VERIFIED_CREATION_METHOD>`; arguments `<PENDING:APPROVED_PUBLIC_GOVERNANCE_OBJECT>`; deposit/gas `<PENDING>`. Executable command BLOCKED until steps 1, 5 and 6 are complete.
- **Required inputs:** OWNER_ACCOUNT_ID and selected factory/source/schema/budget/secure provisioning `<PENDING>`.
- **Expected success checks:** final creation receipt returns the approved DAO identity with intended governance, admin permissions and recoverability. For an existing DAO, skip creation and verify its actual code/governance before authorizing use.
- **Approval needed:** NEEDS APPROVAL for creation/provisioning and every governance mutation.
- **Rollback / stop:** ambiguous factory outcome must be reconciled from receipts before another create request. No automatic deletion or replacement of an existing DAO; any recovery/replacement needs an approved new plan.

## 12. Create/provision the separate agent and permissionless operator

- **Exact action blueprint:** approved sponsor → `AGENT_ACCOUNT_ID`; `CreateAccount`, approved operating `Transfer`, and separately reviewed proposal-only function-call permission whose receiver is VAULT_ACCOUNT_ID and method is `propose`. Operator provisioning is a distinct approved action. Public allowance/provisioning schema and actual command `<PENDING>`.
- **Required inputs:** identities, namespace, sponsor, budgets and secure provisioning route `<PENDING>`.
- **Expected success checks:** agent is not owner and has no vault-account permission; proposal capability is limited as reviewed; operator merely pays execution costs. No signing values are printed/saved to public evidence.
- **Approval needed:** NEEDS APPROVAL for each account creation, transfer and permission/provisioning action.
- **Rollback / stop:** wrong receiver/method/namespace or overly broad authority stops provisioning. Existing on-chain permissions require separately approved revocation/recovery; never erase credential files to claim authority was removed.

## 13. Approve the exact fresh atomic vault deployment batch

- **Exact action blueprint:** approved parent/creator → fresh VAULT_ACCOUNT_ID: `CreateAccount`; `Transfer(<PENDING:APPROVED_STORAGE_AND_OPERATING_BALANCE>)`; `DeployContract(<PENDING:HASH_PINNED_WASM>)`; `FunctionCall(new_with_oracle, <PENDING:COMPLETE_INIT_OBJECT>, <PENDING:MEASURED_GAS>, 0 deposit)`. **No AddKey action at any stage.**
- **Exact initialization fields:** `owner`, `mandate`, `signer`, `intents`, `oracle`, `oracle_kind`; all actual values `<PENDING>`. `oracle_kind` must be the separately selected supported adapter, not an assumed `pyth` default. `new` hardcodes mainnet externals and is unsuitable for this explicit testnet plan.
- **Required inputs:** approved source/manifest/WASM SHA256, final namespace, DAO/creator capability, complete initialization JSON and exact action/gas/funding budget `<PENDING>`.
- **Expected success checks:** independently reviewed emitted actions match this batch; deployment and initialization cannot be split or leave an externally initializable vault. Creator may be a reviewed sponsor distinct from the stored DAO owner only after its local/runtime premise is verified. Fixture six-NEAR/30-TGas values are not approved live requirements.
- **Approval needed:** NEEDS APPROVAL for the concrete entire batch and all funds. Review the actual driver/command as the final approval item.
- **Rollback / stop:** no account permission is temporarily added as a convenience. Do not deploy a plain replacement WASM to old state. Failed/ambiguous creation must be resolved from actual final receipts before any retry; never assume a partially observed action sequence needs replay.

## 14. Execute the approved atomic batch and inspect its final state

- **Exact operation command:** `<PENDING:APPROVED_DRIVER_COMMAND_MATCHING_STEP_13>`; remains BLOCKED here. After its separately approved execution, use:

```sh
python3 -B scripts/inspect-deployment.py --network testnet --rpc '<PENDING:APPROVED_TESTNET_HTTPS_RPC>' --vault '<PENDING:VAULT_ACCOUNT_ID>' --expected-wasm-sha256 '<PENDING:APPROVED_WASM_SHA256>'
```

- **Required inputs:** concrete approval, identities/artifact/action receipt IDs, approved public RPC and evidence locations `<PENDING>`.
- **Expected success checks:** final batch success with no hidden receipt failure; exact expected artifact; distinct DAO owner; expected immutable externals/adapter; zero vault account permissions; absent recovery sinks/identities; registration initially false; agent initially unset. Inspector `live_readiness` still reads UNVERIFIED; exit 0 only means its implemented checks found no explicit failure.
- **Approval needed:** batch NEEDS APPROVAL; following public inspection is read-only, does not authorize funding.
- **Rollback / stop:** freeze use on any mismatch/failed receipt. A successfully created account is not removed automatically; do not fund custody. A keyless vault has no externally signed deployment authority to undo or upgrade it through the existing contract.

## 15. Confirm irreversible sealing and historical voucher premises

- **Exact read check:** step 14 inspector plus approved historical/namespace/action audit; verify no `AddKey`, no account-creation path, and no prior future canonical voucher issuance. No raw permission values are emitted.
- **Conditional permission mutation:** preferred fresh vault has zero permissions from creation, so **no removal transaction is needed**. If a deviation leaves bypass-capable authority, STOP and prepare a separate exact removal plan `<PENDING>`; do not silently salvage this fresh-atomic premise.
- **Required inputs:** approved final receipts and original action list, sink absence/no signing or proxy authority, creator/DAO execution capability, voucher history `<PENDING>`.
- **Expected success checks:** neither vault permissions nor a future sink creation/identity path can bypass the protocol. `s.<vault>` and `c.<vault>` stay absent. Current voucher supply and present permission counts are not lifetime-issuance proof; unresolved mint/burn/resolver/refund history is not waved away by waiting for known receipts.
- **Approval needed:** inspection is read-only; every permission change NEEDS APPROVAL naming the loss of authority.
- **Rollback / stop:** **non-reversible sealing warning:** deleting the last vault account permission removes external deployment/recovery authority. Current source has no DAO upgrade method or early lock override that restores it. Do not promise rollback after sealing; a defective keyless vault may require a new deployment and only whatever reviewed fund-recovery path still works. Missing owner recoverability or uniqueness/history proof blocks custody funding.

## 16. Exercise owner controls and assign the initial agent

- **Exact governed calls:** DAO predecessor → vault `pause({})`, deposit 0; `unpause({})`, exactly 1 yoctoNEAR; `set_agent({"agent": <PENDING:AGENT_ACCOUNT_ID>})`, exactly 1 yoctoNEAR. Call gas `<PENDING:MEASURED_APPROVED_BUDGET>`. Initial assignment is immediate; replacements are queued for 86400 seconds.
- **Required inputs:** reviewed DAO proposal/execute schema, exact action approvals, no treasury balance yet, accounts and budgets `<PENDING>`.
- **Expected success checks:** only DAO predecessor succeeds; member-wallet/outside calls are refused with exact expected errors under separate negative-test approval. `is_paused`, `get_owner`, `get_agent`, `get_pending_agent` match expected transitions; source configuration remains unchanged.
- **Approval needed:** NEEDS APPROVAL for each governed call and negative test, including DAO proposals/votes/executes. Any queued replacement/apply/cancel needs its own approval.
- **Rollback / stop:** failure to pause/unpause/assign via actual governance blocks funds and trading. Stop clients; approved pause can stop unsigned work but cannot revoke an already-issued signature. Do not change owner model or add a vault permission to bypass governance.

## 17. Register derived Intents authority

- **Exact call:** permissionless approved operator → vault `register_intents_key({})`, caller deposit 0; approved prepaid gas `<PENDING>` (sandbox test attaches 200 TGas; no live minimum is established by that fixture).
- **Required inputs:** inspected signer/Intents/domain/gas, operating balance for vault-funded registration, concrete approval and safe registration evidence reader `<PENDING>`.
- **Expected success checks:** final derive/add/callback chain succeeds; `is_key_registered` becomes true; authenticated Intents registration for the vault-derived identity is confirmed without printing identity values. Caller cannot choose the derived identity. Internal derive uses path `intents`, domain 1; Intents add call receives vault-funded 1 yoctoNEAR.
- **Approval needed:** NEEDS APPROVAL; permissionless does not mean read-only or cost-free. Repeating a failed attempt needs another approved budget.
- **Rollback / stop:** keep treasury unfunded if derivation/registration/configuration fails. Registration state false or malformed responses are not fixed by substituting a random identity. Reconcile failure before retry; a true recorded flag does not prove current external authority survived external administrator changes.

## 18. Exercise declared-source delay/removal and governance cancellation

- **Exact calls:** read-only `simulate_mandate_change({"mandate": <PENDING:REVIEWED_CHANGED_POLICY>})`; DAO `set_mandate`/`cancel_pending_mandate`, each exactly 1 yoctoNEAR; permissionless matured `apply_pending_mandate({})`, deposit 0. Negative agent `propose` arguments exactly as step 21 with an unlisted source. Any agent replacement uses `set_agent`, `apply_pending_agent`, `cancel_pending_agent` with their reviewed deposit rules.
- **Required inputs:** initial/changed policies, intended admitted source, exact DAO/negative-test approvals and 24-hour schedule `<PENDING>`.
- **Expected success checks:** unknown source refused without changed public trade state; adding any ID, even first ID to empty, is queued; early apply fails before maturity; mature apply requires `now >= effective_sec`; removal applies immediately and disallowed pending execution is refused. Tightening clears older queued change; replacement queue restarts its timer. Exact/case-sensitive IDs admit labels, not authentic model origin.
- **Approval needed:** NEEDS APPROVAL for every set/cancel/apply, proposal, vote, execute and failed transaction. Preview alone needs no chain mutation approval.
- **Rollback / stop:** queued loosening can be cancelled only through separately approved DAO control; cancellation does not undo an already applied change or issued signature. A live 24-hour source-delay demonstration needs its own schedule and must not delay the required withdrawal beyond the queue deadline below. Do not patch live state/time.

## 19. Register external storage and approve test funding

- **Exact conditional calls:** selected asset/Intents/DAO/operator storage methods, including any `storage_deposit`/balance registration, are `<PENDING:DEPLOYED_ABI_AND_REQUIRED_DEPOSIT>`; transfers specify exact receiver and positive base-unit amount `<PENDING>`. Do not assume all deployed contracts share a storage interface or fixture cost.
- **Required inputs:** actual storage requirements, accounts, asset units, voucher per-attempt storage budget, operating liquidity, counterparties, concrete funding limits `<PENDING>`.
- **Expected success checks:** all mandatory receiver/storage registrations are final; vault/native operating balance supports registration, signing and voucher calls; externally persisted per-ID voucher storage is funded and measured. Funding is small labelled test liquidity under an explicit loss/gas limit.
- **Approval needed:** NEEDS APPROVAL for every registration, deposit, sponsor/counterparty/vault transfer and operating budget.
- **Rollback / stop:** wrong asset/recipient/units, missing registration or unknown storage economics blocks funding. On-chain storage/funding is not undone by changing local JSON; any refund/withdrawal path needs a separate reviewed approval.

## 20. Credit the vault and counterparty Intents ledger

- **Exact NEP141 call blueprint:** approved funding account → approved asset contract `ft_transfer_call({"receiver_id": <PENDING:INTENTS_ACCOUNT>, "amount": <PENDING:DECIMAL_BASE_UNIT_STRING>, "msg": <PENDING:SOURCE_VERIFIED_DEPOSIT_ENCODING_FOR_VAULT>})`. Attached deposit is exactly 1 yoctoNEAR for the reviewed NEP141 interface; gas and registration prerequisites `<PENDING>`. Verify the actual selected contract's interface before using it. Counterparty credit is a separate approved call.
- **Required inputs:** exact asset/Intents/deposit-message version, receiver registration, amounts/units, funding approval, pre-balance snapshot `<PENDING>`.
- **Expected success checks:** final FT transfer/resolver success; vault's Intents ledger credited by the exact intended amount; funding account and Intents/FT effects match receipts; counterparties have the assets needed for the approved matched bundle.
- **Approval needed:** NEEDS APPROVAL for each transfer; vault/FT/counterparty amounts and resolver behavior must be reviewable before submission.
- **Rollback / stop:** ambiguous credit or refund must be reconciled before repeating funding. Do not send FT directly to the vault and assume it credits the Intents ledger. Any recovery of a wrongly credited amount follows the actual reviewed external protocol, not a guessed owner shortcut.

## 21. Choose and approve exactly one core trade path

- **Exact direct proposal call:** agent predecessor → vault `propose({"asset_in": <PENDING>, "asset_out": <PENDING>, "amount_in": <PENDING:U128_DECIMAL_STRING>, "min_amount_out": <PENDING:U128_DECIMAL_STRING>, "reason_hash": <PENDING:64_HEX_COMMITMENT>, "model_id": <PENDING:ADMITTED_SOURCE_ID>})`, deposit 0; reference agent uses 30 TGas.
- **Manual runner alternative:** `cargo run -p mandate-agent -- '<PENDING:REVIEWED_PUBLIC_PLAN_FILE>'`
- **Required inputs:** MANUAL_CONTROL versus REBALANCER_WITH_RELAY choice `<PENDING>`, exact tuple/hash/mandate/fresh snapshot, supervised runner and secure process configuration, actual chain ID/endpoint pin, transaction limit `<PENDING>`.
- **Expected success checks:** submitted proposal tuple/source/hash exactly matches approved input, correct proposal ID and 60-second expiry; no local amount repair or unexpected subsequent proposals. Manual mode uses `manual-v1`; rebalancer uses `rebalance-v1`. Choose the rebalance path below for the existing quote-bound publisher; a manual rationale/outcome file alone is not publisher metadata.
- **Approval needed:** NEEDS APPROVAL for the exact proposal or executing runner, and any resulting external requests. Do not execute both alternatives by default. Readiness/output must match approved account/network before submission.
- **Rollback / stop:** no proposal if paused, halted, active trade/withdrawal, pending unexpired proposal, outstanding signature, missing registration or source refusal. Submitted proposal has no public cancellation method; it can expire, be consumed by approved execution, or be superseded only after expiry. Do not invent `cancel_proposal`.

## 22. Conditional autonomous quote preparation and execution

- **Exact preview:** `cargo run -p mandate-agent -- --auto '<PENDING:REVIEWED_PUBLIC_STRATEGY_CONFIG>' --dry-run`
- **Exact one-decision executing alternative:** `cargo run -p mandate-agent -- --auto '<PENDING:REVIEWED_PUBLIC_STRATEGY_CONFIG>'`
- **Required inputs:** testnet RPC/vault/relay, exactly two mandate assets, target/band, admitted `rebalance-v1`, public config/outbox, provider/quote authorization and secure runtime route `<PENDING>`. `minimum_quote_validity_sec` must be 450–3600; quote expiry must be strictly greater than `now + threshold`, equality fails.
- **Expected success checks:** preview makes public RPC/quote calls only and loads no signing/auth configuration or outbox; returned candidate matches approved exact-input quote/amounts. Execution refreshes the final-block snapshot and engine gates before the same tuple is proposed, and writes matching `proposal-ID.json` quote-binding metadata. Pin testnet chain expectations; example config is not approved configuration.
- **Approval needed:** external quote/provider requests need their own approved scope; the executing command additionally NEEDS APPROVAL for signing/proposal. No `--loop` is started by this checklist.
- **Rollback / stop:** wrong network, unavailable/unauthenticated relay, mismatched asset/amount, expiring quote, missing metadata, new closed gate or transport error stops submission. A prepared quote is neither a fill nor permission to publish. Do not silently refresh/change trade amounts outside approval.

## 23. Execute the approved proposal with a supervised keeper/operator

- **Exact contract call:** approved permissionless operator → vault `execute({})`, deposit 0, 300 TGas as used by the reference keeper.
- **Existing supervised keeper alternative:** `cargo run -p mandate-keeper` (no dry-run or once flag; continuously loops and can submit transactions). Default publication is disabled.
- **Required inputs:** specific unexpired proposal, operator/process account, approved native budget, outbox/supervision/termination condition `<PENDING>`.
- **Expected success checks:** execute begins before proposal expiry; authoritative exact gate rejection or signing result is recorded. Approval reserves cooldown, turnover, high-water mark and deadline before requesting signature. On success record matching proposal ID/payload commitment and expiry, with no signing values printed. Treasury ledger remains unchanged until settlement.
- **Approval needed:** NEEDS APPROVAL for execute or a bounded supervised keeper run, including every allowed retry. The keeper's permissionless role does not authorize an unbounded budget.
- **Rollback / stop:** terminate the keeper after the target outcome/limit; preserve outcomes and receipt evidence. Gate rejection consumes the proposal and adds a strike; three consecutive rejections halt proposals. Owner `resume_proposals({})` requires exactly 1 yoctoNEAR and separate approval. Stopping the keeper or pausing does not revoke a signature. Never automatically retry an ambiguous signing request.

## 24. Publish/match and prove the one funded settlement

- **Exact existing quote-bound publisher command:** `cargo run -p mandate-keeper -- --publish '<PENDING:REVIEWED_PUBLIC_RELAY_CONFIG>'`; relay config must reference the agent's matching quote/proposal metadata. The process may execute and publish; no single-shot CLI exists.
- **Manual matched-bundle alternative:** selected Intents `execute_intents({"signed": <PENDING:EXACT_MATCHED_SIGNED_BUNDLE>})` through `<PENDING:REVIEWED_TESTNET_DRIVER>`, deposits/gas/validation from the selected deployed source `<PENDING>`. No fixture-only signing method is a live operator command.
- **Required inputs:** exact live signed payload/nonce/deadline, counterparty consent and funding, matching quote metadata, Intents/relay identity, authenticated endpoint, approved supervised command/budget, pre/post ledger snapshots and receipt parser `<PENDING>`.
- **Expected success checks:** final matched bundle succeeds on testnet; both parties' input/output ledger deltas match the exact approved quantities; no unexplained debit/credit; actual receipt results establish settlement. Relay acknowledgement and relay-reported SETTLED remain observations, with `settlement_verified:false` in the current publisher. Signature alone proves no funded fill.
- **Approval needed:** NEEDS APPROVAL for relay publication, all matched settlement transactions, counterparty funding and any retry. Choose one transport path, not both by default.
- **Rollback / stop:** unknown relay/send status requires nonce/receipt/ledger reconciliation before retransmission. Do not generate a new nonce/signature to hide uncertainty. The same signed intent may already have settled; no automatic undo exists. Stop after the approved settlement outcome/budget.

## 25. Prove replay refusal under separate approval

- **Exact call:** replay the same already-settled `execute_intents` bundle/nonce from step 24, only if approved; selected deployed interface/deposit/gas remain `<PENDING>`.
- **Required inputs:** final original receipts, exact unchanged bundle retained in the approved runner, expected nonce/replay error from selected source, pre-replay balances and cost approval `<PENDING>`.
- **Expected success checks:** precise replay refusal, no second debit/credit and no hidden downstream mutation. An error string alone is insufficient if transport failed before submission.
- **Approval needed:** NEEDS APPROVAL for this deliberately failing transaction; do not run it automatically after a fill.
- **Rollback / stop:** any acceptance/ledger movement stops trading and invalidates readiness. Preserve evidence; do not attempt another replay or issue compensating trades without approval.

## 26. Optional model/provenance demonstration is a separate scope

- **Exact offline command:** `cargo run -p mandate-agent -- --verify-model '<PENDING:GENERATED_PUBLIC_MODEL_RECORD>' --reason-hash '<PENDING:ON_CHAIN_REASON_HASH>'`; signed records additionally need an independently approved public signer pin through the existing verifier interface.
- **Provider-requesting preview:** `cargo run -p mandate-agent -- --model --dry-run`; this DOES call the configured provider, despite sending no NEAR transaction/outbox. Live `--model` is another separately approved proposal path.
- **Required inputs:** model/provider identity/endpoint/policy, public captured record/plan/hash, independently authenticated public pin, inference cost/privacy/supervision decision `<PENDING>`.
- **Expected success checks:** canonical `mandate-response-json-v1` hash/decision binding recomputed, exact request/response hashes retained, invalid/present unsupported proofs refused; unsigned generic captures remain UNVERIFIED. A pin proves only the supported signature binding, not hardware/model execution, authentic chain state or settlement. No provider is selected implicitly.
- **Approval needed:** offline public verification is local review. Provider lookup/inference, configuration changes and model submission need separate scope/approval; any proposal is NEEDS APPROVAL.
- **Rollback / stop:** reject inconsistent/corrupted records, unsupported proofs, wrong IDs or unapproved privacy exposure. Do not replace a failed provider or label unsigned output as authenticated model provenance. This optional path must not delay the core withdrawal clock.

## 27. Schedule the actual 24-hour withdrawal before the deadline

- **Exact scheduling calculation:** record the planned UTC queue receipt time and `effective_sec = queue_chain_now + 86400`; require at least 86400 seconds of observed real elapsed time as well as chain eligibility.
- **Required inputs:** approved one withdrawal tuple/receiver, clear trade/signature/withdrawal guards, DAO voting/execution latency, operator availability, organizer deadline verification and safety buffer `<PENDING>`.
- **Expected success checks:** **target actual successful queue execution no later than 13 November 2026 00:00 UTC**, preferably earlier. It matures by 14 November 00:00 UTC under normal chain time, leaving nearly a day before the author-supplied 14 November 23:59 UTC deadline. Starting a DAO vote is not starting the vault delay. Queuing at 13 November 23:59 UTC leaves no usable deadline margin and is not an acceptable operational target.
- **Approval needed:** scheduling is planning; queue, early failure probe, mature send and each recovery/rearm are individually NEEDS APPROVAL. Approve them concretely before the scheduled operation, not after an uncertain transaction.
- **Rollback / stop:** if the genuine queue is not confirmed by the schedule, mark the live 24-hour demonstration at risk/UNVERIFIED; never patch time, substitute sandbox maturity, or shorten the mandate delay. A replacement request resets its full 24-hour timer.

## 28. Queue exactly one approved withdrawal authorization

- **Exact governed call:** DAO predecessor → vault `request_withdrawal({"asset_id": <PENDING:LISTED_NEP141_ASSET>, "receiver": <PENDING:APPROVED_RECEIVER>, "amount": <PENDING:POSITIVE_U128_DECIMAL_STRING>})`, exactly 1 yoctoNEAR; gas `<PENDING:MEASURED_APPROVED_BUDGET>`.
- **Required inputs:** tuple/DAO action approval, correct receiver storage, balances, no execution/outstanding signature, budget/time window `<PENDING>`.
- **Expected success checks:** final DAO/vault receipt succeeds; pending tuple exactly matches approval; `effective_sec` is recorded and falls at call chain time +86400; one attempt is armed. Receiver is neither vault nor configured Intents. Save queue block/hash/time and observed UTC without printing signing material.
- **Approval needed:** NEEDS APPROVAL, including DAO proposal/votes/execute. Only this one withdrawal authorization is selected by default.
- **Rollback / stop:** wrong tuple/receiver or unexpected replacement stops subsequent sends. `cancel_pending_withdrawal({})` is DAO-only with exactly 1 yoctoNEAR and separate approval; cancelling a queued request cannot reverse an already-dispatched withdrawal. Do not requeue to repair evidence or reset a timer unnoticed.

## 29. Observe an approved early refusal without boundary-race assumptions

- **Exact call:** approved permissionless operator → vault `apply_pending_withdrawal({})`, deposit 0, at least 280 TGas, scheduled well before maturity so its executed receipt—not submission time—is early.
- **Required inputs:** unchanged queued authorization, exact early-call approval/budget, clear other guards, recorded `effective_sec`, receipt chain timestamp and baseline receiver/ledger balances `<PENDING>`.
- **Expected success checks:** executed chain time is less than `effective_sec`; precise `the withdrawal is still in its delay` failure; unchanged pending tuple, no dispatch/voucher attempt and no receiver/ledger movement. Do not infer the cause from an unspecified error or preflight transport failure.
- **Approval needed:** NEEDS APPROVAL for the deliberately failed gas-paying call. Its potential late execution must be within the already approved withdrawal scope.
- **Rollback / stop:** if the receipt executes at/after maturity, the call may dispatch; treat it as an actual withdrawal attempt and reconcile before any second call. Do not send a one-second-before-boundary live probe and assume it cannot arrive late.

## 30. Dispatch after genuine maturity and prove delivery/refund

- **Exact call:** approved permissionless operator → vault `apply_pending_withdrawal({})`, deposit 0, prepaid gas at least 280 TGas.
- **Required inputs:** queue authorization unchanged, real elapsed 86400 seconds, chain `now >= effective_sec`, no active trade/signature/withdrawal guard, owner-armed attempt, receiver registration and exact send approval `<PENDING>`.
- **Expected success checks:** identified attempt owns Preparing then Dispatched; preparation proves voucher supply1/source1/sinks0 and sufficient asset balance; marker precedes `ft_withdraw`. Final returned amount equal to requested records Delivered and exact receiver/ledger deltas; confirmed zero/refund preserves safe request only if still authorized and returns false. Unknown/partial/failing answers remain unresolved, not delivered.
- **Approval needed:** NEEDS APPROVAL for this exact send. No automatic retry is authorized.
- **Rollback / stop:** reconcile final receipts and both ledger/receiver balances before any retry. Successful delivery consumes authorization. A debit observation is not terminal receiver delivery or proof no later resolver refund. Do not recreate the same matured request after apparent success.

## 31. Conditional withdrawal recovery and one owner-rearmed safe retry

- **Exact recovery call:** approved permissionless operator → vault `recover_withdrawal({})`, deposit 0; wait `now >= active.started_sec +300`; Dispatched recovery needs at least 140 TGas. Preparing timeout invalidates its ID before sending. Use 140 TGas for the approved recovery envelope without assuming Preparing requires the same minimum.
- **Exact safe-retry calls, only after proof/approval:** DAO → `rearm_pending_withdrawal({})`, exactly 1 yoctoNEAR; then operator → `apply_pending_withdrawal({})`, deposit 0, at least 280 TGas. Do not issue a new request/amount by default.
- **Required inputs:** original attempt ID/stage/sequence, exact failure/recovery scenario and proofs, receipt/ledger baseline, governed rearm and independent retry budget `<PENDING>`.
- **Expected success checks:** durable proof requires supply1/source0 and exactly one unit in only sent or cancelled sink. Cancelled proof safely requeues an unarmed still-authorized request; sent proof records DebitObserved and consumes it. Owner rearm preserves original asset/receiver/amount/maturity; next attempt has a new ID; late old callbacks cannot change it. Missing/inconsistent proof retains the guard.
- **Approval needed:** NEEDS APPROVAL for every recovery, cancellation race, rearm and retry. Controlled fault injection or a deliberately blocked receiver requires its own reviewed testnet method/source and approval `<PENDING>`; sandbox-only mock controls are not live operator methods.
- **Rollback / stop:** **F26:** elapsed time, owner authority and a larger balance cannot override unknown Dispatched guard. It may block new withdrawal dispatches and trade authorization indefinitely; the original send may already have moved funds. A successor can be queued, but cannot dispatch through that guard. Do not blindly resend, manually clear state, invent an owner reset or conceal lost availability. If no safe live fault can be produced, label that scenario UNVERIFIED rather than adding another unapproved withdrawal.

## 32. Approve and prepare a distinct SR-1 fault scenario

- **Exact preparation:** reviewed controlled testnet fault mechanism/driver and expected call/receipt sequence `<PENDING>`; no public contract `recover_execution`, `reset`, or generic trade `recover` exists.
- **Exact proposal/execute calls:** agent `propose` with a separately reviewed tuple/commitment/admitted ID, deposit0; operator `execute({})`, deposit0, 300 TGas.
- **Required inputs:** concrete fault-injection source/ABI, signer behavior and transaction budget, safe duplicate-settlement observation, exact scenario/trade approval `<PENDING>`.
- **Expected success checks:** local fault tests first establish the intended failure path; live evidence distinguishes before-request failure from ambiguous/lost/late signer responses. The deployed trusted signer is not silently replaced and live storage/time is not patched.
- **Approval needed:** NEEDS APPROVAL for fault injection, proposal, execute, signer/provider operation and any subsequent trade. This is additional test exposure, not covered by the core trade approval.
- **Rollback / stop:** if a safe controlled fault is unavailable, report SR-1 live fault recovery UNVERIFIED and do not run a random malformed/underfunded request. Stop clients on unexpected signing behavior; do not reinterpret failed transport as signer failure.

## 33. Monitor SR-1 timeout and approve any replacement trade

- **Exact read calls:** `get_chain_time_sec({})`, `is_in_flight({})`, `get_pending_proposal({})`, `get_trading_state({})`, `get_last_proposal_outcome({})`, plus sanitized final execution/signing receipts. Current API exposes no direct execution-ID/start-time getter; derive exact execution identity/start from authenticated callback/receipt evidence through `<PENDING:REVIEWED_RECEIPT_READER>`, not from the boolean alone. Signed-intent data stays in the approved runner without printing signing identity values.
- **Exact replacement calls after proof/approval:** a new approved agent `propose`, deposit0, followed by approved `execute({})`, deposit0, 300 TGas. Never use `recover_withdrawal` for this trade.
- **Required inputs:** identified attempt and actual start time, approved monitor, old signed deadline/nonce disposition, separate replacement tuple/budget/approval `<PENDING>`.
- **Expected success checks:** signing requests stop at `started_sec +300` (equality closes); unresolved lock expires at `started_sec +420` (equality releases). Success/pre-sign failure may clear earlier. Intent lifetime is120 seconds from approval; quota/cooldown remain reserved after ambiguous signing. Old callback cannot change newer state; original and replacement do not produce duplicate ledger movement. Availability after expiry does not prove original settlement or absence of a signature.
- **Approval needed:** monitoring is read-only; every replacement/execute/publish/settlement needs separate approval. Polling is bounded and supervised, not an implicit recurring task.
- **Rollback / stop:** keep operation halted if identity, receipt history or expired authorizations cannot be established. Pausing does not revoke issued signatures. Do not artificially clear a timed lock or double-sign the same outstanding proposal. Preserve UNVERIFIED for an unobserved late-callback/live fault case.

## 34. Abort or close the controlled run

- **Exact local action:** terminate the supervised agent/keeper/updater processes without deleting their evidence. If stopping unsigned vault work is required, separately approved DAO `pause({})`, deposit0, exact measured governance gas `<PENDING>`.
- **Required inputs:** operation outcomes, outstanding proposals/signatures/guards, remaining budget, exact stop/recovery authority `<PENDING>`.
- **Expected success checks:** no unattended signer/keeper loop remains; all authorized attempts have a documented final outcome or explicit unknown state; outstanding signatures are monitored until expiry/receipt resolution; F26 is recorded honestly.
- **Approval needed:** process supervision under the approved run scope; any pause, cancellation, refund/withdrawal, recovery, permission change or deployment NEEDS APPROVAL. No chain deletion or reset is a cleanup action.
- **Rollback / stop:** preserve funds/evidence and stop further sends on any unexplained ledger delta or authority failure. Do not compensate by an unapproved opposite trade, destroy accounts, restore vault permissions, or claim keyless deployment can be rolled back.

## 35. Produce the evidence bundle and reconcile documentation

- **Exact local checks:** `./scripts/check-claims.sh`; review public logs/receipt summaries against the exact approved manifest/artifact and operation IDs. Inspect only allowed public evidence through the approved sanitizer; do not display signing/permission values. No chain step or tracked-evidence refresh runs automatically.
- **Required inputs:** actual approved action/receipt records, source/artifact digests, UTC times, testnet chain/block anchors, gas/storage/cost totals, known limitations and user-approved evidence destination `<PENDING>`.
- **Expected success checks:** each checklist entry has NOT RUN/PASSED/FAILED/UNVERIFIED backed by actual results and matching approval. Genuine queue/maturity elapsed-time evidence is separate from sandbox fixtures. Relay acknowledgements, signatures and DebitObserved are distinguished from settlement/delivery. CI status, oracle support, provider provenance and F26 limitations are not overstated.
- **Approval needed:** local report preparation is documentation work. Commit/push/PR/website publication and any production action are separate scopes; no production command is part of this testnet checklist.
- **Rollback / stop:** source/artifact mismatch, omitted failures or unsupported claims return evidence to review. Preserve prior evidence; do not edit old logs to manufacture a pass. Reconcile stale Pyth-specific plan wording before presenting a final selected-oracle operation.

## Approval/result ledger to complete during execution

For every chain mutation above, retain: step/scenario; action type; testnet sender and receiver; exact public argument commitment; amount/deposit/prepaid gas/total limit; concrete approval record; submitted transaction/receipt IDs; final executed block hash/height/time; result; ledger/receiver deltas; outstanding guards/expiry; next permitted action; observed limitation. **Every entry is NOT RUN at this documentation checkpoint.** No real account, oracle, factory, amount or approval has been substituted for `<PENDING>`.

## Source references and drafting contradictions

- Authoritative contract methods: `contracts/vault/src/lib.rs`; engine time/rounding/gates: `engine.rs`; policy bounds: `mandate.rs`; recovery/uniqueness/guard behavior: `withdrawal.rs`; adapter semantics: `oracle.rs` and implemented Pyth adapter. Sources inspected without protected helper/dependency/credential reads.
- Reference CLI modes: `agent/src/main.rs`, `agent/src/lib.rs`, `agent/src/autonomous.rs`, `agent/src/relay.rs`, `keeper/src/main.rs`, `keeper/src/lib.rs`.
- The existing operation plan now says oracle unselected at its introduction, but still names a Pyth receiver in its pending-input/initializer/setup sections and universally requires64-hex feeds. These must become selected-adapter conditional fields before approval.
- The existing runbook correctly records Pyth Core as unsupported, but its ordered account-resolution list still names Pyth. No usable alternative deployment/updater is established by those documents.
- Standalone CLI, DAO/factory/action adapter, actual secure provisioning, operation config files, raw deposit-message encoding, storage funding and final gas remain pending. No exact executable deployment/governance/settlement driver should be invented to fill those gaps.
- Public `is_in_flight` does not expose stored execution identity/start time. Exact SR-1 assertions need a reviewed receipt reader. The agent has no public proposal-cancel method; the keeper has no once/dry-run flag; manual mode does not generate the rebalancer's quote-binding publisher metadata.
- Site production publication in the runbook is outside this testnet-only checklist. No website execute, merge, push, account deletion or permission recovery is implied by this draft.
