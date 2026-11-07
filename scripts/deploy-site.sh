#!/usr/bin/env bash
# Reproduce production staging from the exact clean main commit.
# Default/--check performs local checks only. --execute-production requires
# explicit project-owner approval and invokes the operator's Vercel CLI.
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/deploy-site.sh [--check | --execute-production | --help]

--check               Validate clean main and the explicit public source files.
--execute-production  Stage those files and run vercel --prod --yes --name mandate-near.
--help                Show this usage without checking the repository.

Production execution requires explicit approval. Authenticate the Vercel CLI
through the approved operator workflow; this script loads no credential files
and prints no authentication settings. Verify mandatenear.xyz and browser assets
after the deployment. The public staging directory is retained for review.
USAGE
}

task_mode="${1:---check}"
if [[ $# -gt 1 ]]; then
  usage >&2
  exit 2
fi
case "$task_mode" in
  --help|-h) usage; exit 0 ;;
  --check|--execute-production) ;;
  *) usage >&2; exit 2 ;;
esac

task_script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
task_repo="$(git -C "$task_script_dir/.." rev-parse --show-toplevel)"
task_branch="$(git -C "$task_repo" symbolic-ref --quiet --short HEAD)"
if [[ "$task_branch" != "main" ]]; then
  printf '%s\n' 'Refused: production staging requires the main branch.' >&2
  exit 1
fi
if [[ -n "$(git -C "$task_repo" status --porcelain --untracked-files=normal)" ]]; then
  printf '%s\n' 'Refused: main must have no tracked changes or untracked files.' >&2
  exit 1
fi
task_revision="$(git -C "$task_repo" rev-parse --verify HEAD)"

# Explicit public allowlist: never archive the repository or traverse/copy
# authentication, environment, project-link, dependency or Git directories.
task_sources=(
  web/site/index.html
  web/site/site.mjs
  web/site/audit.mjs
  web/site/favicon.svg
  web/dashboard/index.html
  web/dashboard/live.mjs
  web/assets/logo.svg
  web/assets/logo-light.svg
)
for task_source in "${task_sources[@]}"; do
  task_entry="$(git -C "$task_repo" ls-tree "$task_revision" -- "$task_source")"
  case "${task_entry%% *}" in
    100644|100755) ;;
    *) printf '%s\n' 'Refused: a required public asset is missing or is not a regular Git file.' >&2; exit 1 ;;
  esac
done
if ! cmp -s \
  <(git -C "$task_repo" show "$task_revision:web/site/audit.mjs") \
  <(git -C "$task_repo" show "$task_revision:web/dashboard/live.mjs"); then
  printf '%s\n' 'Refused: site reader differs from the shared dashboard reader.' >&2
  exit 1
fi
printf 'Validated clean main commit: %s\n' "$task_revision"
if [[ "$task_mode" == "--check" ]]; then
  printf '%s\n' 'Public asset checks passed. Production execution flag was not supplied.'
  exit 0
fi
if ! command -v vercel >/dev/null 2>&1; then
  printf '%s\n' 'Refused: install the approved Vercel CLI before production execution.' >&2
  exit 1
fi

task_stage="$(mktemp -d /private/tmp/mandate-site.XXXXXX)"
mkdir -p -- "$task_stage/site" "$task_stage/dashboard" "$task_stage/assets"
for task_source in "${task_sources[@]}"; do
  task_relative="${task_source#web/}"
  git -C "$task_repo" show "$task_revision:$task_source" > "$task_stage/$task_relative"
done
# Root entrypoints match the current public fixture server; /site/index.html is
# retained for the dashboard's existing evidence link.
for task_name in index.html site.mjs audit.mjs favicon.svg; do
  cp -- "$task_stage/site/$task_name" "$task_stage/$task_name"
done
# Public static-output configuration; no existing project-link or credential
# configuration is loaded or copied by this script.
cat > "$task_stage/vercel.json" <<'JSON'
{
  "version": 2,
  "framework": null,
  "buildCommand": "",
  "installCommand": "",
  "outputDirectory": "."
}
JSON
printf 'Public deployment staging: %s\n' "$task_stage"
printf 'Source commit: %s\n' "$task_revision"
cd -- "$task_stage"
vercel --prod --yes --name mandate-near
