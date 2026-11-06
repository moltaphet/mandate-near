#!/usr/bin/env python3
"""Read-only NEAR deployment inspection. Never prints access-key values."""

import argparse
import base64
import hashlib
import json
import re
import sys
import urllib.error
import urllib.parse
import urllib.request

MAX_RESPONSE = 8 * 1024 * 1024
ACCOUNT = re.compile(r"^(?=.{2,64}$)[a-z0-9]+(?:[._-][a-z0-9]+)*$")
DIGEST = re.compile(r"^[0-9a-f]{64}$")
BASE58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def hash_base58(digest):
    value = int.from_bytes(digest, "big")
    encoded = ""
    while value:
        value, remainder = divmod(value, 58)
        encoded = BASE58[remainder] + encoded
    return "1" * (len(digest) - len(digest.lstrip(b"\x00"))) + encoded


def block_metadata(result):
    value = result.get("block_hash")
    height = result.get("block_height")
    if not isinstance(value, str) or not 32 <= len(value) <= 44 or any(c not in BASE58 for c in value):
        raise RuntimeError("invalid RPC block hash")
    decoded = 0
    for char in value:
        decoded = decoded * 58 + BASE58.index(char)
    byte_count = (decoded.bit_length() + 7) // 8 + len(value) - len(value.lstrip("1"))
    if byte_count != 32:
        raise RuntimeError("invalid RPC block hash")
    if type(height) is not int or not 0 <= height < 2 ** 64:
        raise RuntimeError("invalid RPC block height")
    return value, height


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, _request, _response, _code, _message, _headers, _url):
        raise RuntimeError("public RPC redirects are refused")


class MissingAccount(RuntimeError):
    """Only the requested account/block UNKNOWN_ACCOUNT shape is accepted."""


def account_id(value):
    if not isinstance(value, str) or not ACCOUNT.fullmatch(value):
        raise ValueError("invalid public NEAR account ID")
    return value


def rpc_url(value):
    parsed = urllib.parse.urlsplit(value)
    if parsed.scheme not in ("http", "https") or not parsed.hostname:
        raise ValueError("RPC must be an HTTP(S) endpoint")
    if parsed.username or parsed.password or parsed.query or parsed.fragment:
        raise ValueError("RPC URL must not contain credentials, query parameters or a fragment")
    if parsed.scheme == "http" and parsed.hostname not in ("localhost", "127.0.0.1", "::1"):
        raise ValueError("unencrypted RPC is allowed only for a local sandbox")
    return value


def permission_summary(entries):
    """Accept the RPC shape but discard every public_key and allowance value."""
    if not isinstance(entries, list) or len(entries) > 10000:
        raise ValueError("invalid account permission response")
    result = {"total": len(entries), "full_access": 0, "function_call": 0, "unknown": 0}
    for entry in entries:
        if not isinstance(entry, dict) or not isinstance(entry.get("access_key"), dict):
            raise ValueError("invalid account permission entry")
        permission = entry["access_key"].get("permission")
        if permission == "FullAccess":
            result["full_access"] += 1
        elif isinstance(permission, dict) and set(permission) == {"FunctionCall"}:
            result["function_call"] += 1
        else:
            result["unknown"] += 1
    result["no_access_keys"] = result["total"] == 0
    return result


class Rpc:
    def __init__(self, endpoint):
        self.endpoint = rpc_url(endpoint)
        self.sequence = 0
        self.opener = urllib.request.build_opener(NoRedirect())

    def request(self, method, params):
        self.sequence += 1
        request = urllib.request.Request(
            self.endpoint,
            data=json.dumps({"jsonrpc": "2.0", "id": self.sequence,
                             "method": method, "params": params}).encode(),
            headers={"Content-Type": "application/json"},
        )
        try:
            with self.opener.open(request, timeout=20) as response:
                raw = response.read(MAX_RESPONSE + 1)
        except (urllib.error.URLError, TimeoutError) as error:
            # Do not include URLs, headers, response bodies or credentials in errors.
            raise RuntimeError("public RPC request failed") from error
        if len(raw) > MAX_RESPONSE:
            raise RuntimeError("public RPC response exceeds the size limit")
        payload = json.loads(raw)
        if (not isinstance(payload, dict) or payload.get("jsonrpc") != "2.0"
                or type(payload.get("id")) is not int or payload["id"] != self.sequence):
            raise RuntimeError("invalid JSON-RPC response identity")
        error = payload.get("error")
        data = error.get("data") if isinstance(error, dict) else None
        cause = data.get("cause", {}) if isinstance(data, dict) else {}
        info = cause.get("info", {}) if isinstance(cause, dict) else {}
        if (method == "query" and params.get("request_type") == "view_account"
                and "block_id" in params and isinstance(info, dict)
                and isinstance(cause, dict) and cause.get("name") == "UNKNOWN_ACCOUNT"
                and info.get("account_id") == params["account_id"]
                and info.get("block_hash") == params["block_id"]):
            raise MissingAccount("requested account does not exist at the pinned block")
        if payload.get("error") is not None or not isinstance(payload.get("result"), dict):
            raise RuntimeError("public RPC query was refused")
        return payload["result"]

    def query(self, params):
        return self.request("query", params)

    def status(self):
        return self.request("status", [])


def pinned_query(rpc, params, block_hash, block_height):
    result = rpc.query({**params, "block_id": block_hash})
    observed_hash, observed_height = block_metadata(result)
    if observed_hash != block_hash or observed_height != block_height:
        raise RuntimeError("RPC returned a different block")
    return result


def read_view(rpc, account, method, block_hash, block_height, args=None):
    result = pinned_query(rpc, {
        "request_type": "call_function", "account_id": account_id(account),
        "method_name": method,
        "args_base64": base64.b64encode(json.dumps(args or {}).encode()).decode(),
    }, block_hash, block_height)
    values = result.get("result")
    if not isinstance(values, list) or len(values) > 65536:
        raise RuntimeError("invalid contract view response")
    if any(not isinstance(v, int) or isinstance(v, bool) or not 0 <= v <= 255 for v in values):
        raise RuntimeError("invalid contract view bytes")
    return json.loads(bytes(values))


def code_summary(rpc, account, block_hash, block_height):
    result = pinned_query(rpc, {"request_type": "view_code", "account_id": account_id(account)}, block_hash, block_height)
    code = base64.b64decode(result["code_base64"], validate=True)
    digest = hashlib.sha256(code).digest()
    if result.get("hash") != hash_base58(digest):
        raise RuntimeError("RPC code hash does not match returned WASM")
    return {"account": account, "code_hash": result["hash"],
            "wasm_bytes": len(code), "sha256": digest.hex()}


def sink_summary(rpc, vault, intents, block, height):
    if len(vault) > 62:
        raise RuntimeError("vault name is too long for fixed receipt sinks")
    summaries = []
    for prefix in ("s", "c"):
        account = account_id(prefix + "." + vault)
        try:
            pinned_query(rpc, {"request_type": "view_account", "account_id": account}, block, height)
        except MissingAccount:
            absent = True
        else:
            absent = False
        identities = read_view(rpc, intents, "public_keys_of", block, height, {"account_id": account})
        if not isinstance(identities, list) or len(identities) > 10000:
            raise RuntimeError("invalid receipt sink signing registration response")
        summaries.append({"account": account, "near_account_absent": absent,
                          "registered_intents_identity_count": len(identities)})
    return summaries


def inspect(rpc, vault, expected_sha256=None, expected_chain=None):
    account_id(vault)
    if expected_sha256 is not None and not DIGEST.fullmatch(expected_sha256):
        raise ValueError("expected WASM SHA256 must have 64 lowercase hexadecimal characters")
    chain = rpc.status().get("chain_id")
    if not isinstance(chain, str) or not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", chain):
        raise RuntimeError("invalid RPC chain identity")
    anchor = rpc.query({"request_type": "view_account", "account_id": vault, "finality": "final"})
    block, height = block_metadata(anchor)
    report = {"vault": vault, "chain_id": chain, "block_hash": block, "block_height": height,
              "read_only": True, "live_readiness": "UNVERIFIED",
              "historical_issuer_receipt_quiescence": "UNVERIFIED", "checks": {}}
    report["checks"]["expected_chain_matches"] = None if expected_chain is None else chain == expected_chain
    report["code"] = code_summary(rpc, vault, block, height)
    if anchor.get("code_hash") != report["code"]["code_hash"]:
        raise RuntimeError("account code hash does not match pinned WASM")
    entries = pinned_query(rpc, {"request_type": "view_access_key_list", "account_id": vault}, block, height)["keys"]
    report["permissions"] = permission_summary(entries)
    report["checks"]["no_bypass_access_keys"] = report["permissions"]["no_access_keys"]
    report["checks"]["expected_code_matches"] = (
        None if expected_sha256 is None else report["code"]["sha256"] == expected_sha256
    )
    try:
        owner = account_id(read_view(rpc, vault, "get_owner", block, height))
        report["owner"] = owner
        report["checks"]["distinct_owner"] = owner != vault
        externals = read_view(rpc, vault, "get_externals", block, height)
        if not isinstance(externals, list) or len(externals) != 3:
            raise ValueError("invalid configured external contracts")
        report["externals"] = {
            name: code_summary(rpc, account_id(value), block, height)
            for name, value in zip(("signer", "intents", "oracle"), externals)
        }
        report["checks"]["external_contracts_have_code"] = all(
            item["wasm_bytes"] > 0 for item in report["externals"].values()
        )
        report["paused"] = read_view(rpc, vault, "is_paused", block, height)
        report["oracle_kind"] = read_view(rpc, vault, "get_oracle_kind", block, height)
        if report["oracle_kind"] not in ("price_oracle", "pyth"):
            raise ValueError("invalid oracle provider view")
        report["withdrawal_protocol"] = read_view(rpc, vault, "get_withdrawal_protocol", block, height)
        if report["withdrawal_protocol"] != "unique_voucher_v1":
            raise ValueError("unsupported withdrawal recovery protocol")
        report["receipt_sinks"] = sink_summary(rpc, vault, externals[1], block, height)
        report["checks"]["receipt_sinks_have_no_account_or_registered_identity"] = all(
            item["near_account_absent"] and item["registered_intents_identity_count"] == 0
            for item in report["receipt_sinks"])
        report["recorded_signing_registration"] = read_view(rpc, vault, "is_key_registered", block, height)
        if not isinstance(report["paused"], bool) or not isinstance(report["recorded_signing_registration"], bool):
            raise ValueError("invalid boolean control view")
    except (RuntimeError, ValueError, KeyError, TypeError, json.JSONDecodeError):
        report["checks"]["control_views_valid"] = False
    else:
        report["checks"]["control_views_valid"] = True
    report["blocking_checks"] = [name for name, value in report["checks"].items() if value is False]
    report["unverified_obligations"] = [
        "unknown withdrawal recovery",
        "owner recoverability and governance",
        "oracle feed availability, units and trust",
        "external source/audit/admin correspondence",
        "exact vault live MPC and matched settlement",
        "real elapsed 24-hour withdrawal and failure recovery",
    ]
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rpc", required=True, type=rpc_url)
    parser.add_argument("--vault", required=True, type=account_id)
    parser.add_argument("--expected-wasm-sha256")
    parser.add_argument("--network", choices=("mainnet", "testnet", "localnet"))
    args = parser.parse_args()
    try:
        report = inspect(Rpc(args.rpc), args.vault, args.expected_wasm_sha256, args.network)
    except (ValueError, RuntimeError, KeyError, TypeError, json.JSONDecodeError):
        print("Read-only inspection failed; no deployment readiness claim was made.", file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2))
    return 1 if report["blocking_checks"] else 0


if __name__ == "__main__":
    sys.exit(main())

