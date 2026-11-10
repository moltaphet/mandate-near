#!/usr/bin/env python3
"""Fetch NEAR and USDC prices from a public API and write them for the testnet driver.

This script sends nothing to any chain. It writes one JSON file. The driver's `price`
step reads that file (MANDATE_PRICE_JSON) and calls the testnet oracle's `set_report`,
which is a separate, approved step.

The testnet oracle (contracts/mock-oracle) accepts a report from any caller. A price
written this way is therefore an operator-fed testnet value, not a verified oracle.
See docs/design/testnet-decisions.md.

Usage:
  python3 scripts/live-price-feed.py [--out target/live-prices.json]
"""

import argparse
import json
import math
import sys
import time
import urllib.request

API = "https://api.coingecko.com/api/v3/simple/price"
IDS = "near,usd-coin"
MAX_QUOTE_AGE_SEC = 60  # the vault's max_price_age_sec on vault2; a quote older than this is useless to it
MAX_CLOCK_SKEW_SEC = 5  # a source timestamp this far in the future is refused, not written
NEAR_RANGE = (0.01, 1000.0)  # sanity bounds in USD
USDC_RANGE = (0.95, 1.05)  # a stablecoin outside this band is refused, not written


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
    args = parser.parse_args()

    now = int(time.time())
    data = fetch()
    try:
        near = check_price("NEAR", data["near"], NEAR_RANGE, now)
        usdc = check_price("USDC", data["usd-coin"], USDC_RANGE, now)
    except (KeyError, ValueError) as err:
        print(f"REFUSED | {err}", file=sys.stderr)
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
