//! Full sandbox withdrawal chain, including the vault's balance read and final callback.
//!
//! The early call uses the real queued 24-hour deadline. If `sandbox_fast_forward`
//! advances block height without advancing time, only the pending request's maturity
//! is patched in sandbox STATE before the successful call. That fixture is not evidence
//! that 24 real hours elapsed, and adds no maturity bypass to the production contract.

mod common;

use common::{USDC, World, fund_vault, view, world};
use mandate_vault::{PendingWithdrawal, Vault, WITHDRAW_DELAY_SEC, WITHDRAW_RECOVERY_SEC};
use near_api::types::transaction::actions::{Action, FunctionCallAction};
use near_api::{NearGas, NearToken, Transaction};
use near_sandbox::{FetchData, sandbox::patch::StateRecord};
use near_sdk::base64::Engine;
use near_sdk::base64::engine::general_purpose::STANDARD as BASE64;
use near_sdk::borsh;
use near_sdk::serde_json::{Value, json};
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::{PromiseResult, testing_env};

async fn chain_now(w: &World) -> u64 {
    view(&w.vault, &w.network, "get_chain_time_sec", json!({}))
        .await
        .as_u64()
        .expect("chain time is a u64")
}

async fn send(
    w: &World,
    method: &str,
    args: Value,
    gas_tgas: u64,
    yocto: u128,
) -> Result<(Value, u64), String> {
    let result = w
        .vault
        .call_function(method, args)
        .transaction()
        .gas(NearGas::from_tgas(gas_tgas))
        .deposit(NearToken::from_yoctonear(yocto))
        .with_signer(w.owner.clone(), w.signer.clone())
        .send_to(&w.network)
        .await
        .map_err(|e| e.to_string())?
        .into_result()
        .map_err(|e| format!("{e:?}"))?;
    let gas_burnt = result.total_gas_burnt.as_gas();
    Ok((result.json::<Value>().unwrap_or(Value::Null), gas_burnt))
}

async fn set_withdrawal_ceiling(w: &World, max_per_request: u128) -> testresult::TestResult<()> {
    let mut policy = view(&w.vault, &w.network, "get_mandate", json!({})).await;
    policy["withdrawal_limits"] = json!([{
        "asset_id": USDC, "max_per_request": max_per_request.to_string(),
    }]);
    let (outcome, _) = send(w, "set_mandate", json!({ "mandate": policy }), 30, 1).await?;
    assert_eq!(outcome, json!("Applied"), "adding/tightening the ceiling must apply immediately");
    assert_eq!(view(&w.vault, &w.network, "get_mandate", json!({})).await, policy);
    Ok(())
}

/// Sandbox-only prestate fixture. Locate the complete serialized pending request
/// uniquely, round-trip the current Vault layout, and alter only effective_sec.
async fn patch_only_withdrawal_maturity(
    w: &World,
    pending: &PendingWithdrawal,
) -> testresult::TestResult<()> {
    let vault_id = w.vault.account_id().as_str().parse()?;
    let fetched = w
        ._sandbox
        .patch_state(vault_id)
        .fetch_from(&w._sandbox.rpc_addr, FetchData::new().storage())
        .await?;
    let state_name = BASE64.encode(b"STATE");
    let state_values: Vec<_> = fetched
        .state
        .into_iter()
        .filter_map(|record| match record {
            StateRecord::Data { data_key_base64, value_base64, .. }
                if data_key_base64 == state_name =>
            {
                Some(value_base64)
            }
            _ => None,
        })
        .collect();
    assert_eq!(state_values.len(), 1, "exactly one root STATE is required");
    let original = BASE64.decode(&state_values[0])?;
    let decoded: Vault = borsh::from_slice(&original)?;
    assert!(borsh::to_vec(&decoded)? == original, "current state must round-trip");

    let request_bytes = borsh::to_vec(pending)?;
    let offsets: Vec<_> = original
        .windows(request_bytes.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == request_bytes).then_some(offset))
        .collect();
    assert_eq!(offsets.len(), 1, "the complete pending request must occur once");
    let maturity_offset = offsets[0] + request_bytes.len() - size_of::<u64>();
    let matured_sec = chain_now(w).await;
    let mut patched = original.clone();
    patched[maturity_offset..maturity_offset + size_of::<u64>()]
        .copy_from_slice(&matured_sec.to_le_bytes());
    assert!(patched[..maturity_offset] == original[..maturity_offset]);
    assert!(patched[maturity_offset + 8..] == original[maturity_offset + 8..]);
    let decoded: Vault = borsh::from_slice(&patched)?;
    assert!(borsh::to_vec(&decoded)? == patched, "patched state must round-trip");
    w._sandbox
        .patch_state(w.vault.account_id().as_str().parse()?)
        .storage(state_name, BASE64.encode(patched))
        .send()
        .await?;

    let mut expected = pending.clone();
    expected.effective_sec = matured_sec;
    assert_eq!(
        view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
        near_sdk::serde_json::to_value(expected)?,
        "the maturity fixture must preserve amount, receiver, and asset"
    );
    Ok(())
}

async fn reach_maturity(
    w: &World,
    pending: &PendingWithdrawal,
) -> testresult::TestResult<&'static str> {
    let before = chain_now(w).await;
    tokio::time::timeout(std::time::Duration::from_secs(30), w._sandbox.fast_forward(1000))
        .await??;
    let after = chain_now(w).await;
    let advanced = after.saturating_sub(before);
    println!("CLOCK | sandbox_fast_forward 1000 blocks | {advanced} chain seconds");
    if after >= pending.effective_sec {
        return Ok("sandbox block-time advancement");
    }
    // Large block jumps are not bounded by the SDK's post-request polling timeout.
    // Do not extrapolate a small probe into an unbounded 24-hour fast-forward request.
    patch_only_withdrawal_maturity(w, pending).await?;
    Ok("sandbox-only STATE maturity fixture")
}

async fn ledger_balance(w: &World) -> u128 {
    view(
        &w.intents,
        &w.network,
        "mt_balance_of",
        json!({ "account_id": w.vault.account_id(), "token_id": USDC }),
    )
    .await
    .as_str()
    .expect("ledger amount is a string")
    .parse()
    .expect("ledger amount is u128")
}

async fn receiver_balance(w: &World) -> u128 {
    view(&w.usdc, &w.network, "ft_balance_of", json!({ "account_id": w.stranger }))
        .await
        .as_str()
        .expect("FT amount is a string")
        .parse()
        .expect("FT amount is u128")
}

#[tokio::test]
async fn vault_withdrawal_reads_sends_and_confirms_the_actual_transfer()
-> testresult::TestResult<()> {
    let w = world().await?;
    fund_vault(&w, 5_000_000).await?;
    let ledger_before = ledger_balance(&w).await;
    let receiver_before = receiver_balance(&w).await;
    assert_eq!(ledger_before, 5_000_000);
    set_withdrawal_ceiling(&w, 1_000_000).await?;
    let too_large = send(
        &w,
        "request_withdrawal",
        json!({ "asset_id": USDC, "receiver": w.stranger, "amount": "1000001" }),
        30,
        1,
    )
    .await
    .expect_err("the deployed WASM must refuse one base unit above the ceiling");
    assert!(too_large.contains("the withdrawal exceeds the per-request ceiling"), "{too_large}");
    assert_eq!(view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await, json!(null));
    assert_eq!(view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await, json!(null));
    assert_eq!(
        view(&w.vault, &w.network, "get_last_withdrawal_outcome", json!({})).await,
        json!(null)
    );
    assert_eq!(
        view(&w.vault, &w.network, "is_withdrawal_retry_armed", json!({})).await,
        json!(false)
    );
    assert_eq!(ledger_balance(&w).await, ledger_before);
    assert_eq!(receiver_balance(&w).await, receiver_before);
    let queued_at = chain_now(&w).await;
    send(
        &w,
        "request_withdrawal",
        json!({ "asset_id": USDC, "receiver": w.stranger, "amount": "1000000" }),
        300,
        1,
    )
    .await?;
    let queued = view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await;
    let pending: PendingWithdrawal = near_sdk::serde_json::from_value(queued.clone())?;
    assert!(pending.effective_sec >= queued_at + WITHDRAW_DELAY_SEC);
    let early = send(&w, "apply_pending_withdrawal", json!({}), 300, 0)
        .await
        .expect_err("the actual queued deadline must refuse an early send");
    assert!(early.contains("still in its delay"), "{early}");
    assert_eq!(view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await, queued);
    assert_eq!(ledger_balance(&w).await, ledger_before);
    assert_eq!(receiver_balance(&w).await, receiver_before);

    let maturity_mode = reach_maturity(&w, &pending).await?;
    // Use the production method's stated minimum, not extra gas donated by the caller.
    let (answer, gas_burnt) = send(&w, "apply_pending_withdrawal", json!({}), 280, 0).await?;
    assert_eq!(answer, json!(true), "the final vault callback must confirm delivery");
    assert_eq!(ledger_balance(&w).await, ledger_before - 1_000_000);
    assert_eq!(receiver_balance(&w).await, receiver_before + 1_000_000);
    assert_eq!(view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await, json!(null));
    assert_eq!(view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await, json!(null));
    assert_eq!(
        view(&w.vault, &w.network, "get_last_withdrawal_outcome", json!({})).await,
        json!({ "Delivered": { "withdrawal_id": "1", "amount": "1000000" } })
    );
    assert_eq!(view(&w.vault, &w.network, "is_in_flight", json!({})).await, json!(false));
    let again = send(&w, "apply_pending_withdrawal", json!({}), 280, 0)
        .await
        .expect_err("a delivered request must not be sent twice");
    assert!(again.contains("no withdrawal is queued"), "{again}");
    assert_eq!(ledger_balance(&w).await, ledger_before - 1_000_000);
    assert_eq!(receiver_balance(&w).await, receiver_before + 1_000_000);
    println!(
        "GAS | full vault withdrawal lifecycle, 280 TGas attached, {maturity_mode} | {:.2} TGas",
        gas_burnt as f64 / 1e12
    );
    Ok(())
}

#[tokio::test]
async fn tightened_ceiling_refuses_a_matured_request_before_voucher_issuance()
-> testresult::TestResult<()> {
    let w = world().await?;
    fund_vault(&w, 5_000_000).await?;
    set_withdrawal_ceiling(&w, 1_000_000).await?;
    send(
        &w,
        "request_withdrawal",
        json!({ "asset_id": USDC, "receiver": w.stranger, "amount": "1000000" }),
        30,
        1,
    )
    .await?;
    let pending: PendingWithdrawal = near_sdk::serde_json::from_value(
        view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
    )?;
    set_withdrawal_ceiling(&w, 999_999).await?;
    let maturity_mode = reach_maturity(&w, &pending).await?;
    let queued_before = view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await;
    let ledger_before = ledger_balance(&w).await;
    let receiver_before = receiver_balance(&w).await;
    let voucher = format!("nep245:{}:withdrawal-1", w.vault.account_id());
    let supply_before =
        view(&w.intents, &w.network, "mt_supply", json!({ "token_id": voucher })).await;
    let refused = send(&w, "apply_pending_withdrawal", json!({}), 280, 0)
        .await
        .expect_err("a mature request must still obey the current tightened ceiling");
    assert!(refused.contains("the withdrawal exceeds the per-request ceiling"), "{refused}");
    assert_eq!(
        view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
        queued_before
    );
    assert_eq!(
        view(&w.vault, &w.network, "is_withdrawal_retry_armed", json!({})).await,
        json!(true)
    );
    assert_eq!(view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await, json!(null));
    assert_eq!(view(&w.vault, &w.network, "get_withdrawal_stage", json!({})).await, json!(null));
    assert_eq!(
        view(&w.vault, &w.network, "get_last_withdrawal_outcome", json!({})).await,
        json!(null)
    );
    assert_eq!(ledger_balance(&w).await, ledger_before);
    assert_eq!(receiver_balance(&w).await, receiver_before);
    assert_eq!(
        view(&w.intents, &w.network, "mt_supply", json!({ "token_id": voucher })).await,
        supply_before
    );
    println!(
        "CEILING | matured queued amount1000000 refused by current999999 ceiling | {maturity_mode}"
    );
    Ok(())
}

/// Controls only the explicitly sandbox-only MockFt, before its transfer mutation.
async fn block_receiver(w: &World, blocked: bool) -> testresult::TestResult<()> {
    w.usdc
        .call_function(
            "set_blocked_receiver",
            json!({ "receiver_id": if blocked { Some(w.stranger.clone()) } else { None } }),
        )
        .transaction()
        .gas(NearGas::from_tgas(10))
        .with_signer(w.owner.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    Ok(())
}

#[tokio::test]
async fn a_refunded_vault_withdrawal_requeues_and_retries_the_same_authorized_request()
-> testresult::TestResult<()> {
    let w = world().await?;
    fund_vault(&w, 5_000_000).await?;
    let ledger_before = ledger_balance(&w).await;
    let receiver_before = receiver_balance(&w).await;
    send(
        &w,
        "request_withdrawal",
        json!({ "asset_id": USDC, "receiver": w.stranger, "amount": "1000000" }),
        300,
        1,
    )
    .await?;
    let pending: PendingWithdrawal = near_sdk::serde_json::from_value(
        view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
    )?;
    let maturity_mode = reach_maturity(&w, &pending).await?;
    let authorized_request = view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await;
    block_receiver(&w, true).await?;

    // The actual Intents resolver refunds its debit and returns zero. The vault's
    // final parsed callback must release the guard and retain only this request.
    let (answer, refunded_gas) = send(&w, "apply_pending_withdrawal", json!({}), 280, 0).await?;
    assert_eq!(answer, json!(false), "a refunded transfer must not report delivery");
    assert_eq!(ledger_balance(&w).await, ledger_before, "the ledger debit must be refunded");
    assert_eq!(receiver_balance(&w).await, receiver_before, "the blocked receiver gets nothing");
    assert_eq!(view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await, json!(null));
    assert_eq!(view(&w.vault, &w.network, "is_in_flight", json!({})).await, json!(false));
    assert_eq!(
        view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
        authorized_request,
        "refund must preserve the authorized amount, asset, receiver, and existing deadline"
    );
    assert_eq!(
        view(&w.vault, &w.network, "get_last_withdrawal_outcome", json!({})).await,
        json!({ "Failed": { "withdrawal_id": "1", "reason": "withdrawal refunded" } })
    );

    block_receiver(&w, false).await?;
    assert_eq!(
        view(&w.vault, &w.network, "is_withdrawal_retry_armed", json!({})).await,
        json!(false)
    );
    let unarmed = send(&w, "apply_pending_withdrawal", json!({}), 280, 0)
        .await
        .expect_err("a permissionless caller cannot repeatedly spend voucher storage after refund");
    assert!(unarmed.contains("owner must re-arm"), "{unarmed}");
    assert_eq!(
        view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
        authorized_request
    );
    send(&w, "rearm_pending_withdrawal", json!({}), 30, 1).await?;
    assert_eq!(
        view(&w.vault, &w.network, "is_withdrawal_retry_armed", json!({})).await,
        json!(true)
    );
    // Explicit owner re-arm preserves the existing matured request and allocates a new unit.
    let (answer, retry_gas) = send(&w, "apply_pending_withdrawal", json!({}), 280, 0).await?;
    assert_eq!(answer, json!(true));
    assert_eq!(ledger_balance(&w).await, ledger_before - 1_000_000);
    assert_eq!(receiver_balance(&w).await, receiver_before + 1_000_000);
    assert_eq!(view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await, json!(null));
    assert_eq!(view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await, json!(null));
    assert_eq!(
        view(&w.vault, &w.network, "get_last_withdrawal_outcome", json!({})).await,
        json!({ "Delivered": { "withdrawal_id": "2", "amount": "1000000" } }),
        "the retry must own a new withdrawal id"
    );
    let duplicate = send(&w, "apply_pending_withdrawal", json!({}), 280, 0)
        .await
        .expect_err("the successful retry must not send again");
    assert!(duplicate.contains("no withdrawal is queued"), "{duplicate}");
    assert_eq!(ledger_balance(&w).await, ledger_before - 1_000_000);
    assert_eq!(receiver_balance(&w).await, receiver_before + 1_000_000);
    println!(
        "GAS | full vault withdrawal refunded-zero path, 280 TGas attached, {maturity_mode} | {:.2} TGas",
        refunded_gas as f64 / 1e12
    );
    println!(
        "GAS | full vault withdrawal safe retry after confirmed refund, 280 TGas attached | {:.2} TGas",
        retry_gas as f64 / 1e12
    );
    Ok(())
}

/// A sandbox-only unknown-outcome prestate, produced by the actual Vault public
/// methods with mocked preparation results. No production recovery bypass is
/// exposed, and this fixture is not evidence of a physically lost chain receipt.
async fn install_unknown_dispatched_guard(
    w: &World,
) -> testresult::TestResult<near_sdk::json_types::U64> {
    let fetched = w
        ._sandbox
        .patch_state(w.vault.account_id().as_str().parse()?)
        .fetch_from(&w._sandbox.rpc_addr, FetchData::new().storage())
        .await?;
    let root_name = BASE64.encode(b"STATE");
    let roots: Vec<_> = fetched
        .state
        .into_iter()
        .filter_map(|record| match record {
            StateRecord::Data { data_key_base64, value_base64, .. }
                if data_key_base64 == root_name =>
            {
                Some(value_base64)
            }
            _ => None,
        })
        .collect();
    assert_eq!(roots.len(), 1);
    let bytes = BASE64.decode(&roots[0])?;
    let mut v: Vault = borsh::from_slice(&bytes)?;
    assert_eq!(borsh::to_vec(&v)?, bytes);
    let now = chain_now(w).await;
    let started =
        now.checked_sub(WITHDRAW_RECOVERY_SEC + 1).expect("sandbox timestamp supports fixture");
    let vault_id: near_sdk::AccountId = w.vault.account_id().as_str().parse()?;
    let owner: near_sdk::AccountId = w.owner.as_str().parse()?;
    testing_env!(
        VMContextBuilder::new()
            .current_account_id(vault_id.clone())
            .predecessor_account_id(owner.clone())
            .attached_deposit(NearToken::from_yoctonear(1))
            .block_timestamp((started - WITHDRAW_DELAY_SEC) * 1_000_000_000)
            .build()
    );
    v.request_withdrawal(USDC.into(), w.stranger.as_str().parse()?, near_sdk::json_types::U128(20));
    testing_env!(
        VMContextBuilder::new()
            .current_account_id(vault_id.clone())
            .predecessor_account_id(owner)
            .block_timestamp(started * 1_000_000_000)
            .build()
    );
    drop(v.apply_pending_withdrawal());
    let id = v.get_withdrawal_status().unwrap().id;
    testing_env!(
        VMContextBuilder::new()
            .current_account_id(vault_id.clone())
            .predecessor_account_id(vault_id)
            .block_timestamp(started * 1_000_000_000)
            .build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        vec![PromiseResult::Successful(b"[\"1\",\"100\"]".to_vec())]
    );
    assert!(matches!(v.on_withdrawal_balance(id), near_sdk::PromiseOrValue::Promise(_)));
    testing_env!(
        VMContextBuilder::new()
            .current_account_id(w.vault.account_id().as_str().parse().unwrap())
            .predecessor_account_id(w.vault.account_id().as_str().parse().unwrap())
            .block_timestamp(started * 1_000_000_000)
            .build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        vec![
            PromiseResult::Successful(b"\"1\"".to_vec()),
            PromiseResult::Successful(b"[\"1\"]".to_vec()),
            PromiseResult::Successful(b"[\"0\"]".to_vec()),
            PromiseResult::Successful(b"[\"0\"]".to_vec())
        ]
    );
    assert!(matches!(
        v.on_withdrawal_prepared(id, near_sdk::json_types::U128(100)),
        near_sdk::PromiseOrValue::Promise(_)
    ));
    assert_eq!(v.get_withdrawal_stage(), Some(mandate_vault::WithdrawalStage::Dispatched));
    w._sandbox
        .patch_state(w.vault.account_id().as_str().parse()?)
        .storage(root_name, BASE64.encode(borsh::to_vec(&v)?))
        .send()
        .await?;
    assert_eq!(
        view(&w.vault, &w.network, "get_withdrawal_stage", json!({})).await,
        json!("Dispatched")
    );
    Ok(id)
}

fn voucher_asset(w: &World, id: near_sdk::json_types::U64) -> String {
    format!("nep245:{}:withdrawal-{}", w.vault.account_id(), id.0)
}

async fn issue_fixture_voucher(
    w: &World,
    id: near_sdk::json_types::U64,
) -> testresult::TestResult<()> {
    w.intents
        .call_function(
            "mt_on_transfer",
            json!({
                "sender_id": w.vault.account_id(), "previous_owner_ids": [w.vault.account_id()],
                "token_ids": [format!("withdrawal-{}", id.0)], "amounts": ["1"], "msg": "",
            }),
        )
        .transaction()
        .gas(NearGas::from_tgas(30))
        .with_signer(w.vault.account_id().clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    Ok(())
}

#[tokio::test]
async fn id_bound_wasm_recovery_ignores_nonmatching_calls_and_reconciles_only_the_observed_attempt()
-> testresult::TestResult<()> {
    let w = world().await?;
    fund_vault(&w, 100).await?;
    let id = install_unknown_dispatched_guard(&w).await?;
    issue_fixture_voucher(&w, id).await?;
    let active = view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await;
    let snapshot = view(&w.vault, &w.network, "get_withdrawal_recovery_state", json!({})).await;
    assert_eq!(snapshot["protocol"], "unique_voucher_v1");
    assert_eq!(snapshot["withdrawal_id"], json!(id));
    assert_eq!(snapshot["stage"], "Dispatched");
    assert_eq!(snapshot["started_sec"], active["started_sec"]);
    assert_eq!(
        snapshot["recover_after_sec"],
        active["started_sec"].as_u64().unwrap() + WITHDRAW_RECOVERY_SEC
    );
    assert_eq!(snapshot["required_gas_tgas"], 140);

    let stale = w
        .vault
        .call_function(
            "recover_withdrawal_attempt",
            json!({ "withdrawal_id": (id.0 + 1).to_string() }),
        )
        .transaction()
        .gas(NearGas::from_tgas(30))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .into_result()?;
    assert_eq!(stale.json::<Value>()?, json!(false));
    assert!(stale.receipt_outcomes().iter().all(|o| o.executor_id != *w.intents.account_id()));
    assert_eq!(view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await, active);
    assert_eq!(
        view(&w.vault, &w.network, "get_withdrawal_recovery_state", json!({})).await,
        snapshot
    );
    assert_eq!(ledger_balance(&w).await, 100);
    assert_eq!(receiver_balance(&w).await, 0);

    let current = w
        .vault
        .call_function("recover_withdrawal_attempt", json!({ "withdrawal_id": id }))
        .transaction()
        .gas(NearGas::from_tgas(140))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .into_result()?;
    assert_eq!(current.json::<Value>()?, json!(true));
    assert!(current.receipt_outcomes().iter().all(|o| o.executor_id != *w.usdc.account_id()));
    assert_eq!(
        view(&w.vault, &w.network, "get_withdrawal_recovery_state", json!({})).await,
        json!(null)
    );
    assert_eq!(
        view(&w.vault, &w.network, "is_withdrawal_retry_armed", json!({})).await,
        json!(false)
    );
    assert_eq!(ledger_balance(&w).await, 100);
    assert_eq!(receiver_balance(&w).await, 0);
    println!(
        "RECOVERY | sandbox-only unresolved prestate | nonmatching ID no external call; current ID authenticated cancellation | {:.2} TGas",
        current.total_gas_burnt.as_gas() as f64 / 1e12
    );
    Ok(())
}

#[tokio::test]
async fn id_bound_wasm_recovery_retries_missing_proof_without_releasing_the_guard_or_sending()
-> testresult::TestResult<()> {
    let w = world().await?;
    fund_vault(&w, 100).await?;
    let id = install_unknown_dispatched_guard(&w).await?;
    // Deliberately supply no external voucher. This injected unknown prestate
    // must retain its guard; it is not evidence of a physically lost live receipt.
    let snapshot = view(&w.vault, &w.network, "get_withdrawal_recovery_state", json!({})).await;
    let active = view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await;
    for _ in 0..2 {
        let outcome = w
            .vault
            .call_function("recover_withdrawal_attempt", json!({ "withdrawal_id": id }))
            .transaction()
            .gas(NearGas::from_tgas(140))
            .with_signer(w.stranger.clone(), w.signer.clone())
            .send_to(&w.network)
            .await?
            .into_result()?;
        assert_eq!(outcome.json::<Value>()?, json!(false));
        assert!(outcome.receipt_outcomes().iter().all(|o| o.executor_id != *w.usdc.account_id()));
        assert_eq!(view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await, active);
        assert_eq!(
            view(&w.vault, &w.network, "get_withdrawal_recovery_state", json!({})).await,
            snapshot
        );
        assert_eq!(
            view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
            json!(null)
        );
        assert_eq!(view(&w.vault, &w.network, "is_in_flight", json!({})).await, json!(true));
        assert_eq!(ledger_balance(&w).await, 100);
        assert_eq!(receiver_balance(&w).await, 0);
    }
    Ok(())
}

fn original_fixture_batch(
    w: &World,
    id: near_sdk::json_types::U64,
) -> near_api::advanced::ExecuteSignedTransaction {
    let action = |method: &str, args: Value, gas| {
        Action::FunctionCall(Box::new(FunctionCallAction {
            method_name: method.into(),
            args: near_sdk::serde_json::to_vec(&args).unwrap(),
            gas: NearGas::from_tgas(gas),
            deposit: NearToken::from_yoctonear(1),
        }))
    };
    Transaction::construct(w.vault.account_id().clone(), w.intents.account_id().clone())
        .add_action(action(
            "mt_transfer",
            json!({ "receiver_id": format!("s.{}", w.vault.account_id()),
            "token_id": voucher_asset(w, id), "amount": "1" }),
            30,
        ))
        .add_action(action(
            "ft_withdraw",
            json!({ "token": "usdc.sandbox", "receiver_id": w.stranger, "amount": "20" }),
            100,
        ))
        .with_signer(w.signer.clone())
}

#[tokio::test]
async fn production_recovery_reads_sent_proof_despite_masking_credit_and_consumes_only_the_old_authorization()
-> testresult::TestResult<()> {
    let w = world().await?;
    fund_vault(&w, 100).await?;
    let id = install_unknown_dispatched_guard(&w).await?;
    issue_fixture_voucher(&w, id).await?;
    original_fixture_batch(&w, id).send_to(&w.network).await?.assert_success();
    fund_vault(&w, 25).await?;
    assert_eq!(ledger_balance(&w).await, 105);
    let (answer, gas) = send(&w, "recover_withdrawal", json!({}), 140, 0).await?;
    assert_eq!(answer, json!(true));
    assert_eq!(ledger_balance(&w).await, 105);
    assert_eq!(receiver_balance(&w).await, 20);
    assert_eq!(view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await, json!(null));
    assert_eq!(view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await, json!(null));
    assert_eq!(
        view(&w.vault, &w.network, "get_last_withdrawal_outcome", json!({})).await,
        json!({ "DebitObserved": { "withdrawal_id": id } })
    );
    println!(
        "GAS | production recovery, cancellation fails because original won, masked105, 140 TGas attached | {:.2} TGas",
        gas as f64 / 1e12
    );
    Ok(())
}

#[tokio::test]
async fn production_cancellation_proof_blocks_two_late_originals_and_requires_owner_rearm_before_new_id()
-> testresult::TestResult<()> {
    let w = world().await?;
    fund_vault(&w, 100).await?;
    let id = install_unknown_dispatched_guard(&w).await?;
    issue_fixture_voucher(&w, id).await?;
    let (answer, gas) = send(&w, "recover_withdrawal", json!({}), 140, 0).await?;
    assert_eq!(answer, json!(true));
    assert_eq!(ledger_balance(&w).await, 100);
    assert_eq!(receiver_balance(&w).await, 0);
    let pending = view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await;
    assert_eq!(pending["amount"], "20");
    assert_eq!(
        view(&w.vault, &w.network, "is_withdrawal_retry_armed", json!({})).await,
        json!(false)
    );
    for _ in 0..2 {
        let late = original_fixture_batch(&w, id).send_to(&w.network).await?.assert_failure();
        assert!(late.receipt_outcomes().iter().all(|o| o.executor_id != *w.usdc.account_id()));
        assert_eq!(ledger_balance(&w).await, 100);
        assert_eq!(receiver_balance(&w).await, 0);
    }
    let unarmed = send(&w, "apply_pending_withdrawal", json!({}), 280, 0)
        .await
        .expect_err("owner re-arm is mandatory");
    assert!(unarmed.contains("owner must re-arm"), "{unarmed}");
    assert_eq!(view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await, pending);
    send(&w, "rearm_pending_withdrawal", json!({}), 30, 1).await?;
    assert_eq!(view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await, pending);
    let (answer, _) = send(&w, "apply_pending_withdrawal", json!({}), 280, 0).await?;
    assert_eq!(answer, json!(true));
    assert_eq!(ledger_balance(&w).await, 80);
    assert_eq!(receiver_balance(&w).await, 20);
    assert_eq!(
        view(&w.vault, &w.network, "get_last_withdrawal_outcome", json!({})).await,
        json!({ "Delivered": { "withdrawal_id": "2", "amount": "20" } })
    );
    println!(
        "GAS | production cancellation/proof recovery, two late originals refused, 140 TGas attached | {:.2} TGas",
        gas as f64 / 1e12
    );
    Ok(())
}

#[tokio::test]
async fn historical_voucher_in_an_uncreated_cancelled_sink_is_refused_before_any_withdrawal()
-> testresult::TestResult<()> {
    let w = world().await?;
    fund_vault(&w, 100).await?;
    let cancelled = format!("c.{}", w.vault.account_id());
    // Model a former issuer permission depositing a future canonical unit.
    // No NEAR sink account or registered Intents key is needed for this ledger.
    w.intents
        .call_function(
            "mt_on_transfer",
            json!({
                "sender_id": cancelled, "previous_owner_ids": [w.vault.account_id()],
                "token_ids": ["withdrawal-1"], "amounts": ["1"], "msg": "",
            }),
        )
        .transaction()
        .gas(NearGas::from_tgas(30))
        .with_signer(w.vault.account_id().clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    let voucher = format!("nep245:{}:withdrawal-1", w.vault.account_id());
    assert_eq!(
        view(&w.intents, &w.network, "mt_supply", json!({ "token_id": voucher })).await,
        json!("1")
    );
    assert_eq!(
        view(&w.intents, &w.network, "public_keys_of", json!({ "account_id": cancelled })).await,
        json!([])
    );
    send(
        &w,
        "request_withdrawal",
        json!({ "asset_id": USDC, "receiver": w.stranger, "amount": "20" }),
        30,
        1,
    )
    .await?;
    let pending: PendingWithdrawal = near_sdk::serde_json::from_value(
        view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
    )?;
    reach_maturity(&w, &pending).await?;
    let authorized = view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await;
    let (answer, gas) = send(&w, "apply_pending_withdrawal", json!({}), 280, 0).await?;
    assert_eq!(answer, json!(false));
    assert_eq!(ledger_balance(&w).await, 100);
    assert_eq!(receiver_balance(&w).await, 0);
    assert_eq!(
        view(&w.intents, &w.network, "mt_supply", json!({ "token_id": voucher })).await,
        json!("2")
    );
    assert_eq!(
        view(
            &w.intents,
            &w.network,
            "mt_balance_of",
            json!({ "account_id": w.vault.account_id(), "token_id": voucher })
        )
        .await,
        json!("1")
    );
    assert_eq!(
        view(
            &w.intents,
            &w.network,
            "mt_balance_of",
            json!({ "account_id": cancelled, "token_id": voucher })
        )
        .await,
        json!("1")
    );
    assert_eq!(view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await, json!(null));
    assert_eq!(view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await, authorized);
    assert_eq!(
        view(&w.vault, &w.network, "is_withdrawal_retry_armed", json!({})).await,
        json!(false)
    );
    println!(
        "GAS | guarded preparation refuses static historical cancelled unit, 280 TGas attached | {:.2} TGas",
        gas as f64 / 1e12
    );
    Ok(())
}
