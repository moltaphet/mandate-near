#!/usr/bin/env bash
# Checks the names and paths that README.md, SECURITY.md, THREAT_MODEL.md and docs/
# cite in backticks. A name must appear as an identifier in the Rust sources (or in
# the scripts), and a repo path must exist. This catches a renamed test or a moved
# file. It does not prove the claim: run the named test for that.
#
#   ./scripts/check-claims.sh    exits 1 and lists every name or path that is missing
set -euo pipefail

cd "$(dirname "$0")/.."

docs=(README.md SECURITY.md THREAT_MODEL.md docs/*.md)
missing=0

# Names defined in other contracts or in the OutLayer design, cited on purpose.
external=" execute_intents auth_by_predecessor_id version_key "

# snake_case identifiers in backticks, e.g. `owner_controls_on_a_real_chain`.
for name in $(grep -ohE '`[a-z][a-z0-9]*(_[a-z0-9]+)+`' "${docs[@]}" | tr -d '`' | sort -u); do
  case "$external" in *" $name "*) continue ;; esac
  if ! grep -rqw --include='*.rs' --include='*.sh' "$name" contracts agent keeper scripts; then
    echo "missing identifier: $name"
    missing=1
  fi
done

# Repo paths in backticks, e.g. `scripts/verify.sh` or `docs/TESTNET.md`.
for path in $(grep -ohE '`(scripts|docs|contracts|agent|keeper|spikes|web)/[A-Za-z0-9_./-]+`' "${docs[@]}" | tr -d '`' | sort -u); do
  if [ ! -e "$path" ]; then
    echo "missing path: $path"
    missing=1
  fi
done

if [ "$missing" -eq 0 ]; then
  echo "All cited names and paths exist."
fi
exit "$missing"
