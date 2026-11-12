#!/usr/bin/env python3
"""Maps each hash printed in the testnet evidence logs to the real transaction hash. Read-only.

The driver printed near-api 0.8.7's `result.transaction().get_hash()`, which hashes a transaction
rebuilt from the RPC view with the real hash placed in its `block_hash` field. For every
transaction involving the project's accounts (listed by the public FastNEAR testnet indexer),
this script recomputes that printed value and matches it. Output: one line per log entry, and
the mapping as JSON on the last line. See docs/evidence/TX-HASH-CORRECTIONS.md.

Usage: python3 -B scripts/recover-tx-hashes.py
"""
import glob, re
import base64, hashlib, json, struct, urllib.request
import base58

ACCOUNTS = ["mandate-t1.testnet", "agent.mandate-t1.testnet", "solver.mandate-t1.testnet", "oracle.mandate-t1.testnet",
            "usdc.mandate-t1.testnet", "intents.mandate-t1.testnet", "vault.mandate-t1.testnet", "vault2.mandate-t1.testnet",
            "mandate-t1.sputnikv2.testnet"]

def post(path, body):
    return json.load(urllib.request.urlopen(urllib.request.Request("https://tx.test.fastnear.com" + path,
        data=json.dumps(body).encode(), headers={"Content-Type": "application/json", "User-Agent": "mandate-review"}), timeout=90))

s = lambda b: struct.pack("<I", len(b)) + b
u64 = lambda n: struct.pack("<Q", int(n))
u128 = lambda n: int(n).to_bytes(16, "little")
def pk(k):
    kind, data = k.split(":")
    return (b"\x00" if kind == "ed25519" else b"\x01") + base58.b58decode(data)

def action(a):
    if a == "CreateAccount": return b"\x00"
    (kind, v), = a.items()
    if kind == "DeployContract": return b"\x01" + s(base64.b64decode(v["code"]))
    if kind == "FunctionCall": return b"\x02" + s(v["method_name"].encode()) + s(base64.b64decode(v["args"])) + u64(v["gas"]) + u128(v["deposit"])
    if kind == "Transfer": return b"\x03" + u128(v["deposit"])
    if kind == "AddKey":
        ak = v["access_key"]; perm = ak["permission"]
        if perm == "FullAccess": p = b"\x01"
        else:
            fc = perm["FunctionCall"]
            p = b"\x00" + (b"\x01" + u128(fc["allowance"]) if fc.get("allowance") is not None else b"\x00") + s(fc["receiver_id"].encode()) \
                + struct.pack("<I", len(fc["method_names"])) + b"".join(s(m.encode()) for m in fc["method_names"])
        return b"\x05" + pk(v["public_key"]) + u64(ak["nonce"]) + p
    raise ValueError(kind)

def projected(t, real):
    body = s(t["signer_id"].encode()) + pk(t["public_key"]) + u64(t["nonce"]) + s(t["receiver_id"].encode()) + base58.b58decode(real) \
        + struct.pack("<I", len(t["actions"])) + b"".join(action(a) for a in t["actions"])
    if int(t.get("priority_fee", 0) or 0) > 0:
        body = b"\x01" + body + u64(t["priority_fee"])
    return base58.b58encode(hashlib.sha256(body).digest()).decode()

hashes = set()
for acct in ACCOUNTS:
    for t in post("/v0/account", {"account_id": acct})["account_txs"]:
        hashes.add(t["transaction_hash"])
hl = sorted(hashes); proj = {}; info = {}
for i in range(0, len(hl), 20):
    for tx in post("/v0/transactions", {"tx_hashes": hl[i:i + 20]})["transactions"]:
        t = tx["transaction"]; real = tx["execution_outcome"]["id"]
        try:
            proj[projected(t, real)] = real
        except Exception as e:
            print("cannot project", real, e)
        a = t["actions"][0]
        info[real] = {"signer": t["signer_id"], "receiver": t["receiver_id"],
                      "method": a["FunctionCall"]["method_name"] if isinstance(a, dict) and "FunctionCall" in a else (a if isinstance(a, str) else list(a)[0]),
                      "height": tx["execution_outcome"]["block_height"]}
logs = []
pat = re.compile(r"^TX \| (.*?) \| hash (\S+)")
for f in sorted(glob.glob("docs/evidence/testnet-*/*.log")):
    for n, line in enumerate(open(f)):
        m = pat.match(line.strip())
        if m:
            logs.append({"file": f.split("evidence/")[1], "line": n + 1, "what": m.group(1), "recorded": m.group(2)})
found = {}; missing = []
for e in logs:
    real = proj.get(e["recorded"])
    if real:
        found[e["recorded"]] = real
        i = info[real]
        ok = (e["what"].split(" on ")[0] == i["method"]) if " on " in e["what"] else True
        print("OK " if ok else "?? ", e["file"].split("/")[1][:26].ljust(26), e["recorded"][:8], "->", real, i["method"], "" if ok else "(method differs)")
    else:
        missing.append(e); print("-- ", e["file"].split("/")[1][:26].ljust(26), e["recorded"][:8], "NOT FOUND", e["what"][:60])
print(f"\nmatched {len(found)} of {len(logs)} recorded hashes; chain transactions scanned: {len(hl)}")
print(json.dumps(found))
