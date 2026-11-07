//! Deterministic two-asset rebalance from one final ledger/oracle snapshot.
//! The shared vault engine previews every gate; the on-chain engine remains authoritative.

use std::collections::BTreeMap;

use mandate_vault::engine::{
    Market, SECONDS_PER_DAY, Trade, TradeError, TradingState, check_trade,
};
use mandate_vault::math::{Rounding, U256, usd_micros};
use mandate_vault::oracle::{AssetView, PriceData};
use mandate_vault::{Mandate, OracleKind, TradingStateView};
use near_sdk::json_types::{U64, U128};
use near_sdk::serde_json::{Value, json};
use serde::{Deserialize, Serialize};

use crate::decision::REBALANCE_SOURCE_ID;
use crate::relay::{ProposalBinding, Quote, Relay};
use crate::transport::PublicReader;
use crate::{AgentError, Plan};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomousConfig {
    pub rpc_url: String,
    #[serde(default)]
    pub expected_chain_id: Option<String>,
    pub vault_id: String,
    pub relay_url: String,
    pub asset_a: String,
    pub asset_b: String,
    pub target_a_bps: u16,
    pub band_bps: u16,
    /// Request offers long enough to cover signing and the 120-second intent lifetime.
    pub minimum_quote_validity_sec: u64,
    /// Explicit compatibility for a reviewed older vault without get_oracle_kind.
    #[serde(default)]
    pub legacy_price_oracle: bool,
}

impl AutonomousConfig {
    pub fn validate(&self) -> Result<(), AgentError> {
        PublicReader::new(&self.rpc_url, &self.vault_id)?
            .with_chain(self.expected_chain_id.as_deref())?;
        Relay::new(&self.relay_url, None)?;
        if self.asset_a == self.asset_b
            || !(1..10_000).contains(&self.target_a_bps)
            || self.band_bps == 0
            || self.band_bps >= self.target_a_bps
            || self.band_bps >= 10_000 - self.target_a_bps
            || !(450..=3600).contains(&self.minimum_quote_validity_sec)
        {
            return Err("invalid two-asset target, band or quote validity configuration".into());
        }
        Ok(())
    }
}

pub struct Snapshot {
    pub mandate: Mandate,
    pub trading: TradingState,
    pub market: Market,
    pub block_hash: String,
    pub intents_id: String,
    pub agent_id: Option<String>,
    pub not_ready: Option<String>,
}

impl Snapshot {
    /// All reads use the same final block, including the oracle's aggregate/reports.
    pub async fn read(
        reader: &PublicReader,
        legacy_price_oracle: bool,
    ) -> Result<Self, AgentError> {
        let (now, block) = reader.final_time().await?;
        let vault = reader.vault.as_str();
        let mandate: Mandate = reader.view(vault, "get_mandate", json!({}), &block).await?;
        mandate.validate().map_err(|_| "vault returned an invalid mandate")?;
        let state: TradingStateView =
            reader.view(vault, "get_trading_state", json!({}), &block).await?;
        let (_signer, intents, oracle): (String, String, String) =
            reader.view(vault, "get_externals", json!({}), &block).await?;
        let paused: bool = reader.view(vault, "is_paused", json!({}), &block).await?;
        let halted: bool = reader.view(vault, "is_halted", json!({}), &block).await?;
        let active: bool = reader.view(vault, "is_in_flight", json!({}), &block).await?;
        let registered: bool = reader.view(vault, "is_key_registered", json!({}), &block).await?;
        let agent_id: Option<String> = reader.view(vault, "get_agent", json!({}), &block).await?;
        let pending: Value = reader.view(vault, "get_pending_proposal", json!({}), &block).await?;
        let pending_open = if pending.is_null() {
            false
        } else {
            now < pending["expires_sec"].as_u64().ok_or("invalid pending proposal expiry")?
        };
        let not_ready = if paused {
            Some("vault is paused")
        } else if halted {
            Some("vault is halted")
        } else if active {
            Some("trade or withdrawal is in progress")
        } else if !registered {
            Some("vault signing identity is not registered")
        } else if agent_id.is_none() {
            Some("vault has no configured agent")
        } else if pending_open {
            Some("a proposal is pending")
        } else {
            None
        }
        .map(str::to_owned);
        let ids: Vec<&str> = mandate.assets.iter().map(|a| a.asset_id.as_str()).collect();
        let balances: Vec<U128> = reader
            .view(
                &intents,
                "mt_batch_balance_of",
                json!({"account_id":vault, "token_ids":ids}),
                &block,
            )
            .await?;
        if balances.len() != mandate.assets.len() {
            return Err("ledger balance count mismatch".into());
        }
        let kind: OracleKind = if legacy_price_oracle {
            OracleKind::PriceOracle
        } else {
            reader.view(vault, "get_oracle_kind", json!({}), &block).await?
        };
        let feeds: Vec<&str> = mandate.assets.iter().map(|a| a.price_feed_id.as_str()).collect();
        let prices = match kind {
            OracleKind::PriceOracle => {
                let aggregate: PriceData = reader
                    .view(&oracle, "get_price_data", json!({"asset_ids":feeds}), &block)
                    .await?;
                if aggregate.timestamp.0 / 1_000_000_000 > now {
                    return Err("oracle aggregate is from a future snapshot".into());
                }
                let mut reports = BTreeMap::new();
                for feed in feeds {
                    let report: Option<AssetView> =
                        reader.view(&oracle, "get_asset", json!({"asset_id":feed}), &block).await?;
                    if let Some(report) = report {
                        reports.insert(feed.to_owned(), report);
                    }
                }
                mandate_vault::oracle::prices_for(&mandate, &aggregate, &reports)
            }
            OracleKind::Pyth => {
                let prices: mandate_vault::pyth::PythPrices = reader
                    .view(
                        &oracle,
                        "list_prices_no_older_than",
                        json!({"price_ids":feeds, "age":mandate.max_price_age_sec}),
                        &block,
                    )
                    .await?;
                mandate_vault::pyth::prices_for(&mandate, &prices)
            }
        };
        Ok(Self {
            market: Market {
                now_sec: now,
                prices,
                balances: mandate
                    .assets
                    .iter()
                    .zip(balances)
                    .map(|(a, b)| (a.asset_id.clone(), b.0))
                    .collect(),
            },
            mandate,
            trading: TradingState {
                last_trade_at_sec: state.last_trade_at_sec,
                day_start_sec: state.day_start_sec,
                day_turnover_usd: state.day_turnover_usd.0,
                high_water_mark_usd: state.high_water_mark_usd.0,
                signed_intent_expires_at_sec: state.signed_intent_expires_at_sec,
            },
            block_hash: block,
            intents_id: intents,
            agent_id,
            not_ready,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub asset_in: String,
    pub asset_out: String,
    pub amount_in: u128,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Preparation {
    NoTrade { reason: String },
    Trade { prepared: Box<PreparedTrade> },
}

#[derive(Clone, Debug, Serialize)]
pub struct PreparedTrade {
    pub plan: Plan,
    pub quote: Quote,
    pub block_hash: String,
    pub chain_time_sec: u64,
    pub intents_id: String,
    pub agent_id: String,
}

impl PreparedTrade {
    pub fn binding(self, vault_id: &str, proposal_id: u64) -> ProposalBinding {
        ProposalBinding {
            proposal_id: U64(proposal_id),
            vault_id: vault_id.to_owned(),
            intents_id: self.intents_id,
            block_hash: self.block_hash,
            plan: self.plan,
            quote: self.quote,
        }
    }
}

/// Returns a bounded candidate or a no-trade reason BEFORE any quote/proposal request.
pub fn choose(config: &AutonomousConfig, snapshot: &Snapshot) -> Result<Candidate, AgentError> {
    config.validate()?;
    if let Some(reason) = &snapshot.not_ready {
        return Err(reason.clone().into());
    }
    let mandate = &snapshot.mandate;
    if !mandate.allows_model_id(REBALANCE_SOURCE_ID) {
        return Err("the rebalance decision source is not allowed".into());
    }
    if mandate.assets.len() != 2
        || mandate.asset(&config.asset_a).is_none()
        || mandate.asset(&config.asset_b).is_none()
    {
        return Err("strategy requires exactly the configured two mandate assets".into());
    }
    // This dummy trade is used only to run the shared early timing/price gates.
    // Its later economic rejection is irrelevant until an actual quote is selected.
    let dummy = Trade {
        asset_in: config.asset_a.clone(),
        asset_out: config.asset_b.clone(),
        amount_in: 1,
        min_amount_out: 1,
    };
    if let Err(error) = check_trade(mandate, &snapshot.trading, &dummy, &snapshot.market)
        && matches!(
            error,
            TradeError::SignedIntentOutstanding { .. }
                | TradeError::CooldownActive { .. }
                | TradeError::MissingPrice(_)
                | TradeError::StalePrice(_)
                | TradeError::FuturePrice(_)
                | TradeError::Valuation(..)
        )
    {
        return Err(error.to_string().into());
    }
    let value = |id: &str| -> Result<u128, AgentError> {
        let cfg = mandate.asset(id).ok_or("strategy asset is not allowed")?;
        let balance = snapshot.market.balances.get(id).copied().unwrap_or(0);
        let price = snapshot.market.prices.get(id).ok_or("strategy price is missing")?;
        usd_micros(balance, cfg.decimals, price, Rounding::Down)
            .map_err(|_| "strategy valuation failed".into())
    };
    let a = value(&config.asset_a)?;
    let b = value(&config.asset_b)?;
    let nav = a.checked_add(b).ok_or("strategy NAV overflow")?;
    if nav == 0 {
        return Err("portfolio has no positive oracle value".into());
    }
    let scaled_a = U256::from(a) * U256::from(10_000);
    let upper = U256::from(nav) * U256::from(config.target_a_bps + config.band_bps);
    let lower = U256::from(nav) * U256::from(config.target_a_bps - config.band_bps);
    let target_a =
        ((U256::from(nav) * U256::from(config.target_a_bps)) / U256::from(10_000)).as_u128();
    let (input, output, excess) = if scaled_a > upper {
        (&config.asset_a, &config.asset_b, a - target_a)
    } else if scaled_a < lower {
        (&config.asset_b, &config.asset_a, target_a - a)
    } else {
        return Err("holdings are within the target band".into());
    };
    let today = snapshot.market.now_sec - snapshot.market.now_sec % SECONDS_PER_DAY;
    let spent =
        if snapshot.trading.day_start_sec == today { snapshot.trading.day_turnover_usd } else { 0 };
    let remaining = u128::from(mandate.max_daily_turnover_usd.0).saturating_sub(spent);
    let budget = excess.min(u128::from(mandate.max_trade_usd.0)).min(remaining);
    if budget == 0 {
        return Err("no remaining daily allowance or rebalance value".into());
    }
    let cfg = mandate.asset(input).ok_or("strategy input is not allowed")?;
    let price = snapshot.market.prices.get(input).ok_or("strategy input price is missing")?;
    let balance = snapshot.market.balances.get(input).copied().unwrap_or(0).min(i128::MAX as u128);
    // Monotonic exact valuation search avoids a new inverse-price rounding implementation.
    let mut low = 0u128;
    let mut high = balance;
    while low < high {
        let difference = high - low;
        let mid = low + difference / 2 + difference % 2;
        if usd_micros(mid, cfg.decimals, price, Rounding::Up).is_ok_and(|usd| usd <= budget) {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    if low == 0 {
        return Err("rebalance is below one affordable base unit".into());
    }
    Ok(Candidate { asset_in: input.clone(), asset_out: output.clone(), amount_in: low })
}

pub async fn prepare(config: &AutonomousConfig, relay: &Relay) -> Result<Preparation, AgentError> {
    config.validate()?;
    let reader = PublicReader::new(&config.rpc_url, &config.vault_id)?
        .with_chain(config.expected_chain_id.as_deref())?;
    let snapshot = Snapshot::read(&reader, config.legacy_price_oracle).await?;
    let candidate = match choose(config, &snapshot) {
        Ok(candidate) => candidate,
        Err(reason) => return Ok(Preparation::NoTrade { reason: reason.to_string() }),
    };
    let quote = relay
        .quote(
            &candidate.asset_in,
            &candidate.asset_out,
            candidate.amount_in,
            snapshot.market.now_sec,
            config.minimum_quote_validity_sec,
        )
        .await?;
    preview(config, &snapshot, candidate, quote)
}

/// Re-anchor immediately before a paid proposal, repeat target/readiness/gates, and retain exact quote amounts.
pub async fn refresh(
    config: &AutonomousConfig,
    prepared: PreparedTrade,
) -> Result<PreparedTrade, AgentError> {
    let reader = PublicReader::new(&config.rpc_url, &config.vault_id)?
        .with_chain(config.expected_chain_id.as_deref())?;
    let snapshot = Snapshot::read(&reader, config.legacy_price_oracle).await?;
    let candidate = choose(config, &snapshot)?;
    if candidate.asset_in != prepared.plan.asset_in
        || candidate.asset_out != prepared.plan.asset_out
        || candidate.amount_in != prepared.plan.amount_in.0
    {
        return Err(
            "rebalance changed since the quote; request a new quote before proposing".into()
        );
    }
    match preview(config, &snapshot, candidate, prepared.quote)? {
        Preparation::Trade { prepared } => Ok(*prepared),
        Preparation::NoTrade { reason } => Err(reason.into()),
    }
}

pub fn preview(
    config: &AutonomousConfig,
    snapshot: &Snapshot,
    candidate: Candidate,
    quote: Quote,
) -> Result<Preparation, AgentError> {
    quote.validate(
        &candidate.asset_in,
        &candidate.asset_out,
        candidate.amount_in,
        snapshot.market.now_sec,
        config.minimum_quote_validity_sec,
    )?;
    let trade = Trade {
        asset_in: candidate.asset_in,
        asset_out: candidate.asset_out,
        amount_in: candidate.amount_in,
        min_amount_out: quote.amount_out.0,
    };
    if let Err(error) = check_trade(&snapshot.mandate, &snapshot.trading, &trade, &snapshot.market)
    {
        return Ok(Preparation::NoTrade { reason: error.to_string() });
    }
    let plan = Plan {
        asset_in: trade.asset_in,
        asset_out: trade.asset_out,
        amount_in: U128(trade.amount_in),
        min_amount_out: U128(trade.min_amount_out),
        rationale: format!(
            "Two-asset rebalance toward {} bps in {}; band {} bps; final block {}; quote {}; exact input {}; exact output {}.",
            config.target_a_bps,
            config.asset_a,
            config.band_bps,
            snapshot.block_hash,
            quote.quote_hash,
            trade.amount_in,
            trade.min_amount_out
        ),
        response_commitment: None,
    };
    plan.validate()?;
    Ok(Preparation::Trade {
        prepared: Box::new(PreparedTrade {
            plan,
            quote,
            block_hash: snapshot.block_hash.clone(),
            chain_time_sec: snapshot.market.now_sec,
            intents_id: snapshot.intents_id.clone(),
            agent_id: snapshot.agent_id.clone().ok_or("vault has no configured agent")?,
        }),
    })
}
