//! Actual vault callback chains with a documented-shape Pyth stand-in, the
//! fixture Intents binary and sandbox signer. These establish sandbox wiring,
//! not live Pyth cache availability, real MPC operation or a funded solver fill.

mod common;

use common::{
    USDC, WNEAR, World, extra_asset, fund_extra_assets, fund_vault, now_sec, propose, register_key,
    view, world_with_pyth,
};
use near_api::NearGas;
use near_sdk::serde_json::{Value, json};

fn feed(index: usize) -> String {
    format!("{:064x}", index + 1)
}

fn price(mantissa: i64, confidence: u64, published: u64) -> Value {
    json!({
        "price": mantissa.to_string(),
        "conf": confidence.to_string(),
        "expo": -8,
        "publish_time": published,
    })
}

async fn set_price(w: &World, index: usize, price: Value) -> testresult::TestResult<()> {
    w.oracle
        .call_function("set_pyth_report", json!({ "price_id": feed(index), "price": price }))
        .transaction()
        .gas(NearGas::from_tgas(10))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    Ok(())
}

async fn fresh_prices(w: &World) -> testresult::TestResult<()> {
    fresh_prices_with_confidence(w, 0).await
}

async fn fresh_prices_with_confidence(w: &World, confidence: u64) -> testresult::TestResult<()> {
    let published = now_sec();
    let prices: near_sdk::serde_json::Map<String, Value> = (0..w.extra + 2)
        .map(|index| {
            let value = if index == 0 {
                price(450_000_000, 0, published)
            } else {
                price(100_000_000, confidence, published)
            };
            (feed(index), value)
        })
        .collect();
    w.oracle
        .call_function("set_pyth_reports", json!({ "prices": prices }))
        .transaction()
        .gas(NearGas::from_tgas(30))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    Ok(())
}

async fn execute(w: &World) -> testresult::TestResult<(Value, u64)> {
    let result = w
        .vault
        .call_function("execute", json!({}))
        .transaction()
        .gas(NearGas::from_tgas(300))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    let gas = result.total_gas_burnt.as_gas();
    Ok((result.json::<Value>()?, gas))
}

async fn assert_requested_feeds(w: &World, count: usize) {
    let requested = view(&w.oracle, &w.network, "get_last_pyth_request", json!({})).await;
    assert_eq!(
        requested,
        json!({ "price_ids": (0..count).map(feed).collect::<Vec<_>>(), "age": 60 })
    );
    assert_eq!(view(&w.vault, &w.network, "get_oracle_kind", json!({})).await, json!("pyth"));
}

async fn assert_refusal(
    w: &World,
    expected_asset: &str,
    before: Value,
) -> testresult::TestResult<()> {
    let (outcome, _) = execute(w).await?;
    assert_eq!(
        outcome,
        json!({ "Rejected": {
            "code": "MissingPrice", "reason": format!("MissingPrice: no oracle price for {expected_asset}")
        } }),
    );
    assert_eq!(view(&w.vault, &w.network, "get_trading_state", json!({})).await, before);
    assert_eq!(view(&w.vault, &w.network, "get_signed_intent", json!({})).await, Value::Null);
    assert_eq!(view(&w.vault, &w.network, "get_pending_proposal", json!({})).await, Value::Null);
    assert_eq!(view(&w.vault, &w.network, "is_in_flight", json!({})).await, json!(false));
    assert_eq!(view(&w.vault, &w.network, "get_strikes", json!({})).await, json!(1));
    let recorded = view(&w.vault, &w.network, "get_last_proposal_outcome", json!({})).await;
    assert_eq!(recorded, json!({ "proposal_id": "1", "outcome": outcome }));
    Ok(())
}

async fn assert_signature_accepted(w: &World, outcome: Value) -> testresult::TestResult<()> {
    let signed = view(&w.vault, &w.network, "get_signed_intent", json!({})).await;
    let expiry = signed["expires_sec"].as_u64().expect("integer signature deadline");
    assert_eq!(
        outcome,
        json!({ "Executed": { "proposal_id": "1", "expires_sec": expiry.to_string() } })
    );
    assert_eq!(signed["public_key"], json!(w.derived_key));
    let payload: Value =
        near_sdk::serde_json::from_str(signed["payload"].as_str().expect("payload"))?;
    assert_eq!(payload["signer_id"], json!(w.vault.account_id()));
    assert_eq!(payload["verifying_contract"], json!(w.intents.account_id()));
    assert_eq!(payload["intents"][0]["diff"][USDC], json!("-2000000"));
    assert_eq!(payload["intents"][0]["diff"][WNEAR], json!("450000000000000000000000"));
    let simulation = w
        .intents
        .call_function(
            "simulate_intents",
            json!({ "signed": [{
            "standard": "raw_ed25519", "payload": signed["payload"],
            "public_key": signed["public_key"], "signature": signed["signature"],
        }] }),
        )
        .read_only::<Value>()
        .fetch_from(&w.network)
        .await?
        .data;
    assert!(simulation["intents_executed"].to_string().contains(w.vault.account_id().as_str()));
    let trading = view(&w.vault, &w.network, "get_trading_state", json!({})).await;
    assert_eq!(trading["day_turnover_usd"], json!("2000000"));
    assert_eq!(trading["signed_intent_expires_at_sec"], json!(expiry));
    assert_eq!(view(&w.vault, &w.network, "get_pending_proposal", json!({})).await, Value::Null);
    assert_eq!(view(&w.vault, &w.network, "is_in_flight", json!({})).await, json!(false));
    assert_eq!(view(&w.vault, &w.network, "get_strikes", json!({})).await, json!(0));
    Ok(())
}

#[tokio::test]
async fn pyth_documented_response_signs_exact_intent_and_fixture_verifies_it()
-> testresult::TestResult<()> {
    let w = world_with_pyth(0).await?;
    register_key(&w).await?;
    fund_vault(&w, 10_000_000).await?;
    fresh_prices(&w).await?;
    // Confirm the mock's boundary shape independently from callback deserialization.
    let response = view(
        &w.oracle,
        &w.network,
        "preview_pyth_prices",
        json!({
            "price_ids": [feed(0), feed(1)], "age": 60,
        }),
    )
    .await;
    assert!(response[feed(0)]["price"].is_string());
    assert!(response[feed(0)]["conf"].is_string());
    assert_eq!(response[feed(0)]["expo"], json!(-8));
    assert!(response[feed(0)]["publish_time"].is_u64());
    propose(&w).await?;
    let (outcome, gas) = execute(&w).await?;
    assert_signature_accepted(&w, outcome).await?;
    assert_requested_feeds(&w, 2).await;
    println!("PYTH GAS | 2 assets | {} TGas | sandbox callback chain", gas as f64 / 1e12);
    Ok(())
}

#[tokio::test]
async fn pyth_null_traded_price_rejects_without_quota_or_signature() -> testresult::TestResult<()> {
    let w = world_with_pyth(0).await?;
    register_key(&w).await?;
    fund_vault(&w, 10_000_000).await?;
    fresh_prices(&w).await?;
    set_price(&w, 1, Value::Null).await?;
    let before = view(&w.vault, &w.network, "get_trading_state", json!({})).await;
    propose(&w).await?;
    assert_refusal(&w, USDC, before).await?;
    assert_requested_feeds(&w, 2).await;
    Ok(())
}

#[tokio::test]
async fn pyth_confidence_one_unit_over_limit_rejects_without_quota_or_signature()
-> testresult::TestResult<()> {
    let w = world_with_pyth(0).await?;
    register_key(&w).await?;
    fund_vault(&w, 10_000_000).await?;
    fresh_prices(&w).await?;
    // At price 100,000,000 and 100 bps, 1,000,000 is permitted; one above is refused.
    set_price(&w, 1, price(100_000_000, 1_000_001, now_sec())).await?;
    let before = view(&w.vault, &w.network, "get_trading_state", json!({})).await;
    propose(&w).await?;
    assert_refusal(&w, USDC, before).await?;
    assert_requested_feeds(&w, 2).await;
    Ok(())
}

#[tokio::test]
async fn pyth_twenty_asset_callback_chain_fits_gas_and_checks_every_held_feed()
-> testresult::TestResult<()> {
    let w = world_with_pyth(18).await?;
    assert_eq!(w.extra + 2, mandate_vault::mandate::MAX_ASSETS);
    register_key(&w).await?;
    fund_vault(&w, 10_000_000).await?;
    fund_extra_assets(&w, 1_000_000).await?;
    fresh_prices(&w).await?;
    // A held asset outside the traded pair must be priced too.
    set_price(&w, 19, Value::Null).await?;
    let before = view(&w.vault, &w.network, "get_trading_state", json!({})).await;
    propose(&w).await?;
    assert_refusal(&w, &extra_asset(17), before).await?;
    assert_requested_feeds(&w, 20).await;

    // The equality boundary is accepted on every non-wNEAR feed, including all held assets.
    fresh_prices_with_confidence(&w, 1_000_000).await?;
    propose(&w).await?;
    let (outcome, gas) = execute(&w).await?;
    // This is proposal 2 after the intentional refusal. Assert its exact result separately.
    let signed = view(&w.vault, &w.network, "get_signed_intent", json!({})).await;
    let expiry = signed["expires_sec"].as_u64().expect("deadline");
    assert_eq!(
        outcome,
        json!({ "Executed": { "proposal_id": "2", "expires_sec": expiry.to_string() } })
    );
    assert_eq!(signed["proposal_id"], json!("2"));
    let trading = view(&w.vault, &w.network, "get_trading_state", json!({})).await;
    assert_eq!(trading["day_turnover_usd"], json!("2000000"));
    assert_eq!(view(&w.vault, &w.network, "get_strikes", json!({})).await, json!(0));
    assert_eq!(view(&w.vault, &w.network, "is_in_flight", json!({})).await, json!(false));
    assert_requested_feeds(&w, 20).await;
    assert!(
        gas < NearGas::from_tgas(300).as_gas(),
        "supported limit must complete within NEAR gas cap"
    );
    println!("PYTH GAS | 20 assets | {} TGas | sandbox callback chain", gas as f64 / 1e12);
    Ok(())
}
