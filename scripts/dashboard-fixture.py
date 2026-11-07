#!/usr/bin/env python3
"""Serve the existing site and explicit public RPC fixtures; no live evidence."""
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "web" / "dashboard"
SITE = ROOT.parent / "site"
BLOCK = "3" * 44
NOW = 1793980800
ASSETS = [
    {"asset_id": "nep141:usdc.near", "decimals": 6, "price_feed_id": "b" * 64},
    {"asset_id": "nep141:wrap.near", "decimals": 24, "price_feed_id": "a" * 64},
]
RULES = {"assets": ASSETS, "stable_assets": ["nep141:usdc.near"],
         "max_trade_usd": "50000000", "max_daily_turnover_usd": "200000000",
         "max_position_bps": 6000, "max_slippage_bps": 100, "max_drawdown_bps": 1500,
         "min_trade_interval_sec": 300, "max_price_age_sec": 60,
         "allowed_model_ids": ["manual-v1", "fixture-model"]}
REQUEST = {"asset_id": "nep141:usdc.near", "receiver": "owner.near", "amount": "20000000",
           "effective_sec": NOW - 600}
VIEWS = {
    "get_chain_time_sec": NOW, "get_owner": "owner.near", "get_agent": "agent.near",
    "get_mandate": RULES, "get_externals": ["v1.signer", "intents.near", "pyth-oracle.near"],
    "get_oracle_kind": "pyth", "is_paused": True, "is_halted": False, "get_strikes": 0,
    "get_withdrawal_stage": "Dispatched", "is_withdrawal_retry_armed": False,
    "get_withdrawal_protocol": "unique_voucher_v1",
    "get_withdrawal_recovery_accounts": ["s.vault.near", "c.vault.near"],
    "is_key_registered": True, "is_in_flight": True,
    "get_trading_state": {"last_trade_at_sec": NOW - 700, "day_start_sec": NOW,
                          "day_turnover_usd": "20000000", "high_water_mark_usd": "150000000",
                          "signed_intent_expires_at_sec": 0},
    "get_pending_proposal": None, "get_signed_intent": None, "get_pending_agent": None,
    "get_pending_mandate": {"mandate": RULES, "effective_sec": NOW + 86400},
    "get_last_proposal_outcome": {"proposal_id": "4", "outcome": {
        "Rejected": {"code": "SlippageExceeded", "reason": "<img src=x onerror=alert(1)> fixture refusal"}}},
    "get_pending_withdrawal": None,
    "get_withdrawal_status": {"id": "3", "request_seq": "2", "request": REQUEST,
                              "started_sec": NOW - 600, "balance_before": "100000000"},
    "get_last_withdrawal_outcome": None,
    "mt_batch_balance_of": ["105000000", "1000000000000000000000001"],
}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def reply(self, payload, status=200, content_type="application/json"):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(payload)

    def do_GET(self):
        files = {"/": (SITE / "index.html", "text/html; charset=utf-8"),
                 "/index.html": (SITE / "index.html", "text/html; charset=utf-8"),
                 "/site/index.html": (SITE / "index.html", "text/html; charset=utf-8"),
                 "/audit.mjs": (SITE / "audit.mjs", "text/javascript; charset=utf-8"),
                 "/site.mjs": (SITE / "site.mjs", "text/javascript; charset=utf-8"),
                 "/favicon.svg": (SITE / "favicon.svg", "image/svg+xml"),
                 "/assets/logo.svg": (ROOT.parent / "assets" / "logo.svg", "image/svg+xml"),
                 "/assets/logo-light.svg": (ROOT.parent / "assets" / "logo-light.svg", "image/svg+xml"),
                 "/dashboard/": (ROOT / "index.html", "text/html; charset=utf-8"),
                 "/dashboard/index.html": (ROOT / "index.html", "text/html; charset=utf-8"),
                 "/dashboard/live.mjs": (ROOT / "live.mjs", "text/javascript; charset=utf-8"),
                 "/live.mjs": (ROOT / "live.mjs", "text/javascript; charset=utf-8")}
        if self.path not in files:
            self.reply(b"Not available", 404, "text/plain")
            return
        path, content_type = files[self.path]
        self.reply(path.read_bytes(), content_type=content_type)

    def do_POST(self):
        try:
            size = int(self.headers.get("Content-Length", "0"))
            if self.path != "/" or not 0 < size <= 16384:
                raise ValueError()
            request = json.loads(self.rfile.read(size))
            params = request.get("params", {})
            if request["method"] == "status":
                result = {"chain_id": "local-fixture"}
            elif request["method"] == "query":
                result = {"block_hash": BLOCK, "block_height": 42}
                if params.get("request_type") == "view_account":
                    result["code_hash"] = "5" * 44
                elif params.get("request_type") == "call_function":
                    result["result"] = list(json.dumps(VIEWS[params["method_name"]]).encode())
                else:
                    raise ValueError()
            else:
                raise ValueError()
            self.reply(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}).encode())
        except (ValueError, KeyError, TypeError):
            self.reply(b"Fixture request refused", 400, "text/plain")


if __name__ == "__main__":
    print("Existing site preview: http://127.0.0.1:8124 — NO LIVE EVIDENCE", flush=True)
    ThreadingHTTPServer(("127.0.0.1", 8124), Handler).serve_forever()
