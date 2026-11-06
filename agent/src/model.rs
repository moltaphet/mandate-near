//! Bounded model decisions. Captured provider responses have no provenance
//! verification in this step; a declared model name is not origin proof.

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
        }
    }
}

impl std::error::Error for ChatFailure {}

/// Unmodified UTF-8 HTTP response and the provider's declared response/model IDs.
/// No signature, attestation or model-origin verification is claimed.
#[derive(Clone)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub raw_body: String,
    pub content: String,
}

impl ChatResponse {
    pub fn from_raw(raw_body: String) -> Result<Self, ChatFailure> {
        if raw_body.len() > MAX_CHAT_RESPONSE_BYTES {
            return Err(ChatFailure::ResponseTooLarge);
        }
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
        Ok(Self {
            id: envelope.id,
            model: envelope.model,
            raw_body,
            content: choice.message.content,
        })
    }
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
        // The fakeable client cannot return content or attribution inconsistent
        // with the captured raw envelope. Reject it; do not repair it silently.
        let parsed = match ChatResponse::from_raw(response.raw_body.clone()) {
            Ok(parsed) => parsed,
            Err(error) => return no_trade(error.to_string()),
        };
        if parsed.id != response.id
            || parsed.model != response.model
            || parsed.content != response.content
        {
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
        let plan = validated.plan;
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
        Decision::Trade { plan, response: Some(response) }
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
    fn plan(&self, snapshot: &Snapshot) -> Result<Plan, &'static str> {
        if self.asset_in == self.asset_out {
            return Err("model decision assets must differ");
        }
        if snapshot.mandate.asset(&self.asset_in).is_none()
            || snapshot.mandate.asset(&self.asset_out).is_none()
        {
            return Err("model decision asset is outside the mandate");
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
    let output: ModelOutput = serde_json::from_str(content)
        .map_err(|_| "model decision must match the exact JSON schema")?;
    let plan = output.plan(snapshot)?;
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
}

impl HttpChatClient {
    pub fn from_env() -> Result<Self, ChatFailure> {
        let read = |name| std::env::var(name).map_err(|_| ChatFailure::MissingConfiguration);
        let endpoint_text = read("MODEL_ENDPOINT_URL")?;
        let model_name = read("MODEL_NAME")?;
        let api_key = read("MODEL_API_KEY")?;
        if !bounded_identity(&model_name, 64)
            || [MANUAL_SOURCE_ID, REBALANCE_SOURCE_ID].contains(&model_name.as_str())
            || api_key.trim().is_empty()
            || api_key.len() > 4096
            || endpoint_text.contains(&api_key)
        {
            return Err(ChatFailure::InvalidConfiguration);
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
        Ok(Self { endpoint, model_name, authorization, api_key, client })
    }
}

impl ChatClient for HttpChatClient {
    async fn complete(&self, prompt: String) -> Result<ChatResponse, ChatFailure> {
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
        let mut response = self
            .client
            .post(self.endpoint.clone())
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .json(&json!({
                "model":self.model_name,
                "messages":[
                    {"role":"system","content":MODEL_INSTRUCTIONS},
                    {"role":"user","content":prompt}
                ]
            }))
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() { ChatFailure::Timeout } else { ChatFailure::Transport }
            })?;
        if !response.status().is_success() {
            return Err(ChatFailure::HttpStatus(response.status().as_u16()));
        }
        if response.content_length().is_some_and(|length| length > MAX_CHAT_RESPONSE_BYTES as u64) {
            return Err(ChatFailure::ResponseTooLarge);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            if error.is_timeout() { ChatFailure::Timeout } else { ChatFailure::Transport }
        })? {
            if chunk.len() > MAX_CHAT_RESPONSE_BYTES.saturating_sub(bytes.len()) {
                return Err(ChatFailure::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        let raw_body = String::from_utf8(bytes).map_err(|_| ChatFailure::InvalidResponse)?;
        // Do not retain a provider response that echoes the configured secret.
        if response_contains_marker(&raw_body, &self.api_key)? {
            return Err(ChatFailure::InvalidResponse);
        }
        ChatResponse::from_raw(raw_body)
    }
}
