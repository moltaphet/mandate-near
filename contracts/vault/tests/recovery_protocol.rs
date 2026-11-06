//! Research only: actual Intents same-receipt debit/marker batch mechanics.
//!
//! Initial cases use an ordinary open-mint MockFt marker. Later cases model a
//! proposed unique native NEP245 voucher through the actual Intents receiver API.
//! Exactly-once production issuance, immutable permissions and the vault recovery
//! state machine are not implemented here. No production guard is bypassed.

mod common;

use common::{USDC, World, extra_asset, fund_extra_assets, fund_vault, view, world_with_extra};
use near_api::types::transaction::actions::{
    Action, CreateAccountAction, FunctionCallAction, TransferAction,
};
use near_api::{Account, AccountId, NearGas, NearToken, Transaction};
use near_sdk::serde_json::{Value, json};

fn action(method: &str, args: Value, gas: u64) -> Action {
    Action::FunctionCall(Box::new(FunctionCallAction {
        method_name: method.into(),
        args: near_sdk::serde_json::to_vec(&args).expect("research arguments serialize"),
        gas: NearGas::from_tgas(gas),
        deposit: NearToken::from_yoctonear(1),
    }))
}

fn batch(w: &World, marker_amount: &str) -> near_api::advanced::ExecuteSignedTransaction {
    Transaction::construct(w.vault.account_id().clone(), w.intents.account_id().clone())
        .add_action(action(
            "ft_withdraw",
            json!({ "token": "usdc.sandbox", "receiver_id": w.stranger, "amount": "20" }),
            100,
        ))
        .add_action(action(
            "mt_transfer",
            json!({ "receiver_id": w.owner, "token_id": extra_asset(0), "amount": marker_amount }),
            30,
        ))
        .with_signer(w.signer.clone())
}

async fn balance(w: &World, account: &AccountId, asset: &str) -> Value {
    view(
        &w.intents,
        &w.network,
        "mt_balance_of",
        json!({ "account_id": account, "token_id": asset }),
    )
    .await
}

async fn supply(w: &World, asset: &str) -> Value {
    view(&w.intents, &w.network, "mt_supply", json!({ "token_id": asset })).await
}

async fn receiver_balance(w: &World) -> Value {
    view(&w.usdc, &w.network, "ft_balance_of", json!({ "account_id": w.stranger })).await
}

#[tokio::test]
async fn same_receipt_marker_commits_with_the_debit_and_survives_a_masking_credit()
-> testresult::TestResult<()> {
    let w = world_with_extra(1).await?;
    fund_vault(&w, 100).await?;
    fund_extra_assets(&w, 1).await?;
    let sent =
        batch(&w, "1").send_to(&w.network).await?.into_result().map_err(|e| format!("{e:?}"))?;
    let returned = sent.raw_bytes()?;
    println!(
        "RECOVERY RESEARCH | ft_withdraw Promise followed by unit mt_transfer | returned {:?}",
        String::from_utf8_lossy(&returned)
    );
    assert!(returned.is_empty(), "the last unit action returns empty bytes, not withdrawal U128");
    assert_eq!(balance(&w, w.vault.account_id(), USDC).await, json!("80"));
    assert_eq!(receiver_balance(&w).await, json!("20"));
    assert_eq!(balance(&w, w.vault.account_id(), &extra_asset(0)).await, json!("0"));
    assert_eq!(balance(&w, &w.owner, &extra_asset(0)).await, json!("1"));
    fund_vault(&w, 25).await?;
    assert_eq!(balance(&w, w.vault.account_id(), USDC).await, json!("105"));
    assert_eq!(balance(&w, &w.owner, &extra_asset(0)).await, json!("1"));
    println!(
        "RECOVERY RESEARCH | baseline100, withdraw20, credit25 ->105; separate marker1 | {:.2} TGas",
        sent.total_gas_burnt.as_gas() as f64 / 1e12
    );
    Ok(())
}

#[tokio::test]
async fn failed_second_action_rolls_back_the_first_debit_and_scheduled_transfer()
-> testresult::TestResult<()> {
    let w = world_with_extra(1).await?;
    fund_vault(&w, 100).await?;
    fund_extra_assets(&w, 1).await?;
    let failed = batch(&w, "2").send_to(&w.network).await?.assert_failure();
    assert!(
        failed.receipt_outcomes().iter().all(|outcome| outcome.executor_id != *w.usdc.account_id()),
        "a rolled-back first action must not execute its scheduled underlying FT call"
    );
    assert_eq!(balance(&w, w.vault.account_id(), USDC).await, json!("100"));
    assert_eq!(receiver_balance(&w).await, json!("0"));
    assert_eq!(balance(&w, w.vault.account_id(), &extra_asset(0)).await, json!("1"));
    assert_eq!(balance(&w, &w.owner, &extra_asset(0)).await, json!("0"));
    println!(
        "RECOVERY RESEARCH | second-action failure: debit rolled back, no underlying FT receipt | {:.2} TGas",
        failed.total_gas_burnt.as_gas() as f64 / 1e12
    );
    Ok(())
}

#[tokio::test]
async fn committed_marker_is_not_proof_of_terminal_delivery_or_absence_of_refund()
-> testresult::TestResult<()> {
    let w = world_with_extra(1).await?;
    fund_vault(&w, 100).await?;
    fund_extra_assets(&w, 1).await?;
    w.usdc
        .call_function("set_blocked_receiver", json!({ "receiver_id": w.stranger }))
        .transaction()
        .gas(NearGas::from_tgas(10))
        .with_signer(w.owner.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    let sent =
        batch(&w, "1").send_to(&w.network).await?.into_result().map_err(|e| format!("{e:?}"))?;
    assert!(sent.raw_bytes()?.is_empty(), "the batch return omits the later withdrawal result");
    assert_eq!(balance(&w, w.vault.account_id(), USDC).await, json!("100"));
    assert_eq!(receiver_balance(&w).await, json!("0"));
    assert_eq!(balance(&w, &w.owner, &extra_asset(0)).await, json!("1"));
    assert!(
        sent.receipt_failures().iter().any(|outcome| outcome.executor_id == *w.usdc.account_id()),
        "the underlying FT failure must be visible separately from the successful batch"
    );
    println!(
        "RECOVERY RESEARCH | marker committed but underlying FT failed and debit refunded | {:.2} TGas",
        sent.total_gas_burnt.as_gas() as f64 / 1e12
    );
    Ok(())
}

fn voucher(w: &World, id: &str) -> String {
    format!("nep245:{}:{id}", w.vault.account_id())
}

async fn issue_voucher(w: &World, as_account: &AccountId, id: &str) -> testresult::TestResult<u64> {
    issue_voucher_to(w, as_account, w.vault.account_id(), id).await
}

async fn issue_voucher_to(
    w: &World,
    as_account: &AccountId,
    receiver: &AccountId,
    id: &str,
) -> testresult::TestResult<u64> {
    // Research calls model the proposed private issuer namespace. Actual one-time
    // issuance, immutable authority and production API restrictions are not added.
    let issued = w
        .intents
        .call_function(
            "mt_on_transfer",
            json!({
                "sender_id": receiver,
                "previous_owner_ids": [receiver],
                "token_ids": [id],
                "amounts": ["1"],
                "msg": "",
            }),
        )
        .transaction()
        .gas(NearGas::from_tgas(30))
        .with_signer(as_account.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .into_result()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(issued.json::<Value>()?, json!(["0"]));
    Ok(issued.total_gas_burnt.as_gas())
}

#[tokio::test]
async fn global_supply_exposes_future_voucher_preseeded_outside_both_sinks()
-> testresult::TestResult<()> {
    let w = common::world().await?;
    fund_vault(&w, 100).await?;
    let id = "withdrawal-1";
    let receipt = voucher(&w, id);
    let sent: AccountId = format!("s.{}", w.vault.account_id()).parse()?;
    let cancelled: AccountId = format!("c.{}", w.vault.account_id()).parse()?;
    assert_eq!(supply(&w, &receipt).await, json!("0"));
    assert_eq!(supply(&w, "unparseable").await, Value::Null);

    // A former vault access key can mint the canonical future ID to an outside
    // Intents account without creating either named sink or registering a key.
    issue_voucher_to(&w, w.vault.account_id(), &w.stranger, id).await?;
    assert_eq!(supply(&w, &receipt).await, json!("1"));
    assert_eq!(balance(&w, &w.stranger, &receipt).await, json!("1"));
    assert_eq!(balance(&w, w.vault.account_id(), &receipt).await, json!("0"));
    for sink in [&sent, &cancelled] {
        assert_eq!(balance(&w, sink, &receipt).await, json!("0"));
        assert_eq!(
            view(&w.intents, &w.network, "public_keys_of", json!({ "account_id": sink })).await,
            json!([])
        );
    }

    // Production-shaped issuance creates its source unit while the duplicate
    // is still invisible to source and sink balance checks. The global guard
    // must require exactly one, so this historical issuance is refused.
    issue_voucher(&w, w.vault.account_id(), id).await?;
    assert_eq!(supply(&w, &receipt).await, json!("2"));
    assert_eq!(balance(&w, w.vault.account_id(), &receipt).await, json!("1"));
    assert_eq!(balance(&w, &sent, &receipt).await, json!("0"));
    assert_eq!(balance(&w, &cancelled, &receipt).await, json!("0"));

    // Moving the outside duplicate after the preparation snapshot can forge
    // sent=0/cancelled=1 while the original remains spendable at the source.
    w.intents
        .call_function(
            "mt_transfer",
            json!({ "receiver_id": cancelled, "token_id": receipt, "amount": "1" }),
        )
        .transaction()
        .gas(NearGas::from_tgas(30))
        .deposit(NearToken::from_yoctonear(1))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    assert_eq!(supply(&w, &receipt).await, json!("2"));
    assert_eq!(balance(&w, &w.stranger, &receipt).await, json!("0"));
    assert_eq!(balance(&w, &cancelled, &receipt).await, json!("1"));
    assert_eq!(balance(&w, &sent, &receipt).await, json!("0"));
    assert_eq!(balance(&w, w.vault.account_id(), &receipt).await, json!("1"));

    let original =
        voucher_batch(&w, &receipt, &sent, "20").send_to(&w.network).await?.assert_success();
    assert_eq!(original.json::<Value>()?, json!("20"));
    assert_eq!(balance(&w, w.vault.account_id(), USDC).await, json!("80"));
    assert_eq!(receiver_balance(&w).await, json!("20"));
    assert_eq!(balance(&w, &sent, &receipt).await, json!("1"));
    assert_eq!(balance(&w, &cancelled, &receipt).await, json!("1"));
    assert_eq!(supply(&w, &receipt).await, json!("2"));
    assert_eq!(supply(&w, USDC).await, json!("80"));
    println!(
        "RECOVERY RESEARCH | canonical duplicate outside empty sinks: global supply2; forged cancellation pair retains source1; original debits20; transfers preserve supply"
    );
    Ok(())
}

fn voucher_batch(
    w: &World,
    receipt: &str,
    sent: &AccountId,
    amount: &str,
) -> near_api::advanced::ExecuteSignedTransaction {
    Transaction::construct(w.vault.account_id().clone(), w.intents.account_id().clone())
        .add_action(action(
            "mt_transfer",
            json!({ "receiver_id": sent, "token_id": receipt, "amount": "1" }),
            30,
        ))
        .add_action(action(
            "ft_withdraw",
            json!({ "token": "usdc.sandbox", "receiver_id": w.stranger, "amount": amount }),
            100,
        ))
        .with_signer(w.signer.clone())
}

async fn cancel_voucher(w: &World, receipt: &str, cancelled: &AccountId) -> Result<u64, String> {
    let result = w
        .intents
        .call_function(
            "mt_transfer",
            json!({ "receiver_id": cancelled, "token_id": receipt, "amount": "1" }),
        )
        .transaction()
        .gas(NearGas::from_tgas(30))
        .deposit(NearToken::from_yoctonear(1))
        .with_signer(w.vault.account_id().clone(), w.signer.clone())
        .send_to(&w.network)
        .await
        .map_err(|e| e.to_string())?
        .into_result()
        .map_err(|e| format!("{e:?}"))?;
    Ok(result.total_gas_burnt.as_gas())
}

async fn storage_usage(w: &World) -> testresult::TestResult<u64> {
    Ok(Account(w.intents.account_id().clone())
        .view()
        .fetch_from(&w.network)
        .await?
        .data
        .storage_usage)
}

#[tokio::test]
async fn unique_voucher_original_wins_preserves_the_withdrawal_return_and_cannot_be_forged()
-> testresult::TestResult<()> {
    let w = common::world().await?;
    fund_vault(&w, 100).await?;
    let before_storage = storage_usage(&w).await?;
    let issue_gas = issue_voucher(&w, w.vault.account_id(), "withdrawal-1").await?;
    let issued_storage = storage_usage(&w).await?;
    let receipt = voucher(&w, "withdrawal-1");
    let sent: AccountId = format!("s.{}", w.vault.account_id()).parse()?;
    let cancelled: AccountId = format!("c.{}", w.vault.account_id()).parse()?;
    assert_eq!(balance(&w, w.vault.account_id(), &receipt).await, json!("1"));

    // Supplying the vault as sender_id cannot spoof env::predecessor_account_id().
    issue_voucher(&w, &w.stranger, "withdrawal-1").await?;
    assert_eq!(balance(&w, w.vault.account_id(), &receipt).await, json!("1"));
    assert_eq!(
        balance(&w, w.vault.account_id(), &format!("nep245:{}:withdrawal-1", w.stranger)).await,
        json!("1")
    );

    Transaction::construct(w.stranger.clone(), sent.clone())
        .add_action(Action::CreateAccount(CreateAccountAction {}))
        .add_action(Action::Transfer(TransferAction { deposit: NearToken::from_near(1) }))
        .with_signer(w.signer.clone())
        .send_to(&w.network)
        .await?
        .into_result()
        .expect_err("an outside account cannot create a child under the vault");
    println!("RECOVERY RESEARCH | named sentinel takeover refused");

    let prepared_storage = storage_usage(&w).await?;
    let result = voucher_batch(&w, &receipt, &sent, "20")
        .send_to(&w.network)
        .await?
        .into_result()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.json::<Value>()?, json!("20"), "ft_withdraw LAST must retain U128 return");
    assert_eq!(balance(&w, w.vault.account_id(), USDC).await, json!("80"));
    assert_eq!(receiver_balance(&w).await, json!("20"));
    assert_eq!(balance(&w, &sent, &receipt).await, json!("1"));
    assert_eq!(balance(&w, &cancelled, &receipt).await, json!("0"));
    for sentinel in [&sent, &cancelled] {
        assert_eq!(
            view(&w.intents, &w.network, "public_keys_of", json!({ "account_id": sentinel })).await,
            json!([]),
            "the named sentinel must have no implicit or registered Intents signing authority"
        );
    }
    cancel_voucher(&w, &receipt, &cancelled)
        .await
        .expect_err("cancellation cannot spend the unique unit after the original wins");
    fund_vault(&w, 25).await?;
    assert_eq!(balance(&w, w.vault.account_id(), USDC).await, json!("105"));
    assert_eq!(balance(&w, &sent, &receipt).await, json!("1"));
    let after_storage = storage_usage(&w).await?;
    println!(
        "RECOVERY RESEARCH | native NEP245 voucher issuance | {:.2} TGas; issuer storage growth {} bytes; adversary namespace growth {} bytes",
        issue_gas as f64 / 1e12,
        issued_storage - before_storage,
        prepared_storage - issued_storage
    );
    println!(
        "RECOVERY RESEARCH | unique original wins, marker FIRST + ft_withdraw LAST, 105 masking credit | {:.2} TGas; sent-record growth {} bytes",
        result.total_gas_burnt.as_gas() as f64 / 1e12,
        after_storage - prepared_storage
    );
    Ok(())
}

#[tokio::test]
async fn preexisting_named_sentinel_can_erase_the_proof_and_enable_duplicate_debit()
-> testresult::TestResult<()> {
    let w = common::world().await?;
    fund_vault(&w, 100).await?;
    let sent: AccountId = format!("s.{}", w.vault.account_id()).parse()?;
    // A deployment can inherit an existing subaccount. Empty Intents key views
    // do not remove its NEAR account's ability to call authenticated mt_transfer.
    common::create_account(&w._sandbox, sent.as_str(), 10).await?;
    assert_eq!(
        view(&w.intents, &w.network, "public_keys_of", json!({ "account_id": sent })).await,
        json!([])
    );
    issue_voucher(&w, w.vault.account_id(), "withdrawal-1").await?;
    let receipt = voucher(&w, "withdrawal-1");
    voucher_batch(&w, &receipt, &sent, "20").send_to(&w.network).await?.assert_success();
    assert_eq!(balance(&w, w.vault.account_id(), USDC).await, json!("80"));
    assert_eq!(balance(&w, &sent, &receipt).await, json!("1"));
    w.intents
        .call_function(
            "mt_transfer",
            json!({ "receiver_id": w.vault.account_id(), "token_id": receipt, "amount": "1" }),
        )
        .transaction()
        .gas(NearGas::from_tgas(30))
        .deposit(NearToken::from_yoctonear(1))
        .with_signer(sent.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    assert_eq!(balance(&w, &sent, &receipt).await, json!("0"));
    assert_eq!(balance(&w, w.vault.account_id(), &receipt).await, json!("1"));
    voucher_batch(&w, &receipt, &sent, "20").send_to(&w.network).await?.assert_success();
    assert_eq!(balance(&w, w.vault.account_id(), USDC).await, json!("60"));
    assert_eq!(receiver_balance(&w).await, json!("40"));
    println!(
        "RECOVERY RESEARCH | preexisting NEAR sentinel with zero registered Intents keys moves voucher back; same unit permits two debits"
    );
    Ok(())
}

#[tokio::test]
async fn native_voucher_retains_confirmed_refund_and_each_retry_requires_a_new_unit()
-> testresult::TestResult<()> {
    let w = common::world().await?;
    fund_vault(&w, 100).await?;
    w.usdc
        .call_function("set_blocked_receiver", json!({ "receiver_id": w.stranger }))
        .transaction()
        .gas(NearGas::from_tgas(10))
        .with_signer(w.owner.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    let sent: AccountId = format!("s.{}", w.vault.account_id()).parse()?;
    let cancelled: AccountId = format!("c.{}", w.vault.account_id()).parse()?;
    for attempt in 1..=3 {
        let before = storage_usage(&w).await?;
        let id = format!("refunded-{attempt}");
        let issue_gas = issue_voucher(&w, w.vault.account_id(), &id).await?;
        let issued = storage_usage(&w).await?;
        let receipt = voucher(&w, &id);
        let result =
            voucher_batch(&w, &receipt, &sent, "20").send_to(&w.network).await?.assert_success();
        assert_eq!(result.json::<Value>()?, json!("0"));
        assert_eq!(balance(&w, w.vault.account_id(), USDC).await, json!("100"));
        assert_eq!(receiver_balance(&w).await, json!("0"));
        assert_eq!(balance(&w, w.vault.account_id(), &receipt).await, json!("0"));
        assert_eq!(balance(&w, &sent, &receipt).await, json!("1"));
        let completed = storage_usage(&w).await?;
        cancel_voucher(&w, &receipt, &cancelled)
            .await
            .expect_err("confirmed refund does not recreate the consumed voucher");
        let late =
            voucher_batch(&w, &receipt, &sent, "20").send_to(&w.network).await?.assert_failure();
        assert!(late.receipt_outcomes().iter().all(|o| o.executor_id != *w.usdc.account_id()));
        println!(
            "RECOVERY RESEARCH | confirmed refund attempt{attempt}: consumed unique unit, same-id retry refused; issue {:.2} TGas/{} bytes, batch {:.2} TGas/{} bytes",
            issue_gas as f64 / 1e12,
            issued - before,
            result.total_gas_burnt.as_gas() as f64 / 1e12,
            completed - issued
        );
    }
    Ok(())
}

#[tokio::test]
async fn unique_voucher_cancellation_wins_and_blocks_two_delayed_originals()
-> testresult::TestResult<()> {
    let w = common::world().await?;
    fund_vault(&w, 100).await?;
    issue_voucher(&w, w.vault.account_id(), "withdrawal-1").await?;
    let receipt = voucher(&w, "withdrawal-1");
    let sent: AccountId = format!("s.{}", w.vault.account_id()).parse()?;
    let cancelled: AccountId = format!("c.{}", w.vault.account_id()).parse()?;

    let failed =
        voucher_batch(&w, &receipt, &sent, "101").send_to(&w.network).await?.assert_failure();
    assert!(failed.receipt_outcomes().iter().all(|o| o.executor_id != *w.usdc.account_id()));
    assert_eq!(balance(&w, w.vault.account_id(), &receipt).await, json!("1"));
    assert_eq!(balance(&w, &sent, &receipt).await, json!("0"));
    assert_eq!(balance(&w, w.vault.account_id(), USDC).await, json!("100"));

    let cancel_gas = cancel_voucher(&w, &receipt, &cancelled).await?;
    assert_eq!(balance(&w, &cancelled, &receipt).await, json!("1"));
    assert_eq!(balance(&w, w.vault.account_id(), &receipt).await, json!("0"));
    assert_eq!(
        view(&w.intents, &w.network, "public_keys_of", json!({ "account_id": cancelled })).await,
        json!([])
    );
    for _ in 0..2 {
        let late =
            voucher_batch(&w, &receipt, &sent, "20").send_to(&w.network).await?.assert_failure();
        assert!(late.receipt_outcomes().iter().all(|o| o.executor_id != *w.usdc.account_id()));
        assert_eq!(balance(&w, w.vault.account_id(), USDC).await, json!("100"));
        assert_eq!(receiver_balance(&w).await, json!("0"));
        assert_eq!(balance(&w, &sent, &receipt).await, json!("0"));
        assert_eq!(balance(&w, &cancelled, &receipt).await, json!("1"));
    }
    println!(
        "RECOVERY RESEARCH | unique cancellation wins; two delayed originals cannot debit | {:.2} TGas",
        cancel_gas as f64 / 1e12
    );
    Ok(())
}
