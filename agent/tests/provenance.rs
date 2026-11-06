//! Offline provenance tests. A test key is derived from a label, so the repository
//! holds no key material, and nothing here touches the network.

use mandate_agent::Plan;
use mandate_agent::model::{ChatResponse, sha256_hex};
use mandate_agent::provenance::{
    ModelRecord, ProvenanceError, SignatureResponse, verify_signature,
};
use near_sdk::json_types::U128;
use near_sdk::serde_json::{self, json};
use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
use sha3::{Digest, Keccak256};

const REQUEST_SHA256: &str = "3f2b8c1d9e0a7f6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a8b7c6d5e4f3a2b";
const MODEL: &str = "fixture-model";

fn test_key() -> SecretKey {
    SecretKey::from_slice(&Keccak256::digest(b"mandate-test-signer-v1")).expect("valid key")
}

/// The 20-byte Ethereum-style address of the test key, as `0x…`.
fn address_of(key: &SecretKey) -> String {
    let public = PublicKey::from_secret_key(&Secp256k1::signing_only(), key);
    let hash = Keccak256::digest(&public.serialize_uncompressed()[1..]);
    format!("0x{}", hex(&hash[12..]))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Signs `text` as an Ethereum personal message and returns `0x` + r || s || v.
fn sign_personal(key: &SecretKey, text: &str) -> String {
    let mut hasher = Keccak256::new();
    hasher.update(b"\x19Ethereum Signed Message:\n");
    hasher.update(text.len().to_string().as_bytes());
    hasher.update(text.as_bytes());
    let digest: [u8; 32] = hasher.finalize().into();
    let message = Message::from_slice(&digest).expect("32-byte digest");
    let (recovery, compact) =
        Secp256k1::signing_only().sign_ecdsa_recoverable(&message, key).serialize_compact();
    format!("0x{}{:02x}", hex(&compact), 27 + recovery.to_i32())
}

fn raw_envelope(content: &str) -> String {
    serde_json::to_string(&json!({
        "id": "chat-1",
        "model": MODEL,
        "choices": [{"message": {"role": "assistant", "content": content}, "finish_reason": "stop"}],
    }))
    .unwrap()
}

fn record_from(raw: &str) -> ModelRecord {
    let response =
        ChatResponse::from_raw(raw.to_string(), REQUEST_SHA256.into()).expect("envelope");
    ModelRecord::from_response(&response)
}

fn signature_doc(
    text: &str,
    signature: &str,
    address: &str,
    kind: Option<&str>,
) -> SignatureResponse {
    SignatureResponse {
        text: text.into(),
        signature: signature.into(),
        signing_address: address.into(),
        signing_algo: Some("ecdsa".into()),
        signature_kind: kind.map(str::to_string),
    }
}

#[test]
fn a_provider_tee_signature_over_the_exact_hashes_verifies() {
    let key = test_key();
    let address = address_of(&key);
    let record = record_from(&raw_envelope("decision"));
    let text = format!("{MODEL}:{}:{}", record.request_sha256, record.response_sha256);
    let signature = sign_personal(&key, &text);
    let doc = signature_doc(&text, &signature, &address, Some("provider_tee"));
    assert_eq!(verify_signature(&record, &doc, &address), Ok(()));
}

#[test]
fn gateway_and_legacy_documents_use_their_own_payloads() {
    let key = test_key();
    let address = address_of(&key);
    let record = record_from(&raw_envelope("decision"));
    let gateway_text = format!("{}:{}", record.request_sha256, record.response_sha256);
    let gateway_sig = sign_personal(&key, &gateway_text);
    let gateway = signature_doc(&gateway_text, &gateway_sig, &address, Some("gateway"));
    assert_eq!(verify_signature(&record, &gateway, &address), Ok(()));
    // Legacy: no kind, two fields means gateway.
    let legacy_two = signature_doc(&gateway_text, &gateway_sig, &address, None);
    assert_eq!(verify_signature(&record, &legacy_two, &address), Ok(()));
    // Legacy: no kind, three fields means provider_tee.
    let three = format!("{MODEL}:{}:{}", record.request_sha256, record.response_sha256);
    let three_sig = sign_personal(&key, &three);
    let legacy_three = signature_doc(&three, &three_sig, &address, None);
    assert_eq!(verify_signature(&record, &legacy_three, &address), Ok(()));
}

#[test]
fn a_tampered_stored_response_fails_the_record_check() {
    let key = test_key();
    let address = address_of(&key);
    let mut record = record_from(&raw_envelope("decision"));
    let text = format!("{MODEL}:{}:{}", record.request_sha256, record.response_sha256);
    let doc = signature_doc(&text, &sign_personal(&key, &text), &address, Some("provider_tee"));
    record.raw_body.push(' ');
    assert_eq!(verify_signature(&record, &doc, &address), Err(ProvenanceError::RecordHashMismatch));
}

#[test]
fn a_signature_over_another_response_does_not_verify() {
    let key = test_key();
    let address = address_of(&key);
    let record = record_from(&raw_envelope("decision"));
    let other = format!("{MODEL}:{}:{}", record.request_sha256, sha256_hex(b"other body"));
    let doc = signature_doc(&other, &sign_personal(&key, &other), &address, Some("provider_tee"));
    assert_eq!(verify_signature(&record, &doc, &address), Err(ProvenanceError::SignatureText));
}

#[test]
fn a_signature_by_another_key_does_not_verify_against_the_pin() {
    let key = test_key();
    let other_key = SecretKey::from_slice(&Keccak256::digest(b"another-test-signer")).unwrap();
    let pinned = address_of(&key);
    let record = record_from(&raw_envelope("decision"));
    let text = format!("{MODEL}:{}:{}", record.request_sha256, record.response_sha256);
    let doc = signature_doc(
        &text,
        &sign_personal(&other_key, &text),
        &address_of(&other_key),
        Some("provider_tee"),
    );
    assert_eq!(verify_signature(&record, &doc, &pinned), Err(ProvenanceError::SignerMismatch));
}

#[test]
fn the_reported_signing_address_must_equal_the_pin() {
    let key = test_key();
    let address = address_of(&key);
    let record = record_from(&raw_envelope("decision"));
    let text = format!("{MODEL}:{}:{}", record.request_sha256, record.response_sha256);
    let wrong_reported = "0x0000000000000000000000000000000000000001";
    let doc =
        signature_doc(&text, &sign_personal(&key, &text), wrong_reported, Some("provider_tee"));
    assert_eq!(verify_signature(&record, &doc, &address), Err(ProvenanceError::SignerMismatch));
}

#[test]
fn a_flipped_signature_bit_or_bad_encoding_does_not_verify() {
    let key = test_key();
    let address = address_of(&key);
    let record = record_from(&raw_envelope("decision"));
    let text = format!("{MODEL}:{}:{}", record.request_sha256, record.response_sha256);
    let good = sign_personal(&key, &text);
    let mut bytes = good.clone().into_bytes();
    // Flip one hex digit inside s (chars 2 + 64 .. 2 + 128).
    let index = 2 + 70;
    bytes[index] = if bytes[index] == b'0' { b'1' } else { b'0' };
    let flipped = String::from_utf8(bytes).unwrap();
    let doc = signature_doc(&text, &flipped, &address, Some("provider_tee"));
    assert!(verify_signature(&record, &doc, &address).is_err());

    let short = signature_doc(&text, "0x1234", &address, Some("provider_tee"));
    assert_eq!(
        verify_signature(&record, &short, &address),
        Err(ProvenanceError::SignatureEncoding)
    );
}

#[test]
fn unsupported_algorithm_kind_or_pin_are_refused() {
    let key = test_key();
    let address = address_of(&key);
    let record = record_from(&raw_envelope("decision"));
    let text = format!("{MODEL}:{}:{}", record.request_sha256, record.response_sha256);
    let signature = sign_personal(&key, &text);

    let mut algo = signature_doc(&text, &signature, &address, Some("provider_tee"));
    algo.signing_algo = Some("eddsa".into());
    assert_eq!(
        verify_signature(&record, &algo, &address),
        Err(ProvenanceError::SignatureAlgorithm)
    );

    let unknown = signature_doc(&text, &signature, &address, Some("mystery"));
    assert_eq!(verify_signature(&record, &unknown, &address), Err(ProvenanceError::SignatureKind));

    let doc = signature_doc(&text, &signature, &address, Some("provider_tee"));
    assert_eq!(
        verify_signature(&record, &doc, "not-an-address"),
        Err(ProvenanceError::MalformedAddress)
    );
}

#[test]
fn the_commitment_becomes_the_reason_hash_and_plan_files_cannot_set_it() {
    let plan = Plan {
        asset_in: "a".into(),
        asset_out: "b".into(),
        amount_in: U128(1),
        min_amount_out: U128(1),
        rationale: "reason".into(),
        response_commitment: None,
    };
    assert_eq!(plan.reason_hash(), sha256_hex(b"reason"));

    let committed = Plan { response_commitment: Some("ab".repeat(32)), ..plan.clone() };
    assert_eq!(committed.reason_hash(), "ab".repeat(32));

    // A plan file that tries to choose the commitment is ignored: the field is skipped.
    let from_file: Plan = serde_json::from_value(json!({
        "asset_in": "a",
        "asset_out": "b",
        "amount_in": "1",
        "min_amount_out": "1",
        "rationale": "reason",
        "response_commitment": "cd".repeat(32),
    }))
    .unwrap();
    assert_eq!(from_file.response_commitment, None);
    assert_eq!(from_file.reason_hash(), sha256_hex(b"reason"));
}
