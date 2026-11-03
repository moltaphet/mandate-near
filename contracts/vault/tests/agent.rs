//! The script agent, against the real vault and `intents.near` on a local sandbox.
//! The agent proposes; the keeper executes; the agent reads back what the vault decided.

mod common;

use std::time::Duration;

use common::{MIN_OUT, USDC, WNEAR, World, fund_vault, now_sec, register_key, set_prices, world};
use mandate_agent::{Agent, Outcome, Plan, wait};
use mandate_keeper::{Keeper, Tick};
use near_api::Contract;
use near_sdk::json_types::U128;
use near_sdk::serde_json::{Value, json};

const RATIONALE: &str = "USDC is down 4% on the day; a small buy of wNEAR fits the mandate.";

fn plan() -> Plan {
    Plan {
        asset_in: USDC.into(),
        asset_out: WNEAR.into(),
        amount_in: U128(2_000_000),
        min_amount_out: U128(MIN_OUT.parse().expect("MIN_OUT is a number")),
        rationale: RATIONALE.into(),
    }
}

fn agent(w: &World) -> Agent {
    Agent {
        network: w.network.clone(),
        vault: w.vault.account_id().clone(),
        account: w.agent.clone(),
        signer: w.signer.clone(),
    }
}

fn keeper(w: &World) -> Keeper {
    Keeper {
        network: w.network.clone(),
        vault: w.vault.account_id().clone(),
        account: w.stranger.clone(),
        signer: w.signer.clone(),
    }
}

#[tokio::test]
async fn the_agent_proposes_and_the_keeper_executes_it() -> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    fund_vault(&w, 10_000_000).await?;
    set_prices(&w, now_sec()).await?;
    let a = agent(&w);

    let id = a.propose(&plan()).await.map_err(|e| e.to_string())?;

    // The vault stores the hash of the rationale, not the text.
    let pending: Value = Contract(w.vault.account_id().clone())
        .call_function("get_pending_proposal", json!({}))
        .read_only::<Value>()
        .fetch_from(&w.network)
        .await?
        .data;
    assert_eq!(pending["reason_hash"], json!(plan().reason_hash()));
    assert_eq!(a.outcome(id).await.map_err(|e| e.to_string())?, Outcome::Waiting);

    let Tick::Executed(outcome) = keeper(&w).tick(None).await.map_err(|e| e.to_string())? else {
        panic!("expected the keeper to execute the agent's proposal");
    };
    assert!(outcome.get("Executed").is_some(), "got {outcome}");

    let Outcome::Signed { proposal_id, .. } =
        wait(&a, id, Duration::from_secs(30), Duration::from_secs(1))
            .await
            .map_err(|e| e.to_string())?
    else {
        panic!("expected the agent to see its proposal signed");
    };
    assert_eq!(proposal_id, id);
    Ok(())
}

#[tokio::test]
async fn the_agent_refuses_a_second_proposal_before_spending_gas() -> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    let a = agent(&w);
    a.propose(&plan()).await.map_err(|e| e.to_string())?;

    let err = a.propose(&plan()).await.expect_err("a second proposal must be refused").to_string();
    assert!(err.contains("pending"), "the refusal should come from the agent's own check: {err}");
    Ok(())
}

#[tokio::test]
async fn a_gate_that_closes_is_reported_as_rejected() -> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    // No funds: the balance gate closes when the keeper executes.
    set_prices(&w, now_sec()).await?;
    let a = agent(&w);
    let id = a.propose(&plan()).await.map_err(|e| e.to_string())?;

    let Tick::Executed(outcome) = keeper(&w).tick(None).await.map_err(|e| e.to_string())? else {
        panic!("expected the keeper to execute the agent's proposal");
    };
    assert!(outcome.get("Rejected").is_some(), "got {outcome}");
    assert_eq!(a.outcome(id).await.map_err(|e| e.to_string())?, Outcome::Rejected);
    Ok(())
}
