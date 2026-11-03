//! Sandbox-only stand-in for `price-oracle.near`.
//!
//! It answers with the same JSON shapes as the mainnet oracle (captured on
//! 2026-11-03, see contracts/vault/src/oracle.rs) and applies the same
//! recency rule: an aggregate price is `null` when the newest report is older
//! than `RECENCY_SEC`. Reports are set by the test, so the sandbox can
//! reproduce fresh, stale and missing prices. It is never deployed to a real
//! network.

use std::collections::BTreeMap;

use near_sdk::json_types::{U64, U128};
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

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct MockOracle {
    reports: BTreeMap<String, Report>,
}

#[near]
impl MockOracle {
    #[init]
    pub fn new() -> Self {
        Self { reports: BTreeMap::new() }
    }

    /// Sandbox only: sets the single report for `asset_id`.
    pub fn set_report(
        &mut self,
        asset_id: String,
        multiplier: U128,
        decimals: u8,
        timestamp_sec: U64,
    ) {
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
}
