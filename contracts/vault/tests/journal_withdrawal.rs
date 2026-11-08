//! Actual modified Defuse WASM integration. MPC/oracle/token are sandbox
//! stand-ins; custody, debit, FT scheduling and durable resolver are Defuse.
//! Maturity and missing-callback/fact injections are explicitly labelled.

mod common;
#[path = "common/journal.rs"]
mod journal;

use common::{fund_vault, view};
use journal::{
    BindingCase, block_receiver, executor_send, install_dispatched_fixture,
    install_pending_fact_fixture, ledger_balance, queue_mature, receiver_balance, send, status,
    storage_usage, world, world_with_binding,
};
use mandate_withdrawal_journal::{
    JournalProtocolDescriptor, JournalWithdrawalRecord, JournalWithdrawalRequest,
    JournalWithdrawalState, PROTOCOL_VERSION,
};
use near_api::types::tokens::STORAGE_COST_PER_BYTE;
use near_api::{Account, NearGas, NearToken};
use near_sandbox::{FetchData, sandbox::patch::StateRecord};
use near_sdk::json_types::{U64, U128};
use near_sdk::serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

fn wasm_u32(cursor: &mut &[u8]) -> u32 {
    let mut value = 0;
    for index in 0..5 {
        let (&byte, tail) = cursor.split_first().expect("truncated WASM length/index");
        *cursor = tail;
        assert!(index != 4 || byte & 0xf0 == 0, "overflowing WASM u32");
        value |= u32::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return value;
        }
    }
    unreachable!("fifth WASM u32 byte cannot carry continuation")
}

/// Inspect the actual module's export section with bounded slice/ULEB reads.
/// This checks exported function names, not execution or full WASM validation.
fn wasm_function_exports(code: &[u8]) -> BTreeSet<String> {
    assert!(code.starts_with(b"\0asm\x01\0\0\0"), "version 1 WASM header");
    let mut cursor = &code[8..];
    let mut names = BTreeSet::new();
    let mut saw_exports = false;
    while let Some((&section, rest)) = cursor.split_first() {
        cursor = rest;
        let size = wasm_u32(&mut cursor) as usize;
        let (mut body, tail) = cursor.split_at_checked(size).expect("truncated WASM section");
        cursor = tail;
        if section != 7 {
            continue;
        }
        assert!(!saw_exports, "duplicate WASM export section");
        saw_exports = true;
        for _ in 0..wasm_u32(&mut body) {
            let size = wasm_u32(&mut body) as usize;
            let (name, tail) = body.split_at_checked(size).expect("truncated WASM export name");
            let name = std::str::from_utf8(name).expect("WASM export name is UTF-8");
            let (&kind, tail) = tail.split_first().expect("missing WASM export kind");
            assert!(kind <= 4, "invalid WASM export kind");
            body = tail;
            let _index = wasm_u32(&mut body);
            if kind == 0 {
                assert!(names.insert(name.to_owned()), "duplicate WASM function export");
            }
        }
        assert!(body.is_empty(), "trailing WASM export section bytes");
    }
    assert!(saw_exports, "WASM export section required");
    names
}

#[test]
fn journal_artifact_binding_is_current_without_starting_sandbox() -> testresult::TestResult<()> {
    let code = journal::executor_wasm()?;
    let exports = wasm_function_exports(&code);
    for name in [
        "new_with_withdrawal_journal",
        "get_withdrawal_journal_protocol",
        "get_journal_authorized_vaults",
        "get_journal_withdrawal_status",
        "withdraw_with_id",
        "cancel_withdrawal_with_id",
        "journal_resolve_withdrawal",
    ] {
        assert!(exports.contains(name), "actual modified Defuse lacks function export: {name}");
    }
    println!(
        "ARTIFACT | current source/lock/vendor binding and seven journal function exports verified | no sandbox or runtime behavior asserted"
    );
    Ok(())
}

async fn binding(w: &common::World) -> JournalProtocolDescriptor {
    near_sdk::serde_json::from_value(
        view(&w.vault, &w.network, "get_withdrawal_journal_binding", json!({})).await,
    )
    .unwrap()
}

fn request(w: &common::World, id: u64, amount: u128) -> JournalWithdrawalRequest {
    JournalWithdrawalRequest {
        withdrawal_id: U64(id),
        request_seq: U64(1),
        token: w.usdc.account_id().as_str().parse().unwrap(),
        receiver_id: w.stranger.as_str().parse().unwrap(),
        amount: U128(amount),
    }
}

async fn assert_bound(
    w: &common::World,
    record: &JournalWithdrawalRecord,
    expected: &JournalWithdrawalRequest,
) {
    assert!(record.matches_request(
        &binding(w).await,
        &w.vault.account_id().as_str().parse().unwrap(),
        expected
    ));
}

async fn assert_clear(w: &common::World) {
    assert_eq!(view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await, Value::Null);
    assert_eq!(
        view(&w.vault, &w.network, "get_withdrawal_recovery_state", json!({})).await,
        Value::Null
    );
    assert_eq!(view(&w.vault, &w.network, "is_in_flight", json!({})).await, json!(false));
}

async fn executor_storage_snapshot(
    w: &common::World,
) -> testresult::TestResult<BTreeMap<String, String>> {
    let fetched = w
        ._sandbox
        .patch_state(w.intents.account_id().as_str().parse()?)
        .fetch_from(&w._sandbox.rpc_addr, FetchData::new().storage())
        .await?;
    Ok(fetched
        .state
        .into_iter()
        .filter_map(|record| match record {
            StateRecord::Data { data_key_base64, value_base64, .. } => {
                Some((data_key_base64, value_base64))
            }
            _ => None,
        })
        .collect())
}

async fn executor_code_keys_storage(w: &common::World) -> testresult::TestResult<BTreeSet<String>> {
    let fetched = w
        ._sandbox
        .patch_state(w.intents.account_id().as_str().parse()?)
        .fetch_from(&w._sandbox.rpc_addr, FetchData::new().storage().code().access_keys())
        .await?;
    Ok(fetched
        .state
        .iter()
        .map(|record| {
            format!(
                "{:x}",
                Sha256::digest(
                    near_sdk::serde_json::to_vec(record).expect("sandbox state record JSON")
                )
            )
        })
        .collect())
}

async fn assert_maximum_profile(w: &common::World) {
    assert_eq!(w.vault.account_id().as_str().len(), 62);
    for id in [w.intents.account_id(), w.usdc.account_id(), &w.stranger] {
        assert_eq!(id.as_str().len(), 64);
    }
    let roles: BTreeSet<_> =
        [w.vault.account_id(), w.intents.account_id(), w.usdc.account_id(), &w.stranger]
            .into_iter()
            .collect();
    assert_eq!(roles.len(), 4, "maximum names must remain distinct valid roles");
    let admission = view(&w.intents, &w.network, "get_journal_authorized_vaults", json!({})).await;
    let entries = admission.as_array().unwrap();
    assert_eq!(entries.len(), 20);
    assert!(entries.windows(2).all(|pair| pair[0].as_str() < pair[1].as_str()));
    assert!(entries.iter().any(|entry| entry.as_str() == Some(w.vault.account_id().as_str())));
    assert!(entries.iter().all(|entry| matches!(entry.as_str().unwrap().len(), 62 | 64)));
    assert_eq!(
        view(&w.vault, &w.network, "get_mandate", json!({})).await["stable_assets"],
        json!([journal::asset_id(w)])
    );
}

/// Sandbox-only funding fixture: clone the real executor account, lower only
/// its NEAR amount while retaining existing storage stake plus the documented
/// operating margin, then restore it. Token custody and ledger are never patched.
async fn assert_underfunded_journal_refusal(
    w: &common::World,
    req: &JournalWithdrawalRequest,
) -> testresult::TestResult<()> {
    let original =
        Account(w.intents.account_id().clone()).view().fetch_from(&w.network).await?.data;
    let protected = executor_code_keys_storage(w).await?;
    let ledger = ledger_balance(w).await;
    let receiver = receiver_balance(w).await;
    let bytes =
        original.storage_usage.checked_add(4096).expect("storage usage plus operating margin");
    let amount = u128::from(bytes)
        .checked_mul(STORAGE_COST_PER_BYTE.as_yoctonear())
        .expect("NEAR storage stake");
    let mut lowered = original.clone();
    lowered.amount = NearToken::from_yoctonear(amount);
    assert!(lowered.amount < original.amount, "fixture must only lower the funded account amount");
    w._sandbox
        .patch_state(w.intents.account_id().as_str().parse()?)
        .account(lowered.clone())
        .send()
        .await?;
    assert_eq!(
        Account(w.intents.account_id().clone()).view().fetch_from(&w.network).await?.data,
        lowered
    );
    assert_eq!(
        executor_code_keys_storage(w).await?,
        protected,
        "amount fixture must preserve code, keys and storage"
    );
    for (method, gas) in [("withdraw_with_id", 100), ("cancel_withdrawal_with_id", 30)] {
        let failed = w
            .intents
            .call_function(method, json!({ "request": req }))
            .transaction()
            .gas(NearGas::from_tgas(gas))
            .deposit(NearToken::from_yoctonear(1))
            .with_signer(w.vault.account_id().clone(), w.signer.clone())
            .send_to(&w.network)
            .await?
            .assert_failure();
        assert!(
            format!("{:?}", failed.receipt_failures()).contains(
                "executor needs funded withdrawal journal storage and operating reserves"
            ),
            "must reach the business funding guard, not fail protocol stake: {:?}",
            failed.receipt_failures()
        );
        assert_eq!(
            failed
                .receipt_outcomes()
                .iter()
                .filter(|outcome| outcome.executor_id == *w.intents.account_id())
                .count(),
            1,
            "no executor resolver promise"
        );
        assert!(
            failed
                .receipt_outcomes()
                .iter()
                .all(|outcome| outcome.executor_id != *w.usdc.account_id()),
            "no token promise"
        );
        assert_eq!(executor_code_keys_storage(w).await?, protected);
        assert_eq!(storage_usage(w).await?, original.storage_usage);
        assert_eq!(ledger_balance(w).await, ledger);
        assert_eq!(receiver_balance(w).await, receiver);
        assert!(
            status(w, req.withdrawal_id).await.is_none(),
            "refusal cannot bind or allocate the ID"
        );
    }
    let mut restored =
        Account(w.intents.account_id().clone()).view().fetch_from(&w.network).await?.data;
    restored.amount = original.amount;
    w._sandbox
        .patch_state(w.intents.account_id().as_str().parse()?)
        .account(restored.clone())
        .send()
        .await?;
    assert_eq!(
        Account(w.intents.account_id().clone()).view().fetch_from(&w.network).await?.data,
        restored
    );
    assert_eq!(restored.contract_state, original.contract_state);
    assert_eq!(restored.locked, original.locked);
    assert_eq!(executor_code_keys_storage(w).await?, protected);
    println!(
        "FIXTURE | sandbox-only executor NEAR amount lowered to SDK storage stake +4096 operating bytes, then restored | code/keys/storage/token balances preserved"
    );
    Ok(())
}

#[tokio::test]
async fn journal_maximum_executor_inputs_enforce_funding_then_refund_cancel_and_deliver_at_minimum_gas()
-> testresult::TestResult<()> {
    let w = journal::maximum_world().await?;
    assert_maximum_profile(&w).await;
    // One MAX mint per fresh world avoids overflowing the sandbox token. The
    // refused transfer refunds first, so the same custody can later be delivered.
    fund_vault(&w, u128::MAX).await?;
    let mut req = request(&w, u64::MAX, u128::MAX);
    req.request_seq = U64(u64::MAX);
    assert_underfunded_journal_refusal(&w, &req).await?;
    let before_storage = storage_usage(&w).await?;
    block_receiver(&w, true).await?;
    let failed = executor_send(&w, "withdraw_with_id", &req, 100).await?;
    assert_eq!(failed.token_receipts, 1);
    assert_eq!(failed.executor_receipts, 2, "entry and actual private resolver receipts");
    assert_eq!(failed.failures, 1, "explicit ordinary FT refusal");
    let refunded = status(&w, req.withdrawal_id).await.unwrap();
    assert_bound(&w, &refunded, &req).await;
    assert_eq!(failed.answer, json!(refunded));
    assert_eq!(
        refunded.state,
        JournalWithdrawalState::Refunded { refunded_amount: U128(u128::MAX) }
    );
    assert_eq!(ledger_balance(&w).await, u128::MAX);
    assert_eq!(receiver_balance(&w).await, 0);
    let duplicate = executor_send(&w, "withdraw_with_id", &req, 30).await?;
    assert_eq!(duplicate.answer, json!(refunded));
    assert_eq!(duplicate.token_receipts, 0);
    let cancelled_request =
        JournalWithdrawalRequest { withdrawal_id: U64(u64::MAX - 1), ..req.clone() };
    let cancelled = executor_send(&w, "cancel_withdrawal_with_id", &cancelled_request, 30).await?;
    let fence = status(&w, cancelled_request.withdrawal_id).await.unwrap();
    assert_bound(&w, &fence, &cancelled_request).await;
    assert_eq!(fence.state, JournalWithdrawalState::Cancelled);
    assert_eq!(cancelled.answer, json!(fence));
    assert_eq!(cancelled.token_receipts, 0);
    let late = executor_send(&w, "withdraw_with_id", &cancelled_request, 100).await?;
    assert_eq!(late.answer, json!(fence));
    assert_eq!(late.token_receipts, 0);
    assert_eq!(ledger_balance(&w).await, u128::MAX);
    block_receiver(&w, false).await?;
    let delivery_request =
        JournalWithdrawalRequest { withdrawal_id: U64(u64::MAX - 2), ..req.clone() };
    let delivered = executor_send(&w, "withdraw_with_id", &delivery_request, 100).await?;
    assert_eq!(delivered.token_receipts, 1);
    assert_eq!(delivered.executor_receipts, 2);
    assert_eq!(delivered.failures, 0);
    let record = status(&w, delivery_request.withdrawal_id).await.unwrap();
    assert_bound(&w, &record, &delivery_request).await;
    assert_eq!(delivered.answer, json!(record));
    assert_eq!(
        record.state,
        JournalWithdrawalState::Delivered { delivered_amount: U128(u128::MAX) }
    );
    assert_eq!(ledger_balance(&w).await, 0);
    assert_eq!(receiver_balance(&w).await, u128::MAX);
    assert_eq!(status(&w, req.withdrawal_id).await, Some(refunded));
    assert_eq!(status(&w, cancelled_request.withdrawal_id).await, Some(fence));
    println!(
        "GAS | max tuple/20 admissions |100 TGas entry, actual40 TGas resolver allocation | refund {:.2}, delivery {:.2} TGas burnt",
        failed.gas as f64 / 1e12,
        delivered.gas as f64 / 1e12
    );
    println!(
        "GAS | max tuple cancellation |30 TGas attached | {:.2} TGas burnt",
        cancelled.gas as f64 / 1e12
    );
    println!(
        "STORAGE | max Refunded+Cancelled+Delivered durable records | {} bytes",
        storage_usage(&w).await? - before_storage
    );
    Ok(())
}

#[tokio::test]
async fn journal_maximum_vault_profile_runs_280_gas_refund_delivery_and_140_gas_cancellation_fixture()
-> testresult::TestResult<()> {
    let w = journal::maximum_world().await?;
    assert_maximum_profile(&w).await;
    fund_vault(&w, u128::MAX).await?;
    let authorized = queue_mature(&w, u128::MAX).await?;
    block_receiver(&w, true).await?;
    let refunded = send(&w, "apply_pending_withdrawal", json!({}), 280, 0).await?;
    assert_eq!(refunded.answer, json!(false));
    assert_eq!(refunded.token_receipts, 1);
    assert_eq!(refunded.failures, 1);
    let refund_record = status(&w, U64(1)).await.unwrap();
    assert_bound(&w, &refund_record, &request(&w, 1, u128::MAX)).await;
    assert_eq!(
        refund_record.state,
        JournalWithdrawalState::Refunded { refunded_amount: U128(u128::MAX) }
    );
    assert_eq!(ledger_balance(&w).await, u128::MAX);
    assert_eq!(receiver_balance(&w).await, 0);
    assert_clear(&w).await;
    assert_eq!(
        view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
        json!(authorized)
    );
    assert_eq!(
        view(&w.vault, &w.network, "is_withdrawal_retry_armed", json!({})).await,
        json!(false)
    );
    block_receiver(&w, false).await?;
    send(&w, "rearm_pending_withdrawal", json!({}), 30, 1).await?;
    let delivered = send(&w, "apply_pending_withdrawal", json!({}), 280, 0).await?;
    assert_eq!(delivered.answer, json!(true));
    assert_eq!(delivered.token_receipts, 1);
    assert_eq!(delivered.failures, 0);
    let delivery_record = status(&w, U64(2)).await.unwrap();
    assert_bound(&w, &delivery_record, &request(&w, 2, u128::MAX)).await;
    assert_eq!(
        delivery_record.state,
        JournalWithdrawalState::Delivered { delivered_amount: U128(u128::MAX) }
    );
    assert_eq!(ledger_balance(&w).await, 0);
    assert_eq!(receiver_balance(&w).await, u128::MAX);
    assert_clear(&w).await;
    // A labelled root-state fixture creates an absent external attempt; 140
    // TGas recovery must write a real MAX-amount tombstone before any debit.
    let absent = install_dispatched_fixture(&w, u128::MAX).await?;
    assert!(status(&w, absent.withdrawal_id).await.is_none());
    let cancelled = send(
        &w,
        "recover_withdrawal_attempt",
        json!({ "withdrawal_id": absent.withdrawal_id }),
        140,
        0,
    )
    .await?;
    assert_eq!(cancelled.answer, json!(true));
    assert_eq!(cancelled.token_receipts, 0);
    let fence = status(&w, absent.withdrawal_id).await.unwrap();
    assert_bound(&w, &fence, &absent).await;
    assert_eq!(fence.state, JournalWithdrawalState::Cancelled);
    assert_clear(&w).await;
    assert_eq!(
        view(&w.vault, &w.network, "is_withdrawal_retry_armed", json!({})).await,
        json!(false)
    );
    let late = executor_send(&w, "withdraw_with_id", &absent, 100).await?;
    assert_eq!(late.answer, json!(fence));
    assert_eq!(late.token_receipts, 0);
    assert_eq!(ledger_balance(&w).await, 0);
    assert_eq!(receiver_balance(&w).await, u128::MAX);
    println!(
        "GAS | max vault profile/20 admissions |280 TGas | refund {:.2}, rearmed delivery {:.2} TGas burnt",
        refunded.gas as f64 / 1e12,
        delivered.gas as f64 / 1e12
    );
    println!(
        "GAS | max vault cancellation Dispatched fixture |140 TGas | {:.2} TGas burnt",
        cancelled.gas as f64 / 1e12
    );
    Ok(())
}

#[tokio::test]
async fn journal_wasm_delivers_exactly_once_and_persists_the_bound_terminal_record()
-> testresult::TestResult<()> {
    let w = world().await?;
    assert_eq!(
        view(&w.vault, &w.network, "get_withdrawal_protocol", json!({})).await,
        PROTOCOL_VERSION
    );
    assert_eq!(
        view(&w.intents, &w.network, "get_withdrawal_journal_protocol", json!({})).await,
        json!(binding(&w).await)
    );
    assert_eq!(
        view(&w.intents, &w.network, "get_journal_authorized_vaults", json!({})).await,
        json!([w.vault.account_id()])
    );
    fund_vault(&w, 5_000_000).await?;
    let before_storage = storage_usage(&w).await?;
    queue_mature(&w, 1_000_000).await?;
    let sent = send(&w, "apply_pending_withdrawal", json!({}), 280, 0).await?;
    assert_eq!(sent.answer, json!(true));
    assert_eq!(sent.failures, 0);
    assert_eq!(sent.token_receipts, 1, "one ordinary FT transfer receipt");
    assert_eq!(ledger_balance(&w).await, 4_000_000);
    assert_eq!(receiver_balance(&w).await, 1_000_000);
    let terminal = status(&w, U64(1)).await.expect("durable Delivered");
    assert_bound(&w, &terminal, &request(&w, 1, 1_000_000)).await;
    assert_eq!(
        terminal.state,
        JournalWithdrawalState::Delivered { delivered_amount: U128(1_000_000) }
    );
    assert_clear(&w).await;
    assert_eq!(view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await, Value::Null);
    let duplicate = send(&w, "apply_pending_withdrawal", json!({}), 280, 0)
        .await
        .err()
        .expect("successful withdrawal cannot resend");
    assert!(duplicate.contains("no withdrawal is queued"), "{duplicate}");
    assert_eq!(ledger_balance(&w).await, 4_000_000);
    assert_eq!(receiver_balance(&w).await, 1_000_000);
    let delta = storage_usage(&w).await? - before_storage;
    assert!(delta >= 4100, "durable record storage remains after reserve is released");
    println!(
        "GAS | actual journal vault delivery | 280 TGas attached | {:.2} TGas burnt",
        sent.gas as f64 / 1e12
    );
    println!("STORAGE | actual journal Delivered delta | {delta} bytes");
    Ok(())
}

#[tokio::test]
async fn journal_wasm_explicit_ft_failure_refunds_then_requires_owner_rearm_and_new_id()
-> testresult::TestResult<()> {
    let w = world().await?;
    fund_vault(&w, 100).await?;
    let before_storage = storage_usage(&w).await?;
    let authorized = queue_mature(&w, 20).await?;
    block_receiver(&w, true).await?;
    let failed = send(&w, "apply_pending_withdrawal", json!({}), 280, 0).await?;
    assert_eq!(failed.answer, json!(false), "refund is not delivery");
    assert_eq!(failed.token_receipts, 1);
    assert_eq!(failed.failures, 1, "explicit FT refusal receipt is observed");
    assert_eq!(ledger_balance(&w).await, 100, "native debit is fully restored");
    assert_eq!(receiver_balance(&w).await, 0);
    let refunded = status(&w, U64(1)).await.unwrap();
    assert_bound(&w, &refunded, &request(&w, 1, 20)).await;
    assert_eq!(refunded.state, JournalWithdrawalState::Refunded { refunded_amount: U128(20) });
    assert_clear(&w).await;
    assert_eq!(
        view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
        json!(authorized)
    );
    assert_eq!(
        view(&w.vault, &w.network, "is_withdrawal_retry_armed", json!({})).await,
        json!(false)
    );
    let unarmed = send(&w, "apply_pending_withdrawal", json!({}), 280, 0)
        .await
        .err()
        .expect("owner rearm mandatory");
    assert!(unarmed.contains("owner must re-arm"), "{unarmed}");
    let duplicate = executor_send(&w, "withdraw_with_id", &request(&w, 1, 20), 30).await?;
    assert_eq!(duplicate.answer, json!(refunded));
    assert_eq!(duplicate.token_receipts, 0, "terminal duplicate never retries FT");
    assert_eq!(ledger_balance(&w).await, 100);
    block_receiver(&w, false).await?;
    send(&w, "rearm_pending_withdrawal", json!({}), 30, 1).await?;
    assert_eq!(
        view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
        json!(authorized)
    );
    let retry = send(&w, "apply_pending_withdrawal", json!({}), 280, 0).await?;
    assert_eq!(retry.answer, json!(true));
    assert_eq!(retry.token_receipts, 1);
    assert_eq!(ledger_balance(&w).await, 80);
    assert_eq!(receiver_balance(&w).await, 20);
    assert_eq!(status(&w, U64(1)).await, Some(refunded));
    let second = status(&w, U64(2)).await.unwrap();
    assert_bound(&w, &second, &request(&w, 2, 20)).await;
    assert_eq!(second.state, JournalWithdrawalState::Delivered { delivered_amount: U128(20) });
    assert_clear(&w).await;
    println!(
        "GAS | actual journal explicit FT failure + atomic native refund | {:.2} TGas",
        failed.gas as f64 / 1e12
    );
    println!(
        "GAS | actual journal owner-rearmed new-ID retry | {:.2} TGas",
        retry.gas as f64 / 1e12
    );
    println!(
        "STORAGE | actual journal Refunded + Delivered delta | {} bytes",
        storage_usage(&w).await? - before_storage
    );
    Ok(())
}

#[tokio::test]
async fn journal_executor_exact_duplicate_returns_record_and_changed_tuple_cannot_resend()
-> testresult::TestResult<()> {
    let w = world().await?;
    fund_vault(&w, 100).await?;
    let req = request(&w, 7, 20);
    let first = executor_send(&w, "withdraw_with_id", &req, 100).await?;
    assert_eq!(first.token_receipts, 1);
    let record = status(&w, req.withdrawal_id).await.unwrap();
    assert_eq!(first.answer, json!(record));
    assert_bound(&w, &record, &req).await;
    let before_storage = storage_usage(&w).await?;
    for method in ["withdraw_with_id", "cancel_withdrawal_with_id"] {
        let duplicate = executor_send(&w, method, &req, 30).await?;
        assert_eq!(duplicate.answer, json!(record));
        assert_eq!(duplicate.token_receipts, 0);
        assert_eq!(storage_usage(&w).await?, before_storage);
    }
    for changed in [
        JournalWithdrawalRequest { amount: U128(21), ..req.clone() },
        JournalWithdrawalRequest { request_seq: U64(2), ..req.clone() },
        JournalWithdrawalRequest { receiver_id: w.owner.as_str().parse()?, ..req.clone() },
    ] {
        for method in ["withdraw_with_id", "cancel_withdrawal_with_id"] {
            let failed = w
                .intents
                .call_function(method, json!({ "request": changed }))
                .transaction()
                .gas(NearGas::from_tgas(100))
                .deposit(NearToken::from_yoctonear(1))
                .with_signer(w.vault.account_id().clone(), w.signer.clone())
                .send_to(&w.network)
                .await?
                .assert_failure();
            assert!(
                failed.receipt_outcomes().iter().all(|o| o.executor_id != *w.usdc.account_id())
            );
        }
    }
    assert_eq!(status(&w, req.withdrawal_id).await, Some(record));
    assert_eq!(ledger_balance(&w).await, 80);
    assert_eq!(receiver_balance(&w).await, 20);
    Ok(())
}

#[tokio::test]
async fn journal_executor_minimum_gas_and_predecessor_auth_refuse_without_storage_debit_or_token_promises()
-> testresult::TestResult<()> {
    let w = world().await?;
    fund_vault(&w, 100).await?;
    let before = executor_storage_snapshot(&w).await?;
    let usage = storage_usage(&w).await?;
    // Fresh IDs are essential: an existing exact duplicate deliberately returns
    // its durable record before checking the allocation/dispatch gas minimum.
    for (method, gas, id, source, reason) in [
        ("withdraw_with_id", 99, 31, w.vault.account_id(), "requires at least 100 TGas"),
        ("cancel_withdrawal_with_id", 29, 31, w.vault.account_id(), "requires at least 30 TGas"),
        ("withdraw_with_id", 100, 32, &w.stranger, "vault is not admitted"),
        ("cancel_withdrawal_with_id", 30, 32, &w.stranger, "vault is not admitted"),
    ] {
        let failed = w
            .intents
            .call_function(method, json!({ "request": request(&w, id, 20) }))
            .transaction()
            .gas(NearGas::from_tgas(gas))
            .deposit(NearToken::from_yoctonear(1))
            .with_signer(source.clone(), w.signer.clone())
            .send_to(&w.network)
            .await?
            .assert_failure();
        assert!(
            format!("{:?}", failed.receipt_failures()).contains(reason),
            "expected {reason}, got {:?}",
            failed.receipt_failures()
        );
        assert_eq!(
            failed
                .receipt_outcomes()
                .iter()
                .filter(|o| o.executor_id == *w.intents.account_id())
                .count(),
            1,
            "no executor resolver promise"
        );
        assert!(
            failed.receipt_outcomes().iter().all(|o| o.executor_id != *w.usdc.account_id()
                && o.executor_id != *w.oracle.account_id()
                && o.executor_id != *w.mpc.account_id()),
            "no application cross-contract promise; protocol gas/deposit refund receipts are allowed"
        );
        assert_eq!(executor_storage_snapshot(&w).await?, before);
        assert_eq!(storage_usage(&w).await?, usage);
        assert_eq!(ledger_balance(&w).await, 100);
        assert_eq!(receiver_balance(&w).await, 0);
        assert!(status(&w, U64(id)).await.is_none());
        assert_eq!(
            view(
                &w.intents,
                &w.network,
                "get_journal_withdrawal_status",
                json!({ "vault_id": source, "withdrawal_id": id.to_string() })
            )
            .await,
            Value::Null
        );
    }
    // Exercise native predecessor authentication separately from admission:
    // this admitted vault still fails after it disables predecessor authority.
    // The sandbox key is explicit fixture control, not production impersonation.
    w.intents
        .call_function("disable_auth_by_predecessor_id", json!({}))
        .transaction()
        .gas(NearGas::from_tgas(30))
        .deposit(NearToken::from_yoctonear(1))
        .with_signer(w.vault.account_id().clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    assert_eq!(
        view(
            &w.intents,
            &w.network,
            "is_auth_by_predecessor_id_enabled",
            json!({ "account_id": w.vault.account_id() })
        )
        .await,
        json!(false)
    );
    let disabled = executor_storage_snapshot(&w).await?;
    let disabled_usage = storage_usage(&w).await?;
    for (method, gas) in [("withdraw_with_id", 100), ("cancel_withdrawal_with_id", 30)] {
        let failed = w
            .intents
            .call_function(method, json!({ "request": request(&w, 33, 20) }))
            .transaction()
            .gas(NearGas::from_tgas(gas))
            .deposit(NearToken::from_yoctonear(1))
            .with_signer(w.vault.account_id().clone(), w.signer.clone())
            .send_to(&w.network)
            .await?
            .assert_failure();
        assert!(
            format!("{:?}", failed.receipt_failures())
                .contains("authentication by PREDECESSOR_ID is disabled")
        );
        assert_eq!(
            failed
                .receipt_outcomes()
                .iter()
                .filter(|o| o.executor_id == *w.intents.account_id())
                .count(),
            1
        );
        assert!(failed.receipt_outcomes().iter().all(|o| o.executor_id != *w.usdc.account_id()
            && o.executor_id != *w.oracle.account_id()
            && o.executor_id != *w.mpc.account_id()));
        assert_eq!(executor_storage_snapshot(&w).await?, disabled);
        assert_eq!(storage_usage(&w).await?, disabled_usage);
        assert_eq!(ledger_balance(&w).await, 100);
        assert_eq!(receiver_balance(&w).await, 0);
        assert!(status(&w, U64(33)).await.is_none());
    }
    Ok(())
}

#[tokio::test]
async fn journal_executor_absent_cancelled_tombstone_fences_late_originals_without_debit()
-> testresult::TestResult<()> {
    let w = world().await?;
    fund_vault(&w, 100).await?;
    let req = request(&w, 17, 20);
    let before_storage = storage_usage(&w).await?;
    let cancelled = executor_send(&w, "cancel_withdrawal_with_id", &req, 30).await?;
    assert_eq!(cancelled.token_receipts, 0);
    let record = status(&w, req.withdrawal_id).await.unwrap();
    assert_bound(&w, &record, &req).await;
    assert_eq!(record.state, JournalWithdrawalState::Cancelled);
    assert_eq!(cancelled.answer, json!(record));
    for _ in 0..2 {
        let late = executor_send(&w, "withdraw_with_id", &req, 100).await?;
        assert_eq!(late.answer, json!(record));
        assert_eq!(late.token_receipts, 0);
        assert_eq!(ledger_balance(&w).await, 100);
        assert_eq!(receiver_balance(&w).await, 0);
    }
    assert!(storage_usage(&w).await? - before_storage >= 4100);
    println!(
        "GAS | actual journal absent-key cancellation | {:.2} TGas",
        cancelled.gas as f64 / 1e12
    );
    Ok(())
}

#[tokio::test]
async fn journal_preparing_rejects_plain_executor_domain_mismatch_and_missing_admission()
-> testresult::TestResult<()> {
    for case in [BindingCase::PlainExecutor, BindingCase::DifferentDomain, BindingCase::NotAdmitted]
    {
        let w = world_with_binding(case).await?;
        fund_vault(&w, 100).await?;
        let authorized = queue_mature(&w, 20).await?;
        let before_storage = storage_usage(&w).await?;
        let rejected = send(&w, "apply_pending_withdrawal", json!({}), 280, 0).await?;
        assert_eq!(rejected.answer, json!(false));
        assert_eq!(rejected.token_receipts, 0, "Preparing never sends an FT");
        assert_eq!(rejected.executor_receipts, 2, "only descriptor and admission views");
        assert_eq!(ledger_balance(&w).await, 100);
        assert_eq!(receiver_balance(&w).await, 0);
        assert_eq!(
            storage_usage(&w).await?,
            before_storage,
            "no executor tombstone or debit during preparation"
        );
        assert!(status(&w, U64(1)).await.is_none());
        assert_clear(&w).await;
        assert_eq!(
            view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
            json!(authorized)
        );
        assert_eq!(
            view(&w.vault, &w.network, "is_withdrawal_retry_armed", json!({})).await,
            json!(false)
        );
    }
    Ok(())
}

#[tokio::test]
async fn journal_recovery_uses_real_terminal_fact_with_missing_vault_callback_fixture_and_fences_stale_ids()
-> testresult::TestResult<()> {
    let w = world().await?;
    fund_vault(&w, 100).await?;
    let req = install_dispatched_fixture(&w, 20).await?;
    let sent = executor_send(&w, "withdraw_with_id", &req, 100).await?;
    assert_eq!(sent.token_receipts, 1);
    assert_eq!(
        status(&w, req.withdrawal_id).await.unwrap().state,
        JournalWithdrawalState::Delivered { delivered_amount: U128(20) }
    );
    fund_vault(&w, 25).await?;
    assert_eq!(
        ledger_balance(&w).await,
        105,
        "unrelated credit masks the debit but not the record"
    );
    let recovered = send(
        &w,
        "recover_withdrawal_attempt",
        json!({ "withdrawal_id": req.withdrawal_id }),
        140,
        0,
    )
    .await?;
    assert_eq!(recovered.answer, json!(true));
    assert_eq!(recovered.token_receipts, 0, "recovery does not resend");
    assert_eq!(ledger_balance(&w).await, 105);
    assert_eq!(receiver_balance(&w).await, 20);
    assert_clear(&w).await;
    let newer = install_dispatched_fixture(&w, 15).await?;
    assert!(newer.withdrawal_id.0 > req.withdrawal_id.0);
    let active = view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await;
    let snapshot = view(&w.vault, &w.network, "get_withdrawal_recovery_state", json!({})).await;
    assert_eq!(snapshot["protocol"], PROTOCOL_VERSION);
    assert_eq!(snapshot["withdrawal_id"], json!(newer.withdrawal_id));
    let stale = send(
        &w,
        "recover_withdrawal_attempt",
        json!({ "withdrawal_id": req.withdrawal_id }),
        30,
        0,
    )
    .await?;
    assert_eq!(stale.answer, json!(false));
    assert_eq!(stale.executor_receipts, 0);
    assert_eq!(view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await, active);
    assert_eq!(
        view(&w.vault, &w.network, "get_withdrawal_recovery_state", json!({})).await,
        snapshot
    );
    let cancelled = send(
        &w,
        "recover_withdrawal_attempt",
        json!({ "withdrawal_id": newer.withdrawal_id }),
        140,
        0,
    )
    .await?;
    assert_eq!(cancelled.answer, json!(true), "Cancelled resolves the guard");
    assert_eq!(cancelled.token_receipts, 0);
    let fence = status(&w, newer.withdrawal_id).await.unwrap();
    assert_eq!(fence.state, JournalWithdrawalState::Cancelled);
    for _ in 0..2 {
        let late = executor_send(&w, "withdraw_with_id", &newer, 100).await?;
        assert_eq!(late.answer, json!(fence));
        assert_eq!(late.token_receipts, 0);
    }
    assert_eq!(ledger_balance(&w).await, 105);
    assert_eq!(receiver_balance(&w).await, 20);
    assert_eq!(
        view(&w.vault, &w.network, "is_withdrawal_retry_armed", json!({})).await,
        json!(false)
    );
    send(&w, "rearm_pending_withdrawal", json!({}), 30, 1).await?;
    let retry = send(&w, "apply_pending_withdrawal", json!({}), 280, 0).await?;
    assert_eq!(retry.answer, json!(true));
    assert_eq!(retry.token_receipts, 1);
    assert_eq!(ledger_balance(&w).await, 90);
    assert_eq!(receiver_balance(&w).await, 35);
    let last = view(&w.vault, &w.network, "get_last_withdrawal_outcome", json!({})).await;
    assert_eq!(last["Delivered"]["withdrawal_id"], (newer.withdrawal_id.0 + 1).to_string());
    println!(
        "GAS | real durable Delivered reconciliation with missing-vault-callback fixture | {:.2} TGas",
        recovered.gas as f64 / 1e12
    );
    Ok(())
}

#[tokio::test]
async fn journal_pending_terminal_fact_fixture_keeps_guard_and_never_refunds_or_resends()
-> testresult::TestResult<()> {
    let w = world().await?;
    fund_vault(&w, 100).await?;
    let req = install_dispatched_fixture(&w, 20).await?;
    executor_send(&w, "withdraw_with_id", &req, 100).await?;
    install_pending_fact_fixture(&w, &status(&w, req.withdrawal_id).await.unwrap()).await?;
    let active = view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await;
    let snapshot = view(&w.vault, &w.network, "get_withdrawal_recovery_state", json!({})).await;
    let pending = status(&w, req.withdrawal_id).await.unwrap();
    for _ in 0..2 {
        let result = send(
            &w,
            "recover_withdrawal_attempt",
            json!({ "withdrawal_id": req.withdrawal_id }),
            140,
            0,
        )
        .await?;
        assert_eq!(result.answer, json!(false));
        assert_eq!(result.token_receipts, 0);
        assert_eq!(view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await, active);
        assert_eq!(
            view(&w.vault, &w.network, "get_withdrawal_recovery_state", json!({})).await,
            snapshot
        );
        assert_eq!(
            view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
            Value::Null
        );
        assert_eq!(view(&w.vault, &w.network, "is_in_flight", json!({})).await, json!(true));
        assert_eq!(status(&w, req.withdrawal_id).await, Some(pending.clone()));
        assert_eq!(ledger_balance(&w).await, 80);
        assert_eq!(receiver_balance(&w).await, 20);
    }
    Ok(())
}

#[tokio::test]
async fn journal_actual_inconsistent_request_fact_is_rejected_without_releasing_guard()
-> testresult::TestResult<()> {
    let w = world().await?;
    fund_vault(&w, 100).await?;
    let req = install_dispatched_fixture(&w, 20).await?;
    let changed = JournalWithdrawalRequest { amount: U128(21), ..req.clone() };
    // The public sandbox fixture key deliberately supplies a different tuple.
    // This is not an authority available to an external production caller.
    executor_send(&w, "withdraw_with_id", &changed, 100).await?;
    let record = status(&w, req.withdrawal_id).await.unwrap();
    assert_eq!(record.request, changed);
    let active = view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await;
    let recovery = send(
        &w,
        "recover_withdrawal_attempt",
        json!({ "withdrawal_id": req.withdrawal_id }),
        140,
        0,
    )
    .await?;
    assert_eq!(recovery.answer, json!(false));
    assert_eq!(recovery.token_receipts, 0);
    assert_eq!(recovery.failures, 1, "executor refuses cancellation with changed tuple");
    assert_eq!(view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await, active);
    assert_eq!(view(&w.vault, &w.network, "is_in_flight", json!({})).await, json!(true));
    assert_eq!(status(&w, req.withdrawal_id).await, Some(record));
    assert_eq!(ledger_balance(&w).await, 79);
    assert_eq!(receiver_balance(&w).await, 21);
    Ok(())
}
