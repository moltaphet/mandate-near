//! Mandate vault.
//!
//! Holds a treasury on NEAR Intents and will be the only signer for it
//! (see docs/ARCHITECTURE.md).
//!
//! - `mandate`: owner-defined rules and their validation.
//! - `math`: exact USD valuation (256-bit, conservative rounding).
//! - `engine`: applies the mandate to one proposed trade.
//! - `intent`: builds the exact `raw_ed25519` payload MPC will sign.
//!
//! The on-chain flow that feeds the engine (Intents balances, oracle
//! prices) and requests the MPC signature lands in the next milestone.

pub mod engine;
pub mod intent;
pub mod mandate;
pub mod math;

use near_sdk::{AccountId, PanicOnDefault, assert_one_yocto, env, near, require};

pub use crate::engine::{TradingState, TradingStateView};
pub use crate::mandate::{AssetConfig, Mandate, MandateError};

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Vault {
    owner: AccountId,
    mandate: Mandate,
    paused: bool,
    trading: TradingState,
}

#[near]
impl Vault {
    #[init]
    pub fn new(owner: AccountId, mandate: Mandate) -> Self {
        validate_or_panic(&mandate);
        Self { owner, mandate, paused: false, trading: TradingState::default() }
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

    /// Requires 1 yoctoNEAR so a leaked function-call key cannot loosen rules.
    #[payable]
    pub fn set_mandate(&mut self, mandate: Mandate) {
        assert_one_yocto();
        self.assert_owner();
        validate_or_panic(&mandate);
        self.mandate = mandate;
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
    use crate::mandate::tests::sample;

    fn context(predecessor: AccountId, deposit_yocto: u128) {
        testing_env!(
            VMContextBuilder::new()
                .predecessor_account_id(predecessor)
                .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
                .build()
        );
    }

    fn vault() -> Vault {
        context(accounts(0), 0);
        Vault::new(accounts(1), sample())
    }

    #[test]
    fn init_stores_owner_and_mandate() {
        let v = vault();
        assert_eq!(v.get_owner(), accounts(1));
        assert_eq!(v.get_mandate(), sample());
        assert!(!v.is_paused());
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
}
