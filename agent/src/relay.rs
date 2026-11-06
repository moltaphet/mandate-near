//! Documented NEAR Intents Message Bus protocol.
//!
//! Source: https://docs.near-intents.org/integration/market-makers/message-bus/rpc
//! An acknowledgement or a relay-reported SETTLED status is not independent final receipt proof.

use near_sdk::json_types::{U64, U128};
use near_sdk::serde_json::{Value, json};
use serde::{Deserialize, Serialize};

use crate::transport::JsonRpc;
use crate::{AgentError, Plan};

pub const DEFAULT_RELAY_URL: &str = "https://solver-relay-v2.chaindefuser.com/rpc";
pub const PROTOCOL_SOURCE: &str =
    "https://docs.near-intents.org/integration/market-makers/message-bus/rpc";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub quote_hash: String,
    pub defuse_asset_identifier_in: String,
    pub defuse_asset_identifier_out: String,
    pub amount_in: U128,
    pub amount_out: U128,
    pub expiration_time: String,
}

impl Quote {
    pub fn expires_sec(&self) -> Result<u64, AgentError> {
        let parsed = chrono::DateTime::parse_from_rfc3339(&self.expiration_time)
            .map_err(|_| "quote expiration is not RFC3339")?;
        u64::try_from(parsed.timestamp()).map_err(|_| "quote expiration precedes Unix epoch".into())
    }

    pub fn validate(
        &self,
        input: &str,
        output: &str,
        amount: u128,
        now: u64,
        validity: u64,
    ) -> Result<(), AgentError> {
        if self.defuse_asset_identifier_in != input
            || self.defuse_asset_identifier_out != output
            || self.amount_in.0 != amount
            || amount == 0
            || self.amount_out.0 == 0
            || amount > i128::MAX as u128
            || self.amount_out.0 > i128::MAX as u128
        {
            return Err("quote assets or exact amounts do not match the requested trade".into());
        }
        if self.quote_hash.is_empty()
            || self.quote_hash.len() > 128
            || !self.quote_hash.is_ascii()
            || self.quote_hash.bytes().any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
        {
            return Err("invalid quote identifier".into());
        }
        if self.expires_sec()? <= now.checked_add(validity).ok_or("quote validity overflow")? {
            return Err("quote expired or has insufficient validity".into());
        }
        Ok(())
    }
}

/// Public per-proposal evidence. A different proposal or different signed amounts cannot reuse it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposalBinding {
    pub proposal_id: U64,
    pub vault_id: String,
    pub intents_id: String,
    pub block_hash: String,
    pub plan: Plan,
    pub quote: Quote,
}

impl ProposalBinding {
    pub fn publish_params(
        &self,
        signed: &Value,
        vault: &str,
        now: u64,
    ) -> Result<Value, AgentError> {
        if self.vault_id != vault || signed["proposal_id"] != json!(self.proposal_id) {
            return Err(
                "signed intent and quote metadata refer to different proposals or vaults".into()
            );
        }
        crate::transport::validate_block_hash(&self.block_hash)?;
        self.plan.validate()?;
        self.quote.validate(
            &self.plan.asset_in,
            &self.plan.asset_out,
            self.plan.amount_in.0,
            now,
            0,
        )?;
        if self.plan.min_amount_out != self.quote.amount_out {
            return Err("plan output is not the exact selected quote output".into());
        }
        let payload = signed["payload"]
            .as_str()
            .filter(|s| s.len() <= mandate_vault::MAX_MPC_PAYLOAD)
            .ok_or("signed payload is missing or too large")?;
        let content: Value =
            near_sdk::serde_json::from_str(payload).map_err(|_| "signed payload is malformed")?;
        let expires = signed["expires_sec"].as_u64().ok_or("signed expiry is invalid")?;
        let deadline = content["deadline"].as_str().ok_or("payload deadline is missing")?;
        let timestamp = chrono::DateTime::parse_from_rfc3339(deadline)
            .map_err(|_| "payload deadline is invalid")?;
        if timestamp.timestamp_subsec_nanos() != 0
            || u64::try_from(timestamp.timestamp()).ok() != Some(expires)
            || expires <= now
            || expires >= self.quote.expires_sec()?
        {
            return Err("signed intent expires outside the selected quote validity".into());
        }
        let intents = content["intents"].as_array().ok_or("payload intents are missing")?;
        let expected = json!({self.plan.asset_in.clone(): format!("-{}", self.plan.amount_in.0),
            self.plan.asset_out.clone(): self.plan.min_amount_out.0.to_string()});
        if content["signer_id"] != self.vault_id
            || content["verifying_contract"] != self.intents_id
            || intents.len() != 1
            || intents[0]["intent"] != "token_diff"
            || intents[0]["diff"] != expected
        {
            return Err("signed payload does not match selected quote, vault and verifier".into());
        }
        let public = signed["public_key"]
            .as_str()
            .filter(|s| s.len() <= 128)
            .ok_or("signed public identity is missing")?;
        if !public.starts_with("ed25519:")
            || bs58::decode(&public[8..]).into_vec().map_or(true, |bytes| bytes.len() != 32)
        {
            return Err("signed public identity is not a 32-byte Ed25519 identity".into());
        }
        let signature = signed["signature"]
            .as_str()
            .filter(|s| s.starts_with("ed25519:") && s.len() <= 128 && s.is_ascii())
            .ok_or("signed signature format is invalid")?;
        if bs58::decode(&signature[8..]).into_vec().map_or(true, |bytes| bytes.len() != 64) {
            return Err("signed signature is not 64-byte Ed25519".into());
        }
        Ok(json!([{"quote_hashes":[self.quote.quote_hash], "signed_data":{
            "standard":"raw_ed25519", "payload":payload, "public_key":public, "signature":signature}}]))
    }
}

#[derive(Clone)]
pub struct Relay {
    rpc: JsonRpc,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Acknowledgement {
    pub intent_hash: String,
    /// Always acknowledgement, never settlement.
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RelayObservation {
    pub intent_hash: String,
    /// Relay-reported PENDING/TX_BROADCASTED/SETTLED/NOT_FOUND_OR_NOT_VALID.
    pub relay_status: String,
    pub transaction_hash: Option<String>,
    pub settlement_verified: bool,
}

impl Relay {
    pub fn new(endpoint: &str, auth: Option<&str>) -> Result<Self, AgentError> {
        Ok(Self { rpc: JsonRpc::new(endpoint, auth, 64 * 1024)? })
    }

    pub async fn quote(
        &self,
        input: &str,
        output: &str,
        amount: u128,
        now: u64,
        validity: u64,
    ) -> Result<Quote, AgentError> {
        let result = self
            .rpc
            .call(
                "quote",
                json!([{
                    "defuse_asset_identifier_in":input, "defuse_asset_identifier_out":output,
                    "exact_amount_in":amount.to_string(),
                    "min_deadline_ms":validity.checked_mul(1000).ok_or("quote validity overflow")?
                }]),
            )
            .await?;
        let offers: Vec<Quote> = near_sdk::serde_json::from_value(result)
            .map_err(|_| "relay quote result has an invalid schema")?;
        if offers.len() > 64 {
            return Err("relay returned too many quote offers".into());
        }
        offers
            .into_iter()
            .filter(|offer| offer.validate(input, output, amount, now, validity).is_ok())
            .max_by_key(|offer| offer.amount_out.0)
            .ok_or_else(|| "relay returned no valid matching quote".into())
    }

    pub async fn publish(
        &self,
        binding: &ProposalBinding,
        signed: &Value,
        vault: &str,
        now: u64,
    ) -> Result<Acknowledgement, AgentError> {
        let params = binding.publish_params(signed, vault, now)?;
        let result = self.rpc.call("publish_intent", params).await?;
        if result["status"] != "OK" {
            return Err("relay refused intent publication".into());
        }
        let hash = bounded_id(&result["intent_hash"])?;
        Ok(Acknowledgement { intent_hash: hash, status: "acknowledged_not_settled".into() })
    }

    pub async fn status(&self, intent_hash: &str) -> Result<RelayObservation, AgentError> {
        bounded_id(&json!(intent_hash))?;
        let result = self.rpc.call("get_status", json!([{"intent_hash":intent_hash}])).await?;
        if result["intent_hash"] != intent_hash {
            return Err("relay status identity mismatch".into());
        }
        let status = result["status"]
            .as_str()
            .filter(|status| {
                ["PENDING", "TX_BROADCASTED", "SETTLED", "NOT_FOUND_OR_NOT_VALID"].contains(status)
            })
            .ok_or("unknown relay status")?
            .to_owned();
        let transaction_hash = if result["data"]["hash"].is_null() {
            None
        } else {
            Some(bounded_id(&result["data"]["hash"])?)
        };
        Ok(RelayObservation {
            intent_hash: intent_hash.to_owned(),
            relay_status: status,
            transaction_hash,
            settlement_verified: false,
        })
    }
}

fn bounded_id(value: &Value) -> Result<String, AgentError> {
    value
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 128
                && s.is_ascii()
                && !s.bytes().any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
        })
        .map(str::to_owned)
        .ok_or_else(|| "invalid relay result identifier".into())
}
