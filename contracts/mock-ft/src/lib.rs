//! Sandbox-only minimal NEP-141 token.
//!
//! It exists to fund `intents.near` in tests: `mint` gives a test account
//! tokens, `ft_transfer_call` deposits them the standard way (the receiver is
//! credited, its `ft_on_transfer` answers how much it did not take, and that
//! part is refunded), and `ft_transfer` pays withdrawals out. Storage, metadata
//! and the rest of NEP-141 are left out on purpose. It is never deployed to a
//! real network.

use std::collections::BTreeMap;

use near_sdk::json_types::U128;
use near_sdk::serde_json::{self, json};
use near_sdk::{AccountId, Gas, PanicOnDefault, Promise, assert_one_yocto, env, near, require};

const GAS_FT_ON_TRANSFER: Gas = Gas::from_tgas(100);
const GAS_RESOLVE: Gas = Gas::from_tgas(10);

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct MockFt {
    balances: BTreeMap<AccountId, u128>,
    /// Controlled failure hook for sandbox withdrawal/refund tests only.
    blocked_receiver: Option<AccountId>,
}

#[near]
impl MockFt {
    #[init]
    pub fn new() -> Self {
        Self { balances: BTreeMap::new(), blocked_receiver: None }
    }

    /// Sandbox only: makes ft_transfer refuse this receiver before changing balances.
    /// This open test control is not a production token permission or storage model.
    pub fn set_blocked_receiver(&mut self, receiver_id: Option<AccountId>) {
        self.blocked_receiver = receiver_id;
    }

    /// Sandbox only: credits `amount` to `account_id`.
    pub fn mint(&mut self, account_id: AccountId, amount: U128) {
        *self.balances.entry(account_id).or_default() += amount.0;
    }

    pub fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        U128(self.balances.get(&account_id).copied().unwrap_or(0))
    }

    /// NEP-141 `ft_transfer_call`. Requires 1 yoctoNEAR.
    #[payable]
    pub fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        msg: String,
    ) -> Promise {
        assert_one_yocto();
        let sender = env::predecessor_account_id();
        let balance = self.balances.entry(sender.clone()).or_default();
        require!(*balance >= amount.0, "not enough tokens");
        *balance -= amount.0;
        *self.balances.entry(receiver_id.clone()).or_default() += amount.0;
        Promise::new(receiver_id.clone())
            .function_call(
                "ft_on_transfer".to_string(),
                json!({ "sender_id": sender, "amount": amount, "msg": msg })
                    .to_string()
                    .into_bytes(),
                near_sdk::NearToken::from_yoctonear(0),
                GAS_FT_ON_TRANSFER,
            )
            .then(
                Promise::new(env::current_account_id()).function_call(
                    "ft_resolve_transfer".to_string(),
                    json!({ "sender_id": sender, "receiver_id": receiver_id, "amount": amount })
                        .to_string()
                        .into_bytes(),
                    near_sdk::NearToken::from_yoctonear(0),
                    GAS_RESOLVE,
                ),
            )
    }

    /// NEP-141 `ft_transfer`. Requires 1 yoctoNEAR. Withdrawals from `intents.near` use it.
    #[payable]
    pub fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        assert_one_yocto();
        require!(
            self.blocked_receiver.as_ref() != Some(&receiver_id),
            "sandbox receiver is blocked"
        );
        let _ = memo;
        let sender = env::predecessor_account_id();
        let balance = self.balances.entry(sender).or_default();
        require!(*balance >= amount.0, "not enough tokens");
        *balance -= amount.0;
        *self.balances.entry(receiver_id).or_default() += amount.0;
    }

    /// Refunds what the receiver did not take. Private: only the token calls it.
    #[private]
    pub fn ft_resolve_transfer(
        &mut self,
        sender_id: AccountId,
        receiver_id: AccountId,
        amount: U128,
    ) -> U128 {
        let unused = match env::promise_result_checked(0, 64) {
            Ok(bytes) => serde_json::from_slice::<U128>(&bytes).map_or(amount.0, |u| u.0),
            Err(_) => amount.0,
        }
        .min(amount.0);
        let receiver = self.balances.entry(receiver_id).or_default();
        let refund = unused.min(*receiver);
        *receiver -= refund;
        *self.balances.entry(sender_id).or_default() += refund;
        U128(amount.0 - refund)
    }
}
