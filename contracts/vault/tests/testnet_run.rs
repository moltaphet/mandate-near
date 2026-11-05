//! One run of the vault on NEAR testnet, controlled from the owner's account.
//!
//! It spends testnet NEAR and writes to the chain, so it is ignored by default. Run it with
//!   MANDATE_TESTNET_RUN=1 cargo test -p mandate-vault --test testnet_run -- --ignored --nocapture
//! The owner's key is read from ~/.near-credentials/testnet and is never printed.

use std::sync::Arc;

use near_api::{Account, AccountId, Contract, NearToken, NetworkConfig, SecretKey, Signer};
use near_sdk::serde_json::{Value, json};

const OWNER: &str = "mandate-r1-4715a0.testnet";
const VAULT: &str = "vault.mandate-r1-4715a0.testnet";
const AGENT: &str = "agent.mandate-r1-4715a0.testnet";
const SIGNER: &str = "v1.signer-prod.testnet";
const INTENTS: &str = "intents.testnet";
const ORACLE: &str = "price-oracle.testnet";
const WRAP: &str = "nep141:wrap.testnet";
const USDC: &str = "nep141:usdc.testnet";

/// A key generated for this run and kept in ~/.config/mandate/testnet-keys.json (mode 600).
/// It is never printed.
fn run_key(role: &str) -> testresult::TestResult<SecretKey> {
    let home = std::env::var("HOME")?;
    let file: Value = near_sdk::serde_json::from_str(&std::fs::read_to_string(format!("{home}/.config/mandate/testnet-keys.json"))?)?;
    let secret = file[role]["secret"].as_str().ok_or("no secret for this role")?;
    Ok(secret.parse()?)
}

/// The owner's full-access key, from the local credential file.
fn owner_key() -> testresult::TestResult<SecretKey> {
    let home = std::env::var("HOME")?;
    let path = format!("{home}/.near-credentials/testnet/{OWNER}.json");
    let file: Value = near_sdk::serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let secret = file["private_key"].as_str().ok_or("no private_key field")?;
    Ok(secret.parse()?)
}

fn mandate() -> Value {
    json!({
        "assets": [
            { "asset_id": WRAP, "decimals": 24, "price_feed_id": "wrap.testnet" },
            { "asset_id": USDC, "decimals": 6, "price_feed_id": "usdc.testnet" },
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

/// Sends one call. Returns the answer, or the vault's message, and logs the line.
async fn call(
    network: &NetworkConfig,
    contract: &AccountId,
    method: &str,
    args: Value,
    as_account: &AccountId,
    signer: Arc<Signer>,
    yocto: u128,
) -> Result<Value, String> {
    let result = Contract(contract.clone())
        .call_function(method, args)
        .transaction()
        .gas(near_api::NearGas::from_tgas(100))
        .deposit(NearToken::from_yoctonear(yocto))
        .with_signer(as_account.clone(), signer)
        .send_to(network)
        .await
        .map_err(|e| e.to_string())?
        .into_result();
    match result {
        Ok(ok) => Ok(ok.json::<Value>().unwrap_or(Value::Null)),
        Err(e) => Err(panic_text(&format!("{e:?}"))),
    }
}

/// The quoted panic message from a failed call, or the start of the failure.
fn panic_text(error: &str) -> String {
    let quoted = ["panicked: ", "panic_msg: "]
        .iter()
        .find_map(|marker| error.split(marker).nth(1))
        .and_then(|rest| rest.trim_start_matches(['\\', '"']).split('"').next());
    quoted.unwrap_or(error).chars().take(120).collect()
}

async fn view(network: &NetworkConfig, contract: &AccountId, method: &str, args: Value) -> Value {
    Contract(contract.clone())
        .call_function(method, args)
        .read_only()
        .fetch_from(network)
        .await
        .map(|r| r.data)
        .unwrap_or_else(|e| json!(format!("view error: {e}")))
}

fn log(step: &str, outcome: &str) {
    println!("TESTNET | {step} | {outcome}");
}

#[tokio::test]
#[ignore = "spends testnet NEAR; set MANDATE_TESTNET_RUN=1 and run with --ignored"]
async fn vault_on_testnet() -> testresult::TestResult<()> {
    if std::env::var("MANDATE_TESTNET_RUN").as_deref() != Ok("1") {
        return Ok(());
    }
    let network = NetworkConfig::testnet();
    let owner: AccountId = OWNER.parse()?;
    let vault: AccountId = VAULT.parse()?;
    let agent: AccountId = AGENT.parse()?;
    let owner_sk = owner_key()?;
    let owner_signer = Signer::from_secret_key(owner_sk.clone())?;

    // The vault and the agent are subaccounts of the owner, so the owner's key creates them.
    let vault_sk = run_key("vault")?;
    let agent_sk = run_key("agent")?;
    if std::env::var("MANDATE_TESTNET_STEP").as_deref() == Ok("recheck") {
        return recheck(&network, &owner, &vault, &agent, owner_signer, &agent_sk).await;
    }
    Account::create_account(vault.clone())
        .fund_myself(owner.clone(), NearToken::from_near(5))
        .with_public_key(vault_sk.public_key())
        .with_signer(owner_signer.clone())
        .send_to(&network)
        .await?
        .assert_success();
    log("create vault subaccount, 5 NEAR", "ok");
    Account::create_account(agent.clone())
        .fund_myself(owner.clone(), NearToken::from_millinear(500))
        .with_public_key(agent_sk.public_key())
        .with_signer(owner_signer.clone())
        .send_to(&network)
        .await?
        .assert_success();
    log("create agent subaccount, 0.5 NEAR", "ok");

    // Deploy the testnet build and initialise it in the same transaction.
    let wasm = std::fs::read(cargo_near_build::build_with_cli(Default::default())?)?;
    let vault_signer = Signer::from_secret_key(vault_sk.clone())?;
    Contract::deploy(vault.clone())
        .use_code(wasm)
        .with_init_call(
            "new_with_externals",
            json!({
                "owner": owner,
                "mandate": mandate(),
                "signer": SIGNER,
                "intents": INTENTS,
                "oracle": ORACLE,
            }),
        )?
        .with_signer(vault_signer.clone())
        .send_to(&network)
        .await?
        .assert_success();
    log("deploy and init (new_with_externals)", "ok");
    println!("TESTNET | externals | {}", view(&network, &vault, "get_externals", json!({})).await);

    let s = owner_signer.clone();
    log("set_agent (first agent, applies at once)", &format!("{:?}", call(&network, &vault, "set_agent", json!({ "agent": agent }), &owner, s.clone(), 1).await));
    log("get_agent", &view(&network, &vault, "get_agent", json!({})).await.to_string());

    let mut loosened = mandate();
    loosened["max_trade_usd"] = json!("60000000");
    log("set_mandate (loosening, queued 24 h)", &format!("{:?}", call(&network, &vault, "set_mandate", json!({ "mandate": loosened }), &owner, s.clone(), 1).await));
    log("get_pending_mandate", &view(&network, &vault, "get_pending_mandate", json!({})).await.to_string());
    log("cancel_pending_mandate", &format!("{:?}", call(&network, &vault, "cancel_pending_mandate", json!({}), &owner, s.clone(), 1).await));

    log("request_withdrawal (1 WRAP to owner, queued 24 h)", &format!("{:?}", call(&network, &vault, "request_withdrawal", json!({ "asset_id": WRAP, "receiver": owner, "amount": "1000" }), &owner, s.clone(), 1).await));
    log("apply_pending_withdrawal too early (must refuse)", &format!("{:?}", call(&network, &vault, "apply_pending_withdrawal", json!({}), &owner, s.clone(), 0).await));
    log("cancel_pending_withdrawal", &format!("{:?}", call(&network, &vault, "cancel_pending_withdrawal", json!({}), &owner, s.clone(), 1).await));

    let agent_signer = Signer::from_secret_key(agent_sk.clone())?;
    let proposal = json!({
        "asset_in": USDC,
        "asset_out": WRAP,
        "amount_in": "2000000",
        "min_amount_out": "450000000000000000000000",
        "reason_hash": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
    });
    log("propose (agent)", &format!("{:?}", call(&network, &vault, "propose", proposal, &agent, agent_signer, 0).await));
    log("execute (must fail closed: intents key not registered)", &format!("{:?}", call(&network, &vault, "execute", json!({}), &owner, s.clone(), 0).await));

    // SR-9: the vault account must hold no full-access key once it is set up.
    let del = Account(vault.clone()).delete_key(vault_sk.public_key()).with_signer(vault_signer).send_to(&network).await;
    log("delete the vault's full-access key (SR-9)", &format!("{:?}", del.map(|r| r.assert_success())));
    let keys = Account(vault.clone()).list_keys().fetch_from(&network).await?;
    log("access keys left on the vault", &format!("{}", keys.data.len()));
    Ok(())
}

/// Repeats only the two refusals of the first run, with readable messages. The vault exists.
async fn recheck(
    network: &NetworkConfig,
    owner: &AccountId,
    vault: &AccountId,
    agent: &AccountId,
    owner_signer: Arc<Signer>,
    agent_sk: &SecretKey,
) -> testresult::TestResult<()> {
    let s = owner_signer.clone();
    log("request_withdrawal (recheck)", &format!("{:?}", call(network, vault, "request_withdrawal", json!({ "asset_id": WRAP, "receiver": owner, "amount": "1000" }), owner, s.clone(), 1).await));
    log("apply_pending_withdrawal too early (recheck)", &format!("{:?}", call(network, vault, "apply_pending_withdrawal", json!({}), owner, s.clone(), 0).await));
    log("cancel_pending_withdrawal (recheck)", &format!("{:?}", call(network, vault, "cancel_pending_withdrawal", json!({}), owner, s.clone(), 1).await));
    let agent_signer = Signer::from_secret_key(agent_sk.clone())?;
    let proposal = json!({
        "asset_in": USDC,
        "asset_out": WRAP,
        "amount_in": "2000000",
        "min_amount_out": "450000000000000000000000",
        "reason_hash": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
    });
    log("propose (agent, recheck)", &format!("{:?}", call(network, vault, "propose", proposal, agent, agent_signer, 0).await));
    log("execute (recheck, must fail closed)", &format!("{:?}", call(network, vault, "execute", json!({}), owner, s, 0).await));
    Ok(())
}
