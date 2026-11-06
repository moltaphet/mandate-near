//! Migration from explicitly supported, unversioned legacy state layouts.
//!
//! The pre-sequence layout is from 77ebc0c; 0de5c48 appends withdrawal_seq.
//! Migration must be called privately, atomically with deployment, after the old
//! vault is paused and quiescent. Legacy state did not record an already-sent
//! withdrawal: these checks cannot prove that its receipts have completed.
//! Operators must establish that externally before upgrading, or use a fresh
//! deployment. This is not a recovery path for every historical deployment.

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};

use super::*;

/// The exact serialized field order at 77ebc0c.
#[derive(BorshDeserialize, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
struct LegacyVault {
    owner: AccountId,
    mandate: Mandate,
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
    pending_mandate: Option<PendingMandate>,
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

pub(super) fn migrate() -> Vault {
    require!(
        env::predecessor_account_id() == env::current_account_id(),
        "migration must be called by the vault itself"
    );
    let bytes = env::storage_read(b"STATE")
        .unwrap_or_else(|| env::panic_str("there is no legacy vault state to migrate"));
    require!(borsh::from_slice::<Vault>(&bytes).is_err(), "the vault state is already current");
    if let Ok(old) = borsh::from_slice::<LegacyProviderVault>(&bytes) {
        let mut vault = from_remediated(old.remediated);
        validate_oracle_mandate(&vault.mandate, old.oracle_kind);
        if let Some(pending) = &vault.pending_mandate {
            validate_oracle_mandate(&pending.mandate, old.oracle_kind);
        }
        vault.oracle_kind = old.oracle_kind;
        return vault;
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
    validate_or_panic(&legacy.mandate);
    if let Some(pending) = &legacy.pending_mandate {
        validate_or_panic(&pending.mandate);
    }
    Vault {
        owner: legacy.owner,
        mandate: legacy.mandate,
        paused: legacy.paused,
        trading: legacy.trading,
        agent: legacy.agent,
        pending: legacy.pending,
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
        pending_mandate: legacy.pending_mandate,
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

    fn context(predecessor: &str) {
        testing_env!(
            VMContextBuilder::new()
                .current_account_id("vault.testnet".parse().unwrap())
                .predecessor_account_id(predecessor.parse().unwrap())
                .block_timestamp(NOW * 1_000_000_000)
                .build()
        );
    }

    fn legacy() -> LegacyVault {
        LegacyVault {
            owner: "owner.testnet".parse().unwrap(),
            mandate: sample(),
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
            pending_mandate: Some(PendingMandate {
                mandate: Mandate { max_trade_usd: U64(75_000_000), ..sample() },
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
        assert_eq!(vault.mandate, old.mandate);
        assert_eq!(vault.paused, old.paused);
        assert_eq!(vault.trading, old.trading);
        assert_eq!(vault.agent, old.agent);
        assert_eq!(vault.pending, old.pending);
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
        assert_eq!(vault.pending_mandate, old.pending_mandate);
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
        old.pending = Some(Proposal {
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
        assert_eq!(v.mandate, old.remediated.legacy.legacy.mandate);
        assert_eq!(v.pending_mandate, old.remediated.legacy.legacy.pending_mandate);
        assert_eq!(v.trading, old.remediated.legacy.legacy.trading);
        assert_eq!(v.withdrawal_seq, 41);
        assert_eq!(v.next_withdrawal_id, 11);
        assert_eq!(v.config_revision, 7);
        assert_eq!(v.last_withdrawal_outcome, old.remediated.last_withdrawal_outcome);
        assert!(v.withdrawal_stage.is_none());
        assert!(v.withdrawal_retry_armed.is_none());
        assert!(v.active_withdrawal.is_none());
        assert_eq!(v.get_withdrawal_protocol(), "unique_voucher_v1");
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
