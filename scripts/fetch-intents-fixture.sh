#!/usr/bin/env bash
# Re-download the real intents.near contract from NEAR mainnet and check it
# against the on-chain code hash. The sandbox tests deploy this exact wasm.
#
# Usage: scripts/fetch-intents-fixture.sh [output-file]
# Needs: curl, python3. Read-only: one public RPC call, no keys.
set -euo pipefail

out="${1:-$(cd "$(dirname "$0")/.." && pwd)/contracts/vault/tests/fixtures/intents-near-mainnet.wasm}"
rpc="${NEAR_RPC:-https://free.rpc.fastnear.com}"

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

curl -fsS "$rpc" -H 'Content-Type: application/json' -o "$tmp" -d '{
  "jsonrpc": "2.0", "id": 1, "method": "query",
  "params": { "request_type": "view_code", "finality": "final", "account_id": "intents.near" }
}'

python3 - "$tmp" "$out" <<'PY'
import base64, hashlib, json, sys

ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

def base58(raw: bytes) -> str:
    n = int.from_bytes(raw, "big")
    out = ""
    while n:
        n, r = divmod(n, 58)
        out = ALPHABET[r] + out
    return "1" * (len(raw) - len(raw.lstrip(b"\0"))) + out

with open(sys.argv[1], encoding="utf-8") as f:
    result = json.load(f)["result"]
code = base64.b64decode(result["code_base64"])
digest = hashlib.sha256(code).digest()
local, onchain = base58(digest), result["hash"]
if local != onchain:
    sys.exit(f"MISMATCH: sha256 base58 {local} != on-chain code hash {onchain}")
with open(sys.argv[2], "wb") as f:
    f.write(code)
print(f"ok  {len(code)} bytes  sha256 {digest.hex()}")
print(f"    on-chain code hash {onchain}  at block {result['block_height']}")
PY
