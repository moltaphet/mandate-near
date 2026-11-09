mod common_http;

use common_http::{result, server};
use mandate_agent::Plan;
use mandate_agent::relay::{ProposalBinding, Relay};
use mandate_agent::transport::{JsonRpc, PublicReader, read_public_json, validate_public_path};
use near_sdk::json_types::{U64, U128};
use near_sdk::serde_json::{Value, json};
use std::path::Path;

const NOW: u64 = 1_793_448_000;
const A: &str = "nep141:usd.near";
const B: &str = "nep141:wrap.near";
fn block() -> String {
    bs58::encode([3u8; 32]).into_string()
}
fn offer() -> Value {
    json!({"quote_hash":"fixture-offer-1","defuse_asset_identifier_in":A,
    "defuse_asset_identifier_out":B,"amount_in":"20000000","amount_out":"4000000000000000000000000",
    "expiration_time":mandate_vault::intent::rfc3339_utc(NOW+600)})
}
fn binding() -> ProposalBinding {
    ProposalBinding {
        proposal_id: U64(7),
        vault_id: "vault.near".into(),
        intents_id: "intents.near".into(),
        block_hash: block(),
        plan: Plan {
            asset_in: A.into(),
            asset_out: B.into(),
            amount_in: U128(20_000_000),
            min_amount_out: U128(4 * 10u128.pow(24)),
            rationale: "fixture quote rebalance".into(),
            response_commitment: None,
        },
        quote: near_sdk::serde_json::from_value(offer()).unwrap(),
    }
}
fn signed() -> Value {
    let payload = mandate_vault::intent::token_diff_payload(&mandate_vault::intent::TokenDiff {
        signer_id: "vault.near",
        verifying_contract: "intents.near",
        deadline_sec: NOW + 120,
        nonce: [2; 32],
        asset_in: A,
        asset_out: B,
        amount_in: 20_000_000,
        min_amount_out: 4 * 10u128.pow(24),
    })
    .unwrap();
    json!({"proposal_id":"7","payload":payload,"public_key":format!("ed25519:{}",bs58::encode([0u8;32]).into_string()),
        "signature":format!("ed25519:{}",bs58::encode([0u8;64]).into_string()),"expires_sec":NOW+120})
}

#[tokio::test]
async fn quote_publish_and_status_use_documented_protocol_and_do_not_invent_settlement() {
    let mock=server(|request| match request["method"].as_str().unwrap() {
        "quote"=>{assert_eq!(request["params"],json!([{"defuse_asset_identifier_in":A,"defuse_asset_identifier_out":B,
            "exact_amount_in":"20000000","min_deadline_ms":450000}]));result(json!([offer()]))},
        "publish_intent"=>{assert_eq!(request["params"][0]["quote_hashes"],json!(["fixture-offer-1"]));
            assert_eq!(request["params"][0]["signed_data"]["standard"],"raw_ed25519");result(json!({"status":"OK","intent_hash":"fixture-intent-1"}))},
        "get_status"=>{assert_eq!(request["params"],json!([{"intent_hash":"fixture-intent-1"}]));
            result(json!({"intent_hash":"fixture-intent-1","status":"SETTLED","data":{"hash":block()}}))},
        _=>panic!("unexpected protocol method"),
    }).await;
    // A clearly public fixture value proves the documented header; no credential is read.
    let relay = Relay::new(&mock.url, Some("public-fixture-value")).unwrap();
    let quote = relay.quote(A, B, 20_000_000, NOW, 450).await.unwrap();
    assert_eq!(quote.amount_out.0, 4 * 10u128.pow(24));
    let ack = relay.publish(&binding(), &signed(), "vault.near", NOW).await.unwrap();
    assert_eq!(ack.status, "acknowledged_not_settled");
    let observed = relay.status(&ack.intent_hash).await.unwrap();
    assert_eq!(observed.relay_status, "SETTLED");
    assert!(!observed.settlement_verified);
    assert_eq!(mock.requests.lock().unwrap().len(), 3);
    assert!(
        mock.requests.lock().unwrap().iter().all(|request| request["_fixture_has_auth"] == true)
    );
    mock.task.abort();
}

#[tokio::test]
async fn malformed_expired_and_mismatched_quotes_never_get_selected() {
    for mode in ["amount", "asset", "expired", "malformed"] {
        let mock = server(move |_| {
            let mut quote = offer();
            match mode {
                "amount" => quote["amount_in"] = json!("19999999"),
                "asset" => quote["defuse_asset_identifier_out"] = json!(A),
                "expired" => {
                    quote["expiration_time"] = json!(mandate_vault::intent::rfc3339_utc(NOW))
                }
                _ => quote["amount_out"] = json!("not-an-integer"),
            };
            result(json!([quote]))
        })
        .await;
        assert!(
            Relay::new(&mock.url, None).unwrap().quote(A, B, 20_000_000, NOW, 450).await.is_err()
        );
        mock.task.abort();
    }
}

#[test]
fn publication_association_rejects_wrong_ids_amounts_hashes_curves_and_deadlines() {
    let binding = binding();
    let signed = signed();
    assert!(binding.publish_params(&signed, "vault.near", NOW).is_ok());
    let mut wrong = signed.clone();
    wrong["proposal_id"] = json!("8");
    assert!(binding.publish_params(&wrong, "vault.near", NOW).is_err());
    let mut wrong = signed.clone();
    wrong["signature"] = json!("ed25519:short");
    assert!(binding.publish_params(&wrong, "vault.near", NOW).is_err());
    let mut wrong = signed.clone();
    wrong["public_key"] = json!("secp256k1:short");
    assert!(binding.publish_params(&wrong, "vault.near", NOW).is_err());
    let mut changed = binding.clone();
    changed.plan.min_amount_out = U128(changed.plan.min_amount_out.0 - 1);
    assert!(changed.publish_params(&signed, "vault.near", NOW).is_err());
    let mut changed = binding.clone();
    changed.block_hash = "not-a-block".into();
    assert!(changed.publish_params(&signed, "vault.near", NOW).is_err());
    let mut changed = binding.clone();
    changed.quote.expiration_time = mandate_vault::intent::rfc3339_utc(NOW + 120);
    assert!(changed.publish_params(&signed, "vault.near", NOW).is_err());
    let mut content: Value =
        near_sdk::serde_json::from_str(signed["payload"].as_str().unwrap()).unwrap();
    content["intents"][0]["diff"][B] = json!("4000000000000000000000001");
    let mut wrong = signed;
    wrong["payload"] = json!(near_sdk::serde_json::to_string(&content).unwrap());
    assert!(binding.publish_params(&wrong, "vault.near", NOW).is_err());
}

#[tokio::test]
async fn final_views_are_pinned_and_wrong_height_network_or_hash_is_refused() {
    let mock=server(|request| {
        if request["method"]=="status" {return result(json!({"chain_id":"fixture"}));}
        if request["params"]["method_name"]=="get_chain_time_sec" {
            assert_eq!(request["params"]["finality"],"final");
            result(json!({"block_hash":block(),"block_height":25,"result":near_sdk::serde_json::to_vec(&NOW).unwrap()}))
        } else {
            assert_eq!(request["params"]["block_id"],block());assert!(request["params"].get("finality").is_none());
            result(json!({"block_hash":block(),"block_height":26,"result":near_sdk::serde_json::to_vec(&true).unwrap()}))
        }
    }).await;
    let reader =
        PublicReader::new(&mock.url, "vault.near").unwrap().with_chain(Some("fixture")).unwrap();
    let (_, hash) = reader.final_time().await.unwrap();
    assert!(reader.view::<bool>("vault.near", "is_paused", json!({}), &hash).await.is_err());
    let wrong =
        PublicReader::new(&mock.url, "vault.near").unwrap().with_chain(Some("mainnet")).unwrap();
    assert!(wrong.final_time().await.is_err());
    mock.task.abort();
    let mock=server(|request|if request["method"]=="status" {result(json!({"chain_id":"fixture"}))}
        else {result(json!({"block_hash":"invalid","block_height":25,"result":near_sdk::serde_json::to_vec(&NOW).unwrap()}))}).await;
    assert!(PublicReader::new(&mock.url, "vault.near").unwrap().final_time().await.is_err());
    mock.task.abort();
}

#[tokio::test]
async fn oversized_error_and_mismatched_rpc_envelopes_fail_without_echoing_data() {
    let mock = server(|_| result(json!("x".repeat(70_000)))).await;
    let error = JsonRpc::new(&mock.url, None, 64 * 1024)
        .unwrap()
        .call("quote", json!([]))
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(error, "public RPC response exceeds size limit");
    mock.task.abort();
    let mock = server(
        |_| json!({"jsonrpc":"2.0","id":1,"error":{"message":"do not echo this fixture body"}}),
    )
    .await;
    assert_eq!(
        JsonRpc::new(&mock.url, None, 1024)
            .unwrap()
            .call("quote", json!([]))
            .await
            .unwrap_err()
            .to_string(),
        "public RPC returned an application error"
    );
    mock.task.abort();
    let mock = server(|_| json!({"jsonrpc":"2.0","id":2,"result":[]})).await;
    assert!(JsonRpc::new(&mock.url, None, 1024).unwrap().call("quote", json!([])).await.is_err());
    mock.task.abort();
}

#[test]
fn larger_evidence_files_keep_default_limits_and_prohibited_path_guards() {
    use mandate_agent::transport::{read_public_bytes, write_public, write_public_bounded};
    let dir = std::env::temp_dir().join(format!("mandate-bounded-fixture-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("evidence-public.json");
    let bytes = vec![b' '; 70 * 1024];
    assert_eq!(
        write_public(&path, &bytes).unwrap_err().to_string(),
        "public output exceeds size limit"
    );
    write_public_bounded(&path, &bytes, 1024 * 1024).unwrap();
    assert_eq!(read_public_bytes(&path, 1024 * 1024).unwrap(), bytes);
    assert_eq!(
        read_public_json::<Value>(&path).unwrap_err().to_string(),
        "public file exceeds size limit"
    );
    assert_eq!(
        read_public_bytes(&path, 1024 * 1024 + 1).unwrap_err().to_string(),
        "invalid public file size limit"
    );
    assert_eq!(
        write_public_bounded(&path, &[], 1024 * 1024 + 1).unwrap_err().to_string(),
        "public output exceeds size limit"
    );
    // This path need not exist: validation must refuse it before opening a file.
    assert_eq!(
        read_public_bytes(Path::new(".env"), 1024 * 1024).unwrap_err().to_string(),
        "refused prohibited public-file path"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn prohibited_public_paths_are_refused_before_file_open() {
    for path in [
        ".env",
        ".env.sample",
        "sample.env",
        "example-key.json",
        "/tmp/credentials/sample.json",
        "/tmp/.ssh/sample.json",
        "/tmp/.aws/sample.json",
        "/tmp/.config/sample.json",
    ] {
        assert!(validate_public_path(Path::new(path)).is_err());
        assert!(
            read_public_json::<Value>(Path::new(path))
                .unwrap_err()
                .to_string()
                .contains("prohibited")
        );
    }
    assert!(validate_public_path(Path::new("/private/tmp/rebalance-public.json")).is_ok());
    assert!(JsonRpc::new("https://user:password@example.com/rpc", None, 1024).is_err());
    assert!(JsonRpc::new("http://example.com/rpc", None, 1024).is_err());
}

pub fn public_view(request: Value) -> Value {
    public_view_with_sources(request, &["manual-v1", "rebalance-v1", "fixture-model"])
}

fn public_view_with_sources(request: Value, allowed_model_ids: &[&str]) -> Value {
    if request["method"] == "status" {
        return result(json!({"chain_id":"fixture"}));
    }
    if request["method"] == "quote" {
        return result(json!([offer()]));
    }
    assert_eq!(request["method"], "query");
    let method = request["params"]["method_name"].as_str().unwrap();
    if method != "get_chain_time_sec" {
        assert_eq!(request["params"]["block_id"], block());
    }
    let value = match method {
        "get_chain_time_sec" => json!(NOW),
        "is_paused" | "is_halted" | "is_in_flight" => json!(false),
        "is_key_registered" => json!(true),
        "get_agent" => json!("agent.near"),
        "get_pending_proposal" => Value::Null,
        "get_externals" => json!(["signer.near", "intents.near", "oracle.near"]),
        "get_oracle_kind" => json!("price_oracle"),
        "get_trading_state" => {
            json!({"last_trade_at_sec":null,"day_start_sec":0,"day_turnover_usd":"0","high_water_mark_usd":"0","signed_intent_expires_at_sec":0})
        }
        "get_mandate" => {
            json!({"assets":[{"asset_id":A,"decimals":6,"price_feed_id":"usd.near"},{"asset_id":B,"decimals":24,"price_feed_id":"wrap.near"}],
            "stable_assets":[A],"max_trade_usd":"20000000","max_daily_turnover_usd":"200000000","max_position_bps":6000,
            "max_slippage_bps":100,"max_drawdown_bps":1000,"min_trade_interval_sec":60,"max_price_age_sec":60,
            "allowed_model_ids":allowed_model_ids})
        }
        "mt_batch_balance_of" => json!(["100000000", "10000000000000000000000000"]),
        "get_price_data" => {
            json!({"timestamp":(NOW*1_000_000_000).to_string(),"recency_duration_sec":600,"prices":[
            {"asset_id":"usd.near","price":{"multiplier":"1","decimals":0}},
            {"asset_id":"wrap.near","price":{"multiplier":"5","decimals":0}}]})
        }
        "get_asset" => {
            json!({"reports":[{"oracle_id":"oracle.near","timestamp":(NOW*1_000_000_000).to_string(),"price":{"multiplier":"1","decimals":0}}]})
        }
        _ => panic!("unexpected public view"),
    };
    result(
        json!({"block_hash":block(),"block_height":25,"result":near_sdk::serde_json::to_vec(&value).unwrap()}),
    )
}

#[tokio::test]
async fn denied_rebalance_source_makes_public_reads_and_zero_quote_requests() {
    use mandate_agent::autonomous::{AutonomousConfig, Preparation, prepare};
    for allowed in [Vec::new(), vec!["manual-v1", "fixture-model"], vec!["unknown-model"]] {
        let mock = server(move |request| public_view_with_sources(request, &allowed)).await;
        let config = AutonomousConfig {
            rpc_url: mock.url.clone(),
            expected_chain_id: Some("fixture".into()),
            vault_id: "vault.near".into(),
            relay_url: mock.url.clone(),
            asset_a: A.into(),
            asset_b: B.into(),
            target_a_bps: 5000,
            band_bps: 100,
            minimum_quote_validity_sec: 450,
            legacy_price_oracle: false,
        };
        let Preparation::NoTrade { reason } =
            prepare(&config, &Relay::new(&mock.url, None).unwrap()).await.unwrap()
        else {
            panic!("denied source must never request a trade quote")
        };
        assert_eq!(reason, "the rebalance decision source is not allowed");
        let requests = mock.requests.lock().unwrap();
        assert!(
            requests
                .iter()
                .all(|request| request["method"] == "query" || request["method"] == "status")
        );
        assert_eq!(requests.iter().filter(|request| request["method"] == "quote").count(), 0);
        drop(requests);
        mock.task.abort();
    }
}

#[tokio::test]
async fn dry_run_binary_prepares_from_public_data_with_empty_account_environment() {
    let mock = server(public_view).await;
    let dir = std::env::temp_dir().join(format!("mandate-public-fixture-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rebalance-public.json");
    std::fs::write(&path,near_sdk::serde_json::to_vec(&json!({"rpc_url":mock.url,"expected_chain_id":"fixture","vault_id":"vault.near",
        "relay_url":mock.url,"asset_a":A,"asset_b":B,"target_a_bps":5000,"band_bps":100,"minimum_quote_validity_sec":450,"legacy_price_oracle":false})).unwrap()).unwrap();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_mandate-agent"))
        .env_clear()
        .arg("--auto")
        .arg(&path)
        .arg("--dry-run")
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "dry-run fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = near_sdk::serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "trade");
    assert_eq!(report["prepared"]["plan"]["amount_in"], "20000000");
    assert_eq!(report["prepared"]["plan"]["min_amount_out"], "4000000000000000000000000");
    assert_eq!(mock.requests.lock().unwrap().iter().filter(|r| r["method"] == "quote").count(), 1);
    assert!(
        mock.requests
            .lock()
            .unwrap()
            .iter()
            .all(|r| r["method"] == "query" || r["method"] == "quote" || r["method"] == "status")
    );
    assert!(
        mock.requests.lock().unwrap().iter().all(|request| request["_fixture_has_auth"] == false)
    );
    std::fs::remove_dir_all(dir).unwrap();
    mock.task.abort();
}

fn autonomous_config(url: &str) -> mandate_agent::autonomous::AutonomousConfig {
    mandate_agent::autonomous::AutonomousConfig {
        rpc_url: url.into(),
        expected_chain_id: Some("fixture".into()),
        vault_id: "vault.near".into(),
        relay_url: url.into(),
        asset_a: A.into(),
        asset_b: B.into(),
        target_a_bps: 5000,
        band_bps: 100,
        minimum_quote_validity_sec: 450,
        legacy_price_oracle: false,
    }
}

#[tokio::test]
async fn autonomous_loop_recovers_from_a_public_rpc_outage_without_submission() {
    use mandate_agent::decision::{PreparationAttempt, PreparationLoop, RebalanceDecision};
    use std::sync::atomic::{AtomicBool, Ordering};

    let outage = AtomicBool::new(true);
    let mock = server(move |request| {
        if request["method"] == "status" && outage.swap(false, Ordering::SeqCst) {
            json!({"jsonrpc":"2.0", "id":1, "error":{"message":"untrusted outage detail"}})
        } else {
            public_view(request)
        }
    })
    .await;
    let source =
        RebalanceDecision::new(autonomous_config(&mock.url), Relay::new(&mock.url, None).unwrap());
    let mut iterations = PreparationLoop::new(true);
    assert!(matches!(
        iterations.next(&source).await.unwrap(),
        PreparationAttempt::Retry { after } if after == std::time::Duration::from_secs(10)
    ));
    assert!(matches!(
        iterations.next(&source).await.unwrap(),
        PreparationAttempt::Ready(mandate_agent::autonomous::Preparation::Trade { .. })
    ));
    let requests = mock.requests.lock().unwrap();
    assert_eq!(requests.iter().filter(|r| r["method"] == "quote").count(), 1);
    assert!(
        requests
            .iter()
            .all(|r| ["status", "query", "quote"].contains(&r["method"].as_str().unwrap()))
    );
    drop(requests);
    mock.task.abort();
}

#[tokio::test]
async fn changed_quote_at_refresh_is_discarded_and_the_next_iteration_gets_a_new_quote() {
    use mandate_agent::decision::{PreparationAttempt, PreparationLoop, RebalanceDecision};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let snapshots = AtomicUsize::new(0);
    let mock = server(move |request| {
        if request["params"]["method_name"] == "get_chain_time_sec" {
            snapshots.fetch_add(1, Ordering::SeqCst);
        }
        let current = snapshots.load(Ordering::SeqCst);
        let method = request["params"]["method_name"].as_str().unwrap_or("").to_owned();
        let quote_request = request["method"] == "quote";
        let requested_amount = request["params"][0]["exact_amount_in"].clone();
        let mut response = public_view(request);
        if method == "get_mandate" && current >= 2 {
            let bytes: Vec<u8> =
                near_sdk::serde_json::from_value(response["result"]["result"].clone()).unwrap();
            let mut mandate: Value = near_sdk::serde_json::from_slice(&bytes).unwrap();
            mandate["max_trade_usd"] = json!("10000000");
            response["result"]["result"] = json!(near_sdk::serde_json::to_vec(&mandate).unwrap());
        }
        if quote_request && requested_amount == "10000000" {
            response["result"][0]["quote_hash"] = json!("fixture-offer-refreshed");
            response["result"][0]["amount_in"] = json!("10000000");
            response["result"][0]["amount_out"] = json!("2000000000000000000000000");
        }
        response
    })
    .await;
    let source =
        RebalanceDecision::new(autonomous_config(&mock.url), Relay::new(&mock.url, None).unwrap());
    let mut iterations = PreparationLoop::new(true);
    assert!(matches!(iterations.next(&source).await.unwrap(), PreparationAttempt::Retry { .. }));
    let PreparationAttempt::Ready(mandate_agent::autonomous::Preparation::Trade { prepared }) =
        iterations.next(&source).await.unwrap()
    else {
        panic!("the changed snapshot must receive a new quote on the next iteration")
    };
    assert_eq!(prepared.plan.amount_in.0, 10_000_000);
    assert_eq!(prepared.quote.quote_hash, "fixture-offer-refreshed");
    let requests = mock.requests.lock().unwrap();
    let amounts: Vec<&Value> = requests
        .iter()
        .filter(|r| r["method"] == "quote")
        .map(|r| &r["params"][0]["exact_amount_in"])
        .collect();
    assert_eq!(amounts, vec![&json!("20000000"), &json!("10000000")]);
    assert!(
        requests
            .iter()
            .all(|r| ["status", "query", "quote"].contains(&r["method"].as_str().unwrap()))
    );
    drop(requests);
    mock.task.abort();
}

#[tokio::test]
async fn one_shot_autonomous_binary_still_returns_a_public_preparation_failure() {
    let mock =
        server(|_| json!({"jsonrpc":"2.0","id":1,"error":{"message":"untrusted outage detail"}}))
            .await;
    let dir = std::env::temp_dir().join(format!("mandate-one-shot-public-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("rebalance-public.json");
    std::fs::write(&config, near_sdk::serde_json::to_vec(&autonomous_config(&mock.url)).unwrap())
        .unwrap();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_mandate-agent"))
        .env_clear()
        .arg("--auto")
        .arg(&config)
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("public RPC returned an application error"));
    assert!(!stderr.contains("untrusted outage detail"));
    assert_eq!(mock.requests.lock().unwrap().len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
    mock.task.abort();
}

#[tokio::test]
async fn manual_agent_network_pin_refuses_before_signer_configuration_or_transaction_requests() {
    let mock = server(|request| {
        assert_eq!(request["method"], "status");
        result(json!({"chain_id":"testnet"}))
    })
    .await;
    let dir = std::env::temp_dir().join(format!("mandate-manual-public-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plan-public.json");
    std::fs::write(&path, near_sdk::serde_json::to_vec(&binding().plan).unwrap()).unwrap();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_mandate-agent"))
        .env_clear()
        .env("NEAR_RPC_URL", &mock.url)
        .env("VAULT_ID", "vault.near")
        .env("NEAR_CHAIN_ID", "mainnet")
        .arg(&path)
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("RPC network differs from the configured expected chain"));
    assert!(!stderr.contains("AGENT_ACCOUNT"));
    assert_eq!(mock.requests.lock().unwrap().len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
    mock.task.abort();
}

#[tokio::test]
async fn autonomous_loop_keeps_fail_stop_boundary_after_public_preparation() {
    let mock = server(public_view).await;
    let dir = std::env::temp_dir().join(format!("mandate-fail-stop-public-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rebalance-public.json");
    std::fs::write(&path, near_sdk::serde_json::to_vec(&autonomous_config(&mock.url)).unwrap())
        .unwrap();
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_mandate-agent"))
            .env_clear()
            .arg("--auto")
            .arg(&path)
            .arg("--loop")
            .output(),
    )
    .await
    .expect("the submission boundary must exit without a retry sleep")
    .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("required setting AGENT_ACCOUNT is absent")
    );
    let requests = mock.requests.lock().unwrap();
    assert_eq!(requests.iter().filter(|r| r["method"] == "quote").count(), 1);
    assert_eq!(
        requests.iter().filter(|r| r["params"]["method_name"] == "get_chain_time_sec").count(),
        2
    );
    drop(requests);
    std::fs::remove_dir_all(dir).unwrap();
    mock.task.abort();
}
