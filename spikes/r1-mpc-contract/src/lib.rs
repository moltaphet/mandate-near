//! R1 spike, part 2: a contract (not a wallet) requests an EdDSA signature
//! from the MPC network and receives it in a callback.
//!
//! Spike only: `request_signature` is open to anyone and signs any payload.
//! The real vault builds the payload itself and signs only after mandate checks.

use near_sdk::serde_json::{Value, json};
use near_sdk::{AccountId, Gas, NearToken, PanicOnDefault, Promise, PromiseError, env, near};

/// Domain 1 on `v1.signer` is Ed25519 (checked via `public_key` view).
const ED25519_DOMAIN_ID: u32 = 1;
/// `v1.signer` config: `sign_call_gas_attachment_requirement_tera_gas = 15`.
const SIGN_GAS: Gas = Gas::from_tgas(30);
const CALLBACK_GAS: Gas = Gas::from_tgas(10);

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Spike {
    signer: AccountId,
}

#[near]
impl Spike {
    #[init]
    pub fn new(signer: AccountId) -> Self {
        Self { signer }
    }

    /// `payload_hex`: bytes to sign (≤ 1232). `path`: key derivation path.
    pub fn request_signature(&mut self, payload_hex: String, path: String) -> Promise {
        let args = json!({
            "request": {
                "path": path,
                "payload_v2": { "Eddsa": payload_hex },
                "domain_id": ED25519_DOMAIN_ID,
            }
        });
        Promise::new(self.signer.clone())
            .function_call(
                "sign".to_string(),
                args.to_string().into_bytes(),
                NearToken::from_yoctonear(1),
                SIGN_GAS,
            )
            .then(Self::ext(env::current_account_id()).with_static_gas(CALLBACK_GAS).on_signature())
    }

    #[private]
    pub fn on_signature(&self, #[callback_result] result: Result<Value, PromiseError>) -> Value {
        match result {
            Ok(signature) => signature,
            Err(_) => env::panic_str("MPC sign failed or timed out"),
        }
    }
}
