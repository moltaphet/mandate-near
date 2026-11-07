//! The script agent, against the real vault and `intents.near` on a local sandbox.
//! The agent proposes; the keeper executes; the agent reads back what the vault decided.

mod common;
#[path = "../../../agent/tests/common_http/mod.rs"]
mod common_http;

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
        response_commitment: None,
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

    let Tick::Rejected(outcome) = keeper(&w).tick(None).await.map_err(|e| e.to_string())? else {
        panic!("expected the keeper to execute the agent's proposal");
    };
    assert!(outcome.get("Rejected").is_some(), "got {outcome}");
    assert_eq!(
        a.outcome(id).await.map_err(|e| e.to_string())?,
        Outcome::Rejected {
            code: "InsufficientBalance".into(),
            reason: outcome["Rejected"]["reason"].as_str().expect("rejection reason").into()
        }
    );
    Ok(())
}

#[tokio::test]
async fn an_unretained_proposal_outcome_is_unknown() -> testresult::TestResult<()> {
    let w = world().await?;
    assert_eq!(agent(&w).outcome(999).await.map_err(|e| e.to_string())?, Outcome::Unknown);
    Ok(())
}

#[tokio::test]
async fn quote_driven_rebalance_reaches_a_signed_intent_and_mock_relay_acknowledgement()
-> testresult::TestResult<()> {
    use mandate_agent::autonomous::{AutonomousConfig, Preparation, prepare, refresh};
    use mandate_agent::relay::Relay;
    use mandate_agent::transport::write_public;
    use mandate_keeper::publication::{PublicationConfig, Publisher};

    let w = world().await?;
    register_key(&w).await?;
    fund_vault(&w, 10_000_000).await?;
    set_prices(&w, now_sec()).await?;
    let input = USDC.to_owned();
    let output = WNEAR.to_owned();
    let relay_mock=common_http::server(move |request| {
        match request["method"].as_str().unwrap() {
            "quote"=>{
                let params=&request["params"][0];assert_eq!(params["defuse_asset_identifier_in"],input);
                assert_eq!(params["defuse_asset_identifier_out"],output);
                let amount=params["exact_amount_in"].as_str().unwrap().parse::<u128>().unwrap();
                common_http::result(json!([{"quote_hash":"sandbox-rebalance-offer","defuse_asset_identifier_in":input,
                    "defuse_asset_identifier_out":output,"amount_in":amount.to_string(),
                    "amount_out":(amount*10u128.pow(24)/4_500_000).to_string(),
                    "expiration_time":mandate_vault::intent::rfc3339_utc(now_sec()+900)}]))
            }
            "publish_intent"=>{
                assert_eq!(request["params"][0]["quote_hashes"],json!(["sandbox-rebalance-offer"]));
                assert_eq!(request["params"][0]["signed_data"]["standard"],"raw_ed25519");
                common_http::result(json!({"status":"OK","intent_hash":"sandbox-ack-only"}))
            }
            "get_status"=>common_http::result(json!({"intent_hash":"sandbox-ack-only","status":"PENDING"})),
            _=>panic!("unexpected mock relay method"),
        }
    }).await;
    let config = AutonomousConfig {
        rpc_url: w._sandbox.rpc_addr.clone(),
        expected_chain_id: None,
        vault_id: w.vault.account_id().to_string(),
        relay_url: relay_mock.url.clone(),
        asset_a: USDC.into(),
        asset_b: WNEAR.into(),
        target_a_bps: 5000,
        band_bps: 100,
        minimum_quote_validity_sec: 450,
        legacy_price_oracle: false,
    };
    let Preparation::Trade { prepared } =
        prepare(&config, &Relay::new(&relay_mock.url, None).map_err(|e| e.to_string())?)
            .await
            .map_err(|e| e.to_string())?
    else {
        panic!("actual funded holdings should require a rebalance")
    };
    let prepared = refresh(&config, *prepared).await.map_err(|e| e.to_string())?;
    assert_eq!(prepared.plan.amount_in.0, 5_000_000);
    assert_eq!(prepared.plan.min_amount_out, prepared.quote.amount_out);
    let plan = prepared.plan.clone();
    let id = agent(&w).propose(&plan).await.map_err(|e| e.to_string())?;
    let binding = prepared.binding(&config.vault_id, id);
    let Tick::Executed(_) = keeper(&w).tick(None).await.map_err(|e| e.to_string())? else {
        panic!("quote-driven proposal must pass actual vault gates")
    };
    let Tick::Publish(signed) = keeper(&w).tick(None).await.map_err(|e| e.to_string())? else {
        panic!("keeper must hand off the matching signed proposal")
    };
    assert_eq!(signed["proposal_id"], json!(id.to_string()));
    let content: Value = near_sdk::serde_json::from_str(signed["payload"].as_str().unwrap())?;
    assert_eq!(content["intents"][0]["diff"][USDC], json!(format!("-{}", plan.amount_in.0)));
    assert_eq!(content["intents"][0]["diff"][WNEAR], json!(plan.min_amount_out.0.to_string()));
    let dir =
        std::env::temp_dir().join(format!("mandate-rebalance-fixture-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    write_public(
        &dir.join(format!("proposal-{id}.json")),
        &near_sdk::serde_json::to_vec(&binding)?,
    )
    .map_err(|e| e.to_string())?;
    let publisher = Publisher::new(
        PublicationConfig {
            relay_url: relay_mock.url.clone(),
            expected_chain_id: None,
            metadata_dir: dir.clone(),
        },
        &config.rpc_url,
        &config.vault_id,
        None,
    )
    .map_err(|e| e.to_string())?;
    let ack = publisher.publish(&signed).await.map_err(|e| e.to_string())?;
    assert_eq!(ack.status, "acknowledged_not_settled");
    let status = publisher.status(&ack).await.map_err(|e| e.to_string())?;
    assert_eq!(status.relay_status, "PENDING");
    assert!(!status.settlement_verified);
    let requests = relay_mock.requests.lock().unwrap();
    let publication =
        requests.iter().find(|request| request["method"] == "publish_intent").unwrap();
    assert_eq!(publication["params"][0]["signed_data"]["payload"], signed["payload"]);
    drop(requests);
    std::fs::remove_dir_all(dir)?;
    relay_mock.task.abort();
    Ok(())
}
