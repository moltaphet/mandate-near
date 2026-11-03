//! Sandbox-only stand-in for the MPC signer (`v1.signer`).
//!
//! It exists so tests can run the vault against the real `intents.near`
//! code without a live MPC network. It is **not** part of the product and is
//! never deployed to a real network.
//!
//! `derived_public_key` enforces the same calling convention the vault must
//! follow against the real contract (path, predecessor, Ed25519 domain), so a
//! test fails if the vault asks for the wrong key.

use near_sdk::{AccountId, PanicOnDefault, env, near, require};

/// Path the vault derives its Intents key with; also used by the R1 spike.
pub const EXPECTED_PATH: &str = "intents";
/// Domain 1 is Ed25519 on `v1.signer`.
pub const ED25519_DOMAIN_ID: u32 = 1;

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct MockExternals {
    derived_key: String,
    expected_predecessor: AccountId,
}

#[near]
impl MockExternals {
    #[init]
    pub fn new(derived_key: String, expected_predecessor: AccountId) -> Self {
        Self { derived_key, expected_predecessor }
    }

    /// Mirrors `v1.signer.derived_public_key(path, predecessor, domain_id)`.
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
        self.derived_key.clone()
    }
}
