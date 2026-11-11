//! Adversarial regressions for asynchronous ownership and recovery boundaries.
use super::*;
use near_sdk::test_utils::{VMContextBuilder, accounts};
use near_sdk::{PromiseResult, testing_env};

const T: u64 = 1_793_448_000;

fn context(who: AccountId, deposit: u128, time: u64, result: Option<PromiseResult>) {
    let mut builder = VMContextBuilder::new();
    builder
        .current_account_id(accounts(0))
        .predecessor_account_id(who)
        .attached_deposit(NearToken::from_yoctonear(deposit))
        .block_timestamp(time * 1_000_000_000);
    testing_env!(
        builder.build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        result.into_iter().collect()
    );
}

fn setup() -> Vault {
    context(accounts(1), 0, T, None);
    let mut v = Vault::new(accounts(1), mandate::tests::sample());
    context(accounts(1), 1, T, None);
    v.set_agent(accounts(3));
    v.key_registered = true;
    v.intents_key = Some("ed25519:fixture".into());
    v
}

#[test]
fn execution_gas_boundary_rejects_before_state_and_accepts_equality() {
    let mut v = setup();
    context(accounts(3), 0, T, None);
    v.propose(
        mandate::tests::USDC.into(),
        mandate::tests::WNEAR.into(),
        U128(20_000_000),
        U128(3_960_000_000_000_000_000_000_000),
        "ab".repeat(32),
        "manual-v1".into(),
    );
    let before = near_sdk::borsh::to_vec(&v).unwrap();
    let mut builder = VMContextBuilder::new();
    builder
        .current_account_id(accounts(0))
        .predecessor_account_id(accounts(2))
        .block_timestamp(T * 1_000_000_000)
        .prepaid_gas(Gas::from_gas(Gas::from_tgas(MIN_EXECUTION_GAS_TGAS).as_gas() - 1));
    testing_env!(builder.build());
    let refusal = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(v.execute())))
        .expect_err("one gas unit below the minimum must be refused");
    let message = refusal
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| refusal.downcast_ref::<&str>().copied());
    assert_eq!(message, Some("execution requires at least 300 TGas"));
    assert_eq!(near_sdk::borsh::to_vec(&v).unwrap(), before);
    assert!(near_sdk::test_utils::get_created_receipts().is_empty());

    builder.prepaid_gas(Gas::from_tgas(MIN_EXECUTION_GAS_TGAS));
    testing_env!(builder.build());
    drop(v.execute());
    assert!(v.is_in_flight());
    assert_eq!(v.next_execution_id, 2);
    assert!(!near_sdk::test_utils::get_created_receipts().is_empty());
}

fn run(v: &mut Vault) -> Proposal {
    context(accounts(3), 0, T, None);
    v.propose(
        mandate::tests::USDC.into(),
        mandate::tests::WNEAR.into(),
        U128(20_000_000),
        U128(3_960_000_000_000_000_000_000_000),
        "ab".repeat(32),
        "manual-v1".into(),
    );
    context(accounts(2), 0, T, None);
    let _ = v.execute();
    let proposal = v.pending.clone().unwrap();
    gate_approval(v, proposal.clone());
    proposal
}

fn approved() -> TradingState {
    TradingState { signed_intent_expires_at_sec: T + INTENT_TTL_SEC, ..TradingState::default() }
}

fn failed(value: PromiseOrValue<ExecutionOutcome>, expected: &str) {
    match value {
        PromiseOrValue::Value(ExecutionOutcome::Failed { reason }) => assert_eq!(reason, expected),
        _ => panic!("expected fail-closed result: {expected}"),
    }
}

#[test]
fn tightening_invalidates_an_unsigned_approval() {
    let mut v = setup();
    let proposal = run(&mut v);
    context(accounts(1), 1, T + 1, None);
    v.set_mandate(Mandate { max_trade_usd: U64(10_000_000), ..v.mandate.clone() });
    context(accounts(0), 0, T + 2, None);
    failed(
        v.on_salt(Ok("00000000".into()), proposal, approved(), U64(1)),
        "the execution configuration changed",
    );
    assert!(v.signed_intent.is_none());
    assert!(!v.is_in_flight());
}

#[test]
fn asset_reordering_cannot_relabel_an_old_balance_vector() {
    let mut v = setup();
    let proposal = run(&mut v);
    context(accounts(1), 1, T + 1, None);
    let mut reordered = v.mandate.clone();
    reordered.assets.reverse();
    assert_eq!(v.set_mandate(reordered), MandateChangeOutcome::Applied);
    context(accounts(0), 0, T + 2, None);
    failed(
        v.on_balances(Ok(vec![U128(60), U128(40)]), proposal, U64(1)),
        "the execution configuration changed",
    );
    assert_eq!(v.strikes, 0);
}

#[test]
fn pause_then_unpause_does_not_revive_unsigned_approval() {
    let mut v = setup();
    let proposal = run(&mut v);
    context(accounts(1), 0, T + 1, None);
    v.pause();
    context(accounts(1), 1, T + 2, None);
    v.unpause();
    context(accounts(0), 0, T + 3, None);
    failed(
        v.on_salt(Ok("00000000".into()), proposal, approved(), U64(1)),
        "the execution configuration changed",
    );
}

#[test]
fn configuration_change_does_not_free_an_already_requested_signature() {
    let mut v = setup();
    let proposal = run(&mut v);
    context(accounts(0), 0, T + 1, None);
    assert!(matches!(
        v.on_salt(Ok("00000000".into()), proposal.clone(), approved(), U64(1)),
        PromiseOrValue::Promise(_)
    ));
    context(accounts(1), 0, T + 2, None);
    v.pause();
    assert!(v.is_in_flight());
    context(accounts(0), 0, T + 3, None);
    let outcome = v.on_signed(
        Ok(SignatureResponse { scheme: "Ed25519".into(), signature: vec![0; 64] }),
        proposal.clone(),
        "fixture".into(),
        approved(),
        U64(1),
    );
    assert!(matches!(outcome, ExecutionOutcome::Executed { .. }));
    assert_eq!(v.last_proposal_outcome.as_ref().unwrap().proposal_id, proposal.id);
    assert_eq!(v.trading.signed_intent_expires_at_sec, T + INTENT_TTL_SEC);
}

#[test]
fn expired_lock_callback_cannot_erase_a_new_unexecuted_proposal() {
    let mut v = setup();
    let old = run(&mut v);
    context(accounts(3), 0, T + EXECUTION_WINDOW_SEC + INTENT_TTL_SEC, None);
    let new = v.propose(
        mandate::tests::USDC.into(),
        mandate::tests::WNEAR.into(),
        U128(1),
        U128(1),
        "ab".repeat(32),
        "manual-v1".into(),
    );
    context(accounts(0), 0, T + EXECUTION_WINDOW_SEC + INTENT_TTL_SEC, None);
    assert_eq!(
        v.on_signed(
            Ok(SignatureResponse { scheme: "Ed25519".into(), signature: vec![0; 64] }),
            old,
            "fixture".into(),
            approved(),
            U64(1)
        ),
        superseded()
    );
    assert_eq!(v.pending.as_ref().unwrap().id, new);
    assert!(v.signed_intent.is_none());
}

#[test]
fn salt_at_the_approved_deadline_cannot_request_a_signature() {
    let mut v = setup();
    let proposal = run(&mut v);
    context(accounts(0), 0, T + INTENT_TTL_SEC, None);
    failed(
        v.on_salt(Ok("00000000".into()), proposal, approved(), U64(1)),
        "the approved intent deadline has passed",
    );
    assert!(!v.is_in_flight());
}

fn gate_approval(v: &mut Vault, proposal: Proposal) {
    let mut prices = Vec::new();
    let mut feeds = Vec::new();
    let mut held = Vec::new();
    let mut results = Vec::new();
    for cfg in &v.mandate.assets {
        let multiplier = if cfg.asset_id == mandate::tests::WNEAR { 5 } else { 1 };
        let price = oracle::OraclePrice { multiplier: U128(multiplier), decimals: 0 };
        prices.push(oracle::AssetOptionalPrice {
            asset_id: cfg.price_feed_id.clone(),
            price: Some(price.clone()),
        });
        feeds.push(cfg.price_feed_id.clone());
        held.push((
            cfg.asset_id.clone(),
            U128(if multiplier == 5 { 10 * 10u128.pow(24) } else { 100_000_000 }),
        ));
        let view = oracle::AssetView {
            reports: vec![oracle::Report {
                oracle_id: "fixture.near".into(),
                timestamp: U64(T * 1_000_000_000),
                price,
            }],
        };
        results.push(PromiseResult::Successful(serde_json::to_vec(&Some(view)).unwrap()));
    }
    testing_env!(
        VMContextBuilder::new()
            .current_account_id(accounts(0))
            .predecessor_account_id(accounts(0))
            .block_timestamp(T * 1_000_000_000)
            .build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        results
    );
    assert!(matches!(
        v.on_assets(
            proposal,
            held,
            feeds,
            PriceData { timestamp: U64(T * 1_000_000_000), recency_duration_sec: 600, prices },
            U64(1)
        ),
        PromiseOrValue::Promise(_)
    ));
}

#[test]
fn salt_cannot_sign_after_the_approved_prices_become_stale() {
    let mut v = setup();
    let proposal = run(&mut v);
    gate_approval(&mut v, proposal.clone());
    context(accounts(0), 0, T + 61, None);
    failed(
        v.on_salt(Ok("00000000".into()), proposal, approved(), U64(1)),
        "the approved prices have become stale",
    );
    assert!(!v.is_in_flight());
}

#[test]
fn salt_can_sign_at_the_exact_price_age_boundary() {
    let mut v = setup();
    let proposal = run(&mut v);
    context(accounts(0), 0, T + 60, None);
    assert!(matches!(
        v.on_salt(Ok("00000000".into()), proposal, approved(), U64(1)),
        PromiseOrValue::Promise(_)
    ));
}

#[test]
fn signing_dispatch_reserves_quota_even_if_the_result_arrives_after_lock_expiry() {
    let mut v = setup();
    v.trading.day_start_sec = T - T % engine::SECONDS_PER_DAY;
    v.trading.day_turnover_usd = 180_000_000;
    let proposal = run(&mut v);
    let reserved = TradingState {
        last_trade_at_sec: Some(T),
        day_start_sec: T - T % engine::SECONDS_PER_DAY,
        day_turnover_usd: 200_000_000,
        high_water_mark_usd: 150_000_000,
        signed_intent_expires_at_sec: T + INTENT_TTL_SEC,
    };
    context(accounts(0), 0, T + 1, None);
    assert!(matches!(
        v.on_salt(Ok("00000000".into()), proposal.clone(), reserved.clone(), U64(1)),
        PromiseOrValue::Promise(_)
    ));
    assert_eq!(v.trading, reserved, "authorization must be reserved before a signature can escape");
    context(accounts(0), 0, T + EXECUTION_WINDOW_SEC + INTENT_TTL_SEC, None);
    assert_eq!(
        v.on_signed(
            Ok(SignatureResponse { scheme: "Ed25519".into(), signature: vec![0; 64] }),
            proposal,
            "late fixture".into(),
            reserved.clone(),
            U64(1)
        ),
        superseded()
    );
    assert_eq!(v.trading, reserved, "expired callback must not refund or overwrite quota");
    let price = math::Price { mantissa: U64(1), expo: 0, publish_time_sec: T + 420 };
    let market = Market {
        now_sec: T + 420,
        balances: [
            (mandate::tests::USDC.into(), 100_000_000),
            (mandate::tests::WNEAR.into(), 10 * 10u128.pow(24)),
        ]
        .into(),
        prices: [
            (mandate::tests::USDC.into(), price),
            (mandate::tests::WNEAR.into(), math::Price { mantissa: U64(5), ..price }),
        ]
        .into(),
    };
    let trade = Trade {
        asset_in: mandate::tests::USDC.into(),
        asset_out: mandate::tests::WNEAR.into(),
        amount_in: 20_000_000,
        min_amount_out: 3_960_000_000_000_000_000_000_000,
    };
    assert_eq!(
        check_trade(&v.mandate, &v.trading, &trade, &market).unwrap_err(),
        TradeError::TurnoverCapExceeded { would_be_usd: 220_000_000, max_usd: 200_000_000 }
    );
}

#[test]
fn unicode_salt_fails_without_a_utf8_slice_panic() {
    let mut v = setup();
    let proposal = run(&mut v);
    context(accounts(0), 0, T + 1, None);
    failed(
        v.on_salt(Ok("aéa".into()), proposal, approved(), U64(1)),
        "could not read the intents salt",
    );
    assert_eq!(from_hex("aéa"), None);
    assert_eq!(from_hex("00ff"), Some(vec![0, 255]));
    assert!(!v.is_in_flight());
}

#[test]
fn oversized_successful_signer_answer_keeps_the_timed_lock() {
    let mut v = setup();
    let proposal = run(&mut v);
    context(accounts(0), 0, T + 1, None);
    v.on_signed(
        Err(PromiseError::TooLong(MAX_ANSWER_BYTES + 1)),
        proposal,
        "fixture".into(),
        approved(),
        U64(1),
    );
    assert!(v.is_in_flight());
    context(accounts(2), 0, T + EXECUTION_WINDOW_SEC + INTENT_TTL_SEC, None);
    assert!(!v.is_in_flight());
}

#[test]
fn aggregate_snapshot_beyond_future_skew_is_rejected() {
    let mut v = setup();
    let proposal = run(&mut v);
    context(accounts(0), 0, T + 1, None);
    failed(
        v.on_assets(
            proposal,
            vec![],
            vec![],
            PriceData {
                timestamp: U64((T + 7) * 1_000_000_000),
                recency_duration_sec: 600,
                prices: vec![],
            },
            U64(1),
        ),
        "the oracle snapshot is in the future",
    );
}

fn withdrawal(v: &mut Vault, sent: bool) -> U64 {
    context(accounts(1), 1, T, None);
    v.request_withdrawal(mandate::tests::USDC.into(), accounts(4), U128(20));
    context(accounts(2), 0, T + WITHDRAW_DELAY_SEC, None);
    let _ = v.apply_pending_withdrawal();
    if sent {
        v.active_withdrawal.as_mut().unwrap().balance_before = Some(U128(100));
        v.withdrawal_stage = Some(WithdrawalStage::Dispatched);
    }
    v.active_withdrawal.as_ref().unwrap().id
}

#[test]
#[should_panic(expected = "a withdrawal is unresolved")]
fn proposal_is_blocked_while_a_withdrawal_is_unresolved() {
    let mut v = setup();
    withdrawal(&mut v, true);
    context(accounts(3), 0, T + WITHDRAW_DELAY_SEC, None);
    v.propose(
        mandate::tests::USDC.into(),
        mandate::tests::WNEAR.into(),
        U128(1),
        U128(1),
        "ab".repeat(32),
        "manual-v1".into(),
    );
}

#[test]
#[should_panic(expected = "a withdrawal is unresolved")]
fn second_matured_withdrawal_cannot_send_during_the_first() {
    let mut v = setup();
    withdrawal(&mut v, true);
    context(accounts(1), 1, T + WITHDRAW_DELAY_SEC, None);
    v.request_withdrawal(mandate::tests::USDC.into(), accounts(5), U128(10));
    context(accounts(2), 0, T + 2 * WITHDRAW_DELAY_SEC, None);
    let _ = v.apply_pending_withdrawal();
}

#[test]
fn malformed_and_oversized_withdrawal_answers_do_not_release_or_retry() {
    let mut v = setup();
    let id = withdrawal(&mut v, true);
    for bytes in [b"not-json".to_vec(), vec![b'x'; MAX_ANSWER_BYTES + 1]] {
        context(accounts(0), 0, T + WITHDRAW_DELAY_SEC + 1, Some(PromiseResult::Successful(bytes)));
        assert!(!v.on_withdrawn(id));
        assert!(v.is_in_flight());
        assert!(v.pending_withdrawal.is_none());
    }
}

#[test]
fn failed_returned_promise_after_dispatch_cannot_duplicate_a_debit() {
    let mut v = setup();
    let id = withdrawal(&mut v, true);
    context(accounts(0), 0, T + WITHDRAW_DELAY_SEC + 1, Some(PromiseResult::Failed));
    assert!(!v.on_withdrawn(id));
    assert!(v.is_in_flight());
    assert!(v.pending_withdrawal.is_none());
}

#[test]
fn partial_positive_withdrawal_is_not_full_delivery() {
    let mut v = setup();
    let id = withdrawal(&mut v, true);
    assert!(!v.finish_withdrawal(id, Ok(U128(1))));
    assert!(v.active_withdrawal.is_some());
    assert!(v.last_withdrawal_outcome.is_none());
}

#[test]
fn pre_send_timeout_requeues_safely_and_late_read_cannot_send() {
    let mut v = setup();
    let id = withdrawal(&mut v, false);
    context(accounts(2), 0, T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC, None);
    assert!(matches!(v.recover_withdrawal(), PromiseOrValue::Value(true)));
    assert!(v.active_withdrawal.is_none());
    assert!(v.pending_withdrawal.is_some());
    context(
        accounts(0),
        0,
        T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC,
        Some(PromiseResult::Successful(b"[\"100\"]".to_vec())),
    );
    assert!(matches!(v.on_withdrawal_balance(id), PromiseOrValue::Value(false)));
}

#[test]
fn scalar_balance_observations_cannot_release_the_voucher_guard() {
    let mut v = setup();
    let id = withdrawal(&mut v, true);
    let time = T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC;
    for bytes in [b"[\"81\"]".to_vec(), b"[\"80\"]".to_vec(), b"[\"0\"]".to_vec()] {
        context(accounts(0), 0, time, Some(PromiseResult::Successful(bytes)));
        assert!(!v.on_withdrawal_reconciled(id));
        assert!(v.is_in_flight());
        assert!(v.pending_withdrawal.is_none());
    }
}

#[test]
fn same_asset_credit_can_mask_an_unknown_debit_and_must_remain_guarded() {
    let mut v = setup();
    let id = withdrawal(&mut v, true);
    context(
        accounts(0),
        0,
        T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC,
        Some(PromiseResult::Successful(b"[\"105\"]".to_vec())),
    );
    assert!(!v.on_withdrawal_reconciled(id));
    assert!(v.is_in_flight());
    assert!(v.pending_withdrawal.is_none());
}

#[test]
fn invalid_near_account_grammar_and_unbounded_nep245_are_rejected() {
    for id in ["nep141:.near", "nep141:near.", "nep141:a..b", "nep141:a-_b", "nep141:aa:bb"] {
        assert!(!mandate::is_valid_asset_id(id), "{id}");
        assert!(token_account(id).is_none());
    }
    assert!(mandate::is_valid_asset_id(&format!("nep141:{}", "a".repeat(64))));
    assert!(!mandate::is_valid_asset_id(&format!("nep141:{}", "a".repeat(65))));
    assert!(!mandate::is_valid_asset_id(&format!("nep245:aa:{}", "a".repeat(100_000))));
}

#[test]
#[should_panic(expected = "withdrawal requires at least 280 TGas")]
fn too_little_gas_cannot_dispatch_a_withdrawal() {
    let mut v = setup();
    context(accounts(1), 1, T, None);
    v.request_withdrawal(mandate::tests::USDC.into(), accounts(4), U128(20));
    testing_env!(
        VMContextBuilder::new()
            .current_account_id(accounts(0))
            .predecessor_account_id(accounts(2))
            .block_timestamp((T + WITHDRAW_DELAY_SEC) * 1_000_000_000)
            .prepaid_gas(Gas::from_tgas(279))
            .build()
    );
    let _ = v.apply_pending_withdrawal();
}

#[test]
#[should_panic(expected = "the receiver cannot be the vault or intents.near")]
fn withdrawal_to_the_configured_intents_account_is_rejected() {
    let mut v = setup();
    context(accounts(1), 1, T, None);
    v.request_withdrawal(mandate::tests::USDC.into(), v.intents.clone(), U128(1));
}

fn exact_panic(expected: &str, action: impl FnOnce()) {
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action))
        .expect_err("the delayed operation must be refused");
    let message = error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| error.downcast_ref::<&str>().copied())
        .expect("the contract panic must contain text");
    assert_eq!(message, expected);
}

#[test]
fn later_queue_replacement_resets_the_mandate_delay() {
    const DELTA: u64 = 37;
    let mut v = setup();
    let active = v.mandate.clone();
    let revision = v.config_revision;
    context(accounts(1), 1, T, None);
    let first = Mandate { max_trade_usd: U64(75_000_000), ..active.clone() };
    assert_eq!(
        v.set_mandate(first.clone()),
        MandateChangeOutcome::Queued { effective_sec: U64(T + MANDATE_DELAY_SEC) }
    );
    assert_eq!(
        v.pending_mandate,
        Some(PendingMandate { mandate: first, effective_sec: T + MANDATE_DELAY_SEC })
    );

    context(accounts(1), 1, T + DELTA, None);
    let replacement = Mandate { max_trade_usd: U64(80_000_000), ..active.clone() };
    let new_deadline = T + DELTA + MANDATE_DELAY_SEC;
    assert_eq!(
        v.set_mandate(replacement.clone()),
        MandateChangeOutcome::Queued { effective_sec: U64(new_deadline) }
    );
    let expected =
        Some(PendingMandate { mandate: replacement.clone(), effective_sec: new_deadline });
    assert_eq!(v.pending_mandate, expected);
    assert_eq!(v.mandate, active);
    context(accounts(2), 0, T + MANDATE_DELAY_SEC, None);
    exact_panic("the mandate change is still in its delay", || v.apply_pending_mandate());
    assert_eq!(v.pending_mandate, expected);
    assert_eq!(v.mandate, active);
    assert_eq!(v.config_revision, revision);

    context(accounts(2), 0, new_deadline, None);
    v.apply_pending_mandate();
    assert_eq!(v.mandate, replacement);
    assert!(v.pending_mandate.is_none());
    assert_eq!(v.config_revision, revision + 1);
}

#[test]
fn later_queue_replacement_resets_the_agent_delay() {
    const DELTA: u64 = 37;
    let mut v = setup();
    let active = v.agent.clone();
    let revision = v.config_revision;
    context(accounts(1), 1, T, None);
    v.set_agent(accounts(4));
    assert_eq!(
        v.pending_agent,
        Some(PendingAgent { agent: accounts(4), effective_sec: T + AGENT_DELAY_SEC })
    );

    context(accounts(1), 1, T + DELTA, None);
    v.set_agent(accounts(5));
    let new_deadline = T + DELTA + AGENT_DELAY_SEC;
    let expected = Some(PendingAgent { agent: accounts(5), effective_sec: new_deadline });
    assert_eq!(v.pending_agent, expected);
    assert_eq!(v.agent, active);
    context(accounts(2), 0, T + AGENT_DELAY_SEC, None);
    exact_panic("the agent change is still in its delay", || v.apply_pending_agent());
    assert_eq!(v.pending_agent, expected);
    assert_eq!(v.agent, active);
    assert_eq!(v.config_revision, revision);

    context(accounts(2), 0, new_deadline, None);
    v.apply_pending_agent();
    assert_eq!(v.agent, Some(accounts(5)));
    assert!(v.pending_agent.is_none());
    assert_eq!(v.config_revision, revision + 1);
}

#[test]
fn later_queue_replacement_resets_the_withdrawal_delay() {
    const DELTA: u64 = 37;
    let mut v = setup();
    context(accounts(1), 1, T, None);
    v.request_withdrawal(mandate::tests::USDC.into(), accounts(4), U128(20));
    assert_eq!(
        v.pending_withdrawal,
        Some(PendingWithdrawal {
            asset_id: mandate::tests::USDC.into(),
            receiver: accounts(4),
            amount: U128(20),
            effective_sec: T + WITHDRAW_DELAY_SEC,
        })
    );

    context(accounts(1), 1, T + DELTA, None);
    v.request_withdrawal(mandate::tests::USDC.into(), accounts(5), U128(10));
    let new_deadline = T + DELTA + WITHDRAW_DELAY_SEC;
    let replacement = PendingWithdrawal {
        asset_id: mandate::tests::USDC.into(),
        receiver: accounts(5),
        amount: U128(10),
        effective_sec: new_deadline,
    };
    assert_eq!(v.pending_withdrawal, Some(replacement.clone()));
    assert_eq!(v.withdrawal_seq, 2);
    context(accounts(2), 0, T + WITHDRAW_DELAY_SEC, None);
    exact_panic("the withdrawal is still in its delay", || {
        let _ = v.apply_pending_withdrawal();
    });
    assert_eq!(v.pending_withdrawal, Some(replacement.clone()));
    assert!(v.active_withdrawal.is_none());
    assert_eq!(v.next_withdrawal_id, 1);
    assert_eq!(v.withdrawal_seq, 2);

    context(accounts(2), 0, new_deadline, None);
    let promise: Promise = v.apply_pending_withdrawal();
    drop(promise);
    assert!(v.pending_withdrawal.is_none());
    assert_eq!(
        v.active_withdrawal,
        Some(ActiveWithdrawal {
            id: U64(1),
            request_seq: U64(2),
            request: replacement,
            started_sec: new_deadline,
            balance_before: None,
        })
    );
    assert_eq!(v.next_withdrawal_id, 2);
    assert!(v.last_withdrawal_outcome.is_none());
    assert_eq!(v.trading, TradingState::default());
}

#[test]
fn a_failed_execution_is_recorded_as_the_outcome_of_its_proposal() {
    let mut v = setup();
    let proposal = run(&mut v);
    context(accounts(1), 1, T + 1, None);
    v.set_mandate(Mandate { max_trade_usd: U64(10_000_000), ..v.mandate.clone() });
    context(accounts(0), 0, T + 2, None);
    failed(
        v.on_salt(Ok("00000000".into()), proposal.clone(), approved(), U64(1)),
        "the execution configuration changed",
    );
    let recorded = v.last_proposal_outcome.clone().expect("the failure must be recorded");
    assert_eq!(recorded.proposal_id, proposal.id);
    assert!(matches!(
        recorded.outcome,
        ExecutionOutcome::Failed { ref reason } if reason == "the execution configuration changed"
    ));
}
