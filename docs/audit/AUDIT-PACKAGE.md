# Audit package

For an external reviewer of the Mandate vault. This is a starting kit, not an audit
result. Mandate has **not** been independently audited.

## 1. What to freeze

Pick one merge commit on `main` and record it in the engagement. Everything below
is stated for the revision the testnet run used, `2b043ba` (vault source unchanged
since the model-source change in `20094ed`). Any later change to `contracts/vault/src`
reopens the scope.

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

## 3. Build and reproduce

```sh
./scripts/verify.sh                 # formatting, clippy, unit tests, site and agent suites
./scripts/verify.sh --integration   # adds 14 sandbox targets; about an hour
cargo run -p mandate-vault --example testnet_run -- build   # builds the WASM files into target/testnet-wasm
```

Toolchain: Rust 1.95.0, cargo-near 0.22.0, near-sdk 5.30.0, near-api 0.8.7, near-sandbox
0.4.0. The repository has 389 Rust tests and 33 JavaScript tests. CI runs the full
integration suite on every pull request.

What the tests show: gate ordering and boundaries, owner delays, the withdrawal state
machine against the fixture, migration from the five layouts, and gas at the supported
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
| Price oracle | Correct and fresh. The vault checks age and bounds slippage against it but cannot detect a fresh wrong price. |
| Owner DAO | Its controllers act honestly and keep their keys. A stolen owner key can drain the vault after the 24-hour delay unless cancelled in time. |

## 6. Known findings and accepted residuals

The full table, with status, is in [SECURITY.md](../../SECURITY.md) (SR-1 to SR-9, F26). The ones
a reviewer should look at first:

- **F26.** A withdrawal guard can stay set when a final callback is lost or malformed, and
  nothing clears it early. It is accepted for the prototype and testnet only. See
  [f26-recovery.md](../design/f26-recovery.md).
- **No withdrawal ceiling.** The vault has no maximum withdrawal cap.
- **Model source IDs are declarations.** They are not authenticated model identity.
- **Oracle trust.** No independent loss bound exists against a fresh wrong price.

## 7. Evidence a reviewer can check

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
