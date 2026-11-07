#!/usr/bin/env bash
# Read-only check that the published site matches the repository's current claims.
# It fetches the page and its assets and sends nothing else. Usage:
#   scripts/check-site-live.sh [base-url]        (default https://mandatenear.xyz)
set -euo pipefail

base="${1:-https://mandatenear.xyz}"
base="${base%/}"
page="$(mktemp)"
trap 'rm -f "$page"' EXIT
status=0

code="$(curl -sS -m 30 -o "$page" -w '%{http_code}' "$base/")"
if [[ "$code" != "200" ]]; then
  printf 'FAIL root returned HTTP %s\n' "$code" >&2
  exit 1
fi

# Wording that must not appear: claims the project has withdrawn.
for stale in 'Pyth is selected' 'NEAR Hackathon 2026' 'hackathon scope'; do
  if grep -qF "$stale" "$page"; then
    printf 'FAIL stale text on the live page: %s\n' "$stale" >&2
    status=1
  fi
done

# Wording that must appear: the current oracle status.
if ! grep -qF 'not yet selected' "$page"; then
  printf 'FAIL the live page does not state that the oracle is not yet selected\n' >&2
  status=1
fi

# Assets the page needs.
for path in /assets/logo.svg /assets/logo-light.svg /dashboard/ /site.mjs /audit.mjs; do
  asset="$(curl -sS -m 30 -o /dev/null -w '%{http_code}' "$base$path")"
  if [[ "$asset" != "200" ]]; then
    printf 'FAIL %s returned HTTP %s\n' "$path" "$asset" >&2
    status=1
  fi
done

if [[ "$status" -eq 0 ]]; then
  printf 'OK %s matches the repository claims\n' "$base"
fi
exit "$status"
