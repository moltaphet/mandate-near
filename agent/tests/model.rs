//! Offline model tests: every request uses an in-memory fake, with no provider
//! configuration, environment access, HTTP server or account signing material.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mandate_agent::Plan;
use mandate_agent::autonomous::{self, AutonomousConfig, Snapshot};
use mandate_agent::decision::{Decision, DecisionSource, ManualDecision, RebalanceDecision};
use mandate_agent::model::{
    ChatClient, ChatFailure, ChatResponse, MAX_CHAT_RESPONSE_BYTES, ModelDecision,
    VerificationPolicy, compact_snapshot, extract_decision, optional_model_setting,
    response_contains_marker, sha256_hex, signature_lookup_url, validate_public_request_body,
};
use mandate_agent::provenance::{VerificationStatus, canonical_response_hash, verify_response};
use mandate_agent::relay::Relay;
use mandate_vault::engine::{Market, TradingState};
use mandate_vault::math::Price;
use mandate_vault::{AssetConfig, Mandate};
use near_sdk::json_types::{U64, U128};
use near_sdk::serde_json::{self, Value, json};

const A: &str = "nep141:stable.testnet";
const B: &str = "nep141:wrap.testnet";
const NOW: u64 = 1_800_000_000;
const NEAR: u128 = 10u128.pow(24);

fn snapshot() -> Snapshot {
    Snapshot {
        mandate: Mandate {
            assets: vec![
                AssetConfig { asset_id: A.into(), decimals: 6, price_feed_id: "stable-usd".into() },
                AssetConfig { asset_id: B.into(), decimals: 24, price_feed_id: "near-usd".into() },
            ],
            stable_assets: vec![A.into()],
            max_trade_usd: U64(30_000_000),
            max_daily_turnover_usd: U64(60_000_000),
            max_position_bps: 6000,
            max_slippage_bps: 200,
            max_drawdown_bps: 1000,
            min_trade_interval_sec: 60,
            max_price_age_sec: 120,
            allowed_model_ids: vec![
                "manual-v1".into(),
                "rebalance-v1".into(),
                "fixture-model".into(),
            ],
        },
        trading: TradingState {
            last_trade_at_sec: None,
            day_start_sec: NOW - NOW % 86_400,
            day_turnover_usd: 0,
            high_water_mark_usd: 125_000_000,
            signed_intent_expires_at_sec: 0,
        },
        market: Market {
            now_sec: NOW,
            prices: BTreeMap::from([
                (A.into(), Price { mantissa: U64(100_000_000), expo: -8, publish_time_sec: NOW }),
                (B.into(), Price { mantissa: U64(500_000_000), expo: -8, publish_time_sec: NOW }),
            ]),
            balances: BTreeMap::from([(A.into(), 100_000_000), (B.into(), 5 * NEAR)]),
        },
        block_hash: bs58::encode([1u8; 32]).into_string(),
        intents_id: "intents.testnet".into(),
        agent_id: Some("agent.testnet".into()),
        not_ready: None,
    }
}

fn body() -> Value {
    json!({
        "asset_in":A,
        "asset_out":B,
        "amount_in":"20000000",
        "min_amount_out":(4 * NEAR).to_string(),
        "rationale":"Rebalance within the owner-defined limits.",
        "no_trade":false,
        "no_trade_reason":null
    })
}

fn response(content: &str) -> ChatResponse {
    ChatResponse::from_raw(
        serde_json::to_string(&json!({
            "id":"fixture-chat-1",
            "model":"fixture-model",
            "choices":[{"message":{"role":"assistant","content":content},"finish_reason":"stop"}],
            "usage":{"total_tokens":100},
            "attestation":{"status":"fixture-unverified"}
        }))
        .unwrap(),
        FIXTURE_REQUEST_BODY.into(),
    )
    .unwrap()
}

/// Exact public request bytes; these fixtures never send this request.
const FIXTURE_REQUEST_BODY: &str = r#"{"model":"fixture-model","messages":[{"role":"user","content":"Public fixture snapshot."}]}"#;

#[derive(Clone)]
struct FakeClient {
    answer: Result<ChatResponse, ChatFailure>,
    calls: Arc<AtomicUsize>,
    prompts: Arc<Mutex<Vec<String>>>,
    policy: VerificationPolicy,
}

impl FakeClient {
    fn answer(answer: Result<ChatResponse, ChatFailure>) -> Self {
        Self {
            answer,
            calls: Arc::new(AtomicUsize::new(0)),
            prompts: Arc::new(Mutex::new(vec![])),
            policy: VerificationPolicy::default(),
        }
    }

    fn content(content: &str) -> Self {
        Self::answer(Ok(response(content)))
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn with_policy(mut self, policy: VerificationPolicy) -> Self {
        self.policy = policy;
        self
    }
}

impl ChatClient for FakeClient {
    fn requested_model(&self) -> &str {
        "fixture-model"
    }
    fn verification_policy(&self) -> VerificationPolicy {
        self.policy.clone()
    }
    async fn complete(&self, prompt: String) -> Result<ChatResponse, ChatFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.prompts.lock().unwrap().push(prompt);
        self.answer.clone()
    }
}

fn refusal(decision: Decision) -> String {
    match decision {
        Decision::NoTrade { reason } => reason,
        Decision::Trade { .. } => panic!("an invalid decision must not become a trade"),
    }
}

#[tokio::test]
async fn valid_model_json_preserves_exact_plan_and_raw_unverified_response() {
    let content = body().to_string();
    let original = response(&content);
    let wire = original.raw_body.clone();
    let fake = FakeClient::answer(Ok(original));
    let Decision::Trade { plan, response: Some(captured) } =
        ModelDecision::new(fake.clone()).decide(&snapshot()).await
    else {
        panic!("valid exact amounts must pass the unchanged local engine")
    };
    assert_eq!(plan.asset_in, A);
    assert_eq!(plan.asset_out, B);
    assert_eq!(plan.amount_in.0, 20_000_000);
    assert_eq!(plan.min_amount_out.0, 4 * NEAR);
    assert_eq!(plan.rationale, body()["rationale"]);
    assert_eq!(captured.id, "fixture-chat-1");
    assert_eq!(captured.model, "fixture-model");
    assert_eq!(captured.raw_body, wire);
    assert_eq!(captured.content, content);
    assert_eq!(captured.request_body, FIXTURE_REQUEST_BODY);
    assert_eq!(captured.request_sha256, sha256_hex(FIXTURE_REQUEST_BODY.as_bytes()));
    assert_eq!(captured.response_sha256, sha256_hex(wire.as_bytes()));
    assert_eq!(plan.reason_hash(), canonical_response_hash(&wire).unwrap());
    assert_eq!(verify_response(&captured, None, false).unwrap(), VerificationStatus::Unverified);
    assert_eq!(fake.calls(), 1);
}

#[tokio::test]
async fn malformed_and_fenced_json_fail_without_a_repair_request() {
    for content in ["not json", "```json\n{}\n```", "{} trailing text"] {
        let fake = FakeClient::content(content);
        let reason = refusal(ModelDecision::new(fake.clone()).decide(&snapshot()).await);
        assert!(reason.contains("exact JSON schema"));
        assert_eq!(fake.calls(), 1);
    }
}

#[tokio::test]
async fn every_field_is_required_including_explicit_nullable_reason() {
    for field in [
        "asset_in",
        "asset_out",
        "amount_in",
        "min_amount_out",
        "rationale",
        "no_trade",
        "no_trade_reason",
    ] {
        let mut missing = body();
        missing.as_object_mut().unwrap().remove(field);
        let fake = FakeClient::content(&missing.to_string());
        let reason = refusal(ModelDecision::new(fake.clone()).decide(&snapshot()).await);
        assert!(reason.contains("exact JSON schema"), "missing {field}: {reason}");
        assert_eq!(fake.calls(), 1);
    }
}

#[tokio::test]
async fn unknown_and_duplicate_fields_are_rejected_instead_of_overwritten() {
    let mut extra = body();
    extra["replacement_mandate"] = json!({"max_trade_usd":"999999999"});
    let duplicate = format!(
        "{},\"rationale\":\"second rationale\"}}",
        body().to_string().trim_end_matches('}')
    );
    for content in [extra.to_string(), duplicate] {
        let reason =
            refusal(ModelDecision::new(FakeClient::content(&content)).decide(&snapshot()).await);
        assert!(reason.contains("exact JSON schema"));
    }
}

#[tokio::test]
async fn wrong_json_types_do_not_get_coerced() {
    for (field, value) in [
        ("asset_in", json!(1)),
        ("amount_in", json!(20_000_000)),
        ("rationale", Value::Null),
        ("no_trade", json!("false")),
        ("no_trade_reason", json!(false)),
    ] {
        let mut changed = body();
        changed[field] = value;
        assert!(
            refusal(
                ModelDecision::new(FakeClient::content(&changed.to_string()))
                    .decide(&snapshot())
                    .await
            )
            .contains("exact JSON schema")
        );
    }
}

#[tokio::test]
async fn only_distinct_mandate_assets_are_accepted() {
    for (value, expected) in [
        (A, "model decision assets must differ"),
        ("nep141:outside.testnet", "model decision asset is outside the mandate"),
    ] {
        let mut changed = body();
        changed["asset_out"] = json!(value);
        let reason = refusal(
            ModelDecision::new(FakeClient::content(&changed.to_string())).decide(&snapshot()).await,
        );
        assert_eq!(reason, expected);
    }
}

#[tokio::test]
async fn amounts_are_exact_positive_u128_decimal_strings() {
    for field in ["amount_in", "min_amount_out"] {
        for value in [
            "0",
            "-1",
            "+1",
            "01",
            "1.0",
            "1e6",
            " 1",
            "1 ",
            "",
            "340282366920938463463374607431768211456",
        ] {
            let mut changed = body();
            changed[field] = json!(value);
            let reason = refusal(
                ModelDecision::new(FakeClient::content(&changed.to_string()))
                    .decide(&snapshot())
                    .await,
            );
            assert!(reason.contains("positive u128 decimal integer"), "{field}={value}: {reason}");
        }
    }
    let mut largest = body();
    largest["amount_in"] = json!(u128::MAX.to_string());
    let reason = refusal(
        ModelDecision::new(FakeClient::content(&largest.to_string())).decide(&snapshot()).await,
    );
    assert!(
        reason.contains("InsufficientBalance"),
        "u128::MAX must parse exactly before the balance gate: {reason}"
    );
}

#[tokio::test]
async fn rationale_bound_counts_unicode_characters_and_preserves_text() {
    let mut changed = body();
    changed["rationale"] = json!("🧠".repeat(800));
    let Decision::Trade { plan, .. } =
        ModelDecision::new(FakeClient::content(&changed.to_string())).decide(&snapshot()).await
    else {
        panic!("800 Unicode characters must pass independently of UTF-8 byte length")
    };
    assert_eq!(plan.rationale, "🧠".repeat(800));
    for rationale in [String::new(), " \n\t".into(), "🧠".repeat(801)] {
        changed["rationale"] = json!(rationale);
        assert!(
            refusal(
                ModelDecision::new(FakeClient::content(&changed.to_string()))
                    .decide(&snapshot())
                    .await
            )
            .contains("1 to 800 Unicode characters")
        );
    }
}

#[tokio::test]
async fn explicit_no_trade_stops_after_one_call_with_a_bounded_reason() {
    let mut changed = body();
    changed["no_trade"] = json!(true);
    changed["no_trade_reason"] = json!("Wait for a fresh market signal.");
    let fake = FakeClient::content(&changed.to_string());
    assert_eq!(
        refusal(ModelDecision::new(fake.clone()).decide(&snapshot()).await),
        "Wait for a fresh market signal."
    );
    assert_eq!(fake.calls(), 1);
}

#[tokio::test]
async fn no_trade_still_requires_valid_assets_amounts_and_rationale() {
    for (field, value) in [
        ("asset_in", json!("nep141:outside.testnet")),
        ("amount_in", json!("0")),
        ("rationale", json!("")),
    ] {
        let mut changed = body();
        changed["no_trade"] = json!(true);
        changed["no_trade_reason"] = json!("Explicit no trade.");
        changed[field] = value;
        assert_ne!(
            refusal(
                ModelDecision::new(FakeClient::content(&changed.to_string()))
                    .decide(&snapshot())
                    .await
            ),
            "Explicit no trade."
        );
    }
}

#[tokio::test]
async fn nullable_and_string_no_trade_reasons_remain_schema_valid() {
    for reason in [Value::Null, json!("   ")] {
        let mut changed = body();
        changed["no_trade"] = json!(true);
        changed["no_trade_reason"] = reason;
        assert_eq!(
            refusal(
                ModelDecision::new(FakeClient::content(&changed.to_string()))
                    .decide(&snapshot())
                    .await
            ),
            "model requested no trade"
        );
    }
    let mut changed = body();
    changed["no_trade_reason"] = json!("Additional information does not override no_trade=false.");
    assert!(matches!(
        ModelDecision::new(FakeClient::content(&changed.to_string())).decide(&snapshot()).await,
        Decision::Trade { .. }
    ));
    changed["no_trade"] = json!(true);
    changed["no_trade_reason"] = json!("x".repeat(801));
    assert_eq!(
        refusal(
            ModelDecision::new(FakeClient::content(&changed.to_string())).decide(&snapshot()).await
        ),
        "x".repeat(800)
    );
}

#[tokio::test]
async fn timeout_http_failure_and_missing_configuration_produce_no_trade() {
    for error in [
        ChatFailure::Transport,
        ChatFailure::Timeout,
        ChatFailure::HttpStatus(503),
        ChatFailure::MissingConfiguration,
    ] {
        let fake = FakeClient::answer(Err(error)).with_policy(VerificationPolicy {
            required: true,
            pinned_address: Some("0x0000000000000000000000000000000000000001".into()),
        });
        assert_eq!(
            refusal(ModelDecision::new(fake.clone()).decide(&snapshot()).await),
            error.to_string()
        );
        assert_eq!(fake.calls(), 1);
    }
}

#[tokio::test]
async fn stalled_fake_client_times_out_once_without_network_or_twenty_second_wait() {
    struct StalledClient(Arc<AtomicUsize>);
    impl ChatClient for StalledClient {
        fn requested_model(&self) -> &str {
            "fixture-model"
        }
        async fn complete(&self, _prompt: String) -> Result<ChatResponse, ChatFailure> {
            self.0.fetch_add(1, Ordering::SeqCst);
            std::future::pending().await
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let source = ModelDecision::new(StalledClient(calls.clone()))
        .with_timeout(Duration::from_millis(10))
        .unwrap();
    assert_eq!(refusal(source.decide(&snapshot()).await), "model request timed out");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn inconsistent_client_content_is_rejected_without_repairing_the_capture() {
    let mut mismatched = response(&body().to_string());
    let mut changed = body();
    changed["amount_in"] = json!("10000000");
    mismatched.content = changed.to_string();
    assert_eq!(
        refusal(ModelDecision::new(FakeClient::answer(Ok(mismatched))).decide(&snapshot()).await),
        "model response envelope is invalid"
    );
}

#[tokio::test]
async fn client_hash_and_identity_mutations_never_become_a_trade() {
    for field in ["request_hash", "response_hash", "id", "model"] {
        let mut changed = response(&body().to_string());
        match field {
            "request_hash" => changed.request_sha256 = "0".repeat(64),
            "response_hash" => changed.response_sha256 = "0".repeat(64),
            "id" => changed.id = "fabricated-chat".into(),
            _ => changed.model = "fabricated-model".into(),
        }
        let fake = FakeClient::answer(Ok(changed));
        assert_eq!(
            refusal(ModelDecision::new(fake.clone()).decide(&snapshot()).await),
            "model response envelope is invalid",
            "mutation: {field}"
        );
        assert_eq!(fake.calls(), 1);
    }
}

struct RequestedAs(FakeClient, &'static str);

impl ChatClient for RequestedAs {
    fn requested_model(&self) -> &str {
        self.1
    }
    async fn complete(&self, prompt: String) -> Result<ChatResponse, ChatFailure> {
        self.0.complete(prompt).await
    }
}

fn response_from(model: &str, content: &str) -> ChatResponse {
    ChatResponse::from_raw(
        serde_json::to_string(&json!({
            "id":"fixture-chat-2",
            "model":model,
            "choices":[{"message":{"role":"assistant","content":content},"finish_reason":"stop"}],
        }))
        .unwrap(),
        FIXTURE_REQUEST_BODY.into(),
    )
    .unwrap()
}

#[tokio::test]
async fn a_provider_named_model_cannot_select_an_allowed_source() {
    let mut source = snapshot();
    source.mandate.allowed_model_ids.push("second-model".into());
    // The client requested fixture-model, but the provider replies as an allowed second-model.
    let fake = FakeClient::answer(Ok(response_from("second-model", &body().to_string())));
    assert_eq!(
        refusal(ModelDecision::new(fake.clone()).decide(&source).await),
        "model response envelope is invalid"
    );
    assert_eq!(fake.calls(), 1);
    // When the requested name is the allowed one, the same reply is accepted with that source.
    let matching = RequestedAs(
        FakeClient::answer(Ok(response_from("second-model", &body().to_string()))),
        "second-model",
    );
    let Decision::Trade { response: Some(captured), .. } =
        ModelDecision::new(matching).decide(&source).await
    else {
        panic!("a reply naming the requested allowed model must pass")
    };
    assert_eq!(captured.model, "second-model");
}

fn public_signed_fixture() -> (ChatResponse, String, Value) {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/provenance-public-v1.json"))
        .expect("public synthetic fixture");
    let mut capture = ChatResponse::from_raw(
        fixture["raw_body"].as_str().unwrap().into(),
        fixture["request_body"].as_str().unwrap().into(),
    )
    .unwrap();
    capture.signature_proof = Some(fixture["signature_proof"].as_str().unwrap().into());
    (capture, fixture["pinned_address"].as_str().unwrap().into(), fixture)
}

#[tokio::test]
async fn public_signed_fixture_is_verified_before_the_exact_trade_is_returned() {
    let (capture, pinned, fixture) = public_signed_fixture();
    let fake = FakeClient::answer(Ok(capture))
        .with_policy(VerificationPolicy { required: true, pinned_address: Some(pinned.clone()) });
    let Decision::Trade { plan, response: Some(captured) } =
        ModelDecision::new(fake.clone()).decide(&snapshot()).await
    else {
        panic!("a valid public signature fixture should pass the same trade gates")
    };
    assert_eq!(plan.reason_hash(), fixture["canonical_reason_hash"].as_str().unwrap());
    assert_eq!(captured.response_sha256, fixture["response_sha256"].as_str().unwrap());
    assert_eq!(captured.request_sha256, fixture["request_sha256"].as_str().unwrap());
    assert_eq!(
        verify_response(&captured, Some(&pinned), true).unwrap(),
        VerificationStatus::VerifiedPinnedSigner
    );
    assert_eq!(plan.amount_in.0, 20_000_000);
    assert_eq!(plan.min_amount_out.0, 4 * NEAR);
    assert_eq!(fake.calls(), 1);
}

#[tokio::test]
async fn returned_invalid_or_unsupported_proofs_refuse_even_with_optional_policy() {
    let (original, pinned, _) = public_signed_fixture();
    for mutation in ["signature", "algorithm", "kind", "request", "malformed"] {
        let mut capture = original.clone();
        let mut proof: Value =
            serde_json::from_str(capture.signature_proof.as_ref().unwrap()).unwrap();
        match mutation {
            "signature" => proof["signature"] = json!("0x1234"),
            "algorithm" => proof["signing_algo"] = json!("unsupported"),
            "kind" => proof["signature_kind"] = json!("unsupported"),
            "request" => {
                capture.request_body = FIXTURE_REQUEST_BODY.into();
                capture.request_sha256 = sha256_hex(FIXTURE_REQUEST_BODY.as_bytes());
            }
            _ => {}
        }
        capture.signature_proof =
            Some(if mutation == "malformed" { "not json".into() } else { proof.to_string() });
        let fake = FakeClient::answer(Ok(capture)).with_policy(VerificationPolicy {
            required: false,
            pinned_address: Some(pinned.clone()),
        });
        assert_eq!(
            refusal(ModelDecision::new(fake.clone()).decide(&snapshot()).await),
            "model response signature verification failed",
            "mutation: {mutation}"
        );
        assert_eq!(fake.calls(), 1);
    }
}

#[tokio::test]
async fn supplied_proof_without_an_external_pin_and_required_missing_proof_refuse() {
    let (capture, pinned, _) = public_signed_fixture();
    let fake = FakeClient::answer(Ok(capture));
    assert_eq!(
        refusal(ModelDecision::new(fake.clone()).decide(&snapshot()).await),
        "model response signature verification failed"
    );
    assert_eq!(fake.calls(), 1);
    let missing = FakeClient::content(&body().to_string())
        .with_policy(VerificationPolicy { required: true, pinned_address: Some(pinned) });
    assert_eq!(
        refusal(ModelDecision::new(missing.clone()).decide(&snapshot()).await),
        "model response signature verification failed"
    );
    assert_eq!(missing.calls(), 1);
}

#[tokio::test]
async fn canonical_commitment_preserves_equivalent_outer_json_while_raw_hashes_differ() {
    let first = response(&body().to_string());
    let envelope: Value = serde_json::from_str(&first.raw_body).unwrap();
    let second = ChatResponse::from_raw(
        serde_json::to_string_pretty(&envelope).unwrap(),
        FIXTURE_REQUEST_BODY.into(),
    )
    .unwrap();
    assert_ne!(first.response_sha256, second.response_sha256);
    let mut commitments = Vec::new();
    for capture in [first, second] {
        let Decision::Trade { plan, .. } =
            ModelDecision::new(FakeClient::answer(Ok(capture))).decide(&snapshot()).await
        else {
            panic!("equivalent valid outer envelopes should remain valid decisions")
        };
        commitments.push(plan.reason_hash());
    }
    assert_eq!(commitments[0], commitments[1]);
}

#[tokio::test]
async fn attestation_address_metadata_is_retained_without_claiming_verified_provenance() {
    let mut envelope: Value =
        serde_json::from_str(&response(&body().to_string()).raw_body).unwrap();
    envelope["attestation"] = json!({
        "signing_address":"0x0000000000000000000000000000000000000001",
        "signing_algo":"ecdsa",
        "signature_kind":"provider_tee",
        "status":"unverified public metadata"
    });
    let raw_body = envelope.to_string();
    let capture = ChatResponse::from_raw(raw_body.clone(), FIXTURE_REQUEST_BODY.into()).unwrap();
    assert!(capture.signature_proof.is_none());
    let Decision::Trade { response: Some(retained), .. } =
        ModelDecision::new(FakeClient::answer(Ok(capture))).decide(&snapshot()).await
    else {
        panic!("unsigned attestation metadata does not constitute a chat signature")
    };
    assert_eq!(retained.raw_body, raw_body);
    assert_eq!(verify_response(&retained, None, false).unwrap(), VerificationStatus::Unverified);
}

#[tokio::test]
async fn forged_plan_commitment_and_trade_tuple_do_not_validate_against_the_capture() {
    use mandate_agent::provenance::ModelRecord;
    let Decision::Trade { plan, response: Some(capture) } =
        ModelDecision::new(FakeClient::content(&body().to_string())).decide(&snapshot()).await
    else {
        panic!("valid fixture should produce a committed plan")
    };
    assert!(ModelRecord::from_response(&capture, &plan).is_ok());
    let mut changed = plan.clone();
    changed.response_commitment = Some("0".repeat(64));
    assert!(ModelRecord::from_response(&capture, &changed).is_err());
    let mut changed = plan;
    changed.amount_in = U128(changed.amount_in.0 + 1);
    assert!(ModelRecord::from_response(&capture, &changed).is_err());
}

#[test]
fn inline_unknown_signatures_and_duplicate_capture_fields_are_refused() {
    for field in ["signature", "signatures", "proof", "signature_response", "chat_signature"] {
        let mut envelope: Value =
            serde_json::from_str(&response(&body().to_string()).raw_body).unwrap();
        envelope[field] = json!({"unsupported":"public fixture"});
        assert!(
            matches!(
                ChatResponse::from_raw(envelope.to_string(), FIXTURE_REQUEST_BODY.into()),
                Err(ChatFailure::InvalidProof)
            ),
            "field: {field}"
        );
    }
    let mut nested: Value = serde_json::from_str(&response(&body().to_string()).raw_body).unwrap();
    nested["attestation"] = json!({"signature":"unsupported public fixture"});
    assert!(matches!(
        ChatResponse::from_raw(nested.to_string(), FIXTURE_REQUEST_BODY.into()),
        Err(ChatFailure::InvalidProof)
    ));
    let valid = response(&body().to_string());
    let duplicate = format!("{},\"model\":\"other-model\"}}", valid.raw_body.trim_end_matches('}'));
    assert!(matches!(
        ChatResponse::from_raw(duplicate, FIXTURE_REQUEST_BODY.into()),
        Err(ChatFailure::InvalidResponse)
    ));
    assert!(matches!(
        ChatResponse::from_raw(valid.raw_body, "{\"model\":1,\"model\":2}".into()),
        Err(ChatFailure::InvalidResponse)
    ));
}

#[test]
fn signature_lookup_uses_only_the_configured_https_origin_and_encoded_id() {
    let endpoint = "https://provider.example/v1/chat/completions";
    let base = "https://provider.example/v1/signature/";
    let url = signature_lookup_url(endpoint, base, "chat/a?b", "fixture/model").unwrap();
    assert_eq!(url.path(), "/v1/signature/chat%2Fa%3Fb");
    assert_eq!(
        url.query_pairs()
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect::<Vec<_>>(),
        vec![
            ("signing_algo".to_owned(), "ecdsa".to_owned()),
            ("model".to_owned(), "fixture/model".to_owned()),
        ]
    );
    for invalid in [
        "http://provider.example/v1/signature/",
        "https://other.example/v1/signature/",
        "https://provider.example:444/v1/signature/",
        "https://user@provider.example/v1/signature/",
        "https://provider.example/v1/signature",
        "https://provider.example/v1/signature/?query=public",
        "https://provider.example/v1/signature/#fragment",
    ] {
        assert!(
            matches!(
                signature_lookup_url(endpoint, invalid, "chat-1", "fixture-model"),
                Err(ChatFailure::InvalidConfiguration)
            ),
            "URL: {invalid}"
        );
    }
    for id in [".", ".."] {
        assert!(matches!(
            signature_lookup_url(endpoint, base, id, "fixture-model"),
            Err(ChatFailure::InvalidConfiguration)
        ));
    }
    assert_eq!(
        signature_lookup_url(endpoint, base, "%2E%2E", "fixture-model").unwrap().path(),
        "/v1/signature/%252E%252E"
    );
}

#[test]
fn pure_decision_extraction_retains_strict_schema_without_a_snapshot() {
    let parsed = extract_decision(&body().to_string()).unwrap();
    assert_eq!(parsed.plan.amount_in.0, 20_000_000);
    assert!(!parsed.no_trade);
    let mut changed = body();
    changed["asset_in"] = json!("nep141:outside.testnet");
    assert!(extract_decision(&changed.to_string()).is_ok());
    changed["amount_in"] = json!("01");
    assert_eq!(
        extract_decision(&changed.to_string()).err(),
        Some("model amount_in is not an exact positive u128 decimal integer")
    );
}

#[test]
fn malformed_optional_settings_never_downgrade_to_absence() {
    use std::env::VarError;
    use std::ffi::OsString;
    assert_eq!(
        optional_model_setting(Ok("public fixture".into())),
        Ok(Some("public fixture".into()))
    );
    assert_eq!(optional_model_setting(Err(VarError::NotPresent)), Ok(None));
    assert_eq!(
        optional_model_setting(Err(VarError::NotUnicode(OsString::from(
            "public malformed setting"
        )))),
        Err(ChatFailure::InvalidConfiguration)
    );
}

#[test]
fn complete_request_marker_scan_covers_model_metadata_and_system_instructions() {
    let marker = "public-request-scan-marker";
    for body in [
        json!({"model":marker,"messages":[{"role":"user","content":"public snapshot"}]})
            .to_string(),
        json!({"model":"fixture-model","messages":[{"role":"system","content":marker}]})
            .to_string(),
        "{\"model\":\"public-request-scan-\\u006darker\"}".into(),
    ] {
        assert_eq!(validate_public_request_body(&body, marker), Err(ChatFailure::InvalidSnapshot));
    }
    assert_eq!(validate_public_request_body(FIXTURE_REQUEST_BODY, marker), Ok(()));
}

#[tokio::test]
async fn paused_or_halted_snapshot_makes_zero_model_calls() {
    for reason in ["vault is paused", "vault is halted"] {
        let mut blocked = snapshot();
        blocked.not_ready = Some(reason.into());
        let (capture, pinned, _) = public_signed_fixture();
        let fake = FakeClient::answer(Ok(capture))
            .with_policy(VerificationPolicy { required: true, pinned_address: Some(pinned) });
        assert_eq!(
            refusal(ModelDecision::new(fake.clone()).decide(&blocked).await),
            "vault is not ready for a model decision"
        );
        assert_eq!(fake.calls(), 0);
        assert!(fake.prompts.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn empty_source_allowlist_makes_zero_model_calls() {
    let mut blocked = snapshot();
    blocked.mandate.allowed_model_ids.clear();
    let fake = FakeClient::content(&body().to_string());
    assert_eq!(
        refusal(ModelDecision::new(fake.clone()).decide(&blocked).await),
        "no decision sources are allowed"
    );
    assert_eq!(fake.calls(), 0);
    assert!(fake.prompts.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_unlisted_returned_model_id_refuses_after_one_call() {
    let mut envelope: Value =
        serde_json::from_str(&response(&body().to_string()).raw_body).unwrap();
    envelope["model"] = json!("unknown-model");
    let capture =
        ChatResponse::from_raw(envelope.to_string(), FIXTURE_REQUEST_BODY.into()).unwrap();
    let fake = FakeClient::answer(Ok(capture));
    assert_eq!(
        refusal(ModelDecision::new(fake.clone()).decide(&snapshot()).await),
        "the declared model id is not allowed"
    );
    assert_eq!(fake.calls(), 1);
}

#[tokio::test]
async fn removing_a_previously_allowed_model_refuses_its_next_decision() {
    let mut snapshot = snapshot();
    let fake = FakeClient::content(&body().to_string());
    let source = ModelDecision::new(fake.clone());
    assert!(matches!(source.decide(&snapshot).await, Decision::Trade { .. }));
    snapshot.mandate.allowed_model_ids.retain(|id| id != "fixture-model");
    assert_eq!(refusal(source.decide(&snapshot).await), "the declared model id is not allowed");
    assert_eq!(fake.calls(), 2, "one request for each independent decision, with no retries");
}

#[tokio::test]
async fn invalid_public_snapshot_stops_before_the_model_call() {
    let mut incomplete = snapshot();
    incomplete.market.balances.remove(A);
    let fake = FakeClient::content(&body().to_string());
    assert_eq!(
        refusal(ModelDecision::new(fake.clone()).decide(&incomplete).await),
        "public model snapshot is invalid"
    );
    assert_eq!(fake.calls(), 0);
}

#[tokio::test]
async fn unchanged_local_engine_refuses_excess_size_and_bad_minimum_output() {
    for (field, value, expected) in [
        ("amount_in", "40000000".to_owned(), "TradeTooLarge"),
        ("min_amount_out", NEAR.to_string(), "SlippageExceeded"),
    ] {
        let mut changed = body();
        changed[field] = json!(value);
        assert!(
            refusal(
                ModelDecision::new(FakeClient::content(&changed.to_string()))
                    .decide(&snapshot())
                    .await
            )
            .contains(expected)
        );
    }
}

#[test]
fn compact_prompt_preserves_public_integer_prices_balances_and_rules() {
    let text = compact_snapshot(&snapshot()).unwrap();
    assert!(!text.contains('\n'));
    let input: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(input["final_block_hash"], snapshot().block_hash);
    assert_eq!(input["chain_time_sec"], NOW);
    assert_eq!(input["assets"][1]["balance"], (5 * NEAR).to_string());
    assert_eq!(input["assets"][1]["oracle_price"]["mantissa"], "500000000");
    assert_eq!(input["assets"][1]["oracle_price"]["publish_time_sec"], NOW);
    assert_eq!(input["mandate"]["max_trade_usd"], "30000000");
    assert_eq!(
        input["mandate"]["allowed_model_ids"],
        json!(["manual-v1", "rebalance-v1", "fixture-model"])
    );
    assert_eq!(input["trading"]["high_water_mark_usd"], "125000000");
    for prohibited_field in ["MODEL_ENDPOINT_URL", "MODEL_NAME", "MODEL_API_KEY", "authorization"] {
        assert!(!text.contains(prohibited_field));
    }
}

#[test]
fn captured_envelope_rejects_oversize_and_non_model_reserved_ids() {
    assert!(matches!(
        ChatResponse::from_raw(
            "x".repeat(MAX_CHAT_RESPONSE_BYTES + 1),
            FIXTURE_REQUEST_BODY.into()
        ),
        Err(ChatFailure::ResponseTooLarge)
    ));
    for id in ["manual-v1", "rebalance-v1", ""] {
        let mut envelope: Value =
            serde_json::from_str(&response(&body().to_string()).raw_body).unwrap();
        envelope["model"] = json!(id);
        assert!(matches!(
            ChatResponse::from_raw(envelope.to_string(), FIXTURE_REQUEST_BODY.into()),
            Err(ChatFailure::InvalidResponse)
        ));
    }
}

#[tokio::test]
async fn manual_adapter_preserves_plan_hash_and_existing_structural_only_validation() {
    let plan = Plan {
        asset_in: A.into(),
        asset_out: B.into(),
        amount_in: U128(40_000_000),
        min_amount_out: U128(8 * NEAR),
        rationale: "A manual plan retains its previous behavior.".into(),
        response_commitment: None,
    };
    let adapter = ManualDecision::new(plan.clone());
    assert_eq!(adapter.validated_plan().unwrap(), plan);
    let Decision::Trade { plan: retained, response: None } = adapter.decide(&snapshot()).await
    else {
        panic!("the existing manual source must still defer economic gates to the vault")
    };
    assert_eq!(retained, plan);
    assert_eq!(retained.reason_hash(), plan.reason_hash());
}

#[test]
fn decoded_scan_detects_escaped_and_nested_public_markers_without_credentials() {
    let marker = format!("public-scan-marker-{}", std::process::id());
    let escaped: String =
        marker.chars().map(|character| format!("\\u{:04x}", u32::from(character))).collect();
    // The generated marker is public test data, not an API key or credential.
    let raw = format!("{{\"id\":\"{escaped}\",\"safe\":true}}");
    assert!(!raw.contains(&marker));
    assert!(response_contains_marker(&raw, &marker).unwrap());

    let content =
        body().to_string().replace("Rebalance within the owner-defined limits.", &escaped);
    let nested = response(&content).raw_body;
    assert!(!nested.contains(&marker));
    assert!(response_contains_marker(&nested, &marker).unwrap());

    let duplicate = format!("{{\"debug\":\"{escaped}\",\"debug\":\"safe\"}}");
    assert!(response_contains_marker(&duplicate, &marker).unwrap());
    assert!(!response_contains_marker(&response(&body().to_string()).raw_body, &marker).unwrap());
    let mut public_input = snapshot();
    public_input.mandate.assets[0].price_feed_id = marker.clone();
    assert!(response_contains_marker(&compact_snapshot(&public_input).unwrap(), &marker).unwrap());
    assert!(matches!(
        response_contains_marker(r#"{"message":"{invalid embedded JSON}"}"#, &marker),
        Err(ChatFailure::InvalidResponse)
    ));
}

#[test]
fn decoded_scan_fails_closed_when_embedded_json_exceeds_its_depth_budget() {
    let character = char::from_u32(0x2600 + std::process::id() % 16).unwrap();
    let marker = character.to_string();
    let escaped = format!("\\u{:04x}", u32::from(character));
    let mut nested = format!("{{\"value\":\"{escaped}\"}}");
    for _ in 0..10 {
        nested = json!({"wrapped":nested}).to_string();
    }
    assert!(nested.len() <= MAX_CHAT_RESPONSE_BYTES);
    assert!(!nested.contains(&marker));
    assert!(matches!(
        response_contains_marker(&nested, &marker),
        Err(ChatFailure::InvalidResponse)
    ));
}

#[test]
fn rebalance_adapter_uses_the_existing_exact_candidate_without_network_requests() {
    let config = AutonomousConfig {
        rpc_url: "https://example.invalid/rpc".into(),
        expected_chain_id: Some("fixture".into()),
        vault_id: "vault.testnet".into(),
        relay_url: "https://example.invalid/relay".into(),
        asset_a: A.into(),
        asset_b: B.into(),
        target_a_bps: 5000,
        band_bps: 100,
        minimum_quote_validity_sec: 450,
        legacy_price_oracle: false,
    };
    // Client construction does not send a request. This test calls choose only,
    // never prepare, refresh, quote, or a production model configuration factory.
    let relay = Relay::new(&config.relay_url, None).unwrap();
    let adapter = RebalanceDecision::new(config.clone(), relay);
    let expected = autonomous::choose(&config, &snapshot()).unwrap();
    assert_eq!(adapter.choose(&snapshot()).unwrap(), expected);
    assert_eq!(expected.asset_in, A);
    assert_eq!(expected.amount_in, 30_000_000);
}
