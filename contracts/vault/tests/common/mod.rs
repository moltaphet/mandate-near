//! Shared sandbox world for the integration tests.
//!
//! Real: the `intents.near` wasm (exact mainnet code, see tests/fixtures/README.md)
//! and the vault. Stand-ins, sandbox only: the MPC signer (`mock-externals`,
//! which holds a test key and enforces the `v1.signer` calling convention)
//! and the price oracle (`mock-oracle`, same answer shapes and recency rule).

#![allow(dead_code)]

use near_api::{AccountId, Contract, NearToken, NetworkConfig, Signer};
use near_sdk::serde_json::{Value, json};
use std::sync::Arc;

pub const INTENTS_WASM: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/intents-near-mainnet.wasm");

/// Test seed for the signer stand-in. Not a secret: it only exists in the sandbox.
pub const SIGNER_SEED_HEX: &str =
    "0101010101010101010101010101010101010101010101010101010101010101";

pub const WNEAR: &str = "nep141:wrap.near";
/// USDC is the sandbox mock token, so its asset id names its account.
pub const USDC: &str = "nep141:usdc.sandbox";
/// Price feed ids the mandate uses. The mock oracle is keyed by these.
pub const WNEAR_FEED: &str = "wrap.near";
pub const USDC_FEED: &str = "usdc";

pub struct World {
    pub _sandbox: near_sandbox::Sandbox,
    pub network: NetworkConfig,
    pub signer: Arc<Signer>,
    pub vault: Contract,
    pub intents: Contract,
    pub mpc: Contract,
    pub oracle: Contract,
    pub usdc: Contract,
    pub owner: AccountId,
    pub agent: AccountId,
    pub stranger: AccountId,
    /// The key the signer stand-in derives for the vault (the one the vault registers).
    pub derived_key: String,
}

pub async fn create_account(
    sandbox: &near_sandbox::Sandbox,
    name: &str,
    near: u128,
) -> testresult::TestResult<AccountId> {
    // near-sandbox takes near-sdk's account id type; near-api has its own, so convert via the string.
    let sandbox_id: near_sdk::AccountId = name.parse()?;
    sandbox.create_account(sandbox_id).initial_balance(NearToken::from_near(near)).send().await?;
    Ok(name.parse()?)
}

pub async fn view(
    contract: &Contract,
    network: &NetworkConfig,
    method: &str,
    args: Value,
) -> Value {
    contract
        .call_function(method, args)
        .read_only()
        .fetch_from(network)
        .await
        .unwrap_or_else(|e| panic!("view {method} failed: {e}"))
        .data
}

/// The mandate used by every integration test: wNEAR and USDC, USDC stable.
pub fn mandate() -> Value {
    json!({
        "assets": [
            { "asset_id": WNEAR, "decimals": 24, "price_feed_id": WNEAR_FEED },
            { "asset_id": USDC, "decimals": 6, "price_feed_id": USDC_FEED },
        ],
        "stable_assets": [USDC],
        "max_trade_usd": "50000000",
        "max_daily_turnover_usd": "200000000",
        "max_position_bps": 6000,
        "max_slippage_bps": 100,
        "max_drawdown_bps": 1500,
        "min_trade_interval_sec": 300,
        "max_price_age_sec": 60,
    })
}

/// Builds one contract. Tests run in parallel, and concurrent `cargo near`
/// builds in the same target directory race, so builds take a lock.
fn build_contract(manifest: Option<&str>) -> testresult::TestResult<Vec<u8>> {
    static BUILD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = BUILD_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let opts = match manifest {
        Some(path) => cargo_near_build::BuildOpts::builder().manifest_path(path).build(),
        None => Default::default(),
    };
    Ok(std::fs::read(cargo_near_build::build_with_cli(opts)?)?)
}

pub async fn world() -> testresult::TestResult<World> {
    let vault_wasm = build_contract(None)?;
    let mock_wasm = build_contract(Some("../mock-externals/Cargo.toml"))?;
    let oracle_wasm = build_contract(Some("../mock-oracle/Cargo.toml"))?;
    let ft_wasm = build_contract(Some("../mock-ft/Cargo.toml"))?;
    let intents_wasm = std::fs::read(INTENTS_WASM)?;
    assert!(intents_wasm.starts_with(b"\0asm"), "fixture is not a wasm file");

    let sandbox = near_sandbox::Sandbox::start_sandbox().await?;
    let network = NetworkConfig::from_rpc_url("sandbox", sandbox.rpc_addr.parse()?);
    let signer = Signer::from_secret_key(
        near_sandbox::config::DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY.parse()?,
    )?;

    let owner = create_account(&sandbox, "owner.sandbox", 10).await?;
    let agent = create_account(&sandbox, "agent.sandbox", 10).await?;
    let stranger = create_account(&sandbox, "stranger.sandbox", 10).await?;
    let vault_id = create_account(&sandbox, "vault.sandbox", 10).await?;
    let mpc_id = create_account(&sandbox, "mpc.sandbox", 10).await?;
    let oracle_id = create_account(&sandbox, "oracle.sandbox", 10).await?;
    let usdc_id = create_account(&sandbox, "usdc.sandbox", 10).await?;
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
            json!({ "seed_hex": SIGNER_SEED_HEX, "expected_predecessor": vault_id }),
        )?
        .with_signer(signer.clone())
        .send_to(&network)
        .await?
        .assert_success();

    Contract::deploy(oracle_id.clone())
        .use_code(oracle_wasm)
        .with_init_call("new", json!({}))?
        .with_signer(signer.clone())
        .send_to(&network)
        .await?
        .assert_success();

    Contract::deploy(usdc_id.clone())
        .use_code(ft_wasm)
        .with_init_call("new", json!({}))?
        .with_signer(signer.clone())
        .send_to(&network)
        .await?
        .assert_success();

    Contract::deploy(vault_id.clone())
        .use_code(vault_wasm)
        .with_init_call(
            "new_with_externals",
            json!({
                "owner": owner,
                "mandate": mandate(),
                "signer": mpc_id,
                "intents": intents_id,
                "oracle": oracle_id,
            }),
        )?
        .with_signer(signer.clone())
        .send_to(&network)
        .await?
        .assert_success();

    // The owner names the agent, the only account allowed to propose trades.
    Contract(vault_id.clone())
        .call_function("set_agent", json!({ "agent": agent }))
        .transaction()
        .gas(near_api::NearGas::from_tgas(30))
        .deposit(NearToken::from_yoctonear(1))
        .with_signer(owner.clone(), signer.clone())
        .send_to(&network)
        .await?
        .assert_success();

    let mpc = Contract(mpc_id);
    let derived_key = mpc
        .call_function(
            "derived_public_key",
            json!({ "path": "intents", "predecessor": vault_id, "domain_id": 1 }),
        )
        .read_only::<String>()
        .fetch_from(&network)
        .await?
        .data;

    Ok(World {
        _sandbox: sandbox,
        network,
        signer,
        vault: Contract(vault_id),
        intents: Contract(intents_id),
        mpc,
        oracle: Contract(oracle_id),
        usdc: Contract(usdc_id),
        owner,
        agent,
        stranger,
        derived_key,
    })
}

/// Deposits `amount` of USDC into `intents.near` for the vault, the standard way:
/// the stranger holds minted tokens and calls `ft_transfer_call` with the vault as receiver.
pub async fn fund_vault(w: &World, amount: u128) -> testresult::TestResult<()> {
    let depositor = w.stranger.clone();
    w.usdc
        .call_function("mint", json!({ "account_id": depositor, "amount": amount.to_string() }))
        .transaction()
        .gas(near_api::NearGas::from_tgas(30))
        .with_signer(depositor.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    w.usdc
        .call_function(
            "ft_transfer_call",
            json!({
                "receiver_id": w.intents.account_id(),
                "amount": amount.to_string(),
                "msg": json!({ "receiver_id": w.vault.account_id() }).to_string(),
            }),
        )
        .transaction()
        .gas(near_api::NearGas::from_tgas(200))
        .deposit(NearToken::from_yoctonear(1))
        .with_signer(depositor, w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    Ok(())
}
