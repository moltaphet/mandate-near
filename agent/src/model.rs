//! Bounded model decisions. Raw request and response hashes bind provider proofs;
//! the separately versioned canonical response hash becomes the on-chain reason
//! commitment. A declared model name alone is not origin proof.

use std::fmt;
use std::future::Future;
use std::time::Duration;

use mandate_vault::engine::{Trade, check_trade};
use near_sdk::json_types::U128;
use near_sdk::serde_json::{self, Value, json};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use crate::Plan;
use crate::autonomous::Snapshot;
use crate::decision::{Decision, DecisionSource, MANUAL_SOURCE_ID, REBALANCE_SOURCE_ID};
use crate::provenance::{canonical_response_hash, verify_response};

pub const MAX_CHAT_RESPONSE_BYTES: usize = 64 * 1024;
pub const MAX_RATIONALE_CHARS: usize = 800;
pub const MODEL_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

const MODEL_INSTRUCTIONS: &str = "Choose a trade or no trade using only the supplied public snapshot. Return only one JSON object with exactly these fields: asset_in (string), asset_out (string), amount_in (positive canonical decimal integer string in token base units), min_amount_out (positive canonical decimal integer string in token base units), rationale (nonempty string of at most 800 Unicode characters), no_trade (boolean), no_trade_reason (string or null). Both assets must be distinct assets in the mandate. All asset, amount and rationale fields remain required and valid even for no_trade=true. Never return commands, destinations, replacement limits or extra fields. The vault's existing rules remain authoritative.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatFailure {
    MissingConfiguration,
    InvalidConfiguration,
    Transport,
    Timeout,
    HttpStatus(u16),
    ResponseTooLarge,
    InvalidResponse,
    InvalidSnapshot,
    InvalidProof,
}

impl fmt::Display for ChatFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingConfiguration => f.write_str("model provider configuration is missing"),
            Self::InvalidConfiguration => f.write_str("model provider configuration is invalid"),
            Self::Transport => f.write_str("model request failed or timed out"),
            Self::Timeout => f.write_str("model request timed out"),
            Self::HttpStatus(status) => write!(f, "model provider returned HTTP status {status}"),
            Self::ResponseTooLarge => f.write_str("model response exceeds the size limit"),
            Self::InvalidResponse => f.write_str("model response envelope is invalid"),
            Self::InvalidSnapshot => f.write_str("public model snapshot is invalid"),
            Self::InvalidProof => f.write_str("model response signature verification failed"),
        }
    }
}

impl std::error::Error for ChatFailure {}

/// Exact UTF-8 bodies and declared IDs. Capturing a response does not verify its
/// provenance; ModelDecision checks every supplied signature before a trade.
#[derive(Clone)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub raw_body: String,
    pub content: String,
    pub request_body: String,
    /// SHA-256 of the exact request body bytes sent, as 64 lowercase hex characters.
    pub request_sha256: String,
    /// SHA-256 of the exact response body bytes received, as 64 lowercase hex characters.
    pub response_sha256: String,
    /// Exact separately fetched signature document, or the complete envelope for
    /// an inline document using the documented flat signature fields.
    pub signature_proof: Option<String>,
}

impl ChatResponse {
    pub fn from_raw(raw_body: String, request_body: String) -> Result<Self, ChatFailure> {
        if raw_body.len() > MAX_CHAT_RESPONSE_BYTES || request_body.len() > MAX_CHAT_RESPONSE_BYTES
        {
            return Err(ChatFailure::ResponseTooLarge);
        }
        // The canonical parser also rejects duplicate map fields and unsupported
        // number representations; a JSON decoder must not discard proof fields.
        canonical_response_hash(&request_body).map_err(|_| ChatFailure::InvalidResponse)?;
        canonical_response_hash(&raw_body).map_err(|_| ChatFailure::InvalidResponse)?;
        let signature_proof = inline_signature_proof(&raw_body)?;
        let envelope: ChatEnvelope =
            serde_json::from_str(&raw_body).map_err(|_| ChatFailure::InvalidResponse)?;
        if !bounded_identity(&envelope.id, 128)
            || !bounded_identity(&envelope.model, 64)
            || [MANUAL_SOURCE_ID, REBALANCE_SOURCE_ID].contains(&envelope.model.as_str())
            || envelope.choices.len() != 1
        {
            return Err(ChatFailure::InvalidResponse);
        }
        let choice = envelope.choices.into_iter().next().ok_or(ChatFailure::InvalidResponse)?;
        if choice.message.role != "assistant"
            || choice.message.refusal.is_some()
            || choice.message.tool_calls.is_some_and(|calls| !calls.is_empty())
            || choice.finish_reason.is_some_and(|reason| reason != "stop")
        {
            return Err(ChatFailure::InvalidResponse);
        }
        let response_sha256 = sha256_hex(raw_body.as_bytes());
        Ok(Self {
            id: envelope.id,
            model: envelope.model,
            raw_body,
            content: choice.message.content,
            request_sha256: sha256_hex(request_body.as_bytes()),
            request_body,
            response_sha256,
            signature_proof,
        })
    }
}

fn inline_signature_proof(raw_body: &str) -> Result<Option<String>, ChatFailure> {
    let value: Value = serde_json::from_str(raw_body).map_err(|_| ChatFailure::InvalidResponse)?;
    let object = value.as_object().ok_or(ChatFailure::InvalidResponse)?;
    let has_signature_fields = [
        "signature",
        "signatures",
        "signature_kind",
        "signing_algo",
        "signing_address",
        "proof",
        "signature_response",
        "chat_signature",
    ]
    .iter()
    .any(|field| object.contains_key(*field));
    if !has_signature_fields {
        if object.values().any(contains_signature_fields) {
            return Err(ChatFailure::InvalidProof);
        }
        return Ok(None);
    }
    // Only the documented flat text/signature/address document is supported.
    // An unknown nested signature or proof must never become an unsigned response.
    if object.contains_key("signatures")
        || object.contains_key("proof")
        || object.contains_key("signature_response")
        || object.contains_key("chat_signature")
    {
        return Err(ChatFailure::InvalidProof);
    }
    if object.values().any(contains_signature_fields) {
        return Err(ChatFailure::InvalidProof);
    }
    for field in ["text", "signature", "signing_address"] {
        if !object.get(field).is_some_and(Value::is_string) {
            return Err(ChatFailure::InvalidProof);
        }
    }
    Ok(Some(raw_body.to_owned()))
}

fn contains_signature_fields(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields.iter().any(|(field, value)| {
            ["signature", "signatures", "proof", "signature_response", "chat_signature"]
                .contains(&field.as_str())
                || contains_signature_fields(value)
        }),
        Value::Array(values) => values.iter().any(contains_signature_fields),
        _ => false,
    }
}

/// Trusted operator policy, independent of any address asserted in a response.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VerificationPolicy {
    pub required: bool,
    pub pinned_address: Option<String>,
}

/// Parses an optional setting without turning a present malformed value into an
/// absent setting. The pure input makes configuration errors testable offline.
pub fn optional_model_setting(
    value: Result<String, std::env::VarError>,
) -> Result<Option<String>, ChatFailure> {
    match value {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(ChatFailure::InvalidConfiguration),
    }
}

/// SHA-256 of `bytes` as 64 lowercase hex characters.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// True for exactly 64 lowercase hexadecimal characters.
pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn bounded_identity(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value.is_ascii()
        && !value.bytes().any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
}

#[derive(Deserialize)]
struct ChatEnvelope {
    id: String,
    model: String,
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
    refusal: Option<String>,
    tool_calls: Option<Vec<Value>>,
}

pub trait ChatClient: Send + Sync {
    /// The model name this client requested. A response naming any other model is refused,
    /// so the declared source is always the configured name, never a provider-chosen one.
    fn requested_model(&self) -> &str;
    fn verification_policy(&self) -> VerificationPolicy {
        VerificationPolicy::default()
    }
    fn complete(
        &self,
        prompt: String,
    ) -> impl Future<Output = Result<ChatResponse, ChatFailure>> + Send;
}

pub struct ModelDecision<C> {
    client: C,
    timeout: Duration,
}

impl<C: ChatClient> ModelDecision<C> {
    pub fn new(client: C) -> Self {
        Self { client, timeout: MODEL_REQUEST_TIMEOUT }
    }

    /// Selects a shorter bounded deadline, useful for callers and offline tests.
    /// A caller cannot extend the production maximum or disable the timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Result<Self, ChatFailure> {
        if timeout.is_zero() || timeout > MODEL_REQUEST_TIMEOUT {
            return Err(ChatFailure::InvalidConfiguration);
        }
        self.timeout = timeout;
        Ok(self)
    }
}

impl<C: ChatClient> DecisionSource for ModelDecision<C> {
    async fn decide(&self, snapshot: &Snapshot) -> Decision {
        if snapshot.not_ready.is_some() {
            return no_trade("vault is not ready for a model decision");
        }
        if snapshot.mandate.allowed_model_ids.is_empty() {
            return no_trade("no decision sources are allowed");
        }
        let prompt = match compact_snapshot(snapshot) {
            Ok(prompt) => prompt,
            Err(error) => return no_trade(error.to_string()),
        };
        let response = match tokio::time::timeout(self.timeout, self.client.complete(prompt)).await
        {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => return no_trade(error.to_string()),
            Err(_) => return no_trade(ChatFailure::Timeout.to_string()),
        };
        let supplied_proof = response.signature_proof.clone();
        if supplied_proof.as_ref().is_some_and(|proof| proof.len() > MAX_CHAT_RESPONSE_BYTES) {
            return no_trade(ChatFailure::ResponseTooLarge.to_string());
        }
        // The fakeable client cannot return content or attribution inconsistent
        // with the captured raw envelope. Reject it; do not repair it silently.
        let parsed = match ChatResponse::from_raw(
            response.raw_body.clone(),
            response.request_body.clone(),
        ) {
            Ok(parsed) => parsed,
            Err(error) => return no_trade(error.to_string()),
        };
        if parsed.id != response.id
            || parsed.model != response.model
            || parsed.content != response.content
            || parsed.request_sha256 != response.request_sha256
            || parsed.response_sha256 != response.response_sha256
        {
            return no_trade(ChatFailure::InvalidResponse.to_string());
        }
        let mut response = parsed;
        // Recomputed bodies are authoritative. A separately supplied proof may
        // not replace a proof already present inside the captured response.
        if response.signature_proof.is_some() && response.signature_proof != supplied_proof {
            return no_trade(ChatFailure::InvalidProof.to_string());
        }
        response.signature_proof = supplied_proof;
        if !snapshot.mandate.allows_model_id(&response.model) {
            return no_trade("the declared model id is not allowed");
        }
        // The declared source must also be the model that was requested. A provider cannot
        // select an allowed source by naming it in its reply: both checks must pass.
        if response.model != self.client.requested_model() {
            return no_trade(ChatFailure::InvalidResponse.to_string());
        }
        let validated = match validate_decision(&response.content, snapshot) {
            Ok(validated) => validated,
            Err(reason) => return no_trade(reason),
        };
        if validated.no_trade {
            return no_trade(
                validated.no_trade_reason.as_deref().unwrap_or("model requested no trade"),
            );
        }
        let mut plan = validated.plan;
        let trade = Trade {
            asset_in: plan.asset_in.clone(),
            asset_out: plan.asset_out.clone(),
            amount_in: plan.amount_in.0,
            min_amount_out: plan.min_amount_out.0,
        };
        if let Err(error) =
            check_trade(&snapshot.mandate, &snapshot.trading, &trade, &snapshot.market)
        {
            return no_trade(format!("local vault preview rejected the trade: {}", error.code()));
        }
        let policy = self.client.verification_policy();
        if verify_response(&response, policy.pinned_address.as_deref(), policy.required).is_err() {
            return no_trade(ChatFailure::InvalidProof.to_string());
        }
        plan.response_commitment = match canonical_response_hash(&response.raw_body) {
            Ok(commitment) => Some(commitment),
            Err(_) => return no_trade(ChatFailure::InvalidResponse.to_string()),
        };
        Decision::Trade { plan, response: Some(Box::new(response)) }
    }
}

fn no_trade(reason: impl AsRef<str>) -> Decision {
    let reason: String = reason
        .as_ref()
        .chars()
        .map(|character| if character.is_control() { ' ' } else { character })
        .take(MAX_RATIONALE_CHARS)
        .collect();
    Decision::NoTrade {
        reason: if reason.trim().is_empty() { "model requested no trade".into() } else { reason },
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelOutput {
    asset_in: String,
    asset_out: String,
    amount_in: String,
    min_amount_out: String,
    rationale: String,
    no_trade: bool,
    #[serde(deserialize_with = "required_nullable_string")]
    no_trade_reason: Option<String>,
}

fn required_nullable_string<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    struct NullableString;
    impl<'de> Visitor<'de> for NullableString {
        type Value = Option<String>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a string or null")
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            Ok(Some(value.to_owned()))
        }

        fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
            Ok(Some(value))
        }
    }
    // deserialize_any rejects Serde's missing-field sentinel while accepting an
    // actual JSON null. An ordinary Option<String> would permit an absent field.
    deserializer.deserialize_any(NullableString)
}

impl ModelOutput {
    fn plan(&self) -> Result<Plan, &'static str> {
        if self.asset_in == self.asset_out {
            return Err("model decision assets must differ");
        }
        let amount_in = positive_decimal(&self.amount_in)
            .ok_or("model amount_in is not an exact positive u128 decimal integer")?;
        let min_amount_out = positive_decimal(&self.min_amount_out)
            .ok_or("model min_amount_out is not an exact positive u128 decimal integer")?;
        if self.rationale.trim().is_empty() || self.rationale.chars().count() > MAX_RATIONALE_CHARS
        {
            return Err("model rationale must contain 1 to 800 Unicode characters");
        }
        Ok(Plan {
            asset_in: self.asset_in.clone(),
            asset_out: self.asset_out.clone(),
            amount_in: U128(amount_in),
            min_amount_out: U128(min_amount_out),
            rationale: self.rationale.clone(),
            response_commitment: None,
        })
    }
}

/// Strict model-schema extraction only. Gate approval is deliberately separate:
/// ModelDecision always runs the unchanged engine before returning a trade.
/// A test can submit an extracted adversarial plan directly to demonstrate that
/// the vault remains authoritative even when local preview is bypassed.
pub struct ValidatedDecision {
    pub plan: Plan,
    pub no_trade: bool,
    pub no_trade_reason: Option<String>,
}

pub fn validate_decision(
    content: &str,
    snapshot: &Snapshot,
) -> Result<ValidatedDecision, &'static str> {
    let decision = extract_decision(content)?;
    if snapshot.mandate.asset(&decision.plan.asset_in).is_none()
        || snapshot.mandate.asset(&decision.plan.asset_out).is_none()
    {
        return Err("model decision asset is outside the mandate");
    }
    Ok(decision)
}

/// Pure extraction shared with the offline verifier. It checks the exact schema,
/// distinct assets, positive canonical u128 strings and bounded rationale. The
/// live decision path additionally checks membership and the unchanged engine.
pub fn extract_decision(content: &str) -> Result<ValidatedDecision, &'static str> {
    let output: ModelOutput = serde_json::from_str(content)
        .map_err(|_| "model decision must match the exact JSON schema")?;
    let plan = output.plan()?;
    Ok(ValidatedDecision {
        plan,
        no_trade: output.no_trade,
        no_trade_reason: output.no_trade_reason,
    })
}

fn positive_decimal(value: &str) -> Option<u128> {
    if value.is_empty()
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok().filter(|amount| *amount > 0)
}

/// Compact JSON of public chain state only. Token balances, USD counters and
/// price mantissas are strings, preserving integer precision. Provider settings
/// are never fields in this prompt. This is not the Step 2 canonical response.
pub fn compact_snapshot(snapshot: &Snapshot) -> Result<String, ChatFailure> {
    snapshot.mandate.validate().map_err(|_| ChatFailure::InvalidSnapshot)?;
    let assets: Result<Vec<Value>, ChatFailure> = snapshot
        .mandate
        .assets
        .iter()
        .map(|asset| {
            let balance = snapshot
                .market
                .balances
                .get(&asset.asset_id)
                .ok_or(ChatFailure::InvalidSnapshot)?;
            Ok(json!({
                "asset_id":asset.asset_id,
                "decimals":asset.decimals,
                "price_feed_id":asset.price_feed_id,
                "balance":balance.to_string(),
                "oracle_price":snapshot.market.prices.get(&asset.asset_id)
            }))
        })
        .collect();
    let prompt = serde_json::to_string(&json!({
        "final_block_hash":snapshot.block_hash,
        "chain_time_sec":snapshot.market.now_sec,
        "assets":assets?,
        "mandate":snapshot.mandate,
        "trading":{
            "last_trade_at_sec":snapshot.trading.last_trade_at_sec,
            "day_start_sec":snapshot.trading.day_start_sec,
            "day_turnover_usd":snapshot.trading.day_turnover_usd.to_string(),
            "high_water_mark_usd":snapshot.trading.high_water_mark_usd.to_string(),
            "signed_intent_expires_at_sec":snapshot.trading.signed_intent_expires_at_sec
        }
    }))
    .map_err(|_| ChatFailure::InvalidSnapshot)?;
    if prompt.len() > MAX_CHAT_RESPONSE_BYTES {
        return Err(ChatFailure::InvalidSnapshot);
    }
    Ok(prompt)
}

/// Pure bounded scan of decoded JSON keys and strings, including JSON embedded
/// in chat message content. Every map entry is visited, even duplicate keys.
/// The marker is never returned, formatted into an error, or written to a log.
pub fn response_contains_marker(raw_body: &str, marker: &str) -> Result<bool, ChatFailure> {
    if raw_body.len() > MAX_CHAT_RESPONSE_BYTES {
        return Err(ChatFailure::ResponseTooLarge);
    }
    if marker.is_empty() {
        return Err(ChatFailure::InvalidConfiguration);
    }
    if raw_body.contains(marker) {
        return Ok(true);
    }
    scan_json_strings(raw_body, marker, 0).map_err(|_| ChatFailure::InvalidResponse)
}

/// Rejects an authentication-marker echo anywhere in the complete serialized
/// request, including configured model metadata and system instructions.
pub fn validate_public_request_body(body: &str, marker: &str) -> Result<(), ChatFailure> {
    if response_contains_marker(body, marker).map_err(|_| ChatFailure::InvalidSnapshot)? {
        return Err(ChatFailure::InvalidSnapshot);
    }
    Ok(())
}

fn scan_json_strings(
    text: &str,
    marker: &str,
    nested_strings: u8,
) -> Result<bool, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let found = StringScan { marker, nested_strings }.deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(found)
}

#[derive(Clone, Copy)]
struct StringScan<'a> {
    marker: &'a str,
    nested_strings: u8,
}

impl<'de> DeserializeSeed<'de> for StringScan<'_> {
    type Value = bool;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<bool, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for StringScan<'_> {
    type Value = bool;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded JSON")
    }

    fn visit_bool<E: de::Error>(self, _value: bool) -> Result<bool, E> {
        Ok(false)
    }

    fn visit_i64<E: de::Error>(self, _value: i64) -> Result<bool, E> {
        Ok(false)
    }

    fn visit_u64<E: de::Error>(self, _value: u64) -> Result<bool, E> {
        Ok(false)
    }

    fn visit_f64<E: de::Error>(self, _value: f64) -> Result<bool, E> {
        Ok(false)
    }

    fn visit_unit<E: de::Error>(self) -> Result<bool, E> {
        Ok(false)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<bool, E> {
        if value.contains(self.marker) {
            return Ok(true);
        }
        // A chat content string contains another JSON object. Decode that tree
        // too, so inner rationale/reason Unicode escapes cannot hide an echo.
        if value.trim_start().starts_with(['{', '[']) {
            if self.nested_strings >= 8 {
                return Err(E::custom("nested JSON strings exceed the scan limit"));
            }
            return scan_json_strings(value, self.marker, self.nested_strings + 1)
                .map_err(|_| E::custom("nested JSON strings could not be scanned"));
        }
        Ok(false)
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<bool, E> {
        self.visit_str(&value)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut values: A) -> Result<bool, A::Error> {
        let mut found = false;
        while let Some(value) = values.next_element_seed(self)? {
            found |= value;
        }
        Ok(found)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut values: A) -> Result<bool, A::Error> {
        let mut found = false;
        while let Some(name) = values.next_key::<String>()? {
            found |= name.contains(self.marker);
            found |= values.next_value_seed(self)?;
        }
        Ok(found)
    }
}

/// The production configuration is loaded only by this explicit runtime factory.
/// No endpoint, provider or model name is selected by a fallback.
pub struct HttpChatClient {
    endpoint: reqwest::Url,
    model_name: String,
    authorization: reqwest::header::HeaderValue,
    api_key: String,
    client: reqwest::Client,
    signature_base: Option<String>,
    verification_policy: VerificationPolicy,
}

impl HttpChatClient {
    pub fn from_env() -> Result<Self, ChatFailure> {
        let read = |name| std::env::var(name).map_err(|_| ChatFailure::MissingConfiguration);
        let endpoint_text = read("MODEL_ENDPOINT_URL")?;
        let model_name = read("MODEL_NAME")?;
        let api_key = read("MODEL_API_KEY")?;
        let signature_base = optional_model_setting(std::env::var("MODEL_SIGNATURE_URL"))?;
        let pinned_address = optional_model_setting(std::env::var("MODEL_SIGNING_ADDRESS"))?;
        let required =
            match optional_model_setting(std::env::var("MODEL_SIGNATURE_REQUIRED"))?.as_deref() {
                None | Some("false") => false,
                Some("true") => true,
                _ => return Err(ChatFailure::InvalidConfiguration),
            };
        if !bounded_identity(&model_name, 64)
            || [MANUAL_SOURCE_ID, REBALANCE_SOURCE_ID].contains(&model_name.as_str())
            || api_key.trim().is_empty()
            || api_key.len() > 4096
            || endpoint_text.contains(&api_key)
            || signature_base.as_ref().is_some_and(|base| base.contains(&api_key))
        {
            return Err(ChatFailure::InvalidConfiguration);
        }
        if pinned_address.as_ref().is_some_and(|address| {
            address.len() != 42
                || !address.starts_with("0x")
                || !address.as_bytes()[2..].iter().all(u8::is_ascii_hexdigit)
        }) || ((required || signature_base.is_some()) && pinned_address.is_none())
            || (required && signature_base.is_none())
        {
            return Err(ChatFailure::InvalidConfiguration);
        }
        if let Some(base) = &signature_base {
            signature_lookup_url(&endpoint_text, base, "configuration-check", &model_name)?;
        }
        let endpoint =
            reqwest::Url::parse(&endpoint_text).map_err(|_| ChatFailure::InvalidConfiguration)?;
        if endpoint_text.len() > 2048
            || endpoint.scheme() != "https"
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(ChatFailure::InvalidConfiguration);
        }
        let mut authorization =
            reqwest::header::HeaderValue::from_str(&format!("Bearer {api_key}"))
                .map_err(|_| ChatFailure::InvalidConfiguration)?;
        authorization.set_sensitive(true);
        let client = reqwest::Client::builder()
            .timeout(MODEL_REQUEST_TIMEOUT)
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ChatFailure::InvalidConfiguration)?;
        Ok(Self {
            endpoint,
            model_name,
            authorization,
            api_key,
            client,
            signature_base,
            verification_policy: VerificationPolicy { required, pinned_address },
        })
    }

    async fn complete_bounded(&self, prompt: String) -> Result<ChatResponse, ChatFailure> {
        if prompt.len() > MAX_CHAT_RESPONSE_BYTES {
            return Err(ChatFailure::InvalidSnapshot);
        }
        if response_contains_marker(&prompt, &self.api_key)
            .map_err(|_| ChatFailure::InvalidSnapshot)?
        {
            return Err(ChatFailure::InvalidSnapshot);
        }
        // Zero retries: malformed outputs and failed transport never trigger a
        // repair loop or a second model request.
        // The exact bytes sent are hashed, so a verifier can match them to the record.
        let request_body = serde_json::to_string(&json!({
            "model":self.model_name,
            "messages":[
                {"role":"system","content":MODEL_INSTRUCTIONS},
                {"role":"user","content":prompt}
            ]
        }))
        .map_err(|_| ChatFailure::InvalidSnapshot)?;
        if request_body.len() > MAX_CHAT_RESPONSE_BYTES {
            return Err(ChatFailure::InvalidSnapshot);
        }
        validate_public_request_body(&request_body, &self.api_key)?;
        let response = self
            .client
            .post(self.endpoint.clone())
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(request_body.clone())
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() { ChatFailure::Timeout } else { ChatFailure::Transport }
            })?;
        let raw_body = read_bounded_response(response).await?;
        // Do not retain a provider response that echoes the configured secret.
        if response_contains_marker(&raw_body, &self.api_key)? {
            return Err(ChatFailure::InvalidResponse);
        }
        let mut capture = ChatResponse::from_raw(raw_body, request_body)?;
        if capture.signature_proof.is_none()
            && let Some(base) = &self.signature_base
        {
            let url =
                signature_lookup_url(self.endpoint.as_str(), base, &capture.id, &capture.model)?;
            let proof = self
                .client
                .get(url)
                .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
                .send()
                .await
                .map_err(transport_failure);
            let proof = match proof {
                Ok(response) => read_bounded_response(response).await,
                Err(error) => Err(error),
            }?;
            if response_contains_marker(&proof, &self.api_key)? {
                return Err(ChatFailure::InvalidProof);
            }
            capture.signature_proof = Some(proof);
        }
        Ok(capture)
    }
}

impl ChatClient for HttpChatClient {
    fn requested_model(&self) -> &str {
        &self.model_name
    }

    fn verification_policy(&self) -> VerificationPolicy {
        self.verification_policy.clone()
    }

    async fn complete(&self, prompt: String) -> Result<ChatResponse, ChatFailure> {
        // Inference plus the single proof lookup share the original time budget.
        tokio::time::timeout(MODEL_REQUEST_TIMEOUT, self.complete_bounded(prompt))
            .await
            .map_err(|_| ChatFailure::Timeout)?
    }
}

fn transport_failure(error: reqwest::Error) -> ChatFailure {
    if error.is_timeout() { ChatFailure::Timeout } else { ChatFailure::Transport }
}

async fn read_bounded_response(mut response: reqwest::Response) -> Result<String, ChatFailure> {
    if !response.status().is_success() {
        return Err(ChatFailure::HttpStatus(response.status().as_u16()));
    }
    if response.content_length().is_some_and(|length| length > MAX_CHAT_RESPONSE_BYTES as u64) {
        return Err(ChatFailure::ResponseTooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport_failure)? {
        if chunk.len() > MAX_CHAT_RESPONSE_BYTES.saturating_sub(bytes.len()) {
            return Err(ChatFailure::ResponseTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| ChatFailure::InvalidResponse)
}

/// Pure construction of the documented signature GET URL. The explicit base
/// ends in `/`; the returned chat ID is appended as one encoded path segment.
pub fn signature_lookup_url(
    chat_endpoint: &str,
    signature_base: &str,
    response_id: &str,
    model: &str,
) -> Result<reqwest::Url, ChatFailure> {
    let invalid = || ChatFailure::InvalidConfiguration;
    if chat_endpoint.len() > 2048
        || signature_base.len() > 2048
        || !signature_base.ends_with('/')
        || !bounded_identity(response_id, 128)
        || [".", ".."].contains(&response_id)
        || !bounded_identity(model, 64)
    {
        return Err(invalid());
    }
    let chat = reqwest::Url::parse(chat_endpoint).map_err(|_| invalid())?;
    let mut signature = reqwest::Url::parse(signature_base).map_err(|_| invalid())?;
    for url in [&chat, &signature] {
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
    }
    if chat.origin() != signature.origin() {
        return Err(invalid());
    }
    signature.path_segments_mut().map_err(|_| invalid())?.pop_if_empty().push(response_id);
    signature.query_pairs_mut().append_pair("signing_algo", "ecdsa").append_pair("model", model);
    Ok(signature)
}
