//! Unique, once-issued Intents vouchers fence unknown withdrawal outcomes.
//! Setup must prove s.<vault> and c.<vault> are uncreated and have no signing
//! authority before sealing the vault. The vault never creates those accounts.
//! Current total supply is not lifetime issuance proof: release also requires a
//! fresh atomic deployment or authenticated history proving no old voucher mint,
//! withdrawal/burn, resolver or refund receipt can later replenish an old id.

use super::*;

pub const MIN_WITHDRAWAL_GAS_TGAS: u64 = 280;
pub const MIN_WITHDRAWAL_RECOVERY_GAS_TGAS: u64 = 140;
pub(super) const GAS_VOUCHER: Gas = Gas::from_tgas(30);
pub(super) const GAS_SOURCE_CALLBACK: Gas = Gas::from_tgas(230);
pub(super) const GAS_PREPARED_CALLBACK: Gas = Gas::from_tgas(180);
pub(super) const GAS_CANCEL_CALLBACK: Gas = Gas::from_tgas(90);

#[near(serializers = [json, borsh])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WithdrawalStage {
    Preparing,
    Dispatched,
}

pub(super) fn sinks() -> (AccountId, AccountId) {
    let vault = env::current_account_id();
    require!(
        vault.as_str().len() <= 62,
        "withdrawal recovery requires a vault name of at most 62 bytes"
    );
    (
        format!("s.{vault}").parse().expect("validated sent sink"),
        format!("c.{vault}").parse().expect("validated cancelled sink"),
    )
}

impl Vault {
    pub(super) fn voucher_id(id: U64) -> String {
        format!("withdrawal-{}", id.0)
    }

    pub(super) fn voucher_asset(id: U64) -> String {
        format!("nep245:{}:{}", env::current_account_id(), Self::voucher_id(id))
    }

    pub(super) fn current_withdrawal(
        &self,
        id: U64,
        stage: WithdrawalStage,
    ) -> Option<ActiveWithdrawal> {
        self.active_withdrawal
            .clone()
            .filter(|a| a.id == id && self.withdrawal_stage == Some(stage))
    }

    pub(super) fn prepare_withdrawal(&self, id: U64, asset_id: &str) -> Promise {
        // Both actions share one Intents receipt. The final read returns the
        // issued source voucher and victim balance to the identified callback.
        Promise::new(self.intents.clone())
            .function_call(
                "mt_on_transfer".to_string(),
                json!({
                    "sender_id": env::current_account_id(),
                    "previous_owner_ids": [env::current_account_id()],
                    "token_ids": [Self::voucher_id(id)],
                    "amounts": [U128(1)], "msg": "",
                })
                .to_string()
                .into_bytes(),
                NearToken::from_yoctonear(0),
                GAS_VOUCHER,
            )
            .function_call(
                "mt_batch_balance_of".to_string(),
                json!({ "account_id": env::current_account_id(),
                    "token_ids": [Self::voucher_asset(id), asset_id] })
                .to_string()
                .into_bytes(),
                NearToken::from_yoctonear(0),
                GAS_READ_BALANCES,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_SOURCE_CALLBACK)
                    .with_unused_gas_weight(0)
                    .on_withdrawal_balance(id),
            )
    }

    pub(super) fn send_marked_withdrawal(&self, active: &ActiveWithdrawal) -> Promise {
        let (sent, _) = sinks();
        let token = token_account(&active.request.asset_id).expect("validated nep141 asset");
        // Marker FIRST fences every delayed original. Withdrawal LAST retains
        // its returned terminal U128, including a confirmed zero/refund.
        Promise::new(self.intents.clone())
            .function_call(
                "mt_transfer".to_string(),
                json!({ "receiver_id": sent, "token_id": Self::voucher_asset(active.id),
                    "amount": U128(1) })
                .to_string()
                .into_bytes(),
                NearToken::from_yoctonear(1),
                GAS_VOUCHER,
            )
            .function_call(
                "ft_withdraw".to_string(),
                json!({ "token": token, "receiver_id": active.request.receiver,
                    "amount": active.request.amount })
                .to_string()
                .into_bytes(),
                NearToken::from_yoctonear(1),
                GAS_WITHDRAW,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(Gas::from_tgas(30))
                    .with_unused_gas_weight(0)
                    .on_withdrawn(active.id),
            )
    }

    pub(super) fn cancel_withdrawal_voucher(&self, id: U64) -> Promise {
        let (_, cancelled) = sinks();
        Promise::new(self.intents.clone())
            .function_call(
                "mt_transfer".to_string(),
                json!({ "receiver_id": cancelled, "token_id": Self::voucher_asset(id),
                    "amount": U128(1) })
                .to_string()
                .into_bytes(),
                NearToken::from_yoctonear(1),
                GAS_VOUCHER,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_CANCEL_CALLBACK)
                    .with_unused_gas_weight(0)
                    .on_withdrawal_cancelled(id),
            )
    }

    fn voucher_snapshot(&self, id: U64) -> Promise {
        let (sent, cancelled) = sinks();
        let read = |account: AccountId| {
            Promise::new(self.intents.clone()).function_call(
                "mt_batch_balance_of".to_string(),
                json!({ "account_id": account, "token_ids": [Self::voucher_asset(id)] })
                    .to_string()
                    .into_bytes(),
                NearToken::from_yoctonear(0),
                GAS_READ_BALANCES,
            )
        };
        let supply = Promise::new(self.intents.clone()).function_call(
            "mt_supply".to_string(),
            json!({ "token_id": Self::voucher_asset(id) }).to_string().into_bytes(),
            NearToken::from_yoctonear(0),
            GAS_READ_BALANCES,
        );
        supply.and(read(env::current_account_id())).and(read(sent)).and(read(cancelled))
    }

    pub(super) fn check_withdrawal_preparation(&self, id: U64, before: U128) -> Promise {
        self.voucher_snapshot(id).then(
            Self::ext(env::current_account_id())
                .with_static_gas(GAS_PREPARED_CALLBACK)
                .with_unused_gas_weight(0)
                .on_withdrawal_prepared(id, before),
        )
    }

    pub(super) fn withdrawal_proof(&self, id: U64) -> Promise {
        self.voucher_snapshot(id).then(
            Self::ext(env::current_account_id())
                .with_static_gas(Gas::from_tgas(30))
                .with_unused_gas_weight(0)
                .on_withdrawal_reconciled(id),
        )
    }

    /// Bounded authenticated reads in supply/source/sent/cancelled order.
    pub(super) fn voucher_snapshot_result() -> Option<(u128, u128, u128, u128)> {
        if env::promise_results_count() != 4 {
            return None;
        }
        let supply = read_promise::<Option<U128>>(0).ok()??;
        let source = read_promise::<Vec<U128>>(1).ok()?;
        let sent = read_promise::<Vec<U128>>(2).ok()?;
        let cancelled = read_promise::<Vec<U128>>(3).ok()?;
        if source.len() != 1 || sent.len() != 1 || cancelled.len() != 1 {
            return None;
        }
        Some((supply.0, source[0].0, sent[0].0, cancelled[0].0))
    }

    pub(super) fn withdrawal_failed(&mut self, active: ActiveWithdrawal, reason: &str) {
        self.active_withdrawal = None;
        self.withdrawal_stage = None;
        if active.request_seq.0 == self.withdrawal_seq && self.pending_withdrawal.is_none() {
            self.pending_withdrawal = Some(active.request);
            // Automatic attempts are spent before issuance. Safe retries need
            // explicit owner re-arming, preserving this request's maturity.
            self.withdrawal_retry_armed = None;
        }
        self.last_withdrawal_outcome =
            Some(WithdrawalOutcome::Failed { withdrawal_id: active.id, reason: reason.into() });
    }

    pub(super) fn finish_withdrawal(&mut self, id: U64, result: Result<U128, String>) -> bool {
        let Some(active) = self.current_withdrawal(id, WithdrawalStage::Dispatched) else {
            return false;
        };
        match result {
            Ok(sent) if sent == active.request.amount => {
                self.active_withdrawal = None;
                self.withdrawal_stage = None;
                self.last_withdrawal_outcome =
                    Some(WithdrawalOutcome::Delivered { withdrawal_id: id, amount: sent });
                true
            }
            Ok(U128(0)) => {
                self.withdrawal_failed(active, "withdrawal refunded");
                false
            }
            // Error/partial answers prove neither absence nor full delivery.
            // Recovery uses the durable voucher instead of blindly resending.
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{VMContextBuilder, accounts, get_created_receipts};
    use near_sdk::{PromiseResult, testing_env};

    const T: u64 = 1_793_448_000;

    fn context(who: AccountId, deposit: u128, time: u64, results: Vec<PromiseResult>) {
        testing_env!(
            VMContextBuilder::new()
                .current_account_id(accounts(0))
                .predecessor_account_id(who)
                .attached_deposit(NearToken::from_yoctonear(deposit))
                .block_timestamp(time * 1_000_000_000)
                .build(),
            near_sdk::test_vm_config(),
            near_sdk::RuntimeFeesConfig::test(),
            Default::default(),
            results
        );
    }

    fn preparing() -> (Vault, U64) {
        context(accounts(1), 1, T, vec![]);
        let mut v = Vault::new(accounts(1), mandate::tests::sample());
        v.request_withdrawal(mandate::tests::USDC.into(), accounts(4), U128(20));
        context(accounts(2), 0, T + WITHDRAW_DELAY_SEC, vec![]);
        drop(v.apply_pending_withdrawal());
        let id = v.active_withdrawal.as_ref().unwrap().id;
        assert_eq!(v.withdrawal_stage, Some(WithdrawalStage::Preparing));
        assert!(v.withdrawal_retry_armed.is_none());
        (v, id)
    }

    fn dispatched() -> (Vault, U64) {
        let (mut v, id) = preparing();
        context(
            accounts(0),
            0,
            T + WITHDRAW_DELAY_SEC,
            vec![PromiseResult::Successful(b"[\"1\",\"100\"]".to_vec())],
        );
        assert!(matches!(v.on_withdrawal_balance(id), PromiseOrValue::Promise(_)));
        assert_eq!(v.withdrawal_stage, Some(WithdrawalStage::Preparing));
        context(accounts(0), 0, T + WITHDRAW_DELAY_SEC, snapshot_results(1, 1, 0, 0));
        assert!(matches!(v.on_withdrawal_prepared(id, U128(100)), PromiseOrValue::Promise(_)));
        assert_eq!(v.withdrawal_stage, Some(WithdrawalStage::Dispatched));
        assert_eq!(v.active_withdrawal.as_ref().unwrap().balance_before, Some(U128(100)));
        (v, id)
    }

    fn snapshot_results(
        supply: u128,
        source: u128,
        sent: u128,
        cancelled: u128,
    ) -> Vec<PromiseResult> {
        vec![
            PromiseResult::Successful(serde_json::to_vec(&Some(U128(supply))).unwrap()),
            PromiseResult::Successful(serde_json::to_vec(&vec![U128(source)]).unwrap()),
            PromiseResult::Successful(serde_json::to_vec(&vec![U128(sent)]).unwrap()),
            PromiseResult::Successful(serde_json::to_vec(&vec![U128(cancelled)]).unwrap()),
        ]
    }

    fn proof(sent: u128, cancelled: u128) {
        context(
            accounts(0),
            0,
            T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC,
            snapshot_results(1, 0, sent, cancelled),
        );
    }

    fn limit(v: &mut Vault, amount: u128, time: u64) {
        context(accounts(1), 1, time, vec![]);
        let mut m = v.mandate.clone();
        m.withdrawal_limits = vec![WithdrawalLimit {
            asset_id: mandate::tests::USDC.into(),
            max_per_request: U128(amount),
        }];
        assert_eq!(v.set_mandate(m), MandateChangeOutcome::Applied);
    }

    fn exact_panic(expected: &str, action: impl FnOnce()) {
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action))
            .expect_err("the operation must fail before mutation");
        let text = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied());
        assert_eq!(text, Some(expected));
    }

    #[test]
    fn recovery_snapshot_is_atomic_and_does_not_change_stored_state() {
        context(accounts(1), 0, T, vec![]);
        let empty = Vault::new(accounts(1), mandate::tests::sample());
        assert!(empty.get_withdrawal_recovery_state().is_none());

        let (v, id) = preparing();
        let stored = near_sdk::borsh::to_vec(&v).unwrap();
        let state = v.get_withdrawal_recovery_state().unwrap();
        assert_eq!(state.protocol, "unique_voucher_v1");
        assert_eq!(state.withdrawal_id, id);
        assert_eq!(state.stage, WithdrawalStage::Preparing);
        assert_eq!(state.started_sec, T + WITHDRAW_DELAY_SEC);
        assert_eq!(state.recover_after_sec, T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC);
        assert_eq!(state.required_gas_tgas, MIN_WITHDRAWAL_RECOVERY_GAS_TGAS);
        assert_eq!(near_sdk::borsh::to_vec(&v).unwrap(), stored);
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            json!({
                "protocol": "unique_voucher_v1", "withdrawal_id": "1", "stage": "Preparing",
                "started_sec": T + WITHDRAW_DELAY_SEC,
                "recover_after_sec": T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC,
                "required_gas_tgas": 140,
            })
        );
    }

    #[test]
    fn recovery_attempt_without_an_active_withdrawal_is_a_receipt_free_noop() {
        context(accounts(2), 0, T, vec![]);
        let mut v = Vault::new(accounts(1), mandate::tests::sample());
        let before = near_sdk::borsh::to_vec(&v).unwrap();
        assert!(matches!(v.recover_withdrawal_attempt(U64(1)), PromiseOrValue::Value(false)));
        assert_eq!(near_sdk::borsh::to_vec(&v).unwrap(), before);
        assert!(get_created_receipts().is_empty());
    }

    #[test]
    fn stale_recovery_attempt_cannot_invalidate_or_cancel_a_newer_withdrawal() {
        let (mut v, old) = preparing();
        let mature = T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC;
        context(accounts(2), 0, mature, vec![]);
        assert!(matches!(v.recover_withdrawal_attempt(old), PromiseOrValue::Value(true)));
        context(accounts(1), 1, mature, vec![]);
        v.rearm_pending_withdrawal();
        drop(v.apply_pending_withdrawal());
        let new = v.active_withdrawal.as_ref().unwrap().id;
        assert_ne!(old, new);

        context(accounts(2), 0, mature + WITHDRAW_RECOVERY_SEC, vec![]);
        let before = near_sdk::borsh::to_vec(&v).unwrap();
        assert!(matches!(v.recover_withdrawal_attempt(old), PromiseOrValue::Value(false)));
        assert_eq!(near_sdk::borsh::to_vec(&v).unwrap(), before);
        assert!(get_created_receipts().is_empty());
        assert_eq!(v.get_withdrawal_recovery_state().unwrap().withdrawal_id, new);
    }

    #[test]
    fn bound_recovery_preserves_the_existing_delay_and_preparation_guard() {
        let (mut v, id) = preparing();
        let mature = T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC;
        context(accounts(2), 0, mature - 1, vec![]);
        let before = near_sdk::borsh::to_vec(&v).unwrap();
        exact_panic("the withdrawal recovery window has not passed", || {
            drop(v.recover_withdrawal_attempt(id));
        });
        assert_eq!(near_sdk::borsh::to_vec(&v).unwrap(), before);
        assert!(get_created_receipts().is_empty());

        context(accounts(2), 0, mature, vec![]);
        assert!(matches!(v.recover_withdrawal_attempt(id), PromiseOrValue::Value(true)));
        assert!(v.active_withdrawal.is_none());
        assert!(v.pending_withdrawal.is_some());
        assert!(!v.is_withdrawal_retry_armed());
        assert!(get_created_receipts().is_empty(), "preparation invalidation sends no payment");
    }

    #[test]
    fn bound_dispatched_recovery_keeps_the_gas_boundary_and_only_cancels_its_voucher() {
        let (mut v, id) = dispatched();
        let mature = T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC;
        let set_gas = |gas| {
            testing_env!(
                VMContextBuilder::new()
                    .current_account_id(accounts(0))
                    .predecessor_account_id(accounts(2))
                    .block_timestamp(mature * 1_000_000_000)
                    .prepaid_gas(Gas::from_gas(gas))
                    .build()
            );
        };
        set_gas(Gas::from_tgas(140).as_gas() - 1);
        let before = near_sdk::borsh::to_vec(&v).unwrap();
        exact_panic("recovery requires at least 140 TGas", || {
            drop(v.recover_withdrawal_attempt(id));
        });
        assert_eq!(near_sdk::borsh::to_vec(&v).unwrap(), before);
        assert!(get_created_receipts().is_empty());

        set_gas(Gas::from_tgas(140).as_gas());
        let result = v.recover_withdrawal_attempt(id);
        assert!(matches!(&result, PromiseOrValue::Promise(_)));
        drop(result);
        assert_eq!(near_sdk::borsh::to_vec(&v).unwrap(), before);
        let calls: Vec<_> = get_created_receipts()
            .into_iter()
            .flat_map(|receipt| receipt.actions)
            .filter_map(|action| match action {
                near_sdk::mock::MockAction::FunctionCallWeight { method_name, args, .. } => {
                    Some((String::from_utf8(method_name).unwrap(), args))
                }
                _ => None,
            })
            .collect();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "mt_transfer");
        let cancel: serde_json::Value = serde_json::from_slice(&calls[0].1).unwrap();
        assert_eq!(cancel["token_id"], Vault::voucher_asset(id));
        assert_eq!(cancel["receiver_id"], "c.alice");
        assert_eq!(calls[1].0, "on_withdrawal_cancelled");
        assert!(calls.iter().all(|(method, _)| method != "ft_withdraw"));
    }

    #[test]
    fn withdrawal_request_ceiling_accepts_equality_and_refuses_one_above_without_mutation() {
        context(accounts(1), 1, T, vec![]);
        let mut v = Vault::new(accounts(1), mandate::tests::sample());
        limit(&mut v, 20, T);
        v.request_withdrawal(mandate::tests::USDC.into(), accounts(4), U128(20));
        let before = near_sdk::borsh::to_vec(&v).unwrap();
        exact_panic("the withdrawal exceeds the per-request ceiling", || {
            v.request_withdrawal(mandate::tests::USDC.into(), accounts(4), U128(21));
        });
        assert_eq!(near_sdk::borsh::to_vec(&v).unwrap(), before);
        assert!(v.is_withdrawal_retry_armed());
    }

    #[test]
    fn withdrawal_apply_ceiling_accepts_equality_at_maturity() {
        context(accounts(1), 1, T, vec![]);
        let mut v = Vault::new(accounts(1), mandate::tests::sample());
        limit(&mut v, 20, T);
        v.request_withdrawal(mandate::tests::USDC.into(), accounts(4), U128(20));
        context(accounts(2), 0, T + WITHDRAW_DELAY_SEC, vec![]);
        drop(v.apply_pending_withdrawal());
        assert_eq!(v.active_withdrawal.as_ref().unwrap().request.amount, U128(20));
        assert_eq!(v.withdrawal_stage, Some(WithdrawalStage::Preparing));
    }

    #[test]
    fn withdrawal_apply_tightening_preserves_request_maturity_id_and_arm() {
        let (mut v, _) = preparing();
        // Return to the original queued authorization before any dispatch.
        let active = v.active_withdrawal.clone().unwrap();
        v.withdrawal_failed(active, "test preparation refusal");
        context(accounts(1), 1, T + WITHDRAW_DELAY_SEC, vec![]);
        v.rearm_pending_withdrawal();
        limit(&mut v, 19, T + WITHDRAW_DELAY_SEC);
        context(accounts(2), 0, T + WITHDRAW_DELAY_SEC, vec![]);
        let before = near_sdk::borsh::to_vec(&v).unwrap();
        exact_panic("the withdrawal exceeds the per-request ceiling", || {
            drop(v.apply_pending_withdrawal());
        });
        assert_eq!(near_sdk::borsh::to_vec(&v).unwrap(), before);
        assert!(get_created_receipts().is_empty());
    }

    #[test]
    fn ceiling_tightened_during_preparing_refuses_dispatch_and_requeues_unarmed() {
        let (mut v, id) = preparing();
        let request = v.active_withdrawal.as_ref().unwrap().request.clone();
        limit(&mut v, 19, T + WITHDRAW_DELAY_SEC);
        context(accounts(0), 0, T + WITHDRAW_DELAY_SEC, snapshot_results(1, 1, 0, 0));
        assert!(matches!(v.on_withdrawal_prepared(id, U128(100)), PromiseOrValue::Value(false)));
        assert!(get_created_receipts().is_empty(), "no external send may be scheduled");
        assert!(v.active_withdrawal.is_none() && v.withdrawal_stage.is_none());
        assert_eq!(v.pending_withdrawal, Some(request));
        assert!(!v.is_withdrawal_retry_armed());
        assert_eq!(v.next_withdrawal_id, 2);
        assert_eq!(
            v.last_withdrawal_outcome,
            Some(WithdrawalOutcome::Failed {
                withdrawal_id: id,
                reason: "the withdrawal exceeds the per-request ceiling".into(),
            })
        );
    }

    #[test]
    fn preparing_ceiling_refusal_preserves_newer_replacement_and_its_arm() {
        let (mut v, id) = preparing();
        let replacement_at = T + WITHDRAW_DELAY_SEC + 7;
        limit(&mut v, 10, replacement_at);
        v.request_withdrawal(mandate::tests::USDC.into(), accounts(5), U128(10));
        let replacement = v.pending_withdrawal.clone();
        context(accounts(0), 0, replacement_at, snapshot_results(1, 1, 0, 0));
        assert!(matches!(v.on_withdrawal_prepared(id, U128(100)), PromiseOrValue::Value(false)));
        assert_eq!(v.pending_withdrawal, replacement);
        assert!(v.is_withdrawal_retry_armed());
        assert!(v.active_withdrawal.is_none());
        assert!(get_created_receipts().is_empty());
        // The old callback cannot consume the replacement even with valid proof.
        assert!(matches!(v.on_withdrawal_prepared(id, U128(100)), PromiseOrValue::Value(false)));
        assert_eq!(v.pending_withdrawal, replacement);
    }

    #[test]
    fn confirmed_refund_rearm_cannot_bypass_a_tightened_ceiling() {
        let (mut v, id) = dispatched();
        limit(&mut v, 19, T + WITHDRAW_DELAY_SEC);
        assert!(!v.finish_withdrawal(id, Ok(U128(0))));
        v.rearm_pending_withdrawal();
        context(accounts(2), 0, T + WITHDRAW_DELAY_SEC, vec![]);
        let before = near_sdk::borsh::to_vec(&v).unwrap();
        exact_panic("the withdrawal exceeds the per-request ceiling", || {
            drop(v.apply_pending_withdrawal());
        });
        assert_eq!(near_sdk::borsh::to_vec(&v).unwrap(), before);
        assert!(get_created_receipts().is_empty());
    }

    #[test]
    fn post_dispatch_ceiling_tightening_preserves_terminal_delivery() {
        let (mut v, id) = dispatched();
        limit(&mut v, 19, T + WITHDRAW_DELAY_SEC);
        assert!(v.finish_withdrawal(id, Ok(U128(20))));
        assert!(v.active_withdrawal.is_none());
        assert_eq!(
            v.last_withdrawal_outcome,
            Some(WithdrawalOutcome::Delivered { withdrawal_id: id, amount: U128(20) })
        );
    }

    #[test]
    fn removing_asset_and_its_ceiling_waits_the_full_mandate_delay() {
        context(accounts(1), 1, T, vec![]);
        let mut v = Vault::new(accounts(1), mandate::tests::sample());
        let mut capped = v.mandate.clone();
        capped.withdrawal_limits = vec![WithdrawalLimit {
            asset_id: mandate::tests::WNEAR.into(),
            max_per_request: U128(20),
        }];
        assert_eq!(v.set_mandate(capped.clone()), MandateChangeOutcome::Applied);
        let mut removed = capped.clone();
        removed.assets.retain(|a| a.asset_id != mandate::tests::WNEAR);
        removed.withdrawal_limits.clear();
        assert_eq!(
            v.set_mandate(removed.clone()),
            MandateChangeOutcome::Queued { effective_sec: U64(T + MANDATE_DELAY_SEC) }
        );
        assert_eq!(v.mandate, capped);
        context(accounts(2), 0, T + MANDATE_DELAY_SEC - 1, vec![]);
        exact_panic("the mandate change is still in its delay", || v.apply_pending_mandate());
        assert_eq!(v.mandate, capped);
        context(accounts(2), 0, T + MANDATE_DELAY_SEC, vec![]);
        v.apply_pending_mandate();
        assert_eq!(v.mandate, removed);
    }

    #[test]
    fn removing_a_queued_withdrawal_asset_refuses_apply_without_mutation() {
        context(accounts(1), 1, T, vec![]);
        let mut v = Vault::new(accounts(1), mandate::tests::sample());
        v.request_withdrawal(mandate::tests::WNEAR.into(), accounts(4), U128(20));
        let mut removed = v.mandate.clone();
        removed.assets.retain(|asset| asset.asset_id != mandate::tests::WNEAR);
        assert_eq!(v.set_mandate(removed), MandateChangeOutcome::Applied);
        context(accounts(2), 0, T + WITHDRAW_DELAY_SEC, vec![]);
        let before = near_sdk::borsh::to_vec(&v).unwrap();
        exact_panic("the asset is not in the mandate", || {
            drop(v.apply_pending_withdrawal());
        });
        assert_eq!(near_sdk::borsh::to_vec(&v).unwrap(), before);
        assert!(get_created_receipts().is_empty());
    }

    #[test]
    fn removing_an_asset_during_preparation_refuses_external_dispatch() {
        context(accounts(1), 1, T, vec![]);
        let mut initial = mandate::tests::sample();
        initial.stable_assets.push(mandate::tests::WNEAR.into());
        let mut v = Vault::new(accounts(1), initial);
        v.request_withdrawal(mandate::tests::USDC.into(), accounts(4), U128(20));
        let queued = v.pending_withdrawal.clone();
        context(accounts(2), 0, T + WITHDRAW_DELAY_SEC, vec![]);
        drop(v.apply_pending_withdrawal());
        let id = v.active_withdrawal.as_ref().unwrap().id;
        context(accounts(1), 1, T + WITHDRAW_DELAY_SEC, vec![]);
        let mut removed = v.mandate.clone();
        removed.assets.retain(|asset| asset.asset_id != mandate::tests::USDC);
        removed.stable_assets.retain(|asset| asset != mandate::tests::USDC);
        assert_eq!(v.set_mandate(removed), MandateChangeOutcome::Applied);
        context(accounts(0), 0, T + WITHDRAW_DELAY_SEC, snapshot_results(1, 1, 0, 0));
        assert!(matches!(v.on_withdrawal_prepared(id, U128(100)), PromiseOrValue::Value(false)));
        assert!(get_created_receipts().is_empty());
        assert!(v.active_withdrawal.is_none() && v.withdrawal_stage.is_none());
        assert_eq!(v.pending_withdrawal, queued);
        assert!(!v.is_withdrawal_retry_armed());
        assert_eq!(
            v.last_withdrawal_outcome,
            Some(WithdrawalOutcome::Failed {
                withdrawal_id: id,
                reason: "the asset is not in the mandate".into(),
            })
        );
    }

    #[test]
    fn preparation_requires_exactly_one_voucher_and_sufficient_victim_balance() {
        for answer in [
            "[]",
            "[\"100\"]",
            "[\"0\",\"100\"]",
            "[\"2\",\"100\"]",
            "[\"1\",\"19\"]",
            "[\"1\",\"100\",\"1\"]",
            "invalid",
        ] {
            let (mut v, id) = preparing();
            context(
                accounts(0),
                0,
                T + WITHDRAW_DELAY_SEC,
                vec![PromiseResult::Successful(answer.as_bytes().to_vec())],
            );
            assert!(
                matches!(v.on_withdrawal_balance(id), PromiseOrValue::Value(false)),
                "{answer}"
            );
            assert!(v.active_withdrawal.is_none());
            assert!(v.pending_withdrawal.is_some());
            assert!(!v.is_withdrawal_retry_armed());
            assert_eq!(v.next_withdrawal_id, 2);
        }
    }

    #[test]
    fn prep_timeout_invalidates_old_id_and_late_callback_cannot_dispatch() {
        let (mut v, old) = preparing();
        context(accounts(2), 0, T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC, vec![]);
        assert!(matches!(v.recover_withdrawal(), PromiseOrValue::Value(true)));
        let preserved = v.pending_withdrawal.clone().unwrap();
        assert!(!v.is_withdrawal_retry_armed());
        context(accounts(1), 1, T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC, vec![]);
        v.rearm_pending_withdrawal();
        assert_eq!(v.pending_withdrawal, Some(preserved));
        drop(v.apply_pending_withdrawal());
        let new = v.active_withdrawal.as_ref().unwrap().clone();
        assert_eq!(new.id, U64(2));
        context(
            accounts(0),
            0,
            T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC,
            vec![PromiseResult::Successful(b"[\"1\",\"100\"]".to_vec())],
        );
        assert!(matches!(v.on_withdrawal_balance(old), PromiseOrValue::Value(false)));
        assert_eq!(v.active_withdrawal, Some(new));
        assert_eq!(v.withdrawal_stage, Some(WithdrawalStage::Preparing));
    }

    #[test]
    fn duplicate_preparation_callback_cannot_send_a_second_original() {
        let (mut v, id) = dispatched();
        let active = v.active_withdrawal.clone();
        context(
            accounts(0),
            0,
            T + WITHDRAW_DELAY_SEC,
            vec![PromiseResult::Successful(b"[\"1\",\"100\"]".to_vec())],
        );
        assert!(matches!(v.on_withdrawal_balance(id), PromiseOrValue::Value(false)));
        assert_eq!(v.active_withdrawal, active);
    }

    #[test]
    fn invalid_or_missing_destination_proofs_never_release_unknown_execution() {
        for (sent, cancelled) in [(0, 0), (1, 1), (2, 0), (0, 2), (u128::MAX, 0)] {
            let (mut v, id) = dispatched();
            let active = v.active_withdrawal.clone();
            proof(sent, cancelled);
            assert!(!v.on_withdrawal_reconciled(id));
            assert_eq!(v.active_withdrawal, active);
            assert!(v.pending_withdrawal.is_none());
        }
        let mut failed = snapshot_results(1, 0, 0, 1);
        failed[0] = PromiseResult::Failed;
        let mut malformed = snapshot_results(1, 0, 0, 1);
        malformed[2] = PromiseResult::Successful(b"[]".to_vec());
        for results in [failed, malformed, vec![PromiseResult::Successful(b"[\"1\"]".to_vec())]] {
            let (mut v, id) = dispatched();
            context(accounts(0), 0, T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC, results);
            assert!(!v.on_withdrawal_reconciled(id));
            assert!(v.active_withdrawal.is_some());
        }
    }

    #[test]
    fn positive_sent_proof_consumes_authorization_without_claiming_delivery() {
        let (mut v, id) = dispatched();
        proof(1, 0);
        assert!(v.on_withdrawal_reconciled(id));
        assert_eq!(
            v.last_withdrawal_outcome,
            Some(WithdrawalOutcome::DebitObserved { withdrawal_id: id })
        );
        assert!(v.active_withdrawal.is_none());
        assert!(v.pending_withdrawal.is_none());
        assert!(!v.finish_withdrawal(id, Ok(U128(0))));
        assert!(v.pending_withdrawal.is_none());
    }

    #[test]
    fn cancelled_proof_requeues_unarmed_and_all_old_callbacks_are_inert() {
        let (mut v, old) = dispatched();
        let request = v.active_withdrawal.as_ref().unwrap().request.clone();
        proof(0, 1);
        assert!(v.on_withdrawal_reconciled(old));
        assert_eq!(v.pending_withdrawal, Some(request.clone()));
        assert!(!v.is_withdrawal_retry_armed());
        context(accounts(1), 1, T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC, vec![]);
        v.rearm_pending_withdrawal();
        assert_eq!(v.pending_withdrawal, Some(request));
        drop(v.apply_pending_withdrawal());
        let new = v.active_withdrawal.clone();
        assert!(!v.finish_withdrawal(old, Ok(U128(20))));
        assert!(matches!(v.on_withdrawal_cancelled(old), PromiseOrValue::Value(false)));
        proof(1, 0);
        assert!(!v.on_withdrawal_reconciled(old));
        assert_eq!(v.active_withdrawal, new);
    }

    #[test]
    fn failed_cancellation_still_reads_proof_and_keeps_guard_until_positive() {
        let (mut v, id) = dispatched();
        context(
            accounts(0),
            0,
            T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC,
            vec![PromiseResult::Failed],
        );
        assert!(matches!(v.on_withdrawal_cancelled(id), PromiseOrValue::Promise(_)));
        assert!(v.active_withdrawal.is_some());
        proof(0, 0);
        assert!(!v.on_withdrawal_reconciled(id));
        context(accounts(2), 0, T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC, vec![]);
        assert!(matches!(v.recover_withdrawal(), PromiseOrValue::Promise(_)));
        proof(0, 1);
        assert!(v.on_withdrawal_reconciled(id));
    }

    #[test]
    #[should_panic(expected = "the owner must re-arm the withdrawal attempt")]
    fn confirmed_refund_does_not_allow_permissionless_repeat_issuance() {
        let (mut v, id) = dispatched();
        assert!(!v.finish_withdrawal(id, Ok(U128(0))));
        assert!(!v.is_withdrawal_retry_armed());
        context(accounts(2), 0, T + WITHDRAW_DELAY_SEC, vec![]);
        drop(v.apply_pending_withdrawal());
    }

    #[test]
    #[should_panic(expected = "only the owner can call this method")]
    fn rearm_requires_owner_predecessor() {
        let (mut v, id) = dispatched();
        v.finish_withdrawal(id, Ok(U128(0)));
        context(accounts(2), 1, T + WITHDRAW_DELAY_SEC, vec![]);
        v.rearm_pending_withdrawal();
    }

    #[test]
    #[should_panic(expected = "Requires attached deposit of exactly 1 yoctoNEAR")]
    fn rearm_requires_one_yocto() {
        let (mut v, id) = dispatched();
        v.finish_withdrawal(id, Ok(U128(0)));
        context(accounts(1), 0, T + WITHDRAW_DELAY_SEC, vec![]);
        v.rearm_pending_withdrawal();
    }

    #[test]
    fn replacement_arm_and_delay_survive_an_old_refund() {
        let (mut v, id) = dispatched();
        let replaced_at = T + WITHDRAW_DELAY_SEC + 7;
        context(accounts(1), 1, replaced_at, vec![]);
        v.request_withdrawal(mandate::tests::USDC.into(), accounts(5), U128(10));
        let replacement = v.pending_withdrawal.clone();
        assert_eq!(replacement.as_ref().unwrap().effective_sec, replaced_at + WITHDRAW_DELAY_SEC);
        assert!(v.is_withdrawal_retry_armed());
        v.finish_withdrawal(id, Ok(U128(0)));
        assert_eq!(v.pending_withdrawal, replacement);
        assert!(v.is_withdrawal_retry_armed());
    }

    #[test]
    fn owner_cancellation_cannot_be_resurrected_or_rearmed_by_old_proof() {
        let (mut v, id) = dispatched();
        context(accounts(1), 1, T + WITHDRAW_DELAY_SEC, vec![]);
        v.request_withdrawal(mandate::tests::USDC.into(), accounts(5), U128(10));
        v.cancel_pending_withdrawal();
        proof(0, 1);
        assert!(v.on_withdrawal_reconciled(id));
        assert!(v.pending_withdrawal.is_none());
        assert!(!v.is_withdrawal_retry_armed());
    }

    #[test]
    fn sixty_two_byte_vault_has_valid_inert_sink_names() {
        testing_env!(
            VMContextBuilder::new().current_account_id("a".repeat(62).parse().unwrap()).build()
        );
        let (sent, cancelled) = sinks();
        assert_eq!(sent.as_str().len(), 64);
        assert_eq!(cancelled.as_str().len(), 64);
        assert_eq!(
            Vault::new(accounts(1), mandate::tests::sample()).get_withdrawal_protocol(),
            "unique_voucher_v1"
        );
    }

    #[test]
    #[should_panic(expected = "withdrawal recovery requires a vault name of at most 62 bytes")]
    fn sixty_three_byte_vault_cannot_initialize_unsafe_sink_names() {
        testing_env!(
            VMContextBuilder::new().current_account_id("a".repeat(63).parse().unwrap()).build()
        );
        Vault::new(accounts(1), mandate::tests::sample());
    }

    #[test]
    fn current_supply_source_and_empty_sinks_are_all_required_before_dispatch() {
        for (supply, source, sent, cancelled) in [
            (0, 1, 0, 0),
            (2, 1, 0, 0),
            (1, 0, 0, 0),
            (1, 2, 0, 0),
            (1, 1, 1, 0),
            (1, 1, 0, 1),
            (2, 1, 0, 1),
        ] {
            let (mut v, id) = preparing();
            context(
                accounts(0),
                0,
                T + WITHDRAW_DELAY_SEC,
                snapshot_results(supply, source, sent, cancelled),
            );
            assert!(matches!(
                v.on_withdrawal_prepared(id, U128(100)),
                PromiseOrValue::Value(false)
            ));
            assert!(v.active_withdrawal.is_none());
            assert!(v.pending_withdrawal.is_some());
            assert!(!v.is_withdrawal_retry_armed());
        }
    }

    #[test]
    fn final_seeded_cancelled_proof_requires_consumed_source_and_unique_current_supply() {
        for (supply, source) in [(2, 1), (1, 1), (2, 0), (0, 0)] {
            let (mut v, id) = dispatched();
            context(
                accounts(0),
                0,
                T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC,
                snapshot_results(supply, source, 0, 1),
            );
            assert!(!v.on_withdrawal_reconciled(id));
            assert!(v.active_withdrawal.is_some());
            assert!(v.pending_withdrawal.is_none());
        }
    }

    #[test]
    fn missing_null_failed_and_oversized_supply_never_authorize_preparation() {
        for answer in [
            PromiseResult::Failed,
            PromiseResult::Successful(b"null".to_vec()),
            PromiseResult::Successful(b"{}".to_vec()),
            PromiseResult::Successful(vec![b'x'; MAX_ANSWER_BYTES + 1]),
        ] {
            let (mut v, id) = preparing();
            let mut results = snapshot_results(1, 1, 0, 0);
            results[0] = answer;
            context(accounts(0), 0, T + WITHDRAW_DELAY_SEC, results);
            assert!(matches!(
                v.on_withdrawal_prepared(id, U128(100)),
                PromiseOrValue::Value(false)
            ));
            assert!(v.active_withdrawal.is_none());
            assert!(!v.is_withdrawal_retry_armed());
        }
        let (mut v, id) = preparing();
        context(accounts(0), 0, T + WITHDRAW_DELAY_SEC, vec![]);
        assert!(matches!(v.on_withdrawal_prepared(id, U128(100)), PromiseOrValue::Value(false)));
    }

    #[test]
    fn late_uniqueness_callback_cannot_dispatch_after_preparing_timeout_or_new_id() {
        let (mut v, old) = preparing();
        context(
            accounts(0),
            0,
            T + WITHDRAW_DELAY_SEC,
            vec![PromiseResult::Successful(b"[\"1\",\"100\"]".to_vec())],
        );
        assert!(matches!(v.on_withdrawal_balance(old), PromiseOrValue::Promise(_)));
        assert_eq!(v.withdrawal_stage, Some(WithdrawalStage::Preparing));
        assert_eq!(v.active_withdrawal.as_ref().unwrap().balance_before, None);
        context(accounts(2), 0, T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC, vec![]);
        assert!(matches!(v.recover_withdrawal(), PromiseOrValue::Value(true)));
        context(accounts(1), 1, T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC, vec![]);
        v.rearm_pending_withdrawal();
        drop(v.apply_pending_withdrawal());
        let current = v.active_withdrawal.clone();
        context(
            accounts(0),
            0,
            T + WITHDRAW_DELAY_SEC + WITHDRAW_RECOVERY_SEC,
            snapshot_results(1, 1, 0, 0),
        );
        assert!(matches!(v.on_withdrawal_prepared(old, U128(100)), PromiseOrValue::Value(false)));
        assert_eq!(v.active_withdrawal, current);
        assert_eq!(v.withdrawal_stage, Some(WithdrawalStage::Preparing));
    }

    #[test]
    fn duplicate_uniqueness_callback_cannot_dispatch_or_overwrite_a_sent_phase() {
        let (mut v, id) = dispatched();
        let current = v.active_withdrawal.clone();
        context(accounts(0), 0, T + WITHDRAW_DELAY_SEC, snapshot_results(1, 1, 0, 0));
        assert!(matches!(v.on_withdrawal_prepared(id, U128(100)), PromiseOrValue::Value(false)));
        assert_eq!(v.active_withdrawal, current);
        assert_eq!(v.withdrawal_stage, Some(WithdrawalStage::Dispatched));
    }
}
