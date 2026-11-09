//! Independent check of a vault's signed intent. Anyone can run it; it trusts no operator.
//!
//! Usage (testnet only in this version):
//!   cargo run -p mandate-vault --example verify_intent -- --vault <account>
//!   cargo run -p mandate-vault --example verify_intent -- --fixture <file.json> [--now-sec <unix>]
//!
//! Online mode reads `get_signed_intent`, `get_externals` and the signer's
//! `derived_public_key` over RPC. Offline mode checks a saved fixture of the same values and
//! makes no network call. Each check prints PASS or FAIL with the values it compared. The exit
//! status is 0 only when every check passes.
//!
//! Not covered: USD trade caps, oracle prices, and whether the Intents verifier accepted the
//! intent. Those need the mandate and the chain state at execution time.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use near_api::{Contract, NetworkConfig};
use near_sdk::serde_json::{self, Value, json};
use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

const SIGNER: &str = "v1.signer-prod.testnet";
const INTENTS_KEY_PATH: &str = "intents";
const ED25519_DOMAIN_ID: u32 = 1;

type R<T> = Result<T, Box<dyn Error>>;

/// Everything the checks need, whether it came from RPC or from a fixture.
struct Input {
    vault: String,
    intents: String,
    signed: Value,
    derived_public_key: String,
}

struct Check {
    name: &'static str,
    pass: bool,
    detail: String,
}

fn check(name: &'static str, pass: bool, detail: impl Into<String>) -> Check {
    Check { name, pass, detail: detail.into() }
}

/// Days from 1970-01-01 to the given civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Parses `YYYY-MM-DDTHH:MM:SSZ` (the form NEAR Intents uses) into Unix seconds.
fn parse_utc(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, se) = (n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3_600 + mi * 60 + se;
    u64::try_from(secs).ok()
}

/// Decodes an `ed25519:<base58>` string into its raw bytes.
fn decode_ed25519(s: &str) -> Option<Vec<u8>> {
    bs58::decode(s.strip_prefix("ed25519:")?).into_vec().ok()
}

fn run_checks(input: &Input, now: u64) -> Vec<Check> {
    let mut out = Vec::new();
    let signed = &input.signed;
    let payload_text = signed["payload"].as_str().unwrap_or_default();
    let public_key = signed["public_key"].as_str().unwrap_or_default();
    let signature = signed["signature"].as_str().unwrap_or_default();
    let expires_sec: Option<u64> =
        signed["expires_sec"].as_u64().or_else(|| signed["expires_sec"].as_str()?.parse().ok());

    out.push(check(
        "signed intent present",
        !payload_text.is_empty() && !signature.is_empty(),
        format!("proposal {}", signed["proposal_id"]),
    ));

    // The key must be the one the signer derives for this vault, not a key the caller chose.
    out.push(check(
        "public key equals the signer-derived key",
        !public_key.is_empty() && public_key == input.derived_public_key,
        format!("intent {public_key} | derived {}", input.derived_public_key),
    ));

    let pk_bytes = decode_ed25519(public_key);
    let sig_bytes = decode_ed25519(signature);
    let signature_ok = match (pk_bytes.as_deref(), sig_bytes.as_deref()) {
        (Some(pk), Some(sig)) if pk.len() == 32 && sig.len() == 64 => {
            let key: [u8; 32] = pk.try_into().expect("length checked");
            match (VerifyingKey::from_bytes(&key), Signature::from_slice(sig)) {
                (Ok(vk), Ok(sig)) => vk.verify(payload_text.as_bytes(), &sig).is_ok(),
                _ => false,
            }
        }
        _ => false,
    };
    out.push(check(
        "ed25519 signature verifies over the exact payload bytes",
        signature_ok,
        format!("{} payload bytes", payload_text.len()),
    ));

    let payload: Option<Value> = serde_json::from_str(payload_text).ok();
    out.push(check("payload is JSON", payload.is_some(), "parsed"));
    let payload = payload.unwrap_or(Value::Null);

    let signer_id = payload["signer_id"].as_str().unwrap_or_default();
    out.push(check(
        "payload signer is this vault",
        signer_id == input.vault,
        format!("signer_id {signer_id} | vault {}", input.vault),
    ));

    let verifying = payload["verifying_contract"].as_str().unwrap_or_default();
    out.push(check(
        "payload verifying contract is the Intents account",
        verifying == input.intents,
        format!("verifying_contract {verifying} | intents {}", input.intents),
    ));

    let deadline = payload["deadline"].as_str().and_then(parse_utc);
    out.push(check(
        "deadline parses as UTC",
        deadline.is_some(),
        format!("deadline {}", payload["deadline"]),
    ));
    if let Some(deadline) = deadline {
        out.push(check(
            "signed deadline equals the stored expiry",
            expires_sec == Some(deadline),
            format!("deadline {deadline} | expires_sec {expires_sec:?}"),
        ));
        out.push(check(
            "intent is not past its deadline",
            now <= deadline,
            format!("now {now} | deadline {deadline}"),
        ));
    }

    // Show what the intent moves, so a reader can compare it with the mandate by eye.
    let intents = payload["intents"].to_string();
    out.push(check("intent diffs are present", intents != "null", intents));
    out
}

fn report(checks: &[Check]) -> bool {
    let mut all = true;
    for c in checks {
        let tag = if c.pass { "PASS" } else { "FAIL" };
        println!("{tag} | {} | {}", c.name, c.detail);
        all &= c.pass;
    }
    all
}

async fn fetch_online(vault: &str) -> R<Input> {
    let net = NetworkConfig::testnet();
    let signed = Contract(vault.parse()?)
        .call_function("get_signed_intent", json!({}))
        .read_only::<Value>()
        .fetch_from(&net)
        .await?
        .data;
    if signed.is_null() {
        return Err("the vault holds no signed intent".into());
    }
    let externals = Contract(vault.parse()?)
        .call_function("get_externals", json!({}))
        .read_only::<Value>()
        .fetch_from(&net)
        .await?
        .data;
    let intents = externals[1].as_str().ok_or("unexpected externals list")?.to_string();
    let derived = Contract(SIGNER.parse()?)
        .call_function(
            "derived_public_key",
            json!({ "path": INTENTS_KEY_PATH, "predecessor": vault, "domain_id": ED25519_DOMAIN_ID }),
        )
        .read_only::<Value>()
        .fetch_from(&net)
        .await?
        .data;
    Ok(Input {
        vault: vault.to_string(),
        intents,
        signed,
        derived_public_key: derived.as_str().ok_or("unexpected derived key")?.to_string(),
    })
}

fn fixture_input(path: &str) -> R<Input> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    Ok(Input {
        vault: v["vault"].as_str().ok_or("fixture: vault")?.to_string(),
        intents: v["intents"].as_str().ok_or("fixture: intents")?.to_string(),
        signed: v["signed_intent"].clone(),
        derived_public_key: v["derived_public_key"]
            .as_str()
            .ok_or("fixture: derived key")?
            .to_string(),
    })
}

fn now_sec() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_secs()
}

#[tokio::main]
async fn main() -> R<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value =
        |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let now = match value("--now-sec") {
        Some(s) => s.parse::<u64>()?,
        None => now_sec(),
    };
    let input = if let Some(path) = value("--fixture") {
        fixture_input(&path)?
    } else if let Some(vault) = value("--vault") {
        fetch_online(&vault).await?
    } else {
        return Err(
            "usage: verify_intent --vault <account> | --fixture <file.json> [--now-sec <unix>]"
                .into(),
        );
    };
    let checks = run_checks(&input, now);
    if !report(&checks) {
        std::process::exit(1);
    }
    println!("ALL CHECKS PASSED for {}", input.vault);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!(
        "../../../docs/evidence/testnet-vault2-2026-11-09/signed-intent-proposal-1.json"
    );

    fn recorded() -> Input {
        let v: Value = serde_json::from_str(FIXTURE).expect("fixture JSON");
        Input {
            vault: v["vault"].as_str().unwrap().to_string(),
            intents: v["intents"].as_str().unwrap().to_string(),
            signed: v["signed_intent"].clone(),
            derived_public_key: v["derived_public_key"].as_str().unwrap().to_string(),
        }
    }

    fn all_pass(input: &Input, now: u64) -> bool {
        run_checks(input, now).iter().all(|c| c.pass)
    }

    fn failing(input: &Input, now: u64) -> Vec<&'static str> {
        run_checks(input, now).iter().filter(|c| !c.pass).map(|c| c.name).collect()
    }

    #[test]
    fn utc_parser_matches_the_recorded_expiry() {
        // expires_sec of the recorded intent is 1794226979 = 2026-11-09T12:22:59Z.
        assert_eq!(parse_utc("2026-11-09T12:22:59Z"), Some(1_794_226_979));
        assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_utc("2026-13-09T12:22:59Z"), None);
        assert_eq!(parse_utc("2026-11-09 12:22:59"), None);
    }

    #[test]
    fn recorded_real_intent_passes_before_its_deadline() {
        let input = recorded();
        assert!(all_pass(&input, 1_794_226_900), "{:?}", failing(&input, 1_794_226_900));
    }

    #[test]
    fn recorded_intent_fails_after_its_deadline() {
        let input = recorded();
        assert!(failing(&input, 1_794_227_000).contains(&"intent is not past its deadline"));
    }

    #[test]
    fn tampered_payload_breaks_the_signature() {
        let mut input = recorded();
        let tampered =
            input.signed["payload"].as_str().unwrap().replacen("-2000000", "-2000001", 1);
        input.signed["payload"] = Value::String(tampered);
        assert!(
            failing(&input, 1_794_226_900)
                .contains(&"ed25519 signature verifies over the exact payload bytes")
        );
    }

    #[test]
    fn foreign_key_is_rejected_even_with_a_valid_signature() {
        let mut input = recorded();
        input.derived_public_key = "ed25519:11111111111111111111111111111111".to_string();
        assert!(
            failing(&input, 1_794_226_900).contains(&"public key equals the signer-derived key")
        );
    }

    #[test]
    fn intent_for_another_vault_is_rejected() {
        let mut input = recorded();
        input.vault = "vault.mandate-t1.testnet".to_string();
        assert!(failing(&input, 1_794_226_900).contains(&"payload signer is this vault"));
    }

    #[test]
    fn wrong_intents_account_is_rejected() {
        let mut input = recorded();
        input.intents = "intents.example.testnet".to_string();
        assert!(
            failing(&input, 1_794_226_900)
                .contains(&"payload verifying contract is the Intents account")
        );
    }
}
