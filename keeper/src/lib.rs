//! Permissionless keeper for the Mandate vault.
//!
//! Each tick does at most one thing:
//!
//! - If a withdrawal is active, it waits or reconciles that exact attempt.
//! - If a proposal is pending and has not expired, it calls `execute`. The
//!   vault checks every gate itself, so the keeper cannot change the outcome.
//! - If a signed intent exists that has not been reported yet, it reports it
//!   once, so the caller can publish it to the solver relay.
//!
//! The keeper's account only pays for the transaction. It holds no authority
//! over funds, and a compromised keeper can at most delay trades.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mandate_agent::transport::{JsonRpc, PublicReader, validate_public_path};
use near_api::{AccountId, Contract, NearGas, NetworkConfig, Signer};
use near_sdk::serde_json::{self, Value, json};
use serde::{Deserialize, Serialize};

pub mod publication;

pub type KeeperError = Box<dyn std::error::Error + Send + Sync>;

/// No new automatic signing is safe until the retained operation is reconciled.
/// In particular, callers of the one-step `tick` API must never retry this error.
#[derive(Debug)]
pub struct PaidOutcomeUnknown;

impl std::fmt::Display for PaidOutcomeUnknown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("paid keeper outcome unknown; stop signing and reconcile before retry")
    }
}

impl std::error::Error for PaidOutcomeUnknown {}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RecoveryState {
    protocol: String,
    withdrawal_id: String,
    stage: RecoveryStage,
    started_sec: u64,
    recover_after_sec: u64,
    required_gas_tgas: u64,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
enum RecoveryStage {
    Preparing,
    Dispatched,
}

impl RecoveryState {
    fn decode(value: Value, now: u64) -> Result<Option<Self>, KeeperError> {
        if value.is_null() {
            return Ok(None);
        }
        let state: Self = serde_json::from_value(value).map_err(|_| "invalid recovery snapshot")?;
        canonical_id(&state.withdrawal_id)?;
        let supported_protocol = matches!(
            state.protocol.as_str(),
            "unique_voucher_v1" | "durable_withdrawal_journal_v1"
        );
        if !supported_protocol
            || state.required_gas_tgas != 140
            || state.started_sec > now
            || state.recover_after_sec != state.started_sec.saturating_add(300)
        {
            return Err("unsupported or inconsistent recovery snapshot".into());
        }
        Ok(Some(state))
    }
}

fn canonical_id(id: &str) -> Result<(), KeeperError> {
    let parsed = id.parse::<u64>().map_err(|_| "invalid keeper attempt identifier")?;
    if parsed == 0 || parsed.to_string() != id {
        return Err("invalid keeper attempt identifier".into());
    }
    Ok(())
}

/// In-memory cooldown; a changed withdrawal ID starts a new observation window.
#[derive(Default)]
pub struct KeeperState {
    unresolved_recovery: Option<(String, Instant)>,
}

impl KeeperState {
    fn cooling_down(&mut self, id: &str) -> bool {
        if let Some((previous, started)) = &self.unresolved_recovery
            && previous == id
        {
            return started.elapsed() < Duration::from_secs(30);
        }
        self.unresolved_recovery = None;
        false
    }
}

/// Public context only. Signed transaction bytes and private material are never stored.
#[derive(Serialize)]
struct PaidOperation<'a> {
    version: u64,
    operation: &'a str,
    attempt_id: &'a str,
    transaction_hash: String,
    vault: &'a str,
    account: &'a str,
    expected_chain_id: Option<&'a str>,
}

/// CLI single-writer exclusion and a durable conservative restart fence.
/// An existing operation, even malformed, requires operator reconciliation.
pub struct OperationJournal {
    directory: PathBuf,
    _lock: File,
}

impl OperationJournal {
    pub fn acquire(directory: &Path) -> Result<Self, KeeperError> {
        validate_public_path(directory)?;
        let resolved = directory.canonicalize().map_err(|_| "keeper outbox is unavailable")?;
        validate_public_path(&resolved)?;
        if resolved != directory || !std::fs::symlink_metadata(directory)?.is_dir() {
            return Err("keeper journal requires an unambiguous canonical directory".into());
        }
        let lock_path = resolved.join("keeper.lock");
        if std::fs::symlink_metadata(&lock_path)
            .is_ok_and(|metadata| !metadata.file_type().is_file())
        {
            return Err("refused non-regular keeper lock".into());
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|_| "keeper lock is unavailable")?;
        let path_metadata = std::fs::symlink_metadata(&lock_path)?;
        if !path_metadata.file_type().is_file() {
            return Err("refused non-regular keeper lock".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = lock.metadata()?;
            if metadata.dev() != path_metadata.dev() || metadata.ino() != path_metadata.ino() {
                return Err("keeper lock path changed while opening".into());
            }
            if metadata.nlink() != 1 {
                return Err("refused keeper lock with multiple filesystem names".into());
            }
        }
        lock.try_lock().map_err(|_| "another keeper owns this outbox")?;
        let journal = Self { directory: resolved, _lock: lock };
        journal.ensure_clear()?;
        Ok(journal)
    }

    fn path(&self) -> PathBuf {
        self.directory.join("keeper-operation.json")
    }

    fn ensure_clear(&self) -> Result<(), KeeperError> {
        let path_metadata = std::fs::symlink_metadata(self.directory.join("keeper.lock"))?;
        if !path_metadata.file_type().is_file() {
            return Err("keeper lock path changed; stop signing".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = self._lock.metadata()?;
            if metadata.dev() != path_metadata.dev()
                || metadata.ino() != path_metadata.ino()
                || metadata.nlink() != 1
            {
                return Err("keeper lock path changed; stop signing".into());
            }
        }
        match std::fs::symlink_metadata(self.path()) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => {
                Err("retained keeper operation requires operator reconciliation before signing"
                    .into())
            }
        }
    }

    fn record(&self, operation: &PaidOperation<'_>) -> Result<(), KeeperError> {
        let bytes = serde_json::to_vec_pretty(operation)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.path())
            .map_err(|_| "keeper operation could not be fenced; stop signing")?;
        file.write_all(&bytes)
            .map_err(|_| "keeper operation could not be recorded; stop signing")?;
        file.sync_all().map_err(|_| "keeper operation could not be synced; stop signing")?;
        File::open(&self.directory)?.sync_all()?;
        Ok(())
    }

    fn clear(&self) -> Result<(), KeeperError> {
        std::fs::remove_file(self.path())?;
        File::open(&self.directory)?.sync_all()?;
        Ok(())
    }
}

pub struct Keeper {
    pub network: NetworkConfig,
    pub vault: AccountId,
    /// Explicit operator network pin. None checks identity without a chain pin.
    pub expected_chain_id: Option<String>,
    /// The account that pays for `execute`.
    pub account: AccountId,
    pub signer: Arc<Signer>,
}

#[derive(Debug, PartialEq)]
pub enum Tick {
    /// Nothing to do.
    Idle,
    /// Every gate passed and the vault signed an intent. This is not settlement.
    Executed(Value),
    /// A gate rejected the proposal.
    Rejected(Value),
    /// The transaction succeeded, but an external read or signing step failed.
    Failed(Value),
    /// The vault cannot execute now; no transaction was submitted.
    NotReady { reason: &'static str },
    /// A proposal is pending but has expired. The agent must propose again.
    Expired,
    /// A signed intent that has not been reported yet. Publish it to the solver relay.
    Publish(Value),
    /// Recovery resolved a guard or retained it; neither result proves delivery.
    Recovery { withdrawal_id: String, resolved: bool },
    /// The same active attempt is immature or cooling down; no transaction was sent.
    RecoveryWaiting { withdrawal_id: String, reason: &'static str },
}

impl Keeper {
    pub async fn preflight(&self) -> Result<(), KeeperError> {
        preflight_network(&self.network, self.vault.as_str(), self.expected_chain_id.as_deref())
            .await
    }

    /// Runs one manual step without a durable journal or cross-tick cooldown.
    /// A `PaidOutcomeUnknown` error MUST NOT be retried, including after restart.
    /// Automatic callers use `tick_guarded` and retain its journal and state.
    pub async fn tick(&self, last_reported: Option<&str>) -> Result<Tick, KeeperError> {
        self.tick_inner(last_reported, &mut KeeperState::default(), None).await
    }

    pub async fn tick_guarded(
        &self,
        last_reported: Option<&str>,
        state: &mut KeeperState,
        journal: &OperationJournal,
    ) -> Result<Tick, KeeperError> {
        journal.ensure_clear()?;
        self.tick_inner(last_reported, state, Some(journal)).await
    }

    async fn tick_inner(
        &self,
        last_reported: Option<&str>,
        state: &mut KeeperState,
        journal: Option<&OperationJournal>,
    ) -> Result<Tick, KeeperError> {
        // Read every gate from one final block on a checked endpoint.
        let (reader, now, block) = self.read_context().await?;

        // An already issued signature remains publishable even if the owner has paused
        // new execution. Check it before readiness and pending-proposal state.
        let signed = self.view(&reader, &block, "get_signed_intent").await?;
        let fresh = signed["expires_sec"].as_u64().is_some_and(|e| e > now);
        let already = signed["payload"].as_str() == last_reported;
        if !signed.is_null() && fresh && !already {
            return Ok(Tick::Publish(signed));
        }

        let recovery = RecoveryState::decode(
            self.view(&reader, &block, "get_withdrawal_recovery_state").await?,
            now,
        )?;
        if let Some(recovery) = recovery {
            let id = recovery.withdrawal_id;
            if now < recovery.recover_after_sec {
                return Ok(Tick::RecoveryWaiting {
                    withdrawal_id: id,
                    reason: "recovery window has not passed",
                });
            }
            if state.cooling_down(&id) {
                return Ok(Tick::RecoveryWaiting {
                    withdrawal_id: id,
                    reason: "unresolved recovery cooldown",
                });
            }
            let result = self
                .paid_call(
                    "recover_withdrawal_attempt",
                    &id,
                    json!({"withdrawal_id":id}),
                    140,
                    journal,
                )
                .await?;
            let Some(resolved) = result.as_bool() else {
                return Err(PaidOutcomeUnknown.into());
            };
            if let Some(journal) = journal {
                journal.clear().map_err(|_| PaidOutcomeUnknown)?;
            }
            state.unresolved_recovery =
                if resolved { None } else { Some((id.clone(), Instant::now())) };
            return Ok(Tick::Recovery { withdrawal_id: id, resolved });
        }
        state.unresolved_recovery = None;

        let pending = self.view(&reader, &block, "get_pending_proposal").await?;
        if !pending.is_null() {
            if self
                .view(&reader, &block, "is_in_flight")
                .await?
                .as_bool()
                .ok_or("invalid execution status")?
            {
                return Ok(Tick::NotReady { reason: "an execution is in progress" });
            }
            let expires = pending["expires_sec"].as_u64().ok_or("proposal has no expiry")?;
            if now >= expires {
                return Ok(Tick::Expired);
            }
            if self
                .view(&reader, &block, "is_paused")
                .await?
                .as_bool()
                .ok_or("invalid pause status")?
            {
                return Ok(Tick::NotReady { reason: "the vault is paused" });
            }
            if self
                .view(&reader, &block, "is_halted")
                .await?
                .as_bool()
                .ok_or("invalid halt status")?
            {
                return Ok(Tick::NotReady { reason: "the circuit breaker has halted proposals" });
            }
            if !self
                .view(&reader, &block, "is_key_registered")
                .await?
                .as_bool()
                .ok_or("invalid key status")?
            {
                return Ok(Tick::NotReady { reason: "the intents key is not registered" });
            }
            let id = pending["id"].as_str().ok_or("proposal has no identifier")?;
            canonical_id(id)?;
            let outcome = self.paid_call("execute", id, json!({}), 300, journal).await?;
            if outcome.get("Executed").is_some() && outcome["Executed"]["proposal_id"] != id {
                return Err(PaidOutcomeUnknown.into());
            }
            let tick = execution_tick(outcome).map_err(|_| PaidOutcomeUnknown)?;
            if let Some(journal) = journal {
                journal.clear().map_err(|_| PaidOutcomeUnknown)?;
            }
            return Ok(tick);
        }
        Ok(Tick::Idle)
    }

    async fn read_context(&self) -> Result<(PublicReader, u64, String), KeeperError> {
        let first = self
            .network
            .rpc_endpoints
            .first()
            .ok_or("keeper RPC configuration has no endpoints")?;
        let reader = PublicReader::new(first.url.as_str(), self.vault.as_str())?
            .with_chain(self.expected_chain_id.as_deref())?;
        let (now, block) = reader.final_time().await?;
        for endpoint in &self.network.rpc_endpoints[1..] {
            PublicReader::new(endpoint.url.as_str(), self.vault.as_str())?
                .with_chain(self.expected_chain_id.as_deref())?
                .final_time()
                .await?;
        }
        Ok((reader, now, block))
    }

    async fn view(
        &self,
        reader: &PublicReader,
        block: &str,
        method: &str,
    ) -> Result<Value, KeeperError> {
        reader.view(self.vault.as_str(), method, json!({}), block).await
    }

    async fn paid_call(
        &self,
        method: &str,
        id: &str,
        args: Value,
        gas: u64,
        journal: Option<&OperationJournal>,
    ) -> Result<Value, KeeperError> {
        // Recheck after readiness reads. A different endpoint can otherwise
        // become active between the beginning of the tick and the paid send.
        self.preflight().await?;
        let transaction = Contract(self.vault.clone())
            .call_function(method, args)
            .transaction()
            .gas(NearGas::from_tgas(gas))
            .with_signer(self.account.clone(), self.signer.clone())
            .presign_with(&self.network)
            .await
            .map_err(|_| "keeper transaction preparation failed before broadcast")?;
        let near_api::advanced::TransactionableOrSigned::Signed((signed, _)) =
            &transaction.transaction
        else {
            return Err("keeper transaction was not presigned".into());
        };
        let expected_hash = signed.get_hash();
        let expected_transaction = signed.transaction.clone();
        if let Some(journal) = journal {
            journal
                .record(&PaidOperation {
                    version: 1,
                    operation: method,
                    attempt_id: id,
                    transaction_hash: expected_hash.to_string(),
                    vault: self.vault.as_str(),
                    account: self.account.as_str(),
                    expected_chain_id: self.expected_chain_id.as_deref(),
                })
                .map_err(|_| PaidOutcomeUnknown)?;
        }
        let full = transaction
            .send_to(&self.network)
            .await
            .map_err(|_| PaidOutcomeUnknown)?
            .into_full()
            .ok_or(PaidOutcomeUnknown)?;
        let submitted = checked_paid_result(full, expected_hash, &expected_transaction)?;
        // The SDK discards final_execution_status when constructing its result.
        // Require an explicit FINAL observation for the exact transaction before
        // the operation fence can be cleared, including successful bool results.
        let endpoint = self.network.rpc_endpoints.first().ok_or(PaidOutcomeUnknown)?;
        let raw = JsonRpc::new(endpoint.url.as_str(), None, 1024 * 1024)
            .map_err(|_| PaidOutcomeUnknown)?
            .call(
                "tx",
                json!({
                    "tx_hash":expected_hash.to_string(), "sender_account_id":self.account,
                    "wait_until":"FINAL",
                }),
            )
            .await
            .map_err(|_| PaidOutcomeUnknown)?;
        let confirmed = final_paid_result(raw, expected_hash, &expected_transaction)?;
        if confirmed != submitted {
            return Err(PaidOutcomeUnknown.into());
        }
        serde_json::from_slice(&confirmed).map_err(|_| PaidOutcomeUnknown.into())
    }
}

fn final_paid_result(
    raw: Value,
    expected_hash: near_api::CryptoHash,
    expected_transaction: &near_api::types::transaction::Transaction,
) -> Result<Vec<u8>, KeeperError> {
    if raw["final_execution_status"] != "FINAL"
        || raw["transaction"]["hash"] != expected_hash.to_string()
    {
        return Err(PaidOutcomeUnknown.into());
    }
    let parsed: near_api::types::RpcTransactionResponse =
        serde_json::from_value(raw).map_err(|_| PaidOutcomeUnknown)?;
    let confirmed = near_api::types::transaction::result::ExecutionFinalResult::try_from(
        near_api::advanced::to_final_execution_outcome(parsed),
    )
    .map_err(|_| PaidOutcomeUnknown)?;
    checked_paid_result(confirmed, expected_hash, expected_transaction)
}

fn checked_paid_result(
    result: near_api::types::transaction::result::ExecutionFinalResult,
    expected_hash: near_api::CryptoHash,
    expected_transaction: &near_api::types::transaction::Transaction,
) -> Result<Vec<u8>, KeeperError> {
    // SignedTransactionView omits the original reference block hash. The SDK
    // puts its reported transaction hash in that field when rebuilding the
    // transaction. Compare the full SDK projection of our signed context;
    // this does not claim RPC exposes the original reference block hash.
    let mut expected_projection = expected_transaction.clone();
    match &mut expected_projection {
        near_api::types::transaction::Transaction::V0(transaction) => {
            transaction.block_hash = expected_hash;
        }
        near_api::types::transaction::Transaction::V1(transaction) => {
            transaction.block_hash = expected_hash;
        }
    }
    if result.is_pending()
        || result.outcome().transaction_hash != expected_hash
        || result.transaction() != &expected_projection
    {
        return Err(PaidOutcomeUnknown.into());
    }
    result
        .into_result()
        .map_err(|_| PaidOutcomeUnknown)?
        .raw_bytes()
        .map_err(|_| PaidOutcomeUnknown.into())
}

/// Public network and final-anchor checks before any signing configuration or
/// transaction is needed. Validate every endpoint eligible for SDK failover.
pub async fn preflight_network(
    network: &NetworkConfig,
    vault: &str,
    expected_chain_id: Option<&str>,
) -> Result<(), KeeperError> {
    if network.rpc_endpoints.is_empty() {
        return Err("keeper RPC configuration has no endpoints".into());
    }
    for endpoint in &network.rpc_endpoints {
        PublicReader::new(endpoint.url.as_str(), vault)?
            .with_chain(expected_chain_id)?
            .final_time()
            .await?;
    }
    Ok(())
}

fn execution_tick(outcome: Value) -> Result<Tick, KeeperError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    enum ResultShape {
        Executed { proposal_id: String, expires_sec: String },
        Rejected { code: String, reason: String },
        Failed { reason: String },
    }
    match serde_json::from_value::<ResultShape>(outcome.clone())
        .map_err(|_| "execute returned an invalid or unknown outcome")?
    {
        ResultShape::Executed { proposal_id, expires_sec } => {
            canonical_id(&proposal_id)?;
            canonical_id(&expires_sec)?;
            Ok(Tick::Executed(outcome))
        }
        ResultShape::Rejected { code, reason } if !code.is_empty() && !reason.is_empty() => {
            Ok(Tick::Rejected(outcome))
        }
        ResultShape::Failed { reason } if !reason.is_empty() => Ok(Tick::Failed(outcome)),
        _ => Err("execute returned an empty outcome".into()),
    }
}

/// Retry policy for public errors and known protocol failures. Paid ambiguity
/// must stop the caller before this policy is consulted.
#[derive(Default)]
pub struct RetryBackoff {
    failures: u32,
}

impl RetryBackoff {
    pub fn after_tick(&mut self, tick: &Tick) -> Duration {
        if matches!(tick, Tick::Failed(_)) {
            self.after_error()
        } else {
            self.failures = 0;
            Duration::from_secs(5)
        }
    }

    pub fn after_error(&mut self) -> Duration {
        self.failures = self.failures.saturating_add(1);
        Duration::from_secs((5u64 << self.failures.min(7)).min(600))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_failures_are_not_reported_as_signed() {
        assert!(matches!(
            execution_tick(json!({ "Failed": { "reason": "oracle unavailable" } })).unwrap(),
            Tick::Failed(_)
        ));
        assert!(matches!(
            execution_tick(json!({ "Rejected": { "code": "TradeTooLarge", "reason": "cap" } }))
                .unwrap(),
            Tick::Rejected(_)
        ));
        assert!(matches!(
            execution_tick(json!({ "Executed": { "proposal_id": "1", "expires_sec": "120" } }))
                .unwrap(),
            Tick::Executed(_)
        ));
        assert!(execution_tick(json!({ "Unexpected": {} })).is_err());
        assert!(execution_tick(json!({ "Executed": {}, "Failed": {} })).is_err());
    }

    #[test]
    fn protocol_and_public_failures_share_bounded_backoff() {
        let mut retry = RetryBackoff::default();
        let failed = Tick::Failed(json!({ "Failed": { "reason": "signer unavailable" } }));
        assert_eq!(retry.after_tick(&failed), Duration::from_secs(10));
        assert_eq!(retry.after_error(), Duration::from_secs(20));
        assert_eq!(retry.after_tick(&failed), Duration::from_secs(40));
        for _ in 0..100 {
            assert!(retry.after_tick(&failed) <= Duration::from_secs(600));
        }
        assert_eq!(retry.after_tick(&Tick::Idle), Duration::from_secs(5));
        assert_eq!(retry.after_tick(&failed), Duration::from_secs(10));
    }

    #[test]
    fn recovery_state_is_exact_and_maturity_does_not_change_its_delay() {
        let valid = json!({"protocol":"unique_voucher_v1","withdrawal_id":"1",
            "stage":"Preparing","started_sec":100,"recover_after_sec":400,"required_gas_tgas":140});
        assert!(RecoveryState::decode(valid.clone(), 399).unwrap().is_some());
        assert!(RecoveryState::decode(valid.clone(), 400).unwrap().is_some());
        for (field, value) in [
            ("protocol", json!("durable_future")),
            ("withdrawal_id", json!("+1")),
            ("withdrawal_id", json!("0")),
            ("stage", json!("Sent")),
            ("recover_after_sec", json!(399)),
            ("required_gas_tgas", json!(300)),
            ("extra", json!(true)),
        ] {
            let mut changed = valid.clone();
            changed[field] = value;
            assert!(RecoveryState::decode(changed, 400).is_err());
        }
        assert!(RecoveryState::decode(valid, 99).is_err());
        assert!(RecoveryState::decode(Value::Null, 400).unwrap().is_none());
    }

    #[test]
    fn known_false_recovery_cooldown_expires_and_never_holds_a_new_id() {
        let mut state = KeeperState { unresolved_recovery: Some(("1".into(), Instant::now())) };
        assert!(state.cooling_down("1"));
        assert!(!state.cooling_down("2"));
        state.unresolved_recovery = Some(("2".into(), Instant::now() - Duration::from_secs(31)));
        assert!(!state.cooling_down("2"));
    }

    #[test]
    fn durable_journal_recovery_accepts_both_known_stages_and_exact_snapshots() {
        let valid = json!({"protocol":"durable_withdrawal_journal_v1","withdrawal_id":"9",
            "stage":"Dispatched","started_sec":100,"recover_after_sec":400,"required_gas_tgas":140});
        assert!(RecoveryState::decode(valid.clone(), 399).unwrap().is_some());
        assert!(RecoveryState::decode(valid.clone(), 400).unwrap().is_some());
        let mut preparing = valid.clone();
        preparing["stage"] = json!("Preparing");
        assert!(RecoveryState::decode(preparing, 400).unwrap().is_some());
        for (field, value) in [
            ("stage", json!("Unknown")),
            ("protocol", json!("durable_withdrawal_journal_v2")),
            ("started_sec", json!(401)),
            ("recover_after_sec", json!(399)),
            ("required_gas_tgas", json!(141)),
            ("executor", json!("untrusted.testnet")),
        ] {
            let mut changed = valid.clone();
            changed[field] = value;
            assert!(RecoveryState::decode(changed, 400).is_err());
        }
    }

    #[test]
    fn execute_result_requires_exact_variant_fields_and_u64_strings() {
        for outcome in [
            json!({"Executed":null}),
            json!({"Executed":{"proposal_id":"1","expires_sec":120}}),
            json!({"Executed":{"proposal_id":"01","expires_sec":"120"}}),
            json!({"Failed":{"reason":"unknown","extra":true}}),
            json!({"Rejected":{"code":"cap","reason":""}}),
        ] {
            assert!(execution_tick(outcome).is_err());
        }
    }

    #[test]
    fn journal_is_synced_before_send_and_retained_records_cannot_be_overwritten() {
        let directory =
            std::env::temp_dir().join(format!("mandate-journal-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let directory = directory.canonicalize().unwrap();
        let journal = OperationJournal::acquire(&directory).unwrap();
        let operation = PaidOperation {
            version: 1,
            operation: "recover_withdrawal_attempt",
            attempt_id: "1",
            transaction_hash: "11111111111111111111111111111111".into(),
            vault: "vault.testnet",
            account: "keeper.testnet",
            expected_chain_id: Some("fixture"),
        };
        journal.record(&operation).unwrap();
        let retained = std::fs::read(journal.path()).unwrap();
        let context: Value = serde_json::from_slice(&retained).unwrap();
        assert_eq!(context.as_object().unwrap().len(), 7);
        assert_eq!(context["operation"], "recover_withdrawal_attempt");
        assert_eq!(context["attempt_id"], "1");
        assert!(journal.ensure_clear().is_err());
        assert!(journal.record(&operation).is_err());
        assert_eq!(std::fs::read(journal.path()).unwrap(), retained);
        journal.clear().unwrap();
        journal.ensure_clear().unwrap();
        std::fs::remove_file(directory.join("keeper.lock")).unwrap();
        std::fs::write(directory.join("keeper.lock"), b"").unwrap();
        assert!(journal.ensure_clear().is_err());
        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn final_status_decoder_rejects_optimistic_or_mismatched_public_results() {
        use near_api::types::crypto::secret_key::ED25519SecretKey;
        use near_api::types::transaction::{
            Transaction, TransactionV0,
            actions::{Action, FunctionCallAction},
        };
        use near_sdk::base64::{Engine, engine::general_purpose::STANDARD};
        let block: near_api::CryptoHash = "11111111111111111111111111111111".parse().unwrap();
        let key = near_api::SecretKey::ED25519(ED25519SecretKey::from_secret_key([7; 32]));
        let transaction = Transaction::V0(TransactionV0 {
            signer_id: "keeper.testnet".parse().unwrap(),
            public_key: key.public_key(),
            nonce: 1,
            receiver_id: "vault.testnet".parse().unwrap(),
            block_hash: block,
            actions: vec![Action::FunctionCall(Box::new(FunctionCallAction {
                method_name: "recover_withdrawal_attempt".into(),
                args: b"{\"withdrawal_id\":\"1\"}".to_vec(),
                gas: NearGas::from_tgas(140),
                deposit: near_api::NearToken::from_yoctonear(0),
            }))],
        });
        let hash = transaction.get_hash();
        let raw = json!({
            "final_execution_status":"FINAL", "status":{"SuccessValue":STANDARD.encode(b"false")},
            "transaction":{"signer_id":transaction.signer_id(), "receiver_id":transaction.receiver_id(),
                "public_key":transaction.public_key(), "nonce":1,"signature":key.sign(hash), "hash":hash,
                "actions":[{"FunctionCall":{"method_name":"recover_withdrawal_attempt", "args":STANDARD.encode(b"{\"withdrawal_id\":\"1\"}"),
                    "gas":140_000_000_000_000u64,"deposit":"0"}}]},
            "transaction_outcome":{"id":hash,"block_hash":block,"proof":[],
                "outcome":{"executor_id":"keeper.testnet", "gas_burnt":0,"tokens_burnt":"0","logs":[],
                    "receipt_ids":[],"status":{"SuccessValue":""}}}, "receipts_outcome":[],
        });
        assert_eq!(final_paid_result(raw.clone(), hash, &transaction).unwrap(), b"false");
        for case in 0..7 {
            let mut changed = raw.clone();
            match case {
                0 => changed["final_execution_status"] = json!("EXECUTED_OPTIMISTIC"),
                1 => changed["final_execution_status"] = Value::Null,
                2 => changed["transaction"]["hash"] = json!(block),
                3 => changed["transaction_outcome"]["id"] = json!(block),
                4 => changed["transaction"]["receiver_id"] = json!("other.testnet"),
                5 => changed["transaction"]["priority_fee"] = json!(1),
                _ => changed["status"] = json!("Started"),
            }
            assert!(
                final_paid_result(changed, hash, &transaction)
                    .unwrap_err()
                    .is::<PaidOutcomeUnknown>()
            );
        }
        // The SDK's synthetic block_hash is the view's transaction hash, not
        // the original signing anchor. A different synthetic hash is refused.
        let mut wrong_projection = raw;
        wrong_projection["transaction"]["hash"] = json!(block);
        let parsed: near_api::types::RpcTransactionResponse =
            serde_json::from_value(wrong_projection).unwrap();
        let result = near_api::types::transaction::result::ExecutionFinalResult::try_from(
            near_api::advanced::to_final_execution_outcome(parsed),
        )
        .unwrap();
        assert!(
            checked_paid_result(result, hash, &transaction).unwrap_err().is::<PaidOutcomeUnknown>()
        );
    }
}
