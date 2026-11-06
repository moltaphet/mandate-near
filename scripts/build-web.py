#!/usr/bin/env python3
"""Package the shared read-only reader inside the existing standalone site."""
import argparse
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "web" / "dashboard" / "live.mjs"
DESTINATION = ROOT / "web" / "site" / "audit.mjs"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    content = SOURCE.read_bytes()
    if args.check:
        if not DESTINATION.is_file() or DESTINATION.read_bytes() != content:
            print("Site audit asset is stale; run python3 -B scripts/build-web.py", file=sys.stderr)
            return 1
        print("Site audit asset matches the tested shared reader.")
    else:
        DESTINATION.write_bytes(content)
        print("Packaged the tested reader in the existing site; no publication occurred.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
