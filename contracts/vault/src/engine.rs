//! Per-trade mandate enforcement.
//!
//! Pure function of (mandate, trading state, proposed trade, market
//! snapshot) -> approval with the next state, or the first rule violated.
//! The contract supplies the snapshot from on-chain sources (Intents
//! balances, oracle prices, block time); nothing here trusts the proposer.
//!
//! Check order is fixed and documented on [`check_trade`], so rejections are
//! deterministic and easy to test.

use std::collections::BTreeMap;
use std::fmt;

use near_sdk::json_types::U128;
use near_sdk::near;

use crate::mandate::{AssetConfig, BPS_DENOMINATOR, Mandate};
use crate::math::{MathError, Price, Rounding, usd_micros, wide_mul};

pub const SECONDS_PER_DAY: u64 = 86_400;
/// Lifetime of a signed intent. No new signature until it has passed.
pub const INTENT_TTL_SEC: u64 = 120;
/// Tolerated oracle timestamp ahead of block time (clock skew).
pub const MAX_FUTURE_PRICE_SKEW_SEC: u64 = 5;

/// A proposed swap: give `amount_in` of `asset_in`, receive at least
/// `min_amount_out` of `asset_out`. Amounts are token base units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trade {
    pub asset_in: String,
    pub asset_out: String,
    pub amount_in: u128,
    pub min_amount_out: u128,
}

/// Everything the engine may rely on, gathered by the contract.
#[derive(Clone, Debug, Default)]
pub struct Market {
    pub now_sec: u64,
    pub prices: BTreeMap<String, Price>,
    /// Vault balances on NEAR Intents, by asset id, in base units.
    pub balances: BTreeMap<String, u128>,
}

/// Persistent per-vault trading state, updated only by approved trades.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TradingState {
    pub last_trade_at_sec: Option<u64>,
    /// 00:00 UTC of the day `day_turnover_usd` belongs to.
    pub day_start_sec: u64,
    pub day_turnover_usd: u128,
    pub high_water_mark_usd: u128,
    /// 0 when no signed intent can still execute.
    pub signed_intent_expires_at_sec: u64,
}

/// JSON view of [`TradingState`] (u128 values as strings).
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradingStateView {
    pub last_trade_at_sec: Option<u64>,
    pub day_start_sec: u64,
    pub day_turnover_usd: U128,
    pub high_water_mark_usd: U128,
    pub signed_intent_expires_at_sec: u64,
}

impl From<&TradingState> for TradingStateView {
    fn from(s: &TradingState) -> Self {
        Self {
            last_trade_at_sec: s.last_trade_at_sec,
            day_start_sec: s.day_start_sec,
            day_turnover_usd: U128(s.day_turnover_usd),
            high_water_mark_usd: U128(s.high_water_mark_usd),
            signed_intent_expires_at_sec: s.signed_intent_expires_at_sec,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approval {
    /// USD value of `amount_in` at the oracle, rounded up.
    pub notional_usd: u128,
    /// Portfolio value before the trade, rounded down.
    pub nav_before_usd: u128,
    /// State to reserve before requesting the trade's signature.
    pub next_state: TradingState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TradeError {
    SameAsset,
    AssetNotAllowed(String),
    ZeroAmount,
    SignedIntentOutstanding { until_sec: u64 },
    CooldownActive { until_sec: u64 },
    MissingPrice(String),
    StalePrice(String),
    FuturePrice(String),
    InsufficientBalance(String),
    Valuation(String, MathError),
    TradeTooLarge { notional_usd: u128, max_usd: u128 },
    TurnoverCapExceeded { would_be_usd: u128, max_usd: u128 },
    SlippageExceeded,
    DrawdownDeRiskOnly,
    PositionTooLarge,
    EmptyPortfolio,
}

impl TradeError {
    /// Stable machine-readable code, used in events and tests.
    pub fn code(&self) -> &'static str {
        match self {
            Self::SameAsset => "SameAsset",
            Self::AssetNotAllowed(_) => "AssetNotAllowed",
            Self::ZeroAmount => "ZeroAmount",
            Self::SignedIntentOutstanding { .. } => "SignedIntentOutstanding",
            Self::CooldownActive { .. } => "CooldownActive",
            Self::MissingPrice(_) => "MissingPrice",
            Self::StalePrice(_) => "StalePrice",
            Self::FuturePrice(_) => "FuturePrice",
            Self::InsufficientBalance(_) => "InsufficientBalance",
            Self::Valuation(..) => "Valuation",
            Self::TradeTooLarge { .. } => "TradeTooLarge",
            Self::TurnoverCapExceeded { .. } => "TurnoverCapExceeded",
            Self::SlippageExceeded => "SlippageExceeded",
            Self::DrawdownDeRiskOnly => "DrawdownDeRiskOnly",
            Self::PositionTooLarge => "PositionTooLarge",
            Self::EmptyPortfolio => "EmptyPortfolio",
        }
    }
}

impl fmt::Display for TradeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.code())?;
        match self {
            Self::SameAsset => write!(f, "asset_in and asset_out are the same"),
            Self::AssetNotAllowed(id) => write!(f, "{id} is not in the mandate"),
            Self::ZeroAmount => write!(f, "amount_in and min_amount_out must be non-zero"),
            Self::SignedIntentOutstanding { until_sec } => {
                write!(f, "a signed intent can execute until {until_sec}")
            }
            Self::CooldownActive { until_sec } => write!(f, "next trade allowed at {until_sec}"),
            Self::MissingPrice(id) => write!(f, "no oracle price for {id}"),
            Self::StalePrice(id) => write!(f, "oracle price for {id} is too old"),
            Self::FuturePrice(id) => write!(f, "oracle price for {id} is from the future"),
            Self::InsufficientBalance(id) => write!(f, "vault balance of {id} is too low"),
            Self::Valuation(id, e) => write!(f, "cannot value {id}: {e}"),
            Self::TradeTooLarge { notional_usd, max_usd } => {
                write!(f, "notional {notional_usd} exceeds {max_usd} micro-USD")
            }
            Self::TurnoverCapExceeded { would_be_usd, max_usd } => {
                write!(f, "daily turnover would be {would_be_usd} > {max_usd} micro-USD")
            }
            Self::SlippageExceeded => {
                write!(f, "min_amount_out is below oracle value minus slippage")
            }
            Self::DrawdownDeRiskOnly => {
                write!(f, "drawdown breaker tripped: only trades into stable assets are allowed")
            }
            Self::PositionTooLarge => write!(f, "position would exceed max_position_bps"),
            Self::EmptyPortfolio => write!(f, "portfolio value after the trade would be zero"),
        }
    }
}

/// Checks `trade` against `mandate`, in this order (first failure wins):
///
/// 1. Structure: different assets, both in the mandate, non-zero amounts.
/// 2. Timing: no signed intent still executable; cooldown elapsed.
/// 3. Prices: every asset that is traded or held has a fresh oracle price
///    (`age ≤ max_price_age_sec`, at most `MAX_FUTURE_PRICE_SKEW_SEC` ahead).
/// 4. Balance: the vault holds `amount_in`.
/// 5. Size: notional (value of `amount_in`, rounded up) ≤ `max_trade_usd`.
/// 6. Turnover: today's turnover + notional ≤ `max_daily_turnover_usd`;
///    the day resets at 00:00 UTC.
/// 7. Slippage: value of `min_amount_out` (rounded down) ≥ notional ×
///    (1 − `max_slippage_bps`).
/// 8. Drawdown: if NAV < high-water mark × (1 − `max_drawdown_bps`), only
///    trades into a stable asset are allowed.
/// 9. Position: unless `asset_out` is stable, its worst-case value after
///    the trade ≤ `max_position_bps` of the worst-case NAV after the trade.
///    The received amount is valued at max(value of `min_amount_out`,
///    notional), because a solver may deliver more than the minimum.
///
/// NAV counts only mandate assets; anything else in the account is ignored.
pub fn check_trade(
    mandate: &Mandate,
    state: &TradingState,
    trade: &Trade,
    market: &Market,
) -> Result<Approval, TradeError> {
    let now = market.now_sec;

    // 1. Structure.
    if trade.asset_in == trade.asset_out {
        return Err(TradeError::SameAsset);
    }
    let cfg_in = mandate
        .asset(&trade.asset_in)
        .ok_or_else(|| TradeError::AssetNotAllowed(trade.asset_in.clone()))?;
    let cfg_out = mandate
        .asset(&trade.asset_out)
        .ok_or_else(|| TradeError::AssetNotAllowed(trade.asset_out.clone()))?;
    if trade.amount_in == 0 || trade.min_amount_out == 0 {
        return Err(TradeError::ZeroAmount);
    }

    // 2. Timing.
    if now < state.signed_intent_expires_at_sec {
        return Err(TradeError::SignedIntentOutstanding {
            until_sec: state.signed_intent_expires_at_sec,
        });
    }
    if let Some(last) = state.last_trade_at_sec {
        let until_sec = last.saturating_add(u64::from(mandate.min_trade_interval_sec));
        if now < until_sec {
            return Err(TradeError::CooldownActive { until_sec });
        }
    }

    // 3. Prices for everything that is traded or held.
    for asset in &mandate.assets {
        let id = asset.asset_id.as_str();
        let held = market.balances.get(id).is_some_and(|b| *b > 0);
        if held || id == trade.asset_in || id == trade.asset_out {
            fresh_price(mandate, market, id)?;
        }
    }

    // 4. Balance.
    let balance_in = balance(&market.balances, &trade.asset_in);
    if balance_in < trade.amount_in {
        return Err(TradeError::InsufficientBalance(trade.asset_in.clone()));
    }

    // 5. Size.
    let notional = value(market, cfg_in, trade.amount_in, Rounding::Up)?;
    let max_trade = u128::from(mandate.max_trade_usd.0);
    if notional > max_trade {
        return Err(TradeError::TradeTooLarge { notional_usd: notional, max_usd: max_trade });
    }

    // 6. Turnover.
    let day_start_sec = now - now % SECONDS_PER_DAY;
    let turnover_today =
        if state.day_start_sec == day_start_sec { state.day_turnover_usd } else { 0 };
    let max_daily = u128::from(mandate.max_daily_turnover_usd.0);
    let would_be = turnover_today.saturating_add(notional);
    if would_be > max_daily {
        return Err(TradeError::TurnoverCapExceeded { would_be_usd: would_be, max_usd: max_daily });
    }

    // 7. Slippage: out × 10_000 ≥ notional × (10_000 − slippage).
    let out_value = value(market, cfg_out, trade.min_amount_out, Rounding::Down)?;
    let bps = u128::from(BPS_DENOMINATOR);
    if wide_mul(out_value, bps) < wide_mul(notional, bps - u128::from(mandate.max_slippage_bps)) {
        return Err(TradeError::SlippageExceeded);
    }

    // 8. Drawdown breaker.
    let nav_before = nav(mandate, market, &market.balances)?;
    let high_water_mark = state.high_water_mark_usd.max(nav_before);
    let tripped = wide_mul(nav_before, bps)
        < wide_mul(high_water_mark, bps - u128::from(mandate.max_drawdown_bps));
    let out_is_stable = mandate.is_stable(&trade.asset_out);
    if tripped && !out_is_stable {
        return Err(TradeError::DrawdownDeRiskOnly);
    }

    // 9. Position (worst case on both sides).
    let mut after = market.balances.clone();
    after.insert(trade.asset_in.clone(), balance_in - trade.amount_in);
    let balance_out = balance(&market.balances, &trade.asset_out);
    let balance_out_after = balance_out
        .checked_add(trade.min_amount_out)
        .ok_or_else(|| TradeError::Valuation(trade.asset_out.clone(), MathError::Overflow))?;
    after.insert(trade.asset_out.clone(), balance_out_after);
    let nav_after = nav(mandate, market, &after)?;
    if nav_after == 0 {
        return Err(TradeError::EmptyPortfolio);
    }
    if !out_is_stable {
        let held_up = value(market, cfg_out, balance_out, Rounding::Up)?;
        let received_up = value(market, cfg_out, trade.min_amount_out, Rounding::Up)?.max(notional);
        let position = held_up.saturating_add(received_up);
        if wide_mul(position, bps) > wide_mul(nav_after, u128::from(mandate.max_position_bps)) {
            return Err(TradeError::PositionTooLarge);
        }
    }
    // Every other non-stable asset is checked too. A trade can push an unchanged holding
    // over the cap by shrinking NAV, so only a holding whose share rises is refused here.
    for cfg in &mandate.assets {
        if cfg.asset_id == trade.asset_out || mandate.is_stable(&cfg.asset_id) {
            continue;
        }
        let held_before =
            value(market, cfg, balance(&market.balances, &cfg.asset_id), Rounding::Up)?;
        let held_after = value(market, cfg, balance(&after, &cfg.asset_id), Rounding::Up)?;
        let share_rises = wide_mul(held_after, nav_before) > wide_mul(held_before, nav_after);
        if share_rises
            && wide_mul(held_after, bps) > wide_mul(nav_after, u128::from(mandate.max_position_bps))
        {
            return Err(TradeError::PositionTooLarge);
        }
    }

    Ok(Approval {
        notional_usd: notional,
        nav_before_usd: nav_before,
        next_state: TradingState {
            last_trade_at_sec: Some(now),
            day_start_sec,
            day_turnover_usd: would_be,
            high_water_mark_usd: high_water_mark,
            signed_intent_expires_at_sec: now + INTENT_TTL_SEC,
        },
    })
}

fn fresh_price<'a>(
    mandate: &Mandate,
    market: &'a Market,
    id: &str,
) -> Result<&'a Price, TradeError> {
    let price = market.prices.get(id).ok_or_else(|| TradeError::MissingPrice(id.to_owned()))?;
    let now = market.now_sec;
    if price.publish_time_sec > now.saturating_add(MAX_FUTURE_PRICE_SKEW_SEC) {
        return Err(TradeError::FuturePrice(id.to_owned()));
    }
    if now.saturating_sub(price.publish_time_sec) > u64::from(mandate.max_price_age_sec) {
        return Err(TradeError::StalePrice(id.to_owned()));
    }
    Ok(price)
}

fn balance(balances: &BTreeMap<String, u128>, id: &str) -> u128 {
    balances.get(id).copied().unwrap_or(0)
}

/// Prices were checked for freshness in step 3; a missing one here is a bug
/// upstream, still reported as `MissingPrice` rather than a panic.
fn value(
    market: &Market,
    cfg: &AssetConfig,
    amount: u128,
    rounding: Rounding,
) -> Result<u128, TradeError> {
    if amount == 0 {
        return Ok(0);
    }
    let price = market
        .prices
        .get(&cfg.asset_id)
        .ok_or_else(|| TradeError::MissingPrice(cfg.asset_id.clone()))?;
    usd_micros(amount, cfg.decimals, price, rounding)
        .map_err(|e| TradeError::Valuation(cfg.asset_id.clone(), e))
}

/// Portfolio value of mandate assets, each rounded down.
fn nav(
    mandate: &Mandate,
    market: &Market,
    balances: &BTreeMap<String, u128>,
) -> Result<u128, TradeError> {
    mandate.assets.iter().try_fold(0u128, |acc, cfg| {
        let v = value(market, cfg, balance(balances, &cfg.asset_id), Rounding::Down)?;
        acc.checked_add(v)
            .ok_or_else(|| TradeError::Valuation(cfg.asset_id.clone(), MathError::Overflow))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mandate::tests::{USDC, WNEAR, asset, sample};
    use crate::math::tests::price;
    use proptest::prelude::*;

    /// 2026-10-31T12:00:00Z.
    const NOW: u64 = 1_793_448_000;
    const NEAR: u128 = 1_000_000_000_000_000_000_000_000; // 10^24
    const USD: u128 = 1_000_000; // USDC base units and micro-USD alike

    /// wNEAR at 5 USD, USDC at 1 USD, both just published.
    /// Holdings: 100 USDC + 10 wNEAR = NAV 150 USD.
    fn market() -> Market {
        Market {
            now_sec: NOW,
            prices: BTreeMap::from([
                (WNEAR.into(), price(500_000_000, -8, NOW)),
                (USDC.into(), price(100_000_000, -8, NOW)),
            ]),
            balances: BTreeMap::from([(WNEAR.into(), 10 * NEAR), (USDC.into(), 100 * USD)]),
        }
    }

    /// Spend `usd` USDC on wNEAR, accepting `slip_bps` below fair value.
    fn buy_near(usd: u128, slip_bps: u128) -> Trade {
        let fair_out = usd * NEAR / 5;
        Trade {
            asset_in: USDC.into(),
            asset_out: WNEAR.into(),
            amount_in: usd * USD,
            min_amount_out: fair_out * (10_000 - slip_bps) / 10_000,
        }
    }

    fn sell_near(near_amount: u128, usd_out: u128) -> Trade {
        Trade {
            asset_in: WNEAR.into(),
            asset_out: USDC.into(),
            amount_in: near_amount,
            min_amount_out: usd_out,
        }
    }

    fn check(state: &TradingState, trade: &Trade, market: &Market) -> Result<Approval, TradeError> {
        check_trade(&sample(), state, trade, market)
    }

    fn code(r: Result<Approval, TradeError>) -> &'static str {
        r.map(|_| "Approved").unwrap_or_else(|e| e.code())
    }

    #[test]
    fn approves_an_honest_trade_and_returns_next_state() {
        let approval = check(&TradingState::default(), &buy_near(20, 75), &market()).unwrap();
        assert_eq!(approval.notional_usd, 20 * USD);
        assert_eq!(approval.nav_before_usd, 150 * USD);
        assert_eq!(
            approval.next_state,
            TradingState {
                last_trade_at_sec: Some(NOW),
                day_start_sec: NOW - NOW % SECONDS_PER_DAY,
                day_turnover_usd: 20 * USD,
                high_water_mark_usd: 150 * USD,
                signed_intent_expires_at_sec: NOW + INTENT_TTL_SEC,
            }
        );
    }

    #[test]
    fn structural_rules() {
        let s = TradingState::default();
        let m = market();
        let same = Trade { asset_out: USDC.into(), ..buy_near(20, 0) };
        assert_eq!(code(check(&s, &same, &m)), "SameAsset");

        let unknown_out = Trade { asset_out: "nep141:meme.near".into(), ..buy_near(20, 0) };
        assert_eq!(
            check(&s, &unknown_out, &m).unwrap_err(),
            TradeError::AssetNotAllowed("nep141:meme.near".into())
        );
        let unknown_in = Trade { asset_in: "nep141:meme.near".into(), ..buy_near(20, 0) };
        assert_eq!(code(check(&s, &unknown_in, &m)), "AssetNotAllowed");

        let zero = Trade { amount_in: 0, ..buy_near(20, 0) };
        assert_eq!(code(check(&s, &zero, &m)), "ZeroAmount");
        let zero_out = Trade { min_amount_out: 0, ..buy_near(20, 0) };
        assert_eq!(code(check(&s, &zero_out, &m)), "ZeroAmount");
    }

    #[test]
    fn one_signed_intent_at_a_time() {
        let s = TradingState { signed_intent_expires_at_sec: NOW + 1, ..Default::default() };
        assert_eq!(
            check(&s, &buy_near(20, 0), &market()).unwrap_err(),
            TradeError::SignedIntentOutstanding { until_sec: NOW + 1 }
        );
        let expired = TradingState { signed_intent_expires_at_sec: NOW, ..Default::default() };
        assert_eq!(code(check(&expired, &buy_near(20, 0), &market())), "Approved");
    }

    #[test]
    fn cooldown_boundary() {
        let until = NOW;
        let s = TradingState { last_trade_at_sec: Some(until - 300), ..Default::default() };
        assert_eq!(code(check(&s, &buy_near(20, 0), &market())), "Approved");
        let s = TradingState { last_trade_at_sec: Some(until - 299), ..Default::default() };
        assert_eq!(
            check(&s, &buy_near(20, 0), &market()).unwrap_err(),
            TradeError::CooldownActive { until_sec: NOW + 1 }
        );
    }

    #[test]
    fn price_freshness() {
        let s = TradingState::default();
        let mut m = market();
        m.prices.get_mut(WNEAR).unwrap().publish_time_sec = NOW - 60;
        assert_eq!(code(check(&s, &buy_near(20, 0), &m)), "Approved");
        m.prices.get_mut(WNEAR).unwrap().publish_time_sec = NOW - 61;
        assert_eq!(
            check(&s, &buy_near(20, 0), &m).unwrap_err(),
            TradeError::StalePrice(WNEAR.into())
        );

        m.prices.get_mut(WNEAR).unwrap().publish_time_sec = NOW + MAX_FUTURE_PRICE_SKEW_SEC;
        assert_eq!(code(check(&s, &buy_near(20, 0), &m)), "Approved");
        m.prices.get_mut(WNEAR).unwrap().publish_time_sec = NOW + MAX_FUTURE_PRICE_SKEW_SEC + 1;
        assert_eq!(code(check(&s, &buy_near(20, 0), &m)), "FuturePrice");

        let mut m = market();
        m.prices.remove(USDC);
        assert_eq!(
            check(&s, &buy_near(20, 0), &m).unwrap_err(),
            TradeError::MissingPrice(USDC.into())
        );
    }

    #[test]
    fn held_but_untraded_assets_need_prices_too() {
        let mut mandate = sample();
        let eth = "nep141:eth.omft.near";
        mandate.assets.push(asset(eth, 18));
        let mut m = market();
        m.balances.insert(eth.into(), 1);
        assert_eq!(
            check_trade(&mandate, &TradingState::default(), &buy_near(20, 0), &m).unwrap_err(),
            TradeError::MissingPrice(eth.into())
        );
        // A zero balance does not need a price.
        m.balances.insert(eth.into(), 0);
        assert_eq!(
            code(check_trade(&mandate, &TradingState::default(), &buy_near(20, 0), &m)),
            "Approved"
        );
    }

    #[test]
    fn stable_destination_cannot_increase_an_untouched_over_cap_share() {
        let mut mandate = sample();
        let a = "nep141:asset-a.near";
        let b = "nep141:asset-b.near";
        mandate.assets = vec![asset(a, 6), asset(b, 6), asset(USDC, 6)];
        let price =
            Price { mantissa: near_sdk::json_types::U64(1), expo: 0, publish_time_sec: NOW };
        let market = Market {
            now_sec: NOW,
            prices: [(a.into(), price), (b.into(), price), (USDC.into(), price)].into(),
            balances: [(a.into(), 60 * USD), (b.into(), 20 * USD), (USDC.into(), 20 * USD)].into(),
        };
        // A starts exactly at60%;1% loss on the unrelated B->cash trade raises its share.
        let trade = Trade {
            asset_in: b.into(),
            asset_out: USDC.into(),
            amount_in: 10 * USD,
            min_amount_out: 9_900_000,
        };
        assert_eq!(
            check_trade(&mandate, &TradingState::default(), &trade, &market).unwrap_err(),
            TradeError::PositionTooLarge
        );
    }

    #[test]
    fn reducing_an_overweight_asset_can_leave_it_above_cap() {
        let mut mandate = sample();
        let a = "nep141:asset-a.near";
        mandate.assets = vec![asset(a, 6), asset(USDC, 6)];
        let price =
            Price { mantissa: near_sdk::json_types::U64(1), expo: 0, publish_time_sec: NOW };
        let market = Market {
            now_sec: NOW,
            prices: [(a.into(), price), (USDC.into(), price)].into(),
            balances: [(a.into(), 80 * USD), (USDC.into(), 20 * USD)].into(),
        };
        let trade = Trade {
            asset_in: a.into(),
            asset_out: USDC.into(),
            amount_in: 10 * USD,
            min_amount_out: 10 * USD,
        };
        assert!(check_trade(&mandate, &TradingState::default(), &trade, &market).is_ok());
    }

    #[test]
    fn insufficient_balance() {
        let trade = Trade { amount_in: 100 * USD + 1, ..buy_near(20, 0) };
        assert_eq!(
            check(&TradingState::default(), &trade, &market()).unwrap_err(),
            TradeError::InsufficientBalance(USDC.into())
        );
    }

    #[test]
    fn max_trade_size_boundary() {
        let s = TradingState::default();
        let mut m = market();
        m.balances.insert(USDC.into(), 1_000 * USD);
        assert_eq!(code(check(&s, &buy_near(50, 0), &m)), "Approved");
        let over = Trade { amount_in: 50 * USD + 1, min_amount_out: 10 * NEAR, ..buy_near(50, 0) };
        assert_eq!(
            check(&s, &over, &m).unwrap_err(),
            TradeError::TradeTooLarge { notional_usd: 50 * USD + 1, max_usd: 50 * USD }
        );
    }

    #[test]
    fn daily_turnover_resets_at_utc_midnight() {
        let today = NOW - NOW % SECONDS_PER_DAY;
        let s = TradingState {
            day_start_sec: today,
            day_turnover_usd: 190 * USD,
            ..Default::default()
        };
        assert_eq!(
            check(&s, &buy_near(20, 0), &market()).unwrap_err(),
            TradeError::TurnoverCapExceeded { would_be_usd: 210 * USD, max_usd: 200 * USD }
        );
        assert_eq!(code(check(&s, &buy_near(10, 0), &market())), "Approved");

        let yesterday = TradingState { day_start_sec: today - SECONDS_PER_DAY, ..s };
        let approval = check(&yesterday, &buy_near(20, 0), &market()).unwrap();
        assert_eq!(approval.next_state.day_turnover_usd, 20 * USD);
        assert_eq!(approval.next_state.day_start_sec, today);
    }

    #[test]
    fn slippage_boundary_is_exact() {
        let s = TradingState::default();
        // 20 USD in; 1% max slippage => at least 19.8 USD of wNEAR = 3.96 wNEAR.
        let at_limit = Trade { min_amount_out: 396 * NEAR / 100, ..buy_near(20, 0) };
        assert_eq!(code(check(&s, &at_limit, &market())), "Approved");
        let below = Trade { min_amount_out: 396 * NEAR / 100 - 1, ..buy_near(20, 0) };
        assert_eq!(code(check(&s, &below, &market())), "SlippageExceeded");
    }

    #[test]
    fn drawdown_breaker_allows_only_derisking() {
        // HWM 200 USD, NAV 150 USD => 25% drawdown > 15% limit.
        let s = TradingState { high_water_mark_usd: 200 * USD, ..Default::default() };
        assert_eq!(code(check(&s, &buy_near(20, 0), &market())), "DrawdownDeRiskOnly");
        let derisk = sell_near(2 * NEAR, 99 * USD / 10);
        assert_eq!(code(check(&s, &derisk, &market())), "Approved");

        // Exactly at the limit (NAV = 85% of HWM) is not tripped.
        let hwm = 150 * USD * 10_000 / 8_500;
        let s = TradingState { high_water_mark_usd: hwm, ..Default::default() };
        assert_eq!(code(check(&s, &buy_near(20, 0), &market())), "Approved");
    }

    #[test]
    fn high_water_mark_only_rises() {
        let s = TradingState { high_water_mark_usd: 140 * USD, ..Default::default() };
        let a = check(&s, &buy_near(20, 0), &market()).unwrap();
        assert_eq!(a.next_state.high_water_mark_usd, 150 * USD);
        let s = TradingState { high_water_mark_usd: 160 * USD, ..Default::default() };
        let a = check(&s, &buy_near(20, 0), &market()).unwrap();
        assert_eq!(a.next_state.high_water_mark_usd, 160 * USD);
    }

    #[test]
    fn position_cap_uses_worst_case() {
        let s = TradingState::default();
        // Holding 50 USD of wNEAR in a 150 USD portfolio; cap is 60%.
        // +30 USD: position 80 / NAV ~149.7 = 53% → ok.
        assert_eq!(code(check(&s, &buy_near(30, 100), &market())), "Approved");
        // +40 USD: position 90 / NAV ~149.6 = 60.2% → rejected.
        assert_eq!(code(check(&s, &buy_near(40, 100), &market())), "PositionTooLarge");
    }

    #[test]
    fn stable_destination_skips_position_cap() {
        // Selling all wNEAR into USDC leaves 100% in a stable asset.
        assert_eq!(
            code(check(&TradingState::default(), &sell_near(10 * NEAR, 495 * USD / 10), &market())),
            "Approved"
        );
    }

    #[test]
    fn zero_price_is_a_valuation_error() {
        let mut m = market();
        m.prices.insert(USDC.into(), price(0, -8, NOW));
        assert_eq!(
            check(&TradingState::default(), &buy_near(20, 0), &m).unwrap_err(),
            TradeError::Valuation(USDC.into(), MathError::ZeroPrice)
        );
    }

    #[test]
    fn error_messages_carry_codes() {
        let e = TradeError::SlippageExceeded;
        assert!(e.to_string().starts_with("SlippageExceeded: "));
    }

    #[derive(Clone, Debug)]
    struct Scenario {
        state: TradingState,
        trade: Trade,
        market: Market,
        prior_turnover: u128,
        hwm: u128,
    }

    /// Realistic random trades: wNEAR between 1 and 10 USD, trade value up
    /// to 80 USD, min-out between 98.5% and 101% of fair value, random
    /// holdings, prior turnover and high-water mark. Either direction.
    fn scenario() -> impl Strategy<Value = Scenario> {
        (
            1u128..=80 * USD,
            9_850u128..=10_100,
            100_000_000u64..=1_000_000_000,
            0u128..=500 * USD,
            0u128..=100 * NEAR,
            0u128..=220 * USD,
            0u128..=600 * USD,
            any::<bool>(),
        )
            .prop_map(
                |(usd, factor_bps, mantissa, usdc_bal, near_bal, prior_turnover, hwm, buy)| {
                    let mut market = market();
                    market.prices.insert(WNEAR.into(), price(mantissa, -8, NOW));
                    market.balances =
                        BTreeMap::from([(WNEAR.into(), near_bal), (USDC.into(), usdc_bal)]);
                    // `usd` micro-dollars of wNEAR = usd × 10^26 / mantissa base units.
                    let near_for_usd =
                        usd * 100_000_000_000_000_000_000_000_000 / u128::from(mantissa);
                    let trade = if buy {
                        Trade {
                            asset_in: USDC.into(),
                            asset_out: WNEAR.into(),
                            amount_in: usd,
                            min_amount_out: (near_for_usd * factor_bps / 10_000).max(1),
                        }
                    } else {
                        Trade {
                            asset_in: WNEAR.into(),
                            asset_out: USDC.into(),
                            amount_in: near_for_usd.max(1),
                            min_amount_out: (usd * factor_bps / 10_000).max(1),
                        }
                    };
                    let state = TradingState {
                        day_start_sec: NOW - NOW % SECONDS_PER_DAY,
                        day_turnover_usd: prior_turnover,
                        high_water_mark_usd: hwm,
                        ..Default::default()
                    };
                    Scenario { state, trade, market, prior_turnover, hwm }
                },
            )
    }

    fn v(market: &Market, id: &str, amount: u128, r: Rounding) -> u128 {
        let cfg = sample().asset(id).unwrap().clone();
        usd_micros(amount, cfg.decimals, &market.prices[id], r).unwrap()
    }

    proptest! {
        /// Every approved trade satisfies each rule when re-derived here,
        /// independently of the engine's code path.
        #[test]
        fn approvals_always_satisfy_the_mandate(s in scenario()) {
            let mandate = sample();
            let Scenario { state, trade, market, prior_turnover, hwm } = s;
            if let Ok(a) = check_trade(&mandate, &state, &trade, &market) {
                let bps = 10_000u128;
                let notional = v(&market, &trade.asset_in, trade.amount_in, Rounding::Up);
                let out_down = v(&market, &trade.asset_out, trade.min_amount_out, Rounding::Down);

                prop_assert!(trade.amount_in <= market.balances[&trade.asset_in]);
                prop_assert!(notional <= u128::from(mandate.max_trade_usd.0));
                prop_assert!(prior_turnover + notional <= u128::from(mandate.max_daily_turnover_usd.0));
                prop_assert_eq!(a.next_state.day_turnover_usd, prior_turnover + notional);
                prop_assert!(wide_mul(out_down, bps) >= wide_mul(notional, bps - u128::from(mandate.max_slippage_bps)));
                prop_assert!(a.next_state.high_water_mark_usd >= hwm);
                prop_assert_eq!(a.next_state.signed_intent_expires_at_sec, NOW + INTENT_TTL_SEC);

                let nav_of = |b: &BTreeMap<String, u128>| {
                    v(&market, WNEAR, b[WNEAR], Rounding::Down) + v(&market, USDC, b[USDC], Rounding::Down)
                };
                let nav_before = nav_of(&market.balances);
                let hwm_after = hwm.max(nav_before);
                let tripped = wide_mul(nav_before, bps) < wide_mul(hwm_after, bps - u128::from(mandate.max_drawdown_bps));
                if tripped {
                    prop_assert!(mandate.is_stable(&trade.asset_out), "breaker tripped but bought risk");
                }
                if !mandate.is_stable(&trade.asset_out) {
                    let mut after = market.balances.clone();
                    *after.get_mut(&trade.asset_in).unwrap() -= trade.amount_in;
                    *after.get_mut(&trade.asset_out).unwrap() += trade.min_amount_out;
                    let held = v(&market, &trade.asset_out, market.balances[&trade.asset_out], Rounding::Up);
                    let received = v(&market, &trade.asset_out, trade.min_amount_out, Rounding::Up).max(notional);
                    prop_assert!(wide_mul(held + received, bps) <= wide_mul(nav_of(&after), u128::from(mandate.max_position_bps)));
                }
            }
        }
    }

    /// Guards against a vacuous property: the generator must produce plenty
    /// of approvals and exercise every economic rejection path.
    #[test]
    fn property_generator_covers_approvals_and_rejections() {
        use proptest::strategy::ValueTree;
        use proptest::test_runner::TestRunner;

        let mut runner = TestRunner::deterministic();
        let strategy = scenario();
        let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
        let n = 5_000;
        for _ in 0..n {
            let s = strategy.new_tree(&mut runner).unwrap().current();
            *counts
                .entry(code(check_trade(&sample(), &s.state, &s.trade, &s.market)))
                .or_default() += 1;
        }
        eprintln!("outcomes over {n} generated trades: {counts:?}");
        let approved = counts.get("Approved").copied().unwrap_or(0);
        assert!(approved * 10 >= n, "only {approved}/{n} approvals: {counts:?}");
        for expected in [
            "SlippageExceeded",
            "PositionTooLarge",
            "TradeTooLarge",
            "TurnoverCapExceeded",
            "DrawdownDeRiskOnly",
            "InsufficientBalance",
        ] {
            assert!(counts.contains_key(expected), "{expected} never produced: {counts:?}");
        }
    }
}
