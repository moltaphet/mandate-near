//! Gas burnt by the calls a vault takes, on a local NEAR sandbox. Prints one
//! `GAS` line per call. `scripts/gas-report.sh` records them in docs/evidence/gas.txt.
//! Sandbox gas is the protocol's gas, not a mainnet fee: multiply by the gas price
//! on the network you use.

mod common;

use common::{
    REASON, USDC, WNEAR, World, fund_extra_assets, fund_vault, mandate, now_sec, set_prices, view,
    world, world_with_extra,
};
use near_api::{AccountId, Contract, NearGas, NearToken};
use near_sdk::serde_json::{Value, json};

/// Sends one call and returns its answer and the TGas it burnt.
async fn call(
    w: &World,
    method: &str,
    args: Value,
    as_account: &AccountId,
    yocto: u128,
) -> testresult::TestResult<(Value, f64)> {
    let success = w
        .vault
        .call_function(method, args)
        .transaction()
        .gas(NearGas::from_tgas(300))
        .deposit(NearToken::from_yoctonear(yocto))
        .with_signer(as_account.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .into_result()
        .map_err(|e| format!("{method} failed: {e:?}"))?;
    let burnt = success.total_gas_burnt.as_gas() as f64 / 1e12;
    Ok((success.json::<Value>().unwrap_or(Value::Null), burnt))
}

fn record(operation: &str, tgas: f64) {
    println!("GAS | {operation} | {tgas:.2} TGas");
}

#[tokio::test]
async fn gas_per_call() -> testresult::TestResult<()> {
    let w = world().await?;
    let (agent, owner, stranger) = (w.agent.clone(), w.owner.clone(), w.stranger.clone());

    let (_, burnt) = call(&w, "register_intents_key", json!({}), &stranger, 0).await?;
    record("register_intents_key (derive key, add it on intents.near)", burnt);

    fund_vault(&w, 10_000_000).await?;
    set_prices(&w, now_sec()).await?;

    let proposal = json!({
        "asset_in": USDC,
        "asset_out": WNEAR,
        "amount_in": "2000000",
        "min_amount_out": "450000000000000000000000",
        "model_id": "manual-v1", "reason_hash": REASON,
    });
    let (_, burnt) = call(&w, "propose", proposal, &agent, 0).await?;
    record("propose (agent, one trade, 2 assets in mandate)", burnt);

    let (outcome, burnt) = call(&w, "execute", json!({}), &stranger, 0).await?;
    assert!(outcome.get("Executed").is_some(), "{outcome}");
    record("execute, every gate passes, MPC sign (2 feeds read)", burnt);

    let mut loosened = mandate();
    loosened["max_trade_usd"] = json!("40000000");
    let (_, burnt) = call(&w, "set_mandate", json!({ "mandate": loosened }), &owner, 1).await?;
    record("set_mandate, tightening (applies at once)", burnt);

    let mut looser = mandate();
    looser["max_trade_usd"] = json!("50000000");
    looser["max_slippage_bps"] = json!(200);
    let (queued, burnt) = call(&w, "set_mandate", json!({ "mandate": looser }), &owner, 1).await?;
    assert!(queued["Queued"].is_object(), "{queued}");
    record("set_mandate, loosening (queued for 24 h)", burnt);

    let (_, burnt) = call(&w, "cancel_pending_mandate", json!({}), &owner, 1).await?;
    record("cancel_pending_mandate", burnt);

    let (_, burnt) = call(&w, "pause", json!({}), &owner, 0).await?;
    record("pause", burnt);
    let (_, burnt) = call(&w, "unpause", json!({}), &owner, 1).await?;
    record("unpause", burnt);
    Ok(())
}

/// One `execute` from the stranger. The outer error is a sandbox failure; the inner error
/// is the vault's answer, shortened to the message that says why.
async fn execute_once(w: &World) -> testresult::TestResult<Result<(Value, f64), String>> {
    let result = w
        .vault
        .call_function("execute", json!({}))
        .transaction()
        .gas(NearGas::from_tgas(300))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .into_result();
    Ok(match result {
        Ok(success) => {
            let burnt = success.total_gas_burnt.as_gas() as f64 / 1e12;
            Ok((success.json::<Value>().unwrap_or(Value::Null), burnt))
        }
        Err(e) => Err(short_reason(&format!("{e:?}"))),
    })
}

/// The quoted message from a failure, or the start of the failure if it has none.
fn short_reason(debug: &str) -> String {
    let quoted = debug.split("ExecutionError(\"").nth(1).and_then(|rest| rest.split('"').next());
    quoted.unwrap_or(debug).chars().take(120).collect()
}

/// `execute` as the number of feeds it reads grows. Non-traded assets are held;
/// the traded pair also requires prices, so the chain reads one feed per asset.
/// Each size runs on its own sandbox, because the mandate
/// is fixed at deploy. A failure is recorded, not hidden: it is the answer to SR-3.
#[tokio::test]
async fn gas_by_feed_count() -> testresult::TestResult<()> {
    // Total assets: 16 and 20, the most the mandate allows (MAX_ASSETS), with wNEAR and USDC among them.
    for extra in [14, 18] {
        let w = world_with_extra(extra).await?;
        let (agent, stranger) = (w.agent.clone(), w.stranger.clone());
        let feeds = 2 + extra;

        call(&w, "register_intents_key", json!({}), &stranger, 0).await?;
        fund_vault(&w, 10_000_000).await?;
        fund_extra_assets(&w, 1_000_000).await?;
        set_prices(&w, now_sec()).await?;

        let proposal = json!({
            "asset_in": USDC,
            "asset_out": WNEAR,
            "amount_in": "2000000",
            "min_amount_out": "450000000000000000000000",
            "model_id": "manual-v1", "reason_hash": REASON,
        });
        call(&w, "propose", proposal, &agent, 0).await?;
        match execute_once(&w).await? {
            Ok((outcome, burnt)) => {
                assert!(outcome.get("Executed").is_some(), "{outcome}");
                record(
                    &format!("execute, every gate passes, MPC sign ({feeds} feeds read)"),
                    burnt,
                );
            }
            Err(reason) => {
                // Both sizes are within MAX_ASSETS, so a failure here is a regression.
                panic!("execute failed at {feeds} feeds, within the supported cap: {reason}");
            }
        }
    }
    Ok(())
}

/// The supported maximum combines twenty held-or-traded assets, twenty ceilings,
/// and eight longest source IDs. The accepted ID is last and shares a 63-byte prefix with every rejected
/// ID, exercising the full bounded source lookup on proposal and execution.
#[tokio::test]
async fn gas_at_max_assets_and_last_long_source_id() -> testresult::TestResult<()> {
    let mut w = world_with_extra(18).await?;
    let mut configured = view(&w.vault, &w.network, "get_mandate", json!({})).await;
    assert_eq!(configured["assets"].as_array().unwrap().len(), 20);
    let ids: Vec<String> =
        (b'a'..=b'h').map(|suffix| format!("{}{}", "x".repeat(63), char::from(suffix))).collect();
    assert_eq!(ids.len(), 8);
    assert!(ids.iter().all(|id| id.len() == 64));
    let last = ids.last().unwrap().clone();
    configured["allowed_model_ids"] = json!(ids);
    configured["withdrawal_limits"] = json!(
        configured["assets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|asset| json!({
                "asset_id": asset["asset_id"],
                "max_per_request": u128::MAX.to_string(),
            }))
            .collect::<Vec<_>>()
    );
    assert_eq!(configured["withdrawal_limits"].as_array().unwrap().len(), 20);
    let externals = view(&w.vault, &w.network, "get_externals", json!({})).await;
    assert_eq!(externals.as_array().unwrap().len(), 3);

    // Deploy a fresh instance with this policy, preserving the normal 24-hour
    // update rule instead of changing a pending timer or adding an owner bypass.
    let sandbox_id: near_sdk::AccountId = "gas-max-source.sandbox".parse()?;
    w._sandbox.create_account(sandbox_id).initial_balance(NearToken::from_near(10)).send().await?;
    let vault_id: AccountId = "gas-max-source.sandbox".parse()?;
    // The signer stand-in accepts exactly one predecessor, fixed at its deployment.
    // A new vault therefore needs its own stand-in with the same test seed, so the
    // derived key is unchanged and only the accepted caller is the new vault.
    let signer_sandbox: near_sdk::AccountId = "gas-max-signer.sandbox".parse()?;
    w._sandbox
        .create_account(signer_sandbox)
        .initial_balance(NearToken::from_near(10))
        .send()
        .await?;
    let signer_id: AccountId = "gas-max-signer.sandbox".parse()?;
    let mock_wasm = common::build_contract(Some("../mock-externals/Cargo.toml"))?;
    Contract::deploy(signer_id.clone())
        .use_code(mock_wasm)
        .with_init_call(
            "new",
            json!({ "seed_hex": common::SIGNER_SEED_HEX, "expected_predecessor": vault_id }),
        )?
        .with_signer(w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    let code = std::fs::read(cargo_near_build::build_with_cli(Default::default())?)?;
    Contract::deploy(vault_id.clone())
        .use_code(code)
        .with_init_call(
            "new_with_externals",
            json!({
                "owner":w.owner, "mandate":configured,
                "signer":signer_id, "intents":externals[1], "oracle":externals[2]
            }),
        )?
        .with_signer(w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    w.vault = Contract(vault_id);
    assert_eq!(view(&w.vault, &w.network, "get_mandate", json!({})).await, configured);
    let (owner, agent, stranger) = (w.owner.clone(), w.agent.clone(), w.stranger.clone());
    call(&w, "set_agent", json!({"agent":agent}), &owner, 1).await?;
    assert_eq!(view(&w.vault, &w.network, "get_agent", json!({})).await, json!(agent));
    // Use the existing raw registration call; no cached identity assumption from
    // the originally deployed World is used for this fresh instance.
    call(&w, "register_intents_key", json!({}), &stranger, 0).await?;
    assert_eq!(view(&w.vault, &w.network, "is_key_registered", json!({})).await, json!(true));
    fund_vault(&w, 10_000_000).await?;
    fund_extra_assets(&w, 1_000_000).await?;
    set_prices(&w, now_sec()).await?;
    let assets: Vec<Value> = configured["assets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|asset| asset["asset_id"].clone())
        .collect();
    let balances = view(
        &w.intents,
        &w.network,
        "mt_batch_balance_of",
        json!({
            "account_id":w.vault.account_id(), "token_ids":assets
        }),
    )
    .await;
    assert_eq!(balances.as_array().unwrap().len(), 20);
    assert!(assets.iter().any(|asset| asset.as_str() == Some(WNEAR)));
    for (asset, balance) in assets.iter().zip(balances.as_array().unwrap()) {
        if asset.as_str() != Some(WNEAR) {
            assert!(
                balance.as_str().unwrap().parse::<u128>().unwrap() > 0,
                "each non-destination asset must be held: {asset}, {balance}"
            );
        }
    }
    // WNEAR is the traded destination below, so its price is required even if
    // its prior balance is zero. Every other asset has a positive holding.
    call(
        &w,
        "propose",
        json!({
            "asset_in":USDC, "asset_out":WNEAR,
            "amount_in":"2000000", "min_amount_out":"450000000000000000000000",
            "reason_hash":REASON, "model_id":last
        }),
        &agent,
        0,
    )
    .await?;
    let pending = view(&w.vault, &w.network, "get_pending_proposal", json!({})).await;
    assert_eq!(pending["model_id"], last);
    let (outcome, burnt) = execute_once(&w)
        .await?
        .map_err(|reason| format!("maximum policy execution failed: {reason}"))?;
    assert!(outcome.get("Executed").is_some(), "{outcome}");
    assert_eq!(view(&w.vault, &w.network, "is_in_flight", json!({})).await, json!(false));
    record(
        "execute, every gate passes, MPC sign (20 relevant held/traded feeds; 8 source IDs of 64 bytes; last ID matched)",
        burnt,
    );
    Ok(())
}
