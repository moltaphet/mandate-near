# Operational readiness and release gates

Mandate remains an experimental prototype. Passing local tests establishes the
tested source behavior; it does not establish live custody, a functioning price
updater, real MPC signing, solver fill or safe recovery for every historical
deployment. The submission deadline supplied by the author is 14 November 2026
23:59 UTC. The official contest URL, eligibility and selected track are
UNVERIFIED until the organizer's rules are supplied and checked.

## Read-only review

Run `python3 -B scripts/inspect-deployment.py --network mainnet --rpc https://rpc.mainnet.near.org --vault PUBLIC_ACCOUNT --expected-wasm-sha256 REVIEWED_WASM_SHA256`.
Replace the two placeholders with public account and reviewed artifact values.
The inspector records RPC-reported network identity, anchors one final block, checks returned WASM bytes against their
reported Base58 hash and account metadata, validates hash/height consistency, refuses redirects, and reads control views and external
code at that block. Permission output contains counts and booleans only. No
signing credential is required or emitted. A zero exit code means no implemented
inspection check failed; its readiness field remains UNVERIFIED. The inspector
does not independently authenticate the chosen RPC, inspect owner governance,
check external upgrade roles or prove MPC/settlement interoperability.

The existing cream/serif site at `web/site/index.html` includes the read-only
dashboard at `#vault`. At the user's explicit request its layout was rewritten,
retaining the original paper/card palette and serif/Inter/mono typography.
Responsive navigation, a complete empty/loading/error/snapshot flow, rule and
evidence sections, honest release milestones, FAQ, safe public vault links and
clipboard feedback are integrated into the same page.
The published `https://mandatenear.xyz/` appearance was observed and matches that
source design, but the local changes have not been published. `scripts/build-web.py`
copies the tested reader into the self-contained `web/site/audit.mjs` asset.
`web/site/site.mjs` adds navigation and sharing without wallet transactions.
Direct `file://` opening cannot reliably load browser modules; the page explains
how to open the HTTP preview. The rewritten local desktop/mobile rendering
remains UNVERIFIED because automatic browser approval review failed with a
service error. This was not a determination that the action was unsafe.
The inspector reads a final-block snapshot from a public RPC. Observed state
shows recorded controls, adapter, queued changes, withdrawal phase and listed
Intents ledger balances. It does not infer settlement from signature presence or
expiry. The separate sample mode remains an illustration. To reproduce a local
browser fixture, run `python3 -B scripts/dashboard-fixture.py` and select Local
RPC fixture at http://127.0.0.1:8124. That server serves only whitelisted site/dashboard assets and
fixed public fixture responses; it is not a live vault or a general workspace
file server.

## Oracle deployment choice

`new` and `new_with_externals` retain the PriceOracle adapter. A fresh deployment
may choose `new_with_oracle` with immutable `oracle_kind` equal to `price_oracle`
or `pyth`. Inspect the choice through `get_oracle_kind`. There is no provider
change method on an initialized vault.

PriceOracle uses its aggregate and per-feed report publication data. Pyth uses
`list_prices_no_older_than` with lowercase 64-character hexadecimal feed IDs.
Pyth values require a positive signed price, nonnegative publication time,
exponent from -30 through 0 and confidence strictly below price. Confidence
divided by price must also fit the mandate's existing slippage allowance.
Equality at the confidence allowance passes. These checks are additional input
guards; confidence is not a proof of independent market correctness. Both
adapters still use the same engine, held-asset freshness checks and final
signing-age check.

At mainnet block 223333740, the queried PriceOracle NEAR entry was null. At block
223333992, Pyth accepted the documented request but its NEAR price was null.
Neither observation establishes a usable current oracle path. Before selecting
one, verify each asset/feed identity, quote currency, decimal units, updater
availability, maximum age under actual load, confidence behavior, deployed
binary/source correspondence and administrative roles. A cached null correctly
blocks trade authorization.

## Deployment and migration

Prefer a fresh vault with a distinct recoverable owner. The owner selected
multisig/DAO governance on a separate account; its public AccountId, vote/cancel
procedures and recoverability remain UNVERIFIED. A self-owned vault loses external owner authority when its account
permissions are removed.

Prefer a fresh account created, funded, deployed and initialized atomically
without adding any account access permission. For an existing account, establish
that every pre-seal voucher issuance/burn/resolver/refund receipt has completed before funding
or enabling operations: present permission counts do not establish that history.
Deploy and initialize in the same NEAR action batch. Record the exact source
revision plus allowed-file manifest and WASM SHA256 before execution. Verify
configured signer, Intents and oracle accounts, feed mapping, funded storage and
registered derived signing authority. Remove every bypass-capable vault account
permission only after confirming that the distinct owner remains able to pause,
queue/cancel changes and authorize withdrawals. Do not emit permission values in
evidence. Reinspect the sealed account at a final block.

Appending provider and voucher fields changes Borsh state. A plain WASM replacement is unsafe
for an old layout. The migration recognizes exactly five layouts: `77ebc0c`
pre-sequence, `0de5c48` sequence-enabled, pre-provider remediation and
provider-enabled pre-voucher, and voucher-enabled pre-model admission at
`b43569d`. The fourth and fifth preserve the selected provider. Active and
queued migrated source lists start empty; first admission still waits 24 hours.
Unresolved voucher stages and retry authorization must be resolved. It must execute
privately and atomically with deployment, while paused and without an execution
record, pending proposal, queued withdrawal or future signed deadline. An active
remediation withdrawal guard is refused, never discarded. Inactive execution
snapshot metadata is discarded only after the no-execution and expired-signature
checks; old callbacks retain no authority. Old layouts did not record sent
withdrawals, so external receipt completion must be established separately.
Migration is not an escape from an unresolved historical send.

## Withdrawal proof prerequisites

Withdrawal apply requires at least **280 TGas**; dispatched recovery requires
at least **140 TGas**. Before dispatch, authenticated Intents reads must show
global `mt_supply` exactly one, source balance one, both destination balances
zero and sufficient victim balance. This rejects receipt units preseeded into
any ledger by a former issuer authority. Recovery requires supply one, source
zero and exactly one unit in only one named sink. Malformed, failed or
contradictory proof retains the guard. These checks still depend on trusted
Intents accounting and no outstanding pre-seal voucher issuance/burn/resolver/refund receipt that can mint
after the preparation proof.
`mt_supply` is current wrapped ledger supply, not lifetime issuance. A duplicate
temporarily burned by an earlier NEP245 withdrawal can return through a later
refund. Therefore the required historical quiescence includes external-holder
withdrawals and their resolvers, not only vault-originated mint calls. Use a
fresh atomic account with no added access authority for real funds unless that
complete history can be authenticated independently.
That history must establish that no canonical future withdrawal ID was
previously issued. Merely waiting for known receipts to finish is insufficient:
an undisclosed duplicate holder can start its own burn/refund later.

Both `s.<vault>` and `c.<vault>` must be absent NEAR accounts with no Intents
signing or proxy authority, and the sealed vault must have no later account
creation path. Current signing-list emptiness is insufficient. The inspector
reports observed account/identity absence; it leaves historical issuance and
receipt quiescence **UNVERIFIED**.

Sent proof records initial `DebitObserved`, consumes that authorization and
releases the guard. It does not prove terminal receiver delivery or absence of
a later FT resolver refund; those receipts may overlap subsequent operations.
Cancelled proof safely requeues an unarmed request; owner plus exactly one
yoctoNEAR must rearm it, preserving maturity and allocating a new attempt ID.
Per-ID records persist in external Intents storage and need measured funding.

## Required live evidence, in order

1. Freeze and independently review the final source/artifact and withdrawal
   recovery protocol. Preserve explicit limitations for old unknown operations.
2. Select and validate recoverable owner governance and a functioning oracle
   updater/feed universe. Record external deployment and administrative trust.
3. Prepare the exact deployment, initialization, storage, registration and
   permission-sealing transaction plan for owner authorization.
4. Demonstrate actual vault-to-MPC signing with the chosen Ed25519 domain and
   exact payload, then matched funded Intents settlement. Record final receipt
   hashes, input/output ledger deltas and replay refusal. A relay acknowledgement
   alone proves no fill.
5. Run an owner withdrawal after a genuinely elapsed 86,400-second delay.
   Record receiver and ledger deltas and final callback outcome. A sandbox
   maturity patch proves the later lifecycle, not elapsed wall-clock time.
6. Demonstrate signing failure, ambiguous response, callback loss and late
   responses without duplicate authorization. Demonstrate masked-credit
   withdrawal recovery under the reviewed protocol and actual deployed binary.
7. Record gas, storage and quote/price latency at the supported asset limit,
   with operational alerts for stale feeds, guards and unpublished signatures.

OutLayer/TEE integration remains planned. The optional model client, canonical
capture and declared-source admission are implemented locally; live provider
signature compatibility and authenticated model origin remain UNVERIFIED.
These features must not replace the on-chain authorization boundary or delay proving the
funded core flow. No source review guarantees a hackathon rank or zero defects.
