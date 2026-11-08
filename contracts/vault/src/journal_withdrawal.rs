//! Opt-in recovery from a journal in the actual custody executor. Existing
//! vaults keep their voucher protocol; this binding is written only at fresh init.

use super::*;
use mandate_withdrawal_journal::{
    Hex32, JournalProtocolDescriptor, JournalWithdrawalRecord, JournalWithdrawalRequest,
    JournalWithdrawalState, MAX_JOURNAL_RESPONSE_BYTES, PROTOCOL_VERSION, REQUEST_HASH_SCHEME,
};

const BINDING_KEY: &[u8] = b"\0mandate:vault:withdrawal-journal:v1:binding";
const GAS_JOURNAL_READY: Gas = Gas::from_tgas(230);
const GAS_JOURNAL_RESULT: Gas = Gas::from_tgas(30);
const GAS_JOURNAL_CANCEL: Gas = Gas::from_tgas(30);
const GAS_JOURNAL_CANCEL_CALLBACK: Gas = Gas::from_tgas(80);

/// Fresh deployment inputs for a reviewed, compatible custody executor.
#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct JournalVaultConfig {
    pub signer: AccountId,
    pub intents: AccountId,
    pub oracle: AccountId,
    pub oracle_kind: OracleKind,
    pub ledger_domain: Hex32,
}

#[near]
impl Vault {
    /// Select the journal explicitly at fresh initialization. There is no setter
    /// that retrofits terminal facts onto an old, unknown dispatched withdrawal.
    #[init]
    pub fn new_with_journal(
        owner: AccountId,
        mandate: Mandate,
        journal: JournalVaultConfig,
    ) -> Self {
        require!(!env::state_exists(), "journal mode requires a fresh vault");
        require!(!env::storage_has_key(BINDING_KEY), "journal binding already exists");
        let descriptor = JournalProtocolDescriptor {
            protocol_version: PROTOCOL_VERSION.into(),
            ledger_domain: journal.ledger_domain,
            executor_id: journal.intents.clone(),
            request_hash_scheme: REQUEST_HASH_SCHEME.into(),
        };
        descriptor
            .validate(&journal.intents)
            .unwrap_or_else(|_| env::panic_str("invalid withdrawal journal binding"));
        let vault = Self::init(
            owner,
            mandate,
            journal.signer,
            journal.intents,
            journal.oracle,
            journal.oracle_kind,
        );
        let bytes = near_sdk::borsh::to_vec(&descriptor)
            .unwrap_or_else(|_| env::panic_str("could not encode withdrawal journal binding"));
        env::storage_write(BINDING_KEY, &bytes);
        vault
    }

    /// Local pinned configuration, not a claim that the executor was audited.
    pub fn get_withdrawal_journal_binding(&self) -> Option<JournalProtocolDescriptor> {
        self.journal_binding()
    }

    #[private]
    pub fn on_journal_ready(&mut self, withdrawal_id: U64) -> PromiseOrValue<bool> {
        let Some(active) = self.current_withdrawal(withdrawal_id, WithdrawalStage::Preparing)
        else {
            return PromiseOrValue::Value(false);
        };
        let binding = self.journal_binding().expect("journal preparation has no binding");
        // Only read-only views were scheduled in Preparing. Invalidating this
        // attempt cannot leave a delayed custody send behind it.
        if env::promise_results_count() != 2 {
            self.withdrawal_failed(active, "journal preparation refused before sending");
            return PromiseOrValue::Value(false);
        }
        let descriptor = journal_result::<Option<JournalProtocolDescriptor>>(0);
        let admitted = journal_result::<Option<Vec<AccountId>>>(1);
        let valid = descriptor == Some(Some(binding.clone()))
            && admitted.flatten().is_some_and(|vaults| {
                !vaults.is_empty()
                    && vaults.len() <= 20
                    && vaults.windows(2).all(|pair| pair[0] < pair[1])
                    && vaults.binary_search(&env::current_account_id()).is_ok()
            });
        if !valid
            || now_sec() >= active.started_sec.saturating_add(WITHDRAW_RECOVERY_SEC)
            || active.request_seq.0 != self.withdrawal_seq
            || self.mandate.asset(&active.request.asset_id).is_none()
            || self
                .mandate
                .withdrawal_limit(&active.request.asset_id)
                .is_some_and(|ceiling| active.request.amount.0 > ceiling)
        {
            self.withdrawal_failed(active, "journal preparation refused before sending");
            return PromiseOrValue::Value(false);
        }
        let request = journal_request(&active);
        self.withdrawal_stage = Some(WithdrawalStage::Dispatched);
        PromiseOrValue::Promise(
            Promise::new(self.intents.clone())
                .function_call(
                    "withdraw_with_id",
                    json!({ "request": request }).to_string().into_bytes(),
                    NearToken::from_yoctonear(1),
                    GAS_WITHDRAW,
                )
                .then(
                    Self::ext(env::current_account_id())
                        .with_static_gas(GAS_JOURNAL_RESULT)
                        .with_unused_gas_weight(0)
                        .on_journal_withdrawn(withdrawal_id),
                ),
        )
    }

    #[private]
    pub fn on_journal_withdrawn(&mut self, withdrawal_id: U64) -> bool {
        if env::promise_results_count() != 1 {
            return false;
        }
        journal_result::<JournalWithdrawalRecord>(0)
            .and_then(|record| self.reconcile_journal_record(withdrawal_id, record))
            .unwrap_or(false)
    }

    #[private]
    pub fn on_journal_cancelled(&mut self, withdrawal_id: U64) -> PromiseOrValue<bool> {
        if self.current_withdrawal(withdrawal_id, WithdrawalStage::Dispatched).is_none() {
            return PromiseOrValue::Value(false);
        }
        if env::promise_results_count() == 1
            && journal_result::<JournalWithdrawalRecord>(0)
                .and_then(|record| self.reconcile_journal_record(withdrawal_id, record))
                .is_some()
        {
            return PromiseOrValue::Value(true);
        }
        PromiseOrValue::Promise(self.read_journal_status(withdrawal_id))
    }

    #[private]
    pub fn on_journal_reconciled(&mut self, withdrawal_id: U64) -> bool {
        if env::promise_results_count() != 1 {
            return false;
        }
        journal_result::<Option<JournalWithdrawalRecord>>(0)
            .flatten()
            .and_then(|record| self.reconcile_journal_record(withdrawal_id, record))
            .is_some()
    }
}

impl Vault {
    pub(super) fn journal_binding(&self) -> Option<JournalProtocolDescriptor> {
        let bytes = env::storage_read(BINDING_KEY)?;
        require!(bytes.len() <= MAX_JOURNAL_RESPONSE_BYTES, "journal binding exceeds bound");
        let binding: JournalProtocolDescriptor = near_sdk::borsh::from_slice(&bytes)
            .unwrap_or_else(|_| env::panic_str("invalid stored withdrawal journal binding"));
        binding
            .validate(&self.intents)
            .unwrap_or_else(|_| env::panic_str("stored withdrawal executor does not match"));
        Some(binding)
    }

    pub(super) fn prepare_journal_withdrawal(&self, withdrawal_id: U64) -> Promise {
        let read = |method: &str| {
            Promise::new(self.intents.clone()).function_call(
                method,
                b"{}".to_vec(),
                NearToken::from_yoctonear(0),
                GAS_READ_BALANCES,
            )
        };
        read("get_withdrawal_journal_protocol").and(read("get_journal_authorized_vaults")).then(
            Self::ext(env::current_account_id())
                .with_static_gas(GAS_JOURNAL_READY)
                .with_unused_gas_weight(0)
                .on_journal_ready(withdrawal_id),
        )
    }

    pub(super) fn recover_journal_withdrawal(&self, active: &ActiveWithdrawal) -> Promise {
        Promise::new(self.intents.clone())
            .function_call(
                "cancel_withdrawal_with_id",
                json!({ "request": journal_request(active) }).to_string().into_bytes(),
                NearToken::from_yoctonear(1),
                GAS_JOURNAL_CANCEL,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_JOURNAL_CANCEL_CALLBACK)
                    .with_unused_gas_weight(0)
                    .on_journal_cancelled(active.id),
            )
    }

    fn read_journal_status(&self, withdrawal_id: U64) -> Promise {
        Promise::new(self.intents.clone())
            .function_call(
                "get_journal_withdrawal_status",
                json!({ "vault_id": env::current_account_id(), "withdrawal_id": withdrawal_id })
                    .to_string()
                    .into_bytes(),
                NearToken::from_yoctonear(0),
                GAS_READ_BALANCES,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_JOURNAL_RESULT)
                    .with_unused_gas_weight(0)
                    .on_journal_reconciled(withdrawal_id),
            )
    }

    /// Some(true) is delivery; Some(false) is a terminal refund/cancellation.
    /// None retains the guard. The query callback reports guard resolution,
    /// whereas the initial withdrawal callback reports actual delivery.
    fn reconcile_journal_record(
        &mut self,
        id: U64,
        record: JournalWithdrawalRecord,
    ) -> Option<bool> {
        let active = self.current_withdrawal(id, WithdrawalStage::Dispatched)?;
        let binding = self.journal_binding()?;
        if !record.matches_request(&binding, &env::current_account_id(), &journal_request(&active))
        {
            return None;
        }
        match record.state {
            JournalWithdrawalState::Pending => None,
            JournalWithdrawalState::Delivered { delivered_amount } => {
                self.active_withdrawal = None;
                self.withdrawal_stage = None;
                self.last_withdrawal_outcome = Some(WithdrawalOutcome::Delivered {
                    withdrawal_id: id,
                    amount: delivered_amount,
                });
                Some(true)
            }
            JournalWithdrawalState::Refunded { .. } => {
                self.withdrawal_failed(active, "withdrawal refunded");
                Some(false)
            }
            JournalWithdrawalState::Cancelled => {
                self.withdrawal_failed(active, "withdrawal cancelled before debit");
                Some(false)
            }
        }
    }
}

fn journal_request(active: &ActiveWithdrawal) -> JournalWithdrawalRequest {
    JournalWithdrawalRequest {
        withdrawal_id: active.id,
        request_seq: active.request_seq,
        token: token_account(&active.request.asset_id)
            .expect("validated journal nep141 asset")
            .parse()
            .expect("validated journal token account"),
        receiver_id: active.request.receiver.clone(),
        amount: active.request.amount,
    }
}

fn journal_result<T: DeserializeOwned>(index: u64) -> Option<T> {
    if index >= env::promise_results_count() {
        return None;
    }
    env::promise_result_checked(index, MAX_JOURNAL_RESPONSE_BYTES)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

#[cfg(test)]
#[path = "journal_withdrawal_tests.rs"]
mod tests;
