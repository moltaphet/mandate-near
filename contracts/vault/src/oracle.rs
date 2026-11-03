//! Adapter for the NEAR price oracle (`price-oracle.near`, Burrow-style
//! interface). Pure: turns the oracle's JSON into the engine's [`Price`]s.
//!
//! Two views are needed per trade:
//!
//! - `get_price_data(asset_ids)` gives the oracle's own aggregate, or `null`
//!   when no report is within its `recency_duration_sec` (600 s on mainnet).
//!   Its `timestamp` is the time of the call, not of the price.
//! - `get_asset(asset_id)` lists the individual reports with their real
//!   timestamps. The newest one is the price's publish time, so the
//!   mandate's own (stricter, at most 300 s) age limit can be enforced.
//!
//! An asset missing from either answer gets no price. The engine then
//! rejects the trade with `MissingPrice`: the vault fails closed.

use std::collections::BTreeMap;

use near_sdk::json_types::{U64, U128};
use near_sdk::near;

use crate::mandate::Mandate;
use crate::math::{MIN_PRICE_EXPO, Price};

/// USD price of one whole token: `multiplier / 10^decimals`.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OraclePrice {
    pub multiplier: U128,
    pub decimals: u8,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetOptionalPrice {
    pub asset_id: String,
    pub price: Option<OraclePrice>,
}

/// Answer of `get_price_data`.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceData {
    /// Nanoseconds. The time of the call, not of any report.
    pub timestamp: U64,
    pub recency_duration_sec: u32,
    pub prices: Vec<AssetOptionalPrice>,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub oracle_id: String,
    /// Nanoseconds.
    pub timestamp: U64,
    pub price: OraclePrice,
}

/// Answer of `get_asset`. Other fields (EMAs, signer lists) are ignored.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetView {
    pub reports: Vec<Report>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OracleError {
    ZeroMultiplier,
    MultiplierTooLarge,
    DecimalsOutOfRange(u8),
}

/// Converts the oracle's price to the engine's, stamped with `publish_time_sec`.
pub fn to_price(p: &OraclePrice, publish_time_sec: u64) -> Result<Price, OracleError> {
    if p.multiplier.0 == 0 {
        return Err(OracleError::ZeroMultiplier);
    }
    let mantissa = u64::try_from(p.multiplier.0).map_err(|_| OracleError::MultiplierTooLarge)?;
    let expo = -i32::from(p.decimals);
    if expo < MIN_PRICE_EXPO {
        return Err(OracleError::DecimalsOutOfRange(p.decimals));
    }
    Ok(Price { mantissa: U64(mantissa), expo, publish_time_sec })
}

/// Unix seconds of the newest report, if there is one.
pub fn latest_report_sec(asset: &AssetView) -> Option<u64> {
    asset.reports.iter().map(|r| r.timestamp.0 / 1_000_000_000).max()
}

/// Prices for the assets of `mandate`, keyed by Intents asset id.
///
/// `assets` maps a feed id to its `get_asset` answer. An asset is included
/// only if the oracle returned a usable price for it and a report with a
/// timestamp exists. Everything else is left out on purpose.
pub fn prices_for(
    mandate: &Mandate,
    data: &PriceData,
    assets: &BTreeMap<String, AssetView>,
) -> BTreeMap<String, Price> {
    let mut out = BTreeMap::new();
    for cfg in &mandate.assets {
        let Some(entry) = data.prices.iter().find(|p| p.asset_id == cfg.price_feed_id) else {
            continue;
        };
        let Some(oracle_price) = &entry.price else { continue };
        let Some(published) = assets.get(&cfg.price_feed_id).and_then(latest_report_sec) else {
            continue;
        };
        if let Ok(price) = to_price(oracle_price, published) {
            out.insert(cfg.asset_id.clone(), price);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use near_sdk::serde_json;

    use super::*;
    use crate::engine::{Market, Trade, TradingState, TradeError, check_trade};
    use crate::mandate::tests::{USDC, WNEAR, sample};

    const USDC_FEED: &str = "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";

    /// `get_asset` answers captured from `price-oracle.near` on 2026-11-03.
    /// The newest report is from 2026-06-27 (1782588756 s).
    const WNEAR_ASSET: &str = r#"{"reports":[
        {"oracle_id":"price-oracle.near","timestamp":"1772376912311014515","price":{"multiplier":"115619999","decimals":8}},
        {"oracle_id":"1e4eae7913600d015d9134eee5ed6325845d78bcdf5ba8045d23051c2cc16ccb","timestamp":"1782588756798165888","price":{"multiplier":"185158333","decimals":8}}
    ],"emas":[],"push_signer_accounts":["1e4eae7913600d015d9134eee5ed6325845d78bcdf5ba8045d23051c2cc16ccb"]}"#;
    const USDC_ASSET: &str = r#"{"reports":[
        {"oracle_id":"price-oracle.near","timestamp":"1772376912311014515","price":{"multiplier":"99997746","decimals":8}},
        {"oracle_id":"1e4eae7913600d015d9134eee5ed6325845d78bcdf5ba8045d23051c2cc16ccb","timestamp":"1782588756798165888","price":{"multiplier":"100039200","decimals":8}}
    ],"emas":[],"push_signer_accounts":[]}"#;
    /// `get_price_data` as captured on 2026-11-03: no usable price.
    const PRICE_DATA_NULL: &str = r#"{"timestamp":"1793697676080624873","recency_duration_sec":600,"prices":[
        {"asset_id":"wrap.near","price":null},
        {"asset_id":"17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1","price":null}
    ]}"#;

    fn assets() -> BTreeMap<String, AssetView> {
        BTreeMap::from([
            ("wrap.near".to_owned(), serde_json::from_str(WNEAR_ASSET).unwrap()),
            (USDC_FEED.to_owned(), serde_json::from_str(USDC_ASSET).unwrap()),
        ])
    }

    fn live_data(now_ns: u64) -> PriceData {
        PriceData {
            timestamp: U64(now_ns),
            recency_duration_sec: 600,
            prices: vec![
                AssetOptionalPrice {
                    asset_id: "wrap.near".into(),
                    price: Some(OraclePrice { multiplier: U128(450_000_000), decimals: 8 }),
                },
                AssetOptionalPrice {
                    asset_id: USDC_FEED.into(),
                    price: Some(OraclePrice { multiplier: U128(100_000_000), decimals: 8 }),
                },
            ],
        }
    }

    #[test]
    fn real_mainnet_answers_parse() {
        let a: AssetView = serde_json::from_str(WNEAR_ASSET).unwrap();
        assert_eq!(latest_report_sec(&a), Some(1_782_588_756));
        let d: PriceData = serde_json::from_str(PRICE_DATA_NULL).unwrap();
        assert!(d.prices.iter().all(|p| p.price.is_none()));
    }

    #[test]
    fn converts_to_the_engines_price() {
        let p = to_price(&OraclePrice { multiplier: U128(450_000_000), decimals: 8 }, 7).unwrap();
        assert_eq!(p, Price { mantissa: U64(450_000_000), expo: -8, publish_time_sec: 7 });
    }

    #[test]
    fn unusable_prices_are_refused() {
        let at = |m, d| to_price(&OraclePrice { multiplier: U128(m), decimals: d }, 0);
        assert_eq!(at(0, 8), Err(OracleError::ZeroMultiplier));
        assert_eq!(at(u128::from(u64::MAX) + 1, 8), Err(OracleError::MultiplierTooLarge));
        assert_eq!(at(1, 31), Err(OracleError::DecimalsOutOfRange(31)));
        assert!(at(1, 30).is_ok());
    }

    #[test]
    fn null_oracle_answer_gives_no_prices() {
        let data: PriceData = serde_json::from_str(PRICE_DATA_NULL).unwrap();
        assert!(prices_for(&sample(), &data, &assets()).is_empty());
    }

    #[test]
    fn asset_without_reports_gets_no_price() {
        let mut a = assets();
        a.insert("wrap.near".into(), AssetView { reports: vec![] });
        let prices = prices_for(&sample(), &live_data(1_793_697_676_000_000_000), &a);
        assert!(!prices.contains_key(WNEAR));
        assert!(prices.contains_key(USDC));
    }

    #[test]
    fn publish_time_comes_from_the_newest_report() {
        let prices = prices_for(&sample(), &live_data(1_793_697_676_000_000_000), &assets());
        assert_eq!(prices[WNEAR].publish_time_sec, 1_782_588_756);
        assert_eq!(prices[USDC].publish_time_sec, 1_782_588_756);
    }

    fn market(prices: BTreeMap<String, Price>, now_sec: u64) -> Market {
        Market {
            now_sec,
            prices,
            balances: BTreeMap::from([(USDC.to_owned(), 20_000_000)]),
        }
    }

    fn buy_near() -> Trade {
        Trade {
            asset_in: USDC.into(),
            asset_out: WNEAR.into(),
            amount_in: 20_000_000,
            min_amount_out: 4_400_000_000_000_000_000_000_000,
        }
    }

    /// The oracle's real state today: the newest reports are months old, so
    /// even a non-null aggregate is refused by the mandate's own age limit.
    #[test]
    fn months_old_reports_are_rejected_as_stale() {
        let now_ns = 1_793_697_676_000_000_000;
        let prices = prices_for(&sample(), &live_data(now_ns), &assets());
        let err = check_trade(
            &sample(),
            &TradingState::default(),
            &buy_near(),
            &market(prices, now_ns / 1_000_000_000),
        )
        .unwrap_err();
        assert_eq!(err.code(), "StalePrice");
    }

    #[test]
    fn null_oracle_makes_the_vault_fail_closed() {
        let data: PriceData = serde_json::from_str(PRICE_DATA_NULL).unwrap();
        let prices = prices_for(&sample(), &data, &assets());
        let err = check_trade(
            &sample(),
            &TradingState::default(),
            &buy_near(),
            &market(prices, 1_793_697_676),
        )
        .unwrap_err();
        assert!(matches!(err, TradeError::MissingPrice(_)), "got {err:?}");
    }

    #[test]
    fn fresh_reports_pass_the_gates() {
        let now = 1_793_697_676u64;
        let fresh = |ts_sec: u64, m: &str| -> AssetView {
            serde_json::from_str(&format!(
                r#"{{"reports":[{{"oracle_id":"x","timestamp":"{}","price":{{"multiplier":"{m}","decimals":8}}}}]}}"#,
                ts_sec * 1_000_000_000
            ))
            .unwrap()
        };
        let a = BTreeMap::from([
            ("wrap.near".to_owned(), fresh(now - 10, "450000000")),
            (USDC_FEED.to_owned(), fresh(now - 10, "100000000")),
        ]);
        let prices = prices_for(&sample(), &live_data(now * 1_000_000_000), &a);
        // Spend 2 USDC ($2) for 0.45 wNEAR ($2.025 at $4.50), keeping about
        // 98 USDC so the received position stays well under max_position_bps.
        let trade = Trade {
            asset_in: USDC.into(),
            asset_out: WNEAR.into(),
            amount_in: 2_000_000,
            min_amount_out: 450_000_000_000_000_000_000_000,
        };
        let held = Market {
            now_sec: now,
            prices,
            balances: BTreeMap::from([(USDC.to_owned(), 100_000_000)]),
        };
        let ok = check_trade(&sample(), &TradingState::default(), &trade, &held);
        assert!(ok.is_ok(), "got {ok:?}");
    }
}
