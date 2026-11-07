# Mandate CI proposal — draft for approval

Prepared: 2026-11-07. This document proposes changes; it does not implement them. The workflow and verification-script source was read on main at `6c72705` after PR 16. No proposed classifier or scheduled workflow has been executed or verified.

## Current state

The reviewed `.github/workflows/ci.yml` is 27 lines. It runs on pushes to main and pull requests, grants `contents: read`, and has one job named `verify`. That job always runs `./scripts/verify.sh --integration`. There is no nightly schedule, manual dispatch, or changed-path classification. The runner is `ubuntu-latest`, with Node 22, cargo-near 0.22.0, the repository Rust toolchain, and a Rust cache. The cargo-near installation uses a public version-specific installer piped to a shell; pinning the version does not establish an immutable installer digest.

The reviewed `scripts/verify.sh` is 53 lines and uses `set -euo pipefail`. Its default mode runs:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/check-claims.sh
node --test web/dashboard/live.test.mjs
node --test web/site/site.test.mjs
python3 -B scripts/build-web.py --check
python3 -B scripts/inspect_deployment_test.py
cargo test --workspace --lib
cargo test -p mandate-agent --test autonomous --test protocol --test model --test provenance --test provenance_cli
```

Integration mode additionally executes:

```text
cargo test -p mandate-vault --test sandbox --test intents_registration --test execute_path --test keeper --test agent --test model_path --test attacks --test withdraw --test gas --test settlement --test withdrawal_lifecycle --test recovery_protocol --test pyth_path --test sealed_deployment -- --test-threads=1
```

The script identifies the sandbox dependencies as an Intents fixture, stand-in signer and oracle. Passing this mode would establish local integration evidence for the executed revision. It would not establish live Chain Signatures, live oracle, mainnet, or provider execution evidence.

“Fast” means the existing default verification mode, not a lightweight Markdown-only lint. Clippy still compiles all Rust targets, agent protocol tests use local HTTP/loopback, and cold runners may download dependencies. No duration, speedup, current passing result, or test-count claim is made here.

## Proposed event and mode policy

| Event/change | Verification | Required outcome |
| --- | --- | --- |
| PR containing only approved documentation paths | `./scripts/verify.sh` | Fast checks pass; integration is visibly not run |
| Any contract, executable, fixture, manifest, toolchain, script, workflow, or web-code change | `./scripts/verify.sh --integration` | Fast and sandbox checks pass |
| Unknown or unclassifiable PR changes | `./scripts/verify.sh --integration` | No classification failure can produce a fast waiver |
| Main push | `./scripts/verify.sh --integration` | Validate the merged revision |
| Nightly main | `./scripts/verify.sh --integration` | Repeat sandbox verification on the selected main revision |
| Approved manual main run | `./scripts/verify.sh --integration` | An explicit integration run, with its revision recorded |

Keep the workflow active for every PR. Do not use workflow-level `paths` or `paths-ignore` to omit a required check: a missing run can leave a required context pending and obscure whether verification was performed.

## Conservative path classification

Use a version-controlled documentation allowlist reviewed during rollout. Candidate paths are `README.md`, `SECURITY.md`, `THREAT_MODEL.md`, `LICENSE`, `docs/**/*.md` and `docs/**/*.txt`. These are candidates for the requested docs-only fast mode, including new documentation and evidence summaries. Approval of the exact list remains necessary before implementation. Executable snippets in a Markdown document are documentation, not automatically executed by this workflow.

Require integration for the following categories:

- Anything under contracts, agent or keeper implementation and tests; sandbox fixtures; build manifests and toolchain configuration.
- Scripts, workflows and CI configuration, including changes to classification itself.
- Web HTML, JavaScript, CSS and generated web artifacts. HTML is executable UI content even when a change appears textual.
- Compiled artifacts, runtime configuration and fixtures, even if placed under a documentation directory; `.wasm`, executable HTML/SVG and JSON inputs are not granted a documentation waiver.
- New files or paths absent from the approved documentation allowlist, and any unfamiliar category.

Documentation-only edits use fast mode under the approved path list. Security claims and evidence still require review against code and retained raw logs; neither fast tests nor sandbox integration semantically verifies arbitrary prose. Keep the claims check in fast mode and label its limited scope. A maintainer-only escalation may request additional integration, but it must never downgrade a code PR to fast mode. Fork-authored labels or arbitrary PR inputs must not authorize a downgrade. If documentation becomes an input to executable behavior in a later code change, update classification in that same integration-tested change.

Determine changes between the event's exact PR base and head, not an assumed previous commit. Cover additions, deletions, both paths of renames, nested paths, and unusual filenames. Use structured data or NUL-delimited Git output; do not interpolate filenames or PR text into shell commands. For renamed files, both old and new paths must be eligible for a fast classification. Renaming a contract file into a docs path therefore requires integration.

Before selecting fast mode, prove that the entire change set was inspected. Missing base/head commits, unavailable diffs, API pagination errors, truncated responses, unexpected status values, empty/inconsistent results, and classifier exceptions all select integration. If integration itself cannot run, fail the required check. Do not catch failures and report success. A classification diagnostic should explain the fallback without logging file contents or credentials.

Load classifier logic from the protected base revision or another approved immutable source, rather than executing a PR-modified classifier to authorize its own waiver. Changes to that trusted logic require dedicated review and integration. This reduces self-waivers; it does not by itself make a PR-modifiable workflow tamper-proof. Confirm the available GitHub ruleset/required-workflow controls before claiming that a contributor cannot alter the required workflow. Where the platform supports it, require verification through a workflow sourced from a protected reference. Otherwise explicitly retain this limitation and require maintainer review of every workflow change before merge.

## Stable checks and transparent reporting

Preserve the existing `verify` job identity and workflow identity during the first rollout. The exact required status context and branch protection configuration are UNVERIFIED; inspect their actual values before editing rules. Do not assume the displayed workflow/job name is the configured requirement.

Have the always-present `verify` job choose the mode and propagate the script's exit status. If parallel jobs are later introduced, make the stable aggregate check fail when a required dependency fails, is cancelled, or is unexpectedly skipped. Only the explicitly classified fast path may omit integration. Avoid aggregate conditions that convert missing results into success.

Publish a short job summary containing the source revision, event, classification reason, selected command, and observed exit status. A fast run must say “integration not run.” It must not say “integration passed” or imply that the sandbox matrix was executed. Do not promise execution times until representative cold/warm runs are measured.

Existing formatter, clippy, claims, web checks, native tests and sandbox/gas targets remain in their current modes unless an independently approved change establishes why a check is redundant. This proposal does not weaken checks or alter the scripts.

## Fork safety and reproducibility

Use `pull_request` for testing fork code. Do not execute fork code using a privileged `pull_request_target` job. PR verification must need no repository secrets, model-provider settings, signer material, chain accounts, funded accounts or deployments. Fixtures and stand-ins must remain public and synthetic.

Keep minimal read permissions and avoid write-capable tokens. Do not upload credential-bearing artifacts. Do not promote a PR-produced binary or other artifact into a trusted nightly execution. Review cache scope and GitHub's fork-cache restrictions; trusted runs should rebuild from their own checkout, and untrusted data must not become an executable trusted input through custom cache handling.

Pin external actions to reviewed commit hashes during the approved rollout. For the version-specific cargo-near installer, use an approved immutable artifact or verify its reviewed digest where supported; do not describe the current URL alone as a supply-chain integrity check. Changes here need validation because a different installation method can affect cold-run behavior.

Hosted CI may access public package registries to obtain dependencies and needs loopback for protocol/sandbox tests. “No secrets” does not mean “no network or sockets.” Confirm that fork runs actually execute these checks without protected settings; lack of an observed fork run remains UNVERIFIED.

## Nightly main integration

Add a schedule only after approval. Proposed off-peak schedule: `17 3 * * *` (03:17 UTC daily). This cron is a proposal in this document, not an existing automation. GitHub schedules can be delayed and run from the default branch. Confirm that main is the repository default branch, and explicitly verify the selected checkout revision/ref before running. Fail rather than silently running a different branch if the expected relationship is absent.

Nightly uses the same integration command and public fixture dependencies as main push CI. It does not perform live transactions or contact a private model provider. Use bounded timeouts and record cancellations distinctly. Prevent PR activity from cancelling the nightly by separating concurrency groups. Ordinary GitHub check failures are sufficient initial visibility; no external messaging integration is authorized by this proposal.

## Approval-dependent rollout

1. Inspect the actual default branch, branch/ruleset requirements, required-check identity, fork-workflow policy and available protected required-workflow controls. Preserve existing requirements while gathering this read-only evidence.
2. Obtain approval for the final prose allowlist, escalation control, trusted classifier location, schedule, action/installer pinning and workflow change. This draft grants no rollout authorization.
3. Prepare a small workflow/classification PR after that approval. Keep `verify` stable and retain integration on main pushes. Use a reviewed protected-source classifier and fail-closed fallbacks.
4. Observe controlled docs-only, contract/code, workflow-change, unknown-path, deletion, rename, classification-failure and fork cases. Prove that a fork can run without secrets and that a classification failure either runs integration or fails, never reports fast success.
5. Observe a manual main integration run and the first scheduled run. Confirm their revisions and command outputs. Verify cancellation/timeout behavior and the required check's handling of skipped dependencies if an aggregate is used.
6. Make any branch-rule update only after separate approval and successful observed cases. Do not remove the existing required check before the replacement requirement is active and verified.
7. Review measured duration and maintenance cost before widening the documentation allowlist. Keep integration as the default for categories outside it.

## Evidence still needed

- Actual GitHub ruleset/branch-protection settings and exact required check context: inspect repository settings/API with an authorized read-only operation.
- Current default branch and CI fork policy: inspect repository settings; observe an unprivileged fork PR run.
- Correct classification and trustworthy workflow enforcement: review the approved implementation and observe the rollout cases above. No classifier implementation exists in this proposal.
- Actual fast/integration results and timing at the final revision: execute the approved commands on a suitable runner and preserve their exit statuses and revision. No tests were run for this draft.
- Scheduled execution: inspect a real scheduled run after rollout. A proposed cron or manual run alone does not prove that scheduling works.

Draft only. No repository workflow, script, branch rule, schedule or source file was modified by this proposal.
