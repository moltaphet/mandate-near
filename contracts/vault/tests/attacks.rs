//! Attacks on a live vault. Each test plays one attack against the real contract
//! on a local NEAR sandbox and checks that the vault refuses it. `scripts/attack-demo.sh`
//! runs them in order and records the table in `docs/evidence/attacks.txt`.

mod common;

use common::{
    AMOUNT_IN, MIN_OUT, REASON, USDC, WNEAR, World, fund_vault, mandate, now_sec, register_key,
    set_prices, view, world,
};
use near_api::{AccountId, NearGas, NearToken};
use near_sdk::serde_json::{Value, json};

/// Sends one call to the vault. Returns its JSON answer, or the contract's panic message.
async fn send(
    w: &World,
    method: &str,
    args: Value,
    as_account: &AccountId,
    yocto: u128,
) -> Result<Value, String> {
    let result = w
        .vault
        .call_function(method, args)
        .transaction()
        .gas(NearGas::from_tgas(300))
        .deposit(NearToken::from_yoctonear(yocto))
        .with_signer(as_account.clone(), w.signer.clone())
        .send_to(&w.network)
        .await
        .map_err(|e| e.to_string())?;
    match result.into_result() {
        Ok(value) => Ok(value.json::<Value>().unwrap_or(Value::Null)),
        Err(e) => Err(panic_text(&format!("{e:?}"))),
    }
}

/// The part of a failure that says why: the quoted panic message, or the sandbox's guest panic.
fn panic_text(error: &str) -> String {
    let quoted = ["panicked: ", "panic_msg: "]
        .iter()
        .find_map(|marker| error.split(marker).nth(1))
        .and_then(|rest| rest.trim_start_matches(['\\', '"']).split('"').next());
    quoted.unwrap_or(error).chars().take(120).collect()
}

/// Proposes from the agent account, with the given amounts.
async fn propose(
    w: &World,
    asset_in: &str,
    amount_in: &str,
    min_out: &str,
) -> Result<Value, String> {
    send(
        w,
        "propose",
        json!({
            "asset_in": asset_in,
            "asset_out": WNEAR,
            "amount_in": amount_in,
            "min_amount_out": min_out,
            "reason_hash": REASON,
        }),
        &w.agent.clone(),
        0,
    )
    .await
}

/// The keeper's answer for the pending proposal: the closed gate, or the signature.
async fn execute(w: &World) -> Result<Value, String> {
    send(w, "execute", json!({}), &w.stranger.clone(), 0).await
}

fn report(n: u8, attack: &str, expected: &str, observed: &str) {
    println!("ATTACK {n} | {attack} | expected: {expected} | observed: {observed}");
}

fn rejection_code(outcome: &Value) -> String {
    outcome["Rejected"]["code"].as_str().unwrap_or("not rejected").to_string()
}

#[tokio::test]
async fn attack_1_a_stranger_proposes_a_trade() -> testresult::TestResult<()> {
    let w = world().await?;
    let err = send(
        &w,
        "propose",
        json!({
            "asset_in": USDC,
            "asset_out": WNEAR,
            "amount_in": AMOUNT_IN,
            "min_amount_out": MIN_OUT,
            "reason_hash": REASON,
        }),
        &w.stranger.clone(),
        0,
    )
    .await
    .expect_err("a stranger's proposal must fail");
    assert!(err.contains("only the agent"), "{err}");
    report(1, "a stranger proposes a trade", "refused: only the agent", &format!("refused: {err}"));
    Ok(())
}

#[tokio::test]
async fn attack_2_the_agent_trades_an_asset_outside_the_mandate() -> testresult::TestResult<()> {
    let w = world().await?;
    let scam = "nep141:scam.sandbox";
    let err = propose(&w, scam, AMOUNT_IN, MIN_OUT).await.expect_err("an unlisted asset must fail");
    assert!(err.contains("not in the mandate"), "{err}");
    report(
        2,
        "the agent proposes a trade in a token the owner never listed",
        "refused: not in the mandate",
        &format!("refused: {err}"),
    );
    Ok(())
}

#[tokio::test]
async fn attack_3_an_oversized_trade_passes_the_agent_and_fails_the_gate()
-> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    fund_vault(&w, 100_000_000).await?; // 100 USDC on intents.near
    set_prices(&w, now_sec()).await?;

    // $60 against a $50 per-trade cap. The price is fair, so only the size gate can stop it.
    propose(&w, USDC, "60000000", "13300000000000000000000000").await.map_err(|e| e.to_string())?;
    let outcome = execute(&w).await.map_err(|e| e.to_string())?;
    let strikes = view(&w.vault, &w.network, "get_strikes", json!({})).await;
    assert_eq!(rejection_code(&outcome), "TradeTooLarge", "{outcome}");
    assert_eq!(strikes, json!(1), "exactly one gate rejection must count one strike");
    assert_eq!(view(&w.vault, &w.network, "get_pending_proposal", json!({})).await, Value::Null);
    assert_eq!(view(&w.vault, &w.network, "get_signed_intent", json!({})).await, Value::Null);
    assert_eq!(view(&w.vault, &w.network, "is_in_flight", json!({})).await, json!(false));
    report(
        3,
        "the agent proposes $60 against a $50 per-trade cap",
        "rejected: TradeTooLarge, one strike",
        &format!("rejected: {}, strikes {strikes}", rejection_code(&outcome)),
    );
    Ok(())
}

#[tokio::test]
async fn attack_4_the_agent_accepts_a_quote_far_below_the_oracle_value()
-> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    fund_vault(&w, 10_000_000).await?;
    set_prices(&w, now_sec()).await?;

    // $2 in, 0.4 wNEAR out. At $4.50 that is $1.80, under the $1.98 floor (1% slippage).
    propose(&w, USDC, AMOUNT_IN, "400000000000000000000000").await.map_err(|e| e.to_string())?;
    let outcome = execute(&w).await.map_err(|e| e.to_string())?;
    assert_eq!(rejection_code(&outcome), "SlippageExceeded", "{outcome}");
    report(
        4,
        "the agent accepts $1.80 for $2 of USDC (1% slippage limit)",
        "rejected: SlippageExceeded",
        &format!("rejected: {}", rejection_code(&outcome)),
    );
    Ok(())
}

#[tokio::test]
async fn attack_5_the_trade_is_priced_on_an_old_quote() -> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    fund_vault(&w, 10_000_000).await?;
    // 100 s old: inside the oracle's own recency window, outside the mandate's 60 s limit.
    set_prices(&w, now_sec() - 100).await?;

    propose(&w, USDC, AMOUNT_IN, MIN_OUT).await.map_err(|e| e.to_string())?;
    let outcome = execute(&w).await.map_err(|e| e.to_string())?;
    assert_eq!(rejection_code(&outcome), "StalePrice", "{outcome}");
    report(
        5,
        "the trade is priced on a 100 s old quote (limit 60 s)",
        "rejected: StalePrice",
        &format!("rejected: {}", rejection_code(&outcome)),
    );
    Ok(())
}

#[tokio::test]
async fn attack_6_the_trade_is_priced_on_a_quote_from_the_future() -> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    fund_vault(&w, 10_000_000).await?;
    // An hour ahead of the chain clock: a feed that would never age out.
    set_prices(&w, now_sec() + 3_600).await?;

    propose(&w, USDC, AMOUNT_IN, MIN_OUT).await.map_err(|e| e.to_string())?;
    let outcome = execute(&w).await.map_err(|e| e.to_string())?;
    assert_eq!(rejection_code(&outcome), "MissingPrice", "{outcome}");
    report(
        6,
        "the trade is priced on a quote dated one hour ahead",
        "rejected: MissingPrice (no coherent current report)",
        &format!("rejected: {}", rejection_code(&outcome)),
    );
    Ok(())
}

/// The signed intent in the form `intents.near.simulate_intents` takes it.
fn signed_entry(signed: &Value, payload: &str) -> Value {
    json!({
        "standard": "raw_ed25519",
        "payload": payload,
        "public_key": signed["public_key"],
        "signature": signed["signature"],
    })
}

#[tokio::test]
async fn attack_7_a_signed_trade_is_altered_before_settlement() -> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    fund_vault(&w, 10_000_000).await?;
    set_prices(&w, now_sec()).await?;
    propose(&w, USDC, AMOUNT_IN, MIN_OUT).await.map_err(|e| e.to_string())?;
    let outcome = execute(&w).await.map_err(|e| e.to_string())?;
    assert!(outcome.get("Executed").is_some(), "{outcome}");

    // A solver or relay tries to take $3 instead of the signed $2.
    let signed = view(&w.vault, &w.network, "get_signed_intent", json!({})).await;
    let payload = signed["payload"].as_str().expect("payload");
    let control = w
        .intents
        .call_function("simulate_intents", json!({ "signed": [signed_entry(&signed, payload)] }))
        .read_only::<Value>()
        .fetch_from(&w.network)
        .await?
        .data;
    assert!(
        control["intents_executed"].to_string().contains(w.vault.account_id().as_str()),
        "{control}"
    );
    let altered = payload.replace("-2000000", "-3000000");
    assert_ne!(altered, payload, "the attack must actually alter the signed bytes");
    let err = w
        .intents
        .call_function("simulate_intents", json!({ "signed": [signed_entry(&signed, &altered)] }))
        .read_only::<Value>()
        .fetch_from(&w.network)
        .await
        .expect_err("a changed amount must not verify");
    let refusal = panic_text(&err.to_string());
    assert!(refusal.to_lowercase().contains("invalid signature"), "wrong refusal: {refusal}");
    report(
        7,
        "a relay changes the signed amount from $2 to $3",
        "rejected by intents.near: signature check",
        &format!("rejected: {refusal}"),
    );
    Ok(())
}

#[tokio::test]
async fn attack_8_a_stolen_owner_key_cannot_loosen_the_cap_before_the_delay()
-> testresult::TestResult<()> {
    let w = world().await?;
    register_key(&w).await?;
    fund_vault(&w, 100_000_000).await?;
    set_prices(&w, now_sec()).await?;

    // The stolen key raises the per-trade cap to $150. The change must wait 24 hours.
    let mut loosened = mandate();
    loosened["max_trade_usd"] = json!("150000000");
    let queued = send(&w, "set_mandate", json!({ "mandate": loosened }), &w.owner.clone(), 1)
        .await
        .map_err(|e| e.to_string())?;
    assert!(queued["Queued"].is_object(), "{queued}");

    // The $60 trade that the raised cap would allow is still refused under the old cap.
    propose(&w, USDC, "60000000", "13300000000000000000000000").await.map_err(|e| e.to_string())?;
    let outcome = execute(&w).await.map_err(|e| e.to_string())?;
    assert_eq!(rejection_code(&outcome), "TradeTooLarge", "{outcome}");

    let early = send(&w, "apply_pending_mandate", json!({}), &w.stranger.clone(), 0)
        .await
        .expect_err("the change must not apply early");
    assert!(early.contains("still in its delay"), "{early}");
    let pending = view(&w.vault, &w.network, "get_pending_mandate", json!({})).await;
    report(
        8,
        "a stolen owner key raises the trade cap from $50 to $150",
        "queued 24 h; old cap holds; early apply refused",
        &format!(
            "queued; $60 trade rejected: {}; early apply refused; pending: {}",
            rejection_code(&outcome),
            pending.is_object()
        ),
    );
    Ok(())
}
