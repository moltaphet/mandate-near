#!/usr/bin/env python3
"""Read-only check of the testnet demo's results, using only public RPC view calls.

It prints what the vault2 contract and the Intents verifier record. It sends no
transaction and needs no key. Use it to verify the demo without a block explorer.

Usage:
  python3 -B scripts/demo-readback.py [--rpc https://test.rpc.fastnear.com]
"""

import argparse
import base64
import json
import sys
import urllib.request

VAULT = "vault2.mandate-t1.testnet"
INTENTS = "intents.mandate-t1.testnet"
SOLVER = "solver.mandate-t1.testnet"
USDC = "nep141:usdc.mandate-t1.testnet"
WNEAR = "nep141:wrap.testnet"


def view(rpc, contract, method, args=None):
    body = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "query",
        "params": {
            "request_type": "call_function",
            "finality": "final",
            "account_id": contract,
            "method_name": method,
            "args_base64": base64.b64encode(json.dumps(args or {}).encode()).decode(),
        },
    }
    req = urllib.request.Request(rpc, data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=30) as response:
        result = json.load(response)
    if "error" in result:
        raise RuntimeError(f"{method}: {result['error']}")
    return json.loads(bytes(result["result"]["result"]).decode() or "null")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--rpc", default="https://test.rpc.fastnear.com")
    args = parser.parse_args()
    rpc = args.rpc

    def show(label, value):
        print(f"{label:<44} {value}")

    print(f"Read-only check of the testnet demo via {rpc}")
    print(f"Vault: {VAULT}   Verifier: {INTENTS}")
    print()

    outcome = view(rpc, VAULT, "get_last_proposal_outcome")
    show("Last proposal outcome", json.dumps(outcome))
    show("Pending proposal", json.dumps(view(rpc, VAULT, "get_pending_proposal")))
    show("Strikes (halt at 3)", view(rpc, VAULT, "get_strikes"))
    show("Paused", view(rpc, VAULT, "is_paused"))
    show("Withdrawal status", json.dumps(view(rpc, VAULT, "get_withdrawal_status")))
    show("Last withdrawal outcome", json.dumps(view(rpc, VAULT, "get_last_withdrawal_outcome")))

    mandate = view(rpc, VAULT, "get_mandate")
    show("Allowed model ids", json.dumps(mandate.get("allowed_model_ids")))
    show("Per-trade cap (micro-USD)", mandate.get("max_trade_usd"))
    show("Pending mandate change", json.dumps(view(rpc, VAULT, "get_pending_mandate")))
    print()

    print("Verifier ledgers (base units)")
    for who in (VAULT, SOLVER):
        for token in (USDC, WNEAR):
            balance = view(rpc, INTENTS, "mt_balance_of", {"account_id": who, "token_id": token})
            show(f"{who} · {token}", balance)
    print()
    print("Each value is read from the final block, so rerunning the script shows the latest state.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:  # report, do not hide
        print(f"READBACK FAILED: {error}", file=sys.stderr)
        sys.exit(1)
