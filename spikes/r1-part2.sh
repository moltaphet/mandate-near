#!/usr/bin/env bash
# R1 part 2: contract -> MPC (testnet) -> intents.near simulate (mainnet).
#
# Usage: spikes/r1-part2.sh <spike-contract.testnet>
# Needs: near-cli-rs (`near`), a deployed spikes/r1-mpc-contract signed for
# in the legacy keychain, python3. Costs a little testnet NEAR; nothing on mainnet.
set -euo pipefail

contract="${1:?usage: $0 <spike-contract.testnet>}"
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
tool="$here/r1-intents-signing/target/release/r1-intents-signing"
testnet_rpc="https://test.rpc.fastnear.com"

(cd "$here/r1-intents-signing" && cargo build --release -q)

view() { # view <rpc> <account> <method> <json-args>
  curl -s "$1" -H 'Content-Type: application/json' -d "$(python3 - "$2" "$3" "$4" <<'EOF'
import base64, json, sys
print(json.dumps({"jsonrpc": "2.0", "id": 1, "method": "query", "params": {
  "request_type": "call_function", "finality": "final", "account_id": sys.argv[1],
  "method_name": sys.argv[2], "args_base64": base64.b64encode(sys.argv[3].encode()).decode()}}))
EOF
)" | python3 -c 'import sys, json; print(json.loads(bytes(json.load(sys.stdin)["result"]["result"])))'
}

pk="$(view "$testnet_rpc" v1.signer-prod.testnet derived_public_key \
  "{\"path\":\"intents\",\"predecessor\":\"$contract\",\"domain_id\":1}")"
echo "MPC-derived key for $contract (path 'intents'): $pk"
"$tool" prepare "$pk" "$work"

mpc_sign() { # mpc_sign <hex> -> MPC response JSON on stdout
  near contract call-function as-transaction "$contract" request_signature \
    json-args "{\"payload_hex\":\"$1\",\"path\":\"intents\"}" \
    prepaid-gas '100 Tgas' attached-deposit '0 NEAR' sign-as "$contract" \
    network-config testnet sign-with-legacy-keychain send 2>&1 \
  | python3 -c '
import json, re, sys
text = re.sub(r"\x1b\[[0-9;]*m", "", sys.stdin.read())
start = text.find("{\n  \"scheme\"")
obj, _ = json.JSONDecoder().raw_decode(text[start:])
print(json.dumps(obj, separators=(",", ":")))'
}

status=0
for case in "empty:ok" "token_diff:balance"; do
  name="${case%%:*}"; expect="${case#*:}"
  echo; echo "== $name: requesting MPC signature via $contract"
  response="$(mpc_sign "$(cat "$work/$name.hex")")"
  "$tool" submit "$pk" "$work/$name.json" "$response" "$expect" || status=1
done
exit $status
