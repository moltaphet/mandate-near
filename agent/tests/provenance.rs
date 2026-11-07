//! Public-only offline provenance tests. The synthetic r=s=1 recovery vectors
//! were independently checked using eth_account; no signing scalar or operation.

use mandate_agent::Plan;
use mandate_agent::model::{ChatResponse, extract_decision, sha256_hex};
use mandate_agent::provenance::{
    CANONICAL_RESPONSE_VERSION, ModelRecord, ProvenanceError, SignatureResponse,
    VerificationStatus, canonical_response_bytes, canonical_response_hash, parse_signature_proof,
    verify_response, verify_signature,
};
use near_sdk::json_types::U128;
use near_sdk::serde_json::{self, Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/provenance-public-v1.json")).unwrap()
}

fn captured() -> ChatResponse {
    let fixture = fixture();
    ChatResponse::from_raw(
        fixture["raw_body"].as_str().unwrap().into(),
        fixture["request_body"].as_str().unwrap().into(),
    )
    .unwrap()
}

fn committed_plan(response: &ChatResponse) -> Plan {
    let mut plan = extract_decision(&response.content).unwrap().plan;
    plan.response_commitment = Some(canonical_response_hash(&response.raw_body).unwrap());
    plan
}

fn record() -> ModelRecord {
    let response = captured();
    ModelRecord::from_response(&response, &committed_plan(&response)).unwrap()
}

fn proof(name: &str) -> SignatureResponse {
    parse_signature_proof(fixture()[name].as_str().unwrap()).unwrap()
}

fn pin(name: &str) -> String {
    fixture()[name].as_str().unwrap().into()
}

#[test]
fn a_provider_tee_signature_over_the_exact_hashes_verifies() {
    let record = record();
    let fixture = fixture();
    assert!(fixture["synthetic_public_vector"].as_bool().unwrap());
    assert_eq!(record.request_sha256, fixture["request_sha256"]);
    assert_eq!(record.response_sha256, fixture["response_sha256"]);
    assert_eq!(record.reason_hash, fixture["canonical_reason_hash"]);
    assert_eq!(record.canonical_version, CANONICAL_RESPONSE_VERSION);
    assert_eq!(record.verification, "UNVERIFIED");
    assert_eq!(
        verify_signature(&record, &proof("signature_proof"), &pin("pinned_address")),
        Ok(())
    );
}

#[test]
fn gateway_and_legacy_documents_use_their_own_payloads() {
    let record = record();
    let gateway = proof("gateway_signature_proof");
    let gateway_pin = pin("gateway_pinned_address");
    assert_eq!(verify_signature(&record, &gateway, &gateway_pin), Ok(()));
    let mut legacy_two = gateway;
    legacy_two.signature_kind = None;
    assert_eq!(verify_signature(&record, &legacy_two, &gateway_pin), Ok(()));
    let mut legacy_three = proof("signature_proof");
    legacy_three.signature_kind = None;
    assert_eq!(verify_signature(&record, &legacy_three, &pin("pinned_address")), Ok(()));
}

#[test]
fn a_tampered_stored_response_fails_the_record_check() {
    let mut record = record();
    record.raw_body.push(' ');
    assert_eq!(
        verify_signature(&record, &proof("signature_proof"), &pin("pinned_address")),
        Err(ProvenanceError::RecordHashMismatch)
    );
}

#[test]
fn a_signature_over_another_response_does_not_verify() {
    let mut doc = proof("signature_proof");
    doc.text = format!("fixture-model:{}:{}", record().request_sha256, sha256_hex(b"other body"));
    assert_eq!(
        verify_signature(&record(), &doc, &pin("pinned_address")),
        Err(ProvenanceError::SignatureText)
    );
}

#[test]
fn a_signature_by_another_key_does_not_verify_against_the_pin() {
    // A distinct independently recovered public gateway signer is not the
    // provider pin. Lie in the reported address so rejection reaches recovery.
    let pinned = pin("pinned_address");
    let mut doc = proof("gateway_signature_proof");
    doc.signing_address = pinned.clone();
    assert_eq!(verify_signature(&record(), &doc, &pinned), Err(ProvenanceError::SignerMismatch));
}

#[test]
fn the_reported_signing_address_must_equal_the_pin() {
    let mut doc = proof("signature_proof");
    doc.signing_address = pin("gateway_pinned_address");
    assert_eq!(
        verify_signature(&record(), &doc, &pin("pinned_address")),
        Err(ProvenanceError::SignerMismatch)
    );
}

#[test]
fn a_flipped_signature_bit_or_bad_encoding_does_not_verify() {
    let mut doc = proof("signature_proof");
    let mut bytes = doc.signature.into_bytes();
    // Flip one public s bit (1 to 3): valid encoding, different recovered signer.
    bytes[129] = b'3';
    doc.signature = String::from_utf8(bytes).unwrap();
    assert_eq!(
        verify_signature(&record(), &doc, &pin("pinned_address")),
        Err(ProvenanceError::SignerMismatch)
    );
    doc.signature = "0x1234".into();
    assert_eq!(
        verify_signature(&record(), &doc, &pin("pinned_address")),
        Err(ProvenanceError::SignatureEncoding)
    );
}

#[test]
fn unsupported_algorithm_kind_or_pin_are_refused() {
    let mut doc = proof("signature_proof");
    doc.signing_algo = Some("eddsa".into());
    assert_eq!(
        verify_signature(&record(), &doc, &pin("pinned_address")),
        Err(ProvenanceError::SignatureAlgorithm)
    );
    doc.signing_algo = Some("ecdsa".into());
    doc.signature_kind = Some("mystery".into());
    assert_eq!(
        verify_signature(&record(), &doc, &pin("pinned_address")),
        Err(ProvenanceError::SignatureKind)
    );
    doc.signature_kind = Some("provider_tee".into());
    assert_eq!(
        verify_signature(&record(), &doc, "not-an-address"),
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
    let response = captured();
    let committed = committed_plan(&response);
    assert_eq!(committed.reason_hash(), fixture()["canonical_reason_hash"]);
    let from_file: Plan = serde_json::from_value(json!({
        "asset_in": "a", "asset_out": "b", "amount_in": "1", "min_amount_out": "1",
        "rationale": "reason", "response_commitment": committed.reason_hash(),
    }))
    .unwrap();
    assert_eq!(from_file.response_commitment, None);
    assert_eq!(from_file.reason_hash(), sha256_hex(b"reason"));
}

#[test]
fn canonical_json_sorts_every_object_and_preserves_strings_and_arrays() {
    let input = " {\"é\":2, \"z\":[{\"b\":2,\"a\":1},\"\\u00e9\\n\"],\"a\":\"  exact  \"} \n";
    let expected = "{\"a\":\"  exact  \",\"z\":[{\"a\":1,\"b\":2},\"é\\n\"],\"é\":2}";
    assert_eq!(canonical_response_bytes(input).unwrap(), expected.as_bytes());
    assert_eq!(canonical_response_hash(input).unwrap(), sha256_hex(expected.as_bytes()));
    assert_ne!(sha256_hex(input.as_bytes()), canonical_response_hash(input).unwrap());
    assert_eq!(
        canonical_response_bytes("{\"u\":18446744073709551615,\"s\":-9223372036854775808}")
            .unwrap(),
        b"{\"s\":-9223372036854775808,\"u\":18446744073709551615}"
    );
    assert_eq!(
        canonical_response_bytes("{\"😀\":1,\"\u{E000}\":2}").unwrap(),
        "{\"\u{E000}\":2,\"😀\":1}".as_bytes()
    );
}

#[test]
fn canonical_json_refuses_duplicates_at_every_depth_and_unsupported_numbers() {
    for input in [
        "{\"a\":1,\"a\":2}",
        "{\"a\":1,\"\\u0061\":2}",
        "{\"outer\":[{\"x\":1,\"x\":2}]}",
        "{\"n\":1.0}",
        "{\"n\":1e0}",
        "{\"n\":18446744073709551616}",
        "{\"n\":-9223372036854775809}",
        "{\"a\":1} trailing",
        "{\"a\":1}{\"b\":2}",
    ] {
        assert_eq!(canonical_response_bytes(input), Err(ProvenanceError::CanonicalJson), "{input}");
    }
    let oversized = format!("\"{}\"", "x".repeat(64 * 1024));
    assert_eq!(canonical_response_bytes(&oversized), Err(ProvenanceError::CanonicalJson));
    let too_deep = format!("{}0{}", "[".repeat(200), "]".repeat(200));
    assert_eq!(canonical_response_bytes(&too_deep), Err(ProvenanceError::CanonicalJson));
}

#[test]
fn reordered_response_has_the_same_canonical_commitment_but_different_wire_hash() {
    let response = captured();
    let raw: Value = serde_json::from_str(&response.raw_body).unwrap();
    let reordered = format!(
        "{{ \"model\":{}, \"id\":{}, \"choices\":{} }}\n",
        raw["model"], raw["id"], raw["choices"]
    );
    assert_ne!(sha256_hex(reordered.as_bytes()), response.response_sha256);
    assert_eq!(canonical_response_hash(&reordered).unwrap(), response.response_sha256);
    let alternate = ChatResponse::from_raw(reordered, response.request_body).unwrap();
    let alternate_record =
        ModelRecord::from_response(&alternate, &committed_plan(&alternate)).unwrap();
    assert_ne!(alternate_record.reason_hash, alternate_record.response_sha256);
    assert_eq!(alternate_record.reason_hash, fixture()["canonical_reason_hash"]);
    // Original proof authenticates original wire bytes, not equivalent rewritten bytes.
    assert_eq!(
        verify_signature(&alternate_record, &proof("signature_proof"), &pin("pinned_address")),
        Err(ProvenanceError::SignatureText)
    );
}

#[test]
fn record_checks_request_response_metadata_schema_and_exact_trade_binding() {
    let original = record();
    let mut changed = original.clone();
    changed.request_body.push(' ');
    assert_eq!(changed.check_hashes(), Err(ProvenanceError::RequestHashMismatch));
    changed = original.clone();
    changed.reason_hash = "00".repeat(32);
    assert_eq!(changed.check_hashes(), Err(ProvenanceError::CanonicalHashMismatch));
    changed = original.clone();
    changed.canonical_version = "unknown".into();
    assert_eq!(changed.check_hashes(), Err(ProvenanceError::CanonicalVersion));
    changed = original.clone();
    changed.model = "forged-model".into();
    assert_eq!(changed.check_hashes(), Err(ProvenanceError::DecisionBinding));
    changed = original.clone();
    changed.response_id = "forged-id".into();
    assert_eq!(changed.check_hashes(), Err(ProvenanceError::DecisionBinding));
    changed = original.clone();
    changed.plan.amount_in = "20000001".into();
    assert_eq!(changed.check_hashes(), Err(ProvenanceError::DecisionBinding));
    let response = captured();
    let mut plan = committed_plan(&response);
    plan.min_amount_out = U128(1);
    assert_eq!(ModelRecord::from_response(&response, &plan), Err(ProvenanceError::DecisionBinding));
    let mut forged_capture = captured();
    forged_capture.response_sha256 = "00".repeat(32);
    assert_eq!(
        ModelRecord::from_response(&forged_capture, &committed_plan(&forged_capture)),
        Err(ProvenanceError::RecordHashMismatch)
    );
    changed = original;
    let mut raw: Value = serde_json::from_str(&changed.raw_body).unwrap();
    raw["choices"][0]["message"]["content"] = json!("decision");
    changed.raw_body = serde_json::to_string(&raw).unwrap();
    changed.response_sha256 = sha256_hex(changed.raw_body.as_bytes());
    changed.reason_hash = canonical_response_hash(&changed.raw_body).unwrap();
    assert_eq!(changed.check_hashes(), Err(ProvenanceError::DecisionBinding));
    let mut changed = record();
    let mut raw: Value = serde_json::from_str(&changed.raw_body).unwrap();
    let mut decision: Value =
        serde_json::from_str(raw["choices"][0]["message"]["content"].as_str().unwrap()).unwrap();
    decision["no_trade"] = json!(true);
    raw["choices"][0]["message"]["content"] = json!(serde_json::to_string(&decision).unwrap());
    changed.raw_body = serde_json::to_string(&raw).unwrap();
    changed.response_sha256 = sha256_hex(changed.raw_body.as_bytes());
    changed.reason_hash = canonical_response_hash(&changed.raw_body).unwrap();
    assert_eq!(changed.check_hashes(), Err(ProvenanceError::DecisionBinding));
}

#[test]
fn signed_capture_policy_never_downgrades_invalid_or_missing_required_proof() {
    let mut response = captured();
    assert_eq!(verify_response(&response, None, false), Ok(VerificationStatus::Unverified));
    assert_eq!(verify_response(&response, None, true), Err(ProvenanceError::MissingSignature));
    response.signature_proof = Some(fixture()["signature_proof"].as_str().unwrap().into());
    assert_eq!(verify_response(&response, None, false), Err(ProvenanceError::MissingPin));
    assert_eq!(
        verify_response(&response, Some(&pin("pinned_address")), true),
        Ok(VerificationStatus::VerifiedPinnedSigner)
    );
    response.signature_proof = Some("{\"signature\":\"bad\"}".into());
    assert_eq!(
        verify_response(&response, Some(&pin("pinned_address")), false),
        Err(ProvenanceError::MalformedSignature)
    );
    assert_eq!(
        parse_signature_proof("{\"signature\":\"a\",\"signature\":\"b\"}"),
        Err(ProvenanceError::MalformedSignature)
    );
}

#[test]
fn public_flat_proof_refuses_extra_actual_proof_containers_at_every_depth() {
    let original: Value =
        serde_json::from_str(fixture()["signature_proof"].as_str().unwrap()).unwrap();
    for name in ["signatures", "proof", "signature_response", "chat_signature"] {
        let mut extra = original.clone();
        extra[name] = json!({"signature": "invalid"});
        assert_eq!(
            parse_signature_proof(&serde_json::to_string(&extra).unwrap()),
            Err(ProvenanceError::MalformedSignature)
        );
        let mut nested = original.clone();
        nested["metadata"] = json!([{"nested": {(name): {"signature": "invalid"}}}]);
        assert_eq!(
            parse_signature_proof(&serde_json::to_string(&nested).unwrap()),
            Err(ProvenanceError::MalformedSignature)
        );
    }
    let mut nested_signature = original.clone();
    nested_signature["metadata"] = json!({"signature": "invalid"});
    assert_eq!(
        parse_signature_proof(&serde_json::to_string(&nested_signature).unwrap()),
        Err(ProvenanceError::MalformedSignature)
    );
    let mut container_signature = original.clone();
    container_signature["signature"] = json!({"value": "invalid"});
    assert_eq!(
        parse_signature_proof(&serde_json::to_string(&container_signature).unwrap()),
        Err(ProvenanceError::MalformedSignature)
    );
    // Supported inline evidence retains ordinary complete chat envelope fields.
    let raw: Value = serde_json::from_str(fixture()["raw_body"].as_str().unwrap()).unwrap();
    let mut inline = original;
    for (name, value) in raw.as_object().unwrap() {
        inline[name] = value.clone();
    }
    assert_eq!(
        parse_signature_proof(&serde_json::to_string(&inline).unwrap()).unwrap(),
        proof("signature_proof")
    );
}
