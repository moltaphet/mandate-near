//! Script agent for the Mandate vault.
//!
//! It proposes one trade and reports what the vault decided. It holds no
//! withdrawal or owner-control authority. It can choose any trade inside the
//! mandate, including economically harmful trades that still pass the gates.
//! A permissionless keeper runs `execute`; the vault constructs the signed bytes.
//!
//! Decision evidence stays off chain. Manual/rebalance proposals commit to the
//! rationale; model proposals commit to the canonical captured response envelope.

use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use near_api::{AccountId, Contract, NearGas, NetworkConfig, Signer};
use near_sdk::json_types::{U64, U128};
use near_sdk::serde_json::{Value, json};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub mod autonomous;
pub mod decision;
pub mod model;
pub mod provenance;
pub mod relay;
pub mod transport;

pub type AgentError = Box<dyn Error + Send + Sync>;

/// A trade the agent wants. Mirrors the vault's `propose` arguments, plus the rationale.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub asset_in: String,
    pub asset_out: String,
    pub amount_in: U128,
    pub min_amount_out: U128,
    /// Untrusted reasoning text. Manual/rebalance paths hash this UTF-8 text.
    pub rationale: String,
    /// Set by the model path to SHA-256 of `mandate-response-json-v1` canonical
    /// response bytes. Skipped in both directions; public plan files cannot set it.
    #[serde(skip)]
    pub response_commitment: Option<String>,
}

impl Plan {
    /// Refuses a plan the vault would refuse anyway, before any gas is spent.
    pub fn validate(&self) -> Result<(), AgentError> {
        if self.asset_in == self.asset_out {
            return Err("asset_in and asset_out must differ".into());
        }
        if self.amount_in.0 == 0 || self.min_amount_out.0 == 0 {
            return Err("amounts must be non-zero".into());
        }
        if self.rationale.trim().is_empty() {
            return Err(
                "the rationale is empty: a trade with no stated reason is not proposed".into()
            );
        }
        Ok(())
    }

    /// The value the vault stores, as 64 hex characters. For a model decision this is the
    /// SHA-256 of the canonical response bytes; otherwise SHA-256 of the rationale.
    pub fn reason_hash(&self) -> String {
        if let Some(commitment) = &self.response_commitment {
            return commitment.clone();
        }
        Sha256::digest(self.rationale.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// What the vault says about one proposal.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    /// Every gate passed and the keeper's `execute` signed the intent.
    Signed { proposal_id: u64, expires_sec: u64 },
    /// The vault's retained execution result confirms rejection by this gate.
    Rejected { code: String, reason: String },
    /// The matching pending proposal expired on-chain with no active execution.
    Expired,
    /// Still pending, including execution that continues past proposal TTL.
    Waiting,
    /// The vault's bounded views do not retain a verified decision for this ID.
    /// Absence is not evidence of rejection, expiry or settlement.
    Unknown,
}

#[derive(Deserialize)]
struct PendingProposal {
    id: U64,
    expires_sec: u64,
}

#[derive(Deserialize)]
struct SignedIntentView {
    proposal_id: U64,
    expires_sec: u64,
}

#[derive(Deserialize)]
struct ProposalDecision {
    proposal_id: U64,
    outcome: Value,
}

pub struct Agent {
    pub network: NetworkConfig,
    pub vault: AccountId,
    /// The account the vault names as its agent. It pays for `propose`.
    pub account: AccountId,
    pub signer: Arc<Signer>,
}

impl Agent {
    /// Submits the plan, if the vault is open for a new proposal. Returns the proposal id.
    pub async fn propose(&self, plan: &Plan) -> Result<u64, AgentError> {
        self.propose_from_source(plan, decision::MANUAL_SOURCE_ID).await
    }

    /// The ID is a declared source label, not cryptographic model-origin proof.
    /// Legacy callers use `propose`; model/rebalance paths supply their source.
    pub async fn propose_from_source(
        &self,
        plan: &Plan,
        model_id: &str,
    ) -> Result<u64, AgentError> {
        plan.validate()?;
        self.check_open(model_id).await?;
        let id: U64 = Contract(self.vault.clone())
            .call_function(
                "propose",
                json!({
                    "asset_in": plan.asset_in,
                    "asset_out": plan.asset_out,
                    "amount_in": plan.amount_in,
                    "min_amount_out": plan.min_amount_out,
                    "reason_hash": plan.reason_hash(),
                    "model_id": model_id,
                }),
            )
            .transaction()
            .gas(NearGas::from_tgas(30))
            .with_signer(self.account.clone(), self.signer.clone())
            .send_to(&self.network)
            .await
            .map_err(|_| "proposal transaction transport or signing failed")?
            .into_result()
            .map_err(|_| "vault refused the proposal transaction")?
            .json()
            .map_err(|_| "vault returned an invalid proposal identifier")?;
        Ok(id.0)
    }

    /// The vault's retained view of one proposal. A missing historical record is
    /// `Unknown`, not proof that a gate rejected the proposal.
    pub async fn outcome(&self, proposal_id: u64) -> Result<Outcome, AgentError> {
        // Read time first, then execution and finally pending state. If this chain
        // time is past TTL, a new execution cannot legitimately begin afterward.
        // Later views may advance, so uncertainty stays Waiting/Unknown.
        let now = self.read::<u64>("get_chain_time_sec").await?;
        let in_flight = self.read::<bool>("is_in_flight").await?;
        let signed = self.read::<Option<SignedIntentView>>("get_signed_intent").await?;
        let decision = self.read::<Option<ProposalDecision>>("get_last_proposal_outcome").await?;
        let pending = self.read::<Option<PendingProposal>>("get_pending_proposal").await?;
        resolve_outcome(
            proposal_id,
            signed.as_ref(),
            decision.as_ref(),
            pending.as_ref(),
            in_flight,
            now,
        )
    }

    /// Asks the vault the questions it would answer `no` to, so the agent can say why first.
    async fn check_open(&self, model_id: &str) -> Result<(), AgentError> {
        if self.read::<Option<String>>("get_agent").await?.as_deref() != Some(self.account.as_str())
        {
            return Err("this account is not the configured vault agent".into());
        }
        if !self.read::<bool>("is_key_registered").await? {
            return Err("vault signing identity is not registered".into());
        }
        if !self.read::<mandate_vault::Mandate>("get_mandate").await?.allows_model_id(model_id) {
            return Err("the declared model id is not allowed".into());
        }
        if self.read::<bool>("is_paused").await? {
            return Err("the vault is paused".into());
        }
        if self.read::<bool>("is_halted").await? {
            return Err("the circuit breaker has halted proposals".into());
        }
        if self.read::<bool>("is_in_flight").await? {
            return Err("an execution is in progress; wait for it to finish".into());
        }
        let now = self.read::<u64>("get_chain_time_sec").await?;
        if let Some(p) = self.read::<Option<PendingProposal>>("get_pending_proposal").await?
            && now < p.expires_sec
        {
            return Err(format!(
                "a proposal is pending until {} s; wait for it to be executed or to expire",
                p.expires_sec
            )
            .into());
        }
        if let Some(s) = self.read::<Option<SignedIntentView>>("get_signed_intent").await?
            && now < s.expires_sec
        {
            return Err(format!("a signed intent is outstanding until {} s", s.expires_sec).into());
        }
        Ok(())
    }

    async fn read<T: serde::de::DeserializeOwned + Send + Sync>(
        &self,
        method: &str,
    ) -> Result<T, AgentError> {
        Contract(self.vault.clone())
            .call_function(method, json!({}))
            .read_only::<T>()
            .fetch_from(&self.network)
            .await
            .map(|r| r.data)
            .map_err(|_| "vault view transport or schema validation failed".into())
    }
}

/// Polls until a verified terminal decision or the timeout. Unknown history keeps
/// polling because separate RPC views can briefly straddle a state transition.
pub async fn wait(
    agent: &Agent,
    proposal_id: u64,
    timeout: Duration,
    every: Duration,
) -> Result<Outcome, AgentError> {
    let started = std::time::Instant::now();
    loop {
        let o = agent.outcome(proposal_id).await?;
        if !matches!(o, Outcome::Waiting | Outcome::Unknown) || started.elapsed() >= timeout {
            return Ok(o);
        }
        tokio::time::sleep(every).await;
    }
}

fn resolve_outcome(
    proposal_id: u64,
    signed: Option<&SignedIntentView>,
    decision: Option<&ProposalDecision>,
    pending: Option<&PendingProposal>,
    in_flight: bool,
    now: u64,
) -> Result<Outcome, AgentError> {
    if let Some(signed) = signed
        && signed.proposal_id.0 == proposal_id
    {
        return Ok(Outcome::Signed { proposal_id, expires_sec: signed.expires_sec });
    }
    if let Some(decision) = decision
        && decision.proposal_id.0 == proposal_id
        && let Some(rejected) = decision.outcome.get("Rejected")
    {
        return Ok(Outcome::Rejected {
            code: rejected["code"].as_str().ok_or("invalid recorded rejection code")?.to_owned(),
            reason: rejected["reason"]
                .as_str()
                .ok_or("invalid recorded rejection reason")?
                .to_owned(),
        });
    }
    match pending {
        Some(p) if p.id.0 == proposal_id && (in_flight || now < p.expires_sec) => {
            Ok(Outcome::Waiting)
        }
        Some(p) if p.id.0 == proposal_id => Ok(Outcome::Expired),
        _ => Ok(Outcome::Unknown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(rationale: &str) -> Plan {
        Plan {
            asset_in: "nep141:usdc.sandbox".into(),
            asset_out: "nep141:wrap.near".into(),
            amount_in: U128(2_000_000),
            min_amount_out: U128(450_000_000_000_000_000_000_000),
            rationale: rationale.into(),
            response_commitment: None,
        }
    }

    #[test]
    fn reason_hash_is_sha256_hex_of_the_rationale() {
        // SHA-256 of the empty string, a well-known value.
        assert_eq!(
            plan("").reason_hash(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(plan("x").reason_hash().len(), 64);
    }

    #[test]
    fn refuses_what_the_vault_would_refuse() {
        let mut same = plan("buy");
        same.asset_out = same.asset_in.clone();
        assert!(same.validate().unwrap_err().to_string().contains("must differ"));

        let mut zero = plan("buy");
        zero.amount_in = U128(0);
        assert!(zero.validate().unwrap_err().to_string().contains("non-zero"));

        assert!(plan("   ").validate().unwrap_err().to_string().contains("rationale"));
        assert!(plan("buy").validate().is_ok());
    }

    #[test]
    fn a_plan_reads_from_the_json_a_person_writes() {
        let text = r#"{"asset_in":"nep141:usdc.sandbox","asset_out":"nep141:wrap.near",
            "amount_in":"2000000","min_amount_out":"450000000000000000000000","rationale":"dip"}"#;
        let p: Plan = near_sdk::serde_json::from_str(text).unwrap();
        assert_eq!(p.amount_in, U128(2_000_000));
        assert_eq!(p.rationale, "dip");
    }

    #[test]
    fn active_execution_keeps_an_expired_proposal_waiting() {
        let pending = PendingProposal { id: U64(1), expires_sec: 60 };
        assert_eq!(
            resolve_outcome(1, None, None, Some(&pending), true, 61).unwrap(),
            Outcome::Waiting
        );
        assert_eq!(
            resolve_outcome(1, None, None, Some(&pending), false, 59).unwrap(),
            Outcome::Waiting
        );
        assert_eq!(
            resolve_outcome(1, None, None, Some(&pending), false, 60).unwrap(),
            Outcome::Expired
        );
    }

    #[test]
    fn missing_and_replaced_history_does_not_invent_rejection() {
        let pending = PendingProposal { id: U64(2), expires_sec: 200 };
        let signed = SignedIntentView { proposal_id: U64(3), expires_sec: 300 };
        assert_eq!(resolve_outcome(1, None, None, None, false, 100).unwrap(), Outcome::Unknown);
        assert_eq!(
            resolve_outcome(1, Some(&signed), None, Some(&pending), false, 100).unwrap(),
            Outcome::Unknown
        );
    }

    #[test]
    fn only_a_matching_recorded_gate_failure_is_rejected() {
        let rejected = ProposalDecision {
            proposal_id: U64(1),
            outcome: json!({ "Rejected": { "code": "InsufficientBalance", "reason": "balance" } }),
        };
        assert_eq!(
            resolve_outcome(1, None, Some(&rejected), None, false, 100).unwrap(),
            Outcome::Rejected { code: "InsufficientBalance".into(), reason: "balance".into() }
        );
        assert_eq!(
            resolve_outcome(2, None, Some(&rejected), None, false, 100).unwrap(),
            Outcome::Unknown
        );
        let failed = ProposalDecision {
            proposal_id: U64(1),
            outcome: json!({ "Failed": { "reason": "oracle unavailable" } }),
        };
        assert_eq!(
            resolve_outcome(1, None, Some(&failed), None, false, 100).unwrap(),
            Outcome::Unknown
        );
    }

    #[test]
    fn a_recorded_signature_is_signed_not_settled() {
        let signed = SignedIntentView { proposal_id: U64(1), expires_sec: 120 };
        assert_eq!(
            resolve_outcome(1, Some(&signed), None, None, false, 121).unwrap(),
            Outcome::Signed { proposal_id: 1, expires_sec: 120 }
        );
        assert_eq!(
            near_sdk::serde_json::to_value(Outcome::Unknown).unwrap(),
            json!({ "status": "unknown" })
        );
    }
}
