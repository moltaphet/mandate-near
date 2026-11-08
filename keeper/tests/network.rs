#[path = "../../agent/tests/common_http/mod.rs"]
mod common_http;

use common_http::{result, server};
use mandate_keeper::{Keeper, preflight_network};
use near_api::errors::{PublicKeyError, SignerError};
use near_api::signer::SignerTrait;
use near_api::{AccountId, NetworkConfig, PublicKey, SecretKey, Signer};
use near_sdk::serde_json::{Value, json};

fn final_time() -> Value {
    result(json!({
        "block_hash":"11111111111111111111111111111111",
        "block_height":25,
        "result":near_sdk::serde_json::to_vec(&1_793_448_000u64).unwrap()
    }))
}

/// Supplies only a public identity. Any regression that reaches signing fails
/// without loading or constructing secret material.
struct RefuseSigning;

#[async_trait::async_trait]
impl SignerTrait for RefuseSigning {
    async fn get_secret_key(&self, _: &AccountId, _: PublicKey) -> Result<SecretKey, SignerError> {
        panic!("network mismatch must never reach signing")
    }

    fn get_public_key(&self) -> Result<PublicKey, PublicKeyError> {
        Ok("ed25519:11111111111111111111111111111111".parse().unwrap())
    }
}

fn keeper(url: &str) -> Keeper {
    Keeper {
        network: NetworkConfig::from_rpc_url("fixture", url.parse().unwrap()),
        vault: "vault.testnet".parse().unwrap(),
        expected_chain_id: Some("fixture".into()),
        account: "keeper.testnet".parse().unwrap(),
        signer: Signer::new(RefuseSigning).unwrap(),
    }
}

#[tokio::test]
async fn library_tick_refuses_a_wrong_chain_before_readiness_or_signing() {
    let mock = server(|request| {
        let response = result(json!({"chain_id":"different"}));
        assert_eq!(request["method"], "status");
        response
    })
    .await;
    assert!(
        keeper(&mock.url)
            .tick(None)
            .await
            .unwrap_err()
            .to_string()
            .contains("RPC network differs from the configured expected chain")
    );
    assert_eq!(mock.requests.lock().unwrap().len(), 1);
    mock.task.abort();
}

#[tokio::test]
async fn chain_is_checked_again_after_readiness_and_before_execute() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let checks = AtomicUsize::new(0);
    let mock = server(move |request| {
        let response = match request["method"].as_str().unwrap() {
            "status" => result(json!({"chain_id":if checks.fetch_add(1, Ordering::SeqCst) == 0 { "fixture" } else { "different" }})),
            "query" => {
                assert_eq!(request["params"]["request_type"], "call_function");
                let value = match request["params"]["method_name"].as_str().unwrap() {
                    "get_chain_time_sec" => json!(1_793_448_000u64),
                    "get_signed_intent" => Value::Null,
                    "get_withdrawal_recovery_state" => Value::Null,
                    "get_pending_proposal" => json!({"id":"1","expires_sec":1_793_448_060u64}),
                    "is_in_flight" | "is_paused" | "is_halted" => json!(false),
                    "is_key_registered" => json!(true),
                    other => panic!("unexpected readiness method {other}"),
                };
                result(json!({"block_hash":"11111111111111111111111111111111", "block_height":25,
                    "logs":[], "result":near_sdk::serde_json::to_vec(&value).unwrap()}))
            }
            _ => panic!("network mismatch must prevent transaction requests"),
        };
        let mut response = response;
        response["id"] = request["id"].clone();
        response
    }).await;
    assert!(
        keeper(&mock.url)
            .tick(None)
            .await
            .unwrap_err()
            .to_string()
            .contains("RPC network differs from the configured expected chain")
    );
    let requests = mock.requests.lock().unwrap();
    assert_eq!(requests.iter().filter(|r| r["method"] == "status").count(), 2);
    assert!(requests.iter().any(|r| r["params"]["method_name"] == "is_key_registered"));
    assert!(requests.iter().all(|r| ["status", "query"].contains(&r["method"].as_str().unwrap())));
    drop(requests);
    mock.task.abort();
}

#[tokio::test]
async fn wrong_chain_cli_refuses_before_signer_configuration_or_transaction_requests() {
    let mock = server(|request| {
        assert_eq!(request["method"], "status");
        result(json!({"chain_id":"testnet"}))
    })
    .await;
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_mandate-keeper"))
        .env_clear()
        .env("NEAR_RPC_URL", &mock.url)
        .env("VAULT_ID", "vault.testnet")
        .env("EXPECTED_CHAIN_ID", "mainnet")
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("RPC network differs from the configured expected chain"));
    assert!(!stderr.contains("KEEPER_ACCOUNT"));
    assert_eq!(mock.requests.lock().unwrap().len(), 1);
    mock.task.abort();
}

#[tokio::test]
async fn publication_chain_pin_also_refuses_before_any_keeper_transaction() {
    let mock = server(|request| {
        assert_eq!(request["method"], "status");
        result(json!({"chain_id":"testnet"}))
    })
    .await;
    let dir = std::env::temp_dir().join(format!("mandate-keeper-public-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("relay-public.json");
    std::fs::write(
        &config,
        near_sdk::serde_json::to_vec(&json!({
            "relay_url":mock.url,"metadata_dir":dir,"expected_chain_id":"mainnet"
        }))
        .unwrap(),
    )
    .unwrap();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_mandate-keeper"))
        .env_clear()
        .env("NEAR_RPC_URL", &mock.url)
        .env("VAULT_ID", "vault.testnet")
        .arg("--publish")
        .arg(&config)
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("RPC network differs from the configured expected chain")
    );
    assert_eq!(mock.requests.lock().unwrap().len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
    mock.task.abort();
}

#[tokio::test]
async fn every_failover_endpoint_is_checked_and_unpinned_identity_is_still_required() {
    let good = server(|request| match request["method"].as_str().unwrap() {
        "status" => result(json!({"chain_id":"testnet"})),
        "query" => final_time(),
        _ => panic!("network preflight must never broadcast a transaction"),
    })
    .await;
    let bad = server(|request| {
        assert_eq!(request["method"], "status");
        result(json!({"chain_id":"mainnet"}))
    })
    .await;
    let mut network = NetworkConfig::from_rpc_url("fixture", good.url.parse().unwrap());
    preflight_network(&network, "vault.testnet", None).await.unwrap();
    network
        .rpc_endpoints
        .extend(NetworkConfig::from_rpc_url("other", bad.url.parse().unwrap()).rpc_endpoints);
    assert!(preflight_network(&network, "vault.testnet", Some("testnet")).await.is_err());
    assert_eq!(bad.requests.lock().unwrap().len(), 1);
    good.task.abort();
    bad.task.abort();
    let unidentified = server(|_| result(json!({}))).await;
    let network = NetworkConfig::from_rpc_url("fixture", unidentified.url.parse().unwrap());
    assert!(preflight_network(&network, "vault.testnet", None).await.is_err());
    unidentified.task.abort();
}

#[tokio::test]
async fn conflicting_environment_and_publication_pins_refuse_before_rpc_or_credentials() {
    let mock =
        server(|_| panic!("conflicting configuration must fail before network access")).await;
    let dir = std::env::temp_dir().join(format!("mandate-conflict-public-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("relay-public.json");
    std::fs::write(
        &config,
        near_sdk::serde_json::to_vec(&json!({
            "relay_url":mock.url,"metadata_dir":dir,"expected_chain_id":"mainnet"
        }))
        .unwrap(),
    )
    .unwrap();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_mandate-keeper"))
        .env_clear()
        .env("NEAR_RPC_URL", &mock.url)
        .env("VAULT_ID", "vault.testnet")
        .env("NEAR_CHAIN_ID", "testnet")
        .arg("--publish")
        .arg(&config)
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("keeper environment and publication expected chains differ")
    );
    assert!(mock.requests.lock().unwrap().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
    mock.task.abort();
}
