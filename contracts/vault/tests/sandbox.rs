//! Integration tests: deploy the real wasm to a local NEAR sandbox and exercise
//! owner controls through actual transactions (keys, deposits, re-init).
//!
//! Pattern from the NEAR agent skill `testing-integration-tests` and the
//! `cargo near new` template.

use near_api::{AccountId, Contract, NearToken, NetworkConfig, Signer};
use near_sdk::base64::Engine;
use near_sdk::base64::engine::general_purpose::STANDARD as BASE64;
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
        "assets": [
            { "asset_id": "nep141:wrap.near", "decimals": 24, "price_feed_id": "wrap.near" },
            {
                "asset_id": "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1",
                "decimals": 6,
                "price_feed_id": "usdc",
            },
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
    // near-sandbox takes near-sdk's account id type; near-api has its own, so convert via the string.
    let sandbox_id: near_sdk::AccountId = name.parse()?;
    sandbox.create_account(sandbox_id).initial_balance(NearToken::from_near(10)).send().await?;
    Ok(name.parse()?)
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

    async fn call_view(&self, method: &str, args: Value) -> Value {
        self.vault
            .call_function(method, args)
            .read_only()
            .fetch_from(&self.network)
            .await
            .expect("view call")
            .data
    }
}

#[tokio::test]
async fn outside_accounts_cannot_invoke_callbacks_or_migration() -> testresult::TestResult<()> {
    let e = setup().await?;
    for method in [
        "on_derived_key",
        "on_key_registered",
        "on_balances",
        "on_price_data",
        "on_pyth_prices",
        "on_assets",
        "on_salt",
        "on_signed",
        "on_withdrawal_balance",
        "on_withdrawal_prepared",
        "on_withdrawn",
        "on_withdrawal_cancelled",
        "on_withdrawal_reconciled",
        "migrate",
    ] {
        let args = if method == "on_withdrawal_prepared" {
            json!({ "withdrawal_id": "1", "balance_before": "100" })
        } else {
            json!({})
        };
        let error = e
            .vault
            .call_function(method, args)
            .transaction()
            .with_signer(e.stranger.clone(), e.signer.clone())
            .send_to(&e.network)
            .await?
            .into_result()
            .expect_err("outside callback must be refused");
        let message = format!("{error:?}");
        assert!(message.contains("private"), "{method}: {message}");
    }
    assert_eq!(e.view("get_owner").await, json!(e.owner));
    assert_eq!(e.view("get_pending_proposal").await, Value::Null);
    assert_eq!(e.view("get_withdrawal_status").await, Value::Null);
    Ok(())
}

#[tokio::test]
async fn owner_controls_on_a_real_chain() -> testresult::TestResult<()> {
    let env = setup().await?;
    let (owner, stranger) = (env.owner.clone(), env.stranger.clone());

    assert_eq!(env.view("get_owner").await, json!(owner));
    assert_eq!(env.view("get_mandate").await, mandate(100));
    assert_eq!(env.view("is_paused").await, json!(false));
    assert_eq!(
        env.view("get_trading_state").await,
        json!({
            "last_trade_at_sec": null,
            "day_start_sec": 0,
            "day_turnover_usd": "0",
            "high_water_mark_usd": "0",
            "signed_intent_expires_at_sec": 0,
        })
    );

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

    // Loosening waits 24 h. The owner raises the slippage cap from 50 to 100 bps.
    let sim = env.call_view("simulate_mandate_change", json!({ "mandate": mandate(100) })).await;
    assert_eq!(sim["valid"], json!(true), "{sim}");
    assert_eq!(sim["loosened_fields"], json!(["max_slippage_bps"]), "{sim}");
    assert_eq!(env.view("get_pending_mandate").await, json!(null));

    assert!(env.call(&owner, "set_mandate", json!({ "mandate": mandate(100) }), 1).await);
    let queued = env.view("get_pending_mandate").await;
    assert_eq!(queued["mandate"], mandate(100), "{queued}");
    assert_eq!(env.view("get_mandate").await, mandate(50), "old limits hold during the delay");

    // Applying before the delay is refused; a stranger's cancel is refused; the owner can cancel.
    assert!(!env.call(&stranger, "apply_pending_mandate", json!({}), 0).await);
    assert!(!env.call(&stranger, "cancel_pending_mandate", json!({}), 1).await);
    assert!(!env.call(&owner, "apply_pending_mandate", json!({}), 0).await);
    assert!(env.call(&owner, "cancel_pending_mandate", json!({}), 1).await);
    assert_eq!(env.view("get_pending_mandate").await, json!(null));
    assert_eq!(env.view("get_mandate").await, mandate(50));

    Ok(())
}

#[tokio::test]
async fn a_new_agent_waits_24_hours_on_a_real_chain() -> testresult::TestResult<()> {
    let env = setup().await?;
    let (owner, stranger) = (env.owner.clone(), env.stranger.clone());
    let replacement = create_account(&env._sandbox, "replacement.sandbox").await?;

    // The first agent applies at once: no agent can propose before it is set.
    assert_eq!(env.view("get_agent").await, json!(null));
    assert!(!env.call(&stranger, "set_agent", json!({ "agent": stranger }), 1).await);
    assert!(env.call(&owner, "set_agent", json!({ "agent": stranger }), 1).await);
    assert_eq!(env.view("get_agent").await, json!(stranger));
    assert_eq!(env.view("get_pending_agent").await, json!(null));

    // A later agent is queued. The current agent keeps its rights.
    assert!(!env.call(&owner, "set_agent", json!({ "agent": replacement }), 0).await);
    assert!(env.call(&owner, "set_agent", json!({ "agent": replacement }), 1).await);
    let queued = env.view("get_pending_agent").await;
    assert_eq!(queued["agent"], json!(replacement), "{queued}");
    assert_eq!(env.view("get_agent").await, json!(stranger), "old agent holds during the delay");

    // Applying before the delay is refused; a stranger's cancel is refused; the owner can cancel.
    assert!(!env.call(&stranger, "apply_pending_agent", json!({}), 0).await);
    assert!(!env.call(&stranger, "cancel_pending_agent", json!({}), 1).await);
    assert!(!env.call(&owner, "apply_pending_agent", json!({}), 0).await);
    assert!(env.call(&owner, "cancel_pending_agent", json!({}), 1).await);
    assert_eq!(env.view("get_pending_agent").await, json!(null));
    assert_eq!(env.view("get_agent").await, json!(stranger));

    Ok(())
}

#[tokio::test]
async fn wasm_migration_consumes_historical_state_and_writes_current_state()
-> testresult::TestResult<()> {
    let e = setup().await?;
    let vault_id = e.vault.account_id().clone();
    let original_mandate = json!({
        "assets": [
            { "asset_id": "nep141:wrap.near", "decimals": 24, "price_feed_id": "wrap.near" },
            { "asset_id": "nep141:usdc.testnet", "decimals": 6, "price_feed_id": "usdc" },
        ],
        "stable_assets": ["nep141:usdc.testnet"],
        "max_trade_usd": "50000000",
        "max_daily_turnover_usd": "200000000",
        "max_position_bps": 6000,
        "max_slippage_bps": 100,
        "max_drawdown_bps": 1500,
        "min_trade_interval_sec": 300,
        "max_price_age_sec": 60,
    });
    let mut queued_mandate = original_mandate.clone();
    queued_mandate["max_trade_usd"] = json!("75000000");

    // Independently emitted by the original compiled Vault implementations.
    // Patching old STATE exercises the actual Wasm init(ignore_state) wrapper;
    // it is neither deployed-mainnet evidence nor a funded upgrade test.
    for (commit, state) in [
        ("77ebc0c", include_bytes!("fixtures/legacy-77ebc0c-state.borsh").as_slice()),
        ("0de5c48", include_bytes!("fixtures/legacy-0de5c48-state.borsh").as_slice()),
    ] {
        e._sandbox
            .patch_state(vault_id.as_str().parse()?)
            .storage(BASE64.encode(b"STATE"), BASE64.encode(state))
            .send()
            .await?;

        let external = e
            .vault
            .call_function("migrate", json!({}))
            .transaction()
            .with_signer(e.stranger.clone(), e.signer.clone())
            .send_to(&e.network)
            .await?
            .into_result()
            .expect_err("outside account must not migrate historical state");
        let external = format!("{external:?}");
        assert!(external.contains("private"), "{commit}: {external}");

        assert!(e.call(&vault_id, "migrate", json!({}), 0).await, "{commit}: self migration");
        assert_eq!(e.view("get_owner").await, json!("owner.testnet"), "{commit}");
        assert_eq!(e.view("is_paused").await, json!(true), "{commit}");
        assert_eq!(e.view("get_mandate").await, original_mandate, "{commit}");
        assert_eq!(
            e.view("get_pending_mandate").await,
            json!({ "mandate": queued_mandate, "effective_sec": 1_793_784_076_u64 }),
            "{commit}"
        );
        assert_eq!(e.view("get_agent").await, json!("agent.testnet"), "{commit}");
        assert_eq!(
            e.view("get_pending_agent").await,
            json!({ "agent": "replacement.testnet", "effective_sec": 1_793_784_076_u64 }),
            "{commit}"
        );
        assert_eq!(
            e.view("get_trading_state").await,
            json!({
                "last_trade_at_sec": null,
                "day_start_sec": 0,
                "day_turnover_usd": "0",
                "high_water_mark_usd": "0",
                "signed_intent_expires_at_sec": 0,
            }),
            "{commit}"
        );
        assert_eq!(
            e.view("get_externals").await,
            json!(["v1.signer", "intents.near", "price-oracle.near"]),
            "{commit}"
        );
        assert_eq!(e.view("get_pending_proposal").await, Value::Null, "{commit}");
        assert_eq!(e.view("get_pending_withdrawal").await, Value::Null, "{commit}");
        assert_eq!(e.view("get_withdrawal_status").await, Value::Null, "{commit}");
        assert_eq!(e.view("is_in_flight").await, json!(false), "{commit}");

        let repeated = e
            .vault
            .call_function("migrate", json!({}))
            .transaction()
            .with_signer(vault_id.clone(), e.signer.clone())
            .send_to(&e.network)
            .await?
            .into_result()
            .expect_err("a migrated state must not be migrated twice");
        let repeated = format!("{repeated:?}");
        assert!(repeated.contains("already current"), "{commit}: {repeated}");
    }
    Ok(())
}
