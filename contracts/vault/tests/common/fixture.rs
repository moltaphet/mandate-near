use sha2::{Digest, Sha256};

pub const INTENTS_FIXTURE_SIZE: usize = 1_297_551;
pub const INTENTS_FIXTURE_SHA256: &str =
    "a1afab98916d9f213746544dcf8e4e6bdb22c0be6bc5dfbd8ea8814c9922b547";

pub fn validate_intents_fixture(code: &[u8]) -> Result<(), &'static str> {
    if code.len() != INTENTS_FIXTURE_SIZE {
        return Err("Intents verifier fixture size does not match the reviewed fixture");
    }
    if format!("{:x}", Sha256::digest(code)) != INTENTS_FIXTURE_SHA256 {
        return Err("Intents verifier fixture SHA256 does not match the reviewed fixture");
    }
    Ok(())
}
