//! Explicit opt-in publication using public per-proposal quote metadata.

use std::path::PathBuf;

use mandate_agent::relay::{Acknowledgement, ProposalBinding, Relay, RelayObservation};
use mandate_agent::transport::{PublicReader, read_public_json, validate_public_path};
use near_sdk::json_types::U64;
use near_sdk::serde_json::{Value, json};
use serde::Deserialize;

use crate::KeeperError;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationConfig {
    pub relay_url: String,
    #[serde(default)]
    pub expected_chain_id: Option<String>,
    /// The agent's public outbox containing proposal-<id>.json associations.
    pub metadata_dir: PathBuf,
}

pub struct Publisher {
    relay: Relay,
    reader: PublicReader,
    metadata_dir: PathBuf,
}

impl Publisher {
    pub fn new(
        config: PublicationConfig,
        rpc_url: &str,
        vault: &str,
        auth: Option<&str>,
    ) -> Result<Self, KeeperError> {
        validate_public_path(&config.metadata_dir)?;
        let resolved = config
            .metadata_dir
            .canonicalize()
            .map_err(|_| "public proposal directory is unavailable")?;
        validate_public_path(&resolved)?;
        Ok(Self {
            relay: Relay::new(&config.relay_url, auth)?,
            reader: PublicReader::new(rpc_url, vault)?
                .with_chain(config.expected_chain_id.as_deref())?,
            metadata_dir: resolved,
        })
    }

    /// Returns relay acknowledgement only. No statement about final settlement is made.
    pub async fn publish(&self, signed: &Value) -> Result<Acknowledgement, KeeperError> {
        let id: U64 = near_sdk::serde_json::from_value(signed["proposal_id"].clone())
            .map_err(|_| "signed proposal identifier is invalid")?;
        let binding: ProposalBinding =
            read_public_json(&self.metadata_dir.join(format!("proposal-{}.json", id.0)))?;
        let (now, block) = self.reader.final_time().await?;
        let current: Value =
            self.reader.view(&self.reader.vault, "get_signed_intent", json!({}), &block).await?;
        if &current != signed {
            return Err("signed intent is no longer the vault's current recorded result".into());
        }
        let (_, intents, _): (String, String, String) =
            self.reader.view(&self.reader.vault, "get_externals", json!({}), &block).await?;
        if binding.intents_id != intents {
            return Err("quote metadata verifier does not match the current vault".into());
        }
        self.relay.publish(&binding, signed, &self.reader.vault, now).await
    }

    pub async fn status(
        &self,
        acknowledgement: &Acknowledgement,
    ) -> Result<RelayObservation, KeeperError> {
        self.relay.status(&acknowledgement.intent_hash).await
    }
}
