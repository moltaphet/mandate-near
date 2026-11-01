//! Mandate vault.
//!
//! Holds a treasury on NEAR Intents and will be the only signer for it
//! (see docs/ARCHITECTURE.md and docs/PROJECT.md).
//!
//! - `mandate`: owner-defined rules and their validation.
//! - `math`: exact USD valuation (256-bit, conservative rounding).
//! - `engine`: applies the mandate to one proposed trade.
//! - `intent`: builds the exact `raw_ed25519` payload MPC will sign.
//!
//! Proposals carry only high-level trade parameters. The vault builds every
//! signed byte itself, so an agent cannot hand it a payload. At most one
//! proposal is pending at a time, and no proposal is accepted while a signed
//! intent is still outstanding. A circuit breaker halts proposals after
//! repeated rejections; only the owner can resume them.
//!
//! The execution path (live balances, prices, MPC call) lands in the next
//! milestone; this module provides the state it will use.

pub mod engine;
pub mod intent;
pub mod mandate;
pub mod math;

use near_sdk::json_types::{U64, U128};
use near_sdk::{AccountId, PanicOnDefault, assert_one_yocto, env, near, require};

pub use crate::engine::{TradingState, TradingStateView};
pub use crate::mandate::{AssetConfig, Mandate, MandateError};

/// How long a pending proposal stays executable. Matches the 60 s target in docs/PROJECT.md.
pub const PROPOSAL_TTL_SEC: u64 = 60;
/// Consecutive rejections before the circuit breaker halts new proposals.
pub const MAX_STRIKES: u32 = 3;

/// A trade the agent wants executed. Deliberately has no payload field.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    pub id: U64,
    pub asset_in: String,
    pub asset_out: String,
    pub amount_in: U128,
    pub min_amount_out: U128,
    /// SHA-256 of the agent's off-chain reasoning, as 64 hex characters.
    pub reason_hash: String,
    pub created_sec: u64,
    pub expires_sec: u64,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Vault {
    owner: AccountId,
    mandate: Mandate,
    paused: bool,
    trading: TradingState,
    agent: Option<AccountId>,
    pending: Option<Proposal>,
    next_proposal_id: u64,
    strikes: u32,
    halted: bool,
}

#[near]
impl Vault {
    #[init]
    pub fn new(owner: AccountId, mandate: Mandate) -> Self {
        validate_or_panic(&mandate);
        Self {
            owner,
            mandate,
            paused: false,
            trading: TradingState::default(),
            agent: None,
            pending: None,
            next_proposal_id: 1,
            strikes: 0,
            halted: false,
        }
    }

    /// Cooldown, daily turnover, high-water mark and signed-intent expiry.
    pub fn get_trading_state(&self) -> TradingStateView {
        TradingStateView::from(&self.trading)
    }

    pub fn get_owner(&self) -> AccountId {
        self.owner.clone()
    }

    pub fn get_mandate(&self) -> Mandate {
        self.mandate.clone()
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn get_agent(&self) -> Option<AccountId> {
        self.agent.clone()
    }

    pub fn get_pending_proposal(&self) -> Option<Proposal> {
        self.pending.clone()
    }

    /// True once the circuit breaker has halted new proposals.
    pub fn is_halted(&self) -> bool {
        self.halted
    }

    /// Consecutive rejections recorded since the last success or resume.
    pub fn get_strikes(&self) -> u32 {
        self.strikes
    }

    /// Requires 1 yoctoNEAR so a leaked function-call key cannot loosen rules.
    #[payable]
    pub fn set_mandate(&mut self, mandate: Mandate) {
        assert_one_yocto();
        self.assert_owner();
        validate_or_panic(&mandate);
        self.mandate = mandate;
    }

    /// Sets the only account allowed to propose trades. Requires 1 yoctoNEAR.
    #[payable]
    pub fn set_agent(&mut self, agent: AccountId) {
        assert_one_yocto();
        self.assert_owner();
        self.agent = Some(agent);
    }

    /// The agent submits a trade. Only high-level parameters are accepted;
    /// the vault constructs the signed payload itself at execution time.
    pub fn propose(
        &mut self,
        asset_in: String,
        asset_out: String,
        amount_in: U128,
        min_amount_out: U128,
        reason_hash: String,
    ) -> U64 {
        self.assert_not_paused();
        require!(!self.halted, "proposals are halted by the circuit breaker");
        let agent = self.agent.clone().unwrap_or_else(|| env::panic_str("no agent is configured"));
        require!(env::predecessor_account_id() == agent, "only the agent can propose trades");

        let now = now_sec();
        // One proposal at a time: a new one only after the pending one has expired.
        if let Some(pending) = &self.pending {
            require!(now >= pending.expires_sec, "a proposal is already pending");
        }
        require!(
            self.trading.signed_intent_expires_at_sec <= now,
            "a signed intent is still outstanding"
        );

        require!(asset_in != asset_out, "asset_in and asset_out must differ");
        require!(self.mandate.asset(&asset_in).is_some(), "asset_in is not in the mandate");
        require!(self.mandate.asset(&asset_out).is_some(), "asset_out is not in the mandate");
        require!(amount_in.0 > 0 && min_amount_out.0 > 0, "amounts must be non-zero");
        require!(is_hex_digest(&reason_hash), "reason_hash must be 64 hex characters");

        let id = self.next_proposal_id;
        self.next_proposal_id += 1;
        self.pending = Some(Proposal {
            id: U64(id),
            asset_in,
            asset_out,
            amount_in,
            min_amount_out,
            reason_hash,
            created_sec: now,
            expires_sec: now + PROPOSAL_TTL_SEC,
        });
        U64(id)
    }

    /// Restores proposals after the circuit breaker has halted them. Owner only.
    #[payable]
    pub fn resume_proposals(&mut self) {
        assert_one_yocto();
        self.assert_owner();
        self.halted = false;
        self.strikes = 0;
    }

    /// Kill switch. Deliberately callable with a function-call key (no
    /// deposit) so the owner can stop trading as fast as possible.
    pub fn pause(&mut self) {
        self.assert_owner();
        self.paused = true;
    }

    /// Resuming is sensitive, so it requires a full-access key (1 yoctoNEAR).
    #[payable]
    pub fn unpause(&mut self) {
        assert_one_yocto();
        self.assert_owner();
        self.paused = false;
    }
}

impl Vault {
    fn assert_owner(&self) {
        require!(
            env::predecessor_account_id() == self.owner,
            "only the owner can call this method"
        );
    }

    fn assert_not_paused(&self) {
        require!(!self.paused, "the vault is paused");
    }

    /// Counts a rejected trade. Three in a row halts new proposals.
    /// Called by the execution path (next milestone) when a gate closes.
    #[allow(dead_code)]
    pub(crate) fn record_rejection(&mut self) {
        self.strikes += 1;
        if self.strikes >= MAX_STRIKES {
            self.halted = true;
        }
    }

    /// Clears the strike count after an approved trade.
    #[allow(dead_code)]
    pub(crate) fn record_approval(&mut self) {
        self.strikes = 0;
    }
}

fn now_sec() -> u64 {
    env::block_timestamp() / 1_000_000_000
}

fn is_hex_digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn validate_or_panic(mandate: &Mandate) {
    if let Err(err) = mandate.validate() {
        env::panic_str(&err.to_string());
    }
}

#[cfg(test)]
mod tests {
    use near_sdk::NearToken;
    use near_sdk::test_utils::{VMContextBuilder, accounts};
    use near_sdk::testing_env;

    use super::*;
    use crate::mandate::tests::{USDC, WNEAR, sample};

    /// 2026-10-31T12:00:00Z in nanoseconds.
    const T0: u64 = 1_793_448_000;
    const REASON: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    fn context_at(predecessor: AccountId, deposit_yocto: u128, now_sec: u64) {
        testing_env!(
            VMContextBuilder::new()
                .predecessor_account_id(predecessor)
                .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
                .block_timestamp(now_sec * 1_000_000_000)
                .build()
        );
    }

    fn context(predecessor: AccountId, deposit_yocto: u128) {
        context_at(predecessor, deposit_yocto, T0);
    }

    fn vault() -> Vault {
        context(accounts(0), 0);
        Vault::new(accounts(1), sample())
    }

    /// Owner (accounts(1)) configures the agent (accounts(3)).
    fn vault_with_agent() -> Vault {
        let mut v = vault();
        context(accounts(1), 1);
        v.set_agent(accounts(3));
        v
    }

    fn propose(v: &mut Vault) -> U64 {
        v.propose(
            USDC.into(),
            WNEAR.into(),
            U128(20_000_000),
            U128(3_960_000_000_000_000_000_000_000),
            REASON.into(),
        )
    }

    #[test]
    fn init_stores_owner_and_mandate() {
        let v = vault();
        assert_eq!(v.get_owner(), accounts(1));
        assert_eq!(v.get_mandate(), sample());
        assert!(!v.is_paused());
        assert_eq!(v.get_agent(), None);
        assert!(!v.is_halted());
        assert_eq!(v.get_strikes(), 0);
        assert_eq!(v.get_pending_proposal(), None);
    }

    #[test]
    #[should_panic(expected = "mandate must allow at least one asset")]
    fn init_rejects_invalid_mandate() {
        context(accounts(0), 0);
        Vault::new(accounts(1), Mandate { assets: vec![], ..sample() });
    }

    #[test]
    fn trading_state_starts_empty() {
        let v = vault();
        let s = v.get_trading_state();
        assert_eq!(s.last_trade_at_sec, None);
        assert_eq!(s.day_turnover_usd.0, 0);
        assert_eq!(s.high_water_mark_usd.0, 0);
        assert_eq!(s.signed_intent_expires_at_sec, 0);
    }

    #[test]
    fn owner_updates_mandate_with_one_yocto() {
        let mut v = vault();
        context(accounts(1), 1);
        let updated = Mandate { max_slippage_bps: 50, ..sample() };
        v.set_mandate(updated.clone());
        assert_eq!(v.get_mandate(), updated);
    }

    #[test]
    #[should_panic(expected = "Requires attached deposit of exactly 1 yoctoNEAR")]
    fn set_mandate_requires_one_yocto() {
        let mut v = vault();
        context(accounts(1), 0);
        v.set_mandate(sample());
    }

    #[test]
    #[should_panic(expected = "only the owner can call this method")]
    fn set_mandate_rejects_non_owner() {
        let mut v = vault();
        context(accounts(2), 1);
        v.set_mandate(sample());
    }

    #[test]
    #[should_panic(expected = "max_slippage_bps must not exceed 500")]
    fn set_mandate_rejects_invalid_mandate() {
        let mut v = vault();
        context(accounts(1), 1);
        v.set_mandate(Mandate { max_slippage_bps: 501, ..sample() });
    }

    #[test]
    fn pause_without_deposit_unpause_with_one_yocto() {
        let mut v = vault();
        context(accounts(1), 0);
        v.pause();
        assert!(v.is_paused());

        context(accounts(1), 1);
        v.unpause();
        assert!(!v.is_paused());
    }

    #[test]
    #[should_panic(expected = "only the owner can call this method")]
    fn pause_rejects_non_owner() {
        let mut v = vault();
        context(accounts(2), 0);
        v.pause();
    }

    // ---- Agent gate and proposal shape ----

    #[test]
    fn owner_sets_agent_with_one_yocto() {
        let v = vault_with_agent();
        assert_eq!(v.get_agent(), Some(accounts(3)));
    }

    #[test]
    #[should_panic(expected = "no agent is configured")]
    fn propose_without_agent_panics() {
        let mut v = vault();
        context(accounts(3), 0);
        propose(&mut v);
    }

    #[test]
    #[should_panic(expected = "only the agent can propose trades")]
    fn propose_rejects_non_agent() {
        let mut v = vault_with_agent();
        context(accounts(2), 0);
        propose(&mut v);
    }

    #[test]
    fn agent_proposal_is_stored_with_expiry() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        let id = propose(&mut v);
        assert_eq!(id, U64(1));
        let p = v.get_pending_proposal().expect("pending");
        assert_eq!(p.asset_in, USDC);
        assert_eq!(p.asset_out, WNEAR);
        assert_eq!(p.amount_in, U128(20_000_000));
        assert_eq!(p.created_sec, T0);
        assert_eq!(p.expires_sec, T0 + PROPOSAL_TTL_SEC);
        assert_eq!(p.reason_hash, REASON);
    }

    #[test]
    fn proposal_ids_increase() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        let first = propose(&mut v);
        context_at(accounts(3), 0, T0 + PROPOSAL_TTL_SEC);
        let second = propose(&mut v);
        assert_eq!(first, U64(1));
        assert_eq!(second, U64(2));
    }

    #[test]
    #[should_panic(expected = "asset_in and asset_out must differ")]
    fn propose_rejects_same_asset() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        v.propose(USDC.into(), USDC.into(), U128(1), U128(1), REASON.into());
    }

    #[test]
    #[should_panic(expected = "asset_out is not in the mandate")]
    fn propose_rejects_unlisted_asset() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        v.propose(USDC.into(), "nep141:pepe.near".into(), U128(1), U128(1), REASON.into());
    }

    #[test]
    #[should_panic(expected = "amounts must be non-zero")]
    fn propose_rejects_zero_amount() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        v.propose(USDC.into(), WNEAR.into(), U128(0), U128(1), REASON.into());
    }

    #[test]
    #[should_panic(expected = "reason_hash must be 64 hex characters")]
    fn propose_rejects_bad_reason_hash() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        v.propose(USDC.into(), WNEAR.into(), U128(1), U128(1), "not-a-hash".into());
    }

    // ---- In-flight lock: one pending proposal, no overlap with a signed intent ----

    #[test]
    #[should_panic(expected = "a proposal is already pending")]
    fn second_proposal_blocked_while_first_pending() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        propose(&mut v);
        context_at(accounts(3), 0, T0 + PROPOSAL_TTL_SEC - 1);
        propose(&mut v);
    }

    #[test]
    fn second_proposal_allowed_exactly_at_expiry() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        propose(&mut v);
        context_at(accounts(3), 0, T0 + PROPOSAL_TTL_SEC);
        assert_eq!(propose(&mut v), U64(2));
    }

    #[test]
    #[should_panic(expected = "a signed intent is still outstanding")]
    fn proposal_blocked_while_signed_intent_outstanding() {
        let mut v = vault_with_agent();
        v.trading.signed_intent_expires_at_sec = T0 + 120;
        context(accounts(3), 0);
        propose(&mut v);
    }

    #[test]
    fn proposal_allowed_after_signed_intent_expires() {
        let mut v = vault_with_agent();
        v.trading.signed_intent_expires_at_sec = T0 + 120;
        context_at(accounts(3), 0, T0 + 120);
        assert_eq!(propose(&mut v), U64(1));
    }

    #[test]
    #[should_panic(expected = "the vault is paused")]
    fn pause_blocks_proposals() {
        let mut v = vault_with_agent();
        context(accounts(1), 0);
        v.pause();
        context(accounts(3), 0);
        propose(&mut v);
    }

    // ---- Circuit breaker ----

    #[test]
    fn two_rejections_do_not_halt() {
        let mut v = vault_with_agent();
        v.record_rejection();
        v.record_rejection();
        assert_eq!(v.get_strikes(), 2);
        assert!(!v.is_halted());
    }

    #[test]
    fn three_rejections_halt_and_block_proposals() {
        let mut v = vault_with_agent();
        for _ in 0..MAX_STRIKES {
            v.record_rejection();
        }
        assert!(v.is_halted());
        context(accounts(3), 0);
        let blocked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| propose(&mut v)));
        assert!(blocked.is_err(), "proposal must be refused while halted");
    }

    #[test]
    fn approval_resets_strikes() {
        let mut v = vault_with_agent();
        v.record_rejection();
        v.record_rejection();
        v.record_approval();
        v.record_rejection();
        assert_eq!(v.get_strikes(), 1);
        assert!(!v.is_halted());
    }

    #[test]
    fn owner_resumes_after_halt() {
        let mut v = vault_with_agent();
        for _ in 0..MAX_STRIKES {
            v.record_rejection();
        }
        context(accounts(1), 1);
        v.resume_proposals();
        assert!(!v.is_halted());
        assert_eq!(v.get_strikes(), 0);
        context(accounts(3), 0);
        assert_eq!(propose(&mut v), U64(1));
    }

    #[test]
    #[should_panic(expected = "only the owner can call this method")]
    fn non_owner_cannot_resume() {
        let mut v = vault_with_agent();
        for _ in 0..MAX_STRIKES {
            v.record_rejection();
        }
        context(accounts(3), 1);
        v.resume_proposals();
    }

    #[test]
    #[should_panic(expected = "Requires attached deposit of exactly 1 yoctoNEAR")]
    fn resume_requires_one_yocto() {
        let mut v = vault_with_agent();
        context(accounts(1), 0);
        v.resume_proposals();
    }
}
