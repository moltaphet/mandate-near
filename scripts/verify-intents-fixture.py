#!/usr/bin/env python3
"""Check the exact reviewed Intents verifier without a sandbox or network."""
import argparse
import hashlib
from pathlib import Path
import sys

FIXTURE = Path(__file__).resolve().parent.parent / "contracts/vault/tests/fixtures/intents-near-mainnet.wasm"
EXPECTED_SIZE = 1_297_551
EXPECTED_SHA256 = "a1afab98916d9f213746544dcf8e4e6bdb22c0be6bc5dfbd8ea8814c9922b547"


def verify(path):
    if path.stat().st_size != EXPECTED_SIZE:
        raise ValueError("Intents verifier fixture size does not match the reviewed fixture")
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if digest != EXPECTED_SHA256:
        raise ValueError("Intents verifier fixture SHA256 does not match the reviewed fixture")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", nargs="?", type=Path, default=FIXTURE)
    args = parser.parse_args()
    try:
        verify(args.path)
    except (OSError, ValueError) as error:
        print(f"Fixture verification failed: {error}", file=sys.stderr)
        return 1
    print(f"Reviewed Intents verifier verified: {EXPECTED_SIZE} bytes, SHA256 {EXPECTED_SHA256}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
