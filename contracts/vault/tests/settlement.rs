//! Actual matched, funded settlement against the supplied Intents WASM.
//! Both FT ledgers are funded through deposits. A sandbox-only second signer
//! represents the counterparty; neither live MPC nor a solver relay is implied.

mod common;

use common::{
    REASON, SIGNER_SEED_HEX, USDC, World, create_account, extra_account, extra_asset, fund_vault,
    now_sec, register_key, set_prices, view, world_with_extra,
};
use mandate_vault::intent::{TokenDiff, token_diff_payload, v1_nonce};
use near_api::{Contract, NearGas, NearToken};
use near_sdk::serde_json::{Value, json};

async fn balance(w: &World, account: &near_api::AccountId, asset: &str) -> Value {
    view(
        &w.intents,
        &w.network,
        "mt_balance_of",
        json!({ "account_id": account, "token_id": asset }),
    )
    .await
}

fn signed_entry(payload: &str, public_key: &Value, signature: &Value) -> Value {
    json!({
        "standard": "raw_ed25519",
        "payload": payload,
        "public_key": public_key,
        "signature": signature,
    })
}

async fn counterparty_signer(w: &World) -> testresult::TestResult<Contract> {
    let id = create_account(&w._sandbox, "counterparty-mpc.sandbox", 10).await?;
    let opts = cargo_near_build::BuildOpts::builder()
        .manifest_path("../mock-externals/Cargo.toml")
        .build();
    let wasm = std::fs::read(cargo_near_build::build_with_cli(opts)?)?;
    Contract::deploy(id.clone())
        .use_code(wasm)
        .with_init_call(
            "new",
            json!({ "seed_hex": SIGNER_SEED_HEX, "expected_predecessor": w.stranger }),
        )?
        .with_signer(w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    // The second mock uses the same sandbox-only signing material. Accounts
    // independently authorize this public key; signer_id remains signed bytes.
    w.intents
        .call_function("add_public_key", json!({ "public_key": w.derived_key }))
        .transaction()
        .gas(NearGas::from_tgas(30))
        .deposit(NearToken::from_yoctonear(1))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    Ok(Contract(id))
}

async fn counterparty_intent(
    w: &World,
    mpc: &Contract,
    deadline: u64,
    amount_out: u128,
    nonce_byte: u8,
) -> testresult::TestResult<Value> {
    let salt = view(&w.intents, &w.network, "current_salt", json!({})).await;
    let salt = salt.as_str().expect("Intents salt is hex");
    assert_eq!(salt.len(), 8);
    let mut salt_bytes = [0; 4];
    for (i, byte) in salt_bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&salt[i * 2..i * 2 + 2], 16)?;
    }
    let asset = extra_asset(0);
    let payload = token_diff_payload(&TokenDiff {
        signer_id: w.stranger.as_str(),
        verifying_contract: w.intents.account_id().as_str(),
        deadline_sec: deadline,
        nonce: v1_nonce(salt_bytes, deadline, [nonce_byte; 15]).map_err(|e| e.to_string())?,
        asset_in: &asset,
        amount_in: amount_out,
        asset_out: USDC,
        min_amount_out: 2_000_000,
    })
    .map_err(|e| e.to_string())?;
    let encoded: String = payload.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    let response = mpc
        .call_function(
            "sign",
            json!({ "request": {
                "path": "intents",
                "payload_v2": { "Eddsa": encoded },
                "domain_id": 1,
            } }),
        )
        .transaction()
        .gas(NearGas::from_tgas(30))
        .deposit(NearToken::from_yoctonear(1))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success()
        .json::<Value>()?;
    assert_eq!(response["scheme"], json!("Ed25519"));
    let bytes: Vec<u8> = near_sdk::serde_json::from_value(response["signature"].clone())?;
    assert_eq!(bytes.len(), 64);
    let signature = json!(format!("ed25519:{}", bs58::encode(bytes).into_string()));
    Ok(signed_entry(&payload, &json!(w.derived_key), &signature))
}

async fn execute_bundle(w: &World, signed: Vec<Value>) -> Result<(), String> {
    w.intents
        .call_function("execute_intents", json!({ "signed": signed }))
        .transaction()
        .gas(NearGas::from_tgas(300))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await
        .map_err(|e| e.to_string())?
        .into_result()
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

#[tokio::test]
async fn matched_funded_settlement_is_atomic_and_replay_is_refused() -> testresult::TestResult<()> {
    let w = world_with_extra(1).await?;
    let asset = extra_asset(0);
    let ft = Contract(extra_account(0).parse()?);
    let counterparty = counterparty_signer(&w).await?;
    register_key(&w).await?;
    fund_vault(&w, 10_000_000).await?;
    ft.call_function("mint", json!({ "account_id": w.stranger, "amount": "2000000" }))
        .transaction()
        .gas(NearGas::from_tgas(30))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    ft.call_function(
        "ft_transfer_call",
        json!({
            "receiver_id": w.intents.account_id(),
            "amount": "2000000",
            "msg": json!({ "receiver_id": w.stranger }).to_string(),
        }),
    )
    .transaction()
    .gas(NearGas::from_tgas(200))
    .deposit(NearToken::from_yoctonear(1))
    .with_signer(w.stranger.clone(), w.signer.clone())
    .send_to(&w.network)
    .await?
    .assert_success();
    set_prices(&w, now_sec()).await?;
    w.vault
        .call_function(
            "propose",
            json!({
                "asset_in": USDC, "asset_out": asset,
                "amount_in": "2000000", "min_amount_out": "2000000", "reason_hash": REASON,
            }),
        )
        .transaction()
        .gas(NearGas::from_tgas(30))
        .with_signer(w.agent.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    let outcome = w
        .vault
        .call_function("execute", json!({}))
        .transaction()
        .gas(NearGas::from_tgas(300))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success()
        .json::<Value>()?;
    assert!(outcome.get("Executed").is_some(), "{outcome}");
    let signed = view(&w.vault, &w.network, "get_signed_intent", json!({})).await;
    let deadline = signed["expires_sec"].as_u64().expect("signature deadline");
    let vault_intent = signed_entry(
        signed["payload"].as_str().expect("payload"),
        &signed["public_key"],
        &signed["signature"],
    );
    let vault = w.vault.account_id();
    assert_eq!(balance(&w, vault, USDC).await, json!("10000000"));
    assert_eq!(balance(&w, vault, &asset).await, json!("0"));
    assert_eq!(balance(&w, &w.stranger, USDC).await, json!("0"));
    assert_eq!(balance(&w, &w.stranger, &asset).await, json!("2000000"));

    // The counterparty supplies one atomic unit less than the vault's signed
    // minimum. The entire bundle must fail, leaving every balance unchanged.
    let short = counterparty_intent(&w, &counterparty, deadline, 1_999_999, 7).await?;
    let under_delivery = execute_bundle(&w, vec![vault_intent.clone(), short])
        .await
        .expect_err("one atomic unit of under-delivery must refuse the whole bundle");
    assert!(
        under_delivery.contains("invariant violated")
            && under_delivery.contains("unmatched_deltas"),
        "under-delivery must fail the ledger invariant: {under_delivery}"
    );
    println!("SETTLEMENT | under-delivery refused | unmatched_deltas (one atomic unit)");
    assert_eq!(balance(&w, vault, USDC).await, json!("10000000"));
    assert_eq!(balance(&w, vault, &asset).await, json!("0"));
    assert_eq!(balance(&w, &w.stranger, USDC).await, json!("0"));
    assert_eq!(balance(&w, &w.stranger, &asset).await, json!("2000000"));

    // Reuse the same vault signature: failed atomic execution must not consume
    // its nonce. The exact matching bundle now commits both ledger changes.
    let matching = counterparty_intent(&w, &counterparty, deadline, 2_000_000, 8).await?;
    let bundle = vec![vault_intent, matching];
    execute_bundle(&w, bundle.clone()).await?;
    assert_eq!(balance(&w, vault, USDC).await, json!("8000000"));
    assert_eq!(balance(&w, vault, &asset).await, json!("2000000"));
    assert_eq!(balance(&w, &w.stranger, USDC).await, json!("2000000"));
    assert_eq!(balance(&w, &w.stranger, &asset).await, json!("0"));

    let replay = execute_bundle(&w, bundle).await.expect_err("a used intent must not settle again");
    assert!(replay.to_lowercase().contains("nonce"), "replay refusal must name nonce: {replay}");
    assert_eq!(balance(&w, vault, USDC).await, json!("8000000"));
    assert_eq!(balance(&w, vault, &asset).await, json!("2000000"));
    assert_eq!(balance(&w, &w.stranger, USDC).await, json!("2000000"));
    assert_eq!(balance(&w, &w.stranger, &asset).await, json!("0"));
    Ok(())
}
