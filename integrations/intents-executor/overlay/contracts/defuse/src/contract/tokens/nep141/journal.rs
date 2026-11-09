//! Request-bound withdrawal results at the actual custody/debit boundary.
//!
//! This is an opt-in, fresh-account protocol. The existing Contract Borsh layout
//! is unchanged; domain, records and prepaid refund reserves have separate raw
//! storage namespaces. A missing or malformed terminal result remains Pending.

use crate::contract::{Contract, ContractExt, config::DefuseConfig};
use defuse_core::token_id::nep141::Nep141TokenId;
use defuse_near_utils::REFUND_MEMO;
use mandate_withdrawal_journal::{
    Hex32, JournalProtocolDescriptor, JournalWithdrawalRecord, JournalWithdrawalRequest,
    JournalWithdrawalState, MAX_JOURNAL_RESPONSE_BYTES, PROTOCOL_VERSION, REQUEST_HASH_SCHEME,
};
use near_contract_standards::fungible_token::core::ext_ft_core;
use near_plugins::{Pausable, pause};
use near_sdk::{
    AccountId, FunctionError, Gas, NearToken, PromiseError, PromiseOrValue, assert_one_yocto, env,
    json_types::U64, near, require,
};

const DOMAIN_KEY: &[u8] = b"\0mandate:withdrawal-journal:v1:domain";
const AUTHORIZED_VAULTS_KEY: &[u8] = b"\0mandate:withdrawal-journal:v1:authorized-vaults";
const MAX_AUTHORIZED_VAULTS: usize = 20;
const RECORD_PREFIX: &[u8] = b"\0mandate:withdrawal-journal:v1:record:";
const REFUND_RESERVE_PREFIX: &[u8] = b"\0mandate:withdrawal-journal:v1:refund:";
const RECORD_SLOT_BYTES: usize = MAX_JOURNAL_RESPONSE_BYTES + 4;
// Conservative prepaid space for native refund bookkeeping. Current Amounts
// retains zero entries, so an ordinary refund reuses them; this per-operation
// reserve also avoids making concurrent terminal updates share one free balance.
const REFUND_STORAGE_RESERVE_BYTES: usize = 4096;
const ACCOUNT_OPERATING_RESERVE_BYTES: u64 = 4096;
pub const JOURNAL_WITHDRAW_MIN_GAS: Gas = Gas::from_tgas(100);
pub const JOURNAL_CANCEL_MIN_GAS: Gas = Gas::from_tgas(30);
const JOURNAL_FT_TRANSFER_GAS: Gas = Gas::from_tgas(30);
const JOURNAL_RESOLVER_GAS: Gas = Gas::from_tgas(40);

#[near]
impl Contract {
    /// Initialize a new executor with an immutable, deployment-specific domain.
    /// Existing executors cannot enable the journal through this initializer.
    #[init]
    #[allow(clippy::use_self)] // near-sdk expands Self in the initializer wrapper.
    pub fn new_with_withdrawal_journal(
        config: DefuseConfig,
        ledger_domain: Hex32,
        authorized_vaults: Vec<AccountId>,
    ) -> Self {
        let mut authorized_vaults = authorized_vaults;
        require!(!env::state_exists(), "withdrawal journal requires a fresh executor");
        require!(!env::storage_has_key(DOMAIN_KEY), "withdrawal journal is already initialized");
        let descriptor = descriptor(ledger_domain);
        descriptor
            .validate(&env::current_account_id())
            .unwrap_or_else(|_| env::panic_str("invalid withdrawal journal domain"));
        require!(
            !authorized_vaults.is_empty() && authorized_vaults.len() <= MAX_AUTHORIZED_VAULTS,
            "withdrawal journal needs one to twenty authorized vaults"
        );
        authorized_vaults.sort();
        require!(
            authorized_vaults.windows(2).all(|pair| pair[0] != pair[1])
                && authorized_vaults.iter().all(|vault| vault != &env::current_account_id()),
            "withdrawal journal vaults must be unique and distinct from the executor"
        );
        let authorization = serde_json::to_vec(&authorized_vaults)
            .unwrap_or_else(|_| env::panic_str("could not encode journal vault admission"));
        let contract = Self::new(config);
        assert_storage_budget(storage_bytes(
            DOMAIN_KEY.len() + 32 + AUTHORIZED_VAULTS_KEY.len() + authorization.len(),
        ));
        env::storage_write(DOMAIN_KEY, &descriptor.ledger_domain.bytes());
        env::storage_write(AUTHORIZED_VAULTS_KEY, &authorization);
        contract
    }

    pub fn get_withdrawal_journal_protocol(&self) -> Option<JournalProtocolDescriptor> {
        let bytes = env::storage_read(DOMAIN_KEY)?;
        let domain: [u8; 32] = bytes
            .try_into()
            .unwrap_or_else(|_| env::panic_str("invalid stored withdrawal journal domain"));
        Some(descriptor(Hex32::from_bytes(domain)))
    }

    #[allow(clippy::needless_pass_by_value)] // Owned JSON ABI account argument.
    pub fn get_journal_withdrawal_status(
        &self,
        vault_id: AccountId,
        withdrawal_id: U64,
    ) -> Option<JournalWithdrawalRecord> {
        let descriptor = self.get_withdrawal_journal_protocol()?;
        read_record(&descriptor, &vault_id, withdrawal_id)
    }

    pub fn get_journal_authorized_vaults(&self) -> Option<Vec<AccountId>> {
        self.get_withdrawal_journal_protocol()?;
        let bytes = env::storage_read(AUTHORIZED_VAULTS_KEY)
            .unwrap_or_else(|| env::panic_str("journal vault admission is missing"));
        require!(bytes.len() <= 2048, "journal vault admission exceeds bound");
        let vaults: Vec<AccountId> = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| env::panic_str("invalid stored journal vault admission"));
        require!(
            !vaults.is_empty()
                && vaults.len() <= MAX_AUTHORIZED_VAULTS
                && vaults.windows(2).all(|pair| pair[0] < pair[1]),
            "invalid stored journal vault admission"
        );
        Some(vaults)
    }

    /// One authenticated request reserves/debits custody and schedules exactly
    /// one ordinary `ft_transfer`. An identical duplicate only returns the record.
    #[pause]
    #[payable]
    pub fn withdraw_with_id(
        &mut self,
        request: JournalWithdrawalRequest,
    ) -> PromiseOrValue<JournalWithdrawalRecord> {
        assert_one_yocto();
        let vault_id = self.ensure_auth_predecessor_id();
        self.assert_journal_authorized(&vault_id);
        let descriptor = self.journal_descriptor();
        validate_request(&request);
        if let Some(existing) = read_record(&descriptor, &vault_id, request.withdrawal_id) {
            require!(
                existing.matches_request(&descriptor, &vault_id, &request),
                "withdrawal id is bound to a different request"
            );
            return PromiseOrValue::Value(existing);
        }
        require!(
            env::prepaid_gas() >= JOURNAL_WITHDRAW_MIN_GAS,
            "journal withdrawal requires at least 100 TGas"
        );
        let pending = new_record(&descriptor, &vault_id, request, JournalWithdrawalState::Pending);
        // Serialize and fund every future terminal slot before custody mutation.
        let encoded = encode_record(&pending);
        let record_key = operation_key(RECORD_PREFIX, &vault_id, pending.request.withdrawal_id);
        let reserve_key =
            operation_key(REFUND_RESERVE_PREFIX, &vault_id, pending.request.withdrawal_id);
        assert_storage_budget(storage_bytes(
            record_key.len() + encoded.len() + reserve_key.len() + REFUND_STORAGE_RESERVE_BYTES,
        ));
        self.withdraw(
            &vault_id,
            [(Nep141TokenId::new(pending.request.token.clone()).into(), pending.request.amount.0)],
            Some("journal withdrawal"),
            false,
        )
        .unwrap_or_else(|err| err.panic());
        env::storage_write(&record_key, &encoded);
        env::storage_write(&reserve_key, &vec![0; REFUND_STORAGE_RESERVE_BYTES]);
        PromiseOrValue::Promise(
            ext_ft_core::ext(pending.request.token.clone())
                .with_attached_deposit(NearToken::from_yoctonear(1))
                .with_static_gas(JOURNAL_FT_TRANSFER_GAS)
                .with_unused_gas_weight(0)
                .ft_transfer(pending.request.receiver_id.clone(), pending.request.amount, None)
                .then(
                    Self::ext(env::current_account_id())
                        .with_static_gas(JOURNAL_RESOLVER_GAS)
                        .with_unused_gas_weight(0)
                        .journal_resolve_withdrawal(vault_id, pending.request.withdrawal_id),
                ),
        )
    }

    /// Cancellation can create an irrevocable tombstone before dispatch. A
    /// Pending operation is never cancelled or resent on an assumption.
    #[payable]
    pub fn cancel_withdrawal_with_id(
        &mut self,
        request: JournalWithdrawalRequest,
    ) -> JournalWithdrawalRecord {
        assert_one_yocto();
        let vault_id = self.ensure_auth_predecessor_id();
        self.assert_journal_authorized(&vault_id);
        let descriptor = self.journal_descriptor();
        validate_request(&request);
        if let Some(existing) = read_record(&descriptor, &vault_id, request.withdrawal_id) {
            require!(
                existing.matches_request(&descriptor, &vault_id, &request),
                "withdrawal id is bound to a different request"
            );
            return existing;
        }
        require!(
            env::prepaid_gas() >= JOURNAL_CANCEL_MIN_GAS,
            "journal cancellation requires at least 30 TGas"
        );
        let cancelled =
            new_record(&descriptor, &vault_id, request, JournalWithdrawalState::Cancelled);
        let encoded = encode_record(&cancelled);
        let key = operation_key(RECORD_PREFIX, &vault_id, cancelled.request.withdrawal_id);
        assert_storage_budget(storage_bytes(key.len() + encoded.len()));
        env::storage_write(&key, &encoded);
        cancelled
    }

    /// This callback directly consumes the underlying ordinary FT transfer
    /// result. Explicit failure refunds in the same receipt as terminal status.
    /// Oversized/nonempty success is ambiguous and cannot authorize a refund.
    #[private]
    pub fn journal_resolve_withdrawal(
        &mut self,
        vault_id: AccountId,
        withdrawal_id: U64,
    ) -> JournalWithdrawalRecord {
        let descriptor = self.journal_descriptor();
        let mut record = read_record(&descriptor, &vault_id, withdrawal_id)
            .unwrap_or_else(|| env::panic_str("no journal withdrawal exists"));
        if record.state != JournalWithdrawalState::Pending || env::promise_results_count() != 1 {
            return record;
        }
        let state = match env::promise_result_checked(0, 0) {
            Ok(empty) if empty.is_empty() => {
                JournalWithdrawalState::Delivered { delivered_amount: record.request.amount }
            }
            Err(PromiseError::Failed) => {
                JournalWithdrawalState::Refunded { refunded_amount: record.request.amount }
            }
            _ => return record,
        };
        record.state = state;
        let encoded = encode_record(&record);
        let reserve_key = operation_key(REFUND_RESERVE_PREFIX, &vault_id, withdrawal_id);
        require!(
            env::storage_read(&reserve_key)
                .is_some_and(|bytes| bytes.len() == REFUND_STORAGE_RESERVE_BYTES),
            "withdrawal refund storage reserve is missing"
        );
        // The reserve release, refund and terminal write all roll back together
        // if ledger restoration or serialization fails. No owner reset exists.
        env::storage_remove(&reserve_key);
        let key = operation_key(RECORD_PREFIX, &vault_id, withdrawal_id);
        if matches!(record.state, JournalWithdrawalState::Refunded { .. }) {
            self.deposit(
                vault_id,
                [(
                    Nep141TokenId::new(record.request.token.clone()).into(),
                    record.request.amount.0,
                )],
                Some(REFUND_MEMO),
            )
            .unwrap_or_else(|err| err.panic());
        }
        env::storage_write(&key, &encoded);
        record
    }
}

impl Contract {
    fn assert_journal_authorized(&self, vault_id: &AccountId) {
        let vaults = self
            .get_journal_authorized_vaults()
            .unwrap_or_else(|| env::panic_str("withdrawal journal is not enabled"));
        require!(
            vaults.binary_search(vault_id).is_ok(),
            "vault is not admitted to the withdrawal journal"
        );
    }

    fn journal_descriptor(&self) -> JournalProtocolDescriptor {
        self.get_withdrawal_journal_protocol()
            .unwrap_or_else(|| env::panic_str("withdrawal journal is not enabled"))
    }
}

fn descriptor(ledger_domain: Hex32) -> JournalProtocolDescriptor {
    JournalProtocolDescriptor {
        protocol_version: PROTOCOL_VERSION.into(),
        ledger_domain,
        executor_id: env::current_account_id(),
        request_hash_scheme: REQUEST_HASH_SCHEME.into(),
    }
}

fn validate_request(request: &JournalWithdrawalRequest) {
    request.validate().unwrap_or_else(|_| env::panic_str("invalid journal withdrawal request"));
    require!(
        request.receiver_id != env::current_account_id(),
        "withdrawal receiver cannot be the executor"
    );
}

fn new_record(
    descriptor: &JournalProtocolDescriptor,
    vault_id: &AccountId,
    request: JournalWithdrawalRequest,
    state: JournalWithdrawalState,
) -> JournalWithdrawalRecord {
    JournalWithdrawalRecord::new(descriptor, vault_id.clone(), request, state)
        .unwrap_or_else(|_| env::panic_str("invalid journal withdrawal record"))
}

fn operation_key(prefix: &[u8], vault_id: &AccountId, withdrawal_id: U64) -> Vec<u8> {
    let account_bytes = u32::try_from(vault_id.as_str().len()).expect("bounded account ID length");
    let mut input = account_bytes.to_le_bytes().to_vec();
    input.extend_from_slice(vault_id.as_str().as_bytes());
    input.extend_from_slice(&withdrawal_id.0.to_le_bytes());
    let mut key = prefix.to_vec();
    key.extend_from_slice(&env::sha256(&input));
    key
}

fn encode_record(record: &JournalWithdrawalRecord) -> Vec<u8> {
    let body = serde_json::to_vec(record)
        .unwrap_or_else(|_| env::panic_str("could not encode withdrawal journal record"));
    require!(body.len() <= MAX_JOURNAL_RESPONSE_BYTES, "withdrawal journal record exceeds bound");
    let mut slot = vec![0; RECORD_SLOT_BYTES];
    let body_bytes = u32::try_from(body.len()).expect("bounded journal response length");
    slot[..4].copy_from_slice(&body_bytes.to_le_bytes());
    slot[4..4 + body.len()].copy_from_slice(&body);
    slot
}

fn read_record(
    descriptor: &JournalProtocolDescriptor,
    vault_id: &AccountId,
    withdrawal_id: U64,
) -> Option<JournalWithdrawalRecord> {
    let slot = env::storage_read(&operation_key(RECORD_PREFIX, vault_id, withdrawal_id))?;
    require!(slot.len() == RECORD_SLOT_BYTES, "invalid withdrawal journal slot");
    let len =
        usize::try_from(u32::from_le_bytes(slot[..4].try_into().expect("fixed length prefix")))
            .expect("journal response length fits the target");
    require!(len <= MAX_JOURNAL_RESPONSE_BYTES, "invalid withdrawal journal record length");
    let record: JournalWithdrawalRecord = serde_json::from_slice(&slot[4..4 + len])
        .unwrap_or_else(|_| env::panic_str("invalid stored withdrawal journal record"));
    require!(
        record.request.withdrawal_id == withdrawal_id
            && record.matches_request(descriptor, vault_id, &record.request),
        "stored withdrawal journal binding mismatch"
    );
    Some(record)
}

fn storage_bytes(bytes: usize) -> u64 {
    u64::try_from(bytes).expect("bounded journal storage length")
}

fn assert_storage_budget(additional_bytes: u64) {
    let required_bytes = env::storage_usage()
        .checked_add(additional_bytes)
        .and_then(|usage| usage.checked_add(ACCOUNT_OPERATING_RESERVE_BYTES))
        .unwrap_or_else(|| env::panic_str("withdrawal journal storage budget overflow"));
    let required_balance = env::storage_byte_cost()
        .as_yoctonear()
        .checked_mul(u128::from(required_bytes))
        .and_then(|stake| stake.checked_add(1))
        .unwrap_or_else(|| env::panic_str("withdrawal journal balance budget overflow"));
    require!(
        env::account_balance().as_yoctonear() >= required_balance,
        "executor needs funded withdrawal journal storage and operating reserves"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use defuse_core::{
        engine::StateView,
        fees::{FeesConfig, Pips},
    };
    use near_sdk::{
        PromiseResult,
        json_types::U128,
        mock::with_mocked_blockchain,
        test_utils::{VMContextBuilder, get_created_receipts},
        testing_env,
    };

    fn account(name: &str) -> AccountId {
        name.parse().unwrap()
    }

    fn context(predecessor: &str, gas: u64, result: Vec<PromiseResult>) {
        testing_env!(
            VMContextBuilder::new()
                .current_account_id(account("executor.testnet"))
                .predecessor_account_id(account(predecessor))
                .attached_deposit(NearToken::from_yoctonear(1))
                .account_balance(NearToken::from_near(100))
                .prepaid_gas(Gas::from_tgas(gas))
                .build(),
            near_sdk::test_vm_config(),
            near_sdk::RuntimeFeesConfig::test(),
            Default::default(),
            result,
        );
    }

    fn config() -> DefuseConfig {
        DefuseConfig {
            wnear_id: account("wrap.testnet"),
            fees: FeesConfig { fee: Pips::ZERO, fee_collector: account("fees.testnet") },
            roles: Default::default(),
        }
    }

    fn fresh() -> Contract {
        // Tests deliberately control local storage/results; no live receipt
        // loss or intended-chain deployment is inferred from these fixtures.
        with_mocked_blockchain(|blockchain| {
            let _ = blockchain.take_storage();
        });
        context("vault.testnet", 300, vec![]);
        Contract::new_with_withdrawal_journal(
            config(),
            Hex32::from_bytes([7; 32]),
            vec![account("vault.testnet")],
        )
    }

    fn request() -> JournalWithdrawalRequest {
        JournalWithdrawalRequest {
            withdrawal_id: U64(1),
            request_seq: U64(1),
            token: account("token.testnet"),
            receiver_id: account("receiver.testnet"),
            amount: U128(20),
        }
    }

    fn balance(contract: &Contract) -> u128 {
        StateView::balance_of(
            contract,
            &account("vault.testnet"),
            &Nep141TokenId::new(account("token.testnet")).into(),
        )
    }

    fn fund(contract: &mut Contract) {
        contract
            .deposit(
                account("vault.testnet"),
                [(Nep141TokenId::new(account("token.testnet")).into(), 100)],
                None,
            )
            .unwrap();
    }

    fn pending() -> Contract {
        let mut contract = fresh();
        fund(&mut contract);
        drop(contract.withdraw_with_id(request()));
        assert_eq!(balance(&contract), 80);
        contract
    }

    fn refused(action: impl FnOnce()) {
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)).is_err());
    }

    #[test]
    fn journal_domain_and_admission_are_fresh_only_and_do_not_change_root_layout() {
        let contract = fresh();
        let root = borsh::to_vec(&contract).unwrap();
        let descriptor = contract.get_withdrawal_journal_protocol().unwrap();
        assert_eq!(descriptor.ledger_domain, Hex32::from_bytes([7; 32]));
        assert_eq!(contract.get_journal_authorized_vaults(), Some(vec![account("vault.testnet")]));
        assert_eq!(borsh::to_vec(&contract).unwrap(), root);
        refused(|| {
            drop(Contract::new_with_withdrawal_journal(
                config(),
                Hex32::from_bytes([8; 32]),
                vec![account("other.testnet")],
            ));
        });
        assert_eq!(contract.get_withdrawal_journal_protocol(), Some(descriptor));
    }

    #[test]
    fn journal_initializer_refuses_an_existing_plain_executor_without_adding_metadata() {
        with_mocked_blockchain(|blockchain| {
            let _ = blockchain.take_storage();
        });
        context("vault.testnet", 300, vec![]);
        let plain = Contract::new(config());
        env::state_write(&plain);
        let before = env::storage_read(b"STATE").unwrap();
        refused(|| {
            drop(Contract::new_with_withdrawal_journal(
                config(),
                Hex32::from_bytes([7; 32]),
                vec![account("vault.testnet")],
            ));
        });
        assert_eq!(env::storage_read(b"STATE").unwrap(), before);
        assert!(plain.get_withdrawal_journal_protocol().is_none());
        assert!(!env::storage_has_key(AUTHORIZED_VAULTS_KEY));
    }

    #[test]
    fn journal_admission_is_bounded_unique_and_canonicalized_once_at_initialization() {
        for vaults in [
            vec![],
            vec![account("vault.testnet"), account("vault.testnet")],
            vec![account("executor.testnet")],
            (0..21).map(|id| account(&format!("vault{id}.testnet"))).collect(),
        ] {
            with_mocked_blockchain(|blockchain| {
                let _ = blockchain.take_storage();
            });
            context("vault.testnet", 300, vec![]);
            refused(|| {
                drop(Contract::new_with_withdrawal_journal(
                    config(),
                    Hex32::from_bytes([7; 32]),
                    vaults,
                ));
            });
            assert!(!env::storage_has_key(DOMAIN_KEY));
            assert!(!env::storage_has_key(AUTHORIZED_VAULTS_KEY));
        }
        with_mocked_blockchain(|blockchain| {
            let _ = blockchain.take_storage();
        });
        context("vault.testnet", 300, vec![]);
        let contract = Contract::new_with_withdrawal_journal(
            config(),
            Hex32::from_bytes([7; 32]),
            vec![account("zz.testnet"), account("aa.testnet")],
        );
        assert_eq!(
            contract.get_journal_authorized_vaults(),
            Some(vec![account("aa.testnet"), account("zz.testnet")])
        );
    }

    #[test]
    fn journal_unauthorized_predecessor_cannot_debit_or_allocate_cancelled_tombstones() {
        let mut contract = fresh();
        fund(&mut contract);
        context("stranger.testnet", 300, vec![]);
        let usage = env::storage_usage();
        refused(|| {
            drop(contract.withdraw_with_id(request()));
        });
        refused(|| {
            let _ = contract.cancel_withdrawal_with_id(request());
        });
        assert_eq!(env::storage_usage(), usage);
        assert!(
            contract.get_journal_withdrawal_status(account("stranger.testnet"), U64(1)).is_none()
        );
        assert_eq!(balance(&contract), 100);
        assert!(get_created_receipts().is_empty());
    }

    #[test]
    fn journal_dispatch_is_once_and_changed_request_cannot_reuse_an_id() {
        let mut contract = pending();
        let calls: Vec<_> = get_created_receipts()
            .into_iter()
            .flat_map(|receipt| receipt.actions)
            .filter_map(|action| match action {
                near_sdk::mock::MockAction::FunctionCallWeight { method_name, .. } => {
                    Some(String::from_utf8(method_name).unwrap())
                }
                _ => None,
            })
            .collect();
        assert_eq!(calls, ["ft_transfer", "journal_resolve_withdrawal"]);
        context("vault.testnet", 30, vec![]);
        assert!(matches!(
            contract.withdraw_with_id(request()),
            PromiseOrValue::Value(JournalWithdrawalRecord {
                state: JournalWithdrawalState::Pending,
                ..
            })
        ));
        assert_eq!(
            contract.cancel_withdrawal_with_id(request()).state,
            JournalWithdrawalState::Pending
        );
        let mut changed = request();
        changed.receiver_id = account("other.testnet");
        refused(|| {
            drop(contract.withdraw_with_id(changed));
        });
        assert_eq!(balance(&contract), 80);
        assert!(get_created_receipts().is_empty());
    }

    #[test]
    fn journal_absent_cancellation_irrevocably_fences_a_late_original() {
        let mut contract = fresh();
        fund(&mut contract);
        let cancelled = contract.cancel_withdrawal_with_id(request());
        assert_eq!(cancelled.state, JournalWithdrawalState::Cancelled);
        assert!(
            matches!(contract.withdraw_with_id(request()), PromiseOrValue::Value(record) if record == cancelled)
        );
        assert_eq!(balance(&contract), 100);
        assert!(get_created_receipts().is_empty());
    }

    #[test]
    fn journal_explicit_ft_failure_refunds_once_and_persists_terminal_status() {
        let mut contract = pending();
        context("executor.testnet", 40, vec![PromiseResult::Failed]);
        let refunded = contract.journal_resolve_withdrawal(account("vault.testnet"), U64(1));
        assert_eq!(refunded.state, JournalWithdrawalState::Refunded { refunded_amount: U128(20) });
        assert_eq!(balance(&contract), 100);
        assert_eq!(contract.journal_resolve_withdrawal(account("vault.testnet"), U64(1)), refunded);
        assert_eq!(balance(&contract), 100);
        assert_eq!(
            contract.get_journal_withdrawal_status(account("vault.testnet"), U64(1)),
            Some(refunded.clone())
        );
        assert!(get_created_receipts().is_empty());
        let root = borsh::to_vec(&contract).unwrap();
        drop(contract);
        let restored: Contract = borsh::from_slice(&root).unwrap();
        assert_eq!(
            restored.get_journal_withdrawal_status(account("vault.testnet"), U64(1)),
            Some(refunded)
        );
    }

    #[test]
    fn journal_empty_ft_success_is_terminal_and_reuses_the_prepaid_record_slot() {
        let mut contract = pending();
        let key = operation_key(RECORD_PREFIX, &account("vault.testnet"), U64(1));
        let before = env::storage_read(&key).unwrap().len();
        context("executor.testnet", 40, vec![PromiseResult::Successful(vec![])]);
        let delivered = contract.journal_resolve_withdrawal(account("vault.testnet"), U64(1));
        assert_eq!(
            delivered.state,
            JournalWithdrawalState::Delivered { delivered_amount: U128(20) }
        );
        assert_eq!(env::storage_read(&key).unwrap().len(), before);
        assert_eq!(balance(&contract), 80);
        context("executor.testnet", 40, vec![PromiseResult::Failed]);
        assert_eq!(
            contract.journal_resolve_withdrawal(account("vault.testnet"), U64(1)),
            delivered
        );
        assert_eq!(balance(&contract), 80);
    }

    #[test]
    fn journal_missing_multiple_nonempty_and_oversized_success_keep_pending_without_refund() {
        for results in [
            vec![],
            vec![PromiseResult::Successful(vec![]), PromiseResult::Failed],
            vec![PromiseResult::Successful(b"0".to_vec())],
            vec![PromiseResult::Successful(vec![b'x'; MAX_JOURNAL_RESPONSE_BYTES + 1])],
        ] {
            let mut contract = pending();
            context("executor.testnet", 40, results);
            let usage = env::storage_usage();
            assert_eq!(
                contract.journal_resolve_withdrawal(account("vault.testnet"), U64(1)).state,
                JournalWithdrawalState::Pending
            );
            assert_eq!(balance(&contract), 80);
            assert_eq!(env::storage_usage(), usage);
            assert!(get_created_receipts().is_empty());
        }
    }

    #[test]
    fn journal_underfunded_gas_or_storage_refuses_before_debit_and_dispatch() {
        let mut contract = fresh();
        fund(&mut contract);
        context("vault.testnet", 99, vec![]);
        refused(|| {
            drop(contract.withdraw_with_id(request()));
        });
        assert_eq!(balance(&contract), 100);
        assert!(get_created_receipts().is_empty());
        testing_env!(
            VMContextBuilder::new()
                .current_account_id(account("executor.testnet"))
                .predecessor_account_id(account("vault.testnet"))
                .attached_deposit(NearToken::from_yoctonear(1))
                .account_balance(NearToken::from_yoctonear(1))
                .prepaid_gas(Gas::from_tgas(100))
                .build()
        );
        refused(|| {
            drop(contract.withdraw_with_id(request()));
        });
        assert_eq!(balance(&contract), 100);
        assert!(contract.get_journal_withdrawal_status(account("vault.testnet"), U64(1)).is_none());
        assert!(get_created_receipts().is_empty());
    }
}
