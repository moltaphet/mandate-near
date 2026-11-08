"""Independent v1 commitment vectors: manual Borsh primitives, no project imports."""

import hashlib
import struct


def string(value):
    encoded = value.encode("utf-8")
    return struct.pack("<I", len(encoded)) + encoded


def commitment(executor, vault, domain, operation, sequence, token, receiver, amount):
    preimage = (
        b"mandate-withdrawal-journal-request-v1\0"
        + string("durable_withdrawal_journal_v1")
        + bytes(domain)
        + string(executor)
        + string(vault)
        + operation.to_bytes(8, "little")
        + sequence.to_bytes(8, "little")
        + string(token)
        + string(receiver)
        + amount.to_bytes(16, "little")
    )
    return hashlib.sha256(preimage).hexdigest()


assert commitment(
    "intents.sandbox", "vault.sandbox", [17] * 32, 7, 3,
    "usdc.sandbox", "receiver.sandbox", 1_000_000,
) == "bec91f82f8e4c69f613a54cffab6fc584545e7e726102dd4b0ad679115c3e25e"
assert commitment(
    "intents.testnet", "vault.testnet", list(range(32)), 2**64 - 1, 2**64 - 1,
    "token.testnet", "alice.testnet", 2**128 - 1,
) == "84fcb6eff15010093ea81bc75cf3ad9ebfb2dda265e73f1e4c5e946a283cc56e"
print("Two independent manual-Borsh SHA256 vectors verified.")
