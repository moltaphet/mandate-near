# Source-owned Intents withdrawal executor

This package prepares the pinned public Intents source with the native NEP141
withdrawal-journal module and the shared Mandate protocol crate. The journal
overlay acts in the actual custody/debit/resolver code. It is not a mock or a
proxy around the old `ft_withdraw` callback.

Preparation provenance is recorded in [source-manifest.json](source-manifest.json):
upstream commit `32cd1f9788a347908164fbcc56525dc6542f158f`, archive SHA256
`0c9073d58953d06850b408800047a0f061194e925c9ea1c2259e3910f18c24d0`,
1,324,342 bytes. Correspondence to the original pinned mainnet Intents fixture,
current deployed Intents, or an external professional audit remains
**UNVERIFIED**. This package does not upgrade or authenticate a live executor.

## Prepare explicit local inputs

```sh
python3 -B integrations/intents-executor/prepare.py \
  --archive /absolute/canonical/path/intents-32cd1f9.tar.gz \
  --destination target/intents-executor/source
```

The destination defaults to `target/intents-executor/source`. JSON output
includes `source_root`, the input-manifest path, the effective lock hash and
whether an existing verified tree was reused. Integration harnesses can use
that source root explicitly. Use canonical paths; symbolic input/destination
aliases are refused.

The script verifies archive size/hash before parsing the same in-memory bytes;
rejects unsafe member paths, duplicate paths and special files; and skips the
three recorded links without following them. It verifies original source
hashes, adds the hash-pinned journal overlay, appends `mod journal;` to the
verified module list and copies the four pinned shared-crate files plus the
nine pinned near-plugins vendor files. The deterministic Cargo edits add the
shared crate as a workspace member/dependency, select the vendored near-plugins
through a workspace `[patch.crates-io]` entry, disable NEAR SDK default features,
enable native test support and replace Defuse's ABI-forcing self dev-dependency
with its contract feature.
ABI/schema generation is not part of this build recipe.

The vendored `near-plugins` version is **0.5.3**, licensed **CC0-1.0**. Its
original crates.io archive is pinned at SHA256
`ef03cc3e96b4036f8fb0e4d3589887acc5825311a2e153d6b8ab82f5f208c919`,
41,810 bytes. The normalized manifest, README and all seven source files in
`vendor/near-plugins-0.5.3` retain the exact archive bytes; the archive contains
no separate license file. Preparation stages them under
`crates/mandate-near-plugins` and changes only the normalized manifest by adding
`default-features = false` to its runtime and development NEAR SDK dependencies.
This prevents that dependency from requesting the SDK's default allocator
feature. Original and staged hashes are recorded separately in
`source-manifest.json`; source code and README hashes remain identical.

There is no download, dependency resolution, Cargo execution, credential read
or deployment in preparation. Source links are data and are never executed.

## Lock generation, review and frozen reuse

Initially the original upstream Cargo.lock is preserved and verified. Adding
the shared crate and near-plugins path override requires an explicit separate
lock-generation/review step.
The maintainer must review the resolved public dependencies and record the
final lock as `integrations/intents-executor/Cargo.lock`. When that file is
present, preparation uses its exact bytes deterministically after still
verifying the archive's original baseline lock. Its presence is an input
selection, not evidence that review or a build passed.

The destination contains `.mandate-preparation.json`. Reuse recomputes the
entire expected tree from trusted archive/overlay/shared/vendor/frozen-lock inputs
and compares every source file, mode and input binding. It refuses existing
unverified trees, mutated source, extra files or changed unreviewed locks;
it never clears a user tree. Once an intentionally generated staged lock is
reviewed and copied into this package, an otherwise matching tree can reuse
that exact lock and update only its preparation metadata. Changed source
inputs require a new destination or a separately reviewed synchronization.

## Separate controlled build and tests

The controlled builder accepts the same explicit archive and an optional source
destination/output directory:

```sh
python3 -B integrations/intents-executor/build.py \
  --archive /absolute/canonical/path/intents-32cd1f9.tar.gz \
  --destination target/intents-executor/source
```

It verifies preparation and the frozen lock before and after the offline,
locked official cargo-near build, then writes
`target/intents-executor/build-manifest.json` with the exact source inputs and
optimized artifact hash/size. Defaults are repository-relative; `--output`
selects another controlled output root. The runtime sandbox harness requires
the canonical artifact/manifest and revalidates their source binding. The
builder does not download dependencies, access credentials or deploy.
Inherited compiler/wrapper/Rust flag and Cargo profile/build/target overrides
are refused before output mutation; the builder owns its target directory and
records the pinned tool versions and exact command. `CARGO_BUILD_JOBS` remains
available as a scheduling choice.

From the emitted source root, generate the lock only as the explicit review
step, after the required public crate archives are available:

```sh
cargo +1.95.0 generate-lockfile --offline
```

After reviewing and freezing that lock, use a build directory outside the
prepared source tree:

```sh
CARGO_NET_OFFLINE=true \
  CARGO_TARGET_DIR=/absolute/repository/target/intents-executor/build \
  cargo +1.95.0 near build non-reproducible-wasm \
  --manifest-path /emitted/source/root/contracts/defuse/Cargo.toml \
  --features contract --override-toolchain 1.95.0 --locked --no-abi \
  --out-dir /absolute/repository/target/intents-executor/artifact

CARGO_TARGET_DIR=/absolute/path/intents-executor-build \
  cargo +1.95.0 test -p defuse --features contract --lib journal::tests \
  --locked --offline
```

The canonical runtime artifact is
`target/intents-executor/artifact/defuse.wasm`, produced by official cargo-near
with its WASM optimization step. The intermediate Cargo release output is not
the sandbox runtime fixture. `--no-abi` explicitly omits ABI extraction;
ABI packaging needs separate verification on a compatible compiler/container.
No compiled artifact or passing executor build is claimed by this recipe.
The final source/lock/build inputs, compiler flags, exact artifact hash,
gas/storage evidence and integration results must be recorded after actual runs.
Native test success does not replace asynchronous sandbox or live evidence.

Focused preparation checks, requiring no archive download or build:

```sh
python3 -B integrations/intents-executor/prepare_test.py
python3 -B integrations/intents-executor/build_test.py
```

On a clean CI checkout, the workflow first downloads the exact public codeload
archive selected by the pinned commit and verifies its size/SHA before any
preparation. It prepares the frozen source, separately fetches locked public
dependencies, invokes this offline builder and then runs integration tests.
Local YAML/script validation is not evidence of a passing external CI run.

The overlay is an opt-in fresh-account journal with an immutable domain and
authorized-vault admission. Existing `unique_voucher_v1` and unknown historical
sends remain separate; this package does not retroactively establish their
terminal outcomes. Missing terminal resolver results remain unresolved. The
[protocol specification](../../docs/design/f26-durable-executor.md) records
the independent-review, receipt-proof, storage, migration and release gates.
Mainnet remains **NO-GO**.
