//! Sandbox-only stand-in for the MPC signer (`v1.signer`).
//!
//! It exists so tests can run the vault against the real `intents.near`
//! code without a live MPC network. It is **not** part of the product and is
//! never deployed to a real network.
//!
//! It enforces the same calling convention the vault must follow against the
//! real contract: the `intents` path, the Ed25519 domain, the vault as the
//! predecessor, and 1 yoctoNEAR on `sign`. Unlike the real network it holds a
//! test secret key, so the signatures it returns are real Ed25519 signatures
//! that `intents.near` verifies. The key is derived from the seed passed to
//! `new`, and the seed is a test value only.

use ed25519_dalek::{Signer as _, SigningKey};
use near_sdk::{AccountId, PanicOnDefault, assert_one_yocto, env, near, require};

/// Path the vault derives its Intents key with; also used by the R1 spike.
pub const EXPECTED_PATH: &str = "intents";
/// Domain 1 is Ed25519 on `v1.signer`.
pub const ED25519_DOMAIN_ID: u32 = 1;

#[near(serializers = [json])]
pub struct PayloadV2 {
    #[serde(rename = "Eddsa")]
    pub eddsa: String,
}

#[near(serializers = [json])]
pub struct SignRequest {
    pub path: String,
    pub payload_v2: PayloadV2,
    pub domain_id: u32,
}

/// Same shape as `v1.signer`'s response: `{"scheme": "Ed25519", "signature": [64 bytes]}`.
#[near(serializers = [json])]
pub struct SignatureResponse {
    pub scheme: String,
    pub signature: Vec<u8>,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct MockExternals {
    secret: [u8; 32],
    expected_predecessor: AccountId,
}

#[near]
impl MockExternals {
    /// `seed_hex`: 32 bytes as 64 hex characters. Test value only.
    #[init]
    pub fn new(seed_hex: String, expected_predecessor: AccountId) -> Self {
        let bytes = from_hex(&seed_hex).unwrap_or_else(|| env::panic_str("seed must be hex"));
        let secret: [u8; 32] =
            bytes.try_into().unwrap_or_else(|_| env::panic_str("seed must be 32 bytes"));
        Self { secret, expected_predecessor }
    }

    /// Mirrors `v1.signer.derived_public_key(path, predecessor, domain_id)`.
    /// Returns the public key of the test secret.
    pub fn derived_public_key(
        &self,
        path: String,
        predecessor: Option<AccountId>,
        domain_id: Option<u32>,
    ) -> String {
        require!(path == EXPECTED_PATH, "unexpected derivation path");
        require!(domain_id == Some(ED25519_DOMAIN_ID), "expected the Ed25519 domain (1)");
        let predecessor = predecessor.unwrap_or_else(env::predecessor_account_id);
        require!(predecessor == self.expected_predecessor, "key requested for the wrong account");
        self.public_key()
    }

    /// Mirrors `v1.signer.sign`: signs the 32-byte-or-longer EdDSA payload
    /// for the vault's derived key. Requires 1 yoctoNEAR, like the real signer.
    #[payable]
    pub fn sign(&mut self, request: SignRequest) -> SignatureResponse {
        assert_one_yocto();
        require!(
            env::predecessor_account_id() == self.expected_predecessor,
            "signing requested by the wrong account"
        );
        require!(request.path == EXPECTED_PATH, "unexpected derivation path");
        require!(request.domain_id == ED25519_DOMAIN_ID, "expected the Ed25519 domain (1)");
        let message = from_hex(&request.payload_v2.eddsa)
            .unwrap_or_else(|| env::panic_str("payload must be hex"));
        require!(message.len() <= 1232, "EdDSA payload exceeds 1232 bytes");
        let signature = SigningKey::from_bytes(&self.secret).sign(&message);
        SignatureResponse { scheme: "Ed25519".into(), signature: signature.to_bytes().to_vec() }
    }
}

impl MockExternals {
    fn public_key(&self) -> String {
        let key = SigningKey::from_bytes(&self.secret).verifying_key();
        format!("ed25519:{}", bs58::encode(key.as_bytes()).into_string())
    }
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}
