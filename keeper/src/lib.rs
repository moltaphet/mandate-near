//! Permissionless keeper for the Mandate vault.
//!
//! Each tick does at most one thing:
//!
//! - If a proposal is pending and has not expired, it calls `execute`. The
//!   vault checks every gate itself, so the keeper cannot change the outcome.
//! - If a signed intent exists that has not been reported yet, it reports it
//!   once, so the caller can publish it to the solver relay.
//!
//! The keeper's account only pays for the transaction. It holds no authority
//! over funds, and a compromised keeper can at most delay trades.

use std::sync::Arc;
use std::time::Duration;

use near_api::{AccountId, Contract, NearGas, NetworkConfig, Signer};
use near_sdk::serde_json::{Value, json};

pub mod publication;

pub type KeeperError = Box<dyn std::error::Error + Send + Sync>;

pub struct Keeper {
    pub network: NetworkConfig,
    pub vault: AccountId,
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
}

impl Keeper {
    /// Runs one step. `last_reported` is the payload of the intent already handed off,
    /// so the same intent is not reported twice.
    pub async fn tick(&self, last_reported: Option<&str>) -> Result<Tick, KeeperError> {
        let now = self
            .view("get_chain_time_sec")
            .await?
            .as_u64()
            .ok_or("vault returned an invalid chain time")?;

        // An already issued signature remains publishable even if the owner has paused
        // new execution. Check it before readiness and pending-proposal state.
        let signed = self.view("get_signed_intent").await?;
        let fresh = signed["expires_sec"].as_u64().is_some_and(|e| e > now);
        let already = signed["payload"].as_str() == last_reported;
        if !signed.is_null() && fresh && !already {
            return Ok(Tick::Publish(signed));
        }

        let pending = self.view("get_pending_proposal").await?;
        if !pending.is_null() {
            if self.view("is_in_flight").await?.as_bool().ok_or("invalid execution status")? {
                return Ok(Tick::NotReady { reason: "an execution is in progress" });
            }
            let expires = pending["expires_sec"].as_u64().ok_or("proposal has no expiry")?;
            if now >= expires {
                return Ok(Tick::Expired);
            }
            if self.view("is_paused").await?.as_bool().ok_or("invalid pause status")? {
                return Ok(Tick::NotReady { reason: "the vault is paused" });
            }
            if self.view("is_halted").await?.as_bool().ok_or("invalid halt status")? {
                return Ok(Tick::NotReady { reason: "the circuit breaker has halted proposals" });
            }
            if !self.view("is_key_registered").await?.as_bool().ok_or("invalid key status")? {
                return Ok(Tick::NotReady { reason: "the intents key is not registered" });
            }
            return execution_tick(self.execute().await?);
        }
        Ok(Tick::Idle)
    }

    async fn view(&self, method: &str) -> Result<Value, KeeperError> {
        let data = Contract(self.vault.clone())
            .call_function(method, json!({}))
            .read_only::<Value>()
            .fetch_from(&self.network)
            .await
            .map_err(|_| "keeper view transport failed")?
            .data;
        Ok(data)
    }

    async fn execute(&self) -> Result<Value, KeeperError> {
        let outcome = Contract(self.vault.clone())
            .call_function("execute", json!({}))
            .transaction()
            .gas(NearGas::from_tgas(300))
            .with_signer(self.account.clone(), self.signer.clone())
            .send_to(&self.network)
            .await
            .map_err(|_| "execution transaction transport or signing failed")?
            .into_result()
            .map_err(|_| "vault refused the execution transaction")?
            .json::<Value>()
            .map_err(|_| "vault returned an invalid execution outcome")?;
        Ok(outcome)
    }
}

fn execution_tick(outcome: Value) -> Result<Tick, KeeperError> {
    let object = outcome.as_object().ok_or("execute returned an invalid outcome")?;
    if object.len() != 1 {
        return Err("execute returned an ambiguous outcome".into());
    }
    if object.contains_key("Executed") {
        Ok(Tick::Executed(outcome))
    } else if object.contains_key("Rejected") {
        Ok(Tick::Rejected(outcome))
    } else if object.contains_key("Failed") {
        Ok(Tick::Failed(outcome))
    } else {
        Err("execute returned an unknown outcome".into())
    }
}

/// Retry policy shared by the binary and its tests. Protocol-level failures and
/// transaction failures both back off; a successful or idle tick resets the delay.
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
    fn protocol_and_transport_failures_share_bounded_backoff() {
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
}
