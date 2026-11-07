//! Captured model decision evidence and bounded, versioned response commitments.
//! Raw request/response hashes authenticate wire bytes; the reason hash commits
//! to canonical response JSON. A valid pinned signature authenticates those
//! bytes, not model/TEE origin, an authentic chain snapshot, gate approval,
//! on-chain inclusion or settlement.

use std::fmt;

use near_sdk::serde_json;
use secp256k1::Message;
use secp256k1::Secp256k1;
use secp256k1::ecdsa::{RecoverableSignature, RecoveryId};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::collections::BTreeMap;

use crate::Plan;
use crate::model::{
    ChatResponse, MAX_CHAT_RESPONSE_BYTES, extract_decision, is_sha256_hex, sha256_hex,
};

pub const CANONICAL_RESPONSE_VERSION: &str = "mandate-response-json-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvenanceError {
    MalformedRecord,
    RecordHashMismatch,
    RequestHashMismatch,
    CanonicalJson,
    CanonicalVersion,
    CanonicalHashMismatch,
    DecisionBinding,
    MissingSignature,
    MissingPin,
    MalformedSignature,
    SignatureAlgorithm,
    SignatureKind,
    SignatureText,
    SignatureEncoding,
    SignatureRecovery,
    MalformedAddress,
    SignerMismatch,
}

impl fmt::Display for ProvenanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MalformedRecord => "model record is malformed",
            Self::RecordHashMismatch => "stored response bytes do not match the recorded hash",
            Self::RequestHashMismatch => "stored request bytes do not match the recorded hash",
            Self::CanonicalJson => "model JSON contains unsupported values or duplicate fields",
            Self::CanonicalVersion => "canonical response version is unsupported",
            Self::CanonicalHashMismatch => "canonical response does not match the reason hash",
            Self::DecisionBinding => "stored decision does not match the response or proposal",
            Self::MissingSignature => "required response signature is missing",
            Self::MissingPin => "response signature has no externally pinned address",
            Self::MalformedSignature => "response signature document is malformed",
            Self::SignatureAlgorithm => "signature algorithm is not ecdsa",
            Self::SignatureKind => "signature kind is unknown",
            Self::SignatureText => "signed text does not match the record",
            Self::SignatureEncoding => "signature encoding is invalid",
            Self::SignatureRecovery => "signature could not be recovered",
            Self::MalformedAddress => "pinned or reported address is malformed",
            Self::SignerMismatch => "signature was not made by the pinned address",
        })
    }
}

impl std::error::Error for ProvenanceError {}

/// Canonical JSON value representation. Object keys use Rust string byte order,
/// which is lexicographic UTF-8 order; strings and array order are preserved.
#[derive(Serialize)]
#[serde(untagged)]
enum CanonicalValue {
    Null(()),
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    String(String),
    Array(Vec<CanonicalValue>),
    Object(BTreeMap<String, CanonicalValue>),
}

impl<'de> Deserialize<'de> for CanonicalValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CanonicalVisitor;
        impl<'de> Visitor<'de> for CanonicalVisitor {
            type Value = CanonicalValue;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded JSON with unique object fields and i64/u64 integers")
            }

            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(CanonicalValue::Null(()))
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(CanonicalValue::Bool(value))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(CanonicalValue::Signed(value))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(CanonicalValue::Unsigned(value))
            }

            fn visit_f64<E: de::Error>(self, _value: f64) -> Result<Self::Value, E> {
                Err(E::custom("floating point and exponent numbers are unsupported"))
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(CanonicalValue::String(value.to_owned()))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(CanonicalValue::String(value))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element()? {
                    values.push(value);
                }
                Ok(CanonicalValue::Array(values))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = BTreeMap::new();
                while let Some(name) = map.next_key::<String>()? {
                    if values.contains_key(&name) {
                        return Err(de::Error::custom("duplicate object field"));
                    }
                    values.insert(name, map.next_value()?);
                }
                Ok(CanonicalValue::Object(values))
            }
        }
        deserializer.deserialize_any(CanonicalVisitor)
    }
}

/// `mandate-response-json-v1` canonicalises the entire response envelope:
/// recursively sort decoded object keys in lexicographic UTF-8 byte order;
/// preserve array order and decoded string contents; use serde_json compact
/// UTF-8 JSON escaping and i64/u64 decimal integers; append no newline.
/// Reject duplicate decoded keys at every depth, floats, exponent notation,
/// unsupported integers, trailing data and inputs over 64 KiB. Strings containing
/// decision JSON remain strings: their exact contents are not recanonicalised.
/// This is a versioned project format, not RFC 8785. Raw wire hashes remain
/// separate and must be checked before using the canonical commitment.
pub fn canonical_response_bytes(raw_body: &str) -> Result<Vec<u8>, ProvenanceError> {
    if raw_body.len() > MAX_CHAT_RESPONSE_BYTES {
        return Err(ProvenanceError::CanonicalJson);
    }
    let parsed: CanonicalValue =
        serde_json::from_str(raw_body).map_err(|_| ProvenanceError::CanonicalJson)?;
    serde_json::to_vec(&parsed).map_err(|_| ProvenanceError::CanonicalJson)
}

pub fn canonical_response_hash(raw_body: &str) -> Result<String, ProvenanceError> {
    Ok(sha256_hex(&canonical_response_bytes(raw_body)?))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedPlan {
    pub asset_in: String,
    pub asset_out: String,
    pub amount_in: String,
    pub min_amount_out: String,
    pub rationale: String,
}

impl From<&Plan> for RecordedPlan {
    fn from(plan: &Plan) -> Self {
        Self {
            asset_in: plan.asset_in.clone(),
            asset_out: plan.asset_out.clone(),
            amount_in: plan.amount_in.0.to_string(),
            min_amount_out: plan.min_amount_out.0.to_string(),
            rationale: plan.rationale.clone(),
        }
    }
}

/// Full proposed decision evidence. `verification` is descriptive metadata only;
/// verification always recomputes the binding and checks proof against an
/// external pin. It never trusts the stored status or stored signing address.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRecord {
    pub request_body: String,
    pub request_sha256: String,
    pub response_sha256: String,
    pub response_id: String,
    pub model: String,
    pub raw_body: String,
    pub canonical_version: String,
    pub reason_hash: String,
    pub plan: RecordedPlan,
    pub signature_proof: Option<String>,
    pub verification: String,
}

impl ModelRecord {
    pub fn from_response(response: &ChatResponse, plan: &Plan) -> Result<Self, ProvenanceError> {
        let record = Self {
            request_body: response.request_body.clone(),
            request_sha256: response.request_sha256.clone(),
            response_sha256: response.response_sha256.clone(),
            response_id: response.id.clone(),
            model: response.model.clone(),
            raw_body: response.raw_body.clone(),
            canonical_version: CANONICAL_RESPONSE_VERSION.into(),
            reason_hash: canonical_response_hash(&response.raw_body)?,
            plan: RecordedPlan::from(plan),
            signature_proof: response.signature_proof.clone(),
            verification: "UNVERIFIED".into(),
        };
        record.check_hashes()?;
        if plan.response_commitment.as_deref() != Some(record.reason_hash.as_str()) {
            return Err(ProvenanceError::DecisionBinding);
        }
        record.check_plan(plan)?;
        let captured =
            ChatResponse::from_raw(response.raw_body.clone(), response.request_body.clone())
                .map_err(|_| ProvenanceError::MalformedRecord)?;
        if captured.content != response.content {
            return Err(ProvenanceError::DecisionBinding);
        }
        Ok(record)
    }

    /// Recomputes both wire hashes, canonical hash and all response/plan bindings.
    /// This establishes record consistency, not signature validity or gate approval.
    pub fn check_hashes(&self) -> Result<(), ProvenanceError> {
        if !is_sha256_hex(&self.request_sha256)
            || !is_sha256_hex(&self.response_sha256)
            || !is_sha256_hex(&self.reason_hash)
            || self.request_body.len() > MAX_CHAT_RESPONSE_BYTES
            || self.raw_body.len() > MAX_CHAT_RESPONSE_BYTES
            || self
                .signature_proof
                .as_ref()
                .is_some_and(|proof| proof.len() > MAX_CHAT_RESPONSE_BYTES)
        {
            return Err(ProvenanceError::MalformedRecord);
        }
        if sha256_hex(self.request_body.as_bytes()) != self.request_sha256 {
            return Err(ProvenanceError::RequestHashMismatch);
        }
        if sha256_hex(self.raw_body.as_bytes()) != self.response_sha256 {
            return Err(ProvenanceError::RecordHashMismatch);
        }
        if self.canonical_version != CANONICAL_RESPONSE_VERSION {
            return Err(ProvenanceError::CanonicalVersion);
        }
        if canonical_response_hash(&self.raw_body)? != self.reason_hash {
            return Err(ProvenanceError::CanonicalHashMismatch);
        }
        let captured = ChatResponse::from_raw(self.raw_body.clone(), self.request_body.clone())
            .map_err(|_| ProvenanceError::MalformedRecord)?;
        if captured.id != self.response_id || captured.model != self.model {
            return Err(ProvenanceError::DecisionBinding);
        }
        if captured.signature_proof.is_some() && captured.signature_proof != self.signature_proof {
            return Err(ProvenanceError::MissingSignature);
        }
        let decision =
            extract_decision(&captured.content).map_err(|_| ProvenanceError::DecisionBinding)?;
        if decision.no_trade || RecordedPlan::from(&decision.plan) != self.plan {
            return Err(ProvenanceError::DecisionBinding);
        }
        Ok(())
    }

    pub fn check_plan(&self, plan: &Plan) -> Result<(), ProvenanceError> {
        if RecordedPlan::from(plan) != self.plan
            || plan.response_commitment.as_ref().is_some_and(|hash| hash != &self.reason_hash)
        {
            return Err(ProvenanceError::DecisionBinding);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerificationStatus {
    Unverified,
    VerifiedPinnedSigner,
}

impl VerificationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unverified => "UNVERIFIED",
            Self::VerifiedPinnedSigner => "VERIFIED_PINNED_SIGNER",
        }
    }
}

/// Any present signature must verify; absence is allowed only by explicit
/// optional policy. The pin comes from trusted runtime configuration, never
/// from the captured response. Call this before returning a model Trade.
pub fn verify_response(
    response: &ChatResponse,
    pinned_address: Option<&str>,
    required: bool,
) -> Result<VerificationStatus, ProvenanceError> {
    let mut plan =
        extract_decision(&response.content).map_err(|_| ProvenanceError::DecisionBinding)?.plan;
    plan.response_commitment = Some(canonical_response_hash(&response.raw_body)?);
    let record = ModelRecord::from_response(response, &plan)?;
    let Some(proof) = response.signature_proof.as_deref() else {
        return if required {
            Err(ProvenanceError::MissingSignature)
        } else {
            Ok(VerificationStatus::Unverified)
        };
    };
    let pinned = pinned_address.ok_or(ProvenanceError::MissingPin)?;
    let signature = parse_signature_proof(proof)?;
    verify_signature(&record, &signature, pinned)?;
    Ok(VerificationStatus::VerifiedPinnedSigner)
}

/// Parses a captured raw proof without collapsing duplicate fields or accepting
/// unsupported number encodings. Unknown algorithm/kind are rejected by verification.
pub fn parse_signature_proof(raw_proof: &str) -> Result<SignatureResponse, ProvenanceError> {
    let canonical =
        canonical_response_bytes(raw_proof).map_err(|_| ProvenanceError::MalformedSignature)?;
    let parsed: serde_json::Value =
        serde_json::from_slice(&canonical).map_err(|_| ProvenanceError::MalformedSignature)?;
    reject_unsupported_proof_fields(&parsed, true)?;
    serde_json::from_value(parsed).map_err(|_| ProvenanceError::MalformedSignature)
}

fn reject_unsupported_proof_fields(
    value: &serde_json::Value,
    root: bool,
) -> Result<(), ProvenanceError> {
    match value {
        serde_json::Value::Object(fields) => {
            for (name, value) in fields {
                if ["signatures", "proof", "signature_response", "chat_signature"]
                    .contains(&name.as_str())
                    || (name == "signature" && (!root || !value.is_string()))
                {
                    return Err(ProvenanceError::MalformedSignature);
                }
                reject_unsupported_proof_fields(value, false)?;
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                reject_unsupported_proof_fields(value, false)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// The signature document NEAR AI Cloud returns for a chat id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureResponse {
    pub text: String,
    pub signature: String,
    pub signing_address: String,
    #[serde(default)]
    pub signing_algo: Option<String>,
    #[serde(default)]
    pub signature_kind: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    ProviderTee,
    Gateway,
}

/// `provider_tee` signs `{model}:{request}:{response}`; `gateway` signs `{request}:{response}`.
/// A document without `signature_kind` is legacy: three fields mean provider_tee, two mean gateway.
fn kind_of(signature: &SignatureResponse) -> Result<Kind, ProvenanceError> {
    match signature.signature_kind.as_deref() {
        Some("provider_tee") => Ok(Kind::ProviderTee),
        Some("gateway") => Ok(Kind::Gateway),
        Some(_) => Err(ProvenanceError::SignatureKind),
        None => match signature.text.split(':').count() {
            3 => Ok(Kind::ProviderTee),
            2 => Ok(Kind::Gateway),
            _ => Err(ProvenanceError::SignatureKind),
        },
    }
}

fn expected_text(record: &ModelRecord, kind: Kind) -> String {
    match kind {
        Kind::ProviderTee => {
            format!("{}:{}:{}", record.model, record.request_sha256, record.response_sha256)
        }
        Kind::Gateway => format!("{}:{}", record.request_sha256, record.response_sha256),
    }
}

/// Checks the stored hashes and the signature against the pinned address. Returns
/// `Ok` only if every check passes; any other outcome must leave the record UNVERIFIED.
pub fn verify_signature(
    record: &ModelRecord,
    signature: &SignatureResponse,
    pinned_address: &str,
) -> Result<(), ProvenanceError> {
    record.check_hashes()?;
    if signature.signing_algo.as_deref().is_some_and(|algo| algo != "ecdsa") {
        return Err(ProvenanceError::SignatureAlgorithm);
    }
    let kind = kind_of(signature)?;
    if signature.text != expected_text(record, kind) {
        return Err(ProvenanceError::SignatureText);
    }
    let pinned = parse_address(pinned_address)?;
    if parse_address(&signature.signing_address)? != pinned {
        return Err(ProvenanceError::SignerMismatch);
    }
    if recover_address(&signature.text, &signature.signature)? != pinned {
        return Err(ProvenanceError::SignerMismatch);
    }
    Ok(())
}

/// Ethereum-style personal-message hash: keccak256 of the prefix, the byte length and the text.
fn personal_message_digest(text: &str) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(b"\x19Ethereum Signed Message:\n");
    hasher.update(text.len().to_string().as_bytes());
    hasher.update(text.as_bytes());
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&hasher.finalize());
    digest
}

fn recover_address(text: &str, signature_hex: &str) -> Result<[u8; 20], ProvenanceError> {
    let hex = signature_hex.strip_prefix("0x").unwrap_or(signature_hex);
    let bytes = decode_hex(hex)
        .filter(|bytes| bytes.len() == 65)
        .ok_or(ProvenanceError::SignatureEncoding)?;
    let recovery = match bytes[64] {
        v @ (0 | 1) => v,
        v @ (27 | 28) => v - 27,
        _ => return Err(ProvenanceError::SignatureEncoding),
    };
    let recovery_id = RecoveryId::from_i32(i32::from(recovery))
        .map_err(|_| ProvenanceError::SignatureEncoding)?;
    let signature = RecoverableSignature::from_compact(&bytes[..64], recovery_id)
        .map_err(|_| ProvenanceError::SignatureEncoding)?;
    let message = Message::from_slice(&personal_message_digest(text))
        .map_err(|_| ProvenanceError::SignatureRecovery)?;
    let public_key = Secp256k1::verification_only()
        .recover_ecdsa(&message, &signature)
        .map_err(|_| ProvenanceError::SignatureRecovery)?;
    let uncompressed = public_key.serialize_uncompressed();
    let hash = Keccak256::digest(&uncompressed[1..]);
    let mut address = [0u8; 20];
    address.copy_from_slice(&hash[12..]);
    Ok(address)
}

fn parse_address(value: &str) -> Result<[u8; 20], ProvenanceError> {
    let hex = value.strip_prefix("0x").ok_or(ProvenanceError::MalformedAddress)?;
    let bytes = decode_hex(hex)
        .filter(|bytes| bytes.len() == 20)
        .ok_or(ProvenanceError::MalformedAddress)?;
    let mut address = [0u8; 20];
    address.copy_from_slice(&bytes);
    Ok(address)
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).ok())
        .collect()
}
