//! Provider-focused contract wiring and authority regressions.

use near_sdk::json_types::I64;
use near_sdk::test_utils::{VMContextBuilder, accounts, get_created_receipts};
use near_sdk::{PromiseResult, testing_env};

use super::*;
use crate::mandate::tests::{USDC, WNEAR, sample};

const NOW: u64 = 1_793_697_676;

fn context(who: AccountId, deposit: u128, time: u64, result: Option<PromiseResult>) {
    testing_env!(
        VMContextBuilder::new()
            .current_account_id(accounts(0))
            .predecessor_account_id(who)
            .attached_deposit(NearToken::from_yoctonear(deposit))
            .block_timestamp(time * 1_000_000_000)
            .build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        result.into_iter().collect()
    );
}

fn pyth_mandate() -> Mandate {
    let mut rules = sample();
    rules.assets[0].price_feed_id = "a".repeat(64);
    rules.assets[1].price_feed_id = "b".repeat(64);
    rules
}

fn vault() -> Vault {
    context(accounts(1), 0, NOW, None);
    let mut vault = Vault::new_with_oracle(
        accounts(1),
        pyth_mandate(),
        MAINNET_SIGNER.parse().unwrap(),
        MAINNET_INTENTS.parse().unwrap(),
        "pyth-oracle.near".parse().unwrap(),
        OracleKind::Pyth,
    );
    context(accounts(1), 1, NOW, None);
    vault.set_agent(accounts(3));
    vault.key_registered = true;
    vault
}

fn run(vault: &mut Vault, time: u64) -> Proposal {
    context(accounts(3), 0, time, None);
    vault.propose(
        USDC.into(),
        WNEAR.into(),
        U128(20_000_000),
        U128(4 * 10_u128.pow(24)),
        "ab".repeat(32),
        "manual-v1".into(),
    );
    context(accounts(2), 0, time, None);
    drop(vault.execute());
    vault.pending.clone().unwrap()
}

fn held() -> Vec<(String, U128)> {
    vec![(WNEAR.into(), U128(10 * 10_u128.pow(24))), (USDC.into(), U128(100_000_000))]
}

fn answer(published: u64) -> Vec<u8> {
    serde_json::to_vec(&BTreeMap::from([
        (
            "a".repeat(64),
            Some(pyth::PythPrice {
                price: I64(500_000_000),
                conf: U64(100_000),
                expo: -8,
                publish_time: published as i64,
            }),
        ),
        (
            "b".repeat(64),
            Some(pyth::PythPrice {
                price: I64(100_000_000),
                conf: U64(10_000),
                expo: -8,
                publish_time: published as i64,
            }),
        ),
    ]))
    .unwrap()
}

fn failure(outcome: PromiseOrValue<ExecutionOutcome>, expected: &str) {
    match outcome {
        PromiseOrValue::Value(ExecutionOutcome::Failed { reason }) => assert_eq!(reason, expected),
        _ => panic!("expected fail-closed provider error"),
    }
}

#[test]
fn oracle_kind_serializes_as_explicit_provider_names() {
    assert_eq!(serde_json::to_value(OracleKind::PriceOracle).unwrap(), json!("price_oracle"));
    assert_eq!(serde_json::to_value(OracleKind::Pyth).unwrap(), json!("pyth"));
    assert_eq!(serde_json::from_value::<OracleKind>(json!("pyth")).unwrap(), OracleKind::Pyth);
    assert!(serde_json::from_value::<OracleKind>(json!("Pyth")).is_err());
}

#[test]
fn existing_initializers_keep_the_price_oracle_provider_and_feed_schema() {
    context(accounts(1), 0, NOW, None);
    let defaults = Vault::new(accounts(1), sample());
    assert_eq!(defaults.get_oracle_kind(), OracleKind::PriceOracle);
    assert_eq!(defaults.get_mandate(), sample());
    let custom =
        Vault::new_with_externals(accounts(1), sample(), accounts(2), accounts(3), accounts(4));
    assert_eq!(custom.get_oracle_kind(), OracleKind::PriceOracle);
    assert_eq!(custom.get_externals(), (accounts(2), accounts(3), accounts(4)));
}

#[test]
#[should_panic(expected = "Pyth price feed ids must be 64 lowercase hexadecimal characters")]
fn pyth_initializer_rejects_price_oracle_style_feed_ids() {
    context(accounts(1), 0, NOW, None);
    Vault::new_with_oracle(
        accounts(1),
        sample(),
        accounts(2),
        accounts(3),
        accounts(4),
        OracleKind::Pyth,
    );
}

#[test]
#[should_panic(expected = "Pyth price feed ids must be 64 lowercase hexadecimal characters")]
fn pyth_set_mandate_rejects_invalid_feed_ids_before_queuing() {
    let mut vault = vault();
    let mut next = vault.mandate.clone();
    next.assets[0].price_feed_id = "A".repeat(64);
    context(accounts(1), 1, NOW, None);
    vault.set_mandate(next);
}

#[test]
fn pyth_preview_rejects_generic_feed_ids_without_changing_the_vault() {
    let vault = vault();
    let original = vault.get_mandate();
    let revision = vault.config_revision;
    let preview = vault.simulate_mandate_change(sample());
    assert_eq!(
        preview,
        MandateSimulation {
            valid: false,
            error: Some("Pyth price feed ids must be 64 lowercase hexadecimal characters".into()),
            loosened_fields: vec![],
            outcome: None,
        }
    );
    assert_eq!(vault.get_mandate(), original);
    assert!(vault.get_pending_mandate().is_none());
    assert_eq!(vault.config_revision, revision);
}

#[test]
fn pyth_preview_matches_applied_and_queued_setter_outcomes_for_valid_feeds() {
    let mut vault = vault();
    let original = vault.get_mandate();
    let unchanged = vault.simulate_mandate_change(original.clone());
    assert!(unchanged.valid);
    assert!(unchanged.error.is_none());
    assert!(unchanged.loosened_fields.is_empty());
    context(accounts(1), 1, NOW, None);
    assert_eq!(unchanged.outcome, Some(vault.set_mandate(original.clone())));

    let mut changed = original.clone();
    changed.assets[0].price_feed_id = "c".repeat(64);
    let preview = vault.simulate_mandate_change(changed.clone());
    assert!(preview.valid);
    assert!(preview.error.is_none());
    assert!(!preview.loosened_fields.is_empty());
    assert_eq!(
        preview.outcome,
        Some(MandateChangeOutcome::Queued { effective_sec: U64(NOW + MANDATE_DELAY_SEC) })
    );
    assert_eq!(preview.outcome, Some(vault.set_mandate(changed.clone())));
    assert_eq!(vault.get_mandate(), original);
    assert_eq!(vault.get_pending_mandate().unwrap().mandate, changed);
}

#[test]
#[should_panic(expected = "Pyth price feed ids must be 64 lowercase hexadecimal characters")]
fn pyth_apply_mandate_revalidates_provider_feed_ids() {
    let mut vault = vault();
    vault.pending_mandate = Some(PendingMandate { mandate: sample(), effective_sec: NOW });
    context(accounts(2), 0, NOW, None);
    vault.apply_pending_mandate();
}

#[test]
fn pyth_feed_changes_keep_the_existing_24_hour_loosening_delay() {
    let mut vault = vault();
    let original = vault.mandate.clone();
    let mut next = original.clone();
    next.assets[0].price_feed_id = "c".repeat(64);
    context(accounts(1), 1, NOW, None);
    assert_eq!(
        vault.set_mandate(next.clone()),
        MandateChangeOutcome::Queued { effective_sec: U64(NOW + MANDATE_DELAY_SEC) }
    );
    assert_eq!(vault.mandate, original);
    context(accounts(2), 0, NOW + MANDATE_DELAY_SEC, None);
    vault.apply_pending_mandate();
    assert_eq!(vault.mandate, next);
    assert_eq!(vault.get_oracle_kind(), OracleKind::Pyth);
}

#[test]
fn pyth_balance_callback_sends_the_real_batch_method_and_arguments() {
    let mut vault = vault();
    let proposal = run(&mut vault, NOW);
    context(accounts(0), 0, NOW, None);
    drop(vault.on_balances(
        Ok(held().into_iter().map(|(_, amount)| amount).collect()),
        proposal,
        U64(1),
    ));
    let mut found = false;
    for receipt in get_created_receipts() {
        for action in receipt.actions {
            if let near_sdk::mock::MockAction::FunctionCallWeight {
                method_name,
                args,
                attached_deposit,
                ..
            } = action
            {
                if method_name == b"list_prices_no_older_than" {
                    found = true;
                    assert_eq!(receipt.receiver_id.as_str(), "pyth-oracle.near");
                    assert_eq!(attached_deposit, NearToken::from_yoctonear(0));
                    assert_eq!(
                        serde_json::from_slice::<serde_json::Value>(&args).unwrap(),
                        json!({"price_ids":["a".repeat(64),"b".repeat(64)],"age":60})
                    );
                }
                assert_ne!(method_name, b"get_price_data");
                assert_ne!(method_name, b"get_asset");
            }
        }
    }
    assert!(found, "actual Pyth batch call must be dispatched");
}

#[test]
fn pyth_approved_callback_records_price_deadline_without_early_quota_commit() {
    let mut vault = vault();
    let proposal = run(&mut vault, NOW);
    context(accounts(0), 0, NOW, Some(PromiseResult::Successful(answer(NOW - 10))));
    assert!(matches!(vault.on_pyth_prices(proposal, held(), U64(1)), PromiseOrValue::Promise(_)));
    assert_eq!(vault.execution_context.as_ref().unwrap().price_valid_until_sec, Some(NOW + 50));
    assert_eq!(vault.trading.day_turnover_usd, 0);
    assert_eq!(vault.strikes, 0);
    assert!(vault.signed_intent.is_none());
}

#[test]
fn null_pyth_cache_rejects_exactly_once_without_a_signature_or_quota_reservation() {
    let mut vault = vault();
    let proposal = run(&mut vault, NOW);
    let bytes = serde_json::to_vec(&json!({"a".repeat(64):null,"b".repeat(64):null})).unwrap();
    context(accounts(0), 0, NOW, Some(PromiseResult::Successful(bytes)));
    assert!(matches!(
        vault.on_pyth_prices(proposal, held(), U64(1)),
        PromiseOrValue::Value(ExecutionOutcome::Rejected { code, .. }) if code == "MissingPrice"
    ));
    assert_eq!(vault.strikes, 1);
    assert!(vault.pending.is_none());
    assert!(vault.in_flight.is_none());
    assert!(vault.signed_intent.is_none());
    assert_eq!(vault.trading.day_turnover_usd, 0);
}

#[test]
fn pyth_failed_or_oversized_read_releases_only_its_unsigned_execution() {
    for result in
        [PromiseResult::Failed, PromiseResult::Successful(vec![b'x'; MAX_ANSWER_BYTES + 1])]
    {
        let oversized = matches!(&result, PromiseResult::Successful(_));
        let mut vault = vault();
        let proposal = run(&mut vault, NOW);
        context(accounts(0), 0, NOW, Some(result));
        failure(
            vault.on_pyth_prices(proposal, held(), U64(1)),
            if oversized {
                "cross-contract read 0 exceeded the answer limit"
            } else {
                "cross-contract read 0 failed"
            },
        );
        assert!(vault.in_flight.is_none());
        assert!(vault.pending.is_some());
        assert_eq!(vault.trading.day_turnover_usd, 0);
        assert_eq!(vault.strikes, 0);
    }
}

#[test]
fn pyth_callback_cannot_use_a_previous_configuration_or_expired_window() {
    let mut vault = vault();
    let proposal = run(&mut vault, NOW);
    context(accounts(1), 1, NOW + 1, None);
    vault.set_mandate(Mandate { max_trade_usd: U64(10_000_000), ..vault.mandate.clone() });
    context(accounts(0), 0, NOW + 2, Some(PromiseResult::Successful(answer(NOW))));
    failure(vault.on_pyth_prices(proposal, held(), U64(1)), "the execution configuration changed");
    assert_eq!(vault.trading.day_turnover_usd, 0);

    let mut vault = crate::provider_tests::vault();
    let proposal = run(&mut vault, NOW);
    context(
        accounts(0),
        0,
        NOW + EXECUTION_WINDOW_SEC,
        Some(PromiseResult::Successful(answer(NOW + EXECUTION_WINDOW_SEC))),
    );
    failure(vault.on_pyth_prices(proposal, held(), U64(1)), "the execution signing window expired");
    assert!(vault.in_flight.is_none());
}

#[test]
fn stale_pyth_callback_cannot_change_a_new_execution() {
    let mut vault = vault();
    let old = run(&mut vault, NOW);
    let time = NOW + EXECUTION_WINDOW_SEC + INTENT_TTL_SEC;
    let new = run(&mut vault, time);
    context(accounts(0), 0, time, None);
    failure(vault.on_pyth_prices(old, held(), U64(1)), "superseded by a newer execution");
    assert_eq!(vault.in_flight.as_ref().unwrap().id, U64(2));
    assert_eq!(vault.pending.as_ref().unwrap().id, new.id);
    assert_eq!(vault.trading.day_turnover_usd, 0);
    assert_eq!(vault.strikes, 0);
}
