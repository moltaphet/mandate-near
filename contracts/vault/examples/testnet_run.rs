//! Controlled NEAR testnet run for the Mandate vault. One subcommand per step, so each
//! step is run, checked and recorded on its own.
//!
//! Real on testnet: the vault, the Chain Signatures signer `v1.signer-prod.testnet` and a
//! Sputnik DAO v2 owner. Labelled stand-ins: the oracle (`mock-oracle`) and the Intents
//! verifier (the supplied fixture), because testnet has neither a priced oracle nor an
//! Intents contract. Read docs/design/testnet-decisions.md before using the results.
//!
//! The operator key is read from ~/.near-credentials/testnet/<IMPLICIT>.json and is never
//! printed. Every state-changing step prints its transaction hash.
//!
//! Usage: cargo run -p mandate-vault --example testnet_run -- <step> [args]

use std::sync::Arc;

use near_api::types::SecretKey;
use near_api::types::transaction::actions::{
    Action, CreateAccountAction, DeployContractAction, FunctionCallAction, TransferAction,
};
use near_api::{
    Account, AccountId, Contract, NearGas, NearToken, NetworkConfig, Signer, Transaction,
};
use near_sdk::serde_json::{Value, json};

type R<T> = Result<T, Box<dyn std::error::Error>>;

const IMPLICIT: &str = "114e13f639a265e0f0c8cff7e5c2aa916070325c827f4cb4d0dc5195a9febdb8";
const PARENT: &str = "mandate-t1.testnet";
const DAO_NAME: &str = "mandate-t1";
const FACTORY: &str = "sputnikv2.testnet";
const SIGNER: &str = "v1.signer-prod.testnet";
const WNEAR: &str = "nep141:wrap.testnet";
const WNEAR_FEED: &str = "wrap.testnet";
const USDC_FEED: &str = "usdc";
const REASON: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
const WASM_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/testnet-wasm");
const INTENTS_WASM: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/intents-near-mainnet.wasm");

fn id(s: &str) -> AccountId {
    s.parse().expect("valid account id")
}
fn sub(name: &str) -> AccountId {
    id(&format!("{name}.{PARENT}"))
}
fn vault_id() -> AccountId {
    // The target vault must be named on every run. There is no default: on 9 Nov 2026 a
    // missing name silently fell back to the first testnet vault, and a proposal and an
    // execution ran there by mistake.
    let name = std::env::var("MANDATE_VAULT").unwrap_or_else(|_| {
        panic!("set MANDATE_VAULT to the target vault name (for example vault2); refusing to guess")
    });
    println!("TARGET | vault {name}");
    sub(&name)
}
fn dao() -> AccountId {
    id(&format!("{DAO_NAME}.{FACTORY}"))
}
fn usdc_asset() -> String {
    format!("nep141:{}", sub("usdc"))
}

struct Ctx {
    net: NetworkConfig,
    signer: Arc<Signer>,
    secret: SecretKey,
}

fn load() -> R<Ctx> {
    let home = std::env::var("HOME")?;
    let path = format!("{home}/.near-credentials/testnet/{IMPLICIT}.json");
    let file: Value = near_sdk::serde_json::from_slice(&std::fs::read(path)?)?;
    let secret: SecretKey = file["private_key"].as_str().ok_or("missing key")?.parse()?;
    Ok(Ctx {
        net: NetworkConfig::testnet(),
        signer: Signer::from_secret_key(secret.clone())?,
        secret,
    })
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn wasm(name: &str) -> R<Vec<u8>> {
    Ok(std::fs::read(format!("{WASM_DIR}/{name}.wasm"))?)
}

/// Sends one function call, prints its hash and failures, and returns the success value.
async fn call(
    cx: &Ctx,
    contract: &AccountId,
    caller: &AccountId,
    method: &str,
    args: Value,
    tgas: u64,
    deposit: NearToken,
) -> R<near_api::types::transaction::result::ExecutionSuccess> {
    let result = Contract(contract.clone())
        .call_function(method, args)
        .transaction()
        .gas(NearGas::from_tgas(tgas))
        .deposit(deposit)
        .with_signer(caller.clone(), cx.signer.clone())
        .send_to(&cx.net)
        .await?;
    let hash = result.transaction().get_hash();
    let success = result.into_result().map_err(|e| format!("{method} failed: {e:?}"))?;
    println!(
        "TX | {method} on {contract} by {caller} | hash {hash} | receipt failures: {}",
        success.receipt_failures().len()
    );
    Ok(success)
}

async fn view(cx: &Ctx, contract: &AccountId, method: &str, args: Value) -> R<Value> {
    Ok(Contract(contract.clone())
        .call_function(method, args)
        .read_only::<Value>()
        .fetch_from(&cx.net)
        .await?
        .data)
}

fn now_sec() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_secs()
}

/// The source timestamp of one feed in a price file. Refused when it is dated in the
/// future (beyond 5 s of clock skew) or is older than 60 s, the vault's price age limit.
fn feed_timestamp(v: &Value, key: &str, now: u64) -> R<u64> {
    let ts = v["source_updated_at"][key]
        .as_u64()
        .ok_or_else(|| format!("price file: source_updated_at.{key} missing"))?;
    if ts > now + 5 {
        return Err(format!("{key} quote is {} s in the future; refused", ts - now).into());
    }
    let age = now.saturating_sub(ts);
    if age > 60 {
        return Err(format!("{key} quote is {age} s old (limit 60 s); refetch").into());
    }
    Ok(ts)
}

fn mandate() -> Value {
    json!({
        "assets": [
            { "asset_id": WNEAR, "decimals": 24, "price_feed_id": WNEAR_FEED },
            { "asset_id": usdc_asset(), "decimals": 6, "price_feed_id": USDC_FEED },
        ],
        "stable_assets": [usdc_asset()],
        "max_trade_usd": "50000000",
        "max_daily_turnover_usd": "200000000",
        "max_position_bps": 6000,
        "max_slippage_bps": 100,
        "max_drawdown_bps": 1500,
        "min_trade_interval_sec": 300,
        "max_price_age_sec": 60,
        "allowed_model_ids": ["manual-v1"],
        "withdrawal_limits": [{ "asset_id": usdc_asset(), "max_per_request": "5000000" }],
    })
}

/// An owner call: the DAO is the vault owner, so the call goes through a DAO proposal that
/// the single council member approves. The vault sees the DAO as the predecessor.
async fn owner_call(cx: &Ctx, method: &str, args: Value, desc: &str) -> R<()> {
    let council = id(PARENT);
    let action = json!({
        "method_name": method,
        "args": b64(&near_sdk::serde_json::to_vec(&args)?),
        "deposit": "1",
        "gas": "60000000000000",
    });
    let proposal = json!({ "proposal": {
        "description": desc,
        "kind": { "FunctionCall": { "receiver_id": vault_id(), "actions": [action] } },
    }});
    // The DAO policy sets a 1 NEAR proposal bond, returned when the proposal is approved.
    let added =
        call(cx, &dao(), &council, "add_proposal", proposal, 30, NearToken::from_near(1)).await?;
    let proposal_id: u64 = added.json()?;
    call(
        cx,
        &dao(),
        &council,
        "act_proposal",
        json!({ "id": proposal_id, "action": "VoteApprove" }),
        250,
        NearToken::from_yoctonear(0),
    )
    .await?;
    println!("OWNER | {method} approved through DAO proposal {proposal_id}");
    Ok(())
}

async fn step(cx: &Ctx, name: &str, rest: &[String]) -> R<()> {
    let parent = id(PARENT);
    let pk = cx.secret.public_key();
    match name {
        // 1. The named parent account, created through the testnet registrar.
        "parent" => {
            let r = Account::create_account(parent.clone())
                .fund_myself(id(IMPLICIT), NearToken::from_near(44))
                .with_public_key(pk)
                .with_signer(cx.signer.clone())
                .send_to(&cx.net)
                .await?;
            println!("TX | create {parent} | hash {} ", r.transaction().get_hash());
            r.assert_success();
        }
        // 2. Sub-accounts that need a key. The vault is created later without any key.
        "subs" => {
            for (name, near) in [("oracle", 3), ("usdc", 3), ("intents", 14), ("agent", 1)] {
                let account = sub(name);
                let r = Account::create_account(account.clone())
                    .fund_myself(parent.clone(), NearToken::from_near(near))
                    .with_public_key(pk)
                    .with_signer(cx.signer.clone())
                    .send_to(&cx.net)
                    .await?;
                println!(
                    "TX | create {account} ({near} NEAR) | hash {}",
                    r.transaction().get_hash()
                );
                r.assert_success();
            }
        }
        // 3. Deploy the labelled stand-ins and the Intents fixture.
        "externals" => {
            let deploys: [(AccountId, Vec<u8>, Value); 3] = [
                (sub("oracle"), wasm("mock_oracle")?, json!({})),
                (sub("usdc"), wasm("mock_ft")?, json!({})),
                (
                    sub("intents"),
                    std::fs::read(INTENTS_WASM)?,
                    json!({ "config": {
                        "wnear_id": "wrap.testnet",
                        "fees": { "fee": 0, "fee_collector": format!("fees.{PARENT}") },
                    }}),
                ),
            ];
            for (account, code, init) in deploys {
                let r = Contract::deploy(account.clone())
                    .use_code(code)
                    .with_init_call("new", init)?
                    .with_signer(cx.signer.clone())
                    .send_to(&cx.net)
                    .await?;
                println!("TX | deploy+init {account} | hash {}", r.transaction().get_hash());
                r.assert_success();
            }
        }
        // 4. The owner DAO, a Sputnik DAO v2 with the operator as the single council member.
        "dao" => {
            let args = json!({
                "config": { "name": DAO_NAME, "purpose": "Mandate vault owner (testnet run)", "metadata": "" },
                "policy": [PARENT],
            });
            call(
                cx,
                &id(FACTORY),
                &parent,
                "create",
                json!({ "name": DAO_NAME, "args": b64(&near_sdk::serde_json::to_vec(&args)?) }),
                150,
                NearToken::from_millinear(6200),
            )
            .await?;
        }
        // 5. The vault, created, funded, deployed and initialised in one transaction, no AddKey.
        "vault" => {
            let init = json!({
                "owner": dao(),
                "mandate": mandate(),
                "signer": SIGNER,
                "intents": sub("intents"),
                "oracle": sub("oracle"),
            });
            let vault = vault_id();
            let r = Transaction::construct(parent.clone(), vault.clone())
                .add_action(Action::CreateAccount(CreateAccountAction {}))
                .add_action(Action::Transfer(TransferAction { deposit: NearToken::from_near(7) }))
                .add_action(Action::DeployContract(DeployContractAction {
                    code: wasm("mandate_vault")?,
                }))
                .add_action(Action::FunctionCall(Box::new(FunctionCallAction {
                    method_name: "new_with_externals".into(),
                    args: near_sdk::serde_json::to_vec(&init)?,
                    gas: NearGas::from_tgas(30),
                    deposit: NearToken::from_yoctonear(0),
                })))
                .with_signer(cx.signer.clone())
                .send_to(&cx.net)
                .await?;
            println!(
                "TX | atomic create+deploy+init {vault} | hash {}",
                r.transaction().get_hash()
            );
            r.assert_success();
        }
        // 6. The owner names the agent (through the DAO).
        "set-agent" => {
            owner_call(cx, "set_agent", json!({ "agent": sub("agent") }), "Set the vault agent")
                .await?;
        }
        // 7. Anyone registers the vault's derived key on the Intents verifier.
        "register" => {
            call(
                cx,
                &vault_id(),
                &parent,
                "register_intents_key",
                json!({}),
                200,
                NearToken::from_yoctonear(0),
            )
            .await?;
        }
        // 8. Deposit mock USDC into the Intents ledger for the vault.
        "fund" => {
            let amount = rest.first().map(String::as_str).unwrap_or("10000000");
            let usdc = sub("usdc");
            call(
                cx,
                &usdc,
                &parent,
                "mint",
                json!({ "account_id": parent, "amount": amount }),
                30,
                NearToken::from_yoctonear(0),
            )
            .await?;
            call(
                cx,
                &usdc,
                &parent,
                "ft_transfer_call",
                json!({
                    "receiver_id": sub("intents"),
                    "amount": amount,
                    "msg": json!({ "receiver_id": vault_id() }).to_string(),
                }),
                200,
                NearToken::from_yoctonear(1),
            )
            .await?;
        }
        // 9. Fresh oracle reports: wNEAR at $4.50 and USDC at $1.00.
        "price" => {
            // Default: the fixed stand-in prices (wNEAR $4.50, USDC $1.00), stamped now.
            // MANDATE_PRICE_JSON: a file from scripts/live-price-feed.py. Each feed is stamped
            // with its SOURCE time, so the vault's 60 s age limit applies to the real age.
            let now = now_sec();
            let (wnear_e8, usdc_e8, wnear_ts, usdc_ts, source) =
                match std::env::var("MANDATE_PRICE_JSON") {
                    Ok(path) => {
                        let v: Value = near_sdk::serde_json::from_slice(&std::fs::read(&path)?)?;
                        let wts = feed_timestamp(&v, "NEAR", now)?;
                        let uts = feed_timestamp(&v, "USDC", now)?;
                        let w =
                            v["wnear_usd_e8"].as_str().ok_or("price file: wnear_usd_e8 missing")?;
                        let u =
                            v["usdc_usd_e8"].as_str().ok_or("price file: usdc_usd_e8 missing")?;
                        let src = v["source"].as_str().unwrap_or("unknown");
                        let src = format!(
                            "live: {src}; NEAR quote {} s old, USDC quote {} s old",
                            now.saturating_sub(wts),
                            now.saturating_sub(uts)
                        );
                        (w.to_string(), u.to_string(), wts, uts, src)
                    }
                    Err(_) => (
                        "450000000".to_string(),
                        "100000000".to_string(),
                        now,
                        now,
                        "stand-in constants (not market prices)".to_string(),
                    ),
                };
            println!(
                "PRICE | wNEAR {wnear_e8} @ {wnear_ts} | USDC {usdc_e8} @ {usdc_ts} | e8 units | source {source}"
            );
            for (feed, mult, ts) in
                [(WNEAR_FEED, wnear_e8, wnear_ts), (USDC_FEED, usdc_e8, usdc_ts)]
            {
                call(
                    cx,
                    &sub("oracle"),
                    &parent,
                    "set_report",
                    json!({ "asset_id": feed, "multiplier": mult, "decimals": 8, "timestamp_sec": ts.to_string() }),
                    30,
                    NearToken::from_yoctonear(0),
                )
                .await?;
            }
        }
        // 10. The agent proposes one trade; a permissionless caller executes it.
        "propose" => {
            // Default: 0.45 wNEAR for 2 USDC (the stand-in price). An optional argument
            // sets min_amount_out in yoctoWNEAR, for a live price.
            let min_out = rest.first().map(String::as_str).unwrap_or("450000000000000000000000");
            call(
                cx,
                &vault_id(),
                &sub("agent"),
                "propose",
                json!({
                    "asset_in": usdc_asset(),
                    "asset_out": WNEAR,
                    "amount_in": "2000000",
                    "min_amount_out": min_out,
                    "reason_hash": REASON,
                    "model_id": "manual-v1",
                }),
                30,
                NearToken::from_yoctonear(0),
            )
            .await?;
        }
        "execute" => {
            call(cx, &vault_id(), &parent, "execute", json!({}), 300, NearToken::from_yoctonear(0))
                .await?;
        }
        // 11. Queue one withdrawal through the DAO, then apply it after the real delay.
        "withdraw-queue" => {
            let amount = rest.first().map(String::as_str).unwrap_or("1000000");
            owner_call(
                cx,
                "request_withdrawal",
                json!({ "asset_id": usdc_asset(), "receiver": PARENT, "amount": amount }),
                "Queue one withdrawal",
            )
            .await?;
            println!(
                "PENDING | {}",
                view(cx, &vault_id(), "get_pending_withdrawal", json!({})).await?
            );
        }
        "withdraw-apply" => {
            call(
                cx,
                &vault_id(),
                &parent,
                "apply_pending_withdrawal",
                json!({}),
                290,
                NearToken::from_yoctonear(0),
            )
            .await?;
        }
        "status" => {
            let vault = vault_id();
            for m in [
                "get_owner",
                "get_agent",
                "get_externals",
                "is_key_registered",
                "get_chain_time_sec",
                "is_paused",
                "is_in_flight",
                "get_pending_proposal",
                "get_signed_intent",
                "get_last_proposal_outcome",
                "get_pending_withdrawal",
                "get_withdrawal_status",
                "get_last_withdrawal_outcome",
            ] {
                println!("VIEW | {m} | {}", view(cx, &vault, m, json!({})).await?);
            }
            let balances = view(
                cx,
                &sub("intents"),
                "mt_batch_balance_of",
                json!({ "account_id": vault, "token_ids": [format!("nep141:{}", sub("usdc"))] }),
            )
            .await;
            println!("VIEW | intents ledger (nep141 usdc) | {balances:?}");
        }
        // 0. Build the three WASM files from the current source into target/testnet-wasm.
        "build" => {
            std::fs::create_dir_all(WASM_DIR)?;
            let here = env!("CARGO_MANIFEST_DIR");
            for (name, manifest) in [
                ("mandate_vault", format!("{here}/Cargo.toml")),
                ("mock_oracle", format!("{here}/../mock-oracle/Cargo.toml")),
                ("mock_ft", format!("{here}/../mock-ft/Cargo.toml")),
            ] {
                let opts = cargo_near_build::BuildOpts::builder()
                    .manifest_path(manifest)
                    .no_abi(true)
                    .build();
                let built = cargo_near_build::build_with_cli(opts)?;
                std::fs::copy(built.as_std_path(), format!("{WASM_DIR}/{name}.wasm"))?;
                println!(
                    "BUILD | {name} | {} bytes",
                    std::fs::metadata(format!("{WASM_DIR}/{name}.wasm"))?.len()
                );
            }
        }
        other => return Err(format!("unknown step {other}").into()),
    }
    Ok(())
}

#[tokio::main]
async fn main() -> R<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let name = args.first().ok_or("usage: testnet_run <step> [args]")?;
    let cx = load()?;
    step(&cx, name, &args[1..]).await
}
