# Mandate — controlled testnet runbook draft

Draft only, 7 November 2026. These are preparation steps, not completed chain
actions. Deployment, funding, signing and permission changes require the
project owner's explicit approval for the concrete target and action. No such
operation was performed while preparing this document.

## Account and external-contract decisions

| Component | Selected intent | Evidence still required |
|---|---|---|
| Network | NEAR testnet only | Approved RPC and exact chain identity |
| Vault | Fresh reviewed deployment | Approved account, atomic create/deploy/init and exact artifact |
| Owner | DAO account distinct from vault | DAO implementation, vote/execute/cancel/recovery and permissions |
| Oracle | Pyth, conditional | Required feeds/units, updater, freshness and confidence behavior |
| Signer | Official docs list `v1.signer-prod.testnet` | Deployed account/source/permissions, domain1/config/gas and a successful vault call |
| Custody | Intended testnet Intents deployment | Actual deployed code, ledger behavior, administrator trust and signer registration |

The historical `mandate-r1-4715a0.testnet` account is an MPC spike reference,
not the approved fresh vault. The task owner's read-only inspector run on
7 November 2026 exited 1 at block 276611007, hash
`3o76mvH6agHFN91kuxvCGgZ32ytcuC7cgKGSLjoKakWk`. It observed one FullAccess
permission and 124611 code bytes, SHA256
`3fe886513a58e592d33ffbe3e92f80924957e78de53ac645c455a8b40aea6be0`.
Control views were invalid and no expected artifact pin was supplied
(`expected_code_matches=null`). The code digest was checked for consistency with
the RPC code hash; it was not matched to reviewed source or a reviewed artifact.
Current-vault readiness remains **UNVERIFIED**. Public command evidence is in
`/private/tmp/mandate-review-2026-11-07/selected-account-inspection.txt`.
Historical [testnet notes](TESTNET.md) also recorded an empty Intents account;
that does not establish today's availability. Do not reuse a historical account
or infer current deployment from old receipts.

## Public external-source assessment

Read-only research on 7 November 2026 establishes schema compatibility, not
deployed contract compatibility or a completed chain demonstration:

- [NEAR's implementation docs](https://docs.near.org/chain-abstraction/chain-signatures/implementation)
  list `v1.signer` on mainnet and `v1.signer-prod.testnet` on testnet.
  [The overview](https://docs.near.org/chain-abstraction/chain-signatures)
  identifies domain 1 as Ed25519. Official MPC source pinned at
  `d0fc87b19e2643299e7becdd7d9611afcd5d5a78` accepts `payload_v2` with an
  `Eddsa` hex string, at most 1232 decoded bytes, and uses a one-yocto signing
  deposit. It returns an Ed25519 scheme tag and a 64-byte signature array.
  These facts do not prove that the documented testnet deployment currently
  runs this revision, has the expected domain/configuration or can complete
  the vault's call within its gas and execution window.
- [Pyth's NEAR receiver source](https://github.com/pyth-network/pyth-crosschain/blob/ec40e5f1705a3f6c7818b0f2f1e96a4ac75a7454/target_chains/near/receiver/src/lib.rs#L516)
  provides `list_prices_no_older_than(price_ids, age)`, returning feed-ID map
  entries that can be null. Its Price schema contains signed price, unsigned
  confidence, exponent and publication time. Prices are cached; consumers
  must arrange Hermes updates through payable `update_price_feeds(data)` and
  the receiver's Wormhole verification path. The receiver allows publication
  times up to `age` in the future; Mandate must retain its stricter future
  guard. Source is pinned at `ec40e5f1705a3f6c7818b0f2f1e96a4ac75a7454`.
- The current Pyth addresses page and NEAR README did not establish a deployed
  NEAR receiver account. The older NEAR docs links were generic or unavailable.
  The example update script is insufficient deployment/updater evidence and
  uses `update_price_feed`, whereas the reviewed receiver method is plural.
  An exact receiver account, authenticated code/configuration, feed mapping,
  current cache entries and reliable updater remain **UNVERIFIED**.

Exact URLs, source excerpts and limitations are saved in
`/private/tmp/mandate-review-2026-11-07/external-contract-public-assessment.txt`.

## Ordered evidence run

1. Freeze the final source and artifact. Complete the serial fast/integration
   commands, record failures verbatim and measure final gas. The latest default
   baseline exited 101 on loopback permissions; it is not a complete pass.
2. Resolve the exact approved vault/DAO/signer/Intents/Pyth accounts. Confirm
   fresh held/traded feeds and a functioning updater. Keep the run blocked if
   the necessary testnet contracts or prices are unavailable.
3. Review atomic deployment and initialization with a distinct DAO owner,
   bounded declared model/source IDs and intended external accounts. Verify
   inert recovery sinks, fresh issuer history and sufficient storage funding.
   Authenticate the exact artifact and seal all bypass-capable vault authority
   before custody operations. Migration needs its known-layout/quiescence
   proofs; replacing WASM alone is insufficient.
4. Exercise DAO control operations and the model/source delay policy. Show an
   unknown declared ID refused, delayed addition and immediate removal. These
   IDs are self-reported metadata; gates remain authoritative if an agent lies.
5. Register the vault-derived Intents signing identity through the approved
   Chain Signatures contract. Record final receipt outcomes and registered
   authority without exposing signing or authentication material.
6. Run one approved exact trade. Record the public model capture/canonical
   commitment, proposal tuple, gate result and acquired signature. If a provider
   capture is unsigned, preserve **UNVERIFIED**. Pinned-signature verification
   does not prove hardware/model origin.
7. Fund and match a settlement only after separate approval. Verify both parties'
   ledger deltas and final receipts. A signed intent, relay acknowledgement or
   relay-reported status is insufficient settlement evidence.
8. Queue an approved withdrawal and observe a genuinely elapsed 24-hour boundary.
   Record early refusal, mature application, receiver/ledger deltas and refund
   behavior. Do not alter chain time or patch maturity and call it elapsed delay.
9. Exercise approved SR-1 and withdrawal failure/recovery scenarios. Preserve
   execution identity, expiry, locked-state, proof and owner-rearm evidence.
   An unknown dispatched withdrawal is never blindly resent. F26 remains an
   accepted guard-availability residual; time/balance/owner cannot override it.

Phase 4 sandbox and live-provider demonstrations remain **NOT RUN** until the task
owner records actual results. Save public records and command/receipt evidence
under a dated, revision-bound evidence directory, with no credentials. Keep
synthetic vectors, sandbox receipts and live receipts separately labelled.

## Site publication after separate approval

Review `scripts/deploy-site.sh` and the approved clean `main` revision first.
`--check` validates branch, cleanliness, public assets and reader parity locally.
Only an explicitly approved `--execute-production` invocation calls Vercel:

```sh
scripts/deploy-site.sh --check
scripts/deploy-site.sh --execute-production
```

The script stages an explicit public asset list under `/private/tmp`; it does
not copy repository configuration or authentication files. It generates public
static-output configuration at the staging root. Verify the intended
Vercel account/project before execution. Then confirm the `mandatenear.xyz`
alias, root/site/dashboard paths, logo contrast, mobile and keyboard interaction
in the browser. Production execution and alias verification are not completed.
