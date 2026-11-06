#!/usr/bin/env python3
"""Generate immutable migration inputs by compiling the actual historical vaults.

Only the six explicit, permitted Rust source paths are read with git show.
Builds run offline in a temporary Cargo workspace; no current LegacyVault schema
is imported or serialized. The original Vault methods create the state.
"""

import hashlib
import pathlib
import subprocess
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[4]
DESTINATION = pathlib.Path(__file__).resolve().parent
COMMITS = {
    "preseq": "77ebc0c16a9f4fdb398c3827ba1f88dabe0eea0b",
    "seq": "0de5c48888ff495d017f1daadd1c6ba1e8b5039b",
}
SOURCES = ("lib.rs", "engine.rs", "intent.rs", "mandate.rs", "math.rs", "oracle.rs")

MAIN = r'''
use near_sdk::json_types::{U64, U128};
use near_sdk::serde_json::json;
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::{AccountId, NearToken, env, testing_env};

const NOW: u64 = 1_793_697_676;

fn context(deposit: u128) {
    testing_env!(
        VMContextBuilder::new()
            .current_account_id("vault.testnet".parse().unwrap())
            .predecessor_account_id("owner.testnet".parse().unwrap())
            .signer_account_id("owner.testnet".parse().unwrap())
            .attached_deposit(NearToken::from_yoctonear(deposit))
            .block_timestamp(NOW * 1_000_000_000)
            .build()
    );
}

macro_rules! original_state {
    ($original:ident, $filename:literal, $out:expr) => {{
        context(0);
        let owner: AccountId = "owner.testnet".parse().unwrap();
        let mandate: $original::Mandate = near_sdk::serde_json::from_value(json!({
            "assets": [
                { "asset_id": "nep141:wrap.near", "decimals": 24,
                  "price_feed_id": "wrap.near" },
                { "asset_id": "nep141:usdc.testnet", "decimals": 6,
                  "price_feed_id": "usdc" }
            ],
            "stable_assets": ["nep141:usdc.testnet"],
            "max_trade_usd": "50000000",
            "max_daily_turnover_usd": "200000000",
            "max_position_bps": 6000,
            "max_slippage_bps": 100,
            "max_drawdown_bps": 1500,
            "min_trade_interval_sec": 300,
            "max_price_age_sec": 60
        })).unwrap();
        let mut vault = $original::Vault::new(owner, mandate.clone());
        context(1);
        vault.set_agent("agent.testnet".parse().unwrap());
        vault.set_agent("replacement.testnet".parse().unwrap());
        let mut loosened = mandate;
        loosened.max_trade_usd = U64(75_000_000);
        assert!(matches!(
            vault.set_mandate(loosened),
            $original::MandateChangeOutcome::Queued { .. }
        ));
        // Generate a nonzero historical withdrawal sequence using only public
        // controls, then leave the vault quiescent for migration.
        vault.request_withdrawal(
            "nep141:usdc.testnet".into(),
            "receiver.testnet".parse().unwrap(),
            U128(7_000_000),
        );
        vault.cancel_pending_withdrawal();
        context(0);
        vault.pause();
        assert!(vault.is_paused());
        assert!(vault.get_pending_withdrawal().is_none());
        env::state_write(&vault);
        let bytes = env::storage_read(b"STATE").unwrap();
        std::fs::write($out.join($filename), bytes).unwrap();
    }};
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("output folder"));
    original_state!(historical_preseq, "legacy-77ebc0c-state.borsh", out);
    original_state!(historical_seq, "legacy-0de5c48-state.borsh", out);
}
'''


def main():
    source_hashes = []
    with tempfile.TemporaryDirectory(prefix="mandate-legacy-fixtures-", dir="/private/tmp") as folder:
        harness = pathlib.Path(folder)
        (harness / "Cargo.toml").write_text(
            '[workspace]\nresolver = "3"\nmembers = ["preseq", "seq", "generate"]\n'
        )
        for label, commit in COMMITS.items():
            crate = harness / label
            (crate / "src").mkdir(parents=True)
            (crate / "Cargo.toml").write_text(
                '[package]\n'
                f'name = "historical-{label}"\n'
                'version = "0.0.0"\nedition = "2024"\n'
                '[dependencies]\n'
                'near-sdk = { version = "=5.30.0", features = ["unit-testing"] }\n'
                'uint = { version = "=0.10.2", default-features = false }\n'
                'bs58 = "0.5"\n'
                '[lints.rust.unexpected_cfgs]\nlevel = "warn"\ncheck-cfg = ["cfg(near)"]\n'
            )
            for name in SOURCES:
                source_path = f"contracts/vault/src/{name}"
                data = subprocess.check_output(
                    ["git", "show", f"{commit}:{source_path}"], cwd=ROOT
                )
                (crate / "src" / name).write_bytes(data)
                source_hashes.append((commit, source_path, hashlib.sha256(data).hexdigest()))

        generator = harness / "generate"
        (generator / "src").mkdir(parents=True)
        (generator / "src" / "main.rs").write_text(MAIN)
        (generator / "Cargo.toml").write_text(
            '[package]\nname = "generate-legacy-state"\nversion = "0.0.0"\nedition = "2024"\n'
            '[dependencies]\n'
            'near-sdk = { version = "=5.30.0", features = ["unit-testing"] }\n'
            'historical_preseq = { package = "historical-preseq", path = "../preseq" }\n'
            'historical_seq = { package = "historical-seq", path = "../seq" }\n'
        )
        subprocess.run(
            ["cargo", "+1.95.0", "run", "--offline", "-p", "generate-legacy-state", "--", str(DESTINATION)],
            cwd=harness,
            check=True,
        )

    lines = [
        "# Historical STATE fixtures",
        "",
        "These fixtures are generated by compiling the unmodified Rust vault sources at each",
        "named commit with Rust 1.95.0 and near-sdk 5.30.0, then calling the original Vault::new, set_agent",
        "(initial and queued replacement), set_mandate (queued loosening), request_withdrawal,",
        "cancel_pending_withdrawal, and pause methods in the NEAR unit VM at Unix second",
        "1793697676. env::state_write serializes the original Vault implementation.",
        "",
        "No migration LegacyVault structs or current vault source participate in generation.",
        "The active mandate stays at 50 USD, the queued mandate is 75 USD, both queued control",
        "deadlines are 1793784076, and the sequence-aware source produces withdrawal_seq=2.",
        "Only public fixture account names and configuration are stored; no credentials exist",
        "in either fixture. These are unit-VM state fixtures, not deployed-chain upgrade evidence.",
        "",
        "Reproduce from the repository root:",
        "",
        "```text",
        "python3 contracts/vault/tests/fixtures/generate-legacy-state.py",
        "```",
        "",
        "The script reads only the six explicitly listed historical Rust paths, builds offline",
        "in an isolated /private/tmp Cargo workspace, and writes the two fixture files here.",
        "",
        "## Immutable fixture SHA-256",
        "",
    ]
    for commit in COMMITS.values():
        filename = f"legacy-{commit[:7]}-state.borsh"
        fixture = DESTINATION / filename
        lines.append(f"- {filename}: `{hashlib.sha256(fixture.read_bytes()).hexdigest()}` ({fixture.stat().st_size} bytes)")
    lines.extend(["", "## Original source SHA-256", ""])
    for commit, path, digest in source_hashes:
        lines.append(f"- {commit}:{path}: `{digest}`")
    (DESTINATION / "legacy-state-fixtures.md").write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
