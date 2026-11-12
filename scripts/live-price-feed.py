#!/usr/bin/env python3
"""Fetch NEAR and USDC prices from a public API and write them for the testnet driver.

This script sends nothing to any chain. It writes one JSON file. The driver's `price`
step reads that file (MANDATE_PRICE_JSON) and calls the testnet oracle's `set_report`,
which is a separate, approved step.

The testnet oracle (contracts/mock-oracle, built with parent-writes-only) accepts reports
only from its parent account. A price written this way is still an operator-fed testnet
value, not a verified oracle. See docs/design/testnet-decisions.md.

Two guards run before anything is written (both fail closed):
- Cross-check: NEAR and USDC from CoinGecko must agree with Kraken's last trade within
  MAX_SOURCE_DEVIATION. If Kraken cannot be read, nothing is written.
- Jump limit: a NEAR price more than MAX_JUMP away from the report on chain is refused
  unless --allow-jump is given.

Usage:
  python3 scripts/live-price-feed.py [--out target/live-prices.json] [--allow-jump] [--no-cross-check]
"""

import argparse
import json
import math
import sys
import time
import urllib.request
import base64

API = "https://api.coingecko.com/api/v3/simple/price"
IDS = "near,usd-coin"
MAX_QUOTE_AGE_SEC = 60  # the vault's max_price_age_sec on vault2; a quote older than this is useless to it
MAX_CLOCK_SKEW_SEC = 5  # a source timestamp this far in the future is refused, not written
NEAR_RANGE = (0.01, 1000.0)  # sanity bounds in USD
USDC_RANGE = (0.95, 1.05)  # a stablecoin outside this band is refused, not written
MAX_SOURCE_DEVIATION = 0.02  # CoinGecko and Kraken must agree within 2%
MAX_JUMP = 0.10  # a NEAR price more than 10% from the on-chain report needs --allow-jump
KRAKEN = "https://api.kraken.com/0/public/Ticker?pair=NEARUSD,USDCUSD"
RPC = "https://test.rpc.fastnear.com"
ORACLE = "oracle.mandate-t1.testnet"


def deviation(a: float, b: float) -> float:
    """Relative difference of a from b."""
    return abs(a - b) / b


def kraken_prices() -> dict:
    """Last trade prices from Kraken, as {"NEAR": float, "USDC": float}."""
    req = urllib.request.Request(KRAKEN, headers={"User-Agent": "mandate-price-feed/1.0"})
    with urllib.request.urlopen(req, timeout=15) as resp:
        body = json.loads(resp.read().decode("utf-8"))
    if body.get("error"):
        raise ValueError(f"Kraken error: {body['error']}")
    out = {}
    for pair, info in body["result"].items():
        name = "NEAR" if pair.startswith("NEAR") else "USDC" if pair.startswith("USDC") else None
        if name:
            out[name] = float(info["c"][0])
    if set(out) != {"NEAR", "USDC"}:
        raise ValueError("Kraken answer is missing a pair")
    return out


def onchain_price(feed: str):
    """The oracle's current report for a feed, in USD, or None when there is none."""
    args = base64.b64encode(json.dumps({"asset_id": feed}).encode()).decode()
    body = {"jsonrpc": "2.0", "id": 1, "method": "query", "params": {"request_type": "call_function",
            "finality": "final", "account_id": ORACLE, "method_name": "get_asset", "args_base64": args}}
    req = urllib.request.Request(RPC, data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=15) as resp:
        result = json.loads(bytes(json.loads(resp.read().decode())["result"]["result"]).decode())
    if not result or not result.get("reports"):
        return None
    price = result["reports"][0]["price"]
    return int(price["multiplier"]) / 10 ** price["decimals"]


def fetch() -> dict:
    url = f"{API}?ids={IDS}&vs_currencies=usd&include_last_updated_at=true"
    req = urllib.request.Request(url, headers={"User-Agent": "mandate-price-feed/1.0"})
    with urllib.request.urlopen(req, timeout=15) as resp:
        return json.loads(resp.read().decode("utf-8"))


def check_price(name: str, entry: dict, bounds: tuple, now: int) -> float:
    price = entry.get("usd")
    updated = entry.get("last_updated_at")
    if not isinstance(price, (int, float)) or not math.isfinite(price):
        raise ValueError(f"{name}: price missing or not finite")
    if not (bounds[0] <= price <= bounds[1]):
        raise ValueError(f"{name}: price {price} outside sanity bounds {bounds}")
    if not isinstance(updated, int):
        raise ValueError(f"{name}: source timestamp missing")
    if updated > now + MAX_CLOCK_SKEW_SEC:
        raise ValueError(f"{name}: source timestamp is {updated - now} s in the future")
    if now - updated > MAX_QUOTE_AGE_SEC:
        raise ValueError(f"{name}: source quote is {now - updated} s old (limit {MAX_QUOTE_AGE_SEC} s)")
    return float(price)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", default="target/live-prices.json")
    parser.add_argument("--allow-jump", action="store_true", help="accept a NEAR price far from the on-chain report")
    parser.add_argument("--no-cross-check", action="store_true", help="skip the Kraken cross-check (debugging only)")
    args = parser.parse_args()

    now = int(time.time())
    data = fetch()
    try:
        near = check_price("NEAR", data["near"], NEAR_RANGE, now)
        usdc = check_price("USDC", data["usd-coin"], USDC_RANGE, now)
    except (KeyError, ValueError) as err:
        print(f"REFUSED | {err}", file=sys.stderr)
        return 1

    if not args.no_cross_check:
        try:
            second = kraken_prices()
        except Exception as err:  # fail closed: no second source, no price
            print(f"REFUSED | cross-check source unavailable: {type(err).__name__}", file=sys.stderr)
            return 1
        for name, mine in (("NEAR", near), ("USDC", usdc)):
            if deviation(mine, second[name]) > MAX_SOURCE_DEVIATION:
                print(f"REFUSED | {name}: CoinGecko {mine} and Kraken {second[name]} differ by more than "
                      f"{MAX_SOURCE_DEVIATION:.0%}", file=sys.stderr)
                return 1
        print(f"CHECK | Kraken NEAR ${second['NEAR']:.4f}, USDC ${second['USDC']:.5f}: within {MAX_SOURCE_DEVIATION:.0%}")

    if not args.allow_jump:
        try:
            previous = onchain_price("wrap.testnet")
        except Exception as err:
            print(f"REFUSED | on-chain price unreadable for the jump check: {type(err).__name__}", file=sys.stderr)
            return 1
        if previous is not None and deviation(near, previous) > MAX_JUMP:
            print(f"REFUSED | NEAR {near} is more than {MAX_JUMP:.0%} from the on-chain {previous}; "
                  "rerun with --allow-jump if this is real", file=sys.stderr)
            return 1

    out = {
        "source": "CoinGecko simple/price (public, no key)",
        "fetched_at": now,
        "source_updated_at": {
            "NEAR": data["near"]["last_updated_at"],
            "USDC": data["usd-coin"]["last_updated_at"],
        },
        "near_usd": near,
        "usdc_usd": usdc,
        # The driver takes these as decimal strings in 8-decimal fixed point.
        "wnear_usd_e8": str(round(near * 1e8)),
        "usdc_usd_e8": str(round(usdc * 1e8)),
    }
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(out, f, indent=2)
        f.write("\n")

    print(f"WROTE | {args.out}")
    print(f"PRICE | NEAR ${near:.6f} (wNEAR e8 {out['wnear_usd_e8']}) | USDC ${usdc:.6f} (e8 {out['usdc_usd_e8']})")
    print("NOT SENT | no chain transaction was made. To write these to the testnet oracle, run the")
    print(f"           driver's price step with MANDATE_PRICE_JSON={args.out}, after approval.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
