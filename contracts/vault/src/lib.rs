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
pub mod oracle;

use std::collections::BTreeMap;

use near_sdk::json_types::{U64, U128};
use near_sdk::serde::de::DeserializeOwned;
use near_sdk::serde_json::{self, json};
use near_sdk::{
    AccountId, Gas, NearToken, PanicOnDefault, Promise, PromiseError, PromiseOrValue,
    assert_one_yocto, env, near, require,
};

use crate::engine::{Market, Trade, TradeError, check_trade};
pub use crate::engine::{TradingState, TradingStateView};
use crate::intent::{TokenDiff, token_diff_payload, v1_nonce};
pub use crate::mandate::{AssetConfig, Mandate, MandateError, loosened_fields};
use crate::oracle::{AssetView, PriceData, prices_for};

/// How long a pending proposal stays executable. Matches the 60 s target in docs/PROJECT.md.
pub const PROPOSAL_TTL_SEC: u64 = 60;
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
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutionOutcome {
    /// Every gate passed and the intent is signed and stored.
    Executed { proposal_id: U64, expires_sec: U64 },
    /// A gate closed. The proposal is consumed and a strike is counted.
    Rejected { code: String, reason: String },
    /// A read or the signature failed. The proposal stays pending until it
    /// expires, and no strike is counted.
    Failed { reason: String },
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

/// A new agent waiting for its delay to pass.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingAgent {
    pub agent: AccountId,
    /// Unix seconds from which `apply_pending_agent` may apply it.
    pub effective_sec: u64,
}

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
    /// True between `execute` and its final callback. Blocks a second run.
    in_flight: bool,
    /// The latest signed intent. Replaced by each approved trade.
    signed_intent: Option<SignedIntent>,
    /// A loosened mandate waiting for its delay. At most one.
    pending_mandate: Option<PendingMandate>,
    /// A new agent waiting for its delay. At most one. The current agent keeps its rights.
    pending_agent: Option<PendingAgent>,
}

#[near]
impl Vault {
    /// For mainnet: talks to `v1.signer`, `intents.near` and `price-oracle.near`.
    #[init]
    pub fn new(owner: AccountId, mandate: Mandate) -> Self {
        Self::init(
            owner,
            mandate,
            MAINNET_SIGNER.parse().unwrap_or_else(|_| env::panic_str("bad signer id")),
            MAINNET_INTENTS.parse().unwrap_or_else(|_| env::panic_str("bad intents id")),
            MAINNET_ORACLE.parse().unwrap_or_else(|_| env::panic_str("bad oracle id")),
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
        Self::init(owner, mandate, signer, intents, oracle)
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
        self.in_flight
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
        #[callback_result] key: Result<String, PromiseError>,
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
        #[callback_result] result: Result<(), PromiseError>,
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
        validate_or_panic(&mandate);
        if loosened_fields(&self.mandate, &mandate).is_empty() {
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
        if let Err(err) = mandate.validate() {
            return MandateSimulation {
                valid: false,
                error: Some(err.to_string()),
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
        // An execution in flight still owns the pending proposal: its callbacks clear it.
        require!(!self.in_flight, "an execution is in progress; propose after it finishes");
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
        require!(!self.halted, "proposals are halted by the circuit breaker");
        require!(!self.in_flight, "an execution is already in progress");
        require!(self.key_registered, "the intents key is not registered");
        let proposal =
            self.pending.clone().unwrap_or_else(|| env::panic_str("no pending proposal"));
        let now = now_sec();
        require!(now < proposal.expires_sec, "the pending proposal has expired");
        require!(
            self.trading.signed_intent_expires_at_sec <= now,
            "a signed intent is still outstanding"
        );
        self.in_flight = true;

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
                    .on_balances(proposal),
            )
    }

    #[private]
    pub fn on_balances(
        &mut self,
        #[callback_result] balances: Result<Vec<U128>, PromiseError>,
        proposal: Proposal,
    ) -> PromiseOrValue<ExecutionOutcome> {
        let balances = match balances {
            Ok(b) if b.len() == self.mandate.assets.len() => b,
            _ => return PromiseOrValue::Value(self.failed("could not read vault balances")),
        };
        let held: Vec<(String, U128)> = self
            .mandate
            .assets
            .iter()
            .zip(balances)
            .map(|(a, b)| (a.asset_id.clone(), b))
            .collect();

        // Price every asset the trade touches or the vault holds, as the engine checks.
        let mut feeds = Vec::new();
        for (asset, (id, balance)) in self.mandate.assets.iter().zip(&held) {
            if balance.0 > 0 || *id == proposal.asset_in || *id == proposal.asset_out {
                feeds.push(asset.price_feed_id.clone());
            }
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
                        .on_price_data(proposal, held, feeds),
                ),
        )
    }

    #[private]
    pub fn on_price_data(
        &mut self,
        #[callback_result] data: Result<PriceData, PromiseError>,
        proposal: Proposal,
        held: Vec<(String, U128)>,
        feeds: Vec<String>,
    ) -> PromiseOrValue<ExecutionOutcome> {
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
                    .on_assets(proposal, held, feeds, data),
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
    ) -> PromiseOrValue<ExecutionOutcome> {
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
        let trade = Trade {
            asset_in: proposal.asset_in.clone(),
            asset_out: proposal.asset_out.clone(),
            amount_in: proposal.amount_in.0,
            min_amount_out: proposal.min_amount_out.0,
        };
        match check_trade(&self.mandate, &self.trading, &trade, &market) {
            Err(err) => PromiseOrValue::Value(self.reject(&err)),
            Ok(approval) => PromiseOrValue::Promise(
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
                            .on_salt(proposal, approval.next_state),
                    ),
            ),
        }
    }

    #[private]
    pub fn on_salt(
        &mut self,
        #[callback_result] salt: Result<String, PromiseError>,
        proposal: Proposal,
        next_state: TradingState,
    ) -> PromiseOrValue<ExecutionOutcome> {
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
                        .on_signed(proposal, payload, next_state),
                ),
        )
    }

    #[private]
    pub fn on_signed(
        &mut self,
        #[callback_result] response: Result<SignatureResponse, PromiseError>,
        proposal: Proposal,
        payload: String,
        next_state: TradingState,
    ) -> ExecutionOutcome {
        let signature = match response {
            Ok(r) if r.scheme == "Ed25519" && r.signature.len() == 64 => r.signature,
            _ => return self.failed("the MPC signer did not return an Ed25519 signature"),
        };
        let Some(public_key) = self.intents_key.clone() else {
            return self.failed("the intents key is not recorded");
        };
        let expires_sec = next_state.signed_intent_expires_at_sec;
        self.signed_intent = Some(SignedIntent {
            proposal_id: proposal.id,
            payload,
            public_key,
            signature: format!("ed25519:{}", bs58::encode(&signature).into_string()),
            expires_sec,
        });
        self.trading = next_state;
        self.pending = None;
        self.record_approval();
        self.in_flight = false;
        ExecutionOutcome::Executed { proposal_id: proposal.id, expires_sec: U64(expires_sec) }
    }
}

impl Vault {
    fn init(
        owner: AccountId,
        mandate: Mandate,
        signer: AccountId,
        intents: AccountId,
        oracle: AccountId,
    ) -> Self {
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
            signer,
            intents,
            oracle,
            intents_key: None,
            key_registered: false,
            in_flight: false,
            signed_intent: None,
            pending_mandate: None,
            pending_agent: None,
        }
    }

    /// A closed gate: counts a strike, consumes the proposal.
    fn reject(&mut self, err: &TradeError) -> ExecutionOutcome {
        self.record_rejection();
        self.pending = None;
        self.in_flight = false;
        ExecutionOutcome::Rejected { code: err.code().to_string(), reason: err.to_string() }
    }

    /// A failed read or signature: keeps the proposal, counts nothing.
    fn failed(&mut self, reason: &str) -> ExecutionOutcome {
        self.in_flight = false;
        ExecutionOutcome::Failed { reason: reason.to_string() }
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
    // Bounded, so an oversized answer fails the callback cleanly instead of running out of gas.
    let bytes = env::promise_result_checked(index, MAX_ANSWER_BYTES)
        .map_err(|_| format!("cross-contract read {index} failed"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("bad answer {index}: {e}"))
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
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

    #[test]
    #[should_panic(expected = "an execution is in progress")]
    fn a_proposal_waits_for_the_execution_in_flight() {
        let mut v = vault_with_agent();
        context(accounts(3), 0);
        v.in_flight = true;
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
