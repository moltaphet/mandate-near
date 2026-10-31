//! Integration tests: deploy the real wasm to a local NEAR sandbox and exercise
//! owner controls through actual transactions (keys, deposits, re-init).
//!
//! Pattern from the NEAR agent skill `testing-integration-tests` and the
//! `cargo near new` template.

use near_api::{AccountId, Contract, NearToken, NetworkConfig, Signer};
use near_sdk::serde_json::{Value, json};
use std::sync::Arc;

struct Env {
    _sandbox: near_sandbox::Sandbox,
    network: NetworkConfig,
    signer: Arc<Signer>,
    vault: Contract,
    owner: AccountId,
    stranger: AccountId,
}

fn mandate(max_slippage_bps: u16) -> Value {
    json!({
        "allowed_assets": [
            "nep141:wrap.near",
            "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1",
        ],
        "stable_assets": [
            "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1",
        ],
        "max_trade_usd": "50000000",
        "max_daily_turnover_usd": "200000000",
        "max_position_bps": 6000,
        "max_slippage_bps": max_slippage_bps,
        "max_drawdown_bps": 1500,
        "min_trade_interval_sec": 300,
        "max_price_age_sec": 60,
    })
}

async fn create_account(
    sandbox: &near_sandbox::Sandbox,
    name: &str,
) -> testresult::TestResult<AccountId> {
    let id: AccountId = name.parse()?;
    sandbox.create_account(id.clone()).initial_balance(NearToken::from_near(10)).send().await?;
    Ok(id)
}

async fn setup() -> testresult::TestResult<Env> {
    let wasm = std::fs::read(cargo_near_build::build_with_cli(Default::default())?)?;
    let sandbox = near_sandbox::Sandbox::start_sandbox().await?;
    let network = NetworkConfig::from_rpc_url("sandbox", sandbox.rpc_addr.parse()?);
    let signer = Signer::from_secret_key(
        near_sandbox::config::DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY.parse()?,
    )?;

    let owner = create_account(&sandbox, "owner.sandbox").await?;
    let stranger = create_account(&sandbox, "stranger.sandbox").await?;
    let vault_id = create_account(&sandbox, "vault.sandbox").await?;

    Contract::deploy(vault_id.clone())
        .use_code(wasm)
        .with_init_call("new", json!({ "owner": owner, "mandate": mandate(100) }))?
        .with_signer(signer.clone())
        .send_to(&network)
        .await?
        .assert_success();

    Ok(Env { _sandbox: sandbox, network, signer, vault: Contract(vault_id), owner, stranger })
}

impl Env {
    async fn call(&self, as_account: &AccountId, method: &str, args: Value, yocto: u128) -> bool {
        self.vault
            .call_function(method, args)
            .transaction()
            .deposit(NearToken::from_yoctonear(yocto))
            .with_signer(as_account.clone(), self.signer.clone())
            .send_to(&self.network)
            .await
            .expect("transaction reached the sandbox")
            .is_success()
    }

    async fn view(&self, method: &str) -> Value {
        self.vault
            .call_function(method, ())
            .read_only()
            .fetch_from(&self.network)
            .await
            .expect("view call")
            .data
    }
}

#[tokio::test]
async fn owner_controls_on_a_real_chain() -> testresult::TestResult<()> {
    let env = setup().await?;
    let (owner, stranger) = (env.owner.clone(), env.stranger.clone());

    assert_eq!(env.view("get_owner").await, json!(owner));
    assert_eq!(env.view("get_mandate").await, mandate(100));
    assert_eq!(env.view("is_paused").await, json!(false));

    // NEP-330: cargo-near embeds source metadata so the deployed code can be
    // traced back to this repository and rebuilt reproducibly.
    let source = env.view("contract_source_metadata").await;
    assert_eq!(source["link"], json!("https://github.com/moltaphet/mandate-near"), "{source}");

    // Re-initialisation is impossible (#[init] guard).
    assert!(
        !env.call(&stranger, "new", json!({ "owner": stranger, "mandate": mandate(100) }), 0).await
    );

    // set_mandate: owner only, exactly 1 yocto, rules re-validated.
    assert!(!env.call(&stranger, "set_mandate", json!({ "mandate": mandate(50) }), 1).await);
    assert!(!env.call(&owner, "set_mandate", json!({ "mandate": mandate(50) }), 0).await);
    assert!(!env.call(&owner, "set_mandate", json!({ "mandate": mandate(501) }), 1).await);
    assert!(env.call(&owner, "set_mandate", json!({ "mandate": mandate(50) }), 1).await);
    assert_eq!(env.view("get_mandate").await, mandate(50));

    // Kill switch: owner pauses without a deposit; resuming needs 1 yocto.
    assert!(!env.call(&stranger, "pause", json!({}), 0).await);
    assert!(env.call(&owner, "pause", json!({}), 0).await);
    assert_eq!(env.view("is_paused").await, json!(true));
    assert!(!env.call(&owner, "unpause", json!({}), 0).await);
    assert!(env.call(&owner, "unpause", json!({}), 1).await);
    assert_eq!(env.view("is_paused").await, json!(false));

    Ok(())
}
