# Audit package

For an external reviewer of the Mandate vault. This is a starting kit, not an audit
result. Mandate has **not** been independently audited.

## 1. What to freeze

Pick one merge commit on `main` and record it in the engagement. Everything below
is stated for the revision the testnet run used, `2b043ba` (vault source unchanged
since the model-source change in `20094ed`). Any later change to `contracts/vault/src`
reopens the scope.

The pre-journal hardening candidate added per-asset withdrawal ceilings, a sixth
pre-ceiling migration layout and a 300-TGas execution entry guard. These are
outside the historical hashes/line counts below and require a new frozen
artifact. Its complete configured local scope passed on 8 November: 479 distinct
tests including all 14 sandbox targets. The tested host vault WASM is 411,312 bytes,
SHA256 `4c2e2301a3688397edb1cd1499e415e1d7cd277433aaab24f1fadee15908fde2`.
The [hardening report](HARDENING-2026-11-08.md) records source/lock identity,
test-expectation repairs, command batches and remaining dependency findings.
A frozen commit, reproducible build, CI and independent audit remain required.
Mainnet remains NO-GO.

The subsequent source-owned journal candidate adds `crates/withdrawal-journal`,
the pinned upstream preparation/vendor/lock package, native Intents journal
overlay, fresh vault journal adapter and keeper operation journal. These are
outside both historical artifact identities. Freeze both current vault/executor
WASM hashes and all preparation/build inputs; the pre-journal 479-test result
does not cover the new mode. Exact terminal reconciliation can recover a lost
vault notification after executor commit, while a missing executor resolver can
remain Pending. No finalized-receipt proof verifier is implemented.

Record these three values at freeze time: the commit hash, the SHA256 of the built
vault WASM, and the SHA256 of the Intents verifier fixture.

| Item | Value at `2b043ba` |
|---|---|
| Vault WASM (host build, 391,671 bytes) | `5266d033e30c6311d10f92d494b1cb456471daace03124342d24ededbff70f6f` |
| Intents verifier fixture | `a1afab98916d9f213746544dcf8e4e6bdb22c0be6bc5dfbd8ea8814c9922b547` |

The host build is not a reproducible-build proof. Establish one before relying on the hash.

## 2. Scope

| Path | Lines | Notes |
|---|---|---|
| `contracts/vault/src/lib.rs` | 2,576 | Entry points, execution state machine, callbacks, owner controls. Includes in-file tests. |
| `contracts/vault/src/withdrawal.rs` | 632 | Voucher-based withdrawal protocol. The highest-risk module. |
| `contracts/vault/src/migration.rs` | 1,102 | Five supported historical layouts and their golden fixtures. |
| `contracts/vault/src/engine.rs` | 826 | Pure gate engine. |
| `contracts/vault/src/mandate.rs` | 546 | Policy validation and the loosening rules. |
| `contracts/vault/src/oracle.rs`, `pyth.rs` | 408, 235 | Two price adapters. The oracle for any deployment is not selected. |
| `contracts/vault/src/intent.rs`, `math.rs` | 273, 192 | Intent builder and 256-bit math. |

Out of scope unless agreed: `agent/` and `keeper/` (off-chain clients, about 3,500
lines), the web pages, the mock contracts and the spikes. The agent and keeper hold no
treasury signing key, but they decide when proposals and executions happen.

For the new candidate engagement, include `contracts/vault/src/journal_withdrawal.rs`,
the shared commitment/schema crate, executor native custody/debit/resolver
overlay and its dependency preparation, plus keeper ID-bound recovery and
durable paid-transaction handling. Review permanent tombstones, immutable
domain/admission, funded refund slots, late resolvers, exact status matching,
ordinary NEP141 semantics and executor upgrade/ledger administrators.

## 3. Build and reproduce

```sh
./scripts/verify.sh                 # formatting, clippy, unit tests, site and agent suites
./scripts/verify.sh --integration   # adds 15 targets; requires the prepared journal artifact
cargo run -p mandate-vault --example testnet_run -- build   # builds the WASM files into target/testnet-wasm
```

Toolchain: Rust 1.95.0, cargo-near 0.22.0, near-sdk 5.30.0, near-api 0.8.7, near-sandbox
0.4.0. Use actual run output for test counts and failures. CI is configured to run
the full integration suite on every pull request; its definition is not a pass.

Prepare and build the real executor first using its
[package instructions](../../integrations/intents-executor/README.md); the
`journal_withdrawal` target requires its exact artifact and build manifest.
The current Apple host cannot load cargo-near's extracted ABI dylib with its
SDK/linker combination. `MANDATE_SANDBOX_NO_ABI=1` explicitly selects the
supported no-ABI sandbox build and keeps optimization; default ABI generation
is unchanged. Both journal and legacy targets accept that helper option.
ABI packaging needs a compatible compiler/container verification. Current
localhost/approval infrastructure also prevents executed sandbox evidence;
no compile-only or historical test count closes that gate. See
[operational reproduction commands](../OPERATIONAL-READINESS.md#current-host-verification-limit).

What the pre-journal checkpoint's tests showed: gate ordering and boundaries, owner delays,
the withdrawal state machine against the fixture, migration from six layouts
(the historical baseline above supports five), and gas at the supported
asset limit. What they do not show: behaviour against the real NEAR Intents contracts,
a production oracle, or recovery after a genuinely lost callback.

## 4. What the contract must guarantee

The first claim is in [ARCHITECTURE section 2](../ARCHITECTURE.md): a signature is
requested only after the on-chain gates pass for an exact trade. The reviewer should test
at least these:

1. A stale callback must not clear or commit state belonging to a newer execution (SR-1).
2. Loosening the mandate waits 24 hours; tightening is immediate; a queued change cannot be replaced to skip the wait.
3. A declared source ID must be on the allowlist at propose time and again at execute time.
4. A withdrawal is dispatched at most once per authorization, and an unknown result is never resent.
5. Only the owner account, as predecessor, can queue, cancel or re-arm a withdrawal, and each needs exactly one yoctoNEAR.
6. The vault account carries no access key and has no second path to sign (SR-9).

## 5. External contracts and what is trusted

| Contract | Trust assumed |
|---|---|
| Chain Signatures signer | Returns a correct Ed25519 signature for the vault's derived key only for the vault as caller. |
| NEAR Intents verifier | Verifies the signature, settles `token_diff`, and enforces nonce and expiry. The vault does not re-verify. |
| Source-owned journal executor, if selected | Correct immutable domain/admission, atomic native debit/refund/dispatch and terminal records, permanent tombstones, ordinary NEP141 semantics and reviewed upgrade/ledger administrators. A self-reported descriptor is insufficient. |
| Price oracle | Correct and fresh. The vault checks age and bounds slippage against it but cannot detect a fresh wrong price. |
| Owner DAO | Its controllers act honestly and keep their keys. A stolen owner key can drain the vault after the 24-hour delay unless cancelled in time. |

## 6. Known findings and accepted residuals

The full table, with status, is in [SECURITY.md](../../SECURITY.md) (SR-1 to SR-9, F26). The ones
a reviewer should look at first:

- **F26.** A withdrawal guard can stay set when a final callback is lost or malformed, and
  nothing clears it early. It is accepted for the prototype and testnet only. See
  [durable journal protocol](../design/f26-durable-executor.md): exact terminal
  records can recover lost vault notification, while missing executor resolution
  can remain Pending without an implemented finalized-receipt verifier.
- **Withdrawal-ceiling scope.** The historical `2b043ba` vault has no ceiling.
  The current candidate implements optional per-asset, per-request token-base-unit
  ceilings; omitted assets and migrated policies remain uncapped. There is no
  aggregate or daily withdrawal cap.
- **Model source IDs are declarations.** They are not authenticated model identity.
- **Oracle trust.** No independent loss bound exists against a fresh wrong price.
- **Cashflow and high-water mark.** Withdrawal lowers NAV without lowering the stored
  peak and can trigger drawdown refusal without a trading loss; risk-accounting design remains open.

## 7. Evidence a reviewer can check

- [Hardening review, 8 Nov 2026](HARDENING-2026-11-08.md): all configured local targets, exact tested artifact, dependency result and remaining release requirements.
- [Testnet run, 7 Nov 2026](../evidence/testnet-run-2026-11-07/README.md): a keyless deployment with a DAO owner and a verified testnet MPC signature, with transaction hashes.
- [gas.txt](../evidence/gas.txt): per-call gas on a local sandbox.
- [attacks.txt](../evidence/attacks.txt): eight attack scenarios and their refusals on a local sandbox.
- [Historical reports](../history/README.md): the earlier internal reviews and their finding IDs.

## 8. Questions for the auditors

1. Is the voucher uniqueness argument in `withdrawal.rs` sound, including mint, burn and refund overlap?
2. Can any callback ordering let an old execution overwrite a newer one?
3. Are the prepaid gas values for the signing callbacks safe at the supported asset limit?
4. Is each supported migration layout complete, and does it refuse every unsafe state?
5. Does any owner or agent path bypass the delay or the gates?
6. Which design would you choose for F26, and what would it add to the attack surface?
7. Does the native journal atomically bind debit/dispatch/refund and permanently fence every duplicate and cancellation race? Does keeper transaction finality preserve the no-resend boundary across restarts?
