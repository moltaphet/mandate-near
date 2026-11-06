//! Provenance for model decisions: the outbox record and the offline check of a
//! NEAR AI Cloud chat signature against an address the operator pins.
//!
//! What this proves, when it passes: the stored response bytes hash to the
//! recorded value, and the signature was made by the pinned key over the exact
//! request and response hashes. What it does not prove: that the pinned key
//! belongs to a genuine TEE. That needs the attestation, which is not checked
//! here. Fetching the signature from the provider is not done here either.

use std::fmt;

use secp256k1::Message;
use secp256k1::Secp256k1;
use secp256k1::ecdsa::{RecoverableSignature, RecoveryId};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

use crate::model::{ChatResponse, is_sha256_hex, sha256_hex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvenanceError {
    MalformedRecord,
    RecordHashMismatch,
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

/// The outbox entry written for every proposed model decision. `verification` is
/// `UNVERIFIED` when the agent writes it; only the verifier can change what is proven.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRecord {
    pub request_sha256: String,
    pub response_sha256: String,
    pub response_id: String,
    pub model: String,
    pub raw_body: String,
    pub verification: String,
}

impl ModelRecord {
    pub fn from_response(response: &ChatResponse) -> Self {
        Self {
            request_sha256: response.request_sha256.clone(),
            response_sha256: response.response_sha256.clone(),
            response_id: response.id.clone(),
            model: response.model.clone(),
            raw_body: response.raw_body.clone(),
            verification: "UNVERIFIED".into(),
        }
    }

    /// Recomputes the response hash from the stored bytes.
    pub fn check_hashes(&self) -> Result<(), ProvenanceError> {
        if !is_sha256_hex(&self.request_sha256) || !is_sha256_hex(&self.response_sha256) {
            return Err(ProvenanceError::MalformedRecord);
        }
        if sha256_hex(self.raw_body.as_bytes()) != self.response_sha256 {
            return Err(ProvenanceError::RecordHashMismatch);
        }
        Ok(())
    }
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
