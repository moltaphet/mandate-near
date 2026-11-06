//! Default: permissionless execution plus public file handoff; no relay publication.
//! Explicit publication: mandate-keeper --publish <public-relay-config.json>.
//! Acknowledgement and relay-reported SETTLED remain distinct from verified final receipts.

use std::error::Error;
use std::path::{Path, PathBuf};

use mandate_agent::transport::{JsonRpc, create_public_dir, read_public_json, write_public};
use mandate_keeper::publication::{PublicationConfig, Publisher};
use mandate_keeper::{Keeper, RetryBackoff, Tick};
use near_api::{NetworkConfig, Signer};
use near_sdk::serde_json;

type MainError = Box<dyn Error + Send + Sync>;
fn setting(name: &str) -> Result<String, MainError> {
    std::env::var(name).map_err(|_| format!("required setting {name} is absent").into())
}

#[tokio::main]
async fn main() -> Result<(), MainError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !(args.is_empty() || (args.len() == 2 && args[0] == "--publish")) {
        return Err("usage: mandate-keeper [--publish <public-relay-config.json>]".into());
    }
    let rpc = setting("NEAR_RPC_URL")?;
    JsonRpc::new(&rpc, None, 1024 * 1024)?;
    let vault = setting("VAULT_ID")?;
    let publisher = if args.is_empty() {
        None
    } else {
        let config: PublicationConfig = read_public_json(Path::new(&args[1]))?;
        let auth = std::env::var("RELAY_AUTH").ok();
        Some(Publisher::new(config, &rpc, &vault, auth.as_deref())?)
    };
    let account =
        setting("KEEPER_ACCOUNT")?.parse().map_err(|_| "invalid keeper account configuration")?;
    let parsed = setting("KEEPER_SECRET_KEY")?
        .parse()
        .map_err(|_| "invalid keeper signing configuration")?;
    let signer =
        Signer::from_secret_key(parsed).map_err(|_| "invalid keeper signing configuration")?;
    let keeper = Keeper {
        network: NetworkConfig::from_rpc_url(
            "keeper",
            rpc.parse().map_err(|_| "invalid RPC configuration")?,
        ),
        vault: vault.parse().map_err(|_| "invalid vault configuration")?,
        account,
        signer,
    };
    let outbox = create_public_dir(&PathBuf::from(setting("KEEPER_OUTBOX")?))?;
    let mut last_reported: Option<String> = None;
    let mut retry = RetryBackoff::default();
    loop {
        let result = keeper.tick(last_reported.as_deref()).await;
        let mut pause = match &result {
            // Publication has its own success/failure result. Do not reset its
            // accumulated failures before the relay request completes.
            Ok(Tick::Publish(_)) => std::time::Duration::from_secs(5),
            Ok(tick) => retry.after_tick(tick),
            Err(_) => retry.after_error(),
        };
        match result {
            Ok(Tick::Executed(_)) => {
                println!("vault recorded a signed intent; settlement unverified")
            }
            Ok(Tick::Rejected(_)) => println!("vault rejected the proposal"),
            Ok(Tick::Failed(_)) => {
                eprintln!("execution step failed; retry in {}s", pause.as_secs())
            }
            Ok(Tick::NotReady { reason }) => println!("waiting: {reason}"),
            Ok(Tick::Expired) => println!("proposal expired; waiting for a new proposal"),
            Ok(Tick::Publish(signed)) => {
                let id = signed["proposal_id"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .ok_or("invalid signed proposal identifier")?;
                write_public(
                    &outbox.join(format!("intent-{id}.json")),
                    &serde_json::to_vec_pretty(&signed)?,
                )?;
                if let Some(publisher) = &publisher {
                    match publisher.publish(&signed).await {
                        Ok(ack) => {
                            pause = retry.after_tick(&Tick::Idle);
                            write_public(
                                &outbox.join(format!("relay-{id}.json")),
                                &serde_json::to_vec_pretty(&ack)?,
                            )?;
                            println!("relay acknowledged proposal {id}; settlement unverified");
                            match publisher.status(&ack).await {
                                Ok(observation) => {
                                    write_public(
                                        &outbox.join(format!("relay-observation-{id}.json")),
                                        &serde_json::to_vec_pretty(&observation)?,
                                    )?;
                                    println!(
                                        "relay reports {}; final receipt not independently verified",
                                        observation.relay_status
                                    );
                                }
                                Err(_) => {
                                    eprintln!("relay status unavailable; acknowledgement retained")
                                }
                            }
                            last_reported = signed["payload"].as_str().map(str::to_owned);
                        }
                        Err(_) => {
                            pause = retry.after_error();
                            eprintln!(
                                "relay publication failed or unknown; retry in {}s",
                                pause.as_secs()
                            );
                        }
                    }
                } else {
                    pause = retry.after_tick(&Tick::Idle);
                    println!(
                        "proposal {id} signed; public handoff written; relay publication disabled"
                    );
                    last_reported = signed["payload"].as_str().map(str::to_owned);
                }
            }
            Ok(Tick::Idle) => {}
            Err(_) => eprintln!("keeper tick failed; retry in {}s", pause.as_secs()),
        }
        tokio::time::sleep(pause).await;
    }
}
