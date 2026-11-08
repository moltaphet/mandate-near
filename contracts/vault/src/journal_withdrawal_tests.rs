//! Native fault fixtures for the journal adapter. Promise results are injected
//! authenticated-result snapshots, not evidence of physically lost live receipts.

use super::*;
use mandate_withdrawal_journal::canonical_hash;
use near_sdk::{
    PromiseResult,
    mock::with_mocked_blockchain,
    test_utils::{VMContextBuilder, accounts, get_created_receipts},
    testing_env,
};

const T: u64 = 1_793_448_000;
const READY: u64 = T + WITHDRAW_DELAY_SEC;

struct StorageCleanup;

impl Drop for StorageCleanup {
    fn drop(&mut self) {
        with_mocked_blockchain(|blockchain| {
            let _ = blockchain.take_storage();
        });
    }
}

fn context(who: AccountId, deposit: u128, now: u64, results: Vec<PromiseResult>) {
    testing_env!(
        VMContextBuilder::new()
            .current_account_id(accounts(0))
            .predecessor_account_id(who)
            .attached_deposit(NearToken::from_yoctonear(deposit))
            .block_timestamp(now * 1_000_000_000)
            .prepaid_gas(Gas::from_tgas(300))
            .build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        results,
    );
}

fn clean_storage() -> StorageCleanup {
    with_mocked_blockchain(|blockchain| {
        let _ = blockchain.take_storage();
    });
    context(accounts(1), 0, T, vec![]);
    StorageCleanup
}

fn config() -> JournalVaultConfig {
    JournalVaultConfig {
        signer: accounts(2),
        intents: "executor.testnet".parse().unwrap(),
        oracle: accounts(3),
        oracle_kind: OracleKind::PriceOracle,
        ledger_domain: Hex32::from_bytes([7; 32]),
    }
}

fn fresh() -> (StorageCleanup, Vault) {
    let cleanup = clean_storage();
    let vault = Vault::new_with_journal(accounts(1), mandate::tests::sample(), config());
    (cleanup, vault)
}

fn preparing() -> (StorageCleanup, Vault, U64) {
    let (cleanup, mut vault) = fresh();
    context(accounts(1), 1, T, vec![]);
    vault.request_withdrawal(mandate::tests::USDC.into(), accounts(4), U128(20));
    context(accounts(2), 0, READY, vec![]);
    drop(vault.apply_pending_withdrawal());
    let id = vault.active_withdrawal.as_ref().unwrap().id;
    assert_eq!(vault.get_withdrawal_stage(), Some(WithdrawalStage::Preparing));
    (cleanup, vault, id)
}

fn successful<T: near_sdk::serde::Serialize>(value: &T) -> PromiseResult {
    PromiseResult::Successful(serde_json::to_vec(value).unwrap())
}

fn ready_results(vault: &Vault) -> Vec<PromiseResult> {
    vec![successful(&vault.journal_binding()), successful(&Some(vec![accounts(0)]))]
}

fn dispatched() -> (StorageCleanup, Vault, U64) {
    let (cleanup, mut vault, id) = preparing();
    let proof = ready_results(&vault);
    context(accounts(0), 0, READY + 1, proof);
    let next = vault.on_journal_ready(id);
    assert!(matches!(&next, PromiseOrValue::Promise(_)));
    drop(next);
    assert_eq!(vault.get_withdrawal_stage(), Some(WithdrawalStage::Dispatched));
    (cleanup, vault, id)
}

fn record(vault: &Vault, state: JournalWithdrawalState) -> JournalWithdrawalRecord {
    JournalWithdrawalRecord::new(
        &vault.journal_binding().unwrap(),
        accounts(0),
        journal_request(vault.active_withdrawal.as_ref().unwrap()),
        state,
    )
    .unwrap()
}

fn stored(vault: &Vault) -> Vec<u8> {
    near_sdk::borsh::to_vec(vault).unwrap()
}

fn methods() -> Vec<(String, serde_json::Value, u128)> {
    get_created_receipts()
        .into_iter()
        .flat_map(|receipt| receipt.actions)
        .filter_map(|action| match action {
            near_sdk::mock::MockAction::FunctionCallWeight {
                method_name,
                args,
                attached_deposit,
                ..
            } => Some((
                String::from_utf8(method_name).unwrap(),
                serde_json::from_slice(&args).unwrap(),
                attached_deposit.as_yoctonear(),
            )),
            _ => None,
        })
        .collect()
}

fn rejected(action: impl FnOnce()) {
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)).is_err());
}

#[test]
fn journal_fresh_binding_is_explicit_immutable_and_outside_root_borsh_layout() {
    let (_cleanup, vault) = fresh();
    let binding = vault.get_withdrawal_journal_binding().unwrap();
    assert_eq!(binding.ledger_domain, Hex32::from_bytes([7; 32]));
    assert_eq!(binding.executor_id, config().intents);
    assert_eq!(vault.get_withdrawal_protocol(), PROTOCOL_VERSION);
    let bytes = stored(&vault);
    let legacy_equivalent = Vault::new_with_oracle(
        accounts(1),
        mandate::tests::sample(),
        config().signer,
        config().intents,
        config().oracle,
        config().oracle_kind,
    );
    assert_eq!(stored(&legacy_equivalent), bytes);
    assert_eq!(vault.get_withdrawal_journal_binding(), Some(binding.clone()));
    rejected(|| {
        drop(Vault::new_with_journal(accounts(1), mandate::tests::sample(), config()));
    });
    assert_eq!(vault.get_withdrawal_journal_binding(), Some(binding));
    assert_eq!(stored(&vault), bytes);
}

#[test]
fn journal_cannot_be_enabled_on_an_existing_vault_or_with_an_empty_domain() {
    let _cleanup = clean_storage();
    let vault = Vault::new(accounts(1), mandate::tests::sample());
    env::state_write(&vault);
    let before = env::storage_read(b"STATE").unwrap();
    rejected(|| {
        drop(Vault::new_with_journal(accounts(1), mandate::tests::sample(), config()));
    });
    assert_eq!(env::storage_read(b"STATE").unwrap(), before);
    assert!(!env::storage_has_key(BINDING_KEY));
    with_mocked_blockchain(|blockchain| {
        let _ = blockchain.take_storage();
    });
    let mut empty = config();
    empty.ledger_domain = Hex32::from_bytes([0; 32]);
    rejected(|| {
        drop(Vault::new_with_journal(accounts(1), mandate::tests::sample(), empty));
    });
    assert!(!env::storage_has_key(BINDING_KEY));
}

#[test]
fn legacy_defaults_still_select_the_voucher_preparation_and_no_journal_binding() {
    let _cleanup = clean_storage();
    let mut vault = Vault::new(accounts(1), mandate::tests::sample());
    assert!(vault.get_withdrawal_journal_binding().is_none());
    assert_eq!(vault.get_withdrawal_protocol(), "unique_voucher_v1");
    context(accounts(1), 1, T, vec![]);
    vault.request_withdrawal(mandate::tests::USDC.into(), accounts(4), U128(20));
    context(accounts(2), 0, READY, vec![]);
    drop(vault.apply_pending_withdrawal());
    let calls = methods();
    assert_eq!(
        calls.iter().map(|call| call.0.as_str()).collect::<Vec<_>>(),
        ["mt_on_transfer", "mt_batch_balance_of", "on_withdrawal_balance"]
    );
    assert!(calls.iter().all(|call| !call.0.contains("journal")));
}

#[test]
fn journal_preparing_schedules_only_authenticated_descriptor_and_admission_reads() {
    let (_cleanup, vault, _) = preparing();
    let calls = methods();
    assert_eq!(
        calls.iter().map(|call| call.0.as_str()).collect::<Vec<_>>(),
        ["get_withdrawal_journal_protocol", "get_journal_authorized_vaults", "on_journal_ready"]
    );
    assert!(calls.iter().all(|call| call.2 == 0));
    assert_eq!(calls[0].1, json!({}));
    assert_eq!(calls[1].1, json!({}));
    assert_eq!(vault.active_withdrawal.as_ref().unwrap().balance_before, None);
    let snapshot = vault.get_withdrawal_recovery_state().unwrap();
    assert_eq!(snapshot.protocol, PROTOCOL_VERSION);
    assert_eq!(snapshot.stage, WithdrawalStage::Preparing);
    assert_eq!(snapshot.recover_after_sec, READY + WITHDRAW_RECOVERY_SEC);
}

#[test]
fn journal_ready_exact_tuple_dispatches_the_bound_request_once() {
    let (_cleanup, mut vault, id) = preparing();
    let active = vault.active_withdrawal.clone().unwrap();
    let results = ready_results(&vault);
    let duplicate_results = ready_results(&vault);
    context(accounts(0), 0, READY + WITHDRAW_RECOVERY_SEC - 1, results);
    drop(vault.on_journal_ready(id));
    assert_eq!(vault.get_withdrawal_stage(), Some(WithdrawalStage::Dispatched));
    let calls = methods();
    assert_eq!(
        calls.iter().map(|call| call.0.as_str()).collect::<Vec<_>>(),
        ["withdraw_with_id", "on_journal_withdrawn"]
    );
    assert_eq!(calls[0].1, json!({"request":journal_request(&active)}));
    assert_eq!(calls[0].2, 1);
    let before = stored(&vault);
    context(accounts(0), 0, READY + WITHDRAW_RECOVERY_SEC - 1, duplicate_results);
    assert!(matches!(vault.on_journal_ready(id), PromiseOrValue::Value(false)));
    assert_eq!(stored(&vault), before);
    assert!(methods().is_empty());
}

#[test]
fn journal_ready_missing_failed_malformed_and_wrong_arity_cannot_dispatch() {
    for kind in 0..7 {
        let (_cleanup, mut vault, id) = preparing();
        let mut results = ready_results(&vault);
        match kind {
            0 => results.clear(),
            1 => {
                results.pop();
            }
            2 => results.push(successful(&true)),
            3 => results[0] = PromiseResult::Failed,
            4 => results[1] = successful(&Option::<Vec<AccountId>>::None),
            5 => results[0] = PromiseResult::Successful(b"{}".to_vec()),
            _ => results[1] = PromiseResult::Successful(vec![b'x'; MAX_JOURNAL_RESPONSE_BYTES + 1]),
        }
        let original = vault.active_withdrawal.as_ref().unwrap().request.clone();
        context(accounts(0), 0, READY + 1, results);
        assert!(matches!(vault.on_journal_ready(id), PromiseOrValue::Value(false)), "case {kind}");
        assert!(vault.active_withdrawal.is_none());
        assert_eq!(vault.pending_withdrawal, Some(original));
        assert!(!vault.is_withdrawal_retry_armed());
        assert!(methods().is_empty());
    }
}

#[test]
fn journal_ready_rejects_wrong_domain_source_protocol_and_noncanonical_admission() {
    for kind in 0..9 {
        let (_cleanup, mut vault, id) = preparing();
        let mut descriptor = vault.journal_binding().unwrap();
        let mut admitted = vec![accounts(0)];
        match kind {
            0 => descriptor.ledger_domain = Hex32::from_bytes([8; 32]),
            1 => descriptor.executor_id = accounts(3),
            2 => descriptor.protocol_version = "other_protocol".into(),
            3 => descriptor.request_hash_scheme = "other_hash".into(),
            4 => admitted.clear(),
            5 => admitted = vec![accounts(0), accounts(0)],
            6 => admitted = vec![accounts(1), accounts(0)],
            7 => admitted = vec![accounts(1)],
            _ => {
                admitted = (0..20)
                    .map(|index| format!("vault{index:02}.testnet").parse().unwrap())
                    .collect();
                admitted.push(accounts(0));
                admitted.sort();
            }
        }
        context(
            accounts(0),
            0,
            READY + 1,
            vec![successful(&Some(descriptor)), successful(&Some(admitted))],
        );
        assert!(matches!(vault.on_journal_ready(id), PromiseOrValue::Value(false)), "case {kind}");
        assert!(vault.active_withdrawal.is_none());
        assert!(vault.pending_withdrawal.is_some());
        assert!(!vault.is_withdrawal_retry_armed());
        assert!(methods().is_empty());
    }
}

#[test]
fn journal_ready_at_the_preparation_deadline_cannot_dispatch() {
    let (_cleanup, mut vault, id) = preparing();
    let results = ready_results(&vault);
    context(accounts(0), 0, READY + WITHDRAW_RECOVERY_SEC, results);
    assert!(matches!(vault.on_journal_ready(id), PromiseOrValue::Value(false)));
    assert!(vault.active_withdrawal.is_none());
    assert!(!vault.is_withdrawal_retry_armed());
    assert!(methods().is_empty());
}

#[test]
fn journal_ready_rechecks_a_tightened_ceiling_before_scheduling_custody() {
    let (_cleanup, mut vault, id) = preparing();
    context(accounts(1), 1, READY, vec![]);
    let mut mandate = vault.mandate.clone();
    mandate.withdrawal_limits =
        vec![WithdrawalLimit { asset_id: mandate::tests::USDC.into(), max_per_request: U128(19) }];
    assert_eq!(vault.set_mandate(mandate), MandateChangeOutcome::Applied);
    let results = ready_results(&vault);
    context(accounts(0), 0, READY + 1, results);
    assert!(matches!(vault.on_journal_ready(id), PromiseOrValue::Value(false)));
    assert!(vault.active_withdrawal.is_none());
    assert_eq!(vault.pending_withdrawal.as_ref().unwrap().amount, U128(20));
    assert!(!vault.is_withdrawal_retry_armed());
    assert!(methods().is_empty());
}

#[test]
fn journal_ready_requires_the_asset_to_remain_admitted() {
    let (_cleanup, mut vault, id) = preparing();
    // Native policy-change fault fixture; the callback must refuse even when
    // the prior snapshot would have approved this now-absent asset.
    vault.mandate.assets.retain(|asset| asset.asset_id != mandate::tests::USDC);
    let results = ready_results(&vault);
    context(accounts(0), 0, READY + 1, results);
    assert!(matches!(vault.on_journal_ready(id), PromiseOrValue::Value(false)));
    assert!(methods().is_empty());
    assert!(vault.active_withdrawal.is_none());
    assert!(!vault.is_withdrawal_retry_armed());
}

#[test]
fn journal_owner_replacement_invalidates_old_preparation_without_touching_new_authorization() {
    let (_cleanup, mut vault, id) = preparing();
    context(accounts(1), 1, READY + 1, vec![]);
    vault.request_withdrawal(mandate::tests::USDC.into(), accounts(5), U128(10));
    let replacement = vault.pending_withdrawal.clone();
    let arm = vault.withdrawal_retry_armed;
    let results = ready_results(&vault);
    context(accounts(0), 0, READY + 2, results);
    assert!(matches!(vault.on_journal_ready(id), PromiseOrValue::Value(false)));
    assert!(vault.active_withdrawal.is_none());
    assert_eq!(vault.pending_withdrawal, replacement);
    assert_eq!(vault.withdrawal_retry_armed, arm);
    assert_eq!(replacement.unwrap().effective_sec, READY + 1 + WITHDRAW_DELAY_SEC);
    assert!(methods().is_empty());
}

#[test]
fn journal_preparation_timeout_is_send_free_and_late_ready_is_inert_after_rearming() {
    let (_cleanup, mut vault, old) = preparing();
    let results = ready_results(&vault);
    context(accounts(2), 0, READY + WITHDRAW_RECOVERY_SEC, vec![]);
    assert!(matches!(vault.recover_withdrawal_attempt(old), PromiseOrValue::Value(true)));
    assert!(methods().is_empty());
    assert!(!vault.is_withdrawal_retry_armed());
    context(accounts(1), 1, READY + WITHDRAW_RECOVERY_SEC, vec![]);
    vault.rearm_pending_withdrawal();
    drop(vault.apply_pending_withdrawal());
    let new = vault.active_withdrawal.as_ref().unwrap().id;
    assert_ne!(new, old);
    let before = stored(&vault);
    context(accounts(0), 0, READY + WITHDRAW_RECOVERY_SEC + 1, results);
    assert!(matches!(vault.on_journal_ready(old), PromiseOrValue::Value(false)));
    assert_eq!(stored(&vault), before);
    assert!(methods().is_empty());
}

#[test]
fn journal_dispatched_missing_failed_malformed_oversized_and_pending_results_keep_guard() {
    for kind in 0..8 {
        let (_cleanup, mut vault, id) = dispatched();
        let pending = record(&vault, JournalWithdrawalState::Pending);
        let results = match kind {
            0 => vec![],
            1 => vec![PromiseResult::Failed],
            2 => vec![PromiseResult::Successful(b"{}".to_vec())],
            3 => vec![PromiseResult::Successful(vec![b'x'; MAX_JOURNAL_RESPONSE_BYTES + 1])],
            4 => vec![successful(&pending)],
            5 => vec![successful(&pending), successful(&true)],
            6 => vec![successful(&U128(20))],
            _ => vec![successful(&Option::<JournalWithdrawalRecord>::None)],
        };
        let before = stored(&vault);
        context(accounts(0), 0, READY + 2, results);
        assert!(!vault.on_journal_withdrawn(id), "case {kind}");
        assert!(!vault.on_journal_reconciled(id), "query case {kind}");
        assert_eq!(stored(&vault), before);
        assert!(vault.pending_withdrawal.is_none());
        assert!(methods().is_empty());
    }
}

#[test]
fn journal_initial_callback_reports_delivery_but_not_refund_or_cancellation_as_delivery() {
    for state in [
        JournalWithdrawalState::Delivered { delivered_amount: U128(20) },
        JournalWithdrawalState::Refunded { refunded_amount: U128(20) },
        JournalWithdrawalState::Cancelled,
    ] {
        let (_cleanup, mut vault, id) = dispatched();
        let original = vault.active_withdrawal.as_ref().unwrap().request.clone();
        let terminal = record(&vault, state.clone());
        context(accounts(0), 0, READY + 2, vec![successful(&terminal)]);
        assert_eq!(
            vault.on_journal_withdrawn(id),
            matches!(state, JournalWithdrawalState::Delivered { .. })
        );
        assert!(vault.active_withdrawal.is_none());
        assert!(vault.withdrawal_stage.is_none());
        if matches!(state, JournalWithdrawalState::Delivered { .. }) {
            assert!(vault.pending_withdrawal.is_none());
            assert_eq!(
                vault.last_withdrawal_outcome,
                Some(WithdrawalOutcome::Delivered { withdrawal_id: id, amount: U128(20) })
            );
        } else {
            assert_eq!(vault.pending_withdrawal, Some(original));
            assert!(!vault.is_withdrawal_retry_armed());
        }
        assert!(methods().is_empty());
    }
}

#[test]
fn journal_recovery_callbacks_report_all_authenticated_terminal_states_as_guard_resolution() {
    for initial_cancel_callback in [false, true] {
        for state in [
            JournalWithdrawalState::Delivered { delivered_amount: U128(20) },
            JournalWithdrawalState::Refunded { refunded_amount: U128(20) },
            JournalWithdrawalState::Cancelled,
        ] {
            let (_cleanup, mut vault, id) = dispatched();
            let terminal = record(&vault, state);
            context(accounts(0), 0, READY + 2, vec![successful(&terminal)]);
            if initial_cancel_callback {
                assert!(matches!(vault.on_journal_cancelled(id), PromiseOrValue::Value(true)));
            } else {
                assert!(vault.on_journal_reconciled(id));
            }
            assert!(vault.active_withdrawal.is_none());
            assert!(methods().is_empty());
        }
    }
}

#[test]
fn journal_terminal_amount_or_any_bound_field_mismatch_never_releases_or_requeues() {
    for kind in 0..13 {
        let (_cleanup, mut vault, id) = dispatched();
        let binding = vault.journal_binding().unwrap();
        let mut invalid =
            record(&vault, JournalWithdrawalState::Delivered { delivered_amount: U128(20) });
        match kind {
            0 => invalid.protocol_version = "other_protocol".into(),
            1 => invalid.ledger_domain = Hex32::from_bytes([8; 32]),
            2 => invalid.executor_id = accounts(3),
            3 => invalid.vault_id = accounts(1),
            4 => invalid.request.withdrawal_id = U64(id.0 + 1),
            5 => invalid.request.request_seq = U64(invalid.request.request_seq.0 + 1),
            6 => invalid.request.token = accounts(3),
            7 => invalid.request.receiver_id = accounts(5),
            8 => {
                invalid.request.amount = U128(21);
                invalid.state = JournalWithdrawalState::Delivered { delivered_amount: U128(21) };
            }
            9 => invalid.request_hash = Hex32::from_bytes([0; 32]),
            10 => invalid.state = JournalWithdrawalState::Delivered { delivered_amount: U128(19) },
            11 => invalid.state = JournalWithdrawalState::Refunded { refunded_amount: U128(19) },
            _ => {
                invalid.vault_id = accounts(1);
                invalid.request.receiver_id = accounts(5);
                invalid.request.request_seq = U64(100);
            }
        }
        if (4..=8).contains(&kind) || kind == 12 {
            invalid.request_hash = canonical_hash(&binding, &invalid.vault_id, &invalid.request);
        }
        let before = stored(&vault);
        context(accounts(0), 0, READY + 2, vec![successful(&invalid)]);
        assert!(!vault.on_journal_withdrawn(id), "case {kind}");
        assert!(!vault.on_journal_reconciled(id), "query case {kind}");
        assert_eq!(stored(&vault), before);
        assert!(vault.pending_withdrawal.is_none());
        assert!(methods().is_empty());
    }
}

#[test]
fn journal_unresolved_cancel_response_only_schedules_authenticated_status_read() {
    for kind in 0..4 {
        let (_cleanup, mut vault, id) = dispatched();
        let pending = record(&vault, JournalWithdrawalState::Pending);
        let results = match kind {
            0 => vec![PromiseResult::Failed],
            1 => vec![successful(&pending)],
            2 => vec![],
            _ => vec![PromiseResult::Successful(vec![b'x'; MAX_JOURNAL_RESPONSE_BYTES + 1])],
        };
        let before = stored(&vault);
        context(accounts(0), 0, READY + WITHDRAW_RECOVERY_SEC, results);
        let next = vault.on_journal_cancelled(id);
        assert!(matches!(&next, PromiseOrValue::Promise(_)));
        drop(next);
        assert_eq!(stored(&vault), before);
        let calls = methods();
        assert_eq!(
            calls.iter().map(|call| call.0.as_str()).collect::<Vec<_>>(),
            ["get_journal_withdrawal_status", "on_journal_reconciled"]
        );
        assert_eq!(calls[0].1, json!({"vault_id":accounts(0), "withdrawal_id":id}));
        assert_eq!(calls[0].2, 0);
    }
}

#[test]
fn journal_failed_callback_result_fixture_can_be_reconciled_later_without_another_withdrawal() {
    let (_cleanup, mut vault, id) = dispatched();
    let delivered =
        record(&vault, JournalWithdrawalState::Delivered { delivered_amount: U128(20) });
    context(accounts(0), 0, READY + 2, vec![PromiseResult::Failed]);
    assert!(!vault.on_journal_withdrawn(id));
    context(accounts(2), 0, READY + WITHDRAW_RECOVERY_SEC, vec![]);
    drop(vault.recover_withdrawal_attempt(id));
    let calls = methods();
    assert_eq!(
        calls.iter().map(|call| call.0.as_str()).collect::<Vec<_>>(),
        ["cancel_withdrawal_with_id", "on_journal_cancelled"]
    );
    assert!(calls.iter().all(|call| call.0 != "withdraw_with_id"));
    context(accounts(0), 0, READY + WITHDRAW_RECOVERY_SEC + 1, vec![successful(&Some(delivered))]);
    assert!(vault.on_journal_reconciled(id));
    assert!(vault.active_withdrawal.is_none());
    assert!(vault.pending_withdrawal.is_none());
    assert_eq!(
        vault.last_withdrawal_outcome,
        Some(WithdrawalOutcome::Delivered { withdrawal_id: id, amount: U128(20) })
    );
    assert!(methods().is_empty());
}

#[test]
fn journal_old_callbacks_and_id_bound_recovery_cannot_touch_a_new_dispatched_attempt() {
    let (_cleanup, mut vault, old) = dispatched();
    let old_cancelled = record(&vault, JournalWithdrawalState::Cancelled);
    let old_delivered =
        record(&vault, JournalWithdrawalState::Delivered { delivered_amount: U128(20) });
    context(accounts(0), 0, READY + 2, vec![successful(&old_cancelled)]);
    assert!(matches!(vault.on_journal_cancelled(old), PromiseOrValue::Value(true)));
    context(accounts(1), 1, READY + 3, vec![]);
    vault.rearm_pending_withdrawal();
    drop(vault.apply_pending_withdrawal());
    let new = vault.active_withdrawal.as_ref().unwrap().id;
    let ready = ready_results(&vault);
    context(accounts(0), 0, READY + 4, ready);
    drop(vault.on_journal_ready(new));
    assert_ne!(new, old);
    let before = stored(&vault);
    context(accounts(0), 0, READY + 5, vec![successful(&old_delivered)]);
    assert!(!vault.on_journal_withdrawn(old));
    assert!(!vault.on_journal_reconciled(old));
    assert!(matches!(vault.on_journal_cancelled(old), PromiseOrValue::Value(false)));
    assert!(matches!(vault.on_journal_ready(old), PromiseOrValue::Value(false)));
    assert!(matches!(vault.recover_withdrawal_attempt(old), PromiseOrValue::Value(false)));
    assert_eq!(stored(&vault), before);
    assert!(methods().is_empty());
}

#[test]
fn journal_old_terminal_refund_cannot_resurrect_an_owner_cancelled_replacement() {
    let (_cleanup, mut vault, id) = dispatched();
    let refunded = record(&vault, JournalWithdrawalState::Refunded { refunded_amount: U128(20) });
    context(accounts(1), 1, READY + 2, vec![]);
    vault.request_withdrawal(mandate::tests::USDC.into(), accounts(5), U128(10));
    vault.cancel_pending_withdrawal();
    context(accounts(0), 0, READY + 3, vec![successful(&refunded)]);
    assert!(!vault.on_journal_withdrawn(id));
    assert!(vault.active_withdrawal.is_none());
    assert!(vault.pending_withdrawal.is_none());
    assert!(!vault.is_withdrawal_retry_armed());
}

#[test]
fn journal_legacy_preparation_and_terminal_callback_dialects_are_inert() {
    let (_cleanup, mut vault, id) = preparing();
    let before = stored(&vault);
    context(accounts(0), 0, READY + 1, vec![successful(&vec![U128(1), U128(100)])]);
    assert!(matches!(vault.on_withdrawal_balance(id), PromiseOrValue::Value(false)));
    assert_eq!(stored(&vault), before);
    assert!(methods().is_empty());
    context(
        accounts(0),
        0,
        READY + 1,
        vec![
            successful(&Some(U128(1))),
            successful(&vec![U128(1)]),
            successful(&vec![U128(0)]),
            successful(&vec![U128(0)]),
        ],
    );
    assert!(matches!(vault.on_withdrawal_prepared(id, U128(100)), PromiseOrValue::Value(false)));
    assert_eq!(stored(&vault), before);
    let ready = ready_results(&vault);
    context(accounts(0), 0, READY + 2, ready);
    drop(vault.on_journal_ready(id));
    let dispatched = stored(&vault);
    context(accounts(0), 0, READY + 3, vec![successful(&U128(20))]);
    assert!(!vault.on_withdrawn(id));
    assert_eq!(stored(&vault), dispatched);
    context(
        accounts(0),
        0,
        READY + 3,
        vec![
            successful(&Some(U128(1))),
            successful(&vec![U128(0)]),
            successful(&vec![U128(1)]),
            successful(&vec![U128(0)]),
        ],
    );
    assert!(!vault.on_withdrawal_reconciled(id));
    assert!(matches!(vault.on_withdrawal_cancelled(id), PromiseOrValue::Value(false)));
    assert_eq!(stored(&vault), dispatched);
    assert!(methods().is_empty());
}
