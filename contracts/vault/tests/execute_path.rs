//! `execute` on a local NEAR sandbox: the vault reads balances from the real
//! `intents.near`, prices from the oracle stand-in, applies the gates, and
//! asks the signer stand-in to sign. Each closed gate is a named rejection
//! that consumes the proposal and counts a strike.

mod common;

use common::{
    AMOUNT_IN, MIN_OUT, REASON, USDC, WNEAR, World, fund_vault, now_sec, propose, register_key,
    set_prices, view, world,
};
use near_api::NearGas;
use near_sdk::serde_json::{Value, json};

/// Runs `execute` as a stranger (it is permissionless) and returns its outcome.
async fn execute(w: &World) -> testresult::TestResult<Value> {
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
    Ok(outcome)
}

async fn strikes(w: &World) -> Value {
    view(&w.vault, &w.network, "get_strikes", json!({})).await
}

#[tokio::test]
async fn execute_without_a_proposal_is_refused() -> testresult::TestResult<()> {
    let w = world().await?;
    w.vault
        .call_function("execute", json!({}))
        .transaction()
        .gas(NearGas::from_tgas(300))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_failure();
    Ok(())
}

#[tokio::test]
async fn a_missing_oracle_price_rejects_and_counts_a_strike() -> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    // No reports set: the oracle answers null, so the engine has no price.
    propose(&w).await?;

    let outcome = execute(&w).await?;
    assert_eq!(outcome["Rejected"]["code"], json!("MissingPrice"), "got {outcome}");
    assert_eq!(view(&w.vault, &w.network, "get_pending_proposal", json!({})).await, Value::Null);
    assert_eq!(strikes(&w).await, json!(1));
    assert_eq!(view(&w.vault, &w.network, "is_in_flight", json!({})).await, json!(false));
    assert_eq!(view(&w.vault, &w.network, "get_signed_intent", json!({})).await, Value::Null);
    Ok(())
}

#[tokio::test]
async fn an_old_price_is_refused_by_the_mandate_age_limit() -> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    // 100 s old: inside the oracle's 600 s recency, outside the mandate's 60 s limit.
    set_prices(&w, now_sec() - 100).await?;
    propose(&w).await?;

    let outcome = execute(&w).await?;
    assert_eq!(outcome["Rejected"]["code"], json!("StalePrice"), "got {outcome}");
    assert_eq!(strikes(&w).await, json!(1));
    Ok(())
}

#[tokio::test]
async fn an_unfunded_vault_is_rejected_and_three_strikes_halt_the_vault()
-> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;

    // Fresh prices, but the vault holds nothing on intents.near.
    set_prices(&w, now_sec()).await?;
    for expected in 1..=3u64 {
        propose(&w).await?;
        let outcome = execute(&w).await?;
        assert_eq!(outcome["Rejected"]["code"], json!("InsufficientBalance"), "got {outcome}");
        assert_eq!(strikes(&w).await, json!(expected));
    }

    // The third strike halts proposals, and the agent cannot propose again.
    assert_eq!(view(&w.vault, &w.network, "is_halted", json!({})).await, json!(true));
    let refused = w
        .vault
        .call_function(
            "propose",
            json!({
                "asset_in": USDC,
                "asset_out": WNEAR,
                "amount_in": AMOUNT_IN,
                "min_amount_out": MIN_OUT,
                "reason_hash": REASON,
            }),
        )
        .transaction()
        .gas(NearGas::from_tgas(30))
        .with_signer(w.agent.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?;
    refused.assert_failure();

    // Only the owner can resume, and resuming clears the strikes.
    w.vault
        .call_function("resume_proposals", json!({}))
        .transaction()
        .gas(NearGas::from_tgas(30))
        .deposit(near_api::NearToken::from_yoctonear(1))
        .with_signer(w.owner.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    assert_eq!(view(&w.vault, &w.network, "is_halted", json!({})).await, json!(false));
    assert_eq!(strikes(&w).await, json!(0));
    Ok(())
}

#[tokio::test]
async fn a_paused_vault_does_not_execute() -> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    set_prices(&w, now_sec()).await?;
    propose(&w).await?;
    w.vault
        .call_function("pause", json!({}))
        .transaction()
        .gas(NearGas::from_tgas(30))
        .with_signer(w.owner.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();

    w.vault
        .call_function("execute", json!({}))
        .transaction()
        .gas(NearGas::from_tgas(300))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_failure();
    // The refused call changed nothing: the proposal is still pending, and no strike was counted.
    assert!(view(&w.vault, &w.network, "get_pending_proposal", json!({})).await.is_object());
    assert_eq!(strikes(&w).await, json!(0));
    Ok(())
}

/// The signed intent in the form `intents.near.simulate_intents` takes it.
fn signed_entry(signed: &Value, payload: &str) -> Value {
    json!({
        "standard": "raw_ed25519",
        "payload": payload,
        "public_key": signed["public_key"],
        "signature": signed["signature"],
    })
}

async fn simulate(w: &World, signed: Value) -> Result<Value, String> {
    w.intents
        .call_function("simulate_intents", json!({ "signed": [signed] }))
        .read_only::<Value>()
        .fetch_from(&w.network)
        .await
        .map(|r| r.data)
        .map_err(|e| e.to_string())
}

/// The full path: funded vault, the vault decides, signs, and the real verifier accepts the signature.
#[tokio::test]
async fn a_funded_trade_is_signed_and_intents_accepts_it() -> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    fund_vault(&w, 10_000_000).await?; // 10 USDC
    set_prices(&w, now_sec()).await?;
    propose(&w).await?;

    let outcome = execute(&w).await?;
    assert_eq!(outcome["Executed"]["proposal_id"], json!("1"), "got {outcome}");

    let signed = view(&w.vault, &w.network, "get_signed_intent", json!({})).await;
    assert_eq!(signed["public_key"], json!(w.derived_key));
    assert!(signed["signature"].as_str().is_some_and(|s| s.starts_with("ed25519:")), "{signed}");
    // The vault recorded the trade: turnover is the notional, $2 in micro-dollars.
    let state = view(&w.vault, &w.network, "get_trading_state", json!({})).await;
    assert_eq!(state["day_turnover_usd"], json!("2000000"), "{state}");
    assert_eq!(view(&w.vault, &w.network, "get_pending_proposal", json!({})).await, Value::Null);
    assert_eq!(strikes(&w).await, json!(0));
    assert_eq!(view(&w.vault, &w.network, "is_in_flight", json!({})).await, json!(false));

    // The real verifier accepts the signature and the intent.
    let payload = signed["payload"].as_str().expect("payload");
    let sim = simulate(&w, signed_entry(&signed, payload)).await.map_err(|e| e.to_string())?;
    assert!(sim["intents_executed"].to_string().contains("vault.sandbox"), "{sim}");
    Ok(())
}

/// The signature covers the exact bytes: changing one amount is rejected by the real verifier.
#[tokio::test]
async fn a_tampered_payload_is_rejected_by_the_real_verifier() -> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    fund_vault(&w, 10_000_000).await?;
    set_prices(&w, now_sec()).await?;
    propose(&w).await?;
    let outcome = execute(&w).await?;
    assert!(outcome.get("Executed").is_some(), "got {outcome}");

    let signed = view(&w.vault, &w.network, "get_signed_intent", json!({})).await;
    let original = signed["payload"].as_str().expect("payload");
    let control = simulate(&w, signed_entry(&signed, original)).await?;
    assert!(control["intents_executed"].to_string().contains("vault.sandbox"), "{control}");
    let payload = original.replace("-2000000", "-3000000");
    assert_ne!(payload, original, "the control and tampered bytes must differ");
    let error = simulate(&w, signed_entry(&signed, &payload))
        .await
        .expect_err("a changed amount must not verify");
    assert!(error.contains("invalid signature"), "{error}");
    Ok(())
}

/// A stored signed intent blocks the next proposal until it expires.
#[tokio::test]
async fn a_signed_intent_blocks_the_next_proposal() -> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    fund_vault(&w, 10_000_000).await?;
    set_prices(&w, now_sec()).await?;
    propose(&w).await?;
    execute(&w).await?;

    let refused = w
        .vault
        .call_function(
            "propose",
            json!({
                "asset_in": USDC,
                "asset_out": WNEAR,
                "amount_in": AMOUNT_IN,
                "min_amount_out": MIN_OUT,
                "reason_hash": REASON,
            }),
        )
        .transaction()
        .gas(NearGas::from_tgas(30))
        .with_signer(w.agent.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?;
    refused.assert_failure();
    Ok(())
}
