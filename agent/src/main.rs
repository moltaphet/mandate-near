//! Manual mode: mandate-agent <public-plan.json>.
//! Autonomous mode: mandate-agent --auto <public-config.json> [--dry-run | --loop].
//! Model mode: mandate-agent --model [--dry-run]; provider settings come only from environment.
//! Autonomous dry-run reads public RPC/quotes only and loads no signing/auth material.
//! Model dry-run calls its configured provider but sends no NEAR transaction or outbox write.
//! Model verification: mandate-agent --verify-model <record.json> ... runs offline checks only.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mandate_agent::autonomous::{AutonomousConfig, Preparation};
use mandate_agent::decision::{
    Decision, DecisionSource, MANUAL_SOURCE_ID, ManualDecision, PreparationAttempt,
    PreparationLoop, REBALANCE_SOURCE_ID, RebalanceDecision,
};
use mandate_agent::model::{HttpChatClient, ModelDecision};
use mandate_agent::provenance::{ModelRecord, parse_signature_proof, verify_signature};
use mandate_agent::relay::Relay;
use mandate_agent::transport::{
    PublicReader, create_public_dir, read_public_bytes, read_public_json, write_public,
    write_public_bounded,
};
use mandate_agent::{Agent, AgentError, Plan, wait};
use near_api::{NetworkConfig, Signer};
use near_sdk::serde_json::{self, json};

fn setting(name: &str) -> Result<String, AgentError> {
    std::env::var(name).map_err(|_| format!("required setting {name} is absent").into())
}

fn expected_chain() -> Result<Option<String>, AgentError> {
    let optional = |name| match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => Err("invalid expected chain configuration"),
    };
    let shared = optional("NEAR_CHAIN_ID")?;
    let explicit = optional("EXPECTED_CHAIN_ID")?;
    if shared.as_ref().zip(explicit.as_ref()).is_some_and(|(a, b)| a != b) {
        return Err("agent expected chain environment settings differ".into());
    }
    Ok(shared.or(explicit))
}

async fn signed_agent(rpc_url: &str, vault_id: &str) -> Result<Agent, AgentError> {
    PublicReader::new(rpc_url, vault_id)?
        .with_chain(expected_chain()?.as_deref())?
        .final_time()
        .await?;
    authenticated_agent(rpc_url, vault_id)
}

fn authenticated_agent(rpc_url: &str, vault_id: &str) -> Result<Agent, AgentError> {
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

async fn submit(
    agent: &Agent,
    plan: &Plan,
    outbox: &Path,
    model_id: &str,
) -> Result<u64, AgentError> {
    write_public(
        &outbox.join(format!("rationale-{}.txt", plan.reason_hash())),
        plan.rationale.as_bytes(),
    )?;
    let id = agent.propose_from_source(plan, model_id).await?;
    println!("proposal {id} submitted; reason hash {}", plan.reason_hash());
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

/// Offline verification authenticates no chain or settlement. Stored signatures
/// are always checked against an externally supplied public pin; flags cannot
/// downgrade them. Public file path and byte limits apply before any file open.
fn verify_model(args: &[String]) -> Result<(), AgentError> {
    const USAGE: &str = "usage: mandate-agent --verify-model <record.json> [--reason-hash H] [--plan <public-plan.json>] [--signature <signature.json>] [--signing-address 0x…]";
    let record_path = args.first().filter(|arg| !arg.starts_with('-')).ok_or(USAGE)?;
    let mut reason_hash = None;
    let mut plan_path = None;
    let mut signature_path = None;
    let mut signing_address = None;
    let mut flags = std::collections::BTreeSet::new();
    let mut rest = args[1..].iter();
    while let Some(flag) = rest.next() {
        if !flags.insert(flag.as_str()) {
            return Err(USAGE.into());
        }
        let value = rest.next().ok_or(USAGE)?;
        match flag.as_str() {
            "--reason-hash" => reason_hash = Some(value.clone()),
            "--plan" => plan_path = Some(value.clone()),
            "--signature" => signature_path = Some(value.clone()),
            "--signing-address" => signing_address = Some(value.clone()),
            _ => return Err(USAGE.into()),
        }
    }
    let record_bytes = read_public_bytes(Path::new(record_path), 1024 * 1024)?;
    let record: ModelRecord =
        serde_json::from_slice(&record_bytes).map_err(|_| "model record is malformed")?;
    record.check_hashes().map_err(|error| error.to_string())?;
    if reason_hash.as_ref().is_some_and(|expected| expected != &record.reason_hash) {
        return Err(
            "the supplied reason hash does not match the canonical response commitment".into()
        );
    }
    if let Some(path) = plan_path {
        let plan: Plan = read_public_json(Path::new(&path))?;
        record.check_plan(&plan).map_err(|error| error.to_string())?;
    }
    let supplied_signature = signature_path
        .as_ref()
        .map(|path| -> Result<_, AgentError> {
            let bytes = read_public_bytes(Path::new(path), 64 * 1024)?;
            let proof = std::str::from_utf8(&bytes).map_err(|_| "signature is malformed")?;
            parse_signature_proof(proof).map_err(|error| error.to_string().into())
        })
        .transpose()?;
    let stored_signature = record
        .signature_proof
        .as_ref()
        .map(|proof| parse_signature_proof(proof).map_err(|error| error.to_string()))
        .transpose()?;
    let has_signature = stored_signature.is_some() || supplied_signature.is_some();
    if has_signature != signing_address.is_some() {
        return Err(
            "a signature requires an external signing address; a pin alone proves nothing".into()
        );
    }
    if let Some(address) = &signing_address {
        for signature in stored_signature.iter().chain(supplied_signature.iter()) {
            verify_signature(&record, signature, address).map_err(|error| error.to_string())?;
        }
    }
    let verification = if has_signature { "VERIFIED_PINNED_SIGNER" } else { "UNVERIFIED" };
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "record": "hashes_and_decision_match",
            "canonical_version": record.canonical_version,
            "reason_hash": record.reason_hash,
            "request_sha256": record.request_sha256,
            "response_sha256": record.response_sha256,
            "verification": verification,
            "signature": verification,
            "signing_address": signing_address,
            "model": record.model,
            "response_id": record.response_id,
            "chain_authenticated": false,
            "snapshot_authenticated": false,
            "gate_approval_verified": false,
            "settlement_verified": false,
            "hardware_attestation_verified": false,
        }))?
    );
    Ok(())
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
        let expected_chain = expected_chain()?;
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
        // Optional: the candidate trade sizes in whole dollars, for example "1,2".
        let mut source = ModelDecision::new(client);
        if let Ok(sizes) = std::env::var("MODEL_CANDIDATE_USD") {
            let parsed: Result<Vec<u128>, _> =
                sizes.split(',').map(|s| s.trim().parse::<u128>()).collect();
            source = match parsed.ok().map(|sizes| source.with_candidate_sizes(sizes)) {
                Some(Ok(source)) => source,
                _ => {
                    println!(
                        "no trade: MODEL_CANDIDATE_USD must list whole-dollar sizes, for example 1,2"
                    );
                    return Ok(());
                }
            };
        }
        match source.decide(&snapshot).await {
            Decision::NoTrade { reason } => println!("no trade: {reason}"),
            Decision::Trade { plan, response } => {
                let response = response.ok_or("model decision omitted its response record")?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "status": "model_candidate",
                        "response_id": response.id,
                        "model_id": response.model,
                        "verification": if response.signature_proof.is_some() { "VERIFIED_PINNED_SIGNER" } else { "UNVERIFIED" },
                        "asset_in": plan.asset_in,
                        "asset_out": plan.asset_out,
                        "amount_in": plan.amount_in,
                        "min_amount_out": plan.min_amount_out,
                        "reason_hash": plan.reason_hash(),
                        "request_sha256": response.request_sha256,
                        "response_sha256": response.response_sha256,
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
                let agent = signed_agent(&rpc_url, &vault_id).await?;
                if fresh.agent_id.as_deref() != Some(agent.account.as_str()) {
                    return Err("configured account is not this vault's agent".into());
                }
                let outbox = create_public_dir(&PathBuf::from(setting("AGENT_OUTBOX")?))?;
                // Written before submission, so the exact bytes exist even if the send fails.
                let mut record = ModelRecord::from_response(&response, &plan)
                    .map_err(|error| error.to_string())?;
                record.verification = if response.signature_proof.is_some() {
                    "VERIFIED_PINNED_SIGNER".into()
                } else {
                    "UNVERIFIED".into()
                };
                write_public_bounded(
                    &outbox.join(format!("model-{}.json", response.response_sha256)),
                    &serde_json::to_vec_pretty(&record)?,
                    1024 * 1024,
                )?;
                let id = submit(&agent, &plan, &outbox, &response.model).await?;
                record_outcome(&agent, id, &plan, &outbox).await?;
            }
        }
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "--verify-model") {
        return verify_model(&args[1..]);
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
        let mut config: AutonomousConfig = read_public_json(Path::new(path))?;
        let environment_chain = expected_chain()?;
        if config
            .expected_chain_id
            .as_ref()
            .zip(environment_chain.as_ref())
            .is_some_and(|(a, b)| a != b)
        {
            return Err("agent environment and autonomous expected chains differ".into());
        }
        config.expected_chain_id = config.expected_chain_id.or(environment_chain);
        config.validate()?;
        // Authentication is loaded only for explicitly executing autonomous mode.
        let auth = if dry { None } else { std::env::var("RELAY_AUTH").ok() };
        let relay = Relay::new(&config.relay_url, auth.as_deref())?;
        let source = RebalanceDecision::new(config.clone(), relay);
        if dry {
            let prepared = source.prepare().await?;
            println!("{}", serde_json::to_string_pretty(&prepared)?);
            return Ok(());
        }
        let mut preparation = PreparationLoop::new(repeat);
        loop {
            let prepared = match preparation.next(&source).await? {
                PreparationAttempt::Ready(prepared) => prepared,
                PreparationAttempt::Retry { after } => {
                    // Provider errors can contain untrusted data. Log only the
                    // operation and bounded delay, never their response text.
                    eprintln!("autonomous preparation failed; retry in {}s", after.as_secs());
                    tokio::time::sleep(after).await;
                    continue;
                }
            };
            match prepared {
                Preparation::NoTrade { reason } => println!("no trade: {reason}"),
                Preparation::Trade { prepared } => {
                    // Public preparation is complete. Errors beyond this point
                    // stop the process: a missing send result must be reconciled
                    // before another proposal is submitted.
                    let agent = authenticated_agent(&config.rpc_url, &config.vault_id)?;
                    if agent.account.as_str() != prepared.agent_id {
                        return Err("configured account is not this vault's agent".into());
                    }
                    let outbox = create_public_dir(&PathBuf::from(setting("AGENT_OUTBOX")?))?;
                    let plan = prepared.plan.clone();
                    let id = submit(&agent, &plan, &outbox, REBALANCE_SOURCE_ID).await?;
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
    let agent = signed_agent(&setting("NEAR_RPC_URL")?, &setting("VAULT_ID")?).await?;
    let outbox = create_public_dir(&PathBuf::from(setting("AGENT_OUTBOX")?))?;
    let id = submit(&agent, &plan, &outbox, MANUAL_SOURCE_ID).await?;
    record_outcome(&agent, id, &plan, &outbox).await
}
