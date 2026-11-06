//! Pyth NEAR `list_prices_no_older_than` adapter. A cache entry is not proof
//! that a live updater exists or that its market price is correct.

use std::collections::BTreeMap;

use near_sdk::json_types::{I64, U64};
use near_sdk::near;

use crate::mandate::Mandate;
use crate::math::{MIN_PRICE_EXPO, Price};

/// Exact JSON shape used by the Pyth NEAR receiver: signed price is a string,
/// confidence is a string, exponent and publication time are JSON numbers.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythPrice {
    pub price: I64,
    pub conf: U64,
    pub expo: i32,
    pub publish_time: i64,
}

pub type PythPrices = BTreeMap<String, Option<PythPrice>>;

pub fn valid_feed_id(feed: &str) -> bool {
    feed.len() == 64
        && feed.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PythError {
    NonPositivePrice,
    NegativePublishTime,
    ExponentOutOfRange,
    ConfidenceNotBelowPrice,
    ConfidenceExceedsSlippage,
}

/// Confidence must fit the mandate's existing slippage budget. This is an
/// additional provider guard; it is not an independent market-price oracle.
pub fn to_price(value: &PythPrice, max_slippage_bps: u16) -> Result<Price, PythError> {
    let mantissa = u64::try_from(value.price.0)
        .ok()
        .filter(|price| *price > 0)
        .ok_or(PythError::NonPositivePrice)?;
    let publish_time_sec =
        u64::try_from(value.publish_time).map_err(|_| PythError::NegativePublishTime)?;
    if !(MIN_PRICE_EXPO..=0).contains(&value.expo) {
        return Err(PythError::ExponentOutOfRange);
    }
    if value.conf.0 >= mantissa {
        return Err(PythError::ConfidenceNotBelowPrice);
    }
    // u64 × 10_000 and u64 × u16 are both bounded well below u128::MAX.
    if u128::from(value.conf.0) * 10_000 > u128::from(mantissa) * u128::from(max_slippage_bps) {
        return Err(PythError::ConfidenceExceedsSlippage);
    }
    Ok(Price { mantissa: U64(mantissa), expo: value.expo, publish_time_sec })
}

/// Missing, malformed-value and excessive-confidence entries are omitted.
/// The actual engine then rejects any held/traded asset without a price.
pub fn prices_for(mandate: &Mandate, prices: &PythPrices) -> BTreeMap<String, Price> {
    mandate
        .assets
        .iter()
        .filter_map(|asset| {
            if !valid_feed_id(&asset.price_feed_id) {
                return None;
            }
            let value = prices.get(&asset.price_feed_id)?.as_ref()?;
            let price = to_price(value, mandate.max_slippage_bps).ok()?;
            Some((asset.asset_id.clone(), price))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use near_sdk::serde_json::{self, json};

    use super::*;
    use crate::engine::{Market, Trade, TradeError, TradingState, check_trade};
    use crate::mandate::tests::{USDC, WNEAR, sample};

    const NOW: u64 = 1_793_697_676;

    fn quote(price: i64, conf: u64, expo: i32, published: i64) -> PythPrice {
        PythPrice { price: I64(price), conf: U64(conf), expo, publish_time: published }
    }

    pub(super) fn mandate() -> Mandate {
        let mut mandate = sample();
        mandate.assets[0].price_feed_id = "a".repeat(64);
        mandate.assets[1].price_feed_id = "b".repeat(64);
        mandate
    }

    fn prices(published: i64) -> PythPrices {
        BTreeMap::from([
            ("a".repeat(64), Some(quote(500_000_000, 100_000, -8, published))),
            ("b".repeat(64), Some(quote(100_000_000, 10_000, -8, published))),
        ])
    }

    fn check(prices: &PythPrices) -> Result<crate::engine::Approval, TradeError> {
        let mandate = mandate();
        check_trade(
            &mandate,
            &TradingState::default(),
            &Trade {
                asset_in: USDC.into(),
                asset_out: WNEAR.into(),
                amount_in: 20_000_000,
                min_amount_out: 4 * 10_u128.pow(24),
            },
            &Market {
                now_sec: NOW,
                prices: prices_for(&mandate, prices),
                balances: BTreeMap::from([
                    (USDC.into(), 100_000_000),
                    (WNEAR.into(), 10 * 10_u128.pow(24)),
                ]),
            },
        )
    }

    #[test]
    fn pyth_price_uses_the_exact_receiver_json_shape() {
        let value = quote(500_000_000, 100_000, -8, NOW as i64);
        let serialized = serde_json::to_value(&value).unwrap();
        assert_eq!(
            serialized,
            json!({"price":"500000000","conf":"100000","expo":-8,"publish_time":NOW})
        );
        assert_eq!(serde_json::from_value::<PythPrice>(serialized).unwrap(), value);
        let answer: PythPrices = serde_json::from_value(json!({
            "a".repeat(64): {"price":"500000000","conf":"100000","expo":-8,"publish_time":NOW},
            "b".repeat(64): null,
        }))
        .unwrap();
        assert!(answer[&"a".repeat(64)].is_some());
        assert!(answer[&"b".repeat(64)].is_none());
    }

    #[test]
    fn pyth_receiver_fields_do_not_accept_lossy_or_wrong_json_types() {
        for (field, wrong) in [
            ("price", json!(100_000_000)),
            ("price", json!("9223372036854775808")),
            ("conf", json!(1)),
            ("conf", json!("-1")),
            ("conf", json!("18446744073709551616")),
            ("expo", json!("-8")),
            ("publish_time", json!("1793697676")),
        ] {
            let mut value = serde_json::to_value(quote(100_000_000, 1, -8, NOW as i64)).unwrap();
            value[field] = wrong;
            assert!(
                serde_json::from_value::<PythPrice>(value).is_err(),
                "wrong {field} type/range"
            );
        }
    }

    #[test]
    fn pyth_signed_fields_and_exponents_fail_closed() {
        for price in [i64::MIN, -1, 0] {
            assert_eq!(to_price(&quote(price, 0, -8, 1), 100), Err(PythError::NonPositivePrice));
        }
        assert_eq!(to_price(&quote(100, 0, -8, -1), 100), Err(PythError::NegativePublishTime));
        for expo in [i32::MIN, -31, 1, i32::MAX] {
            assert_eq!(to_price(&quote(100, 0, expo, 1), 100), Err(PythError::ExponentOutOfRange));
        }
        for expo in [-30, 0] {
            assert!(to_price(&quote(i64::MAX, 0, expo, i64::MAX), 0).is_ok());
        }
    }

    #[test]
    fn pyth_confidence_limit_accepts_equality_and_refuses_one_unit_above() {
        assert!(to_price(&quote(10_000, 100, -8, 1), 100).is_ok());
        assert_eq!(
            to_price(&quote(10_000, 101, -8, 1), 100),
            Err(PythError::ConfidenceExceedsSlippage)
        );
        assert!(to_price(&quote(100, 0, -8, 1), 0).is_ok());
        assert_eq!(to_price(&quote(100, 1, -8, 1), 0), Err(PythError::ConfidenceExceedsSlippage));
        for conf in [100, u64::MAX] {
            assert_eq!(
                to_price(&quote(100, conf, -8, 1), 10_000),
                Err(PythError::ConfidenceNotBelowPrice)
            );
        }
        assert_eq!(
            to_price(&quote(i64::MAX, i64::MAX as u64 - 1, 0, 0), 500),
            Err(PythError::ConfidenceExceedsSlippage)
        );
    }

    #[test]
    fn pyth_feed_ids_are_exact_lowercase_hex() {
        assert!(valid_feed_id(&"a".repeat(64)));
        assert!(valid_feed_id(&"0".repeat(64)));
        for feed in [
            "a".repeat(63),
            "a".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
            format!("0x{}", "a".repeat(64)),
            "é".repeat(32),
        ] {
            assert!(!valid_feed_id(&feed));
        }
    }

    #[test]
    fn actual_engine_enforces_pyth_age_and_future_boundaries() {
        assert!(check(&prices(NOW as i64 - 60)).is_ok());
        assert!(matches!(check(&prices(NOW as i64 - 61)), Err(TradeError::StalePrice { .. })));
        assert!(check(&prices(NOW as i64 + 5)).is_ok());
        assert!(matches!(check(&prices(NOW as i64 + 6)), Err(TradeError::FuturePrice { .. })));
    }

    #[test]
    fn missing_or_invalid_pyth_entry_reaches_the_actual_missing_price_gate() {
        let mut answer = prices(NOW as i64);
        answer.insert("a".repeat(64), None);
        assert!(matches!(check(&answer), Err(TradeError::MissingPrice { .. })));
        answer.insert("a".repeat(64), Some(quote(-1, 0, -8, NOW as i64)));
        assert!(matches!(check(&answer), Err(TradeError::MissingPrice { .. })));
        answer.insert("a".repeat(64), Some(quote(500_000_000, 5_000_001, -8, NOW as i64)));
        assert!(matches!(check(&answer), Err(TradeError::MissingPrice { .. })));
    }
}
