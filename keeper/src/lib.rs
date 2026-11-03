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
use std::time::{SystemTime, UNIX_EPOCH};

use near_api::{AccountId, Contract, NearGas, NetworkConfig, Signer};
use near_sdk::serde_json::{Value, json};

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
    /// `execute` ran. The value is the vault's outcome: Executed, Rejected or Failed.
    Executed(Value),
    /// A proposal is pending but has expired. The agent must propose again.
    Expired,
    /// A signed intent that has not been reported yet. Publish it to the solver relay.
    Publish(Value),
}

impl Keeper {
    /// Runs one step. `last_reported` is the payload of the intent already handed off,
    /// so the same intent is not reported twice.
    pub async fn tick(&self, last_reported: Option<&str>) -> Result<Tick, KeeperError> {
        let now = now_sec();

        let pending = self.view("get_pending_proposal").await?;
        if !pending.is_null() {
            let expires = pending["expires_sec"].as_u64().ok_or("proposal has no expiry")?;
            if now >= expires {
                return Ok(Tick::Expired);
            }
            return Ok(Tick::Executed(self.execute().await?));
        }

        let signed = self.view("get_signed_intent").await?;
        let fresh = signed["expires_sec"].as_u64().is_some_and(|e| e > now);
        let already = signed["payload"].as_str() == last_reported;
        if !signed.is_null() && fresh && !already {
            return Ok(Tick::Publish(signed));
        }
        Ok(Tick::Idle)
    }

    async fn view(&self, method: &str) -> Result<Value, KeeperError> {
        let data = Contract(self.vault.clone())
            .call_function(method, json!({}))
            .read_only::<Value>()
            .fetch_from(&self.network)
            .await
            .map_err(|e| e.to_string())?
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
            .map_err(|e| e.to_string())?
            .into_result()
            .map_err(|e| format!("execute failed: {e:?}"))?
            .json::<Value>()
            .map_err(|e| e.to_string())?;
        Ok(outcome)
    }
}

fn now_sec() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
