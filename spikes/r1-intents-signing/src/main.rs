//! R1 spike, part 1: is a `raw_ed25519` intent, signed over its exact JSON
//! bytes, accepted by the live NEAR Intents verifier?
//!
//! The vault will get that signature from the MPC network (`v1.signer`,
//! domain 1), which signs the raw message bytes with `env::ed25519_verify`
//! semantics. Here a local key stands in for MPC. Its implicit account
//! (hex of the public key) needs no registration on `intents.near`.
//!
//! Everything runs through `simulate_intents`, a free view call. No funds, no
//! transactions, no keys stored.
//!
//! Usage:
//!   cargo run --release                          local key stands in for MPC
//!   cargo run --release -- prepare <PK> <DIR>    write payloads for an MPC-derived key
//!   cargo run --release -- submit <PK> <PAYLOAD_FILE> <MPC_RESPONSE_JSON> <EXPECT>
//!
//! `prepare`/`submit` carry part 2: the payload is signed by the real MPC
//! network (via the spike contract) and the result is checked on mainnet.
//! `EXPECT` is `ok` or a substring of the expected error.

use std::process::ExitCode;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};

const INTENTS: &str = "intents.near";
const DEFAULT_RPC: &str = "https://free.rpc.fastnear.com";
/// MPC `v1.signer` rejects EdDSA payloads larger than this.
const MPC_EDDSA_PAYLOAD_MAX: usize = 1232;

struct Case {
    name: &'static str,
    signed: Value,
    expect: Expect,
}

enum Expect {
    Ok,
    ErrContains(&'static str),
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("prepare") if args.len() == 3 => prepare(&args[1], &args[2]),
        Some("submit") if args.len() == 5 => submit(&args[1], &args[2], &args[3], &args[4]),
        Some("prepare" | "submit") => {
            eprintln!("bad arguments; see usage at the top of src/main.rs");
            ExitCode::FAILURE
        }
        _ => local_golden_test(),
    }
}

fn local_golden_test() -> ExitCode {
    let rpc = DEFAULT_RPC;
    let key = SigningKey::from_bytes(&rand::random());
    let account = hex::encode(key.verifying_key().as_bytes());
    println!("rpc: {rpc}\nsigner (implicit account): {account}\n");

    let empty = payload(&account, INTENTS, json!([]));
    let swap = payload(&account, INTENTS, sample_token_diff());
    println!("token_diff payload: {} bytes (MPC limit {MPC_EDDSA_PAYLOAD_MAX})", swap.len());
    println!("golden payload:\n{swap}\n");

    let tampered = {
        let mut signed = sign(&key, &empty);
        let original = signed["payload"].as_str().unwrap().to_owned();
        signed["payload"] = json!(original.replacen("\"intents\":[]", "\"intents\": []", 1));
        signed
    };

    let cases = [
        Case {
            name: "valid signature, no intents",
            signed: sign(&key, &empty),
            expect: Expect::Ok,
        },
        Case {
            name: "payload modified after signing",
            signed: tampered,
            expect: Expect::ErrContains("signature"),
        },
        Case {
            name: "wrong verifying_contract",
            signed: sign(&key, &payload(&account, "evil.near", json!([]))),
            expect: Expect::ErrContains("verifying"),
        },
        Case {
            name: "valid token_diff, unfunded account (must fail on balance, not signature)",
            signed: sign(&key, &swap),
            expect: Expect::ErrContains("balance"),
        },
    ];

    let mut failures = 0;
    for case in cases {
        let outcome = simulate(rpc, &case.signed);
        let pass = match (&case.expect, &outcome) {
            (Expect::Ok, Ok(_)) => true,
            (Expect::ErrContains(needle), Err(msg)) => msg.to_lowercase().contains(needle),
            _ => false,
        };
        if !pass {
            failures += 1;
        }
        let shown = match &outcome {
            Ok(v) => format!("ok {}", truncate(&v.to_string(), 300)),
            Err(e) => format!("err {}", truncate(e, 300)),
        };
        println!("[{}] {}\n       {shown}\n", if pass { "PASS" } else { "FAIL" }, case.name);
    }

    if failures == 0 {
        println!("R1 part 1: all {} cases behaved as expected.", 4);
        ExitCode::SUCCESS
    } else {
        println!("R1 part 1: {failures} case(s) did not behave as expected.");
        ExitCode::FAILURE
    }
}

/// Writes `empty.json` and `token_diff.json` payloads (plus `.hex` for the MPC
/// call) whose `signer_id` is the implicit account of the MPC-derived key.
fn prepare(public_key: &str, dir: &str) -> ExitCode {
    let pk = match parse_public_key(public_key) {
        Ok(pk) => pk,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let account = hex::encode(pk);
    for (name, intents) in [("empty", json!([])), ("token_diff", sample_token_diff())] {
        let payload = payload(&account, INTENTS, intents);
        std::fs::write(format!("{dir}/{name}.json"), &payload).expect("write payload");
        std::fs::write(format!("{dir}/{name}.hex"), hex::encode(payload.as_bytes()))
            .expect("write hex");
        println!("{name}: {} bytes -> {dir}/{name}.json", payload.len());
    }
    println!("signer (implicit account of MPC key): {account}");
    ExitCode::SUCCESS
}

/// Verifies an MPC response locally, then on mainnet via `simulate_intents`.
fn submit(public_key: &str, payload_file: &str, response: &str, expect: &str) -> ExitCode {
    let pk = parse_public_key(public_key).expect("public key");
    let payload = std::fs::read_to_string(payload_file).expect("read payload");
    let response: Value = serde_json::from_str(response).expect("MPC response is JSON");
    assert_eq!(response["scheme"], "Ed25519", "unexpected scheme in {response}");
    let sig: Vec<u8> = response["signature"]
        .as_array()
        .expect("signature byte array")
        .iter()
        .map(|b| b.as_u64().expect("byte") as u8)
        .collect();
    let sig: [u8; 64] = sig.try_into().expect("64-byte signature");

    let local_ok = ed25519_dalek::VerifyingKey::from_bytes(&pk)
        .expect("valid ed25519 point")
        .verify_strict(payload.as_bytes(), &ed25519_dalek::Signature::from_bytes(&sig))
        .is_ok();
    println!(
        "local ed25519 verify against derived key: {}",
        if local_ok { "ok" } else { "FAILED" }
    );

    let signed = json!({
        "standard": "raw_ed25519",
        "payload": payload,
        "public_key": format!("ed25519:{}", bs58::encode(pk).into_string()),
        "signature": format!("ed25519:{}", bs58::encode(sig).into_string()),
    });
    let outcome = simulate(DEFAULT_RPC, &signed);
    let pass = local_ok
        && match (&outcome, expect) {
            (Ok(_), "ok") => true,
            (Err(msg), needle) if needle != "ok" => msg.to_lowercase().contains(needle),
            _ => false,
        };
    match &outcome {
        Ok(v) => println!("mainnet simulate_intents: ok {}", truncate(&v.to_string(), 300)),
        Err(e) => println!("mainnet simulate_intents: err {}", truncate(e, 300)),
    }
    println!(
        "[{}] MPC-signed {payload_file} (expect: {expect})",
        if pass { "PASS" } else { "FAIL" }
    );
    if pass { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn parse_public_key(s: &str) -> Result<[u8; 32], String> {
    let b58 = s.strip_prefix("ed25519:").ok_or("public key must start with ed25519:")?;
    bs58::decode(b58)
        .into_vec()
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "public key must be 32 bytes".to_string())
}

fn sample_token_diff() -> Value {
    json!([{
        "intent": "token_diff",
        "diff": {
            "nep141:wrap.near": "-1000000000000000000000000",
            "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1": "2000000",
        },
    }])
}

/// The exact string that gets signed. The vault will build the same shape.
fn payload(signer: &str, verifying_contract: &str, intents: Value) -> String {
    let deadline = (chrono::Utc::now() + chrono::Duration::minutes(15))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let nonce: [u8; 32] = rand::random();
    json!({
        "signer_id": signer,
        "verifying_contract": verifying_contract,
        "deadline": deadline,
        "nonce": BASE64.encode(nonce),
        "intents": intents,
    })
    .to_string()
}

fn sign(key: &SigningKey, payload: &str) -> Value {
    let signature = key.sign(payload.as_bytes());
    json!({
        "standard": "raw_ed25519",
        "payload": payload,
        "public_key": format!("ed25519:{}", bs58::encode(key.verifying_key().as_bytes()).into_string()),
        "signature": format!("ed25519:{}", bs58::encode(signature.to_bytes()).into_string()),
    })
}

fn simulate(rpc: &str, signed: &Value) -> Result<Value, String> {
    let args = json!({ "signed": [signed] }).to_string();
    let body = json!({
        "jsonrpc": "2.0",
        "id": "r1",
        "method": "query",
        "params": {
            "request_type": "call_function",
            "finality": "final",
            "account_id": INTENTS,
            "method_name": "simulate_intents",
            "args_base64": BASE64.encode(args),
        },
    });
    let response: Value = ureq::post(rpc)
        .send_json(body)
        .map_err(|e| format!("rpc transport: {e}"))?
        .into_json()
        .map_err(|e| format!("rpc decode: {e}"))?;

    if let Some(error) = response.get("error") {
        return Err(error.to_string());
    }
    let result = &response["result"];
    if let Some(error) = result.get("error") {
        return Err(error.to_string());
    }
    let bytes: Vec<u8> = result["result"]
        .as_array()
        .ok_or_else(|| format!("unexpected response: {response}"))?
        .iter()
        .map(|b| b.as_u64().unwrap_or(0) as u8)
        .collect();
    serde_json::from_slice(&bytes).map_err(|e| format!("result decode: {e}"))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("{}…", &s[..max]) }
}
