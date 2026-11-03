//! The keeper, against the real vault and `intents.near` on a local sandbox.
//! One tick runs a pending proposal, then reports the signed intent exactly once.

mod common;

use common::{fund_vault, now_sec, propose, register_key, set_prices, world};
use mandate_keeper::{Keeper, Tick};
use near_sdk::serde_json::json;

fn keeper(w: &common::World) -> Keeper {
    Keeper {
        network: w.network.clone(),
        vault: w.vault.account_id().clone(),
        account: w.stranger.clone(),
        signer: w.signer.clone(),
    }
}

#[tokio::test]
async fn keeper_runs_a_proposal_then_reports_the_signed_intent_once() -> testresult::TestResult<()>
{
    let w = world().await?;
    register_key(&w).await?;
    fund_vault(&w, 10_000_000).await?;
    set_prices(&w, now_sec()).await?;
    propose(&w).await?;
    let k = keeper(&w);

    // First tick: the proposal is executed, and the vault says so.
    let Tick::Executed(outcome) = k.tick(None).await.map_err(|e| e.to_string())? else {
        panic!("expected the keeper to execute the pending proposal");
    };
    assert!(outcome.get("Executed").is_some(), "got {outcome}");

    // Second tick: the signed intent is new, so it is handed off once.
    let Tick::Publish(signed) = k.tick(None).await.map_err(|e| e.to_string())? else {
        panic!("expected the signed intent to be reported");
    };
    let payload = signed["payload"].as_str().expect("payload").to_owned();
    assert_eq!(signed["public_key"], json!(w.derived_key));

    // Third tick, with that payload already reported: nothing to do.
    assert_eq!(k.tick(Some(&payload)).await.map_err(|e| e.to_string())?, Tick::Idle);
    Ok(())
}

#[tokio::test]
async fn keeper_does_nothing_without_a_proposal() -> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    assert_eq!(keeper(&w).tick(None).await.map_err(|e| e.to_string())?, Tick::Idle);
    Ok(())
}
