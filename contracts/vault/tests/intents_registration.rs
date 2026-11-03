//! The vault registers its MPC-derived key on the **real** `intents.near`
//! contract, running unmodified inside a local NEAR sandbox.
//!
//! `intents.near` is the exact mainnet code (see tests/fixtures/README.md).
//! The MPC signer is a stand-in that enforces the `derived_public_key`
//! calling convention; everything else is the real thing.

mod common;

use common::{view, world};
use near_sdk::serde_json::{Value, json};

#[tokio::test]
async fn vault_registers_its_mpc_key_on_the_real_intents_contract() -> testresult::TestResult<()> {
    let w = world().await?;
    let vault_id = w.vault.account_id().to_string();
    let key = w.derived_key.as_str();

    // Before: nothing is registered, and the vault says so.
    assert_eq!(view(&w.vault, &w.network, "is_key_registered", json!({})).await, json!(false));
    assert_eq!(view(&w.vault, &w.network, "get_intents_key", json!({})).await, Value::Null);
    let before =
        view(&w.intents, &w.network, "public_keys_of", json!({ "account_id": vault_id })).await;
    assert!(!before.to_string().contains(key), "key must not be registered yet: {before}");

    // Anyone can trigger registration; the key cannot be chosen by the caller.
    w.vault
        .call_function("register_intents_key", json!({}))
        .transaction()
        .gas(near_api::NearGas::from_tgas(200))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();

    // After: the vault recorded the key, and the real verifier lists it for the vault's account.
    assert_eq!(view(&w.vault, &w.network, "get_intents_key", json!({})).await, json!(key));
    assert_eq!(view(&w.vault, &w.network, "is_key_registered", json!({})).await, json!(true));
    let after =
        view(&w.intents, &w.network, "public_keys_of", json!({ "account_id": vault_id })).await;
    assert!(after.to_string().contains(key), "intents.near must list the key: {after}");
    let has = view(
        &w.intents,
        &w.network,
        "has_public_key",
        json!({ "account_id": vault_id, "public_key": key }),
    )
    .await;
    assert_eq!(has, json!(true));

    // A second registration is refused, and the recorded state stays true.
    w.vault
        .call_function("register_intents_key", json!({}))
        .transaction()
        .gas(near_api::NearGas::from_tgas(200))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_failure();
    assert_eq!(view(&w.vault, &w.network, "is_key_registered", json!({})).await, json!(true));
    Ok(())
}

#[tokio::test]
async fn signer_stand_in_rejects_a_key_request_for_another_account() -> testresult::TestResult<()> {
    let w = world().await?;
    let wrong = w
        .mpc
        .call_function(
            "derived_public_key",
            json!({ "path": "intents", "predecessor": w.stranger, "domain_id": 1 }),
        )
        .read_only::<Value>()
        .fetch_from(&w.network)
        .await;
    assert!(wrong.is_err(), "the stand-in must refuse a key for an account that is not the vault");
    Ok(())
}
