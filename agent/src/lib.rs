//! Script agent for the Mandate vault.
//!
//! It proposes one trade and reports what the vault decided. It holds no
//! authority over funds: the vault checks every gate, and a permissionless
//! keeper runs `execute`. The agent can only ask, and it cannot change the answer.
//!
//! The reasoning stays off chain. Only its SHA-256 goes into the proposal, so
//! anyone holding the rationale text can check it against the on-chain hash.

use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use near_api::{AccountId, Contract, NearGas, NetworkConfig, Signer};
use near_sdk::json_types::{U64, U128};
use near_sdk::serde_json::json;
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub type AgentError = Box<dyn Error + Send + Sync>;

/// A trade the agent wants. Mirrors the vault's `propose` arguments, plus the rationale.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Plan {
    pub asset_in: String,
    pub asset_out: String,
    pub amount_in: U128,
    pub min_amount_out: U128,
    /// The agent's reasoning, in plain text. Only its hash is sent on chain.
    pub rationale: String,
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

    /// SHA-256 of the rationale, as 64 hex characters: the value the vault stores.
    pub fn reason_hash(&self) -> String {
        Sha256::digest(self.rationale.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// What the vault says about one proposal.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Every gate passed and the keeper's `execute` signed the intent.
    Signed { proposal_id: u64, expires_sec: u64 },
    /// The proposal is gone, but nothing was signed for it: a gate closed and a strike was counted.
    /// The vault does not store the gate's name, so the reason is in the `execute` result of the keeper.
    Rejected,
    /// The proposal expired before anyone executed it. No strike.
    Expired,
    /// Still pending.
    Waiting,
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
        plan.validate()?;
        self.check_open(now_sec()).await?;
        let id: U64 = Contract(self.vault.clone())
            .call_function(
                "propose",
                json!({
                    "asset_in": plan.asset_in,
                    "asset_out": plan.asset_out,
                    "amount_in": plan.amount_in,
                    "min_amount_out": plan.min_amount_out,
                    "reason_hash": plan.reason_hash(),
                }),
            )
            .transaction()
            .gas(NearGas::from_tgas(30))
            .with_signer(self.account.clone(), self.signer.clone())
            .send_to(&self.network)
            .await
            .map_err(|e| e.to_string())?
            .into_result()
            .map_err(|e| format!("propose failed: {e:?}"))?
            .json()
            .map_err(|e| e.to_string())?;
        Ok(id.0)
    }

    /// The vault's view of one proposal. Call it until the answer is not `Waiting`.
    pub async fn outcome(&self, proposal_id: u64) -> Result<Outcome, AgentError> {
        let now = now_sec();
        if let Some(signed) = self.read::<Option<SignedIntentView>>("get_signed_intent").await?
            && signed.proposal_id.0 == proposal_id
        {
            return Ok(Outcome::Signed { proposal_id, expires_sec: signed.expires_sec });
        }
        match self.read::<Option<PendingProposal>>("get_pending_proposal").await? {
            Some(p) if p.id.0 == proposal_id && now >= p.expires_sec => Ok(Outcome::Expired),
            Some(p) if p.id.0 == proposal_id => Ok(Outcome::Waiting),
            // Gone and not signed: the keeper executed it and a gate closed.
            _ => Ok(Outcome::Rejected),
        }
    }

    /// Asks the vault the questions it would answer `no` to, so the agent can say why first.
    async fn check_open(&self, now: u64) -> Result<(), AgentError> {
        if self.read::<bool>("is_paused").await? {
            return Err("the vault is paused".into());
        }
        if self.read::<bool>("is_halted").await? {
            return Err("the circuit breaker has halted proposals".into());
        }
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
            .map_err(|e| e.to_string().into())
    }
}

/// Polls `outcome` until the proposal is decided or `timeout` passes. Returns the last answer.
pub async fn wait(
    agent: &Agent,
    proposal_id: u64,
    timeout: Duration,
    every: Duration,
) -> Result<Outcome, AgentError> {
    let started = std::time::Instant::now();
    loop {
        let o = agent.outcome(proposal_id).await?;
        if o != Outcome::Waiting || started.elapsed() >= timeout {
            return Ok(o);
        }
        tokio::time::sleep(every).await;
    }
}

fn now_sec() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
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
}
