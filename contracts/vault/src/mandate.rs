//! Owner-defined trading mandate.
//!
//! This module is pure: no `env` calls, no I/O. It defines the rules and
//! validates that a configuration is consistent; `engine` applies them to
//! individual trades.

use std::collections::BTreeSet;
use std::fmt;

use near_sdk::json_types::{U64, U128};
use near_sdk::{AccountId, near};

/// Basis points in 100%.
pub const BPS_DENOMINATOR: u16 = 10_000;
/// Hard ceiling on tolerated slippage, whatever the owner configures.
pub const MAX_SLIPPAGE_BPS_CAP: u16 = 500;
/// Hard ceiling on accepted oracle price age.
pub const MAX_PRICE_AGE_SEC_CAP: u32 = 300;
/// Bounds per-trade loops over the universe and the mandate's storage cost. It also bounds the
/// execute chain's gas: one price feed per asset, each reserved at 8 TGas. 22 assets passed on
/// the sandbox and 24 failed, so 20 leaves a margin. See docs/evidence/gas-limit.txt (SECURITY SR-3).
pub const MAX_ASSETS: usize = 20;
/// Token decimals above this are rejected; keeps USD math inside 256 bits.
pub const MAX_DECIMALS: u8 = 30;
/// Bounds the owner-defined declared decision-source allowlist.
pub const MAX_MODEL_IDS: usize = 8;
/// Declared source identifiers are bounded by UTF-8 byte length.
pub const MAX_MODEL_ID_BYTES: usize = 64;
const MAX_PRICE_FEED_ID_LEN: usize = 128;

/// One tradable asset.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetConfig {
    /// NEAR Intents asset id, e.g. `nep141:wrap.near`.
    pub asset_id: String,
    /// Token decimals: `amount` base units = `amount / 10^decimals` tokens.
    pub decimals: u8,
    /// Identifier of this asset's USD price feed on the oracle.
    pub price_feed_id: String,
}

/// Maximum token-base-unit amount authorized by one withdrawal request.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithdrawalLimit {
    pub asset_id: String,
    pub max_per_request: U128,
}

/// USD amounts are integer micro-dollars (1 USD = 1_000_000).
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mandate {
    /// Assets the vault may hold or trade.
    pub assets: Vec<AssetConfig>,
    /// Subset of `assets` (by id) treated as cash when de-risking.
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
    /// Declared decision sources allowed to propose trades. Empty denies all.
    /// Membership does not prove that a model generated the decision.
    pub allowed_model_ids: Vec<String>,
    /// Per-request ceilings. Assets without an entry retain legacy uncapped withdrawals.
    #[serde(default)]
    pub withdrawal_limits: Vec<WithdrawalLimit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MandateError {
    NoAssets,
    TooManyAssets,
    InvalidAssetId(String),
    DuplicateAsset(String),
    InvalidDecimals(String),
    InvalidPriceFeedId(String),
    NoStableAsset,
    StableNotAllowed(String),
    ZeroTradeLimit,
    TradeLimitAboveDailyTurnover,
    BpsOutOfRange(&'static str),
    SlippageAboveCap,
    PriceAgeOutOfRange,
    TooManyModelIds,
    InvalidModelId(String),
    DuplicateModelId(String),
    TooManyWithdrawalLimits,
    WithdrawalAssetNotAllowed(String),
    DuplicateWithdrawalLimit(String),
    ZeroWithdrawalLimit(String),
}

impl fmt::Display for MandateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAssets => write!(f, "mandate must allow at least one asset"),
            Self::TooManyAssets => write!(f, "mandate may allow at most {MAX_ASSETS} assets"),
            Self::InvalidAssetId(id) => write!(f, "invalid asset id: {id}"),
            Self::DuplicateAsset(id) => write!(f, "duplicate asset id: {id}"),
            Self::InvalidDecimals(id) => {
                write!(f, "decimals of {id} must not exceed {MAX_DECIMALS}")
            }
            Self::InvalidPriceFeedId(id) => write!(f, "invalid price_feed_id for {id}"),
            Self::NoStableAsset => write!(f, "mandate must name at least one stable asset"),
            Self::StableNotAllowed(id) => write!(f, "stable asset not in assets: {id}"),
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
            Self::TooManyModelIds => {
                write!(f, "mandate may allow at most {MAX_MODEL_IDS} model ids")
            }
            Self::InvalidModelId(id) => {
                write!(
                    f,
                    "model id must be nonblank and at most {MAX_MODEL_ID_BYTES} UTF-8 bytes: {id}"
                )
            }
            Self::DuplicateModelId(id) => write!(f, "duplicate model id: {id}"),
            Self::TooManyWithdrawalLimits => {
                write!(f, "mandate may have at most {MAX_ASSETS} withdrawal ceilings")
            }
            Self::WithdrawalAssetNotAllowed(id) => {
                write!(f, "withdrawal ceiling asset not in assets: {id}")
            }
            Self::DuplicateWithdrawalLimit(id) => write!(f, "duplicate withdrawal ceiling: {id}"),
            Self::ZeroWithdrawalLimit(id) => {
                write!(f, "withdrawal ceiling of {id} must be greater than zero")
            }
        }
    }
}

impl Mandate {
    /// Rejects configurations that are internally inconsistent or unsafe.
    pub fn validate(&self) -> Result<(), MandateError> {
        if self.assets.is_empty() {
            return Err(MandateError::NoAssets);
        }
        // stable_assets must be a subset, so this bounds both lists.
        if self.assets.len() > MAX_ASSETS {
            return Err(MandateError::TooManyAssets);
        }
        let mut seen = BTreeSet::new();
        for asset in &self.assets {
            let id = &asset.asset_id;
            if !is_valid_asset_id(id) {
                return Err(MandateError::InvalidAssetId(id.clone()));
            }
            if !seen.insert(id.as_str()) {
                return Err(MandateError::DuplicateAsset(id.clone()));
            }
            if asset.decimals > MAX_DECIMALS {
                return Err(MandateError::InvalidDecimals(id.clone()));
            }
            if !is_valid_price_feed_id(&asset.price_feed_id) {
                return Err(MandateError::InvalidPriceFeedId(id.clone()));
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

        if self.allowed_model_ids.len() > MAX_MODEL_IDS {
            return Err(MandateError::TooManyModelIds);
        }
        let mut seen_model_ids = BTreeSet::new();
        for id in &self.allowed_model_ids {
            if id.trim().is_empty() || id.len() > MAX_MODEL_ID_BYTES {
                return Err(MandateError::InvalidModelId(id.clone()));
            }
            if !seen_model_ids.insert(id.as_str()) {
                return Err(MandateError::DuplicateModelId(id.clone()));
            }
        }

        if self.withdrawal_limits.len() > MAX_ASSETS {
            return Err(MandateError::TooManyWithdrawalLimits);
        }
        let mut seen_limits = BTreeSet::new();
        for limit in &self.withdrawal_limits {
            if !seen.contains(limit.asset_id.as_str()) {
                return Err(MandateError::WithdrawalAssetNotAllowed(limit.asset_id.clone()));
            }
            if !seen_limits.insert(limit.asset_id.as_str()) {
                return Err(MandateError::DuplicateWithdrawalLimit(limit.asset_id.clone()));
            }
            if limit.max_per_request.0 == 0 {
                return Err(MandateError::ZeroWithdrawalLimit(limit.asset_id.clone()));
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

    pub fn asset(&self, asset_id: &str) -> Option<&AssetConfig> {
        self.assets.iter().find(|a| a.asset_id == asset_id)
    }

    pub fn is_stable(&self, asset_id: &str) -> bool {
        self.stable_assets.iter().any(|s| s == asset_id)
    }

    pub fn allows_model_id(&self, model_id: &str) -> bool {
        self.allowed_model_ids.iter().any(|id| id == model_id)
    }

    /// None preserves the uncapped authorization used by older policies.
    pub fn withdrawal_limit(&self, asset_id: &str) -> Option<u128> {
        self.withdrawal_limits
            .iter()
            .find(|limit| limit.asset_id == asset_id)
            .map(|limit| limit.max_per_request.0)
    }
}

fn check_bps(value: u16, field: &'static str) -> Result<(), MandateError> {
    if value == 0 || value > BPS_DENOMINATOR {
        return Err(MandateError::BpsOutOfRange(field));
    }
    Ok(())
}

/// Only the bounded NEP141 surface has a supported owner withdrawal path.
pub(crate) fn is_valid_asset_id(id: &str) -> bool {
    let Some(account) = id.strip_prefix("nep141:") else {
        return false;
    };
    id.len() <= 71 && account.parse::<AccountId>().is_ok()
}

/// Opaque oracle feed id: 1..=128 chars of `[A-Za-z0-9._:-]`.
fn is_valid_price_feed_id(id: &str) -> bool {
    (1..=MAX_PRICE_FEED_ID_LEN).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
}

/// The limits that `new` loosens compared with `old`, by field name.
///
/// A loosened field lets the vault do something it could not do before:
/// a higher cap, a shorter cooldown, a longer price age, an asset that was not
/// listed, a changed asset config, a new stable asset, or a new decision source. Anything else is a
/// tightening (or no change). The owner applies a tightening at once; a
/// loosening waits (see `MANDATE_DELAY_SEC`). Unknown changes count as loosening.
pub fn loosened_fields(old: &Mandate, new: &Mandate) -> Vec<&'static str> {
    let mut out = Vec::new();
    if new.max_trade_usd.0 > old.max_trade_usd.0 {
        out.push("max_trade_usd");
    }
    if new.max_daily_turnover_usd.0 > old.max_daily_turnover_usd.0 {
        out.push("max_daily_turnover_usd");
    }
    if new.max_position_bps > old.max_position_bps {
        out.push("max_position_bps");
    }
    if new.max_slippage_bps > old.max_slippage_bps {
        out.push("max_slippage_bps");
    }
    if new.max_drawdown_bps > old.max_drawdown_bps {
        out.push("max_drawdown_bps");
    }
    if new.min_trade_interval_sec < old.min_trade_interval_sec {
        out.push("min_trade_interval_sec");
    }
    if new.max_price_age_sec > old.max_price_age_sec {
        out.push("max_price_age_sec");
    }
    let listed = |id: &str| old.assets.iter().find(|a| a.asset_id == id);
    let asset_loosened = new.assets.iter().any(|a| listed(&a.asset_id) != Some(a));
    if asset_loosened {
        out.push("assets");
    }
    if new.stable_assets.iter().any(|id| !old.stable_assets.contains(id)) {
        out.push("stable_assets");
    }
    if new.allowed_model_ids.iter().any(|id| !old.allowed_model_ids.contains(id)) {
        out.push("allowed_model_ids");
    }
    // Removing a listed asset must not hide the loosening of its old ceiling.
    if old.withdrawal_limits.iter().any(|old_limit| {
        new.withdrawal_limit(&old_limit.asset_id)
            .is_none_or(|new_limit| new_limit > old_limit.max_per_request.0)
    }) {
        out.push("withdrawal_limits");
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const WNEAR: &str = "nep141:wrap.near";
    pub(crate) const USDC: &str =
        "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";

    pub(crate) fn asset(asset_id: &str, decimals: u8) -> AssetConfig {
        AssetConfig {
            asset_id: asset_id.into(),
            decimals,
            price_feed_id: asset_id.trim_start_matches("nep141:").into(),
        }
    }

    pub(crate) fn sample() -> Mandate {
        Mandate {
            assets: vec![asset(WNEAR, 24), asset(USDC, 6)],
            stable_assets: vec![USDC.into()],
            max_trade_usd: U64(50_000_000),
            max_daily_turnover_usd: U64(200_000_000),
            max_position_bps: 6_000,
            max_slippage_bps: 100,
            max_drawdown_bps: 1_500,
            min_trade_interval_sec: 300,
            max_price_age_sec: 60,
            allowed_model_ids: vec![
                "manual-v1".into(),
                "rebalance-v1".into(),
                "fixture-model".into(),
            ],
            withdrawal_limits: vec![],
        }
    }

    fn capped(amount: u128) -> Mandate {
        Mandate {
            withdrawal_limits: vec![WithdrawalLimit {
                asset_id: USDC.into(),
                max_per_request: U128(amount),
            }],
            ..sample()
        }
    }

    #[test]
    fn withdrawal_ceiling_schema_is_a_positive_unique_allowed_subset() {
        assert!(sample().validate().is_ok());
        assert!(capped(u128::MAX).validate().is_ok());
        assert_eq!(capped(0).validate(), Err(MandateError::ZeroWithdrawalLimit(USDC.into())));
        let mut duplicate = capped(1);
        duplicate.withdrawal_limits.push(duplicate.withdrawal_limits[0].clone());
        assert_eq!(duplicate.validate(), Err(MandateError::DuplicateWithdrawalLimit(USDC.into())));
        let mut unknown = capped(1);
        unknown.withdrawal_limits[0].asset_id = "nep141:unknown.near".into();
        assert_eq!(
            unknown.validate(),
            Err(MandateError::WithdrawalAssetNotAllowed("nep141:unknown.near".into()))
        );
        let mut oversized = capped(1);
        oversized.withdrawal_limits = vec![oversized.withdrawal_limits[0].clone(); MAX_ASSETS + 1];
        assert_eq!(oversized.validate(), Err(MandateError::TooManyWithdrawalLimits));
    }

    #[test]
    fn withdrawal_ceiling_lookup_and_legacy_json_preserve_uncapped_assets() {
        let mandate = capped(17);
        assert_eq!(mandate.withdrawal_limit(USDC), Some(17));
        assert_eq!(mandate.withdrawal_limit(WNEAR), None);
        let mut legacy_json = near_sdk::serde_json::to_value(sample()).unwrap();
        legacy_json.as_object_mut().unwrap().remove("withdrawal_limits");
        let restored: Mandate = near_sdk::serde_json::from_value(legacy_json).unwrap();
        assert_eq!(restored, sample());
        let json = near_sdk::serde_json::to_value(mandate).unwrap();
        assert_eq!(json["withdrawal_limits"][0]["max_per_request"], "17");
    }

    #[test]
    fn withdrawal_ceiling_increases_and_removal_are_loosening() {
        let old = capped(20);
        assert_eq!(loosened_fields(&sample(), &old), Vec::<&str>::new());
        assert!(loosened_fields(&old, &capped(20)).is_empty());
        assert!(loosened_fields(&old, &capped(19)).is_empty());
        assert_eq!(loosened_fields(&old, &capped(21)), ["withdrawal_limits"]);
        assert_eq!(loosened_fields(&old, &sample()), ["withdrawal_limits"]);
        let mut remove_asset = old.clone();
        remove_asset.withdrawal_limits.clear();
        remove_asset.assets.retain(|asset| asset.asset_id != USDC);
        remove_asset.stable_assets = vec![WNEAR.into()];
        assert!(remove_asset.validate().is_ok());
        assert!(loosened_fields(&old, &remove_asset).contains(&"withdrawal_limits"));
    }

    #[test]
    fn loosened_fields_names_each_loosened_limit() {
        let old = sample();
        assert!(loosened_fields(&old, &old).is_empty());
        assert_eq!(
            loosened_fields(&old, &Mandate { max_trade_usd: U64(60_000_000), ..sample() }),
            vec!["max_trade_usd"]
        );
        assert_eq!(
            loosened_fields(&old, &Mandate { min_trade_interval_sec: 60, ..sample() }),
            vec!["min_trade_interval_sec"]
        );
        assert!(
            loosened_fields(&old, &Mandate { min_trade_interval_sec: 600, ..sample() }).is_empty()
        );
        assert!(loosened_fields(&old, &Mandate { max_slippage_bps: 50, ..sample() }).is_empty());
    }

    #[test]
    fn adding_an_asset_or_a_stable_asset_is_loosening_and_removing_is_not() {
        let old = sample();
        let mut added = sample();
        added.assets.push(asset("nep141:new.near", 18));
        assert_eq!(loosened_fields(&old, &added), vec!["assets"]);

        let mut removed = sample();
        removed.assets.pop();
        assert!(loosened_fields(&old, &removed).is_empty());

        let stable = Mandate { stable_assets: vec![USDC.into(), WNEAR.into()], ..sample() };
        assert_eq!(loosened_fields(&old, &stable), vec!["stable_assets"]);
    }

    #[test]
    fn changing_an_existing_asset_config_counts_as_loosening() {
        let old = sample();
        let mut changed = sample();
        changed.assets[0].price_feed_id = "other-feed".into();
        assert_eq!(loosened_fields(&old, &changed), vec!["assets"]);
    }

    #[test]
    fn sample_is_valid() {
        assert_eq!(sample().validate(), Ok(()));
    }

    #[test]
    fn model_allowlist_empty_denies_all_and_exact_capacity_is_valid() {
        let mut m = sample();
        m.allowed_model_ids.clear();
        assert_eq!(m.validate(), Ok(()));
        assert!(!m.allows_model_id("manual-v1"));
        m.allowed_model_ids = (0..MAX_MODEL_IDS).map(|i| format!("model-{i}")).collect();
        assert_eq!(m.validate(), Ok(()));
        assert!(m.allows_model_id("model-0"));
        assert!(!m.allows_model_id("model-other"));
        m.allowed_model_ids.push("model-over-cap".into());
        assert_eq!(m.validate(), Err(MandateError::TooManyModelIds));
    }

    #[test]
    fn model_id_bound_counts_utf8_bytes_and_rejects_blank_ids() {
        for id in ["x".repeat(MAX_MODEL_ID_BYTES), "é".repeat(MAX_MODEL_ID_BYTES / 2)] {
            assert_eq!(id.len(), MAX_MODEL_ID_BYTES);
            let m = Mandate { allowed_model_ids: vec![id], ..sample() };
            assert_eq!(m.validate(), Ok(()));
        }
        for id in [
            String::new(),
            " \n\t".into(),
            "\u{2003}".into(),
            "x".repeat(MAX_MODEL_ID_BYTES + 1),
            "é".repeat(MAX_MODEL_ID_BYTES / 2 + 1),
        ] {
            let m = Mandate { allowed_model_ids: vec![id.clone()], ..sample() };
            assert_eq!(m.validate(), Err(MandateError::InvalidModelId(id)));
        }
    }

    #[test]
    fn model_ids_reject_exact_duplicates_and_preserve_exact_identity() {
        let m = Mandate {
            allowed_model_ids: vec!["fixture-model".into(), "fixture-model".into()],
            ..sample()
        };
        assert_eq!(m.validate(), Err(MandateError::DuplicateModelId("fixture-model".into())));
        let m = Mandate {
            allowed_model_ids: vec!["fixture-model".into(), "Fixture-model".into()],
            ..sample()
        };
        assert_eq!(m.validate(), Ok(()));
        assert!(!m.allows_model_id(" fixture-model"));
    }

    #[test]
    fn source_additions_loosen_and_removal_or_reordering_does_not() {
        let old = sample();
        let mut reordered = old.clone();
        reordered.allowed_model_ids.reverse();
        assert!(loosened_fields(&old, &reordered).is_empty());
        let mut removed = old.clone();
        removed.allowed_model_ids.clear();
        assert!(loosened_fields(&old, &removed).is_empty());
        assert_eq!(loosened_fields(&removed, &old), vec!["allowed_model_ids"]);
        let mut added = old.clone();
        added.allowed_model_ids.push("new-model".into());
        assert_eq!(loosened_fields(&old, &added), vec!["allowed_model_ids"]);
        let mixed = Mandate {
            allowed_model_ids: vec!["manual-v1".into(), "new-model".into()],
            ..old.clone()
        };
        assert_eq!(loosened_fields(&old, &mixed), vec!["allowed_model_ids"]);
        assert!(loosened_fields(&old, &old).is_empty());
    }

    #[test]
    fn lookup_helpers() {
        let m = sample();
        assert_eq!(m.asset(WNEAR).map(|a| a.decimals), Some(24));
        assert!(m.asset("nep141:other.near").is_none());
        assert!(m.is_stable(USDC));
        assert!(!m.is_stable(WNEAR));
    }

    #[test]
    fn rejects_empty_assets() {
        let m = Mandate { assets: vec![], ..sample() };
        assert_eq!(m.validate(), Err(MandateError::NoAssets));
    }

    #[test]
    fn caps_asset_count() {
        let mut m = sample();
        let at_cap = MAX_ASSETS - m.assets.len();
        m.assets.extend((0..at_cap).map(|i| asset(&format!("nep141:token{i}.near"), 18)));
        assert_eq!(m.validate(), Ok(()));

        m.assets.push(asset("nep141:one-too-many.near", 18));
        assert_eq!(m.validate(), Err(MandateError::TooManyAssets));
    }

    #[test]
    fn rejects_bad_asset_ids() {
        for bad in ["wrap.near", "nep141:", "nep141:Wrap.near", "erc20:0xabc", "nep245:mt.near"] {
            let mut m = sample();
            m.assets.push(AssetConfig { price_feed_id: "feed".into(), ..asset(bad, 18) });
            assert_eq!(m.validate(), Err(MandateError::InvalidAssetId(bad.into())), "{bad}");
        }
    }

    #[test]
    fn rejects_nep245_without_a_supported_withdrawal_path() {
        let mut m = sample();
        m.assets.push(AssetConfig {
            price_feed_id: "bnb-usd".into(),
            ..asset("nep245:v2_1.omni.hot.tg:56_11111111111111111111", 18)
        });
        assert!(matches!(m.validate(), Err(MandateError::InvalidAssetId(_))));
    }

    #[test]
    fn rejects_duplicates() {
        let mut m = sample();
        m.assets.push(asset(WNEAR, 24));
        assert_eq!(m.validate(), Err(MandateError::DuplicateAsset(WNEAR.into())));

        let mut m = sample();
        m.stable_assets.push(USDC.into());
        assert_eq!(m.validate(), Err(MandateError::DuplicateAsset(USDC.into())));
    }

    #[test]
    fn decimals_bound() {
        let mut m = sample();
        m.assets[0].decimals = MAX_DECIMALS;
        assert_eq!(m.validate(), Ok(()));
        m.assets[0].decimals = MAX_DECIMALS + 1;
        assert_eq!(m.validate(), Err(MandateError::InvalidDecimals(WNEAR.into())));
    }

    #[test]
    fn price_feed_id_rules() {
        for bad in ["", "has space", "quote\"", &"x".repeat(MAX_PRICE_FEED_ID_LEN + 1)] {
            let mut m = sample();
            m.assets[0].price_feed_id = bad.to_string();
            assert_eq!(m.validate(), Err(MandateError::InvalidPriceFeedId(WNEAR.into())), "{bad}");
        }
        let mut m = sample();
        m.assets[0].price_feed_id =
            "0xc415de8d2eba7db216527dff4b60e8f3a5311c740dadb233e13e12547e226750".into();
        assert_eq!(m.validate(), Ok(()));
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
