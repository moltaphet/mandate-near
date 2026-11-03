//! The vault registers its MPC-derived key on the **real** `intents.near`
//! contract, running unmodified inside a local NEAR sandbox.
//!
//! `intents.near` is the exact mainnet code (see tests/fixtures/README.md).
//! The MPC signer is a stand-in that enforces the `derived_public_key`
//! calling convention; everything else is the real thing.

use near_api::{AccountId, Contract, NearToken, NetworkConfig, Signer};
use near_sdk::serde_json::{Value, json};
use std::sync::Arc;

/// A valid Ed25519 key, as derived by mainnet MPC in the R1 spike.
const DERIVED_KEY: &str = "ed25519:6dn7BHkg3cTuM5s9nnXc6Vkf14pEHZpvtcDKaR8q3ZeT";
const INTENTS_WASM: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/intents-near-mainnet.wasm");

struct World {
    _sandbox: near_sandbox::Sandbox,
    network: NetworkConfig,
    signer: Arc<Signer>,
    vault: Contract,
    intents: Contract,
    mpc: Contract,
    stranger: AccountId,
}

async fn create_account(
    sandbox: &near_sandbox::Sandbox,
    name: &str,
    near: u128,
) -> testresult::TestResult<AccountId> {
    // near-sandbox takes near-sdk's account id type; near-api has its own, so convert via the string.
    let sandbox_id: near_sdk::AccountId = name.parse()?;
    sandbox.create_account(sandbox_id).initial_balance(NearToken::from_near(near)).send().await?;
    Ok(name.parse()?)
}

async fn view(contract: &Contract, network: &NetworkConfig, method: &str, args: Value) -> Value {
    contract
        .call_function(method, args)
        .read_only()
        .fetch_from(network)
        .await
        .unwrap_or_else(|e| panic!("view {method} failed: {e}"))
        .data
}

async fn world() -> testresult::TestResult<World> {
    let vault_wasm = std::fs::read(cargo_near_build::build_with_cli(Default::default())?)?;
    let mock_wasm = std::fs::read(cargo_near_build::build_with_cli(
        cargo_near_build::BuildOpts::builder()
            .manifest_path("../mock-externals/Cargo.toml")
            .build(),
    )?)?;
    let intents_wasm = std::fs::read(INTENTS_WASM)?;
    assert!(intents_wasm.starts_with(b"\0asm"), "fixture is not a wasm file");

    let sandbox = near_sandbox::Sandbox::start_sandbox().await?;
    let network = NetworkConfig::from_rpc_url("sandbox", sandbox.rpc_addr.parse()?);
    let signer = Signer::from_secret_key(
        near_sandbox::config::DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY.parse()?,
    )?;

    let owner = create_account(&sandbox, "owner.sandbox", 10).await?;
    let stranger = create_account(&sandbox, "stranger.sandbox", 10).await?;
    let vault_id = create_account(&sandbox, "vault.sandbox", 10).await?;
    let mpc_id = create_account(&sandbox, "mpc.sandbox", 10).await?;
    // The real verifier is 1.3 MB, so its account needs enough balance for storage.
    let intents_id = create_account(&sandbox, "intents.sandbox", 100).await?;

    Contract::deploy(intents_id.clone())
        .use_code(intents_wasm)
        .with_init_call(
            "new",
            json!({
                "config": {
                    "wnear_id": "wrap.sandbox",
                    "fees": { "fee": 0, "fee_collector": "fees.sandbox" },
                }
            }),
        )?
        .with_signer(signer.clone())
        .send_to(&network)
        .await?
        .assert_success();

    Contract::deploy(mpc_id.clone())
        .use_code(mock_wasm)
        .with_init_call(
            "new",
            json!({ "derived_key": DERIVED_KEY, "expected_predecessor": vault_id }),
        )?
        .with_signer(signer.clone())
        .send_to(&network)
        .await?
        .assert_success();

    let mandate = json!({
        "assets": [
            { "asset_id": "nep141:wrap.near", "decimals": 24, "price_feed_id": "wrap.near" },
            { "asset_id": "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1", "decimals": 6, "price_feed_id": "usdc" },
        ],
        "stable_assets": ["nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1"],
        "max_trade_usd": "50000000",
        "max_daily_turnover_usd": "200000000",
        "max_position_bps": 6000,
        "max_slippage_bps": 100,
        "max_drawdown_bps": 1500,
        "min_trade_interval_sec": 300,
        "max_price_age_sec": 60,
    });
    Contract::deploy(vault_id.clone())
        .use_code(vault_wasm)
        .with_init_call(
            "new_with_externals",
            json!({ "owner": owner, "mandate": mandate, "signer": mpc_id, "intents": intents_id }),
        )?
        .with_signer(signer.clone())
        .send_to(&network)
        .await?
        .assert_success();

    Ok(World {
        _sandbox: sandbox,
        network,
        signer,
        vault: Contract(vault_id),
        intents: Contract(intents_id),
        mpc: Contract(mpc_id),
        stranger,
    })
}

#[tokio::test]
async fn vault_registers_its_mpc_key_on_the_real_intents_contract() -> testresult::TestResult<()> {
    let w = world().await?;
    let vault_id = w.vault.account_id().to_string();

    // Before: nothing is registered, and the vault says so.
    assert_eq!(view(&w.vault, &w.network, "is_key_registered", json!({})).await, json!(false));
    assert_eq!(view(&w.vault, &w.network, "get_intents_key", json!({})).await, Value::Null);
    let before =
        view(&w.intents, &w.network, "public_keys_of", json!({ "account_id": vault_id })).await;
    assert!(!before.to_string().contains(DERIVED_KEY), "key must not be registered yet: {before}");

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
    assert_eq!(view(&w.vault, &w.network, "get_intents_key", json!({})).await, json!(DERIVED_KEY));
    assert_eq!(view(&w.vault, &w.network, "is_key_registered", json!({})).await, json!(true));
    let after =
        view(&w.intents, &w.network, "public_keys_of", json!({ "account_id": vault_id })).await;
    assert!(after.to_string().contains(DERIVED_KEY), "intents.near must list the key: {after}");
    let has = view(
        &w.intents,
        &w.network,
        "has_public_key",
        json!({ "account_id": vault_id, "public_key": DERIVED_KEY }),
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
