//! Builds the exact NEAR Intents payload the vault asks MPC to sign.
//!
//! Standard: `raw_ed25519`. The verifier checks Ed25519 over the UTF-8 bytes
//! of this string (proven in docs/spikes/R1.md), so the string produced here
//! *is* the commitment: amounts, deadline and nonce cannot change after
//! signing.
//!
//! Field order is alphabetical, matching the payloads accepted on mainnet in R1.

use std::collections::BTreeMap;
use std::fmt;

use near_sdk::base64::Engine;
use near_sdk::base64::engine::general_purpose::STANDARD as BASE64;
use near_sdk::serde::Serialize;
use near_sdk::serde_json;

/// First 4 bytes of a versioned nonce (`near/intents`
/// `contracts/defuse/core/src/nonce/versioned.rs`).
pub const VERSIONED_NONCE_PREFIX: [u8; 4] = [0x56, 0x28, 0xf6, 0xc6];
/// Borsh enum index of `VersionedNonce::V1`.
const NONCE_V1: u8 = 0;
/// The nonce stores the deadline as i64 nanoseconds.
pub const MAX_DEADLINE_SEC: u64 = (i64::MAX / 1_000_000_000) as u64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntentError {
    SameAsset,
    /// `token_diff` amounts are i128 on the verifier.
    AmountTooLarge,
    DeadlineOutOfRange,
}

impl fmt::Display for IntentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SameAsset => write!(f, "token_diff needs two different assets"),
            Self::AmountTooLarge => write!(f, "amount does not fit in i128"),
            Self::DeadlineOutOfRange => write!(f, "deadline beyond {MAX_DEADLINE_SEC}"),
        }
    }
}

/// V1 versioned nonce, 32 bytes:
/// `5628f6c6 ‖ 00 ‖ salt (4) ‖ deadline ns (i64 LE, 8) ‖ random (15)`.
///
/// `salt` must be one `intents.near` currently accepts (`current_salt`).
/// The verifier rejects the nonce after `deadline_sec`, so used nonces can
/// be garbage-collected instead of stored forever (legacy random nonces
/// are slated for removal upstream).
pub fn v1_nonce(
    salt: [u8; 4],
    deadline_sec: u64,
    random: [u8; 15],
) -> Result<[u8; 32], IntentError> {
    if deadline_sec > MAX_DEADLINE_SEC {
        return Err(IntentError::DeadlineOutOfRange);
    }
    let deadline_ns = (deadline_sec as i64) * 1_000_000_000;
    let mut nonce = [0u8; 32];
    nonce[0..4].copy_from_slice(&VERSIONED_NONCE_PREFIX);
    nonce[4] = NONCE_V1;
    nonce[5..9].copy_from_slice(&salt);
    nonce[9..17].copy_from_slice(&deadline_ns.to_le_bytes());
    nonce[17..32].copy_from_slice(&random);
    Ok(nonce)
}

/// `YYYY-MM-DDTHH:MM:SSZ` for a Unix timestamp (proleptic Gregorian,
/// algorithm from H. Hinnant, "chrono-Compatible Low-Level Date Algorithms").
pub fn rfc3339_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let sec_of_day = secs % 86_400;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        sec_of_day / 3_600,
        sec_of_day % 3_600 / 60,
        sec_of_day % 60
    )
}

/// A swap the vault commits to: lose `amount_in` of `asset_in`, gain at
/// least `min_amount_out` of `asset_out`, before `deadline_sec`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenDiff<'a> {
    pub signer_id: &'a str,
    pub verifying_contract: &'a str,
    pub deadline_sec: u64,
    pub nonce: [u8; 32],
    pub asset_in: &'a str,
    pub amount_in: u128,
    pub asset_out: &'a str,
    pub min_amount_out: u128,
}

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
struct Payload<'a> {
    deadline: String,
    intents: [Intent<'a>; 1],
    nonce: String,
    signer_id: &'a str,
    verifying_contract: &'a str,
}

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
struct Intent<'a> {
    diff: BTreeMap<&'a str, String>,
    intent: &'static str,
}

/// The exact string to sign with `raw_ed25519`.
pub fn token_diff_payload(t: &TokenDiff<'_>) -> Result<String, IntentError> {
    if t.asset_in == t.asset_out {
        return Err(IntentError::SameAsset);
    }
    let amount_in = i128::try_from(t.amount_in).map_err(|_| IntentError::AmountTooLarge)?;
    let amount_out = i128::try_from(t.min_amount_out).map_err(|_| IntentError::AmountTooLarge)?;
    if t.deadline_sec > MAX_DEADLINE_SEC {
        return Err(IntentError::DeadlineOutOfRange);
    }
    let payload = Payload {
        deadline: rfc3339_utc(t.deadline_sec),
        intents: [Intent {
            diff: BTreeMap::from([
                (t.asset_in, (-amount_in).to_string()),
                (t.asset_out, amount_out.to_string()),
            ]),
            intent: "token_diff",
        }],
        nonce: BASE64.encode(t.nonce),
        signer_id: t.signer_id,
        verifying_contract: t.verifying_contract,
    };
    Ok(serde_json::to_string(&payload).expect("payload serialization cannot fail"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mandate::tests::{USDC, WNEAR};
    use proptest::prelude::*;

    const SALT: [u8; 4] = [0x25, 0x28, 0x12, 0xb3];
    const RANDOM: [u8; 15] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    /// 2026-10-31T12:02:00Z.
    const DEADLINE: u64 = 1_793_448_120;

    #[test]
    fn rfc3339_known_dates() {
        // Vectors computed independently with Python's datetime.
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339_utc(1_793_448_000), "2026-10-31T12:00:00Z");
        assert_eq!(rfc3339_utc(1_793_466_051), "2026-10-31T17:00:51Z");
        assert_eq!(rfc3339_utc(4_102_444_800), "2100-01-01T00:00:00Z");
    }

    #[test]
    fn v1_nonce_layout() {
        let n = v1_nonce(SALT, DEADLINE, RANDOM).unwrap();
        assert_eq!(&n[0..4], &VERSIONED_NONCE_PREFIX);
        assert_eq!(n[4], 0);
        assert_eq!(&n[5..9], &SALT);
        assert_eq!(
            i64::from_le_bytes(n[9..17].try_into().unwrap()),
            DEADLINE as i64 * 1_000_000_000
        );
        assert_eq!(&n[17..], &RANDOM);
        // Independent vector (Python).
        assert_eq!(BASE64.encode(n), "Vij2xgAlKBKzADCxI5Cb4xgBAgMEBQYHCAkKCwwNDg8=");
    }

    #[test]
    fn v1_nonce_deadline_bound() {
        assert!(v1_nonce(SALT, MAX_DEADLINE_SEC, RANDOM).is_ok());
        assert_eq!(
            v1_nonce(SALT, MAX_DEADLINE_SEC + 1, RANDOM),
            Err(IntentError::DeadlineOutOfRange)
        );
    }

    fn sample<'a>() -> TokenDiff<'a> {
        TokenDiff {
            signer_id: "vault.near",
            verifying_contract: "intents.near",
            deadline_sec: DEADLINE,
            nonce: v1_nonce(SALT, DEADLINE, RANDOM).unwrap(),
            asset_in: USDC,
            amount_in: 20_000_000,
            asset_out: WNEAR,
            min_amount_out: 3_970_000_000_000_000_000_000_000,
        }
    }

    #[test]
    fn golden_payload_matches_independent_encoding() {
        // Python: json.dumps(payload, separators=(',', ':'), sort_keys=True)
        let expected = concat!(
            r#"{"deadline":"2026-10-31T12:02:00Z","intents":[{"diff":{"#,
            r#""nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1":"-20000000","#,
            r#""nep141:wrap.near":"3970000000000000000000000"},"intent":"token_diff"}],"#,
            r#""nonce":"Vij2xgAlKBKzADCxI5Cb4xgBAgMEBQYHCAkKCwwNDg8=","#,
            r#""signer_id":"vault.near","verifying_contract":"intents.near"}"#
        );
        assert_eq!(token_diff_payload(&sample()).unwrap(), expected);
    }

    #[test]
    fn payload_fits_mpc_eddsa_limit() {
        // MPC v1.signer accepts EdDSA payloads up to 1232 bytes (R1, F6).
        let longest = TokenDiff {
            signer_id: &"a".repeat(64),
            asset_in: "nep245:v2_1.omni.hot.tg:56_11111111111111111111",
            amount_in: i128::MAX as u128,
            min_amount_out: i128::MAX as u128,
            ..sample()
        };
        assert!(token_diff_payload(&longest).unwrap().len() <= 1232);
    }

    #[test]
    fn rejects_invalid_diffs() {
        assert_eq!(
            token_diff_payload(&TokenDiff { asset_out: USDC, ..sample() }),
            Err(IntentError::SameAsset)
        );
        assert_eq!(
            token_diff_payload(&TokenDiff { amount_in: i128::MAX as u128 + 1, ..sample() }),
            Err(IntentError::AmountTooLarge)
        );
        assert_eq!(
            token_diff_payload(&TokenDiff { min_amount_out: u128::MAX, ..sample() }),
            Err(IntentError::AmountTooLarge)
        );
        assert_eq!(
            token_diff_payload(&TokenDiff { deadline_sec: MAX_DEADLINE_SEC + 1, ..sample() }),
            Err(IntentError::DeadlineOutOfRange)
        );
    }

    proptest! {
        /// Our calendar math agrees with chrono for every second up to the
        /// nonce's deadline limit (year 2262).
        #[test]
        fn rfc3339_matches_chrono(secs in 0u64..=MAX_DEADLINE_SEC) {
            let expected = chrono::DateTime::from_timestamp(secs as i64, 0)
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            prop_assert_eq!(rfc3339_utc(secs), expected);
        }

        /// The diff always nets `-amount_in` / `+min_amount_out`, whatever the order of ids.
        #[test]
        fn diff_signs_are_correct(amount_in in 1u128..=i128::MAX as u128, out in 1u128..=i128::MAX as u128) {
            let p = token_diff_payload(&TokenDiff { amount_in, min_amount_out: out, ..sample() }).unwrap();
            let v: serde_json::Value = serde_json::from_str(&p).unwrap();
            let diff = &v["intents"][0]["diff"];
            prop_assert_eq!(diff[USDC].as_str().unwrap(), format!("-{amount_in}"));
            prop_assert_eq!(diff[WNEAR].as_str().unwrap(), out.to_string());
        }
    }
}
