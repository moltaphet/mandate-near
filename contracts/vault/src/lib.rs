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
//! `execute` runs the whole decision on chain in one transaction: it reads
//! balances from `intents.near` and prices from the oracle, applies the nine
//! gates, builds the payload with a V1 nonce, and asks the MPC signer to sign
//! it. The signed intent is stored for a keeper to publish.

pub mod engine;
pub mod intent;
pub mod mandate;
pub mod math;
mod migration;
pub mod oracle;
pub mod pyth;
mod withdrawal;
pub use withdrawal::{MIN_WITHDRAWAL_GAS_TGAS, MIN_WITHDRAWAL_RECOVERY_GAS_TGAS, WithdrawalStage};

use std::collections::BTreeMap;

use near_sdk::json_types::{U64, U128};
use near_sdk::serde::de::DeserializeOwned;
use near_sdk::serde_json::{self, json};
use near_sdk::{
    AccountId, Gas, NearToken, PanicOnDefault, Promise, PromiseError, PromiseOrValue,
    assert_one_yocto, env, near, require,
};

use crate::engine::{INTENT_TTL_SEC, Market, Trade, TradeError, check_trade};
pub use crate::engine::{TradingState, TradingStateView};
use crate::intent::{TokenDiff, token_diff_payload, v1_nonce};
pub use crate::mandate::{AssetConfig, Mandate, MandateError, loosened_fields};
use crate::oracle::{AssetView, PriceData, prices_for};

/// Fixed at initialization. Switching providers requires a separately reviewed deployment.
#[near(serializers = [json, borsh])]
#[serde(rename_all = "snake_case")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OracleKind {
    PriceOracle,
    Pyth,
}

/// How long a pending proposal stays executable. Matches the 60 s target in docs/PROJECT.md.
pub const PROPOSAL_TTL_SEC: u64 = 60;
/// How long one execution may build and sign an intent. Its callbacks stop signing after this.
/// The lock it holds runs out once this window and the intent's lifetime have both passed, so
/// no intent from that execution can still be valid when the next one starts.
pub const EXECUTION_WINDOW_SEC: u64 = 300;
/// Consecutive rejections before the circuit breaker halts new proposals.
pub const MAX_STRIKES: u32 = 3;

/// Defaults for NEAR mainnet. Sandbox and testnet use `new_with_externals`.
pub const MAINNET_SIGNER: &str = "v1.signer";
pub const MAINNET_INTENTS: &str = "intents.near";
pub const MAINNET_ORACLE: &str = "price-oracle.near";
/// Derivation path for the vault's NEAR Intents key (the same path the R1 spike used).
pub const INTENTS_KEY_PATH: &str = "intents";
/// Domain 1 on the MPC signer is Ed25519.
pub const ED25519_DOMAIN_ID: u32 = 1;
/// `v1.signer` accepts EdDSA payloads of at most this many bytes.
pub const MAX_MPC_PAYLOAD: usize = 1232;
/// Largest oracle or intents answer a callback will read.
const MAX_ANSWER_BYTES: usize = 64 * 1024;

const GAS_DERIVE_KEY: Gas = Gas::from_tgas(10);
const GAS_ADD_KEY_CALLBACK: Gas = Gas::from_tgas(60);
const GAS_ADD_KEY: Gas = Gas::from_tgas(30);
const GAS_KEY_REGISTERED_CALLBACK: Gas = Gas::from_tgas(10);
const GAS_READ_BALANCES: Gas = Gas::from_tgas(10);
const GAS_READ_PRICES: Gas = Gas::from_tgas(10);
const GAS_READ_ASSET: Gas = Gas::from_tgas(8);
const GAS_READ_SALT: Gas = Gas::from_tgas(5);
/// `v1.signer` needs at least 15 TGas attached; 30 leaves room for the signer.
const GAS_SIGN: Gas = Gas::from_tgas(30);
const GAS_CALLBACK: Gas = Gas::from_tgas(15);
/// `ft_withdraw` on `intents.near`, which moves the tokens out of the vault's balance.
const GAS_WITHDRAW: Gas = Gas::from_tgas(100);

/// A trade the agent wants executed. Deliberately has no payload field.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    pub id: U64,
    pub asset_in: String,
    pub asset_out: String,
    pub amount_in: U128,
    pub min_amount_out: U128,
    /// Off-chain evidence commitment, as 64 hex characters. Its interpretation
    /// depends on the declared source; this contract does not authenticate model origin.
    pub reason_hash: String,
    pub created_sec: u64,
    pub expires_sec: u64,
    pub model_id: String,
}

/// The execution that holds the in-flight lock. Its callbacks act only while they carry its id.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InFlight {
    pub id: U64,
    pub started_sec: u64,
}

/// Immutable interpretation of reads made by one execution.
#[near(serializers = [borsh])]
#[derive(Clone)]
struct ExecutionContext {
    id: U64,
    revision: u64,
    assets: Vec<AssetConfig>,
    proposal_id: U64,
    /// Last second at which every approved oracle price still satisfies the age guard.
    price_valid_until_sec: Option<u64>,
}

/// An intent the MPC signer has signed. A keeper publishes it off chain; the
/// verifier (`intents.near`) settles it only if the signature and nonce check out.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedIntent {
    pub proposal_id: U64,
    /// The exact string the signature covers (`raw_ed25519`).
    pub payload: String,
    /// `ed25519:<base58>`, the key registered on `intents.near`.
    pub public_key: String,
    /// `ed25519:<base58>` over `payload`.
    pub signature: String,
    /// Unix seconds after which the intent can no longer settle.
    pub expires_sec: u64,
}

/// Result of one `execute` call.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutionOutcome {
    /// Every gate passed and the intent is signed and stored.
    Executed { proposal_id: U64, expires_sec: U64 },
    /// A gate closed. The proposal is consumed and a strike is counted.
    Rejected { code: String, reason: String },
    /// A read or signing failed. No strike is counted; a failure after requesting
    /// a signature retains the timed lock because signature absence is unproved.
    Failed { reason: String },
}

/// Only the most recent confirmed outcome is retained; this is not historical storage.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposalOutcome {
    pub proposal_id: U64,
    pub outcome: ExecutionOutcome,
}

/// How long a loosening of the mandate waits before it applies. Tightening applies at once.
/// The owner can cancel a queued change in this window.
pub const MANDATE_DELAY_SEC: u64 = 24 * 60 * 60;

/// A loosened mandate waiting for its delay to pass.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingMandate {
    pub mandate: Mandate,
    /// Unix seconds from which `apply_pending_mandate` may apply it.
    pub effective_sec: u64,
}

/// How long a change of agent waits before it applies. The same delay as a loosened mandate.
pub const AGENT_DELAY_SEC: u64 = MANDATE_DELAY_SEC;

/// How long a withdrawal waits before it is sent. The same delay as a loosened mandate, so a
/// stolen owner key has a day to be noticed and cancelled before any funds leave.
pub const WITHDRAW_DELAY_SEC: u64 = MANDATE_DELAY_SEC;

/// A new agent waiting for its delay to pass.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingAgent {
    pub agent: AccountId,
    /// Unix seconds from which `apply_pending_agent` may apply it.
    pub effective_sec: u64,
}

/// A withdrawal of one asset from the vault's balance, waiting for its delay to pass.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingWithdrawal {
    /// The mandate asset, as `nep141:<token account>`.
    pub asset_id: String,
    pub receiver: AccountId,
    pub amount: U128,
    /// Unix seconds from which `apply_pending_withdrawal` may send it.
    pub effective_sec: u64,
}

/// A withdrawal owns the guard until its identified callback or reconciliation resolves it.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveWithdrawal {
    pub id: U64,
    pub request_seq: U64,
    pub request: PendingWithdrawal,
    pub started_sec: u64,
    /// Set before sending; `None` means the balance read has not sent anything.
    pub balance_before: Option<U128>,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WithdrawalOutcome {
    Delivered {
        withdrawal_id: U64,
        amount: U128,
    },
    Failed {
        withdrawal_id: U64,
        reason: String,
    },
    /// Ledger debit confirmed; receiver delivery is not proved by this observation.
    DebitObserved {
        withdrawal_id: U64,
    },
}

pub const WITHDRAW_RECOVERY_SEC: u64 = 300;

/// What `set_mandate` did with the new mandate.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MandateChangeOutcome {
    /// Nothing loosened: the mandate is in force now.
    Applied,
    /// A limit was loosened: the mandate applies after `effective_sec`.
    Queued { effective_sec: U64 },
}

/// What `simulate_mandate_change` would do. Changes nothing.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MandateSimulation {
    pub valid: bool,
    /// Why the mandate is refused, when `valid` is false.
    pub error: Option<String>,
    /// The loosened fields. Empty means the change applies at once.
    pub loosened_fields: Vec<String>,
    /// `None` when the mandate is invalid.
    pub outcome: Option<MandateChangeOutcome>,
}

/// `v1.signer` response: `{"scheme": "Ed25519", "signature": [64 bytes]}`.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureResponse {
    pub scheme: String,
    pub signature: Vec<u8>,
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
    /// MPC signer, NEAR Intents verifier and price oracle this vault talks to.
    signer: AccountId,
    intents: AccountId,
    oracle: AccountId,
    /// The vault's MPC-derived Ed25519 key, as returned by the signer.
    intents_key: Option<String>,
    /// True once `intents` confirmed `add_public_key` for `intents_key`.
    key_registered: bool,
    /// The execution that holds the lock, if any. Set by `execute`; cleared when it ends, or
    /// by time once its window and the intent's lifetime have passed (`execution_locked`).
    in_flight: Option<InFlight>,
    /// The id the next execution takes. Callbacks of an earlier execution carry an older id.
    next_execution_id: u64,
    /// The latest signed intent. Replaced by each approved trade.
    signed_intent: Option<SignedIntent>,
    /// A loosened mandate waiting for its delay. At most one.
    pending_mandate: Option<PendingMandate>,
    /// A new agent waiting for its delay. At most one. The current agent keeps its rights.
    pending_agent: Option<PendingAgent>,
    /// A withdrawal waiting for its delay. At most one; a newer request replaces it.
    pending_withdrawal: Option<PendingWithdrawal>,
    /// Moves on with every withdrawal request and cancel. A send that fails returns its
    /// request to the queue only while this number is still the one it was sent under.
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

#[near]
impl Vault {
    /// Upgrade only known, quiescent legacy layouts. Deployment authority is required.
    #[init(ignore_state)]
    #[private]
    pub fn migrate() -> Self {
        migration::migrate()
    }

    /// For mainnet: talks to `v1.signer`, `intents.near` and `price-oracle.near`.
    #[init]
    pub fn new(owner: AccountId, mandate: Mandate) -> Self {
        Self::init(
            owner,
            mandate,
            MAINNET_SIGNER.parse().unwrap_or_else(|_| env::panic_str("bad signer id")),
            MAINNET_INTENTS.parse().unwrap_or_else(|_| env::panic_str("bad intents id")),
            MAINNET_ORACLE.parse().unwrap_or_else(|_| env::panic_str("bad oracle id")),
            OracleKind::PriceOracle,
        )
    }

    /// For testnet and sandbox, where the signer, verifier and oracle live at other accounts.
    #[init]
    pub fn new_with_externals(
        owner: AccountId,
        mandate: Mandate,
        signer: AccountId,
        intents: AccountId,
        oracle: AccountId,
    ) -> Self {
        Self::init(owner, mandate, signer, intents, oracle, OracleKind::PriceOracle)
    }

    /// Selects an immutable provider and explicit external accounts at deployment.
    #[init]
    pub fn new_with_oracle(
        owner: AccountId,
        mandate: Mandate,
        signer: AccountId,
        intents: AccountId,
        oracle: AccountId,
        oracle_kind: OracleKind,
    ) -> Self {
        Self::init(owner, mandate, signer, intents, oracle, oracle_kind)
    }

    pub fn get_oracle_kind(&self) -> OracleKind {
        self.oracle_kind
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

    pub fn get_intents_key(&self) -> Option<String> {
        self.intents_key.clone()
    }

    pub fn is_key_registered(&self) -> bool {
        self.key_registered
    }

    /// `(signer, intents, oracle)`.
    pub fn get_externals(&self) -> (AccountId, AccountId, AccountId) {
        (self.signer.clone(), self.intents.clone(), self.oracle.clone())
    }

    /// The latest signed intent, if any. Keepers publish it to the solver relay.
    pub fn get_signed_intent(&self) -> Option<SignedIntent> {
        self.signed_intent.clone()
    }

    pub fn is_in_flight(&self) -> bool {
        self.execution_locked(now_sec()) || self.active_withdrawal.is_some()
    }

    pub fn get_chain_time_sec(&self) -> u64 {
        now_sec()
    }

    pub fn get_last_proposal_outcome(&self) -> Option<ProposalOutcome> {
        self.last_proposal_outcome.clone()
    }

    pub fn get_withdrawal_status(&self) -> Option<ActiveWithdrawal> {
        self.active_withdrawal.clone()
    }

    pub fn get_last_withdrawal_outcome(&self) -> Option<WithdrawalOutcome> {
        self.last_withdrawal_outcome.clone()
    }

    /// Registers the vault's MPC-derived key on `intents.near`, so that
    /// signatures made with it count as the vault's own.
    ///
    /// Permissionless: the key is the one the MPC signer derives for this
    /// vault's account, so whoever triggers this cannot choose it. Once the
    /// key is registered, calling again is refused.
    /// The vault attaches the 1 yoctoNEAR that `add_public_key` requires,
    /// authenticating to `intents.near` as itself (predecessor auth).
    pub fn register_intents_key(&mut self) -> Promise {
        require!(!self.key_registered, "the intents key is already registered");
        Promise::new(self.signer.clone())
            .function_call(
                "derived_public_key".to_string(),
                json!({
                    "path": INTENTS_KEY_PATH,
                    "predecessor": env::current_account_id(),
                    "domain_id": ED25519_DOMAIN_ID,
                })
                .to_string()
                .into_bytes(),
                NearToken::from_yoctonear(0),
                GAS_DERIVE_KEY,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_ADD_KEY_CALLBACK)
                    .on_derived_key(),
            )
    }

    #[private]
    pub fn on_derived_key(
        &mut self,
        #[callback_result(max_bytes = MAX_ANSWER_BYTES)] key: Result<String, PromiseError>,
    ) -> Promise {
        let key = match key {
            Ok(key) => key,
            Err(_) => env::panic_str("the MPC signer did not return a derived key"),
        };
        require!(key.starts_with("ed25519:"), "derived key is not an Ed25519 key");
        self.intents_key = Some(key.clone());
        Promise::new(self.intents.clone())
            .function_call(
                "add_public_key".to_string(),
                json!({ "public_key": key }).to_string().into_bytes(),
                NearToken::from_yoctonear(1),
                GAS_ADD_KEY,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_KEY_REGISTERED_CALLBACK)
                    .on_key_registered(),
            )
    }

    #[private]
    pub fn on_key_registered(
        &mut self,
        #[callback_result(max_bytes = MAX_ANSWER_BYTES)] result: Result<(), PromiseError>,
    ) -> bool {
        // A failure never downgrades an earlier success.
        if result.is_ok() {
            self.key_registered = true;
        }
        self.key_registered
    }

    /// Sets the mandate. Requires 1 yoctoNEAR so a leaked function-call key cannot change rules.
    ///
    /// A change that loosens any limit is queued for `MANDATE_DELAY_SEC` and replaces any
    /// earlier queued change. A change that only tightens applies at once and drops any
    /// queued change, which was written against the old limits.
    #[payable]
    pub fn set_mandate(&mut self, mandate: Mandate) -> MandateChangeOutcome {
        assert_one_yocto();
        self.assert_owner();
        validate_oracle_mandate(&mandate, self.oracle_kind);
        if loosened_fields(&self.mandate, &mandate).is_empty() {
            self.advance_revision();
            self.mandate = mandate;
            self.pending_mandate = None;
            return MandateChangeOutcome::Applied;
        }
        let effective_sec = now_sec() + MANDATE_DELAY_SEC;
        self.pending_mandate = Some(PendingMandate { mandate, effective_sec });
        MandateChangeOutcome::Queued { effective_sec: U64(effective_sec) }
    }

    /// Applies a queued loosening once its delay has passed. Anyone may call it: the owner
    /// fixed the content when it was queued, and only the time is checked here.
    pub fn apply_pending_mandate(&mut self) {
        let Some(pending) = self.pending_mandate.clone() else {
            env::panic_str("no mandate change is queued");
        };
        require!(now_sec() >= pending.effective_sec, "the mandate change is still in its delay");
        validate_oracle_mandate(&pending.mandate, self.oracle_kind);
        self.advance_revision();
        self.mandate = pending.mandate;
        self.pending_mandate = None;
    }

    /// Drops a queued loosening before it applies. Requires 1 yoctoNEAR.
    #[payable]
    pub fn cancel_pending_mandate(&mut self) {
        assert_one_yocto();
        self.assert_owner();
        require!(self.pending_mandate.is_some(), "no mandate change is queued");
        self.pending_mandate = None;
    }

    /// The queued loosening, if any.
    pub fn get_pending_mandate(&self) -> Option<PendingMandate> {
        self.pending_mandate.clone()
    }

    /// What `set_mandate` would do with this mandate, without doing it.
    pub fn simulate_mandate_change(&self, mandate: Mandate) -> MandateSimulation {
        if let Err(error) = oracle_mandate_validation(&mandate, self.oracle_kind) {
            return MandateSimulation {
                valid: false,
                error: Some(error),
                loosened_fields: vec![],
                outcome: None,
            };
        }
        let fields = loosened_fields(&self.mandate, &mandate);
        let outcome = if fields.is_empty() {
            MandateChangeOutcome::Applied
        } else {
            MandateChangeOutcome::Queued { effective_sec: U64(now_sec() + MANDATE_DELAY_SEC) }
        };
        MandateSimulation {
            valid: true,
            error: None,
            loosened_fields: fields.into_iter().map(str::to_owned).collect(),
            outcome: Some(outcome),
        }
    }

    /// Sets the only account allowed to propose trades. Requires 1 yoctoNEAR.
    ///
    /// The first agent applies at once: no agent can propose before it is set. Any later
    /// change is queued for `AGENT_DELAY_SEC` and replaces an earlier queued change. The
    /// current agent keeps its rights meanwhile. The owner can cancel the change, or pause
    /// the vault at once.
    #[payable]
    pub fn set_agent(&mut self, agent: AccountId) {
        assert_one_yocto();
        self.assert_owner();
        if self.agent.is_none() {
            self.advance_revision();
            self.agent = Some(agent);
            return;
        }
        let effective_sec = now_sec() + AGENT_DELAY_SEC;
        self.pending_agent = Some(PendingAgent { agent, effective_sec });
    }

    /// Applies a queued agent change once its delay has passed. Anyone may call it: the owner
    /// fixed the agent when it was queued, and only the time is checked here.
    pub fn apply_pending_agent(&mut self) {
        let Some(pending) = self.pending_agent.clone() else {
            env::panic_str("no agent change is queued");
        };
        require!(now_sec() >= pending.effective_sec, "the agent change is still in its delay");
        self.advance_revision();
        self.agent = Some(pending.agent);
        self.pending_agent = None;
    }

    /// Drops a queued agent change before it applies. Requires 1 yoctoNEAR.
    #[payable]
    pub fn cancel_pending_agent(&mut self) {
        assert_one_yocto();
        self.assert_owner();
        require!(self.pending_agent.is_some(), "no agent change is queued");
        self.pending_agent = None;
    }

    /// The queued agent change, if any.
    pub fn get_pending_agent(&self) -> Option<PendingAgent> {
        self.pending_agent.clone()
    }

    /// Queues a withdrawal of `amount` of a mandate asset to `receiver`. Requires 1 yoctoNEAR.
    /// Refused while an execution holds the lock or a signed intent is outstanding, so a
    /// withdrawal cannot race a trade. A newer request replaces a queued one.
    #[payable]
    pub fn request_withdrawal(&mut self, asset_id: String, receiver: AccountId, amount: U128) {
        assert_one_yocto();
        self.assert_owner();
        require!(
            self.mandate.assets.iter().any(|a| a.asset_id == asset_id),
            "the asset is not in the mandate"
        );
        require!(token_account(&asset_id).is_some(), "only nep141 assets can be withdrawn");
        require!(amount.0 > 0, "the amount must be positive");
        // Tokens sent to the vault or to intents.near would not return to the owner's control.
        require!(
            receiver != env::current_account_id() && receiver != self.intents,
            "the receiver cannot be the vault or intents.near"
        );
        let now = now_sec();
        require!(!self.execution_locked(now), "an execution is in progress");
        require!(
            self.trading.signed_intent_expires_at_sec <= now,
            "a signed intent is still outstanding"
        );
        self.pending_withdrawal = Some(PendingWithdrawal {
            asset_id,
            receiver,
            amount,
            effective_sec: now + WITHDRAW_DELAY_SEC,
        });
        self.withdrawal_seq =
            self.withdrawal_seq.checked_add(1).expect("withdrawal sequence exhausted");
        self.withdrawal_retry_armed = Some(U64(self.withdrawal_seq));
    }

    /// Drops a queued withdrawal before it is sent. Requires 1 yoctoNEAR.
    #[payable]
    pub fn cancel_pending_withdrawal(&mut self) {
        assert_one_yocto();
        self.assert_owner();
        require!(self.pending_withdrawal.is_some(), "no withdrawal is queued");
        self.pending_withdrawal = None;
        self.withdrawal_seq =
            self.withdrawal_seq.checked_add(1).expect("withdrawal sequence exhausted");
        self.withdrawal_retry_armed = None;
    }

    /// The queued withdrawal, if any.
    pub fn get_pending_withdrawal(&self) -> Option<PendingWithdrawal> {
        self.pending_withdrawal.clone()
    }

    /// Re-arms one safe retry without changing the owner-authorized request or maturity.
    #[payable]
    pub fn rearm_pending_withdrawal(&mut self) {
        assert_one_yocto();
        self.assert_owner();
        require!(self.active_withdrawal.is_none(), "a withdrawal is unresolved");
        require!(self.pending_withdrawal.is_some(), "no withdrawal is queued");
        require!(!self.is_withdrawal_retry_armed(), "the withdrawal attempt is already armed");
        self.withdrawal_retry_armed = Some(U64(self.withdrawal_seq));
    }

    pub fn is_withdrawal_retry_armed(&self) -> bool {
        self.pending_withdrawal.is_some()
            && self.withdrawal_retry_armed == Some(U64(self.withdrawal_seq))
    }

    pub fn get_withdrawal_stage(&self) -> Option<WithdrawalStage> {
        self.withdrawal_stage
    }

    pub fn get_withdrawal_protocol(&self) -> String {
        "unique_voucher_v1".into()
    }

    pub fn get_withdrawal_recovery_accounts(&self) -> (AccountId, AccountId) {
        withdrawal::sinks()
    }

    /// Persists a unique id and spends the single owner-armed attempt before issuing its voucher.
    pub fn apply_pending_withdrawal(&mut self) -> Promise {
        let pending = self
            .pending_withdrawal
            .clone()
            .unwrap_or_else(|| env::panic_str("no withdrawal is queued"));
        let now = now_sec();
        require!(now >= pending.effective_sec, "the withdrawal is still in its delay");
        require!(self.active_withdrawal.is_none(), "a withdrawal is unresolved");
        require!(!self.execution_locked(now), "an execution is in progress");
        require!(
            self.trading.signed_intent_expires_at_sec <= now,
            "a signed intent is still outstanding"
        );
        require!(self.is_withdrawal_retry_armed(), "the owner must re-arm the withdrawal attempt");
        require!(
            env::prepaid_gas() >= Gas::from_tgas(MIN_WITHDRAWAL_GAS_TGAS),
            "withdrawal requires at least 280 TGas"
        );
        require!(token_account(&pending.asset_id).is_some(), "only nep141 assets can be withdrawn");
        require!(
            pending.receiver != env::current_account_id() && pending.receiver != self.intents,
            "the receiver cannot be the vault or intents.near"
        );
        let id = U64(self.next_withdrawal_id);
        self.next_withdrawal_id =
            self.next_withdrawal_id.checked_add(1).expect("withdrawal id exhausted");
        self.withdrawal_retry_armed = None;
        self.pending_withdrawal = None;
        self.active_withdrawal = Some(ActiveWithdrawal {
            id,
            request_seq: U64(self.withdrawal_seq),
            request: pending.clone(),
            started_sec: now,
            balance_before: None,
        });
        self.withdrawal_stage = Some(WithdrawalStage::Preparing);
        self.prepare_withdrawal(id, &pending.asset_id)
    }

    #[private]
    pub fn on_withdrawal_balance(&mut self, withdrawal_id: U64) -> PromiseOrValue<bool> {
        let Some(active) = self.current_withdrawal(withdrawal_id, WithdrawalStage::Preparing)
        else {
            return PromiseOrValue::Value(false);
        };
        let before = match read_promise::<Vec<U128>>(0) {
            Ok(b) if b.len() == 2 && b[0].0 == 1 && b[1].0 >= active.request.amount.0 => b[1],
            _ => {
                self.withdrawal_failed(
                    active,
                    "could not read a unique voucher and sufficient withdrawal balance",
                );
                return PromiseOrValue::Value(false);
            }
        };
        // No original is scheduled until the unique CURRENT supply, source unit
        // and both empty destinations are independently read for this same id.
        PromiseOrValue::Promise(self.check_withdrawal_preparation(withdrawal_id, before))
    }

    #[private]
    pub fn on_withdrawal_prepared(
        &mut self,
        withdrawal_id: U64,
        balance_before: U128,
    ) -> PromiseOrValue<bool> {
        let Some(active) = self.current_withdrawal(withdrawal_id, WithdrawalStage::Preparing)
        else {
            return PromiseOrValue::Value(false);
        };
        if balance_before.0 < active.request.amount.0
            || Self::voucher_snapshot_result() != Some((1, 1, 0, 0))
        {
            self.withdrawal_failed(
                active,
                "withdrawal voucher supply, source or destinations are not unique and empty",
            );
            return PromiseOrValue::Value(false);
        }
        self.active_withdrawal.as_mut().expect("active withdrawal").balance_before =
            Some(balance_before);
        self.withdrawal_stage = Some(WithdrawalStage::Dispatched);
        PromiseOrValue::Promise(self.send_marked_withdrawal(&active))
    }

    #[private]
    pub fn on_withdrawn(&mut self, withdrawal_id: U64) -> bool {
        self.finish_withdrawal(withdrawal_id, read_promise::<U128>(0))
    }

    /// Before dispatch, invalidate the preparing id. Afterwards race its unique
    /// voucher to cancellation; only authenticated positive destination proof releases the guard.
    pub fn recover_withdrawal(&mut self) -> PromiseOrValue<bool> {
        let active = self
            .active_withdrawal
            .clone()
            .unwrap_or_else(|| env::panic_str("no withdrawal is unresolved"));
        require!(
            now_sec() >= active.started_sec.saturating_add(WITHDRAW_RECOVERY_SEC),
            "the withdrawal recovery window has not passed"
        );
        if self.withdrawal_stage == Some(WithdrawalStage::Preparing) {
            self.withdrawal_failed(active, "withdrawal preparation timed out before sending");
            return PromiseOrValue::Value(true);
        }
        require!(
            self.withdrawal_stage == Some(WithdrawalStage::Dispatched),
            "invalid withdrawal phase"
        );
        require!(
            env::prepaid_gas() >= Gas::from_tgas(MIN_WITHDRAWAL_RECOVERY_GAS_TGAS),
            "recovery requires at least 140 TGas"
        );
        PromiseOrValue::Promise(self.cancel_withdrawal_voucher(active.id))
    }

    #[private]
    pub fn on_withdrawal_cancelled(&mut self, withdrawal_id: U64) -> PromiseOrValue<bool> {
        if self.current_withdrawal(withdrawal_id, WithdrawalStage::Dispatched).is_none() {
            return PromiseOrValue::Value(false);
        }
        // Both successful and failed cancellation results require durable proof.
        // A failed transfer may mean the original or a previous cancellation won.
        if env::promise_results_count() == 1 {
            match env::promise_result_checked(0, MAX_ANSWER_BYTES) {
                Ok(_) => {
                    env::log_str("withdrawal cancellation acknowledged; reading destination proof")
                }
                Err(_) => env::log_str(
                    "withdrawal cancellation failed or unknown; reading destination proof",
                ),
            }
        } else {
            env::log_str("withdrawal cancellation result missing; reading destination proof");
        }
        PromiseOrValue::Promise(self.withdrawal_proof(withdrawal_id))
    }

    #[private]
    pub fn on_withdrawal_reconciled(&mut self, withdrawal_id: U64) -> bool {
        let Some(active) = self.current_withdrawal(withdrawal_id, WithdrawalStage::Dispatched)
        else {
            return false;
        };
        let Some((supply, source, sent, cancelled)) = Self::voucher_snapshot_result() else {
            return false;
        };
        // Current-supply checks do not prove absence of historical outstanding
        // burn/refund receipts. That is an explicit sealed-deployment premise.
        if supply != 1 || source != 0 {
            return false;
        }
        match (sent, cancelled) {
            (1, 0) => {
                self.active_withdrawal = None;
                self.withdrawal_stage = None;
                self.last_withdrawal_outcome =
                    Some(WithdrawalOutcome::DebitObserved { withdrawal_id });
                true
            }
            (0, 1) => {
                self.withdrawal_failed(active, "withdrawal cancelled before debit");
                true
            }
            _ => false,
        }
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
        model_id: String,
    ) -> U64 {
        self.assert_not_paused();
        require!(self.active_withdrawal.is_none(), "a withdrawal is unresolved");
        require!(!self.halted, "proposals are halted by the circuit breaker");
        // An execution in flight still owns the pending proposal: its callbacks clear it.
        require!(
            !self.execution_locked(now_sec()),
            "an execution is in progress; propose after it finishes"
        );
        let agent = self.agent.clone().unwrap_or_else(|| env::panic_str("no agent is configured"));
        require!(env::predecessor_account_id() == agent, "only the agent can propose trades");
        require!(
            self.mandate.allowed_model_ids.contains(&model_id),
            "the declared model id is not allowed"
        );

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
        self.next_proposal_id =
            self.next_proposal_id.checked_add(1).expect("proposal id exhausted");
        self.pending = Some(Proposal {
            id: U64(id),
            asset_in,
            asset_out,
            amount_in,
            min_amount_out,
            reason_hash,
            created_sec: now,
            expires_sec: now + PROPOSAL_TTL_SEC,
            model_id,
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
        self.advance_revision();
        self.paused = true;
    }

    /// Resuming is sensitive, so it requires a full-access key (1 yoctoNEAR).
    #[payable]
    pub fn unpause(&mut self) {
        assert_one_yocto();
        self.assert_owner();
        self.paused = false;
    }

    /// Decides the pending proposal and, if every gate passes, has the MPC
    /// signer sign the intent. Permissionless: anyone may trigger it, because
    /// the vault builds every signed byte itself.
    ///
    /// The calls run as one chain: vault balances, oracle prices, the oracle's
    /// publish time for each feed, the intents salt, then `sign`. A closed
    /// gate rejects the proposal and counts a strike. A failed read or
    /// signature leaves the proposal pending until it expires, with no strike.
    pub fn execute(&mut self) -> Promise {
        self.assert_not_paused();
        require!(self.active_withdrawal.is_none(), "a withdrawal is unresolved");
        require!(!self.halted, "proposals are halted by the circuit breaker");
        let proposal =
            self.pending.clone().unwrap_or_else(|| env::panic_str("no pending proposal"));
        let now = now_sec();
        require!(!self.execution_locked(now), "an execution is already in progress");
        require!(self.key_registered, "the intents key is not registered");
        require!(now < proposal.expires_sec, "the pending proposal has expired");
        require!(
            self.mandate.allowed_model_ids.contains(&proposal.model_id),
            "the declared model id is no longer allowed"
        );
        require!(
            self.trading.signed_intent_expires_at_sec <= now,
            "a signed intent is still outstanding"
        );
        let execution = U64(self.next_execution_id);
        self.next_execution_id =
            self.next_execution_id.checked_add(1).expect("execution id exhausted");
        self.in_flight = Some(InFlight { id: execution, started_sec: now });
        self.execution_context = Some(ExecutionContext {
            id: execution,
            revision: self.config_revision,
            assets: self.mandate.assets.clone(),
            proposal_id: proposal.id,
            price_valid_until_sec: None,
        });

        let token_ids: Vec<String> =
            self.mandate.assets.iter().map(|a| a.asset_id.clone()).collect();
        Promise::new(self.intents.clone())
            .function_call(
                "mt_batch_balance_of".to_string(),
                json!({ "account_id": env::current_account_id(), "token_ids": token_ids })
                    .to_string()
                    .into_bytes(),
                NearToken::from_yoctonear(0),
                GAS_READ_BALANCES,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_CALLBACK)
                    .on_balances(proposal, execution),
            )
    }

    #[private]
    pub fn on_balances(
        &mut self,
        #[callback_result(max_bytes = MAX_ANSWER_BYTES)] balances: Result<Vec<U128>, PromiseError>,
        proposal: Proposal,
        execution: U64,
    ) -> PromiseOrValue<ExecutionOutcome> {
        if !self.is_current(execution) {
            return PromiseOrValue::Value(superseded());
        }
        if !self.configuration_current(execution) {
            return PromiseOrValue::Value(self.failed("the execution configuration changed"));
        }
        let requested = self.execution_context.as_ref().expect("execution snapshot").assets.clone();
        let balances = match balances {
            Ok(b) if b.len() == requested.len() => b,
            _ => return PromiseOrValue::Value(self.failed("could not read vault balances")),
        };
        let held: Vec<(String, U128)> =
            requested.iter().zip(balances).map(|(a, b)| (a.asset_id.clone(), b)).collect();

        // Price every asset the trade touches or the vault holds, as the engine checks.
        let mut feeds = Vec::new();
        for (asset, (id, balance)) in requested.iter().zip(&held) {
            if balance.0 > 0 || *id == proposal.asset_in || *id == proposal.asset_out {
                feeds.push(asset.price_feed_id.clone());
            }
        }
        if self.oracle_kind == OracleKind::Pyth {
            feeds.sort_unstable();
            feeds.dedup();
            return PromiseOrValue::Promise(
                Promise::new(self.oracle.clone())
                    .function_call(
                        "list_prices_no_older_than".to_string(),
                        json!({ "price_ids": feeds, "age": self.mandate.max_price_age_sec })
                            .to_string()
                            .into_bytes(),
                        NearToken::from_yoctonear(0),
                        GAS_READ_PRICES,
                    )
                    .then(
                        Self::ext(env::current_account_id())
                            .with_static_gas(GAS_CALLBACK)
                            .on_pyth_prices(proposal, held, execution),
                    ),
            );
        }
        PromiseOrValue::Promise(
            Promise::new(self.oracle.clone())
                .function_call(
                    "get_price_data".to_string(),
                    json!({ "asset_ids": feeds.clone() }).to_string().into_bytes(),
                    NearToken::from_yoctonear(0),
                    GAS_READ_PRICES,
                )
                .then(
                    Self::ext(env::current_account_id())
                        .with_static_gas(GAS_CALLBACK)
                        .on_price_data(proposal, held, feeds, execution),
                ),
        )
    }

    #[private]
    pub fn on_pyth_prices(
        &mut self,
        proposal: Proposal,
        held: Vec<(String, U128)>,
        execution: U64,
    ) -> PromiseOrValue<ExecutionOutcome> {
        if !self.is_current(execution) {
            return PromiseOrValue::Value(superseded());
        }
        if !self.configuration_current(execution) {
            return PromiseOrValue::Value(self.failed("the execution configuration changed"));
        }
        if self.oracle_kind != OracleKind::Pyth {
            return PromiseOrValue::Value(self.failed("the oracle provider does not match"));
        }
        if !self.window_open() {
            return PromiseOrValue::Value(self.failed("the execution signing window expired"));
        }
        let prices = match read_promise::<pyth::PythPrices>(0) {
            Ok(prices) => prices,
            Err(reason) => return PromiseOrValue::Value(self.failed(&reason)),
        };
        let market = Market {
            now_sec: now_sec(),
            prices: pyth::prices_for(&self.mandate, &prices),
            balances: held.into_iter().map(|(id, balance)| (id, balance.0)).collect(),
        };
        self.approve_market(proposal, market, execution)
    }

    #[private]
    pub fn on_price_data(
        &mut self,
        #[callback_result(max_bytes = MAX_ANSWER_BYTES)] data: Result<PriceData, PromiseError>,
        proposal: Proposal,
        held: Vec<(String, U128)>,
        feeds: Vec<String>,
        execution: U64,
    ) -> PromiseOrValue<ExecutionOutcome> {
        if !self.is_current(execution) {
            return PromiseOrValue::Value(superseded());
        }
        if !self.configuration_current(execution) {
            return PromiseOrValue::Value(self.failed("the execution configuration changed"));
        }
        let data = match data {
            Ok(d) => d,
            Err(_) => return PromiseOrValue::Value(self.failed("could not read oracle prices")),
        };
        // One `get_asset` per feed, joined, so every publish time arrives in one callback.
        let reads = feeds
            .iter()
            .map(|feed| {
                Promise::new(self.oracle.clone()).function_call(
                    "get_asset".to_string(),
                    json!({ "asset_id": feed }).to_string().into_bytes(),
                    NearToken::from_yoctonear(0),
                    GAS_READ_ASSET,
                )
            })
            .reduce(Promise::and);
        let Some(reads) = reads else {
            return PromiseOrValue::Value(self.failed("no price feeds to read"));
        };
        PromiseOrValue::Promise(
            reads.then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_CALLBACK)
                    .on_assets(proposal, held, feeds, data, execution),
            ),
        )
    }

    #[private]
    pub fn on_assets(
        &mut self,
        proposal: Proposal,
        held: Vec<(String, U128)>,
        feeds: Vec<String>,
        data: PriceData,
        execution: U64,
    ) -> PromiseOrValue<ExecutionOutcome> {
        if !self.is_current(execution) {
            return PromiseOrValue::Value(superseded());
        }
        if !self.configuration_current(execution) {
            return PromiseOrValue::Value(self.failed("the execution configuration changed"));
        }
        if data.timestamp.0 > env::block_timestamp().saturating_add(5_000_000_000) {
            return PromiseOrValue::Value(self.failed("the oracle snapshot is in the future"));
        }
        let mut assets = BTreeMap::new();
        for (i, feed) in feeds.into_iter().enumerate() {
            match read_promise::<Option<AssetView>>(i as u64) {
                Ok(Some(view)) => {
                    assets.insert(feed, view);
                }
                Ok(None) => {}
                Err(reason) => return PromiseOrValue::Value(self.failed(&reason)),
            }
        }
        let market = Market {
            now_sec: now_sec(),
            prices: prices_for(&self.mandate, &data, &assets),
            balances: held.into_iter().map(|(id, b)| (id, b.0)).collect(),
        };
        self.approve_market(proposal, market, execution)
    }

    #[private]
    pub fn on_salt(
        &mut self,
        #[callback_result(max_bytes = MAX_ANSWER_BYTES)] salt: Result<String, PromiseError>,
        proposal: Proposal,
        next_state: TradingState,
        execution: U64,
    ) -> PromiseOrValue<ExecutionOutcome> {
        self.sign_approved(salt, proposal, next_state, execution)
    }
}

impl Vault {
    fn approve_market(
        &mut self,
        proposal: Proposal,
        market: Market,
        execution: U64,
    ) -> PromiseOrValue<ExecutionOutcome> {
        let trade = Trade {
            asset_in: proposal.asset_in.clone(),
            asset_out: proposal.asset_out.clone(),
            amount_in: proposal.amount_in.0,
            min_amount_out: proposal.min_amount_out.0,
        };
        match check_trade(&self.mandate, &self.trading, &trade, &market) {
            Err(err) => PromiseOrValue::Value(self.reject(&err)),
            Ok(approval) => {
                self.execution_context
                    .as_mut()
                    .expect("execution snapshot")
                    .price_valid_until_sec = market
                    .prices
                    .values()
                    .map(|price| {
                        price
                            .publish_time_sec
                            .saturating_add(u64::from(self.mandate.max_price_age_sec))
                    })
                    .min();
                PromiseOrValue::Promise(
                    Promise::new(self.intents.clone())
                        .function_call(
                            "current_salt".to_string(),
                            b"{}".to_vec(),
                            NearToken::from_yoctonear(0),
                            GAS_READ_SALT,
                        )
                        .then(
                            Self::ext(env::current_account_id())
                                .with_static_gas(GAS_CALLBACK)
                                .on_salt(proposal, approval.next_state, execution),
                        ),
                )
            }
        }
    }

    fn sign_approved(
        &mut self,
        salt: Result<String, PromiseError>,
        proposal: Proposal,
        next_state: TradingState,
        execution: U64,
    ) -> PromiseOrValue<ExecutionOutcome> {
        if !self.is_current(execution) {
            return PromiseOrValue::Value(superseded());
        }
        // Past the window, no new intent may be built: its deadline could outlive the lock.
        if !self.window_open() {
            return PromiseOrValue::Value(self.failed("the execution window has passed"));
        }
        // The owner's pause also stops an execution that is already under way.
        if self.paused {
            return PromiseOrValue::Value(self.failed("the vault is paused"));
        }
        if !self.configuration_current(execution) {
            return PromiseOrValue::Value(self.failed("the execution configuration changed"));
        }
        if next_state.signed_intent_expires_at_sec <= now_sec() {
            return PromiseOrValue::Value(self.failed("the approved intent deadline has passed"));
        }
        let Some(price_deadline) =
            self.execution_context.as_ref().and_then(|ctx| ctx.price_valid_until_sec)
        else {
            return PromiseOrValue::Value(self.failed("the price approval is not recorded"));
        };
        if now_sec() > price_deadline {
            return PromiseOrValue::Value(self.failed("the approved prices have become stale"));
        }
        let salt = salt
            .ok()
            .and_then(|hex| from_hex(&hex))
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok());
        let Some(salt) = salt else {
            return PromiseOrValue::Value(self.failed("could not read the intents salt"));
        };
        let deadline_sec = next_state.signed_intent_expires_at_sec;
        // Only needs to be unlikely to repeat: the salt and deadline do most of the work.
        let random: [u8; 15] = env::random_seed_array()[..15].try_into().expect("15 bytes");
        let nonce = match v1_nonce(salt, deadline_sec, random) {
            Ok(nonce) => nonce,
            Err(err) => return PromiseOrValue::Value(self.failed(&err.to_string())),
        };
        let signer_id = env::current_account_id().to_string();
        let payload = match token_diff_payload(&TokenDiff {
            signer_id: &signer_id,
            verifying_contract: self.intents.as_str(),
            deadline_sec,
            nonce,
            asset_in: &proposal.asset_in,
            amount_in: proposal.amount_in.0,
            asset_out: &proposal.asset_out,
            min_amount_out: proposal.min_amount_out.0,
        }) {
            Ok(payload) if payload.len() <= MAX_MPC_PAYLOAD => payload,
            Ok(_) => return PromiseOrValue::Value(self.failed("payload exceeds the MPC limit")),
            Err(err) => return PromiseOrValue::Value(self.failed(&err.to_string())),
        };

        // A signature may escape even when its callback fails or arrives too late.
        // Reserve the approved quota before dispatch; never refund it from a callback.
        self.trading = next_state.clone();
        PromiseOrValue::Promise(
            Promise::new(self.signer.clone())
                .function_call(
                    "sign".to_string(),
                    json!({
                        "request": {
                            "path": INTENTS_KEY_PATH,
                            "payload_v2": { "Eddsa": to_hex(payload.as_bytes()) },
                            "domain_id": ED25519_DOMAIN_ID,
                        }
                    })
                    .to_string()
                    .into_bytes(),
                    NearToken::from_yoctonear(1),
                    GAS_SIGN,
                )
                .then(
                    Self::ext(env::current_account_id())
                        .with_static_gas(GAS_CALLBACK)
                        .on_signed(proposal, payload, next_state, execution),
                ),
        )
    }
}

#[near]
impl Vault {
    #[private]
    pub fn on_signed(
        &mut self,
        #[callback_result(max_bytes = MAX_ANSWER_BYTES)] response: Result<
            SignatureResponse,
            PromiseError,
        >,
        proposal: Proposal,
        payload: String,
        next_state: TradingState,
        execution: U64,
    ) -> ExecutionOutcome {
        if !self.is_current(execution) {
            return superseded();
        }
        let signature = match response {
            Ok(r) if r.scheme == "Ed25519" && r.signature.len() == 64 => r.signature,
            // Post-request failure cannot prove that no valid signature became visible.
            Err(PromiseError::Failed) => {
                return self.failed_holding_lock("the MPC signer call failed");
            }
            Err(_) => return self.failed_holding_lock("the MPC signer answer was not readable"),
            // An answer that is not an Ed25519 signature may still carry one: keep the lock.
            Ok(_) => {
                return self
                    .failed_holding_lock("the MPC signer did not return an Ed25519 signature");
            }
        };
        let Some(public_key) = self.intents_key.clone() else {
            return self.failed_holding_lock("the intents key is not recorded");
        };
        let expires_sec = next_state.signed_intent_expires_at_sec;
        self.signed_intent = Some(SignedIntent {
            proposal_id: proposal.id,
            payload,
            public_key,
            signature: format!("ed25519:{}", bs58::encode(&signature).into_string()),
            expires_sec,
        });
        if self.pending.as_ref().is_some_and(|p| p.id == proposal.id) {
            self.pending = None;
        }
        self.record_approval();
        self.in_flight = None;
        let outcome =
            ExecutionOutcome::Executed { proposal_id: proposal.id, expires_sec: U64(expires_sec) };
        self.last_proposal_outcome =
            Some(ProposalOutcome { proposal_id: proposal.id, outcome: outcome.clone() });
        outcome
    }
}

impl Vault {
    fn init(
        owner: AccountId,
        mandate: Mandate,
        signer: AccountId,
        intents: AccountId,
        oracle: AccountId,
        oracle_kind: OracleKind,
    ) -> Self {
        // The owner must be an account that can act on its own. The vault cannot sign as
        // itself once its full-access keys are removed, so an owner equal to the vault
        // would leave the owner controls unreachable.
        require!(
            owner != env::current_account_id(),
            "the owner cannot be the vault account itself"
        );
        withdrawal::sinks();
        validate_oracle_mandate(&mandate, oracle_kind);
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
            signer,
            intents,
            oracle,
            intents_key: None,
            key_registered: false,
            in_flight: None,
            next_execution_id: 1,
            signed_intent: None,
            pending_mandate: None,
            pending_agent: None,
            pending_withdrawal: None,
            withdrawal_seq: 0,
            config_revision: 0,
            execution_context: None,
            last_proposal_outcome: None,
            active_withdrawal: None,
            next_withdrawal_id: 1,
            last_withdrawal_outcome: None,
            oracle_kind,
            withdrawal_stage: None,
            withdrawal_retry_armed: None,
        }
    }

    /// A closed gate: counts a strike, consumes the proposal. No signature was requested.
    fn reject(&mut self, err: &TradeError) -> ExecutionOutcome {
        let proposal_id = self.pending.as_ref().map(|p| p.id);
        self.record_rejection();
        self.pending = None;
        self.in_flight = None;
        let outcome =
            ExecutionOutcome::Rejected { code: err.code().to_string(), reason: err.to_string() };
        if let Some(proposal_id) = proposal_id {
            self.last_proposal_outcome =
                Some(ProposalOutcome { proposal_id, outcome: outcome.clone() });
        }
        outcome
    }

    /// A failure before requesting signing: keeps the proposal, counts nothing.
    fn failed(&mut self, reason: &str) -> ExecutionOutcome {
        self.in_flight = None;
        ExecutionOutcome::Failed { reason: reason.to_string() }
    }

    /// A failure after the signer may have answered. The lock stays until it runs out, so a
    /// second intent for the proposal is not built while the first one could still settle.
    fn failed_holding_lock(&mut self, reason: &str) -> ExecutionOutcome {
        ExecutionOutcome::Failed { reason: reason.to_string() }
    }

    /// True while the execution holding the lock may still build and sign an intent.
    fn window_open(&self) -> bool {
        self.in_flight
            .as_ref()
            .is_some_and(|run| now_sec() < run.started_sec + EXECUTION_WINDOW_SEC)
    }

    /// True while an execution holds the lock. The lock runs out once its window and the
    /// intent's lifetime have both passed: by then no intent from it is still valid.
    fn execution_locked(&self, now: u64) -> bool {
        self.in_flight
            .as_ref()
            .is_some_and(|run| now < run.started_sec + EXECUTION_WINDOW_SEC + INTENT_TTL_SEC)
    }

    /// Callbacks act only for the execution that holds the lock. A late callback from an
    /// earlier execution changes nothing.
    fn is_current(&self, execution: U64) -> bool {
        self.execution_locked(now_sec())
            && self.in_flight.as_ref().is_some_and(|run| run.id == execution)
    }

    fn configuration_current(&self, execution: U64) -> bool {
        self.execution_context.as_ref().is_some_and(|ctx| {
            ctx.id == execution
                && ctx.revision == self.config_revision
                && self.pending.as_ref().is_some_and(|p| p.id == ctx.proposal_id)
        })
    }

    fn advance_revision(&mut self) {
        self.config_revision =
            self.config_revision.checked_add(1).expect("configuration revision exhausted");
    }

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
    pub(crate) fn record_rejection(&mut self) {
        self.strikes += 1;
        if self.strikes >= MAX_STRIKES {
            self.halted = true;
        }
    }

    /// Clears the strike count after an approved trade.
    pub(crate) fn record_approval(&mut self) {
        self.strikes = 0;
    }
}

fn now_sec() -> u64 {
    env::block_timestamp() / 1_000_000_000
}

/// Result of the cross-contract call at `index` in the current callback.
/// Joined calls (`Promise::and`) arrive as one callback, indexed in order.
fn read_promise<T: DeserializeOwned>(index: u64) -> Result<T, String> {
    if index >= env::promise_results_count() {
        return Err(format!("cross-contract read {index} is missing"));
    }
    // Bounded, so an oversized answer fails the callback cleanly instead of running out of gas.
    let bytes =
        env::promise_result_checked(index, MAX_ANSWER_BYTES).map_err(|error| match error {
            PromiseError::Failed => format!("cross-contract read {index} failed"),
            _ => format!("cross-contract read {index} exceeded the answer limit"),
        })?;
    serde_json::from_slice(&bytes).map_err(|e| format!("bad answer {index}: {e}"))
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    s.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            Some(((high << 4) | low) as u8)
        })
        .collect()
}

fn is_hex_digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// What a callback from an execution that no longer holds the lock returns. It changes nothing.
fn superseded() -> ExecutionOutcome {
    ExecutionOutcome::Failed { reason: "superseded by a newer execution".to_string() }
}

/// The token account of a `nep141:` asset id, or `None` for any other kind of asset.
fn token_account(asset_id: &str) -> Option<&str> {
    asset_id.strip_prefix("nep141:").filter(|account| account.parse::<AccountId>().is_ok())
}

fn oracle_mandate_validation(mandate: &Mandate, kind: OracleKind) -> Result<(), String> {
    mandate.validate().map_err(|error| error.to_string())?;
    if kind == OracleKind::Pyth
        && !mandate.assets.iter().all(|asset| pyth::valid_feed_id(&asset.price_feed_id))
    {
        return Err("Pyth price feed ids must be 64 lowercase hexadecimal characters".to_string());
    }
    Ok(())
}

fn validate_or_panic(mandate: &Mandate) {
    validate_oracle_mandate(mandate, OracleKind::PriceOracle);
}

fn validate_oracle_mandate(mandate: &Mandate, kind: OracleKind) {
    if let Err(error) = oracle_mandate_validation(mandate, kind) {
        env::panic_str(&error);
    }
}

#[cfg(test)]
mod provider_tests;

#[cfg(test)]
mod remediation_tests;

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
            "manual-v1".into(),
        )
    }

    #[test]
    fn unapproved_source_is_refused_before_proposal_state_changes() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        let sequence = v.next_proposal_id;
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            v.propose(
                USDC.into(),
                WNEAR.into(),
                U128(1),
                U128(1),
                REASON.into(),
                "unapproved-model".into(),
            );
        }))
        .expect_err("unknown source must be refused");
        let message = error
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| error.downcast_ref::<&str>().copied())
            .unwrap();
        assert!(message.contains("the declared model id is not allowed"));
        assert_eq!(v.next_proposal_id, sequence);
        assert!(v.pending.is_none());
        assert!(v.in_flight.is_none());
    }

    #[test]
    fn source_removal_blocks_a_pending_proposal_before_execution_lock() {
        let mut v = vault_with_agent();
        v.key_registered = true;
        context(accounts(3), 0);
        let id = propose(&mut v);
        assert_eq!(v.pending.as_ref().unwrap().model_id, "manual-v1");
        let mut tighter = v.mandate.clone();
        tighter.allowed_model_ids.retain(|source| source != "manual-v1");
        context(accounts(1), 1);
        assert_eq!(v.set_mandate(tighter), MandateChangeOutcome::Applied);
        context(accounts(4), 0);
        let sequence = v.next_execution_id;
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = v.execute();
        }))
        .expect_err("removed source must be refused");
        let message = error
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| error.downcast_ref::<&str>().copied())
            .unwrap();
        assert!(message.contains("the declared model id is no longer allowed"));
        assert_eq!(v.next_execution_id, sequence);
        assert!(v.in_flight.is_none());
        assert_eq!(v.pending.as_ref().unwrap().id, id);
        assert_eq!(v.strikes, 0);
    }

    #[test]
    fn first_source_addition_waits_the_full_mandate_delay() {
        let mut v = vault_with_agent();
        let mut empty = v.mandate.clone();
        empty.allowed_model_ids.clear();
        context(accounts(1), 1);
        assert_eq!(v.set_mandate(empty.clone()), MandateChangeOutcome::Applied);
        let mut first = empty;
        first.allowed_model_ids.push("fixture-model".into());
        let deadline = T0 + MANDATE_DELAY_SEC;
        assert_eq!(
            v.set_mandate(first),
            MandateChangeOutcome::Queued { effective_sec: U64(deadline) }
        );
        assert!(v.mandate.allowed_model_ids.is_empty());
        context_at(accounts(4), 0, deadline - 1);
        let error =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| v.apply_pending_mandate()))
                .expect_err("source must not apply early");
        let message = error
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| error.downcast_ref::<&str>().copied())
            .unwrap();
        assert!(message.contains("the mandate change is still in its delay"));
        assert!(v.mandate.allowed_model_ids.is_empty());
        context_at(accounts(4), 0, deadline);
        v.apply_pending_mandate();
        assert_eq!(v.mandate.allowed_model_ids, ["fixture-model"]);
        assert!(v.pending_mandate.is_none());
    }

    #[test]
    fn init_stores_owner_and_mandate() {
        let v = vault();
        assert_eq!(v.get_owner(), accounts(1));
        assert_eq!(v.get_mandate(), sample());
        assert!(!v.is_paused());
        assert_eq!(v.get_agent(), None);
        assert_eq!(
            v.get_externals(),
            (
                MAINNET_SIGNER.parse().unwrap(),
                MAINNET_INTENTS.parse().unwrap(),
                MAINNET_ORACLE.parse().unwrap(),
            )
        );
        assert_eq!(v.get_intents_key(), None);
        assert!(!v.is_key_registered());
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
    fn tightening_applies_at_once() {
        let mut v = vault();
        context(accounts(1), 1);
        let tighter = Mandate { max_trade_usd: U64(10_000_000), ..sample() };
        assert_eq!(v.set_mandate(tighter.clone()), MandateChangeOutcome::Applied);
        assert_eq!(v.get_mandate(), tighter);
        assert_eq!(v.get_pending_mandate(), None);
    }

    #[test]
    fn loosening_waits_for_the_delay_and_the_old_limits_hold_meanwhile() {
        let mut v = vault();
        context(accounts(1), 1);
        let looser = Mandate { max_slippage_bps: 200, ..sample() };
        let outcome = v.set_mandate(looser.clone());
        assert_eq!(
            outcome,
            MandateChangeOutcome::Queued { effective_sec: U64(T0 + MANDATE_DELAY_SEC) }
        );
        assert_eq!(v.get_mandate(), sample());
        assert_eq!(v.get_pending_mandate().map(|p| p.mandate), Some(looser.clone()));

        context_at(accounts(2), 0, T0 + MANDATE_DELAY_SEC);
        v.apply_pending_mandate();
        assert_eq!(v.get_mandate(), looser);
        assert_eq!(v.get_pending_mandate(), None);
    }

    #[test]
    #[should_panic(expected = "the mandate change is still in its delay")]
    fn a_loosening_cannot_be_applied_early() {
        let mut v = vault();
        context(accounts(1), 1);
        v.set_mandate(Mandate { max_slippage_bps: 200, ..sample() });
        context_at(accounts(2), 0, T0 + MANDATE_DELAY_SEC - 1);
        v.apply_pending_mandate();
    }

    #[test]
    fn the_owner_can_cancel_a_queued_loosening() {
        let mut v = vault();
        context(accounts(1), 1);
        v.set_mandate(Mandate { max_slippage_bps: 200, ..sample() });
        v.cancel_pending_mandate();
        assert_eq!(v.get_pending_mandate(), None);
        assert_eq!(v.get_mandate(), sample());
    }

    #[test]
    fn a_second_loosening_replaces_the_first() {
        let mut v = vault();
        context(accounts(1), 1);
        v.set_mandate(Mandate { max_slippage_bps: 200, ..sample() });
        let second = Mandate { max_slippage_bps: 300, ..sample() };
        v.set_mandate(second.clone());
        assert_eq!(v.get_pending_mandate().map(|p| p.mandate), Some(second));
    }

    #[test]
    fn a_tightening_drops_a_queued_loosening() {
        let mut v = vault();
        context(accounts(1), 1);
        v.set_mandate(Mandate { max_slippage_bps: 200, ..sample() });
        let tighter = Mandate { max_slippage_bps: 50, ..sample() };
        assert_eq!(v.set_mandate(tighter.clone()), MandateChangeOutcome::Applied);
        assert_eq!(v.get_pending_mandate(), None);
        assert_eq!(v.get_mandate(), tighter);
    }

    #[test]
    fn simulate_reports_the_effect_and_changes_nothing() {
        let v = vault();
        let sim = v.simulate_mandate_change(Mandate { max_slippage_bps: 200, ..sample() });
        assert!(sim.valid);
        assert_eq!(sim.loosened_fields, vec!["max_slippage_bps".to_string()]);
        assert_eq!(
            sim.outcome,
            Some(MandateChangeOutcome::Queued { effective_sec: U64(T0 + MANDATE_DELAY_SEC) })
        );
        assert_eq!(v.get_pending_mandate(), None);
        assert_eq!(v.get_mandate(), sample());

        let bad = v.simulate_mandate_change(Mandate { max_slippage_bps: 501, ..sample() });
        assert!(!bad.valid);
        assert!(bad.error.unwrap().contains("max_slippage_bps"));
        assert_eq!(bad.outcome, None);
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

    // ---- Withdrawals: the owner queues, anyone sends after the delay ----

    fn queue_withdrawal(v: &mut Vault) {
        context_at(accounts(1), 1, T0);
        v.request_withdrawal(USDC.into(), accounts(4), U128(1_000_000));
    }

    fn sent_phase(v: &mut Vault) -> U64 {
        let active = v.active_withdrawal.as_mut().expect("active");
        active.balance_before = Some(active.request.amount);
        v.withdrawal_stage = Some(WithdrawalStage::Dispatched);
        active.id
    }

    #[test]
    fn a_withdrawal_waits_for_the_delay() {
        let mut v = vault();
        queue_withdrawal(&mut v);
        assert_eq!(
            v.get_pending_withdrawal(),
            Some(PendingWithdrawal {
                asset_id: USDC.into(),
                receiver: accounts(4),
                amount: U128(1_000_000),
                effective_sec: T0 + WITHDRAW_DELAY_SEC,
            })
        );
    }

    #[test]
    #[should_panic(expected = "only the owner can call this method")]
    fn request_withdrawal_rejects_non_owner() {
        let mut v = vault();
        context_at(accounts(2), 1, T0);
        v.request_withdrawal(USDC.into(), accounts(4), U128(1));
    }

    #[test]
    #[should_panic(expected = "Requires attached deposit of exactly 1 yoctoNEAR")]
    fn request_withdrawal_requires_one_yocto() {
        let mut v = vault();
        context_at(accounts(1), 0, T0);
        v.request_withdrawal(USDC.into(), accounts(4), U128(1));
    }

    #[test]
    #[should_panic(expected = "the asset is not in the mandate")]
    fn request_withdrawal_refuses_an_unlisted_asset() {
        let mut v = vault();
        context_at(accounts(1), 1, T0);
        v.request_withdrawal("nep141:scam.near".into(), accounts(4), U128(1));
    }

    #[test]
    #[should_panic(expected = "the amount must be positive")]
    fn request_withdrawal_refuses_zero() {
        let mut v = vault();
        context_at(accounts(1), 1, T0);
        v.request_withdrawal(USDC.into(), accounts(4), U128(0));
    }

    #[test]
    #[should_panic(expected = "an execution is in progress")]
    fn request_withdrawal_refuses_while_the_lock_holds() {
        let mut v = vault();
        v.in_flight = running(1, T0);
        context_at(accounts(1), 1, T0);
        v.request_withdrawal(USDC.into(), accounts(4), U128(1));
    }

    #[test]
    #[should_panic(expected = "a signed intent is still outstanding")]
    fn request_withdrawal_refuses_while_a_signed_intent_is_open() {
        let mut v = vault();
        v.trading.signed_intent_expires_at_sec = T0 + 120;
        context_at(accounts(1), 1, T0);
        v.request_withdrawal(USDC.into(), accounts(4), U128(1));
    }

    #[test]
    #[should_panic(expected = "the withdrawal is still in its delay")]
    fn apply_withdrawal_refused_before_the_delay() {
        let mut v = vault();
        queue_withdrawal(&mut v);
        context_at(accounts(2), 0, T0 + WITHDRAW_DELAY_SEC - 1);
        let _ = v.apply_pending_withdrawal();
    }

    #[test]
    fn apply_withdrawal_takes_it_after_the_delay() {
        let mut v = vault();
        queue_withdrawal(&mut v);
        context_at(accounts(2), 0, T0 + WITHDRAW_DELAY_SEC);
        let _ = v.apply_pending_withdrawal();
        assert_eq!(v.get_pending_withdrawal(), None);
    }

    #[test]
    #[should_panic(expected = "no withdrawal is queued")]
    fn apply_without_a_withdrawal_panics() {
        let mut v = vault();
        context_at(accounts(2), 0, T0 + WITHDRAW_DELAY_SEC);
        let _ = v.apply_pending_withdrawal();
    }

    #[test]
    #[should_panic(expected = "a signed intent is still outstanding")]
    fn apply_withdrawal_refused_while_a_signed_intent_is_open() {
        let mut v = vault();
        queue_withdrawal(&mut v);
        v.trading.signed_intent_expires_at_sec = T0 + WITHDRAW_DELAY_SEC + 60;
        context_at(accounts(2), 0, T0 + WITHDRAW_DELAY_SEC);
        let _ = v.apply_pending_withdrawal();
    }

    #[test]
    fn a_failed_dispatched_withdrawal_stays_locked_without_retry() {
        let mut v = vault();
        queue_withdrawal(&mut v);
        context_at(accounts(2), 0, T0 + WITHDRAW_DELAY_SEC);
        let _ = v.apply_pending_withdrawal();
        let id = sent_phase(&mut v);
        context(accounts(0), 0);
        v.finish_withdrawal(id, Err("cross-contract read 0 failed".into()));
        assert_eq!(v.get_pending_withdrawal(), None);
        assert!(v.active_withdrawal.is_some());
    }

    #[test]
    fn a_refunded_withdrawal_is_queued_again() {
        // intents.near answers a refund with a zero transfer, which is not delivery.
        let mut v = vault();
        queue_withdrawal(&mut v);
        let pending = v.get_pending_withdrawal().expect("queued");
        context_at(accounts(2), 0, T0 + WITHDRAW_DELAY_SEC);
        let _ = v.apply_pending_withdrawal();
        let id = sent_phase(&mut v);
        context(accounts(0), 0);
        v.finish_withdrawal(id, Ok(U128(0)));
        assert_eq!(v.get_pending_withdrawal(), Some(pending));
    }

    #[test]
    fn a_delivered_withdrawal_is_not_queued_again() {
        let mut v = vault();
        queue_withdrawal(&mut v);
        context_at(accounts(2), 0, T0 + WITHDRAW_DELAY_SEC);
        let _ = v.apply_pending_withdrawal();
        let id = sent_phase(&mut v);
        context(accounts(0), 0);
        v.finish_withdrawal(id, Ok(U128(1_000_000)));
        assert_eq!(v.get_pending_withdrawal(), None);
    }

    #[test]
    fn a_failed_withdrawal_does_not_replace_a_newer_request() {
        let mut v = vault();
        queue_withdrawal(&mut v);
        context_at(accounts(2), 0, T0 + WITHDRAW_DELAY_SEC);
        let _ = v.apply_pending_withdrawal();
        let id = sent_phase(&mut v);
        context_at(accounts(1), 1, T0 + WITHDRAW_DELAY_SEC);
        v.request_withdrawal(USDC.into(), accounts(5), U128(2_000_000));
        let newer = v.get_pending_withdrawal();
        context(accounts(0), 0);
        v.finish_withdrawal(id, Err("cross-contract read 0 failed".into()));
        assert_eq!(v.get_pending_withdrawal(), newer);
    }

    #[test]
    fn a_failed_withdrawal_does_not_return_after_a_cancel() {
        // Sent A; queued B; cancelled B; then A fails. A must not come back.
        let mut v = vault();
        queue_withdrawal(&mut v);
        context_at(accounts(2), 0, T0 + WITHDRAW_DELAY_SEC);
        let _ = v.apply_pending_withdrawal();
        let id = sent_phase(&mut v);
        context_at(accounts(1), 1, T0 + WITHDRAW_DELAY_SEC);
        v.request_withdrawal(USDC.into(), accounts(5), U128(2_000_000));
        v.cancel_pending_withdrawal();
        context(accounts(0), 0);
        v.finish_withdrawal(id, Err("cross-contract read 0 failed".into()));
        assert_eq!(v.get_pending_withdrawal(), None);
    }

    #[test]
    #[should_panic(expected = "the receiver cannot be the vault or intents.near")]
    fn withdrawal_cannot_be_sent_to_the_vault_itself() {
        let mut v = vault();
        context_at(accounts(1), 1, T0);
        let me = env::current_account_id();
        v.request_withdrawal(USDC.into(), me, U128(1_000_000));
    }

    #[test]
    #[should_panic(expected = "the owner cannot be the vault account itself")]
    fn the_owner_cannot_be_the_vault_itself() {
        testing_env!(
            VMContextBuilder::new()
                .current_account_id(accounts(1))
                .predecessor_account_id(accounts(0))
                .block_timestamp(T0 * 1_000_000_000)
                .build()
        );
        let _ = Vault::new(accounts(1), sample());
    }

    #[test]
    fn the_owner_cancels_a_queued_withdrawal() {
        let mut v = vault();
        queue_withdrawal(&mut v);
        context_at(accounts(1), 1, T0 + 1);
        v.cancel_pending_withdrawal();
        assert_eq!(v.get_pending_withdrawal(), None);
    }

    // ---- Agent gate and proposal shape ----

    #[test]
    fn owner_sets_agent_with_one_yocto() {
        let v = vault_with_agent();
        assert_eq!(v.get_agent(), Some(accounts(3)));
        assert_eq!(v.get_pending_agent(), None);
    }

    #[test]
    #[should_panic(expected = "only the owner can call this method")]
    fn set_agent_rejects_non_owner() {
        let mut v = vault_with_agent();
        context_at(accounts(2), 1, T0);
        v.set_agent(accounts(4));
    }

    #[test]
    fn a_later_agent_waits_and_the_old_agent_keeps_its_rights() {
        let mut v = vault_with_agent();
        context_at(accounts(1), 1, T0);
        v.set_agent(accounts(4));
        assert_eq!(v.get_agent(), Some(accounts(3)));
        assert_eq!(
            v.get_pending_agent(),
            Some(PendingAgent { agent: accounts(4), effective_sec: T0 + AGENT_DELAY_SEC })
        );

        context_at(accounts(3), 0, T0 + 1);
        propose(&mut v);
        assert!(v.get_pending_proposal().is_some());
    }

    #[test]
    fn a_second_queued_agent_replaces_the_first() {
        let mut v = vault_with_agent();
        context_at(accounts(1), 1, T0);
        v.set_agent(accounts(4));
        v.set_agent(accounts(5));
        assert_eq!(v.get_pending_agent().map(|p| p.agent), Some(accounts(5)));
    }

    #[test]
    #[should_panic(expected = "the agent change is still in its delay")]
    fn a_queued_agent_does_not_apply_early() {
        let mut v = vault_with_agent();
        context_at(accounts(1), 1, T0);
        v.set_agent(accounts(4));
        context_at(accounts(2), 0, T0 + AGENT_DELAY_SEC - 1);
        v.apply_pending_agent();
    }

    #[test]
    fn a_queued_agent_applies_after_the_delay() {
        let mut v = vault_with_agent();
        context_at(accounts(1), 1, T0);
        v.set_agent(accounts(4));
        context_at(accounts(2), 0, T0 + AGENT_DELAY_SEC);
        v.apply_pending_agent();
        assert_eq!(v.get_agent(), Some(accounts(4)));
        assert_eq!(v.get_pending_agent(), None);
    }

    #[test]
    fn a_queued_agent_can_be_cancelled() {
        let mut v = vault_with_agent();
        context_at(accounts(1), 1, T0);
        v.set_agent(accounts(4));
        v.cancel_pending_agent();
        assert_eq!(v.get_pending_agent(), None);
        assert_eq!(v.get_agent(), Some(accounts(3)));
    }

    #[test]
    #[should_panic(expected = "no agent change is queued")]
    fn cancel_without_a_queued_agent_panics() {
        let mut v = vault_with_agent();
        context_at(accounts(1), 1, T0);
        v.cancel_pending_agent();
    }

    #[test]
    #[should_panic(expected = "no agent change is queued")]
    fn apply_without_a_queued_agent_panics() {
        let mut v = vault_with_agent();
        context_at(accounts(2), 0, T0 + AGENT_DELAY_SEC);
        v.apply_pending_agent();
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

    /// The lock as an execution with `id` holds it, started at `started`.
    fn running(id: u64, started: u64) -> Option<InFlight> {
        Some(InFlight { id: U64(id), started_sec: started })
    }

    #[test]
    #[should_panic(expected = "an execution is in progress")]
    fn a_proposal_waits_for_the_execution_in_flight() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        v.in_flight = running(1, T0);
        propose(&mut v);
    }

    #[test]
    #[should_panic(expected = "an execution is already in progress")]
    fn execute_refuses_while_the_lock_holds() {
        let mut v = vault_with_agent();
        v.key_registered = true;
        context(accounts(3), 0);
        propose(&mut v);
        v.in_flight = running(1, T0);
        context(accounts(2), 0);
        let _ = v.execute();
    }

    #[test]
    fn the_lock_runs_out_after_the_window_and_the_intent_lifetime() {
        let mut v = vault_with_agent();
        v.in_flight = running(1, T0);
        let runs_out = T0 + EXECUTION_WINDOW_SEC + INTENT_TTL_SEC;
        context_at(accounts(2), 0, runs_out - 1);
        assert!(v.is_in_flight());
        context_at(accounts(2), 0, runs_out);
        assert!(!v.is_in_flight());
    }

    #[test]
    fn a_proposal_goes_through_once_the_lock_has_run_out() {
        let mut v = vault_with_agent();
        v.in_flight = running(1, T0);
        context_at(accounts(3), 0, T0 + EXECUTION_WINDOW_SEC + INTENT_TTL_SEC);
        propose(&mut v);
        assert!(v.get_pending_proposal().is_some());
    }

    #[test]
    fn each_execution_takes_a_new_id_and_the_next_waits_for_the_lock() {
        let mut v = vault_with_agent();
        v.key_registered = true;
        context(accounts(3), 0);
        propose(&mut v);
        context(accounts(2), 0);
        let _ = v.execute();
        assert_eq!(v.in_flight, running(1, T0));

        let runs_out = T0 + EXECUTION_WINDOW_SEC + INTENT_TTL_SEC;
        context_at(accounts(3), 0, runs_out);
        propose(&mut v);
        context_at(accounts(2), 0, runs_out);
        let _ = v.execute();
        assert_eq!(v.in_flight, running(2, runs_out));
    }

    #[test]
    fn a_late_salt_builds_no_intent_and_frees_the_lock() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        propose(&mut v);
        let proposal = v.get_pending_proposal().expect("pending");
        v.in_flight = running(1, T0);
        context_at(accounts(0), 0, T0 + EXECUTION_WINDOW_SEC);
        match v.on_salt(Ok("00000000".to_string()), proposal, TradingState::default(), U64(1)) {
            PromiseOrValue::Value(ExecutionOutcome::Failed { reason }) => {
                assert_eq!(reason, "the execution window has passed");
            }
            _ => panic!("a late salt must not start a signature"),
        }
        assert_eq!(v.in_flight, None);
        assert!(v.get_pending_proposal().is_some(), "a failed run keeps the proposal");
    }

    #[test]
    fn a_callback_from_an_earlier_execution_changes_nothing() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        propose(&mut v);
        let proposal = v.get_pending_proposal().expect("pending");
        v.in_flight = running(2, T0);
        context(accounts(0), 0);

        match v.on_salt(
            Err(PromiseError::Failed),
            proposal.clone(),
            TradingState::default(),
            U64(1),
        ) {
            PromiseOrValue::Value(ExecutionOutcome::Failed { reason }) => {
                assert_eq!(reason, "superseded by a newer execution");
            }
            _ => panic!("a stale salt must do nothing"),
        }
        let late = SignatureResponse { scheme: "Ed25519".to_string(), signature: vec![0; 64] };
        let outcome =
            v.on_signed(Ok(late), proposal, String::new(), TradingState::default(), U64(1));
        assert!(matches!(outcome, ExecutionOutcome::Failed { .. }));

        assert_eq!(v.in_flight, running(2, T0));
        assert_eq!(v.get_pending_proposal().map(|p| p.id), Some(U64(1)));
        assert_eq!(v.strikes, 0);
        assert_eq!(v.get_signed_intent(), None);
    }

    #[test]
    fn a_failed_signer_call_keeps_the_timed_lock() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        propose(&mut v);
        let proposal = v.get_pending_proposal().expect("pending");
        v.in_flight = running(1, T0);
        context(accounts(0), 0);
        v.on_signed(
            Err(PromiseError::Failed),
            proposal,
            String::new(),
            TradingState::default(),
            U64(1),
        );
        assert_eq!(v.in_flight, running(1, T0));
    }

    #[test]
    fn a_malformed_signer_answer_keeps_the_lock() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        propose(&mut v);
        let proposal = v.get_pending_proposal().expect("pending");
        v.in_flight = running(1, T0);
        context(accounts(0), 0);
        let odd = SignatureResponse { scheme: "ECDSA".to_string(), signature: vec![0; 64] };
        v.on_signed(Ok(odd), proposal, String::new(), TradingState::default(), U64(1));
        assert_eq!(v.in_flight, running(1, T0));
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
        v.propose(USDC.into(), USDC.into(), U128(1), U128(1), REASON.into(), "manual-v1".into());
    }

    #[test]
    #[should_panic(expected = "asset_out is not in the mandate")]
    fn propose_rejects_unlisted_asset() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        v.propose(
            USDC.into(),
            "nep141:pepe.near".into(),
            U128(1),
            U128(1),
            REASON.into(),
            "manual-v1".into(),
        );
    }

    #[test]
    #[should_panic(expected = "amounts must be non-zero")]
    fn propose_rejects_zero_amount() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        v.propose(USDC.into(), WNEAR.into(), U128(0), U128(1), REASON.into(), "manual-v1".into());
    }

    #[test]
    #[should_panic(expected = "reason_hash must be 64 hex characters")]
    fn propose_rejects_bad_reason_hash() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        v.propose(
            USDC.into(),
            WNEAR.into(),
            U128(1),
            U128(1),
            "not-a-hash".into(),
            "manual-v1".into(),
        );
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
