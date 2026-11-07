use std::collections::BTreeMap;

use mandate_agent::autonomous::{
    AutonomousConfig, Candidate, Preparation, Snapshot, choose, preview,
};
use mandate_agent::relay::Quote;
use mandate_vault::engine::{Market, TradingState};
use mandate_vault::math::Price;
use mandate_vault::{AssetConfig, Mandate};
use near_sdk::json_types::{U64, U128};

const A: &str = "nep141:usd.near";
const B: &str = "nep141:wrap.near";
const NOW: u64 = 1_793_448_000;
const NEAR: u128 = 10u128.pow(24);

fn config() -> AutonomousConfig {
    AutonomousConfig {
        rpc_url: "http://127.0.0.1:1".into(),
        expected_chain_id: Some("localnet".into()),
        vault_id: "vault.near".into(),
        relay_url: "http://127.0.0.1:1/rpc".into(),
        asset_a: A.into(),
        asset_b: B.into(),
        target_a_bps: 5000,
        band_bps: 100,
        minimum_quote_validity_sec: 450,
        legacy_price_oracle: false,
    }
}
fn snapshot() -> Snapshot {
    Snapshot {
        mandate: Mandate {
            assets: vec![
                AssetConfig { asset_id: A.into(), decimals: 6, price_feed_id: "usd.near".into() },
                AssetConfig { asset_id: B.into(), decimals: 24, price_feed_id: "wrap.near".into() },
            ],
            stable_assets: vec![A.into()],
            max_trade_usd: U64(20_000_000),
            max_daily_turnover_usd: U64(200_000_000),
            max_position_bps: 6000,
            max_slippage_bps: 100,
            max_drawdown_bps: 1000,
            min_trade_interval_sec: 60,
            max_price_age_sec: 60,
            allowed_model_ids: vec![
                "manual-v1".into(),
                "rebalance-v1".into(),
                "fixture-model".into(),
            ],
        },
        trading: TradingState::default(),
        market: Market {
            now_sec: NOW,
            prices: BTreeMap::from([
                (A.into(), Price { mantissa: U64(1), expo: 0, publish_time_sec: NOW }),
                (B.into(), Price { mantissa: U64(5), expo: 0, publish_time_sec: NOW }),
            ]),
            balances: BTreeMap::from([(A.into(), 100_000_000), (B.into(), 10 * NEAR)]),
        },
        block_hash: bs58::encode([4u8; 32]).into_string(),
        intents_id: "intents.near".into(),
        agent_id: Some("agent.near".into()),
        not_ready: None,
    }
}
fn quote(amount: u128, out: u128) -> Quote {
    Quote {
        quote_hash: "fixture-offer-1".into(),
        defuse_asset_identifier_in: A.into(),
        defuse_asset_identifier_out: B.into(),
        amount_in: U128(amount),
        amount_out: U128(out),
        expiration_time: mandate_vault::intent::rfc3339_utc(NOW + 600),
    }
}

#[test]
fn exact_candidate_respects_trade_daily_and_balance_caps() {
    let config = config();
    let mut snapshot = snapshot();
    assert_eq!(
        choose(&config, &snapshot).unwrap(),
        Candidate { asset_in: A.into(), asset_out: B.into(), amount_in: 20_000_000 }
    );
    snapshot.trading.day_start_sec = NOW - NOW % 86_400;
    snapshot.trading.day_turnover_usd = 197_000_000;
    assert_eq!(choose(&config, &snapshot).unwrap().amount_in, 3_000_000);
    snapshot.trading.day_start_sec -= 86_400;
    assert_eq!(choose(&config, &snapshot).unwrap().amount_in, 20_000_000);
    snapshot.market.balances.insert(A.into(), u128::MAX);
    assert_eq!(choose(&config, &snapshot).unwrap_err().to_string(), "strategy NAV overflow");
}

#[test]
fn reverse_rebalance_keeps_24_decimal_precision() {
    let config = config();
    let mut snapshot = snapshot();
    snapshot.market.balances.insert(A.into(), 10_000_000);
    let candidate = choose(&config, &snapshot).unwrap();
    assert_eq!(candidate.asset_in, B);
    assert_eq!(candidate.asset_out, A);
    assert_eq!(candidate.amount_in, 4 * NEAR);
}

#[test]
fn denied_or_removed_rebalance_source_refuses_before_strategy_and_quote() {
    let config = config();
    let mut snapshot = snapshot();
    assert!(choose(&config, &snapshot).is_ok());
    snapshot.mandate.allowed_model_ids.retain(|id| id != "rebalance-v1");
    assert_eq!(
        choose(&config, &snapshot).unwrap_err().to_string(),
        "the rebalance decision source is not allowed"
    );
    snapshot.mandate.allowed_model_ids = vec!["unknown-model".into()];
    assert_eq!(
        choose(&config, &snapshot).unwrap_err().to_string(),
        "the rebalance decision source is not allowed"
    );
    snapshot.mandate.allowed_model_ids.clear();
    assert_eq!(
        choose(&config, &snapshot).unwrap_err().to_string(),
        "the rebalance decision source is not allowed"
    );
}

#[test]
fn cooldown_stale_data_and_exhausted_allowance_refuse_before_quote() {
    let config = config();
    let mut snapshot = snapshot();
    snapshot.trading.last_trade_at_sec = Some(NOW - 59);
    assert!(choose(&config, &snapshot).unwrap_err().to_string().starts_with("CooldownActive:"));
    snapshot.trading.last_trade_at_sec = Some(NOW - 60);
    assert!(choose(&config, &snapshot).is_ok());
    snapshot.market.prices.get_mut(A).unwrap().publish_time_sec = NOW - 61;
    assert!(choose(&config, &snapshot).unwrap_err().to_string().starts_with("StalePrice:"));
    snapshot.market.prices.get_mut(A).unwrap().publish_time_sec = NOW - 60;
    assert!(choose(&config, &snapshot).is_ok());
    snapshot.trading.day_start_sec = NOW - NOW % 86_400;
    snapshot.trading.day_turnover_usd = 200_000_000;
    assert_eq!(
        choose(&config, &snapshot).unwrap_err().to_string(),
        "no remaining daily allowance or rebalance value"
    );
}

#[test]
fn target_band_is_no_trade_and_pending_signature_blocks() {
    let config = config();
    let mut snapshot = snapshot();
    snapshot.market.balances.insert(A.into(), 50_000_000);
    assert_eq!(
        choose(&config, &snapshot).unwrap_err().to_string(),
        "holdings are within the target band"
    );
    snapshot.trading.signed_intent_expires_at_sec = NOW + 1;
    assert!(
        choose(&config, &snapshot).unwrap_err().to_string().starts_with("SignedIntentOutstanding:")
    );
}

#[test]
fn quote_exact_output_is_used_and_shared_engine_rejects_economic_loss() {
    let config = config();
    let snapshot = snapshot();
    let candidate = choose(&config, &snapshot).unwrap();
    let Preparation::Trade { prepared } =
        preview(&config, &snapshot, candidate.clone(), quote(20_000_000, 4 * NEAR)).unwrap()
    else {
        panic!("valid quote should pass shared gates")
    };
    assert_eq!(prepared.plan.min_amount_out.0, 4 * NEAR);
    assert!(prepared.plan.rationale.contains("fixture-offer-1"));
    let Preparation::NoTrade { reason } =
        preview(&config, &snapshot, candidate, quote(20_000_000, 3 * NEAR)).unwrap()
    else {
        panic!("underpriced quote must not be proposed")
    };
    assert!(reason.starts_with("SlippageExceeded:"));
}

#[test]
fn quote_window_and_invalid_configuration_fail_closed() {
    let mut config = config();
    config.minimum_quote_validity_sec = 420;
    assert!(config.validate().is_err());
    let mut quote = quote(20_000_000, 4 * NEAR);
    quote.expiration_time = mandate_vault::intent::rfc3339_utc(NOW + 450);
    assert!(quote.validate(A, B, 20_000_000, NOW, 450).is_err());
    quote.expiration_time = mandate_vault::intent::rfc3339_utc(NOW + 451);
    assert!(quote.validate(A, B, 20_000_000, NOW, 450).is_ok());
    quote.defuse_asset_identifier_out = A.into();
    assert!(quote.validate(A, B, 20_000_000, NOW, 450).is_err());
}
