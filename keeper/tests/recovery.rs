#[path = "../../agent/tests/common_http/mod.rs"]
mod common_http;

use common_http::{result, server as http_server};
use mandate_keeper::{Keeper, KeeperState, OperationJournal, PaidOutcomeUnknown, Tick};
use near_api::types::crypto::secret_key::ED25519SecretKey;
use near_api::types::transaction::{SignedTransaction, actions::Action};
use near_api::{NetworkConfig, SecretKey, Signer};
use near_sdk::base64::{Engine, engine::general_purpose::STANDARD};
use near_sdk::serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const NOW: u64 = 1_793_448_000;
const BLOCK: &str = "11111111111111111111111111111111";

async fn server<F>(handler: F) -> common_http::Mock
where
    F: Fn(Value) -> Value + Send + Sync + 'static,
{
    server_with_status(handler, |_| {}).await
}

async fn server_with_status<F, G>(handler: F, alter_status: G) -> common_http::Mock
where
    F: Fn(Value) -> Value + Send + Sync + 'static,
    G: Fn(&mut Value) + Send + Sync + 'static,
{
    let submitted: Mutex<Option<Value>> = Mutex::new(None);
    http_server(move |request| {
        if request["method"] == "tx" {
            let mut observation =
                submitted.lock().unwrap().clone().expect("status follows a submission");
            assert_eq!(
                request["params"],
                json!({"tx_hash":observation["transaction"]["hash"],
                "sender_account_id":"keeper.testnet", "wait_until":"FINAL"})
            );
            alter_status(&mut observation);
            let mut reply = result(observation);
            reply["id"] = request["id"].clone();
            return reply;
        }
        let reply = handler(request.clone());
        if request["method"] == "send_tx" {
            *submitted.lock().unwrap() = Some(reply["result"].clone());
        }
        reply
    })
    .await
}

fn directory() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "mandate-keeper-recovery-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&path).unwrap();
    path.canonicalize().unwrap()
}

// A deterministic, public fixture identity. No operator credentials are loaded.
fn fixture_key() -> SecretKey {
    SecretKey::ED25519(ED25519SecretKey::from_secret_key([7; 32]))
}

fn keeper(url: &str) -> Keeper {
    let mut network = NetworkConfig::from_rpc_url("fixture", url.parse().unwrap());
    for endpoint in &mut network.rpc_endpoints {
        endpoint.retries = 1;
    }
    Keeper {
        network,
        vault: "vault.testnet".parse().unwrap(),
        expected_chain_id: Some("fixture".into()),
        account: "keeper.testnet".parse().unwrap(),
        signer: Signer::from_secret_key(fixture_key()).unwrap(),
    }
}

fn snapshot(id: &str, started: u64) -> Value {
    json!({
        "protocol":"unique_voucher_v1", "withdrawal_id":id,
        "stage":"Dispatched", "started_sec":started,
        "recover_after_sec":started.saturating_add(300), "required_gas_tgas":140
    })
}

fn view(value: Value) -> Value {
    result(json!({"block_hash":BLOCK,"block_height":25,"logs":[],
        "result":near_sdk::serde_json::to_vec(&value).unwrap()}))
}

fn public_response(request: &Value, recovery: Value) -> Value {
    match request["method"].as_str().unwrap() {
        "status" => result(json!({"chain_id":"fixture"})),
        "query" => {
            let params = &request["params"];
            if params["request_type"] == "view_access_key" {
                return result(json!({"block_hash":BLOCK,"block_height":25,
                    "nonce":0,"permission":"FullAccess"}));
            }
            let value = match params["method_name"].as_str().unwrap() {
                "get_chain_time_sec" => json!(NOW),
                "get_signed_intent" => Value::Null,
                "get_withdrawal_recovery_state" => recovery,
                "get_pending_proposal" => Value::Null,
                other => panic!("unexpected keeper view {other}"),
            };
            if params["method_name"] != "get_chain_time_sec" {
                assert_eq!(params["block_id"], BLOCK, "views use the same final anchor");
            }
            view(value)
        }
        other => panic!("unexpected public request {other}"),
    }
}

fn sent_transaction(request: &Value) -> SignedTransaction {
    let bytes = STANDARD.decode(request["params"]["signed_tx_base64"].as_str().unwrap()).unwrap();
    // NEAR's legacy V0 wire format has no enum tag. The SDK's derived
    // Transaction deserializer expects one, unlike its V0 serializer.
    use near_sdk::borsh::BorshDeserialize;
    let mut remaining = bytes.as_slice();
    let transaction =
        near_api::types::transaction::TransactionV0::deserialize_reader(&mut remaining).unwrap();
    let signature = near_api::types::Signature::deserialize_reader(&mut remaining).unwrap();
    assert!(remaining.is_empty());
    SignedTransaction::new(signature, near_api::types::transaction::Transaction::V0(transaction))
}

fn send_response(request: &Value, outcome: Value) -> Value {
    let signed = sent_transaction(request);
    let transaction = &signed.transaction;
    let actions: Vec<Value> = transaction.actions().iter().map(|action| {
        let Action::FunctionCall(call) = action else { panic!("only a function call is authorized") };
        json!({"FunctionCall":{"method_name":call.method_name,"args":STANDARD.encode(&call.args),
            "gas":call.gas.as_gas(),"deposit":call.deposit.as_yoctonear().to_string()}})
    }).collect();
    let encoded = STANDARD.encode(near_sdk::serde_json::to_vec(&outcome).unwrap());
    result(json!({
        "final_execution_status":"FINAL", "status":{"SuccessValue":encoded},
        "transaction":{
            "signer_id":transaction.signer_id(),"receiver_id":transaction.receiver_id(),
            "public_key":transaction.public_key(),"nonce":transaction.nonce(),
            "signature":signed.signature,"hash":signed.get_hash(),"actions":actions
        },
        "transaction_outcome":{
            "id":signed.get_hash(),"block_hash":BLOCK,"proof":[],
            "outcome":{"executor_id":transaction.signer_id(),"gas_burnt":0,"tokens_burnt":"0",
                "logs":[],"receipt_ids":[],"status":{"SuccessValue":""}}
        },"receipts_outcome":[]
    }))
}

#[tokio::test]
async fn paid_fixture_response_matches_the_near_rpc_schema_without_network() {
    let key = fixture_key();
    let signed = near_api::Contract("vault.testnet".parse().unwrap())
        .call_function("recover_withdrawal_attempt", json!({"withdrawal_id":"1"}))
        .transaction()
        .gas(near_api::NearGas::from_tgas(140))
        .with_signer(
            "keeper.testnet".parse().unwrap(),
            Signer::from_secret_key(key.clone()).unwrap(),
        )
        .presign_offline(key.public_key(), BLOCK.parse().unwrap(), 1)
        .await
        .unwrap();
    let signed = signed.transaction.signed().unwrap();
    let request = json!({"params":{"signed_tx_base64":STANDARD.encode(near_sdk::borsh::to_vec(&signed).unwrap())}});
    let response = send_response(&request, json!(false));
    let parsed: near_api::types::RpcTransactionResponse =
        near_sdk::serde_json::from_value(response["result"].clone()).unwrap();
    let near_api::types::RpcTransactionResponse::Variant1 { transaction, status, .. } = parsed
    else {
        panic!()
    };
    assert_eq!(transaction.hash.to_string(), signed.get_hash().to_string());
    assert_eq!(
        near_sdk::serde_json::to_value(status).unwrap(),
        json!({"SuccessValue":STANDARD.encode(b"false")})
    );
}

#[tokio::test]
async fn strict_snapshot_and_early_window_never_reach_signing() {
    let mut cases = vec![snapshot("1", NOW - 299)];
    for (field, value) in [
        ("protocol", json!("invented")),
        ("withdrawal_id", json!("01")),
        ("withdrawal_id", json!(0)),
        ("withdrawal_id", json!("18446744073709551616")),
        ("stage", json!("Unknown")),
        ("required_gas_tgas", json!(139)),
        ("started_sec", json!(NOW + 1)),
        ("recover_after_sec", json!(NOW - 1)),
        ("started_sec", json!("1793447000")),
        ("extra", json!(true)),
    ] {
        let mut malformed = snapshot("1", NOW - 300);
        malformed[field] = value;
        cases.push(malformed);
    }
    for (index, recovery) in cases.into_iter().enumerate() {
        let mock = server(move |request| {
            assert!(matches!(request["method"].as_str(), Some("status" | "query")));
            assert_ne!(request["params"]["request_type"], "view_access_key");
            let mut response = public_response(&request, recovery.clone());
            response["id"] = request["id"].clone();
            response
        })
        .await;
        let observed = keeper(&mock.url).tick(None).await;
        if index == 0 {
            assert!(matches!(observed.unwrap(), Tick::RecoveryWaiting { .. }));
        } else {
            assert!(observed.is_err());
        }
        mock.task.abort();
    }
}

#[tokio::test]
async fn issued_signature_handoff_precedes_withdrawal_recovery() {
    let signed = json!({"proposal_id":"5","expires_sec":NOW + 60,"payload":"public fixture"});
    let returned = signed.clone();
    let mock = server(move |request| {
        assert_ne!(request["params"]["method_name"], "get_withdrawal_recovery_state");
        let mut response = if request["params"]["method_name"] == "get_signed_intent" {
            view(returned.clone())
        } else {
            public_response(&request, snapshot("1", NOW - 300))
        };
        response["id"] = request["id"].clone();
        response
    })
    .await;
    assert_eq!(keeper(&mock.url).tick(None).await.unwrap(), Tick::Publish(signed));
    mock.task.abort();
}

#[tokio::test]
async fn absent_snapshot_is_idle_and_missing_abi_or_wrong_anchor_fails_before_signing() {
    for case in 0..3 {
        let mock = server(move |request| {
            assert_ne!(request["params"]["request_type"], "view_access_key");
            let mut response = public_response(&request, Value::Null);
            if request["params"]["method_name"] == "get_withdrawal_recovery_state" {
                if case == 1 {
                    response = json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":"missing ABI"}});
                } else if case == 2 {
                    response["result"]["block_height"] = json!(26);
                }
            }
            response["id"] = request["id"].clone();
            response
        }).await;
        let outcome = keeper(&mock.url).tick(None).await;
        if case == 0 {
            assert_eq!(outcome.unwrap(), Tick::Idle);
        } else {
            assert!(outcome.is_err());
        }
        mock.task.abort();
    }
}

#[tokio::test]
async fn recovery_rechecks_chain_before_preparing_or_broadcasting() {
    let checks = AtomicUsize::new(0);
    let mock = server(move |request| {
        assert_ne!(request["params"]["request_type"], "view_access_key");
        let mut response = public_response(&request, snapshot("1", NOW - 300));
        if request["method"] == "status" && checks.fetch_add(1, Ordering::SeqCst) > 0 {
            response = result(json!({"chain_id":"wrong"}));
        }
        response["id"] = request["id"].clone();
        response
    })
    .await;
    let error = keeper(&mock.url).tick(None).await.unwrap_err();
    assert!(error.to_string().contains("RPC network differs"));
    assert!(!error.is::<PaidOutcomeUnknown>());
    assert_eq!(mock.requests.lock().unwrap().iter().filter(|r| r["method"] == "status").count(), 2);
    mock.task.abort();
}

#[tokio::test]
async fn raced_new_attempt_does_not_change_the_submitted_recovery_id() {
    let checks = AtomicUsize::new(0);
    let mock = server(move |request| {
        let mut response = if request["method"] == "send_tx" {
            let signed = sent_transaction(&request);
            let Action::FunctionCall(call) = &signed.transaction.actions()[0] else { panic!() };
            let args: Value = near_sdk::serde_json::from_slice(&call.args).unwrap();
            assert_eq!(args, json!({"withdrawal_id":"8"}));
            send_response(&request, json!(false)) // The actual vault rejects stale IDs without receipts.
        } else {
            if request["method"] == "status" {
                checks.fetch_add(1, Ordering::SeqCst);
            }
            let recovery = if checks.load(Ordering::SeqCst) > 1 {
                snapshot("9", NOW)
            } else {
                snapshot("8", NOW - 300)
            };
            public_response(&request, recovery)
        };
        response["id"] = request["id"].clone();
        response
    })
    .await;
    let outbox = directory();
    let journal = OperationJournal::acquire(&outbox).unwrap();
    let keeper = keeper(&mock.url);
    let mut state = KeeperState::default();
    assert_eq!(
        keeper.tick_guarded(None, &mut state, &journal).await.unwrap(),
        Tick::Recovery { withdrawal_id: "8".into(), resolved: false }
    );
    assert_eq!(
        keeper.tick_guarded(None, &mut state, &journal).await.unwrap(),
        Tick::RecoveryWaiting {
            withdrawal_id: "9".into(),
            reason: "recovery window has not passed"
        }
    );
    drop(journal);
    std::fs::remove_dir_all(outbox).unwrap();
    mock.task.abort();
}

#[tokio::test]
async fn paid_rpc_failure_is_typed_and_retains_the_operation_fence() {
    let outbox = directory();
    let mock = server(|request| {
        let mut response = if request["method"] == "send_tx" {
            // A malformed RPC response cannot establish rejection or absence.
            result(json!({"unavailable":true}))
        } else {
            public_response(&request, snapshot("1", NOW - 300))
        };
        response["id"] = request["id"].clone();
        response
    })
    .await;
    let journal = OperationJournal::acquire(&outbox).unwrap();
    assert!(
        keeper(&mock.url)
            .tick_guarded(None, &mut KeeperState::default(), &journal)
            .await
            .unwrap_err()
            .is::<PaidOutcomeUnknown>()
    );
    assert!(outbox.join("keeper-operation.json").is_file());
    assert_eq!(
        mock.requests.lock().unwrap().iter().filter(|r| r["method"] == "send_tx").count(),
        1
    );
    drop(journal);
    std::fs::remove_dir_all(outbox).unwrap();
    mock.task.abort();
}

#[tokio::test]
async fn only_exact_final_status_clears_the_paid_operation_fence() {
    for case in 0..8 {
        let outbox = directory();
        let mock = server_with_status(
            move |request| {
                let mut response = if request["method"] == "send_tx" {
                    let mut response = send_response(&request, json!(false));
                    if case == 0 {
                        response["result"]["final_execution_status"] = json!("EXECUTED_OPTIMISTIC");
                    }
                    response
                } else {
                    public_response(&request, snapshot("1", NOW - 300))
                };
                response["id"] = request["id"].clone();
                response
            },
            move |response| match case {
                0 => response["final_execution_status"] = json!("EXECUTED_OPTIMISTIC"),
                1 => response["transaction_outcome"]["id"] = json!(BLOCK),
                2 => response["transaction"]["receiver_id"] = json!("other.testnet"),
                3 => response["status"]["SuccessValue"] = json!(STANDARD.encode(b"true")),
                4 => response["final_execution_status"] = Value::Null,
                5 => response["transaction"]["hash"] = json!(BLOCK),
                6 => response["transaction"]["priority_fee"] = json!(1),
                _ => {}
            },
        )
        .await;
        let journal = OperationJournal::acquire(&outbox).unwrap();
        let outcome =
            keeper(&mock.url).tick_guarded(None, &mut KeeperState::default(), &journal).await;
        if case == 7 {
            assert_eq!(
                outcome.unwrap(),
                Tick::Recovery { withdrawal_id: "1".into(), resolved: false }
            );
            assert!(!outbox.join("keeper-operation.json").exists());
        } else {
            assert!(outcome.unwrap_err().is::<PaidOutcomeUnknown>());
            assert!(outbox.join("keeper-operation.json").is_file());
        }
        let calls = mock.requests.lock().unwrap();
        assert_eq!(calls.iter().filter(|request| request["method"] == "send_tx").count(), 1);
        assert_eq!(calls.iter().filter(|request| request["method"] == "tx").count(), 1);
        drop(calls);
        drop(journal);
        std::fs::remove_dir_all(outbox).unwrap();
        mock.task.abort();
    }
}

#[tokio::test]
async fn binary_exits_after_ambiguous_paid_result_instead_of_retrying() {
    let outbox = directory();
    let mock = server(|request| {
        let mut response = if request["method"] == "send_tx" {
            send_response(&request, json!("true")) // A string is not an exact boolean result.
        } else {
            public_response(&request, snapshot("1", NOW - 300))
        };
        response["id"] = request["id"].clone();
        response
    })
    .await;
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_mandate-keeper"))
            .kill_on_drop(true)
            .env_clear()
            .env("NEAR_RPC_URL", &mock.url)
            .env("VAULT_ID", "vault.testnet")
            .env("EXPECTED_CHAIN_ID", "fixture")
            .env("KEEPER_OUTBOX", &outbox)
            .env("KEEPER_ACCOUNT", "keeper.testnet")
            .env("KEEPER_SECRET_KEY", fixture_key().to_string())
            .output(),
    )
    .await
    .expect("ambiguous paid outcome must exit without retry sleep")
    .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr).unwrap().contains("paid keeper outcome unknown"));
    assert!(outbox.join("keeper-operation.json").is_file());
    assert_eq!(
        mock.requests.lock().unwrap().iter().filter(|r| r["method"] == "send_tx").count(),
        1
    );
    std::fs::remove_dir_all(outbox).unwrap();
    mock.task.abort();
}

#[tokio::test]
async fn false_recovery_is_id_bound_fenced_and_cooled_down() {
    let outbox = directory();
    let guard_path = outbox.join("keeper-operation.json");
    let proof_path = guard_path.clone();
    let recovery = Arc::new(Mutex::new(snapshot("8", NOW - 300)));
    let observed = recovery.clone();
    let mock = server(move |request| {
        let mut response = if request["method"] == "send_tx" {
            let signed = sent_transaction(&request);
            let Action::FunctionCall(call) = &signed.transaction.actions()[0] else { panic!() };
            assert_eq!(call.method_name, "recover_withdrawal_attempt");
            assert_eq!(call.gas.as_gas(), 140_000_000_000_000);
            assert_eq!(call.deposit.as_yoctonear(), 0);
            let args: Value = near_sdk::serde_json::from_slice(&call.args).unwrap();
            assert_eq!(args["withdrawal_id"], observed.lock().unwrap()["withdrawal_id"]);
            let recorded: Value =
                near_sdk::serde_json::from_slice(&std::fs::read(&proof_path).unwrap()).unwrap();
            assert_eq!(recorded["attempt_id"], args["withdrawal_id"]);
            assert_eq!(recorded["transaction_hash"], signed.get_hash().to_string());
            assert_eq!(recorded.as_object().unwrap().len(), 7);
            send_response(&request, json!(false))
        } else {
            public_response(&request, observed.lock().unwrap().clone())
        };
        response["id"] = request["id"].clone();
        response
    })
    .await;
    let keeper = keeper(&mock.url);
    let journal = OperationJournal::acquire(&outbox).unwrap();
    let mut state = KeeperState::default();
    assert_eq!(
        keeper.tick_guarded(None, &mut state, &journal).await.unwrap(),
        Tick::Recovery { withdrawal_id: "8".into(), resolved: false }
    );
    assert!(!guard_path.exists());
    assert!(matches!(
        keeper.tick_guarded(None, &mut state, &journal).await.unwrap(),
        Tick::RecoveryWaiting { .. }
    ));
    recovery.lock().unwrap()["withdrawal_id"] = json!("9");
    assert_eq!(
        keeper.tick_guarded(None, &mut state, &journal).await.unwrap(),
        Tick::Recovery { withdrawal_id: "9".into(), resolved: false }
    );
    assert_eq!(
        mock.requests.lock().unwrap().iter().filter(|r| r["method"] == "send_tx").count(),
        2
    );
    drop(journal);
    std::fs::remove_dir_all(outbox).unwrap();
    mock.task.abort();
}

#[tokio::test]
async fn malformed_paid_outcome_stops_the_session_and_survives_restart() {
    let outbox = directory();
    let mock = server(|request| {
        let mut response = if request["method"] == "send_tx" {
            send_response(&request, json!({"resolved":true}))
        } else {
            public_response(&request, snapshot("1", NOW - 300))
        };
        response["id"] = request["id"].clone();
        response
    })
    .await;
    let keeper = keeper(&mock.url);
    let journal = OperationJournal::acquire(&outbox).unwrap();
    let mut state = KeeperState::default();
    assert!(
        keeper
            .tick_guarded(None, &mut state, &journal)
            .await
            .unwrap_err()
            .is::<PaidOutcomeUnknown>()
    );
    let requests = mock.requests.lock().unwrap().len();
    assert!(keeper.tick_guarded(None, &mut state, &journal).await.is_err());
    assert_eq!(mock.requests.lock().unwrap().len(), requests);
    drop(journal);
    assert!(OperationJournal::acquire(&outbox).is_err());
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_mandate-keeper"))
        .env_clear()
        .env("NEAR_RPC_URL", &mock.url)
        .env("VAULT_ID", "vault.testnet")
        .env("EXPECTED_CHAIN_ID", "fixture")
        .env("KEEPER_OUTBOX", &outbox)
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("operator reconciliation before signing"));
    assert!(!stderr.contains("KEEPER_ACCOUNT"));
    assert_eq!(
        mock.requests.lock().unwrap().iter().filter(|r| r["method"] == "send_tx").count(),
        1
    );
    std::fs::remove_dir_all(outbox).unwrap();
    mock.task.abort();
}

#[test]
fn outbox_excludes_concurrent_writers_and_refuses_symlinks() {
    let outbox = directory();
    let journal = OperationJournal::acquire(&outbox).unwrap();
    assert!(OperationJournal::acquire(&outbox).is_err());
    drop(journal);
    OperationJournal::acquire(&outbox).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let target = outbox.join("public-target");
        std::fs::write(&target, b"unchanged").unwrap();
        std::fs::remove_file(outbox.join("keeper.lock")).unwrap();
        symlink(&target, outbox.join("keeper.lock")).unwrap();
        assert!(OperationJournal::acquire(&outbox).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"unchanged");
        std::fs::remove_file(outbox.join("keeper.lock")).unwrap();
        symlink(&target, outbox.join("keeper-operation.json")).unwrap();
        assert!(OperationJournal::acquire(&outbox).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"unchanged");
    }
    std::fs::remove_dir_all(outbox).unwrap();
}
