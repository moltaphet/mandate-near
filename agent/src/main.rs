//! Runs the script agent once: reads a plan, proposes it, and waits for the vault's answer.
//!
//! Usage: `mandate-agent plan.json`
//!
//! Environment:
//! - `NEAR_RPC_URL`: RPC endpoint (mainnet, testnet or sandbox).
//! - `VAULT_ID`: the vault contract account.
//! - `AGENT_ACCOUNT` and `AGENT_SECRET_KEY`: the account the vault names as its agent.
//! - `AGENT_OUTBOX`: directory for the rationale text and the outcome report.

use std::path::PathBuf;
use std::time::Duration;

use mandate_agent::{Agent, AgentError, Plan, wait};
use near_api::{NetworkConfig, Signer};
use near_sdk::serde_json::{self, json};

fn env(name: &str) -> Result<String, AgentError> {
    std::env::var(name).map_err(|_| format!("{name} is not set").into())
}

#[tokio::main]
async fn main() -> Result<(), AgentError> {
    let path = std::env::args().nth(1).ok_or("usage: mandate-agent <plan.json>")?;
    let plan: Plan = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    plan.validate()?;

    let outbox = PathBuf::from(env("AGENT_OUTBOX")?);
    std::fs::create_dir_all(&outbox)?;
    // The rationale stays off chain. Keeping it next to the proposal lets anyone check it against the stored hash.
    std::fs::write(outbox.join(format!("rationale-{}.txt", plan.reason_hash())), &plan.rationale)?;

    let agent = Agent {
        network: NetworkConfig::from_rpc_url("agent", env("NEAR_RPC_URL")?.parse()?),
        vault: env("VAULT_ID")?.parse()?,
        account: env("AGENT_ACCOUNT")?.parse()?,
        signer: Signer::from_secret_key(env("AGENT_SECRET_KEY")?.parse()?)?,
    };

    let id = agent.propose(&plan).await?;
    println!("proposal {id} submitted; reason hash {}", plan.reason_hash());

    // A keeper has to execute it. The vault expires an unexecuted proposal after its TTL, and that comes back as Expired.
    let outcome = wait(&agent, id, Duration::from_secs(900), Duration::from_secs(5)).await?;
    println!("proposal {id}: {outcome:?}");

    let report = json!({ "proposal_id": id, "reason_hash": plan.reason_hash(), "outcome": format!("{outcome:?}") });
    std::fs::write(outbox.join(format!("outcome-{id}.json")), serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
