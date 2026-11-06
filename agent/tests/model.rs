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
    compact_snapshot, response_contains_marker,
};
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
    )
    .unwrap()
}

#[derive(Clone)]
struct FakeClient {
    answer: Result<ChatResponse, ChatFailure>,
    calls: Arc<AtomicUsize>,
    prompts: Arc<Mutex<Vec<String>>>,
}

impl FakeClient {
    fn answer(answer: Result<ChatResponse, ChatFailure>) -> Self {
        Self { answer, calls: Arc::new(AtomicUsize::new(0)), prompts: Arc::new(Mutex::new(vec![])) }
    }

    fn content(content: &str) -> Self {
        Self::answer(Ok(response(content)))
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ChatClient for FakeClient {
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
        let fake = FakeClient::answer(Err(error));
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
async fn paused_or_halted_snapshot_makes_zero_model_calls() {
    for reason in ["vault is paused", "vault is halted"] {
        let mut blocked = snapshot();
        blocked.not_ready = Some(reason.into());
        let fake = FakeClient::content(&body().to_string());
        assert_eq!(
            refusal(ModelDecision::new(fake.clone()).decide(&blocked).await),
            "vault is not ready for a model decision"
        );
        assert_eq!(fake.calls(), 0);
        assert!(fake.prompts.lock().unwrap().is_empty());
    }
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
    assert_eq!(input["trading"]["high_water_mark_usd"], "125000000");
    for prohibited_field in ["MODEL_ENDPOINT_URL", "MODEL_NAME", "MODEL_API_KEY", "authorization"] {
        assert!(!text.contains(prohibited_field));
    }
}

#[test]
fn captured_envelope_rejects_oversize_and_non_model_reserved_ids() {
    assert!(matches!(
        ChatResponse::from_raw("x".repeat(MAX_CHAT_RESPONSE_BYTES + 1)),
        Err(ChatFailure::ResponseTooLarge)
    ));
    for id in ["manual-v1", "rebalance-v1", ""] {
        let mut envelope: Value =
            serde_json::from_str(&response(&body().to_string()).raw_body).unwrap();
        envelope["model"] = json!(id);
        assert!(matches!(
            ChatResponse::from_raw(envelope.to_string()),
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
