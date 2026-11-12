//! Sandbox-only stand-in for the PriceOracle and Pyth NEAR receivers.
//!
//! It answers with the same JSON shapes as the mainnet oracle (captured on
//! 2026-11-03, see contracts/vault/src/oracle.rs) and applies the same
//! recency rule: an aggregate price is `null` when the newest report is older
//! than `RECENCY_SEC`. Reports are set by the test, so the sandbox can
//! reproduce fresh, stale and missing prices. It is never deployed to mainnet.
//! The testnet stand-in (`oracle.mandate-t1.testnet`) is built with the
//! `parent-writes-only` feature, so only its parent account can write prices.

use std::collections::BTreeMap;

use near_sdk::json_types::{I64, U64, U128};
use near_sdk::{PanicOnDefault, env, near};

/// Matches the `recency_duration_sec` of the mainnet oracle.
pub const RECENCY_SEC: u32 = 600;

#[near(serializers = [borsh])]
#[derive(Clone)]
pub struct Report {
    pub multiplier: U128,
    pub decimals: u8,
    pub timestamp_sec: u64,
}

#[near(serializers = [json])]
pub struct OraclePrice {
    pub multiplier: U128,
    pub decimals: u8,
}

#[near(serializers = [json])]
pub struct AssetOptionalPrice {
    pub asset_id: String,
    pub price: Option<OraclePrice>,
}

#[near(serializers = [json])]
pub struct PriceData {
    pub timestamp: U64,
    pub recency_duration_sec: u32,
    pub prices: Vec<AssetOptionalPrice>,
}

#[near(serializers = [json])]
pub struct ReportOut {
    pub oracle_id: String,
    pub timestamp: U64,
    pub price: OraclePrice,
}

#[near(serializers = [json])]
pub struct AssetView {
    pub reports: Vec<ReportOut>,
    pub emas: Vec<String>,
}

/// The Pyth receiver's documented wire shape: string price/conf, numeric expo/time.
#[near(serializers = [json, borsh])]
#[derive(Clone)]
pub struct PythPrice {
    pub price: I64,
    pub conf: U64,
    pub expo: i32,
    pub publish_time: i64,
}

#[near(serializers = [json, borsh])]
#[derive(Clone)]
pub struct PythRequest {
    pub price_ids: Vec<String>,
    pub age: u64,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct MockOracle {
    reports: BTreeMap<String, Report>,
    pyth_reports: BTreeMap<String, PythPrice>,
    last_pyth_request: Option<PythRequest>,
}

#[near]
impl MockOracle {
    #[init]
    pub fn new() -> Self {
        Self { reports: BTreeMap::new(), pyth_reports: BTreeMap::new(), last_pyth_request: None }
    }

    /// Sets the single report for `asset_id`. Open in the sandbox; parent-only on testnet.
    pub fn set_report(
        &mut self,
        asset_id: String,
        multiplier: U128,
        decimals: u8,
        timestamp_sec: U64,
    ) {
        assert_reporter();
        self.reports
            .insert(asset_id, Report { multiplier, decimals, timestamp_sec: timestamp_sec.0 });
    }

    /// Aggregate price per asset, `null` when there is no report within recency.
    pub fn get_price_data(&self, asset_ids: Vec<String>) -> PriceData {
        let now = env::block_timestamp() / 1_000_000_000;
        let prices = asset_ids
            .into_iter()
            .map(|asset_id| {
                let price = self
                    .reports
                    .get(&asset_id)
                    .filter(|r| now.saturating_sub(r.timestamp_sec) <= u64::from(RECENCY_SEC))
                    .map(|r| OraclePrice { multiplier: r.multiplier, decimals: r.decimals });
                AssetOptionalPrice { asset_id, price }
            })
            .collect();
        PriceData {
            timestamp: U64(env::block_timestamp()),
            recency_duration_sec: RECENCY_SEC,
            prices,
        }
    }

    /// The individual report with its real timestamp, regardless of recency.
    pub fn get_asset(&self, asset_id: String) -> Option<AssetView> {
        self.reports.get(&asset_id).map(|r| AssetView {
            reports: vec![ReportOut {
                oracle_id: "mock-oracle.sandbox".into(),
                timestamp: U64(r.timestamp_sec * 1_000_000_000),
                price: OraclePrice { multiplier: r.multiplier, decimals: r.decimals },
            }],
            emas: vec![],
        })
    }

    /// Sandbox only: `None` removes the cache entry so the receiver answers null.
    pub fn set_pyth_report(&mut self, price_id: String, price: Option<PythPrice>) {
        assert_reporter();
        if let Some(price) = price {
            self.pyth_reports.insert(price_id, price);
        } else {
            self.pyth_reports.remove(&price_id);
        }
    }

    /// Sandbox only: publish all cache entries together to avoid a slow test setup
    /// making the first feed stale before the actual callback chain begins.
    pub fn set_pyth_reports(&mut self, prices: BTreeMap<String, PythPrice>) {
        self.pyth_reports = prices;
    }

    /// Same arguments and response map as the Pyth NEAR receiver. Recording the
    /// request lets a sandbox test prove which feeds/age the vault actually used.
    pub fn list_prices_no_older_than(
        &mut self,
        price_ids: Vec<String>,
        age: u64,
    ) -> BTreeMap<String, Option<PythPrice>> {
        self.last_pyth_request = Some(PythRequest { price_ids: price_ids.clone(), age });
        self.preview_pyth_prices(price_ids, age)
    }

    /// Read-only fixture inspection of the exact response generated by the batch call.
    pub fn preview_pyth_prices(
        &self,
        price_ids: Vec<String>,
        age: u64,
    ) -> BTreeMap<String, Option<PythPrice>> {
        let now = env::block_timestamp() / 1_000_000_000;
        price_ids
            .into_iter()
            .map(|id| {
                let price = self
                    .pyth_reports
                    .get(&id)
                    .filter(|price| {
                        u64::try_from(price.publish_time)
                            .is_ok_and(|published| now.saturating_sub(published) <= age)
                    })
                    .cloned();
                (id, price)
            })
            .collect()
    }

    pub fn get_last_pyth_request(&self) -> Option<PythRequest> {
        self.last_pyth_request.clone()
    }
}

/// With `parent-writes-only`, only the account one level up may write prices: for
/// `oracle.mandate-t1.testnet` that is `mandate-t1.testnet`. Without it, anyone may.
fn assert_reporter() {
    #[cfg(feature = "parent-writes-only")]
    {
        let me = env::current_account_id();
        let parent = me.as_str().split_once('.').map_or("", |(_, parent)| parent);
        near_sdk::require!(
            env::predecessor_account_id().as_str() == parent,
            "only the parent account can write prices"
        );
    }
}
