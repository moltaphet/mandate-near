//! Manual mode: mandate-agent <public-plan.json>.
//! Autonomous mode: mandate-agent --auto <public-config.json> [--dry-run | --loop].
//! Model mode: mandate-agent --model [--dry-run]; provider settings come only from environment.
//! Autonomous dry-run reads public RPC/quotes only and loads no signing/auth material.
//! Model dry-run calls its configured provider but sends no NEAR transaction or outbox write.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mandate_agent::autonomous::{AutonomousConfig, Preparation};
use mandate_agent::decision::{Decision, DecisionSource, ManualDecision, RebalanceDecision};
use mandate_agent::model::{HttpChatClient, ModelDecision};
use mandate_agent::relay::Relay;
use mandate_agent::transport::{
    JsonRpc, PublicReader, create_public_dir, read_public_json, write_public,
};
use mandate_agent::{Agent, AgentError, Plan, wait};
use near_api::{NetworkConfig, Signer};
use near_sdk::serde_json::{self, json};

fn setting(name: &str) -> Result<String, AgentError> {
    std::env::var(name).map_err(|_| format!("required setting {name} is absent").into())
}

fn signed_agent(rpc_url: &str, vault_id: &str) -> Result<Agent, AgentError> {
    JsonRpc::new(rpc_url, None, 1024 * 1024)?;
    let account =
        setting("AGENT_ACCOUNT")?.parse().map_err(|_| "invalid agent account configuration")?;
    let parsed =
        setting("AGENT_SECRET_KEY")?.parse().map_err(|_| "invalid agent signing configuration")?;
    let signer =
        Signer::from_secret_key(parsed).map_err(|_| "invalid agent signing configuration")?;
    Ok(Agent {
        network: NetworkConfig::from_rpc_url(
            "agent",
            rpc_url.parse().map_err(|_| "invalid RPC configuration")?,
        ),
        vault: vault_id.parse().map_err(|_| "invalid vault configuration")?,
        account,
        signer,
    })
}

async fn submit(agent: &Agent, plan: &Plan, outbox: &Path) -> Result<u64, AgentError> {
    write_public(
        &outbox.join(format!("rationale-{}.txt", plan.reason_hash())),
        plan.rationale.as_bytes(),
    )?;
    let id = agent.propose(plan).await?;
    println!("proposal {id} submitted; rationale hash {}", plan.reason_hash());
    Ok(id)
}

async fn record_outcome(
    agent: &Agent,
    id: u64,
    plan: &Plan,
    outbox: &Path,
) -> Result<(), AgentError> {
    let outcome = wait(agent, id, Duration::from_secs(900), Duration::from_secs(5)).await?;
    println!("proposal {id}: {outcome:?}; settlement remains unverified");
    let report = json!({"proposal_id":id,"reason_hash":plan.reason_hash(),"outcome":outcome,"settlement_verified":false});
    write_public(&outbox.join(format!("outcome-{id}.json")), &serde_json::to_vec_pretty(&report)?)
}

#[tokio::main]
async fn main() -> Result<(), AgentError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--model") {
        if args.len() > 2 || args.get(1).is_some_and(|arg| arg != "--dry-run") {
            return Err("usage: mandate-agent --model [--dry-run]".into());
        }
        let dry = args.get(1).is_some_and(|arg| arg == "--dry-run");
        let rpc_url = setting("NEAR_RPC_URL")?;
        let vault_id = setting("VAULT_ID")?;
        let expected_chain = std::env::var("NEAR_CHAIN_ID").ok();
        let reader =
            PublicReader::new(&rpc_url, &vault_id)?.with_chain(expected_chain.as_deref())?;
        let snapshot = match mandate_agent::autonomous::Snapshot::read(&reader, false).await {
            Ok(snapshot) => snapshot,
            Err(_) => {
                println!("no trade: a validated final-block snapshot could not be read");
                return Ok(());
            }
        };
        // Refuse closed controls before constructing a model client or making an inference call.
        if let Some(reason) = &snapshot.not_ready {
            println!("no trade: {reason}");
            return Ok(());
        }
        let client = match HttpChatClient::from_env() {
            Ok(client) => client,
            Err(error) => {
                println!("no trade: {error}");
                return Ok(());
            }
        };
        match ModelDecision::new(client).decide(&snapshot).await {
            Decision::NoTrade { reason } => println!("no trade: {reason}"),
            Decision::Trade { plan, response } => {
                let response = response.ok_or("model decision omitted its response record")?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "status": "model_candidate",
                        "response_id": response.id,
                        "model_id": response.model,
                        "verification": "UNVERIFIED",
                        "asset_in": plan.asset_in,
                        "asset_out": plan.asset_out,
                        "amount_in": plan.amount_in,
                        "min_amount_out": plan.min_amount_out,
                        "reason_hash": plan.reason_hash(),
                        "block_hash": snapshot.block_hash,
                        "dry_run": dry
                    }))?
                );
                if dry {
                    return Ok(());
                }
                // Fresh public reads rerun the same engine; amounts are never repaired or replaced.
                let fresh = match mandate_agent::autonomous::Snapshot::read(&reader, false).await {
                    Ok(snapshot) => snapshot,
                    Err(_) => {
                        println!("no trade: the final-block snapshot could not be refreshed");
                        return Ok(());
                    }
                };
                if let Some(reason) = &fresh.not_ready {
                    println!("no trade: {reason}");
                    return Ok(());
                }
                let trade = mandate_vault::engine::Trade {
                    asset_in: plan.asset_in.clone(),
                    asset_out: plan.asset_out.clone(),
                    amount_in: plan.amount_in.0,
                    min_amount_out: plan.min_amount_out.0,
                };
                if let Err(error) = mandate_vault::engine::check_trade(
                    &fresh.mandate,
                    &fresh.trading,
                    &trade,
                    &fresh.market,
                ) {
                    println!("no trade: {}", error.code());
                    return Ok(());
                }
                let agent = signed_agent(&rpc_url, &vault_id)?;
                if fresh.agent_id.as_deref() != Some(agent.account.as_str()) {
                    return Err("configured account is not this vault's agent".into());
                }
                let outbox = create_public_dir(&PathBuf::from(setting("AGENT_OUTBOX")?))?;
                let id = submit(&agent, &plan, &outbox).await?;
                record_outcome(&agent, id, &plan, &outbox).await?;
            }
        }
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "--auto") {
        let path = args
            .get(1)
            .ok_or("usage: mandate-agent --auto <public-config.json> [--dry-run | --loop]")?;
        if args[2..].iter().any(|arg| arg != "--dry-run" && arg != "--loop") || args.len() > 3 {
            return Err("invalid autonomous flags".into());
        }
        let dry = args.get(2).is_some_and(|arg| arg == "--dry-run");
        let repeat = args.get(2).is_some_and(|arg| arg == "--loop");
        let config: AutonomousConfig = read_public_json(Path::new(path))?;
        config.validate()?;
        // Authentication is loaded only for explicitly executing autonomous mode.
        let auth = if dry { None } else { std::env::var("RELAY_AUTH").ok() };
        let relay = Relay::new(&config.relay_url, auth.as_deref())?;
        let source = RebalanceDecision::new(config.clone(), relay);
        loop {
            let prepared = source.prepare().await?;
            if dry {
                println!("{}", serde_json::to_string_pretty(&prepared)?);
                return Ok(());
            }
            match prepared {
                Preparation::NoTrade { reason } => println!("no trade: {reason}"),
                Preparation::Trade { prepared } => {
                    let prepared = source.refresh(*prepared).await?;
                    let agent = signed_agent(&config.rpc_url, &config.vault_id)?;
                    if agent.account.as_str() != prepared.agent_id {
                        return Err("configured account is not this vault's agent".into());
                    }
                    let outbox = create_public_dir(&PathBuf::from(setting("AGENT_OUTBOX")?))?;
                    let plan = prepared.plan.clone();
                    let id = submit(&agent, &plan, &outbox).await?;
                    let binding = prepared.binding(&config.vault_id, id);
                    write_public(
                        &outbox.join(format!("proposal-{id}.json")),
                        &serde_json::to_vec_pretty(&binding)?,
                    )?;
                    record_outcome(&agent, id, &plan, &outbox).await?;
                }
            }
            if !repeat {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    }
    if args.len() != 1 || args[0].starts_with('-') {
        return Err("usage: mandate-agent <public-plan.json> OR --auto <public-config.json> [--dry-run | --loop] OR --model [--dry-run]".into());
    }
    let plan: Plan = read_public_json(Path::new(&args[0]))?;
    let plan = ManualDecision::new(plan).validated_plan()?;
    let agent = signed_agent(&setting("NEAR_RPC_URL")?, &setting("VAULT_ID")?)?;
    let outbox = create_public_dir(&PathBuf::from(setting("AGENT_OUTBOX")?))?;
    let id = submit(&agent, &plan, &outbox).await?;
    record_outcome(&agent, id, &plan, &outbox).await
}
