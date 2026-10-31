//! Owner-defined trading mandate.
//!
//! This module is pure: no `env` calls, no I/O. All rules the vault enforces
//! live here so they can be tested exhaustively off-chain.

use std::collections::BTreeSet;
use std::fmt;

use near_sdk::json_types::U64;
use near_sdk::near;

/// Basis points in 100%.
pub const BPS_DENOMINATOR: u16 = 10_000;
/// Hard ceiling on tolerated slippage, whatever the owner configures.
pub const MAX_SLIPPAGE_BPS_CAP: u16 = 500;
/// Hard ceiling on accepted oracle price age.
pub const MAX_PRICE_AGE_SEC_CAP: u32 = 300;
/// Bounds per-trade loops over the universe and the mandate's storage cost.
pub const MAX_ASSETS: usize = 32;

/// USD amounts are integer micro-dollars (1 USD = 1_000_000).
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mandate {
    /// NEAR Intents asset ids the vault may hold or trade, e.g. `nep141:wrap.near`.
    pub allowed_assets: Vec<String>,
    /// Subset of `allowed_assets` treated as cash when de-risking.
    pub stable_assets: Vec<String>,
    pub max_trade_usd: U64,
    pub max_daily_turnover_usd: U64,
    /// Max weight of any single non-stable asset after a trade.
    pub max_position_bps: u16,
    /// Min-out must be within this distance of the oracle value.
    pub max_slippage_bps: u16,
    /// NAV drop from high-water mark that trips the circuit breaker.
    pub max_drawdown_bps: u16,
    pub min_trade_interval_sec: u32,
    pub max_price_age_sec: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MandateError {
    NoAssets,
    TooManyAssets,
    InvalidAssetId(String),
    DuplicateAsset(String),
    NoStableAsset,
    StableNotAllowed(String),
    ZeroTradeLimit,
    TradeLimitAboveDailyTurnover,
    BpsOutOfRange(&'static str),
    SlippageAboveCap,
    PriceAgeOutOfRange,
}

impl fmt::Display for MandateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAssets => write!(f, "mandate must allow at least one asset"),
            Self::TooManyAssets => write!(f, "mandate may allow at most {MAX_ASSETS} assets"),
            Self::InvalidAssetId(id) => write!(f, "invalid asset id: {id}"),
            Self::DuplicateAsset(id) => write!(f, "duplicate asset id: {id}"),
            Self::NoStableAsset => write!(f, "mandate must name at least one stable asset"),
            Self::StableNotAllowed(id) => write!(f, "stable asset not in allowed_assets: {id}"),
            Self::ZeroTradeLimit => write!(f, "max_trade_usd must be greater than zero"),
            Self::TradeLimitAboveDailyTurnover => {
                write!(f, "max_trade_usd must not exceed max_daily_turnover_usd")
            }
            Self::BpsOutOfRange(field) => write!(f, "{field} must be in 1..=10000"),
            Self::SlippageAboveCap => {
                write!(f, "max_slippage_bps must not exceed {MAX_SLIPPAGE_BPS_CAP}")
            }
            Self::PriceAgeOutOfRange => {
                write!(f, "max_price_age_sec must be in 1..={MAX_PRICE_AGE_SEC_CAP}")
            }
        }
    }
}

impl Mandate {
    /// Rejects configurations that are internally inconsistent or unsafe.
    pub fn validate(&self) -> Result<(), MandateError> {
        if self.allowed_assets.is_empty() {
            return Err(MandateError::NoAssets);
        }
        // stable_assets must be a subset, so this bounds both lists.
        if self.allowed_assets.len() > MAX_ASSETS {
            return Err(MandateError::TooManyAssets);
        }
        let mut seen = BTreeSet::new();
        for id in &self.allowed_assets {
            if !is_valid_asset_id(id) {
                return Err(MandateError::InvalidAssetId(id.clone()));
            }
            if !seen.insert(id.as_str()) {
                return Err(MandateError::DuplicateAsset(id.clone()));
            }
        }

        if self.stable_assets.is_empty() {
            return Err(MandateError::NoStableAsset);
        }
        let mut seen_stable = BTreeSet::new();
        for id in &self.stable_assets {
            if !seen.contains(id.as_str()) {
                return Err(MandateError::StableNotAllowed(id.clone()));
            }
            if !seen_stable.insert(id.as_str()) {
                return Err(MandateError::DuplicateAsset(id.clone()));
            }
        }

        if self.max_trade_usd.0 == 0 {
            return Err(MandateError::ZeroTradeLimit);
        }
        if self.max_trade_usd.0 > self.max_daily_turnover_usd.0 {
            return Err(MandateError::TradeLimitAboveDailyTurnover);
        }

        check_bps(self.max_position_bps, "max_position_bps")?;
        check_bps(self.max_slippage_bps, "max_slippage_bps")?;
        check_bps(self.max_drawdown_bps, "max_drawdown_bps")?;
        if self.max_slippage_bps > MAX_SLIPPAGE_BPS_CAP {
            return Err(MandateError::SlippageAboveCap);
        }

        if self.max_price_age_sec == 0 || self.max_price_age_sec > MAX_PRICE_AGE_SEC_CAP {
            return Err(MandateError::PriceAgeOutOfRange);
        }
        Ok(())
    }
}

fn check_bps(value: u16, field: &'static str) -> Result<(), MandateError> {
    if value == 0 || value > BPS_DENOMINATOR {
        return Err(MandateError::BpsOutOfRange(field));
    }
    Ok(())
}

/// Accepts `nep141:<account>` and `nep245:<account>:<token>` ids as used by
/// the NEAR Intents verifier. Account part: lowercase `[a-z0-9._-]`.
fn is_valid_asset_id(id: &str) -> bool {
    let Some((standard, rest)) = id.split_once(':') else {
        return false;
    };
    let account = match standard {
        "nep141" => rest,
        "nep245" => match rest.split_once(':') {
            Some((account, token)) if !token.is_empty() => account,
            _ => return false,
        },
        _ => return false,
    };
    (2..=64).contains(&account.len())
        && account
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample() -> Mandate {
        Mandate {
            allowed_assets: vec![
                "nep141:wrap.near".into(),
                "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1".into(),
            ],
            stable_assets: vec![
                "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1".into(),
            ],
            max_trade_usd: U64(50_000_000),
            max_daily_turnover_usd: U64(200_000_000),
            max_position_bps: 6_000,
            max_slippage_bps: 100,
            max_drawdown_bps: 1_500,
            min_trade_interval_sec: 300,
            max_price_age_sec: 60,
        }
    }

    #[test]
    fn sample_is_valid() {
        assert_eq!(sample().validate(), Ok(()));
    }

    #[test]
    fn rejects_empty_assets() {
        let m = Mandate { allowed_assets: vec![], ..sample() };
        assert_eq!(m.validate(), Err(MandateError::NoAssets));
    }

    #[test]
    fn caps_asset_count() {
        let mut m = sample();
        let at_cap = MAX_ASSETS - m.allowed_assets.len();
        m.allowed_assets.extend((0..at_cap).map(|i| format!("nep141:token{i}.near")));
        assert_eq!(m.validate(), Ok(()));

        m.allowed_assets.push("nep141:one-too-many.near".into());
        assert_eq!(m.validate(), Err(MandateError::TooManyAssets));
    }

    #[test]
    fn rejects_bad_asset_ids() {
        for bad in ["wrap.near", "nep141:", "nep141:Wrap.near", "erc20:0xabc", "nep245:mt.near"] {
            let mut m = sample();
            m.allowed_assets.push(bad.into());
            assert_eq!(m.validate(), Err(MandateError::InvalidAssetId(bad.into())), "{bad}");
        }
    }

    #[test]
    fn accepts_nep245_ids() {
        let mut m = sample();
        m.allowed_assets.push("nep245:v2_1.omni.hot.tg:56_11111111111111111111".into());
        assert_eq!(m.validate(), Ok(()));
    }

    #[test]
    fn rejects_duplicates() {
        let mut m = sample();
        m.allowed_assets.push("nep141:wrap.near".into());
        assert_eq!(m.validate(), Err(MandateError::DuplicateAsset("nep141:wrap.near".into())));
    }

    #[test]
    fn stable_must_be_allowed() {
        let m = Mandate { stable_assets: vec!["nep141:usdt.tether-token.near".into()], ..sample() };
        assert_eq!(
            m.validate(),
            Err(MandateError::StableNotAllowed("nep141:usdt.tether-token.near".into()))
        );
    }

    #[test]
    fn requires_a_stable_asset() {
        let m = Mandate { stable_assets: vec![], ..sample() };
        assert_eq!(m.validate(), Err(MandateError::NoStableAsset));
    }

    #[test]
    fn trade_limits_are_consistent() {
        let zero = Mandate { max_trade_usd: U64(0), ..sample() };
        assert_eq!(zero.validate(), Err(MandateError::ZeroTradeLimit));

        let above = Mandate { max_trade_usd: U64(300_000_000), ..sample() };
        assert_eq!(above.validate(), Err(MandateError::TradeLimitAboveDailyTurnover));
    }

    #[test]
    fn bps_bounds() {
        let zero = Mandate { max_position_bps: 0, ..sample() };
        assert_eq!(zero.validate(), Err(MandateError::BpsOutOfRange("max_position_bps")));

        let over = Mandate { max_drawdown_bps: 10_001, ..sample() };
        assert_eq!(over.validate(), Err(MandateError::BpsOutOfRange("max_drawdown_bps")));

        let slippage = Mandate { max_slippage_bps: MAX_SLIPPAGE_BPS_CAP + 1, ..sample() };
        assert_eq!(slippage.validate(), Err(MandateError::SlippageAboveCap));
    }

    #[test]
    fn price_age_bounds() {
        for age in [0, MAX_PRICE_AGE_SEC_CAP + 1] {
            let m = Mandate { max_price_age_sec: age, ..sample() };
            assert_eq!(m.validate(), Err(MandateError::PriceAgeOutOfRange), "{age}");
        }
    }
}
