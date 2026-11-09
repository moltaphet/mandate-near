# Hardening review — 8 November 2026

This report records fixes in the local `feat/withdrawal-ceiling` working tree
based on `a582d36d0233eec2607fed85fc0cab8007a960a9`. It covers the code and
local test artifacts, rather than a deployed release. No public-chain
transactions, deployment, credentials, commit or push were needed.

This is the **pre-journal hardening checkpoint**. Its 479-test scope and vault
artifact below do not cover the subsequently added source-owned executor
journal, shared protocol crate, vault journal adapter or keeper operation
journal. Those changes require their own final artifact and asynchronous
verification; see the [durable journal design](../design/f26-durable-executor.md)
and [current verification limits](../OPERATIONAL-READINESS.md#current-host-verification-limit).

## Closed defects

| Finding | Result and regression coverage |
| --- | --- |
| P1: incomplete withdrawal ceilings broke compilation | Added positive, unique, bounded per-asset U128 ceilings; enforced before request/apply mutation and before preparation dispatch. Missing ceilings preserve legacy uncapped behavior. Native tests cover equality, one-above, tightening, replacement generations, refunds and stale callbacks. |
| P2: underfunded permissionless execution could commit a lock | execute now requires 300 TGas before changing execution state. A native boundary test checks one gas unit below/equality; a sandbox regression checks a stranger's 30-TGas refusal and immediate successful retry of the same proposal. |
| P2: keeper checked the expected network after possible execution | Public preflight now runs before credentials, each tick and immediately before execution. Every endpoint eligible for SDK failover is validated. Six regressions cover wrong chains, configuration conflicts and a network change after readiness. |
| P2: routine pre-submission errors killed autonomous loop mode | Only public preparation/quote refresh retries with bounded 10–600 second backoff. One-shot still returns errors; submission/outcome ambiguity still stops the process. Library/mock-RPC regressions cover outage recovery and stale quotes; binary regressions cover one-shot failures, network pins and the fail-stop boundary. |
| P2: Intents verifier fixture identity was unenforced | Rust and Python checks require the exact reviewed SHA256 and size before sandbox deployment. A valid same-size substitute WASM is refused. Default fetching now writes a separate comparison artifact. |
| P3: shared preview URLs returned 404 | Preview routing parses the URL pathname before its allowlist. Three routing regressions cover shared root URLs and forbidden paths. |
| P3: public evidence and configuration descriptions disagreed | Documentation scopes historical passes to their revisions, describes six migration layouts and distinguishes local evidence from live readiness. Dashboard validates/displays current and queued ceilings, exact base units and uncapped omissions. Environment examples use the runtime's actual setting names. |
| Dependency: production wee_alloc was unmaintained | Disabled NEAR SDK default features so all contracts use Rust's standard Wasm allocator. Removed wee_alloc and its unused dependency family from Cargo.lock. The rebuilt artifact passed the full configured sandbox scope, including gas and withdrawal/recovery targets. |
| Dependency: old Quinn/Zstd lock entries matched OSV advisories | Selected semver-compatible quinn-proto 0.11.19, Zstd 0.13.4 and zstd-safe 7.4.0 without upgrading other dependency families. Final local tests passed; the final 491-package OSV query has no Quinn/Zstd matches. Two native development dependency findings remain below. |

Ceiling relaxation cannot hide behind asset removal: increasing or removing
an existing ceiling always waits 86,400 seconds. Tightening applies immediately.
The frozen pre-ceiling migration preserves source allowlists, queued change
deadlines, counters and price provider, starts with empty ceilings and retains
all paused/quiescent and unknown-withdrawal guards. Both user-supplied actual
pre-ceiling state fixtures are tested. The user's existing withdrawal tests
and three fixture/exporter files were preserved.

## Verification

After the user completed the official crate download, all configured local
verification components passed on the final locked dependency graph: 360 fast
Rust tests, 36 JavaScript tests, 19 Python tests and 64 distinct sandbox tests
across all 14 configured targets, for 479 distinct tests. Formatting, the
workspace all-target build check, strict all-target Clippy, local references
and shared-reader parity also passed. These results follow the allocator and
dependency changes; earlier initial-graph passes are separate historical evidence.

| Scope | Recorded final result |
| --- | --- |
| Formatting, workspace all-target build check and Clippy with warnings denied | Passed; formatting and strict Clippy were checked again after the test-expectation repairs |
| Workspace Rust library tests | 269 passed: vault 259, agent 8, keeper 2 |
| Five client targets, directly invoked with --locked --offline | 82 passed: autonomous 7, model 39, protocol 14, provenance 15, provenance_cli 7 |
| Keeper network target, directly invoked with --locked --offline | 6 passed |
| Rust Intents fixture identity | 3 passed |
| Dashboard and site JavaScript | 36 passed |
| Python inspector, preview and fixture checks | 19 passed: 12 + 3 + 4 |
| Documentation references and shared-reader parity | Passed |
| Complete 14-target serial sandbox scope | 64 distinct tests passed across three command batches; the two interrupted batches and complete affected-target reruns are recorded below |
| WASM ceiling lifecycle regressions | Passed within sandbox and withdrawal_lifecycle |
| Online dependency advisory lookup | User-run query of 491 public package/version pairs completed against the final lock; two matches remain |
| Browser visual inspection and target-chain live release verification | Not performed |

| Sandbox target | Passed tests | Final successful batch |
| --- | ---: | --- |
| agent | 5 | 1 |
| attacks | 8 | 1 |
| execute_path | 9 | 1 |
| gas | 3 | 1 |
| intents_registration | 2 | 1 |
| keeper | 4 | 1 |
| model_path | 6 | 1 |
| pyth_path | 4 | 1 |
| recovery_protocol | 8 | 1 |
| sandbox | 5 | 2 |
| sealed_deployment | 1 | 2 |
| settlement | 1 | 2 |
| withdraw | 2 | 3 |
| withdrawal_lifecycle | 6 | 3 |
| **Total** | **64** | All 14 targets |

An earlier ordinary verify.sh invocation passed preceding checks, then exited
101 because its HTTP fixture could not bind loopback under the execution
sandbox (PermissionDenied: Operation not permitted). The same client targets
passed when invoked directly with Cargo. An approved invocation subsequently
completed verify.sh with exit 0 on the initial dependency graph. After the
dependency changes, ordinary Cargo downloads failed DNS resolution and automatic
approval review failed before the approved fetch. The user then ran cargo fetch
--locked directly. All three downloaded archive SHA256 values match Cargo.lock,
and the final all-target offline check passed.

The final verify.sh wrapper invocation was not executed because automatic
approval review failed. Its complete configured scope passed through direct
commands instead; no final wrapper exit-0 claim is made. The user subsequently
ran the final OSV query and advisory-detail lookup in their terminal, resolving
the earlier infrastructure block for that evidence.

Final fast Rust commands (all exited 0):

```sh
cargo check --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --lib --locked --offline
cargo test -p mandate-agent --test autonomous --test protocol --test model --test provenance --test provenance_cli --locked --offline
cargo test -p mandate-keeper --test network --locked --offline
cargo test -p mandate-vault --test fixture_identity --locked --offline
cargo fmt --all -- --check
```

Serial sandbox command batches, in order:

```sh
# Batch 1: nine targets passed (49 tests); exited 101 at two stale sandbox JSON assertions.
cargo test -p mandate-vault --test sandbox --test intents_registration --test execute_path --test keeper --test agent --test model_path --test attacks --test withdraw --test gas --test settlement --test withdrawal_lifecycle --test recovery_protocol --test pyth_path --test sealed_deployment --locked --offline -- --test-threads=1
# Batch 2: sandbox, sealed_deployment and settlement passed (7 tests); exited 101 at one stale withdraw JSON assertion.
cargo test -p mandate-vault --test sandbox --test sealed_deployment --test settlement --test withdraw --test withdrawal_lifecycle --locked --offline -- --test-threads=1
# Batch 3: full withdraw and withdrawal_lifecycle targets passed (8 tests); exited 0.
cargo test -p mandate-vault --test withdraw --test withdrawal_lifecycle --locked --offline -- --test-threads=1
```

The failed assertions expected the old mandate JSON object without
withdrawal_limits; actual output correctly contained an empty list. Only
test expectations and the shared new-policy fixture were updated. Explicit
withdrawal_limits: [] in that fixture is semantically identical to legacy
omission; omission remains exercised by actual WASM initialization and historical
migration tests. No production source, dependency lock or historical Borsh
fixture changed during these repairs. Both affected targets were rerun in full;
successful tests repeated during failed batches are counted only once above.

| Tested identity | Value |
| --- | --- |
| Cargo.lock SHA256 | e53712759c0f89ce98a86aa7569b0b0ab6dcb55c752e9465635844c51ca7e287 |
| Production source/manifests fingerprint (33 files; includes Cargo.lock) | 2ac94ff0add8866d469b542a519437aba65bef83dea6514a7b721c21a6e00dd9 |
| Vault WASM | 411,312 bytes; SHA256 4c2e2301a3688397edb1cd1499e415e1d7cd277433aaab24f1fadee15908fde2 |
| Reviewed Intents fixture | 1,297,551 bytes; SHA256 a1afab98916d9f213746544dcf8e4e6bdb22c0be6bc5dfbd8ea8814c9922b547 |

The production fingerprint and rebuilt WASM identity remained unchanged
throughout all three sandbox batches. The per-file digest manifest, artifact
manifest and command logs are retained with the local verification evidence.
This host build is not an independently reproduced release build or a passing
CI result for a frozen commit.

Sandbox verification uses the reviewed Intents WASM fixture, a mock signer,
mock tokens and oracle stand-ins. Withdrawal maturity tests first check the
actual queued deadline, then explicitly label any sandbox-only STATE maturity
patch if fast-forward does not advance 24 hours. They do not prove elapsed
live withdrawal delays, production oracle operation or funded public settlement.

The audit's earlier integration run was interrupted (exit 130), after selected
targets passed. That run is historical partial evidence. The earlier low-gas
exploit regression passed because it confirmed the bug; the new refusal
regression verifies its remediation.

## Dependency advisory result

The user authorized sending public Cargo.lock package names and versions to
OSV. The initial scan covered 497 public package/version pairs and found six
affected pairs. The final user-run query covered 491 pairs against the exact
lock hash above. Its result is retained in
[DEPENDENCY-SCAN-2026-11-08.json](DEPENDENCY-SCAN-2026-11-08.json).
Known Quinn/Zstd advisory matches and the removed wee_alloc match are absent.
This is a scoped advisory query, not a general security certificate.

| Remaining match | Applicability and disposition |
| --- | --- |
| crypto-bigint 0.5.5, RUSTSEC-2026-0387 | Several affected operations can take secret-dependent branches. Upstream fixes this in 0.7.6; the current production client stack uses 0.7.10. Version 0.5.5 is inherited by the native NEAR test-VM stack, where the inspected path verifies public signatures rather than processing project-held cryptographic secrets. It is absent from production client/WASM normal dependency trees. No confirmed project secret-timing exploit was found. The current SDK constraint has no compatible fix; no incompatible SDK fork or advisory ignore was added. |
| unsafe-libyaml 0.2.11, RUSTSEC-2024-0449 | Informational unmaintained advisory. The native test path is serde_yaml → near-parameters → NEAR SDK unit-testing/runtime, parsing compile-time embedded SDK configuration and protocol-diff YAML. No externally supplied YAML path was found in project usage. It is absent from production client/WASM normal dependency trees. Maintenance risk remains pending an upstream replacement. |

Local review of the downloaded patched crates also found a remaining unused
prepared-dictionary lifetime concern in zstd-safe 7.4.0's ref_ddict. A
compile-only reproduction succeeded; no dangling-memory access was executed
and no corresponding call was found in project or inspected NEAR code. This
separate inherited library concern does not change the factual final OSV
result and is not a reproduced project exploit. cargo-audit is not installed.

## Remaining design and release requirements

- **Unknown dispatched withdrawals (F26):** insufficient authenticated proof
  retains the custody guard indefinitely. No elapsed-time, balance-only or
  owner bypass was added. A safe resolution needs independently reviewed
  evidence/protocol support; clearing the guard blindly can permit a double send.
- **Cashflow and high-water mark:** a successful withdrawal lowers NAV without
  lowering the stored peak and can trigger drawdown refusal without trading
  loss. Cashflow-adjusted risk accounting requires a separate design decision.
- **Ceiling scope:** limits are per request. Missing limits are uncapped;
  repeated withdrawals are not governed by an aggregate or daily ceiling.
- **Keeper failover:** every configured send endpoint must pass preflight.
  An unavailable endpoint blocks that tick. Preserving availability would
  require filtering the actual send configuration to freshly validated endpoints.
- **Dependency maintenance:** the two native development findings above remain
  open, with their limited current reachability recorded. A clean advisory
  result is not claimed.
- **Review and publication checks:** freeze and review the changes, obtain CI
  evidence for that commit and complete browser visual QA. The automated local
  checks do not substitute for those outstanding checks.
- **Release evidence:** freeze source and artifacts, reproduce the build and
  independently review governance, a live functioning oracle/updater, funded
  settlement, elapsed live withdrawal/recovery and the exact final source/WASM.
  Model identifiers do not prove model origin; signatures and relay
  acknowledgements do not prove settlement. Mainnet remains **NO-GO**.

Cargo.lock adds references to already locked development dependencies
sha2 0.10.9 and async-trait, removes the disabled allocator family and
updates only the selected quinn-proto/Zstd entries. No deployment, commit,
push or independent audit was performed during this hardening work.
