//! Migration from explicitly supported, unversioned legacy state layouts.
//!
//! The pre-sequence layout is from 77ebc0c; 0de5c48 appends withdrawal_seq.
//! Migration must be called privately, atomically with deployment, after the old
//! vault is paused and quiescent. Legacy state did not record an already-sent
//! withdrawal: these checks cannot prove that its receipts have completed.
//! Operators must establish that externally before upgrading, or use a fresh
//! deployment. This is not a recovery path for every historical deployment.
//! Frozen pre-model nested types keep every historical Borsh layout independent
//! of the current mandate and proposal schemas. Migrated model lists start empty.

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};

use super::*;

/// The mandate field order before any model-source admission list existed.
#[derive(Clone, Debug, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
struct PreModelMandate {
    assets: Vec<AssetConfig>,
    stable_assets: Vec<String>,
    max_trade_usd: U64,
    max_daily_turnover_usd: U64,
    max_position_bps: u16,
    max_slippage_bps: u16,
    max_drawdown_bps: u16,
    min_trade_interval_sec: u32,
    max_price_age_sec: u32,
}

impl From<PreModelMandate> for Mandate {
    fn from(old: PreModelMandate) -> Self {
        Self {
            assets: old.assets,
            stable_assets: old.stable_assets,
            max_trade_usd: old.max_trade_usd,
            max_daily_turnover_usd: old.max_daily_turnover_usd,
            max_position_bps: old.max_position_bps,
            max_slippage_bps: old.max_slippage_bps,
            max_drawdown_bps: old.max_drawdown_bps,
            min_trade_interval_sec: old.min_trade_interval_sec,
            max_price_age_sec: old.max_price_age_sec,
            allowed_model_ids: Vec::new(),
            withdrawal_limits: Vec::new(),
        }
    }
}

/// No source identifier is invented for an old proposal. Migration requires None.
#[derive(BorshDeserialize, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
struct PreModelProposal {
    id: U64,
    asset_in: String,
    asset_out: String,
    amount_in: U128,
    min_amount_out: U128,
    reason_hash: String,
    created_sec: u64,
    expires_sec: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
struct PreModelPendingMandate {
    mandate: PreModelMandate,
    effective_sec: u64,
}

impl From<PreModelPendingMandate> for PendingMandate {
    fn from(old: PreModelPendingMandate) -> Self {
        Self { mandate: old.mandate.into(), effective_sec: old.effective_sec }
    }
}

/// The exact serialized field order at 77ebc0c.
#[derive(BorshDeserialize, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
struct LegacyVault {
    owner: AccountId,
    mandate: PreModelMandate,
    paused: bool,
    trading: TradingState,
    agent: Option<AccountId>,
    pending: Option<PreModelProposal>,
    next_proposal_id: u64,
    strikes: u32,
    halted: bool,
    signer: AccountId,
    intents: AccountId,
    oracle: AccountId,
    intents_key: Option<String>,
    key_registered: bool,
    in_flight: Option<InFlight>,
    next_execution_id: u64,
    signed_intent: Option<SignedIntent>,
    pending_mandate: Option<PreModelPendingMandate>,
    pending_agent: Option<PendingAgent>,
    pending_withdrawal: Option<PendingWithdrawal>,
}

/// Borsh adds no framing for a nested struct, preserving the 0de5c48 layout.
#[derive(BorshDeserialize, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
struct LegacyVaultWithSeq {
    legacy: LegacyVault,
    withdrawal_seq: u64,
}

/// Exact remediation layout before appending the immutable oracle provider.
/// The nested structs have no Borsh framing and preserve the field order.
#[derive(BorshDeserialize, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
struct LegacyRemediatedVault {
    legacy: LegacyVaultWithSeq,
    config_revision: u64,
    execution_context: Option<ExecutionContext>,
    last_proposal_outcome: Option<ProposalOutcome>,
    active_withdrawal: Option<ActiveWithdrawal>,
    next_withdrawal_id: u64,
    last_withdrawal_outcome: Option<WithdrawalOutcome>,
}

/// Exact provider-enabled layout immediately before the unique-voucher protocol.
#[derive(BorshDeserialize, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
struct LegacyProviderVault {
    remediated: LegacyRemediatedVault,
    oracle_kind: OracleKind,
}

/// Exact b43569d voucher-enabled state before adding model-source fields.
#[derive(BorshDeserialize, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
struct LegacyVoucherVault {
    provider: LegacyProviderVault,
    withdrawal_stage: Option<WithdrawalStage>,
    withdrawal_retry_armed: Option<U64>,
}

/// Exact mandate at a582d36, after source admission and before withdrawal ceilings.
#[derive(Clone, Debug, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
struct PreCeilingMandate {
    assets: Vec<AssetConfig>,
    stable_assets: Vec<String>,
    max_trade_usd: U64,
    max_daily_turnover_usd: U64,
    max_position_bps: u16,
    max_slippage_bps: u16,
    max_drawdown_bps: u16,
    min_trade_interval_sec: u32,
    max_price_age_sec: u32,
    allowed_model_ids: Vec<String>,
}

impl From<PreCeilingMandate> for Mandate {
    fn from(old: PreCeilingMandate) -> Self {
        Self {
            assets: old.assets,
            stable_assets: old.stable_assets,
            max_trade_usd: old.max_trade_usd,
            max_daily_turnover_usd: old.max_daily_turnover_usd,
            max_position_bps: old.max_position_bps,
            max_slippage_bps: old.max_slippage_bps,
            max_drawdown_bps: old.max_drawdown_bps,
            min_trade_interval_sec: old.min_trade_interval_sec,
            max_price_age_sec: old.max_price_age_sec,
            allowed_model_ids: old.allowed_model_ids,
            withdrawal_limits: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
struct PreCeilingPendingMandate {
    mandate: PreCeilingMandate,
    effective_sec: u64,
}

/// Frozen actual Vault layout emitted by the source-admission release a582d36.
#[derive(BorshDeserialize, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
struct PreCeilingVault {
    owner: AccountId,
    mandate: PreCeilingMandate,
    paused: bool,
    trading: TradingState,
    agent: Option<AccountId>,
    pending: Option<Proposal>,
    next_proposal_id: u64,
    strikes: u32,
    halted: bool,
    signer: AccountId,
    intents: AccountId,
    oracle: AccountId,
    intents_key: Option<String>,
    key_registered: bool,
    in_flight: Option<InFlight>,
    next_execution_id: u64,
    signed_intent: Option<SignedIntent>,
    pending_mandate: Option<PreCeilingPendingMandate>,
    pending_agent: Option<PendingAgent>,
    pending_withdrawal: Option<PendingWithdrawal>,
    withdrawal_seq: u64,
    config_revision: u64,
    execution_context: Option<ExecutionContext>,
    last_proposal_outcome: Option<ProposalOutcome>,
    active_withdrawal: Option<ActiveWithdrawal>,
    next_withdrawal_id: u64,
    last_withdrawal_outcome: Option<WithdrawalOutcome>,
    oracle_kind: OracleKind,
    withdrawal_stage: Option<WithdrawalStage>,
    withdrawal_retry_armed: Option<U64>,
}

pub(super) fn migrate() -> Vault {
    require!(
        env::predecessor_account_id() == env::current_account_id(),
        "migration must be called by the vault itself"
    );
    let bytes = env::storage_read(b"STATE")
        .unwrap_or_else(|| env::panic_str("there is no legacy vault state to migrate"));
    require!(borsh::from_slice::<Vault>(&bytes).is_err(), "the vault state is already current");
    if let Ok(old) = borsh::from_slice::<PreCeilingVault>(&bytes) {
        return from_pre_ceiling(old);
    }
    if let Ok(old) = borsh::from_slice::<LegacyVoucherVault>(&bytes) {
        require!(
            old.provider.remediated.active_withdrawal.is_none(),
            "resolve the withdrawal guard before migrating"
        );
        require!(
            old.withdrawal_stage.is_none(),
            "resolve the withdrawal voucher phase before migrating"
        );
        require!(
            old.withdrawal_retry_armed.is_none(),
            "resolve the withdrawal retry authorization before migrating"
        );
        return from_provider(old.provider);
    }
    if let Ok(old) = borsh::from_slice::<LegacyProviderVault>(&bytes) {
        return from_provider(old);
    }
    if let Ok(old) = borsh::from_slice::<LegacyRemediatedVault>(&bytes) {
        return from_remediated(old);
    }
    let (legacy, withdrawal_seq) = if let Ok(old) = borsh::from_slice::<LegacyVaultWithSeq>(&bytes)
    {
        (old.legacy, old.withdrawal_seq)
    } else if let Ok(old) = borsh::from_slice::<LegacyVault>(&bytes) {
        (old, 0)
    } else {
        env::panic_str("unsupported legacy vault state layout");
    };
    from_legacy(legacy, withdrawal_seq)
}

fn from_pre_ceiling(old: PreCeilingVault) -> Vault {
    withdrawal::sinks();
    require!(
        old.owner != env::current_account_id(),
        "the legacy owner cannot be the vault account itself"
    );
    require!(old.paused, "pause the legacy vault before migrating");
    require!(old.in_flight.is_none(), "the legacy execution must finish before migrating");
    require!(old.pending.is_none(), "clear the legacy proposal before migrating");
    require!(old.pending_withdrawal.is_none(), "cancel the legacy withdrawal before migrating");
    require!(old.active_withdrawal.is_none(), "resolve the withdrawal guard before migrating");
    require!(
        old.withdrawal_stage.is_none(),
        "resolve the withdrawal voucher phase before migrating"
    );
    require!(
        old.withdrawal_retry_armed.is_none(),
        "resolve the withdrawal retry authorization before migrating"
    );
    let now = now_sec();
    require!(
        old.trading.signed_intent_expires_at_sec <= now
            && old.signed_intent.as_ref().is_none_or(|intent| intent.expires_sec <= now),
        "wait for the legacy signed intent to expire before migrating"
    );
    let mandate: Mandate = old.mandate.into();
    let pending_mandate = old.pending_mandate.map(|pending| PendingMandate {
        mandate: pending.mandate.into(),
        effective_sec: pending.effective_sec,
    });
    validate_oracle_mandate(&mandate, old.oracle_kind);
    if let Some(pending) = &pending_mandate {
        validate_oracle_mandate(&pending.mandate, old.oracle_kind);
    }
    Vault {
        owner: old.owner,
        mandate,
        paused: old.paused,
        trading: old.trading,
        agent: old.agent,
        pending: None,
        next_proposal_id: old.next_proposal_id,
        strikes: old.strikes,
        halted: old.halted,
        signer: old.signer,
        intents: old.intents,
        oracle: old.oracle,
        intents_key: old.intents_key,
        key_registered: old.key_registered,
        in_flight: None,
        next_execution_id: old.next_execution_id,
        signed_intent: old.signed_intent,
        pending_mandate,
        pending_agent: old.pending_agent,
        pending_withdrawal: None,
        withdrawal_seq: old.withdrawal_seq,
        config_revision: old.config_revision,
        // No live execution exists. Discard only its expired orphan read snapshot.
        execution_context: None,
        last_proposal_outcome: old.last_proposal_outcome,
        active_withdrawal: None,
        next_withdrawal_id: old.next_withdrawal_id,
        last_withdrawal_outcome: old.last_withdrawal_outcome,
        oracle_kind: old.oracle_kind,
        withdrawal_stage: None,
        withdrawal_retry_armed: None,
    }
}

fn from_provider(old: LegacyProviderVault) -> Vault {
    let mut vault = from_remediated(old.remediated);
    validate_oracle_mandate(&vault.mandate, old.oracle_kind);
    if let Some(pending) = &vault.pending_mandate {
        validate_oracle_mandate(&pending.mandate, old.oracle_kind);
    }
    vault.oracle_kind = old.oracle_kind;
    vault
}

fn from_remediated(old: LegacyRemediatedVault) -> Vault {
    require!(old.active_withdrawal.is_none(), "resolve the withdrawal guard before migrating");
    // An inert orphan snapshot is discarded only after from_legacy requires no
    // in-flight execution and expired signing deadlines. Preserve monotonic ids.
    let mut vault = from_legacy(old.legacy.legacy, old.legacy.withdrawal_seq);
    vault.config_revision = old.config_revision;
    vault.last_proposal_outcome = old.last_proposal_outcome;
    vault.next_withdrawal_id = old.next_withdrawal_id;
    vault.last_withdrawal_outcome = old.last_withdrawal_outcome;
    vault
}

fn from_legacy(legacy: LegacyVault, withdrawal_seq: u64) -> Vault {
    withdrawal::sinks();
    require!(
        legacy.owner != env::current_account_id(),
        "the legacy owner cannot be the vault account itself"
    );
    require!(legacy.paused, "pause the legacy vault before migrating");
    require!(legacy.in_flight.is_none(), "the legacy execution must finish before migrating");
    require!(legacy.pending.is_none(), "clear the legacy proposal before migrating");
    require!(legacy.pending_withdrawal.is_none(), "cancel the legacy withdrawal before migrating");
    let now = now_sec();
    require!(
        legacy.trading.signed_intent_expires_at_sec <= now
            && legacy.signed_intent.as_ref().is_none_or(|intent| intent.expires_sec <= now),
        "wait for the legacy signed intent to expire before migrating"
    );
    let mandate: Mandate = legacy.mandate.into();
    let pending_mandate: Option<PendingMandate> = legacy.pending_mandate.map(Into::into);
    validate_or_panic(&mandate);
    if let Some(pending) = &pending_mandate {
        validate_or_panic(&pending.mandate);
    }
    Vault {
        owner: legacy.owner,
        mandate,
        paused: legacy.paused,
        trading: legacy.trading,
        agent: legacy.agent,
        pending: None,
        next_proposal_id: legacy.next_proposal_id,
        strikes: legacy.strikes,
        halted: legacy.halted,
        signer: legacy.signer,
        intents: legacy.intents,
        oracle: legacy.oracle,
        intents_key: legacy.intents_key,
        key_registered: legacy.key_registered,
        in_flight: legacy.in_flight,
        next_execution_id: legacy.next_execution_id,
        signed_intent: legacy.signed_intent,
        pending_mandate,
        pending_agent: legacy.pending_agent,
        pending_withdrawal: legacy.pending_withdrawal,
        withdrawal_seq,
        config_revision: 0,
        execution_context: None,
        last_proposal_outcome: None,
        active_withdrawal: None,
        next_withdrawal_id: 1,
        last_withdrawal_outcome: None,
        oracle_kind: OracleKind::PriceOracle,
        withdrawal_stage: None,
        withdrawal_retry_armed: None,
    }
}

#[cfg(test)]
mod tests {
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;

    use super::*;
    use crate::mandate::tests::{USDC, WNEAR, sample};

    const NOW: u64 = 1_793_697_676;
    const GOLDEN_NOW: u64 = 1_793_448_000;
    const PRICE_ORACLE_GOLDEN: &[u8] =
        include_bytes!("../tests/fixtures/pre-model-voucher-b43569d-price-oracle-state.borsh");
    const PYTH_GOLDEN: &[u8] =
        include_bytes!("../tests/fixtures/pre-model-voucher-b43569d-pyth-state.borsh");
    const PRE_CEILING_PRICE_ORACLE: &[u8] =
        include_bytes!("../tests/fixtures/pre-ceiling-a582d36-price-oracle-state.borsh");
    const PRE_CEILING_PYTH: &[u8] =
        include_bytes!("../tests/fixtures/pre-ceiling-a582d36-pyth-state.borsh");

    fn pre_ceiling_bytes(kind: OracleKind) -> &'static [u8] {
        match kind {
            OracleKind::PriceOracle => PRE_CEILING_PRICE_ORACLE,
            OracleKind::Pyth => PRE_CEILING_PYTH,
        }
    }

    fn assert_pre_ceiling_golden(kind: OracleKind, length: usize, digest: &str) {
        context_at("vault.testnet", 0, GOLDEN_NOW);
        let bytes = pre_ceiling_bytes(kind);
        assert_eq!(bytes.len(), length, "actual pre-ceiling Vault fixture length");
        assert_eq!(to_hex(&env::sha256(bytes)), digest, "actual pre-ceiling Vault SHA-256");
        assert!(borsh::from_slice::<Vault>(bytes).is_err(), "the old state requires migration");
        let old: PreCeilingVault = borsh::from_slice(bytes).unwrap();
        env::storage_write(b"STATE", bytes);
        let vault = migrate();
        let mut expected = golden_mandate(kind);
        expected.allowed_model_ids =
            vec!["manual-v1".into(), "rebalance-v1".into(), "fixture-model".into()];
        assert_eq!(vault.mandate, expected);
        assert_eq!(vault.owner.as_str(), "dao.testnet");
        assert_eq!(vault.owner, old.owner);
        assert!(vault.paused && vault.halted);
        assert_eq!(vault.strikes, 2);
        assert_eq!(vault.trading, old.trading);
        assert_eq!(vault.agent, old.agent);
        assert!(vault.pending.is_none() && vault.in_flight.is_none());
        assert_eq!(vault.next_proposal_id, 17);
        assert_eq!(vault.next_execution_id, 23);
        assert_eq!(vault.signer, old.signer);
        assert_eq!(vault.intents, old.intents);
        assert_eq!(vault.oracle, old.oracle);
        assert_eq!(vault.intents_key, old.intents_key);
        assert_eq!(vault.key_registered, old.key_registered);
        assert_eq!(vault.signed_intent, old.signed_intent);
        let mut pending_policy = expected.clone();
        pending_policy.max_trade_usd = U64(75_000_000);
        pending_policy.allowed_model_ids =
            vec!["manual-v1".into(), "fixture-model".into(), "next-model-v1".into()];
        assert_eq!(
            vault.pending_mandate,
            Some(PendingMandate {
                mandate: pending_policy,
                effective_sec: GOLDEN_NOW + MANDATE_DELAY_SEC,
            })
        );
        assert_eq!(vault.pending_agent, old.pending_agent);
        assert!(vault.pending_withdrawal.is_none() && vault.active_withdrawal.is_none());
        assert_eq!(vault.withdrawal_seq, 41);
        assert_eq!(vault.config_revision, 7);
        assert_eq!(vault.last_proposal_outcome, old.last_proposal_outcome);
        assert_eq!(vault.next_withdrawal_id, 11);
        assert_eq!(vault.last_withdrawal_outcome, old.last_withdrawal_outcome);
        assert_eq!(vault.oracle_kind, kind);
        assert!(vault.withdrawal_stage.is_none() && vault.withdrawal_retry_armed.is_none());
        assert!(vault.execution_context.is_none(), "quiescent orphan snapshot is discarded");
        assert!(vault.mandate.withdrawal_limits.is_empty());
        assert!(vault.pending_mandate.unwrap().mandate.withdrawal_limits.is_empty());
        let encoded = borsh::to_vec(&vault.mandate).unwrap();
        assert!(borsh::from_slice::<Mandate>(&encoded).is_ok());
    }

    #[test]
    fn migration_accepts_actual_pre_ceiling_price_oracle_state() {
        assert_pre_ceiling_golden(
            OracleKind::PriceOracle,
            1345,
            "fb8ebf36d092d224ceb935b12e369031f7e400ff9dd24c86558e3d7640911dfc",
        );
    }

    #[test]
    fn migration_accepts_actual_pre_ceiling_pyth_state() {
        assert_pre_ceiling_golden(
            OracleKind::Pyth,
            1509,
            "f99060f217db651d4e7d6de24b3b92597a43e758cd0bfa4a4bf5c16366238e8f",
        );
    }

    fn assert_pre_ceiling_refused(expected: &str, change: impl FnOnce(&mut PreCeilingVault)) {
        context_at("vault.testnet", 0, GOLDEN_NOW);
        let mut old: PreCeilingVault = borsh::from_slice(PRE_CEILING_PRICE_ORACLE).unwrap();
        change(&mut old);
        let bytes = borsh::to_vec(&old).unwrap();
        env::storage_write(b"STATE", &bytes);
        let panic = match std::panic::catch_unwind(migrate) {
            Ok(_) => panic!("migration must refuse"),
            Err(panic) => panic,
        };
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied());
        assert_eq!(message, Some(expected));
        assert_eq!(env::storage_read(b"STATE"), Some(bytes), "refusal preserves old state");
    }

    #[test]
    fn pre_ceiling_migration_requires_paused_quiescent_authority() {
        assert_pre_ceiling_refused("pause the legacy vault before migrating", |old| {
            old.paused = false
        });
        assert_pre_ceiling_refused("the legacy owner cannot be the vault account itself", |old| {
            old.owner = "vault.testnet".parse().unwrap();
        });
        assert_pre_ceiling_refused("the legacy execution must finish before migrating", |old| {
            old.in_flight = Some(InFlight { id: U64(22), started_sec: GOLDEN_NOW - 1000 });
        });
        assert_pre_ceiling_refused("clear the legacy proposal before migrating", |old| {
            old.pending = Some(Proposal {
                id: U64(16),
                asset_in: USDC.into(),
                asset_out: WNEAR.into(),
                amount_in: U128(1),
                min_amount_out: U128(1),
                reason_hash: "ab".repeat(32),
                created_sec: GOLDEN_NOW - 120,
                expires_sec: GOLDEN_NOW - 60,
                model_id: "manual-v1".into(),
            });
        });
        for stored_deadline in [false, true] {
            assert_pre_ceiling_refused(
                "wait for the legacy signed intent to expire before migrating",
                |old| {
                    if stored_deadline {
                        old.signed_intent.as_mut().unwrap().expires_sec = GOLDEN_NOW + 1;
                    } else {
                        old.trading.signed_intent_expires_at_sec = GOLDEN_NOW + 1;
                    }
                },
            );
        }
    }

    #[test]
    fn pre_ceiling_migration_never_discards_withdrawal_authorization() {
        let request = PendingWithdrawal {
            asset_id: USDC.into(),
            receiver: "receiver.testnet".parse().unwrap(),
            amount: U128(20),
            effective_sec: GOLDEN_NOW,
        };
        assert_pre_ceiling_refused("cancel the legacy withdrawal before migrating", |old| {
            old.pending_withdrawal = Some(request.clone());
        });
        assert_pre_ceiling_refused("resolve the withdrawal guard before migrating", |old| {
            old.active_withdrawal = Some(ActiveWithdrawal {
                id: U64(10),
                request_seq: U64(41),
                request: request.clone(),
                started_sec: GOLDEN_NOW - 1000,
                balance_before: Some(U128(100)),
            });
        });
        assert_pre_ceiling_refused(
            "resolve the withdrawal voucher phase before migrating",
            |old| {
                old.withdrawal_stage = Some(WithdrawalStage::Dispatched);
            },
        );
        assert_pre_ceiling_refused(
            "resolve the withdrawal retry authorization before migrating",
            |old| {
                old.withdrawal_retry_armed = Some(U64(41));
            },
        );
    }

    impl From<Mandate> for PreModelMandate {
        fn from(mandate: Mandate) -> Self {
            Self {
                assets: mandate.assets,
                stable_assets: mandate.stable_assets,
                max_trade_usd: mandate.max_trade_usd,
                max_daily_turnover_usd: mandate.max_daily_turnover_usd,
                max_position_bps: mandate.max_position_bps,
                max_slippage_bps: mandate.max_slippage_bps,
                max_drawdown_bps: mandate.max_drawdown_bps,
                min_trade_interval_sec: mandate.min_trade_interval_sec,
                max_price_age_sec: mandate.max_price_age_sec,
            }
        }
    }

    fn context(predecessor: &str) {
        context_at(predecessor, 0, NOW);
    }

    fn context_at(predecessor: &str, deposit: u128, time: u64) {
        testing_env!(
            VMContextBuilder::new()
                .current_account_id("vault.testnet".parse().unwrap())
                .predecessor_account_id(predecessor.parse().unwrap())
                .attached_deposit(NearToken::from_yoctonear(deposit))
                .block_timestamp(time * 1_000_000_000)
                .build()
        );
    }

    fn legacy() -> LegacyVault {
        LegacyVault {
            owner: "owner.testnet".parse().unwrap(),
            mandate: sample().into(),
            paused: true,
            trading: TradingState {
                last_trade_at_sec: Some(NOW - 200),
                day_start_sec: NOW - NOW % engine::SECONDS_PER_DAY,
                day_turnover_usd: 12_000_000,
                high_water_mark_usd: 99_000_000,
                signed_intent_expires_at_sec: NOW,
            },
            agent: Some("agent.testnet".parse().unwrap()),
            pending: None,
            next_proposal_id: 17,
            strikes: 2,
            halted: true,
            signer: MAINNET_SIGNER.parse().unwrap(),
            intents: MAINNET_INTENTS.parse().unwrap(),
            oracle: MAINNET_ORACLE.parse().unwrap(),
            intents_key: Some("registered-public-fixture".into()),
            key_registered: true,
            in_flight: None,
            next_execution_id: 23,
            signed_intent: Some(SignedIntent {
                proposal_id: U64(16),
                payload: "expired payload fixture".into(),
                public_key: "public fixture".into(),
                signature: "expired signature fixture".into(),
                expires_sec: NOW,
            }),
            pending_mandate: Some(PreModelPendingMandate {
                mandate: PreModelMandate { max_trade_usd: U64(75_000_000), ..sample().into() },
                effective_sec: NOW + MANDATE_DELAY_SEC,
            }),
            pending_agent: Some(PendingAgent {
                agent: "replacement.testnet".parse().unwrap(),
                effective_sec: NOW + AGENT_DELAY_SEC,
            }),
            pending_withdrawal: None,
        }
    }

    fn write_legacy(old: &LegacyVault) {
        env::storage_write(b"STATE", &borsh::to_vec(old).unwrap());
    }

    fn assert_preserved(vault: &Vault, old: &LegacyVault, seq: u64) {
        assert_eq!(vault.owner, old.owner);
        assert_eq!(vault.mandate, Mandate::from(old.mandate.clone()));
        assert!(vault.mandate.allowed_model_ids.is_empty());
        assert_eq!(vault.paused, old.paused);
        assert_eq!(vault.trading, old.trading);
        assert_eq!(vault.agent, old.agent);
        assert!(vault.pending.is_none() && old.pending.is_none());
        assert_eq!(vault.next_proposal_id, old.next_proposal_id);
        assert_eq!(vault.strikes, old.strikes);
        assert_eq!(vault.halted, old.halted);
        assert_eq!(vault.signer, old.signer);
        assert_eq!(vault.intents, old.intents);
        assert_eq!(vault.oracle, old.oracle);
        assert_eq!(vault.oracle_kind, OracleKind::PriceOracle);
        assert_eq!(vault.intents_key, old.intents_key);
        assert_eq!(vault.key_registered, old.key_registered);
        assert_eq!(vault.in_flight, old.in_flight);
        assert_eq!(vault.next_execution_id, old.next_execution_id);
        assert_eq!(vault.signed_intent, old.signed_intent);
        assert_eq!(vault.pending_mandate, old.pending_mandate.clone().map(PendingMandate::from));
        assert!(vault.pending_mandate.as_ref().unwrap().mandate.allowed_model_ids.is_empty());
        assert_eq!(vault.pending_agent, old.pending_agent);
        assert_eq!(vault.pending_withdrawal, old.pending_withdrawal);
        assert_eq!(vault.withdrawal_seq, seq);
        assert_eq!(vault.config_revision, 0);
        assert!(vault.execution_context.is_none());
        assert!(vault.last_proposal_outcome.is_none());
        assert!(vault.active_withdrawal.is_none());
        assert_eq!(vault.next_withdrawal_id, 1);
        assert!(vault.last_withdrawal_outcome.is_none());
    }

    #[test]
    fn migration_preserves_the_pre_sequence_legacy_layout() {
        context("vault.testnet");
        let old = legacy();
        write_legacy(&old);
        assert_preserved(&migrate(), &old, 0);
    }

    #[test]
    fn migration_preserves_the_sequence_legacy_layout() {
        context("vault.testnet");
        let old = LegacyVaultWithSeq { legacy: legacy(), withdrawal_seq: 41 };
        env::storage_write(b"STATE", &borsh::to_vec(&old).unwrap());
        assert_preserved(&migrate(), &old.legacy, 41);
    }

    /// Expected public-method effects, independent of the migration's LegacyVault schema.
    fn assert_historical_fixture(bytes: &[u8], withdrawal_seq: u64, expected_sha256: &str) {
        context("vault.testnet");
        assert_eq!(to_hex(&env::sha256(bytes)), expected_sha256, "immutable fixture SHA-256");
        env::storage_write(b"STATE", bytes);
        let vault = migrate();
        let original_mandate = Mandate {
            assets: vec![
                AssetConfig {
                    asset_id: "nep141:wrap.near".into(),
                    decimals: 24,
                    price_feed_id: "wrap.near".into(),
                },
                AssetConfig {
                    asset_id: "nep141:usdc.testnet".into(),
                    decimals: 6,
                    price_feed_id: "usdc".into(),
                },
            ],
            stable_assets: vec!["nep141:usdc.testnet".into()],
            max_trade_usd: U64(50_000_000),
            max_daily_turnover_usd: U64(200_000_000),
            max_position_bps: 6_000,
            max_slippage_bps: 100,
            max_drawdown_bps: 1_500,
            min_trade_interval_sec: 300,
            max_price_age_sec: 60,
            allowed_model_ids: Vec::new(),
            withdrawal_limits: Vec::new(),
        };
        assert_eq!(vault.owner.as_str(), "owner.testnet");
        assert!(vault.paused);
        assert_eq!(vault.mandate, original_mandate);
        assert_eq!(vault.agent.as_ref().map(|id| id.as_str()), Some("agent.testnet"));
        assert_eq!(
            vault.pending_mandate,
            Some(PendingMandate {
                mandate: Mandate { max_trade_usd: U64(75_000_000), ..original_mandate },
                effective_sec: NOW + MANDATE_DELAY_SEC,
            })
        );
        assert_eq!(
            vault.pending_agent,
            Some(PendingAgent {
                agent: "replacement.testnet".parse().unwrap(),
                effective_sec: NOW + AGENT_DELAY_SEC,
            })
        );
        assert!(vault.pending_withdrawal.is_none());
        assert_eq!(vault.withdrawal_seq, withdrawal_seq);
        assert_eq!(vault.trading, TradingState::default());
        assert!(vault.pending.is_none());
        assert_eq!(vault.next_proposal_id, 1);
        assert_eq!(vault.next_execution_id, 1);
        assert_eq!(vault.strikes, 0);
        assert!(!vault.halted);
        assert_eq!(vault.signer.as_str(), MAINNET_SIGNER);
        assert_eq!(vault.intents.as_str(), MAINNET_INTENTS);
        assert_eq!(vault.oracle.as_str(), MAINNET_ORACLE);
        assert_eq!(vault.oracle_kind, OracleKind::PriceOracle);
        assert!(vault.intents_key.is_none());
        assert!(!vault.key_registered);
        assert!(vault.in_flight.is_none());
        assert!(vault.signed_intent.is_none());
        assert_eq!(vault.config_revision, 0);
        assert!(vault.execution_context.is_none());
        assert!(vault.last_proposal_outcome.is_none());
        assert!(vault.active_withdrawal.is_none());
        assert_eq!(vault.next_withdrawal_id, 1);
        assert!(vault.last_withdrawal_outcome.is_none());
    }

    #[test]
    fn migration_accepts_pre_sequence_state_emitted_by_the_historical_vault() {
        assert_historical_fixture(
            include_bytes!("../tests/fixtures/legacy-77ebc0c-state.borsh"),
            0,
            "3d9c70ab5bcc78ca2d28e18c9874034bdf3230376c8d3c209d5e7c0e4308c848",
        );
    }

    #[test]
    fn migration_accepts_sequence_state_emitted_by_the_historical_vault() {
        assert_historical_fixture(
            include_bytes!("../tests/fixtures/legacy-0de5c48-state.borsh"),
            2,
            "aa8735db9ac4ef54286252703d07fb13a9eab4a14adbfa7f2fbb83f9476359a7",
        );
    }

    #[test]
    #[should_panic(expected = "migration must be called by the vault itself")]
    fn migration_rejects_an_external_caller() {
        context("owner.testnet");
        write_legacy(&legacy());
        migrate();
    }

    #[test]
    #[should_panic(expected = "pause the legacy vault before migrating")]
    fn migration_rejects_an_unpaused_vault() {
        context("vault.testnet");
        let mut old = legacy();
        old.paused = false;
        write_legacy(&old);
        migrate();
    }

    #[test]
    #[should_panic(expected = "the legacy execution must finish before migrating")]
    fn migration_rejects_even_an_expired_execution_record() {
        context("vault.testnet");
        let mut old = legacy();
        old.in_flight = Some(InFlight { id: U64(22), started_sec: NOW - 1_000 });
        write_legacy(&old);
        migrate();
    }

    #[test]
    #[should_panic(expected = "clear the legacy proposal before migrating")]
    fn migration_rejects_a_pending_proposal() {
        context("vault.testnet");
        let mut old = legacy();
        old.pending = Some(PreModelProposal {
            id: U64(16),
            asset_in: USDC.into(),
            asset_out: WNEAR.into(),
            amount_in: U128(1),
            min_amount_out: U128(1),
            reason_hash: "0".repeat(64),
            created_sec: NOW - 120,
            expires_sec: NOW - 60,
        });
        write_legacy(&old);
        migrate();
    }

    #[test]
    #[should_panic(expected = "cancel the legacy withdrawal before migrating")]
    fn migration_rejects_a_queued_withdrawal() {
        context("vault.testnet");
        let mut old = legacy();
        old.pending_withdrawal = Some(PendingWithdrawal {
            asset_id: USDC.into(),
            receiver: "owner.testnet".parse().unwrap(),
            amount: U128(1),
            effective_sec: NOW + WITHDRAW_DELAY_SEC,
        });
        write_legacy(&old);
        migrate();
    }

    #[test]
    #[should_panic(expected = "wait for the legacy signed intent to expire before migrating")]
    fn migration_rejects_an_outstanding_signature() {
        context("vault.testnet");
        let mut old = legacy();
        old.trading.signed_intent_expires_at_sec = NOW + 1;
        write_legacy(&old);
        migrate();
    }

    #[test]
    #[should_panic(expected = "wait for the legacy signed intent to expire before migrating")]
    fn migration_checks_the_stored_signature_deadline_too() {
        context("vault.testnet");
        let mut old = legacy();
        old.signed_intent.as_mut().unwrap().expires_sec = NOW + 1;
        write_legacy(&old);
        migrate();
    }

    #[test]
    #[should_panic(expected = "the legacy owner cannot be the vault account itself")]
    fn migration_rejects_a_self_owned_legacy_vault() {
        context("vault.testnet");
        let mut old = legacy();
        old.owner = "vault.testnet".parse().unwrap();
        write_legacy(&old);
        migrate();
    }

    #[test]
    #[should_panic(expected = "unsupported legacy vault state layout")]
    fn migration_rejects_unknown_state() {
        context("vault.testnet");
        env::storage_write(b"STATE", b"unsupported state fixture");
        migrate();
    }

    #[test]
    #[should_panic(expected = "the vault state is already current")]
    fn migration_rejects_current_state() {
        context("vault.testnet");
        let vault = Vault::new("owner.testnet".parse().unwrap(), sample());
        env::storage_write(b"STATE", &borsh::to_vec(&vault).unwrap());
        migrate();
    }

    #[test]
    fn migration_preserves_the_quiescent_pre_provider_remediation_state() {
        context("vault.testnet");
        let old = LegacyRemediatedVault {
            legacy: LegacyVaultWithSeq { legacy: legacy(), withdrawal_seq: 41 },
            config_revision: 7,
            execution_context: None,
            last_proposal_outcome: Some(ProposalOutcome {
                proposal_id: U64(16),
                outcome: ExecutionOutcome::Rejected {
                    code: "TradeTooLarge".into(),
                    reason: "fixture rejection".into(),
                },
            }),
            active_withdrawal: None,
            next_withdrawal_id: 11,
            last_withdrawal_outcome: Some(WithdrawalOutcome::Delivered {
                withdrawal_id: U64(10),
                amount: U128(20),
            }),
        };
        env::storage_write(b"STATE", &borsh::to_vec(&old).unwrap());
        let upgraded = migrate();
        assert_eq!(upgraded.owner, old.legacy.legacy.owner);
        assert_eq!(upgraded.trading, old.legacy.legacy.trading);
        assert_eq!(upgraded.withdrawal_seq, 41);
        assert_eq!(upgraded.config_revision, 7);
        assert_eq!(upgraded.last_proposal_outcome, old.last_proposal_outcome);
        assert_eq!(upgraded.last_withdrawal_outcome, old.last_withdrawal_outcome);
        assert_eq!(upgraded.next_withdrawal_id, 11);
        assert_eq!(upgraded.oracle_kind, OracleKind::PriceOracle);
        assert!(upgraded.mandate.allowed_model_ids.is_empty());
        assert!(upgraded.pending_mandate.unwrap().mandate.allowed_model_ids.is_empty());
    }

    #[test]
    #[should_panic(expected = "resolve the withdrawal guard before migrating")]
    fn migration_never_discards_a_pre_provider_withdrawal_guard() {
        context("vault.testnet");
        let old = LegacyRemediatedVault {
            legacy: LegacyVaultWithSeq { legacy: legacy(), withdrawal_seq: 41 },
            config_revision: 7,
            execution_context: None,
            last_proposal_outcome: None,
            active_withdrawal: Some(ActiveWithdrawal {
                id: U64(10),
                request_seq: U64(41),
                request: PendingWithdrawal {
                    asset_id: USDC.into(),
                    receiver: "owner.testnet".parse().unwrap(),
                    amount: U128(20),
                    effective_sec: NOW,
                },
                started_sec: NOW,
                balance_before: Some(U128(100)),
            }),
            next_withdrawal_id: 11,
            last_withdrawal_outcome: None,
        };
        env::storage_write(b"STATE", &borsh::to_vec(&old).unwrap());
        migrate();
    }

    #[test]
    fn migration_discards_an_inert_snapshot_and_refuses_its_stale_callback() {
        context("vault.testnet");
        let old = LegacyRemediatedVault {
            legacy: LegacyVaultWithSeq { legacy: legacy(), withdrawal_seq: 41 },
            config_revision: 7,
            execution_context: Some(ExecutionContext {
                id: U64(22),
                revision: 7,
                assets: sample().assets,
                proposal_id: U64(16),
                price_valid_until_sec: Some(NOW - 1),
            }),
            last_proposal_outcome: None,
            active_withdrawal: None,
            next_withdrawal_id: 11,
            last_withdrawal_outcome: None,
        };
        assert!(old.legacy.legacy.in_flight.is_none());
        env::storage_write(b"STATE", &borsh::to_vec(&old).unwrap());
        let mut upgraded = migrate();
        assert!(upgraded.execution_context.is_none());
        assert_eq!(upgraded.trading, old.legacy.legacy.trading);
        assert_eq!(upgraded.next_execution_id, old.legacy.legacy.next_execution_id);
        let proposal = Proposal {
            id: U64(16),
            asset_in: USDC.into(),
            asset_out: WNEAR.into(),
            amount_in: U128(20_000_000),
            min_amount_out: U128(4 * 10_u128.pow(24)),
            reason_hash: "ab".repeat(32),
            created_sec: NOW - 120,
            expires_sec: NOW - 60,
            model_id: "manual-v1".into(),
        };
        let outcome =
            upgraded.on_salt(Err(PromiseError::Failed), proposal, TradingState::default(), U64(22));
        assert!(
            matches!(outcome, PromiseOrValue::Value(ExecutionOutcome::Failed { reason }) if reason == "superseded by a newer execution")
        );
        assert_eq!(upgraded.trading, old.legacy.legacy.trading);
        assert!(upgraded.in_flight.is_none());
        assert!(upgraded.pending.is_none());
    }

    #[test]
    #[should_panic(expected = "mandate must allow at least one asset")]
    fn migration_revalidates_the_legacy_mandate() {
        context("vault.testnet");
        let mut old = legacy();
        old.mandate.assets.clear();
        write_legacy(&old);
        migrate();
    }

    #[test]
    #[should_panic(expected = "invalid asset id: nep245:mt.near:asset")]
    fn migration_revalidates_a_queued_mandate() {
        context("vault.testnet");
        let mut old = legacy();
        // The legacy validator accepted NEP245; the new recovery surface does not.
        old.pending_mandate.as_mut().unwrap().mandate.assets[0].asset_id =
            "nep245:mt.near:asset".into();
        write_legacy(&old);
        migrate();
    }

    fn provider_legacy() -> LegacyProviderVault {
        let mut source = legacy();
        source.mandate.assets[0].price_feed_id = "11".repeat(32);
        source.mandate.assets[1].price_feed_id = "22".repeat(32);
        source.pending_mandate.as_mut().unwrap().mandate.assets = source.mandate.assets.clone();
        LegacyProviderVault {
            remediated: LegacyRemediatedVault {
                legacy: LegacyVaultWithSeq { legacy: source, withdrawal_seq: 41 },
                config_revision: 7,
                execution_context: None,
                last_proposal_outcome: None,
                active_withdrawal: None,
                next_withdrawal_id: 11,
                last_withdrawal_outcome: Some(WithdrawalOutcome::DebitObserved {
                    withdrawal_id: U64(10),
                }),
            },
            oracle_kind: OracleKind::Pyth,
        }
    }

    #[test]
    fn quiescent_provider_migration_preserves_identity_and_starts_no_voucher_attempt() {
        context("vault.testnet");
        let old = provider_legacy();
        env::storage_write(b"STATE", &borsh::to_vec(&old).unwrap());
        let v = migrate();
        assert_eq!(v.oracle_kind, OracleKind::Pyth);
        assert_eq!(v.mandate, Mandate::from(old.remediated.legacy.legacy.mandate.clone()));
        assert_eq!(
            v.pending_mandate,
            old.remediated.legacy.legacy.pending_mandate.clone().map(PendingMandate::from)
        );
        assert_eq!(v.trading, old.remediated.legacy.legacy.trading);
        assert_eq!(v.withdrawal_seq, 41);
        assert_eq!(v.next_withdrawal_id, 11);
        assert_eq!(v.config_revision, 7);
        assert_eq!(v.last_withdrawal_outcome, old.remediated.last_withdrawal_outcome);
        assert!(v.withdrawal_stage.is_none());
        assert!(v.withdrawal_retry_armed.is_none());
        assert!(v.active_withdrawal.is_none());
        assert_eq!(v.get_withdrawal_protocol(), "unique_voucher_v1");
        assert!(v.mandate.allowed_model_ids.is_empty());
        assert!(v.pending_mandate.as_ref().unwrap().mandate.allowed_model_ids.is_empty());
    }

    #[test]
    #[should_panic(expected = "resolve the withdrawal guard before migrating")]
    fn provider_migration_cannot_discard_an_old_unknown_withdrawal() {
        context("vault.testnet");
        let mut old = provider_legacy();
        old.remediated.active_withdrawal = Some(ActiveWithdrawal {
            id: U64(10),
            request_seq: U64(41),
            started_sec: NOW,
            balance_before: Some(U128(100)),
            request: PendingWithdrawal {
                asset_id: USDC.into(),
                receiver: "owner.testnet".parse().unwrap(),
                amount: U128(20),
                effective_sec: NOW,
            },
        });
        env::storage_write(b"STATE", &borsh::to_vec(&old).unwrap());
        migrate();
    }

    /// Expected values come from the actual old Vault exporter, not its decoder.
    fn golden_mandate(kind: OracleKind) -> Mandate {
        Mandate {
            assets: vec![
                AssetConfig {
                    asset_id: WNEAR.into(),
                    decimals: 24,
                    price_feed_id: if kind == OracleKind::Pyth {
                        "11".repeat(32)
                    } else {
                        "wrap.near".into()
                    },
                },
                AssetConfig {
                    asset_id: USDC.into(),
                    decimals: 6,
                    price_feed_id: if kind == OracleKind::Pyth {
                        "22".repeat(32)
                    } else {
                        "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1".into()
                    },
                },
            ],
            stable_assets: vec![USDC.into()],
            max_trade_usd: U64(50_000_000),
            max_daily_turnover_usd: U64(200_000_000),
            max_position_bps: 6_000,
            max_slippage_bps: 100,
            max_drawdown_bps: 1_500,
            min_trade_interval_sec: 300,
            max_price_age_sec: 60,
            allowed_model_ids: Vec::new(),
            withdrawal_limits: Vec::new(),
        }
    }

    fn assert_voucher_golden(kind: OracleKind, bytes: &[u8], length: usize, digest: &str) -> Vault {
        context_at("vault.testnet", 0, GOLDEN_NOW);
        assert_eq!(bytes.len(), length, "immutable actual-old-Vault fixture length");
        assert_eq!(to_hex(&env::sha256(bytes)), digest, "immutable actual-old-Vault SHA-256");
        env::storage_write(b"STATE", bytes);
        let mut vault = migrate();
        let mandate = golden_mandate(kind);
        assert_eq!(vault.owner.as_str(), "dao.testnet");
        assert_eq!(vault.mandate, mandate);
        for source in ["manual-v1", "rebalance-v1", "fixture-model"] {
            assert!(!vault.mandate.allows_model_id(source));
        }
        assert!(vault.paused && vault.halted);
        assert_eq!(vault.strikes, 2);
        let trading = TradingState {
            last_trade_at_sec: Some(GOLDEN_NOW - 200),
            day_start_sec: GOLDEN_NOW - GOLDEN_NOW % engine::SECONDS_PER_DAY,
            day_turnover_usd: 12_000_000,
            high_water_mark_usd: 99_000_000,
            signed_intent_expires_at_sec: GOLDEN_NOW,
        };
        assert_eq!(vault.trading, trading);
        assert_eq!(vault.agent.as_ref().unwrap().as_str(), "agent.testnet");
        assert!(vault.pending.is_none() && vault.in_flight.is_none());
        assert_eq!(vault.next_proposal_id, 17);
        assert_eq!(vault.next_execution_id, 23);
        assert_eq!(vault.signer.as_str(), "signer.testnet");
        assert_eq!(vault.intents.as_str(), "intents.testnet");
        assert_eq!(
            vault.oracle.as_str(),
            if kind == OracleKind::Pyth { "pyth-oracle.testnet" } else { "price-oracle.testnet" }
        );
        assert_eq!(vault.oracle_kind, kind);
        assert_eq!(vault.intents_key.as_deref(), Some("registered-public-fixture"));
        assert!(vault.key_registered);
        assert_eq!(
            vault.signed_intent,
            Some(SignedIntent {
                proposal_id: U64(16),
                payload: "expired public payload fixture".into(),
                public_key: "public fixture".into(),
                signature: "expired public signature fixture".into(),
                expires_sec: GOLDEN_NOW,
            })
        );
        assert_eq!(
            vault.pending_mandate,
            Some(PendingMandate {
                mandate: Mandate { max_trade_usd: U64(75_000_000), ..mandate },
                effective_sec: GOLDEN_NOW + MANDATE_DELAY_SEC,
            })
        );
        assert_eq!(
            vault.pending_agent,
            Some(PendingAgent {
                agent: "replacement.testnet".parse().unwrap(),
                effective_sec: GOLDEN_NOW + AGENT_DELAY_SEC,
            })
        );
        assert!(vault.pending_withdrawal.is_none() && vault.active_withdrawal.is_none());
        assert!(vault.withdrawal_stage.is_none() && vault.withdrawal_retry_armed.is_none());
        assert_eq!(vault.withdrawal_seq, 41);
        assert_eq!(vault.next_withdrawal_id, 11);
        assert_eq!(vault.config_revision, 7);
        assert_eq!(
            vault.last_proposal_outcome,
            Some(ProposalOutcome {
                proposal_id: U64(16),
                outcome: ExecutionOutcome::Rejected {
                    code: "TradeTooLarge".into(),
                    reason: "public fixture rejection".into(),
                },
            })
        );
        assert_eq!(
            vault.last_withdrawal_outcome,
            Some(WithdrawalOutcome::DebitObserved { withdrawal_id: U64(10) })
        );
        assert!(vault.execution_context.is_none(), "expired orphan snapshot is discarded");
        let old_proposal = Proposal {
            id: U64(16),
            asset_in: USDC.into(),
            asset_out: WNEAR.into(),
            amount_in: U128(20_000_000),
            min_amount_out: U128(4 * 10_u128.pow(24)),
            reason_hash: "ab".repeat(32),
            created_sec: GOLDEN_NOW - 120,
            expires_sec: GOLDEN_NOW - 60,
            model_id: "manual-v1".into(),
        };
        assert!(matches!(
            vault.on_salt(Err(PromiseError::Failed), old_proposal, TradingState::default(), U64(22)),
            PromiseOrValue::Value(ExecutionOutcome::Failed { reason })
                if reason == "superseded by a newer execution"
        ));
        assert_eq!(vault.trading, trading);
        assert_eq!(vault.next_execution_id, 23);
        assert!(vault.pending.is_none() && vault.in_flight.is_none());
        vault
    }

    #[test]
    fn migration_accepts_actual_pre_model_price_oracle_voucher_state() {
        assert_voucher_golden(
            OracleKind::PriceOracle,
            PRICE_ORACLE_GOLDEN,
            1244,
            "dd0fdd89250e4e8d69d36f53c647e8e868d0a28df4e5653dc21ac39bd5d3d8a4",
        );
    }

    #[test]
    fn migration_accepts_actual_pre_model_pyth_voucher_state() {
        assert_voucher_golden(
            OracleKind::Pyth,
            PYTH_GOLDEN,
            1408,
            "3aad251192859840065506be36d655119adaa2e998749f10a9d118fea02ed892",
        );
    }

    #[test]
    fn a_migrated_queued_mandate_stays_empty_and_model_addition_waits_another_day() {
        let mut vault = assert_voucher_golden(
            OracleKind::Pyth,
            PYTH_GOLDEN,
            1408,
            "3aad251192859840065506be36d655119adaa2e998749f10a9d118fea02ed892",
        );
        let original_maturity = GOLDEN_NOW + MANDATE_DELAY_SEC;
        context_at("caller.testnet", 0, original_maturity);
        vault.apply_pending_mandate();
        assert_eq!(vault.mandate.max_trade_usd, U64(75_000_000));
        assert!(vault.mandate.allowed_model_ids.is_empty());
        let mut enabled = vault.mandate.clone();
        enabled.allowed_model_ids = vec!["manual-v1".into()];
        context_at("dao.testnet", 1, original_maturity);
        assert_eq!(
            vault.set_mandate(enabled.clone()),
            MandateChangeOutcome::Queued {
                effective_sec: U64(original_maturity + MANDATE_DELAY_SEC)
            }
        );
        assert!(vault.mandate.allowed_model_ids.is_empty());
        assert_eq!(vault.pending_mandate.as_ref().unwrap().mandate, enabled);
        context_at("caller.testnet", 0, original_maturity + MANDATE_DELAY_SEC);
        vault.apply_pending_mandate();
        assert_eq!(vault.mandate.allowed_model_ids, vec!["manual-v1"]);
        context_at("dao.testnet", 1, original_maturity + MANDATE_DELAY_SEC);
        vault.unpause();
        vault.resume_proposals();
        context_at("agent.testnet", 0, original_maturity + MANDATE_DELAY_SEC);
        assert_eq!(
            vault.propose(
                USDC.into(),
                WNEAR.into(),
                U128(20_000_000),
                U128(4 * 10_u128.pow(24)),
                "ab".repeat(32),
                "manual-v1".into(),
            ),
            U64(17)
        );
        assert_eq!(vault.pending.as_ref().unwrap().model_id, "manual-v1");
    }

    #[test]
    #[should_panic(expected = "the declared model id is not allowed")]
    fn a_migrated_vault_refuses_reserved_sources_without_an_owner_addition() {
        let mut vault = assert_voucher_golden(
            OracleKind::Pyth,
            PYTH_GOLDEN,
            1408,
            "3aad251192859840065506be36d655119adaa2e998749f10a9d118fea02ed892",
        );
        context_at("dao.testnet", 1, GOLDEN_NOW);
        vault.unpause();
        vault.resume_proposals();
        context_at("agent.testnet", 0, GOLDEN_NOW);
        vault.propose(
            USDC.into(),
            WNEAR.into(),
            U128(20_000_000),
            U128(4 * 10_u128.pow(24)),
            "ab".repeat(32),
            "manual-v1".into(),
        );
    }

    #[test]
    #[should_panic(expected = "the mandate change is still in its delay")]
    fn a_migrated_model_addition_cannot_apply_one_second_before_its_delay() {
        let mut vault = assert_voucher_golden(
            OracleKind::Pyth,
            PYTH_GOLDEN,
            1408,
            "3aad251192859840065506be36d655119adaa2e998749f10a9d118fea02ed892",
        );
        let mut enabled = vault.mandate.clone();
        enabled.allowed_model_ids = vec!["manual-v1".into()];
        context_at("dao.testnet", 1, GOLDEN_NOW);
        assert_eq!(
            vault.set_mandate(enabled),
            MandateChangeOutcome::Queued { effective_sec: U64(GOLDEN_NOW + MANDATE_DELAY_SEC) }
        );
        context_at("caller.testnet", 0, GOLDEN_NOW + MANDATE_DELAY_SEC - 1);
        vault.apply_pending_mandate();
    }

    fn voucher_legacy() -> LegacyVoucherVault {
        borsh::from_slice(PYTH_GOLDEN).unwrap()
    }

    fn write_voucher(old: &LegacyVoucherVault) {
        env::storage_write(b"STATE", &borsh::to_vec(old).unwrap());
    }

    #[test]
    #[should_panic(expected = "resolve the withdrawal guard before migrating")]
    fn model_migration_never_discards_an_actual_pre_model_withdrawal_guard() {
        context("vault.testnet");
        let mut old = voucher_legacy();
        old.provider.remediated.active_withdrawal = Some(ActiveWithdrawal {
            id: U64(10),
            request_seq: U64(41),
            request: PendingWithdrawal {
                asset_id: USDC.into(),
                receiver: "dao.testnet".parse().unwrap(),
                amount: U128(20),
                effective_sec: GOLDEN_NOW,
            },
            started_sec: GOLDEN_NOW,
            balance_before: Some(U128(100)),
        });
        old.withdrawal_stage = Some(WithdrawalStage::Dispatched);
        write_voucher(&old);
        migrate();
    }

    #[test]
    #[should_panic(expected = "resolve the withdrawal voucher phase before migrating")]
    fn model_migration_refuses_an_orphaned_voucher_phase() {
        context("vault.testnet");
        let mut old = voucher_legacy();
        old.withdrawal_stage = Some(WithdrawalStage::Preparing);
        write_voucher(&old);
        migrate();
    }

    #[test]
    #[should_panic(expected = "resolve the withdrawal retry authorization before migrating")]
    fn model_migration_refuses_an_unresolved_retry_authorization() {
        context("vault.testnet");
        let mut old = voucher_legacy();
        old.withdrawal_retry_armed = Some(U64(41));
        write_voucher(&old);
        migrate();
    }

    #[test]
    #[should_panic(expected = "withdrawal recovery requires a vault name of at most 62 bytes")]
    fn migration_refuses_an_unusable_recovery_namespace() {
        testing_env!(
            VMContextBuilder::new()
                .current_account_id("a".repeat(63).parse().unwrap())
                .predecessor_account_id("a".repeat(63).parse().unwrap())
                .block_timestamp(NOW * 1_000_000_000)
                .build()
        );
        write_legacy(&legacy());
        migrate();
    }
}
