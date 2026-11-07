//! Offline command tests using independent public recovery vectors and valid
//! strict trade decisions. No provider/network/authentication or signing scalar.

use std::path::{Path, PathBuf};
use std::process::Command;

use mandate_agent::model::{ChatResponse, extract_decision, sha256_hex};
use mandate_agent::provenance::{ModelRecord, canonical_response_hash};
use near_sdk::serde_json::{self, Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/provenance-public-v1.json")).unwrap()
}

fn captured_record(signed: bool) -> ModelRecord {
    let fixture = fixture();
    let mut response = ChatResponse::from_raw(
        fixture["raw_body"].as_str().unwrap().into(),
        fixture["request_body"].as_str().unwrap().into(),
    )
    .unwrap();
    if signed {
        response.signature_proof = Some(fixture["signature_proof"].as_str().unwrap().into());
    }
    let mut plan = extract_decision(&response.content).unwrap().plan;
    plan.response_commitment = Some(canonical_response_hash(&response.raw_body).unwrap());
    ModelRecord::from_response(&response, &plan).unwrap()
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("mandate-provenance-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn record(&self, record: &ModelRecord) -> PathBuf {
        self.write("model-record.json", &serde_json::to_vec_pretty(record).unwrap())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(path: &Path, flags: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mandate-agent"))
        .arg("--verify-model")
        .arg(path)
        .args(flags)
        .output()
        .unwrap()
}

fn success(output: &std::process::Output) -> Value {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

fn refusal(output: &std::process::Output, expected: &str) {
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(expected), "expected {expected:?}; got {stderr}");
}

#[test]
fn verify_model_passes_for_the_pinned_signer_and_fails_otherwise() {
    let scratch = Scratch::new("public-signed");
    let record = captured_record(false);
    let record_path = scratch.record(&record);
    let fixture = fixture();
    let signature =
        scratch.write("signature.json", fixture["signature_proof"].as_str().unwrap().as_bytes());
    let pinned = fixture["pinned_address"].as_str().unwrap();
    let output = success(&run(
        &record_path,
        &[
            "--signature",
            signature.to_str().unwrap(),
            "--signing-address",
            pinned,
            "--reason-hash",
            &record.reason_hash,
        ],
    ));
    assert_eq!(output["signature"], "VERIFIED_PINNED_SIGNER");
    assert_eq!(output["verification"], "VERIFIED_PINNED_SIGNER");
    assert_eq!(output["record"], "hashes_and_decision_match");
    assert_eq!(output["reason_hash"], record.reason_hash);
    assert_eq!(output["chain_authenticated"], false);
    assert_eq!(output["settlement_verified"], false);
    assert_eq!(output["hardware_attestation_verified"], false);
    let wrong = run(
        &record_path,
        &[
            "--signature",
            signature.to_str().unwrap(),
            "--signing-address",
            fixture["gateway_pinned_address"].as_str().unwrap(),
        ],
    );
    refusal(&wrong, "not made by the pinned address");
}

#[test]
fn verify_model_without_a_signature_reports_not_checked_and_checks_the_reason_hash() {
    let scratch = Scratch::new("public-unsigned");
    let record = captured_record(false);
    let path = scratch.record(&record);
    let plain = success(&run(&path, &[]));
    assert_eq!(plain["verification"], "UNVERIFIED");
    assert_eq!(plain["signature"], "UNVERIFIED");
    assert_eq!(plain["reason_hash"], record.reason_hash);
    success(&run(&path, &["--reason-hash", &record.reason_hash]));
    refusal(&run(&path, &["--reason-hash", &"00".repeat(32)]), "supplied reason hash");
    let fixture = fixture();
    let proof =
        scratch.write("signature.json", fixture["signature_proof"].as_str().unwrap().as_bytes());
    refusal(&run(&path, &["--signature", proof.to_str().unwrap()]), "external signing address");
}

#[test]
fn stored_signature_is_always_checked_and_cannot_be_downgraded_to_unsigned() {
    let scratch = Scratch::new("embedded-proof");
    let mut record = captured_record(true);
    let path = scratch.record(&record);
    refusal(&run(&path, &[]), "external signing address");
    let fixture = fixture();
    let pinned = fixture["pinned_address"].as_str().unwrap();
    let output = success(&run(&path, &["--signing-address", pinned]));
    assert_eq!(output["verification"], "VERIFIED_PINNED_SIGNER");
    record.signature_proof = Some("{\"signature\":\"bad\"}".into());
    let path = scratch.record(&record);
    refusal(&run(&path, &["--signing-address", pinned]), "signature document is malformed");
    let external =
        scratch.write("signature.json", fixture["signature_proof"].as_str().unwrap().as_bytes());
    refusal(
        &run(&path, &["--signature", external.to_str().unwrap(), "--signing-address", pinned]),
        "signature document is malformed",
    );
}

#[test]
fn verifier_refuses_tampered_wire_hashes_metadata_and_decision() {
    let scratch = Scratch::new("tampered-record");
    let original = captured_record(false);
    let mut changed = original.clone();
    changed.raw_body.push(' ');
    refusal(&run(&scratch.record(&changed), &[]), "stored response bytes");
    changed = original.clone();
    changed.request_sha256 = "00".repeat(32);
    refusal(&run(&scratch.record(&changed), &[]), "stored request bytes");
    changed = original.clone();
    changed.model = "fabricated-model".into();
    refusal(&run(&scratch.record(&changed), &[]), "stored decision does not match");
    changed = original.clone();
    changed.response_id = "fabricated-response".into();
    refusal(&run(&scratch.record(&changed), &[]), "stored decision does not match");
    changed = original.clone();
    changed.plan.amount_in = "1".into();
    refusal(&run(&scratch.record(&changed), &[]), "stored decision does not match");
    changed = original;
    let mut body: Value = serde_json::from_str(&changed.raw_body).unwrap();
    body["choices"][0]["message"]["content"] = json!("not a decision");
    changed.raw_body = serde_json::to_string(&body).unwrap();
    changed.response_sha256 = sha256_hex(changed.raw_body.as_bytes());
    changed.reason_hash = canonical_response_hash(&changed.raw_body).unwrap();
    refusal(&run(&scratch.record(&changed), &[]), "stored decision does not match");
}

#[test]
fn verifier_checks_public_plan_tuple_and_refuses_duplicate_flags() {
    let scratch = Scratch::new("public-plan");
    let record = captured_record(false);
    let path = scratch.record(&record);
    let mut plan = serde_json::to_value(&record.plan).unwrap();
    let plan_path = scratch.write("plan.json", &serde_json::to_vec(&plan).unwrap());
    success(&run(&path, &["--plan", plan_path.to_str().unwrap()]));
    plan["min_amount_out"] = json!("1");
    let plan_path = scratch.write("plan.json", &serde_json::to_vec(&plan).unwrap());
    refusal(
        &run(&path, &["--plan", plan_path.to_str().unwrap()]),
        "stored decision does not match",
    );
    refusal(
        &run(&path, &["--reason-hash", &record.reason_hash, "--reason-hash", &record.reason_hash]),
        "usage:",
    );
    refusal(&run(&path, &["--unknown", "value"]), "usage:");
}

#[test]
fn verifier_refuses_duplicate_record_fields_prohibited_paths_and_oversized_files() {
    let scratch = Scratch::new("bounded-input");
    let record = captured_record(false);
    let json = serde_json::to_string(&record).unwrap();
    let duplicate = format!("{{\"model\":\"forged\",{}", &json[1..]);
    let path = scratch.write("duplicate.json", duplicate.as_bytes());
    refusal(&run(&path, &[]), "model record is malformed");
    // This prohibited path is intentionally absent; it must be rejected before
    // any attempt to resolve or open a file. No protected file is created.
    refusal(&run(&scratch.0.join(".env"), &[]), "refused prohibited public-file path");
    let path = scratch.write("oversized.json", &vec![b' '; 1024 * 1024 + 1]);
    refusal(&run(&path, &[]), "public file exceeds size limit");
}

#[cfg(unix)]
#[test]
fn verifier_resolves_allowed_symlinks_before_reading_the_public_record() {
    let scratch = Scratch::new("public-symlink");
    let path = scratch.record(&captured_record(false));
    let link = scratch.0.join("model-link.json");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    let output = success(&run(&link, &[]));
    assert_eq!(output["verification"], "UNVERIFIED");
}
