//! Withdrawals on a local NEAR sandbox: the owner queues one, and the vault's own call to
//! `intents.near` moves the tokens out. The vault's call runs after 24 h, which the sandbox
//! clock cannot reach, so this file checks the queue on the chain and the `intents.near`
//! call the vault makes, sent from the account that holds the balance.

mod common;

use common::{USDC, World, mandate, view, world};
use near_api::{NearGas, NearToken};
use near_sdk::serde_json::{Value, json};

/// Sends one call and returns the answer, or the vault's panic message.
async fn send(
    w: &World,
    method: &str,
    args: Value,
    as_account: &near_api::AccountId,
    yocto: u128,
) -> Result<Value, String> {
    let result = w
        .vault
        .call_function(method, args)
        .transaction()
        .gas(NearGas::from_tgas(300))
        .deposit(NearToken::from_yoctonear(yocto))
        .with_signer(as_account.clone(), w.signer.clone())
        .send_to(&w.network)
        .await
        .map_err(|e| e.to_string())?;
    match result.into_result() {
        Ok(value) => Ok(value.json::<Value>().unwrap_or(Value::Null)),
        Err(e) => Err(format!("{e:?}")),
    }
}

#[tokio::test]
async fn owner_queues_and_cancels_a_withdrawal_on_a_real_chain() -> testresult::TestResult<()> {
    let w = world().await?;
    let (owner, stranger) = (w.owner.clone(), w.stranger.clone());

    assert_eq!(view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await, json!(null));
    send(
        &w,
        "request_withdrawal",
        json!({ "asset_id": USDC, "receiver": stranger, "amount": "1000000" }),
        &owner,
        0,
    )
    .await
    .expect_err("a withdrawal request needs 1 yoctoNEAR");
    send(
        &w,
        "request_withdrawal",
        json!({ "asset_id": USDC, "receiver": stranger, "amount": "1000000" }),
        &stranger,
        1,
    )
    .await
    .expect_err("only the owner can request a withdrawal");
    send(
        &w,
        "request_withdrawal",
        json!({ "asset_id": USDC, "receiver": stranger, "amount": "1000000" }),
        &owner,
        1,
    )
    .await?;
    let queued = view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await;
    assert_eq!(queued["amount"], json!("1000000"), "{queued}");

    let early = send(&w, "apply_pending_withdrawal", json!({}), &stranger, 0)
        .await
        .expect_err("the withdrawal must not be sent early");
    assert!(early.contains("still in its delay"), "{early}");

    send(&w, "cancel_pending_withdrawal", json!({}), &stranger, 1)
        .await
        .expect_err("only the owner can cancel");
    send(&w, "cancel_pending_withdrawal", json!({}), &owner, 1).await?;
    assert_eq!(view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await, json!(null));

    // The mandate is unchanged by the queue.
    assert_eq!(view(&w.vault, &w.network, "get_mandate", json!({})).await, mandate());
    Ok(())
}

#[tokio::test]
async fn intents_moves_tokens_out_for_the_account_that_holds_them() -> testresult::TestResult<()> {
    let w = world().await?;
    let holder = w.stranger.clone();

    // Deposit 5 USDC into intents.near for the holder, as the vault's funding does.
    w.usdc
        .call_function("mint", json!({ "account_id": holder, "amount": "5000000" }))
        .transaction()
        .gas(NearGas::from_tgas(30))
        .with_signer(holder.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    w.usdc
        .call_function(
            "ft_transfer_call",
            json!({
                "receiver_id": w.intents.account_id(),
                "amount": "5000000",
                "msg": json!({ "receiver_id": holder }).to_string(),
            }),
        )
        .transaction()
        .gas(NearGas::from_tgas(200))
        .deposit(NearToken::from_yoctonear(1))
        .with_signer(holder.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    let before = view(
        &w.intents,
        &w.network,
        "mt_balance_of",
        json!({ "account_id": holder, "token_id": USDC }),
    )
    .await;
    assert_eq!(before, json!("5000000"));

    // The call the vault sends after its delay: `ft_withdraw` with 1 yoctoNEAR.
    let sent = w
        .intents
        .call_function(
            "ft_withdraw",
            json!({ "token": "usdc.sandbox", "receiver_id": holder, "amount": "1000000" }),
        )
        .transaction()
        .gas(NearGas::from_tgas(300))
        .deposit(NearToken::from_yoctonear(1))
        .with_signer(holder.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .into_result()
        .map_err(|e| format!("{e:?}"))?;
    let burnt = sent.total_gas_burnt.as_gas() as f64 / 1e12;
    println!("GAS | intents ft_withdraw, one token | {burnt:.2} TGas");

    let after = view(
        &w.intents,
        &w.network,
        "mt_balance_of",
        json!({ "account_id": holder, "token_id": USDC }),
    )
    .await;
    assert_eq!(after, json!("4000000"));
    let received =
        view(&w.usdc, &w.network, "ft_balance_of", json!({ "account_id": holder })).await;
    assert_eq!(received, json!("1000000"));
    Ok(())
}
