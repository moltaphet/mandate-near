//! Actual sandbox deployment without ever adding a vault account permission.
//! This verifies the fresh issuer runtime premise, not a production deployment.

mod common;

use common::{USDC, mandate, view};
use near_api::types::transaction::actions::{
    Action, CreateAccountAction, DeployContractAction, FunctionCallAction, TransferAction,
};
use near_api::{Account, AccountId, Contract, NearGas, NearToken, Transaction};
use near_sdk::serde_json::{Value, json};

#[tokio::test]
async fn fresh_atomic_deployment_has_no_vault_permission_and_retains_external_owner_controls()
-> testresult::TestResult<()> {
    let w = common::world().await?;
    // This specific build output is needed to test deployment of the current
    // source. No dependency or other target-directory content is inspected.
    let opts = cargo_near_build::BuildOpts::builder()
        .manifest_path(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .build();
    let code = std::fs::read(cargo_near_build::build_with_cli(opts)?)?;
    println!("DEPLOYMENT | reviewed WASM byte length {}", code.len());
    let vault_id: AccountId = format!("sealed.{}", w.owner).parse()?;
    let initialize = json!({
        "owner": w.owner,
        "mandate": mandate(),
        "signer": w.mpc.account_id(),
        "intents": w.intents.account_id(),
        "oracle": w.oracle.account_id(),
    });
    // The parent owner creates/funds/deploys/initializes its fresh child in one
    // receipt. There is deliberately no AddKey action at any stage.
    let deployed = Transaction::construct(w.owner.clone(), vault_id.clone())
        .add_action(Action::CreateAccount(CreateAccountAction {}))
        .add_action(Action::Transfer(TransferAction { deposit: NearToken::from_near(6) }))
        .add_action(Action::DeployContract(DeployContractAction { code }))
        .add_action(Action::FunctionCall(Box::new(FunctionCallAction {
            method_name: "new_with_externals".into(),
            args: near_sdk::serde_json::to_vec(&initialize)?,
            gas: NearGas::from_tgas(30),
            deposit: NearToken::from_yoctonear(0),
        })))
        .with_signer(w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    assert!(deployed.receipt_failures().is_empty());
    let vault = Contract(vault_id.clone());
    assert_eq!(view(&vault, &w.network, "get_owner", json!({})).await, json!(w.owner));
    assert_ne!(vault_id, w.owner);
    assert!(Account(vault_id.clone()).list_keys().fetch_from(&w.network).await?.data.is_empty());

    // The deployer's signer cannot send any vault-originated issuer call: the
    // account has never had an access permission from which to authorize it.
    let attempted_issuer = w
        .intents
        .call_function(
            "mt_on_transfer",
            json!({
                "sender_id": w.stranger,
                "previous_owner_ids": [w.stranger],
                "token_ids": ["withdrawal-1"],
                "amounts": ["1"],
                "msg": "",
            }),
        )
        .transaction()
        .gas(NearGas::from_tgas(30))
        .with_signer(vault_id.clone(), w.signer.clone())
        .send_to(&w.network)
        .await;
    let missing_authority = attempted_issuer
        .expect_err("an account without permissions cannot authorize a transaction");
    let error = format!("{missing_authority:?}");
    assert!(
        error.contains("UNKNOWN_ACCESS_KEY") || error.contains("AccessKeyDoesNotExist"),
        "the issuer refusal must identify missing access authority, not a transport failure"
    );
    let voucher = format!("nep245:{vault_id}:withdrawal-1");
    assert_eq!(
        view(&w.intents, &w.network, "mt_supply", json!({ "token_id": voucher })).await,
        json!("0")
    );

    let denied = vault
        .call_function("pause", json!({}))
        .transaction()
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .into_result()
        .expect_err("an outside account must not gain owner authority from keyless deployment");
    assert!(format!("{denied:?}").contains("only the owner can call this method"));
    vault
        .call_function("pause", json!({}))
        .transaction()
        .with_signer(w.owner.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    assert_eq!(view(&vault, &w.network, "is_paused", json!({})).await, json!(true));
    vault
        .call_function("unpause", json!({}))
        .transaction()
        .deposit(NearToken::from_yoctonear(1))
        .with_signer(w.owner.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    assert_eq!(view(&vault, &w.network, "is_paused", json!({})).await, json!(false));

    let before = view(&vault, &w.network, "get_chain_time_sec", json!({})).await;
    vault
        .call_function(
            "request_withdrawal",
            json!({ "asset_id": USDC, "receiver": w.owner, "amount": "20" }),
        )
        .transaction()
        .deposit(NearToken::from_yoctonear(1))
        .with_signer(w.owner.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    let queued = view(&vault, &w.network, "get_pending_withdrawal", json!({})).await;
    let after = view(&vault, &w.network, "get_chain_time_sec", json!({})).await;
    assert_eq!(queued["asset_id"], json!(USDC));
    assert_eq!(queued["receiver"], json!(w.owner));
    assert_eq!(queued["amount"], json!("20"));
    let effective = queued["effective_sec"].as_u64().expect("withdrawal maturity is JSON u64");
    let before = before.as_u64().expect("chain time is JSON u64");
    let after = after.as_u64().expect("chain time is JSON u64");
    assert!(
        (before + mandate_vault::WITHDRAW_DELAY_SEC..=after + mandate_vault::WITHDRAW_DELAY_SEC)
            .contains(&effective)
    );
    let early = vault
        .call_function("apply_pending_withdrawal", json!({}))
        .transaction()
        .gas(NearGas::from_tgas(280))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .into_result()
        .expect_err("fresh issuer deployment must preserve the owner delay");
    assert!(format!("{early:?}").contains("the withdrawal is still in its delay"));
    assert_eq!(view(&vault, &w.network, "get_pending_withdrawal", json!({})).await, queued);
    vault
        .call_function("cancel_pending_withdrawal", json!({}))
        .transaction()
        .deposit(NearToken::from_yoctonear(1))
        .with_signer(w.owner.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    assert_eq!(view(&vault, &w.network, "get_pending_withdrawal", json!({})).await, Value::Null);
    assert!(Account(vault_id).list_keys().fetch_from(&w.network).await?.data.is_empty());
    println!(
        "DEPLOYMENT | atomic CreateAccount/Transfer/DeployContract/init with no AddKey: zero vault permissions; prior issuer transaction refused; distinct owner pause/unpause/24h queue/cancel work"
    );
    Ok(())
}
