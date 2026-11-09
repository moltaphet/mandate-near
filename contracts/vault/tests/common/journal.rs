//! Fresh journal worlds use the source-owned, modified Defuse WASM. The old
//! mainnet verifier fixture and legacy world remain separate and unchanged.
#![allow(dead_code)]

use crate::common::{SIGNER_SEED_HEX, World, build_contract, create_account, mandate, view};
use mandate_vault::{
    ActiveWithdrawal, PendingWithdrawal, Vault, WITHDRAW_DELAY_SEC, WITHDRAW_RECOVERY_SEC,
};
use mandate_withdrawal_journal::{
    Hex32, JournalProtocolDescriptor, JournalWithdrawalRecord, JournalWithdrawalRequest,
    JournalWithdrawalState, MAX_JOURNAL_RESPONSE_BYTES,
};
use near_api::{Account, Contract, NearGas, NearToken, NetworkConfig, Signer};
use near_sandbox::{FetchData, sandbox::patch::StateRecord};
use near_sdk::base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use near_sdk::borsh;
use near_sdk::json_types::{U64, U128};
use near_sdk::serde_json::{Value, json};
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::{PromiseOrValue, PromiseResult, env, testing_env};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const DOMAIN_BYTE: u8 = 0x27;
const BINDING_KEY: &[u8] = b"\0mandate:vault:withdrawal-journal:v1:binding";
const RECORD_PREFIX: &[u8] = b"\0mandate:withdrawal-journal:v1:record:";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Requires a separately recorded build binding; absent/stale artifacts fail
/// explicitly. This local provenance check is not an independent audit claim.
pub(super) fn executor_wasm() -> testresult::TestResult<Vec<u8>> {
    let root = repo_root();
    let package = root.join("integrations/intents-executor");
    let target = root.join("target/intents-executor");
    let source: Value =
        near_sdk::serde_json::from_slice(&std::fs::read(package.join("source-manifest.json"))?)?;
    let build: Value =
        near_sdk::serde_json::from_slice(&std::fs::read(target.join("build-manifest.json"))?)?;
    let source_root =
        Path::new(build["source_root"].as_str().expect("recorded executor source_root"));
    assert!(source_root.is_absolute(), "build source root must be absolute");
    let preparation: Value = near_sdk::serde_json::from_slice(&std::fs::read(
        source_root.join(".mandate-preparation.json"),
    )?)?;
    assert_eq!(
        preparation["inputs"], build["inputs"],
        "executor build inputs must match the prepared source"
    );
    let inputs = &preparation["inputs"];
    assert_eq!(inputs["recipe"], source["recipe"]);
    assert_eq!(inputs["archive_commit"], source["upstream"]["commit"]);
    assert_eq!(inputs["archive_sha256"], source["upstream"]["archive_sha256"]);
    assert_eq!(inputs["archive_bytes"], source["upstream"]["archive_bytes"]);
    assert_eq!(inputs["overlay_files"], source["overlay_files"]);
    assert_eq!(inputs["shared_crate_files"], source["shared_crate_files"]);
    assert_eq!(inputs["vendor_crate"], source["vendor_crate"]);
    let vendor = &source["vendor_crate"];
    let vendor_source = package.join(vendor["source"].as_str().expect("pinned vendor source"));
    let vendor_destination =
        source_root.join(vendor["destination"].as_str().expect("pinned vendor destination"));
    for (path, hash) in vendor["original_files"].as_object().expect("original vendor hashes") {
        assert_eq!(
            hash,
            &json!(sha256(&std::fs::read(vendor_source.join(path))?)),
            "vendor source changed: {path}"
        );
    }
    for (path, hash) in vendor["staged_files"].as_object().expect("staged vendor hashes") {
        assert_eq!(
            hash,
            &json!(sha256(&std::fs::read(vendor_destination.join(path))?)),
            "staged vendor changed: {path}"
        );
    }
    assert_eq!(inputs["lock_source"], "reviewed-package-lock");
    assert_eq!(
        inputs["effective_lock_sha256"],
        sha256(&std::fs::read(package.join("Cargo.lock"))?)
    );
    for (path, expected) in preparation["files"].as_object().expect("prepared source hash map") {
        let actual = std::fs::read(source_root.join(path))?;
        assert_eq!(expected["bytes"], actual.len(), "prepared source size changed: {path}");
        assert_eq!(expected["sha256"], sha256(&actual), "prepared source hash changed: {path}");
    }
    for (path, hash) in source["overlay_files"].as_object().expect("overlay hash map") {
        assert_eq!(hash, &json!(sha256(&std::fs::read(package.join("overlay").join(path))?)));
    }
    for (path, hash) in source["shared_crate_files"].as_object().expect("shared hash map") {
        assert_eq!(
            hash,
            &json!(sha256(&std::fs::read(root.join("crates/withdrawal-journal").join(path))?))
        );
    }
    let code = std::fs::read(target.join("artifact/defuse.wasm"))?;
    assert!(code.starts_with(b"\0asm"), "executor artifact must be WASM");
    assert_eq!(build["artifact"]["bytes"], code.len());
    assert_eq!(build["artifact"]["sha256"], sha256(&code));
    println!("ARTIFACT | actual modified Defuse | {} bytes | sha256 {}", code.len(), sha256(&code));
    Ok(code)
}

#[derive(Clone, Copy)]
pub enum BindingCase {
    Valid,
    DifferentDomain,
    NotAdmitted,
    PlainExecutor,
}

pub async fn world() -> testresult::TestResult<World> {
    world_with_binding(BindingCase::Valid).await
}

pub async fn world_with_binding(case: BindingCase) -> testresult::TestResult<World> {
    world_with_profile(case, false).await
}

/// Distinct longest supported account IDs: the shared Vault initializer keeps
/// its 62-byte sink-name bound; ordinary executor/token/receiver IDs allow 64.
/// Nineteen additional immutable admission entries need no deployed accounts.
pub async fn maximum_world() -> testresult::TestResult<World> {
    world_with_profile(BindingCase::Valid, true).await
}

async fn world_with_profile(case: BindingCase, maximum: bool) -> testresult::TestResult<World> {
    let intents_wasm = executor_wasm()?;
    let vault_wasm = build_contract(None)?;
    let mpc_wasm = build_contract(Some("../mock-externals/Cargo.toml"))?;
    let oracle_wasm = build_contract(Some("../mock-oracle/Cargo.toml"))?;
    let ft_wasm = build_contract(Some("../mock-ft/Cargo.toml"))?;
    let sandbox = near_sandbox::Sandbox::start_sandbox().await?;
    let network = NetworkConfig::from_rpc_url("sandbox", sandbox.rpc_addr.parse()?);
    let signer = Signer::from_secret_key(
        near_sandbox::config::DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY.parse()?,
    )?;
    let owner = create_account(&sandbox, "owner.sandbox", 10).await?;
    let agent = create_account(&sandbox, "agent.sandbox", 10).await?;
    let receiver_name =
        if maximum { format!("{}.sandbox", "r".repeat(56)) } else { "stranger.sandbox".into() };
    let vault_name =
        if maximum { format!("{}.sandbox", "v".repeat(54)) } else { "vault.sandbox".into() };
    let token_name =
        if maximum { format!("{}.sandbox", "t".repeat(56)) } else { "usdc.sandbox".into() };
    let executor_name =
        if maximum { format!("{}.sandbox", "e".repeat(56)) } else { "intents.sandbox".into() };
    let stranger = create_account(&sandbox, &receiver_name, 10).await?;
    let vault_id = create_account(&sandbox, &vault_name, 10).await?;
    let mpc_id = create_account(&sandbox, "mpc.sandbox", 10).await?;
    let oracle_id = create_account(&sandbox, "oracle.sandbox", 10).await?;
    let usdc_id = create_account(&sandbox, &token_name, 10).await?;
    let intents_id = create_account(&sandbox, &executor_name, 100).await?;
    let domain = Hex32::from_bytes([DOMAIN_BYTE; 32]);
    let config = json!({ "wnear_id": "wrap.sandbox", "fees": { "fee": 0, "fee_collector": "fees.sandbox" } });
    let (init, args) = if matches!(case, BindingCase::PlainExecutor) {
        ("new", json!({ "config": config }))
    } else {
        let admitted = if matches!(case, BindingCase::NotAdmitted) { &stranger } else { &vault_id };
        let mut authorized_vaults = vec![admitted.clone()];
        if maximum {
            for id in 0..19 {
                authorized_vaults.push(format!("a{id:02}{}.sandbox", "x".repeat(53)).parse()?);
            }
        }
        (
            "new_with_withdrawal_journal",
            json!({ "config": config, "ledger_domain": domain, "authorized_vaults": authorized_vaults }),
        )
    };
    Contract::deploy(intents_id.clone())
        .use_code(intents_wasm)
        .with_init_call(init, args)?
        .with_signer(signer.clone())
        .send_to(&network)
        .await?
        .assert_success();
    Contract::deploy(mpc_id.clone())
        .use_code(mpc_wasm)
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
    let pinned_domain = if matches!(case, BindingCase::DifferentDomain) {
        Hex32::from_bytes([DOMAIN_BYTE + 1; 32])
    } else {
        domain
    };
    let mut policy = mandate();
    let asset = format!("nep141:{usdc_id}");
    policy["assets"][1]["asset_id"] = json!(asset);
    policy["stable_assets"] = json!([asset]);
    Contract::deploy(vault_id.clone()).use_code(vault_wasm)
        .with_init_call("new_with_journal", json!({ "owner": owner, "mandate": policy, "journal": {
            "signer": mpc_id, "intents": intents_id, "oracle": oracle_id, "oracle_kind": "price_oracle", "ledger_domain": pinned_domain,
        } }))?.with_signer(signer.clone()).send_to(&network).await?.assert_success();
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
        extra: 0,
    })
}

pub struct Observed {
    pub answer: Value,
    pub gas: u64,
    pub token_receipts: usize,
    pub executor_receipts: usize,
    pub failures: usize,
}

pub fn asset_id(w: &World) -> String {
    format!("nep141:{}", w.usdc.account_id())
}

pub async fn send(
    w: &World,
    method: &str,
    args: Value,
    gas: u64,
    yocto: u128,
) -> Result<Observed, String> {
    let result = w
        .vault
        .call_function(method, args)
        .transaction()
        .gas(NearGas::from_tgas(gas))
        .deposit(NearToken::from_yoctonear(yocto))
        .with_signer(w.owner.clone(), w.signer.clone())
        .send_to(&w.network)
        .await
        .map_err(|e| e.to_string())?
        .into_result()
        .map_err(|e| format!("{e:?}"))?;
    Ok(Observed {
        answer: result.json::<Value>().unwrap_or(Value::Null),
        gas: result.total_gas_burnt.as_gas(),
        token_receipts: result
            .receipt_outcomes()
            .iter()
            .filter(|o| o.executor_id == *w.usdc.account_id())
            .count(),
        executor_receipts: result
            .receipt_outcomes()
            .iter()
            .filter(|o| o.executor_id == *w.intents.account_id())
            .count(),
        failures: result.receipt_failures().len(),
    })
}

/// Sandbox accounts have public fixture signing keys. Calling as the vault is
/// test control, not a claim that a sealed production vault can be impersonated.
pub async fn executor_send(
    w: &World,
    method: &str,
    request: &JournalWithdrawalRequest,
    gas: u64,
) -> Result<Observed, String> {
    let result = w
        .intents
        .call_function(method, json!({ "request": request }))
        .transaction()
        .gas(NearGas::from_tgas(gas))
        .deposit(NearToken::from_yoctonear(1))
        .with_signer(w.vault.account_id().clone(), w.signer.clone())
        .send_to(&w.network)
        .await
        .map_err(|e| e.to_string())?
        .into_result()
        .map_err(|e| format!("{e:?}"))?;
    Ok(Observed {
        answer: result.json::<Value>().unwrap_or(Value::Null),
        gas: result.total_gas_burnt.as_gas(),
        token_receipts: result
            .receipt_outcomes()
            .iter()
            .filter(|o| o.executor_id == *w.usdc.account_id())
            .count(),
        executor_receipts: result
            .receipt_outcomes()
            .iter()
            .filter(|o| o.executor_id == *w.intents.account_id())
            .count(),
        failures: result.receipt_failures().len(),
    })
}

pub async fn ledger_balance(w: &World) -> u128 {
    view(
        &w.intents,
        &w.network,
        "mt_balance_of",
        json!({ "account_id": w.vault.account_id(), "token_id": asset_id(w) }),
    )
    .await
    .as_str()
    .expect("ledger string")
    .parse()
    .expect("ledger u128")
}

pub async fn receiver_balance(w: &World) -> u128 {
    view(&w.usdc, &w.network, "ft_balance_of", json!({ "account_id": w.stranger }))
        .await
        .as_str()
        .expect("FT string")
        .parse()
        .expect("FT u128")
}

pub async fn storage_usage(w: &World) -> testresult::TestResult<u64> {
    Ok(Account(w.intents.account_id().clone())
        .view()
        .fetch_from(&w.network)
        .await?
        .data
        .storage_usage)
}

pub async fn status(w: &World, id: U64) -> Option<JournalWithdrawalRecord> {
    near_sdk::serde_json::from_value(
        view(
            &w.intents,
            &w.network,
            "get_journal_withdrawal_status",
            json!({
                "vault_id": w.vault.account_id(), "withdrawal_id": id,
            }),
        )
        .await,
    )
    .expect("strict journal record")
}

pub async fn block_receiver(w: &World, blocked: bool) -> testresult::TestResult<()> {
    w.usdc
        .call_function(
            "set_blocked_receiver",
            json!({ "receiver_id": if blocked { json!(w.stranger) } else { Value::Null } }),
        )
        .transaction()
        .gas(NearGas::from_tgas(30))
        .with_signer(w.stranger.clone(), w.signer.clone())
        .send_to(&w.network)
        .await?
        .assert_success();
    Ok(())
}

pub async fn chain_now(w: &World) -> u64 {
    view(&w.vault, &w.network, "get_chain_time_sec", json!({})).await.as_u64().expect("chain time")
}

async fn root_state(w: &World) -> testresult::TestResult<Vec<u8>> {
    let fetched = w
        ._sandbox
        .patch_state(w.vault.account_id().as_str().parse()?)
        .fetch_from(&w._sandbox.rpc_addr, FetchData::new().storage())
        .await?;
    let roots: Vec<_> = fetched
        .state
        .into_iter()
        .filter_map(|r| match r {
            StateRecord::Data { data_key_base64, value_base64, .. }
                if data_key_base64 == BASE64.encode(b"STATE") =>
            {
                Some(value_base64)
            }
            _ => None,
        })
        .collect();
    assert_eq!(roots.len(), 1, "one root STATE");
    let bytes = BASE64.decode(&roots[0])?;
    let vault: Vault = borsh::from_slice(&bytes)?;
    assert_eq!(borsh::to_vec(&vault)?, bytes, "root must round-trip");
    Ok(bytes)
}

/// Changes only the full PendingWithdrawal's final effective_sec bytes. The
/// real early call still checks its real 24-hour deadline. This sandbox fixture
/// is never evidence of elapsed wall time and preserves the separate binding.
pub async fn mature(w: &World, pending: &PendingWithdrawal) -> testresult::TestResult<()> {
    let original = root_state(w).await?;
    let request = borsh::to_vec(pending)?;
    let offsets: Vec<_> = original
        .windows(request.len())
        .enumerate()
        .filter_map(|(i, b)| (b == request).then_some(i))
        .collect();
    assert_eq!(offsets.len(), 1);
    let offset = offsets[0] + request.len() - 8;
    let mut patched = original.clone();
    let now = chain_now(w).await;
    patched[offset..offset + 8].copy_from_slice(&now.to_le_bytes());
    assert_eq!(patched[..offset], original[..offset]);
    assert_eq!(patched[offset + 8..], original[offset + 8..]);
    let decoded: Vault = borsh::from_slice(&patched)?;
    assert_eq!(borsh::to_vec(&decoded)?, patched);
    let binding_before =
        view(&w.vault, &w.network, "get_withdrawal_journal_binding", json!({})).await;
    w._sandbox
        .patch_state(w.vault.account_id().as_str().parse()?)
        .storage(BASE64.encode(b"STATE"), BASE64.encode(patched))
        .send()
        .await?;
    let mut expected = pending.clone();
    expected.effective_sec = now;
    assert_eq!(
        view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
        json!(expected)
    );
    assert_eq!(
        view(&w.vault, &w.network, "get_withdrawal_journal_binding", json!({})).await,
        binding_before
    );
    println!("CLOCK | sandbox-only STATE maturity fixture | only effective_sec patched");
    Ok(())
}

pub async fn queue_mature(w: &World, amount: u128) -> testresult::TestResult<PendingWithdrawal> {
    send(
        w,
        "request_withdrawal",
        json!({ "asset_id": asset_id(w), "receiver": w.stranger, "amount": amount.to_string() }),
        30,
        1,
    )
    .await?;
    let pending: PendingWithdrawal = near_sdk::serde_json::from_value(
        view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
    )?;
    let early = send(w, "apply_pending_withdrawal", json!({}), 280, 0)
        .await
        .err()
        .expect("real queued delay refuses early send");
    assert!(early.contains("withdrawal is still in its delay"), "{early}");
    mature(w, &pending).await?;
    Ok(near_sdk::serde_json::from_value(
        view(&w.vault, &w.network, "get_pending_withdrawal", json!({})).await,
    )?)
}

fn context(w: &World, second: u64, private: bool, deposit: u128, results: Vec<PromiseResult>) {
    testing_env!(
        VMContextBuilder::new()
            .current_account_id(w.vault.account_id().as_str().parse().unwrap())
            .predecessor_account_id(
                if private { w.vault.account_id() } else { &w.owner }.as_str().parse().unwrap()
            )
            .attached_deposit(NearToken::from_yoctonear(deposit))
            .prepaid_gas(NearGas::from_tgas(300))
            .block_timestamp(second * 1_000_000_000)
            .build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        results
    );
}

/// Installs only root state generated with the real Vault public transitions
/// and actual executor view data. Promises are discarded in the native fixture.
/// An actual WASM executor call then establishes the external durable result.
/// This is a labelled missing-vault-callback prestate, not physical receipt loss.
pub async fn install_dispatched_fixture(
    w: &World,
    amount: u128,
) -> testresult::TestResult<JournalWithdrawalRequest> {
    let original = root_state(w).await?;
    let mut vault: Vault = borsh::from_slice(&original)?;
    assert!(vault.get_withdrawal_status().is_none());
    let binding_value =
        view(&w.vault, &w.network, "get_withdrawal_journal_binding", json!({})).await;
    let binding: JournalProtocolDescriptor =
        near_sdk::serde_json::from_value(binding_value.clone())?;
    let descriptor =
        view(&w.intents, &w.network, "get_withdrawal_journal_protocol", json!({})).await;
    let admission = view(&w.intents, &w.network, "get_journal_authorized_vaults", json!({})).await;
    let started =
        chain_now(w).await.checked_sub(WITHDRAW_RECOVERY_SEC + 1).expect("fixture timestamp");
    near_sdk::mock::with_mocked_blockchain(|b| {
        b.take_storage();
    });
    context(w, started - WITHDRAW_DELAY_SEC, false, 1, vec![]);
    env::storage_write(BINDING_KEY, &borsh::to_vec(&binding)?);
    vault.request_withdrawal(asset_id(w), w.stranger.as_str().parse()?, U128(amount));
    context(w, started, false, 0, vec![]);
    drop(vault.apply_pending_withdrawal());
    let id = vault.get_withdrawal_status().unwrap().id;
    context(
        w,
        started,
        true,
        0,
        vec![
            PromiseResult::Successful(near_sdk::serde_json::to_vec(&descriptor)?),
            PromiseResult::Successful(near_sdk::serde_json::to_vec(&admission)?),
        ],
    );
    assert!(matches!(vault.on_journal_ready(id), PromiseOrValue::Promise(_)));
    let active: ActiveWithdrawal = vault.get_withdrawal_status().unwrap();
    assert_eq!(vault.get_withdrawal_stage(), Some(mandate_vault::WithdrawalStage::Dispatched));
    let patched = borsh::to_vec(&vault)?;
    assert_eq!(borsh::to_vec(&borsh::from_slice::<Vault>(&patched)?)?, patched);
    w._sandbox
        .patch_state(w.vault.account_id().as_str().parse()?)
        .storage(BASE64.encode(b"STATE"), BASE64.encode(patched))
        .send()
        .await?;
    assert_eq!(view(&w.vault, &w.network, "get_withdrawal_status", json!({})).await, json!(active));
    assert_eq!(
        view(&w.vault, &w.network, "get_withdrawal_journal_binding", json!({})).await,
        binding_value
    );
    println!(
        "FIXTURE | sandbox-only missing-vault-callback Dispatched prestate | actual executor views, native public vault transitions"
    );
    Ok(JournalWithdrawalRequest {
        withdrawal_id: active.id,
        request_seq: active.request_seq,
        token: w.usdc.account_id().as_str().parse()?,
        receiver_id: active.request.receiver,
        amount: active.request.amount,
    })
}

/// Models an unavailable terminal fact by replacing one actual terminal record
/// with Pending in sandbox storage. Does not modify custody balances or the
/// vault guard, and never claims the real resolver receipt was physically lost.
pub async fn install_pending_fact_fixture(
    w: &World,
    terminal: &JournalWithdrawalRecord,
) -> testresult::TestResult<()> {
    assert!(matches!(terminal.state, JournalWithdrawalState::Delivered { .. }));
    let mut pending = terminal.clone();
    pending.state = JournalWithdrawalState::Pending;
    let body = near_sdk::serde_json::to_vec(&pending)?;
    assert!(body.len() <= MAX_JOURNAL_RESPONSE_BYTES);
    let mut slot = vec![0; 4 + MAX_JOURNAL_RESPONSE_BYTES];
    slot[..4].copy_from_slice(&(body.len() as u32).to_le_bytes());
    slot[4..4 + body.len()].copy_from_slice(&body);
    let name = w.vault.account_id().as_str();
    let mut input = (name.len() as u32).to_le_bytes().to_vec();
    input.extend_from_slice(name.as_bytes());
    input.extend_from_slice(&terminal.request.withdrawal_id.0.to_le_bytes());
    let mut key = RECORD_PREFIX.to_vec();
    key.extend_from_slice(&Sha256::digest(input));
    w._sandbox
        .patch_state(w.intents.account_id().as_str().parse()?)
        .storage(BASE64.encode(key), BASE64.encode(slot))
        .send()
        .await?;
    assert_eq!(status(w, terminal.request.withdrawal_id).await, Some(pending));
    println!(
        "FIXTURE | sandbox-only unavailable terminal fact | one padded executor record set Pending, no balance patch"
    );
    Ok(())
}
