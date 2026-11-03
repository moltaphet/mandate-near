//! Runs the keeper loop.
//!
//! Environment:
//! - `NEAR_RPC_URL`: RPC endpoint (mainnet, testnet or sandbox).
//! - `VAULT_ID`: the vault contract account.
//! - `KEEPER_ACCOUNT` and `KEEPER_SECRET_KEY`: the account that pays for `execute`.
//! - `KEEPER_OUTBOX`: directory where signed intents are written for publication.
//!   Solver relay publication needs a partner key, so the hand-off is a file for now.

use std::error::Error;
use std::path::PathBuf;
use std::time::Duration;

use mandate_keeper::{Keeper, Tick};
use near_api::{NetworkConfig, Signer};

fn env(name: &str) -> Result<String, Box<dyn Error>> {
    std::env::var(name).map_err(|_| format!("{name} is not set").into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let keeper = Keeper {
        network: NetworkConfig::from_rpc_url("keeper", env("NEAR_RPC_URL")?.parse()?),
        vault: env("VAULT_ID")?.parse()?,
        account: env("KEEPER_ACCOUNT")?.parse()?,
        signer: Signer::from_secret_key(env("KEEPER_SECRET_KEY")?.parse()?)?,
    };
    let outbox = PathBuf::from(env("KEEPER_OUTBOX")?);
    std::fs::create_dir_all(&outbox)?;

    let mut last_reported: Option<String> = None;
    loop {
        match keeper.tick(last_reported.as_deref()).await {
            Ok(Tick::Executed(outcome)) => println!("executed: {outcome}"),
            Ok(Tick::Expired) => println!("pending proposal expired; waiting for a new one"),
            Ok(Tick::Publish(signed)) => {
                let id = signed["proposal_id"].as_str().unwrap_or("unknown");
                let path = outbox.join(format!("intent-{id}.json"));
                std::fs::write(&path, near_sdk::serde_json::to_vec_pretty(&signed)?)?;
                println!("signed intent for proposal {id} written to {}", path.display());
                last_reported = signed["payload"].as_str().map(str::to_owned);
            }
            Ok(Tick::Idle) => {}
            Err(err) => eprintln!("keeper tick failed: {err}"),
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
