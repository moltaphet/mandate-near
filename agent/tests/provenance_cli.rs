//! Runs the `--verify-model` command on real files. No network, no environment.

use std::path::{Path, PathBuf};
use std::process::Command;

use mandate_agent::model::ChatResponse;
use mandate_agent::provenance::ModelRecord;
use near_sdk::serde_json::{self, json};
use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
use sha3::{Digest, Keccak256};

const REQUEST_SHA256: &str = "3f2b8c1d9e0a7f6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a8b7c6d5e4f3a2b";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn key(label: &[u8]) -> SecretKey {
    SecretKey::from_slice(&Keccak256::digest(label)).expect("valid key")
}

fn address_of(key: &SecretKey) -> String {
    let public = PublicKey::from_secret_key(&Secp256k1::signing_only(), key);
    format!("0x{}", hex(&Keccak256::digest(&public.serialize_uncompressed()[1..])[12..]))
}

fn sign_personal(key: &SecretKey, text: &str) -> String {
    let mut hasher = Keccak256::new();
    hasher.update(b"\x19Ethereum Signed Message:\n");
    hasher.update(text.len().to_string().as_bytes());
    hasher.update(text.as_bytes());
    let digest: [u8; 32] = hasher.finalize().into();
    let message = Message::from_slice(&digest).expect("digest");
    let (recovery, compact) =
        Secp256k1::signing_only().sign_ecdsa_recoverable(&message, key).serialize_compact();
    format!("0x{}{:02x}", hex(&compact), 27 + recovery.to_i32())
}

/// A scratch directory unique to this test run.
fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("mandate-provenance-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn write_fixture(dir: &Path, signer: &SecretKey) -> (PathBuf, PathBuf) {
    let raw = serde_json::to_string(&json!({
        "id": "chat-cli",
        "model": "fixture-model",
        "choices": [{"message": {"role": "assistant", "content": "decision"}, "finish_reason": "stop"}],
    }))
    .unwrap();
    let response = ChatResponse::from_raw(raw, REQUEST_SHA256.into()).expect("envelope");
    let record = ModelRecord::from_response(&response);
    let text = format!("fixture-model:{}:{}", record.request_sha256, record.response_sha256);
    let signature = json!({
        "text": text,
        "signature": sign_personal(signer, &text),
        "signing_address": address_of(signer),
        "signing_algo": "ecdsa",
        "signature_kind": "provider_tee",
    });
    let record_path = dir.join("model-record.json");
    let signature_path = dir.join("signature.json");
    std::fs::write(&record_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    std::fs::write(&signature_path, serde_json::to_vec_pretty(&signature).unwrap()).unwrap();
    (record_path, signature_path)
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mandate-agent"))
        .arg("--verify-model")
        .args(args)
        .output()
        .expect("agent binary runs")
}

#[test]
fn verify_model_passes_for_the_pinned_signer_and_fails_otherwise() {
    let signer = key(b"mandate-test-signer-v1");
    let other = key(b"another-test-signer");
    let dir = scratch("pass");
    let (record, signature) = write_fixture(&dir, &signer);
    let record_text = record.to_str().unwrap();
    let signature_text = signature.to_str().unwrap();

    let ok = run(&[
        record_text,
        "--signature",
        signature_text,
        "--signing-address",
        &address_of(&signer),
    ]);
    assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stderr));
    let stdout = String::from_utf8_lossy(&ok.stdout);
    assert!(stdout.contains("\"signature\": \"VERIFIED\""), "{stdout}");

    let wrong = run(&[
        record_text,
        "--signature",
        signature_text,
        "--signing-address",
        &address_of(&other),
    ]);
    assert!(!wrong.status.success());
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("not made by the pinned address"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_model_without_a_signature_reports_not_checked_and_checks_the_reason_hash() {
    let signer = key(b"mandate-test-signer-v1");
    let dir = scratch("hashes");
    let (record, _) = write_fixture(&dir, &signer);
    let record_text = record.to_str().unwrap();
    let stored: ModelRecord = serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();

    let plain = run(&[record_text]);
    assert!(plain.status.success());
    assert!(String::from_utf8_lossy(&plain.stdout).contains("\"signature\": \"NOT_CHECKED\""));

    let matching = run(&[record_text, "--reason-hash", &stored.response_sha256]);
    assert!(matching.status.success());

    let mismatched = run(&[record_text, "--reason-hash", &"0".repeat(64)]);
    assert!(!mismatched.status.success());

    let half = run(&[record_text, "--signature", "x.json"]);
    assert!(!half.status.success(), "a signature needs its pinned address");

    let _ = std::fs::remove_dir_all(&dir);
}
