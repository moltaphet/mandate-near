//! Native exporter for the actual Vault serializer at a582d36, before ceiling schema changes.
//! Reproduce only with that historical checkout and this temporary test-module include.
use super::*;
use near_sdk::borsh;
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::testing_env;
use std::io::Write;

const SOURCE: &str = "a582d36d0233eec2607fed85fc0cab8007a960a9";
const NOW: u64 = 1_793_448_000;
const OUTPUT: &str = "/private/tmp/mandate-hardening-2026-11-07/pre-ceiling-actual-vault";

fn actual_state(kind: OracleKind) -> Vault {
    testing_env!(
        VMContextBuilder::new()
            .current_account_id("vault.testnet".parse().unwrap())
            .predecessor_account_id("vault.testnet".parse().unwrap())
            .block_timestamp(NOW * 1_000_000_000)
            .build()
    );
    let mut mandate = mandate::tests::sample();
    if kind == OracleKind::Pyth {
        mandate.assets[0].price_feed_id = "11".repeat(32);
        mandate.assets[1].price_feed_id = "22".repeat(32);
    }
    let mut vault = Vault::new_with_oracle(
        "dao.testnet".parse().unwrap(),
        mandate.clone(),
        "signer.testnet".parse().unwrap(),
        "intents.testnet".parse().unwrap(),
        if kind == OracleKind::Pyth { "pyth-oracle.testnet" } else { "price-oracle.testnet" }
            .parse()
            .unwrap(),
        kind,
    );
    vault.paused = true;
    vault.halted = true;
    vault.strikes = 2;
    vault.agent = Some("agent.testnet".parse().unwrap());
    vault.trading = TradingState {
        last_trade_at_sec: Some(NOW - 200),
        day_start_sec: NOW - NOW % engine::SECONDS_PER_DAY,
        day_turnover_usd: 12_000_000,
        high_water_mark_usd: 99_000_000,
        signed_intent_expires_at_sec: NOW,
    };
    vault.next_proposal_id = 17;
    vault.next_execution_id = 23;
    vault.intents_key = Some("registered-public-fixture".into());
    vault.key_registered = true;
    vault.signed_intent = Some(SignedIntent {
        proposal_id: U64(16),
        payload: "expired public payload fixture".into(),
        public_key: "public fixture".into(),
        signature: "expired public signature fixture".into(),
        expires_sec: NOW,
    });
    let mut pending = mandate.clone();
    pending.max_trade_usd = U64(75_000_000);
    pending.allowed_model_ids =
        vec!["manual-v1".into(), "fixture-model".into(), "next-model-v1".into()];
    vault.pending_mandate = Some(PendingMandate {
        mandate: pending,
        effective_sec: NOW + MANDATE_DELAY_SEC,
    });
    vault.pending_agent = Some(PendingAgent {
        agent: "replacement.testnet".parse().unwrap(),
        effective_sec: NOW + AGENT_DELAY_SEC,
    });
    vault.withdrawal_seq = 41;
    vault.config_revision = 7;
    vault.execution_context = Some(ExecutionContext {
        id: U64(22),
        revision: 7,
        assets: mandate.assets,
        proposal_id: U64(16),
        price_valid_until_sec: Some(NOW - 1),
    });
    vault.last_proposal_outcome = Some(ProposalOutcome {
        proposal_id: U64(16),
        outcome: ExecutionOutcome::Rejected {
            code: "TradeTooLarge".into(),
            reason: "public fixture rejection".into(),
        },
    });
    vault.next_withdrawal_id = 11;
    vault.last_withdrawal_outcome =
        Some(WithdrawalOutcome::DebitObserved { withdrawal_id: U64(10) });
    assert_eq!(std::any::type_name::<Vault>(), "mandate_vault::Vault");
    assert_eq!(vault.mandate.allowed_model_ids, ["manual-v1", "rebalance-v1", "fixture-model"]);
    assert_eq!(
        vault.pending_mandate.as_ref().unwrap().mandate.allowed_model_ids,
        ["manual-v1", "fixture-model", "next-model-v1"]
    );
    assert!(vault.pending.is_none() && vault.in_flight.is_none());
    assert!(vault.pending_withdrawal.is_none() && vault.active_withdrawal.is_none());
    assert!(vault.withdrawal_stage.is_none() && vault.withdrawal_retry_armed.is_none());
    vault
}

#[test]
fn export_actual_current_vault_states() {
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(revision.status.success());
    assert_eq!(std::str::from_utf8(&revision.stdout).unwrap().trim(), SOURCE);
    std::fs::create_dir_all(OUTPUT).unwrap();
    for (kind, label) in [(OracleKind::PriceOracle, "price-oracle"), (OracleKind::Pyth, "pyth")] {
        let vault = actual_state(kind);
        // Serialize the ACTUAL historical Vault type, never a proposed migration decoder.
        let bytes = borsh::to_vec(&vault).unwrap();
        let roundtrip: Vault = borsh::from_slice(&bytes).unwrap();
        assert_eq!(roundtrip.mandate, vault.mandate);
        assert_eq!(roundtrip.pending_mandate, vault.pending_mandate);
        assert_eq!(roundtrip.oracle_kind, kind);
        let path = format!("{OUTPUT}/pre-ceiling-a582d36-{label}-state.borsh");
        let mut output = std::fs::OpenOptions::new().create_new(true).write(true).open(&path).unwrap();
        output.write_all(&bytes).unwrap();
        output.sync_all().unwrap();
        println!("ACTUAL_VAULT | source={SOURCE} | provider={label} | bytes={} | sha256={} | path={path}", bytes.len(), to_hex(&env::sha256(&bytes)));
    }
}
