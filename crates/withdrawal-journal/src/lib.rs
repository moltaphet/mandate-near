//! Shared request commitments and strict wire types for the proposed executor journal.
//! These types validate bindings; they do not authenticate executor code or outcomes.

#![forbid(unsafe_code)]

use borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::{
    AccountId,
    json_types::{U64, U128},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Visitor};
use sha2::{Digest, Sha256};
use std::fmt;

pub const PROTOCOL_VERSION: &str = "durable_withdrawal_journal_v1";
pub const REQUEST_HASH_SCHEME: &str = "sha256-borsh-v1";
pub const MAX_JOURNAL_RESPONSE_BYTES: usize = 4096;
const REQUEST_HASH_DOMAIN: &[u8] = b"mandate-withdrawal-journal-request-v1";

/// A fixed Borsh byte array with a strictly lowercase hexadecimal JSON encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Hex32([u8; 32]);

impl Hex32 {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn bytes(&self) -> [u8; 32] {
        self.0
    }
}

impl Serialize for Hex32 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        const ALPHABET: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = [0u8; 64];
        for (index, byte) in self.0.iter().enumerate() {
            encoded[index * 2] = ALPHABET[(byte >> 4) as usize];
            encoded[index * 2 + 1] = ALPHABET[(byte & 15) as usize];
        }
        serializer.serialize_str(std::str::from_utf8(&encoded).expect("hex encoding is ASCII"))
    }
}

impl<'de> Deserialize<'de> for Hex32 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct HexVisitor;
        impl<'de> Visitor<'de> for HexVisitor {
            type Value = Hex32;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a lowercase 64-character hexadecimal digest")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Hex32, E> {
                let raw = value.as_bytes();
                if raw.len() != 64 {
                    return Err(E::custom("invalid 32-byte hexadecimal value"));
                }
                let nibble = |byte: u8| match byte {
                    b'0'..=b'9' => Some(byte - b'0'),
                    b'a'..=b'f' => Some(byte - b'a' + 10),
                    _ => None,
                };
                let mut decoded = [0u8; 32];
                for (index, pair) in raw.chunks_exact(2).enumerate() {
                    let high = nibble(pair[0])
                        .ok_or_else(|| E::custom("invalid 32-byte hexadecimal value"))?;
                    let low = nibble(pair[1])
                        .ok_or_else(|| E::custom("invalid 32-byte hexadecimal value"))?;
                    decoded[index] = high << 4 | low;
                }
                Ok(Hex32(decoded))
            }
        }
        deserializer.deserialize_str(HexVisitor)
    }
}

#[cfg(feature = "abi")]
impl schemars::JsonSchema for Hex32 {
    fn schema_name() -> String {
        "Hex32".into()
    }

    fn json_schema(_: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
        use schemars::schema::{InstanceType, Schema, SchemaObject, StringValidation};
        Schema::Object(SchemaObject {
            instance_type: Some(InstanceType::String.into()),
            string: Some(Box::new(StringValidation {
                min_length: Some(64),
                max_length: Some(64),
                pattern: Some("^[0-9a-f]{64}$".into()),
            })),
            ..Default::default()
        })
    }
}

#[cfg_attr(feature = "abi", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalWithdrawalRequest {
    pub withdrawal_id: U64,
    pub request_seq: U64,
    pub token: AccountId,
    pub receiver_id: AccountId,
    pub amount: U128,
}

impl JournalWithdrawalRequest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.withdrawal_id.0 == 0 {
            return Err(ProtocolError::InvalidWithdrawalId);
        }
        if self.request_seq.0 == 0 {
            return Err(ProtocolError::InvalidRequestSequence);
        }
        if self.amount.0 == 0 {
            return Err(ProtocolError::InvalidAmount);
        }
        Ok(())
    }
}

#[cfg_attr(feature = "abi", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalProtocolDescriptor {
    pub protocol_version: String,
    pub ledger_domain: Hex32,
    pub executor_id: AccountId,
    pub request_hash_scheme: String,
}

impl JournalProtocolDescriptor {
    pub fn validate(&self, expected_executor: &AccountId) -> Result<(), ProtocolError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedProtocol);
        }
        if self.request_hash_scheme != REQUEST_HASH_SCHEME {
            return Err(ProtocolError::UnsupportedHashScheme);
        }
        if self.ledger_domain.bytes() == [0; 32] {
            return Err(ProtocolError::EmptyLedgerDomain);
        }
        if &self.executor_id != expected_executor {
            return Err(ProtocolError::ExecutorMismatch);
        }
        Ok(())
    }
}

#[cfg_attr(feature = "abi", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[serde(deny_unknown_fields)]
pub enum JournalWithdrawalState {
    Pending,
    Delivered { delivered_amount: U128 },
    Refunded { refunded_amount: U128 },
    Cancelled,
}

#[cfg_attr(feature = "abi", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalWithdrawalRecord {
    pub protocol_version: String,
    pub ledger_domain: Hex32,
    pub executor_id: AccountId,
    pub vault_id: AccountId,
    pub request: JournalWithdrawalRequest,
    pub request_hash: Hex32,
    pub state: JournalWithdrawalState,
}

impl JournalWithdrawalRecord {
    pub fn new(
        descriptor: &JournalProtocolDescriptor,
        vault_id: AccountId,
        request: JournalWithdrawalRequest,
        state: JournalWithdrawalState,
    ) -> Result<Self, ProtocolError> {
        descriptor.validate(&descriptor.executor_id)?;
        request.validate()?;
        let record = Self {
            protocol_version: descriptor.protocol_version.clone(),
            ledger_domain: descriptor.ledger_domain,
            executor_id: descriptor.executor_id.clone(),
            vault_id: vault_id.clone(),
            request_hash: canonical_hash(descriptor, &vault_id, &request),
            request: request.clone(),
            state,
        };
        record.validate(descriptor, &vault_id, &request)?;
        Ok(record)
    }

    /// A match requires a valid descriptor, exact binding and valid terminal amounts.
    pub fn matches_request(
        &self,
        descriptor: &JournalProtocolDescriptor,
        vault_id: &AccountId,
        request: &JournalWithdrawalRequest,
    ) -> bool {
        self.validate(descriptor, vault_id, request).is_ok()
    }

    pub fn validate(
        &self,
        descriptor: &JournalProtocolDescriptor,
        vault_id: &AccountId,
        request: &JournalWithdrawalRequest,
    ) -> Result<(), ProtocolError> {
        descriptor.validate(&self.executor_id)?;
        request.validate()?;
        self.request.validate()?;
        if self.protocol_version != descriptor.protocol_version
            || self.ledger_domain != descriptor.ledger_domain
            || &self.vault_id != vault_id
            || &self.request != request
        {
            return Err(ProtocolError::RequestBindingMismatch);
        }
        if self.request_hash != canonical_hash(descriptor, vault_id, request) {
            return Err(ProtocolError::RequestHashMismatch);
        }
        match &self.state {
            JournalWithdrawalState::Delivered { delivered_amount }
                if *delivered_amount != request.amount =>
            {
                Err(ProtocolError::TerminalAmountMismatch)
            }
            JournalWithdrawalState::Refunded { refunded_amount }
                if *refunded_amount != request.amount =>
            {
                Err(ProtocolError::TerminalAmountMismatch)
            }
            _ => Ok(()),
        }
    }
}

/// SHA256 over the fixed domain plus the v1 Borsh tuple; contains no NEAR host calls.
/// Descriptor/request validation is separate, so callers must validate their context.
pub fn canonical_hash(
    descriptor: &JournalProtocolDescriptor,
    vault_id: &AccountId,
    request: &JournalWithdrawalRequest,
) -> Hex32 {
    let encoded = borsh::to_vec(&(
        &descriptor.protocol_version,
        descriptor.ledger_domain,
        &descriptor.executor_id,
        vault_id,
        request.withdrawal_id.0,
        request.request_seq.0,
        &request.token,
        &request.receiver_id,
        request.amount.0,
    ))
    .expect("fixed withdrawal tuple serialization must succeed");
    let mut hash = Sha256::new();
    hash.update(REQUEST_HASH_DOMAIN);
    hash.update([0]);
    hash.update(encoded);
    Hex32::from_bytes(hash.finalize().into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    InvalidWithdrawalId,
    InvalidRequestSequence,
    InvalidAmount,
    UnsupportedProtocol,
    UnsupportedHashScheme,
    EmptyLedgerDomain,
    ExecutorMismatch,
    RequestBindingMismatch,
    RequestHashMismatch,
    TerminalAmountMismatch,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidWithdrawalId => "withdrawal ID must be positive",
            Self::InvalidRequestSequence => "request sequence must be positive",
            Self::InvalidAmount => "withdrawal amount must be positive",
            Self::UnsupportedProtocol => "unsupported withdrawal journal protocol",
            Self::UnsupportedHashScheme => "unsupported withdrawal request hash scheme",
            Self::EmptyLedgerDomain => "withdrawal ledger domain must be nonzero",
            Self::ExecutorMismatch => "withdrawal executor does not match",
            Self::RequestBindingMismatch => "withdrawal request binding does not match",
            Self::RequestHashMismatch => "withdrawal request hash does not match",
            Self::TerminalAmountMismatch => "withdrawal terminal amount does not match",
        })
    }
}

impl std::error::Error for ProtocolError {}
