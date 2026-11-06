import test from "node:test";
import assert from "node:assert/strict";
import { createRpcClient, createSnapshotLoader, deadlineState, formatUnits, MAX_RESPONSE_BYTES,
  readAuditSnapshot, renderSnapshot, validateAccount, validateReceiptUrl, validateRpcUrl } from "./live.mjs";

const BLOCK = "3".repeat(44), CODE = "5".repeat(44), OTHER = "6".repeat(44);
const NOW = 1793980800;
const config = { account: "vault.near", network: "mainnet", rpc: "https://rpc.mainnet.near.org", expectedCodeHash: CODE };
const rules = () => ({ assets: [
  { asset_id: "nep141:usdc.near", decimals: 6, price_feed_id: "usdc" },
  { asset_id: "nep141:wrap.near", decimals: 24, price_feed_id: "near" },
], stable_assets: ["nep141:usdc.near"], max_trade_usd: "50000000", max_daily_turnover_usd: "200000000",
  max_position_bps: 6000, max_slippage_bps: 100, max_drawdown_bps: 1500, min_trade_interval_sec: 300, max_price_age_sec: 60 });
function views() {
  return { get_chain_time_sec: NOW, get_owner: "owner.near", get_mandate: rules(), get_agent: "agent.near",
    get_externals: ["v1.signer", "intents.near", "price-oracle.near"], get_oracle_kind: "price_oracle",
    get_trading_state: { last_trade_at_sec: null, day_start_sec: NOW, day_turnover_usd: "0", high_water_mark_usd: "1500000000", signed_intent_expires_at_sec: 0 },
    is_paused: false, is_halted: false, get_strikes: 0, is_key_registered: true, is_in_flight: false,
    get_pending_proposal: null, get_signed_intent: null, get_last_proposal_outcome: null,
    get_pending_mandate: null, get_pending_agent: null, get_pending_withdrawal: null,
    get_withdrawal_status: null, get_last_withdrawal_outcome: null,
    get_withdrawal_stage: null, is_withdrawal_retry_armed: false, get_withdrawal_protocol: "unique_voucher_v1",
    get_withdrawal_recovery_accounts: ["s.vault.near", "c.vault.near"],
    mt_batch_balance_of: ["1234567", "1000000000000000000000001"] };
}
function fixture({ values = views(), chain = "mainnet", code = CODE, alter = () => {} } = {}) {
  const calls = [];
  const fetchImpl = async (url, options) => {
    assert.equal(url, config.rpc + "/");
    assert.equal(options.method, "POST"); assert.equal(options.credentials, "omit");
    const request = JSON.parse(options.body); calls.push(request);
    let result;
    if (request.method === "status") result = { chain_id: chain };
    else if (request.params.request_type === "view_account") result = { block_hash: BLOCK, block_height: 42, code_hash: code };
    else {
      const name = request.params.method_name;
      assert.ok(Object.hasOwn(values, name), `unexpected read ${name}`);
      result = { block_hash: BLOCK, block_height: 42, result: [...new TextEncoder().encode(JSON.stringify(values[name]))] };
    }
    const reply = { jsonrpc: "2.0", id: request.id, result };
    alter(reply, request);
    return new Response(JSON.stringify(reply), { status: 200, headers: { "content-type": "application/json" } });
  };
  return { fetchImpl, calls };
}
test("atomic final-block snapshot pins every subsequent vault and ledger read", async () => {
  const f = fixture(), snapshot = await readAuditSnapshot(config, f);
  assert.equal(snapshot.now, BigInt(NOW)); assert.equal(snapshot.block.height, 42n);
  assert.equal(snapshot.identity, "Matches supplied hash");
  const queries = f.calls.filter((call) => call.method === "query");
  assert.equal(queries[0].params.method_name, "get_chain_time_sec");
  assert.equal(queries[0].params.finality, "final");
  for (const query of queries.slice(1)) {
    assert.equal(query.params.block_id, BLOCK); assert.equal(query.params.finality, undefined);
    assert.notEqual(query.params.request_type, "view_access_key_list");
  }
  const ledger = queries.at(-1);
  assert.equal(ledger.params.account_id, "intents.near");
  assert.deepEqual(JSON.parse(atob(ledger.params.args_base64)), { account_id: "vault.near", token_ids: ["nep141:usdc.near", "nep141:wrap.near"] });
  assert.equal(formatUnits(snapshot.balances[1].units, 24), "1.000000000000000000000001");
});
test("wrong network stops before querying the account", async () => {
  const f = fixture({ chain: "testnet" });
  await assert.rejects(readAuditSnapshot(config, f), /Wrong chain/);
  assert.equal(f.calls.length, 1);
});
test("wrong code identity stops before reading vault views", async () => {
  const f = fixture({ code: OTHER });
  await assert.rejects(readAuditSnapshot(config, f), /Code mismatch/);
  assert.equal(f.calls.length, 3);
});
test("missing expected code never implies verified identity", async () => {
  const snapshot = await readAuditSnapshot({ ...config, expectedCodeHash: "" }, fixture());
  assert.match(snapshot.identity, /UNVERIFIED/);
  assert.match(snapshot.readiness.at(-1), /UNVERIFIED/);
});
test("oracle provider is observed explicitly and only supported names are accepted", async () => {
  const values = views(); values.get_oracle_kind = "pyth";
  values.get_mandate.assets[0].price_feed_id = "a".repeat(64);
  values.get_mandate.assets[1].price_feed_id = "b".repeat(64);
  values.get_externals[2] = "pyth-oracle.near";
  const snapshot = await readAuditSnapshot(config, fixture({ values })), container = new FakeElement("div");
  assert.equal(snapshot.oracleKind, "pyth"); renderSnapshot(container, snapshot, fakeDocument);
  assert.match(container.textContent, /Oracle provider Pyth/);
  assert.match(container.textContent, /oracle freshness\/correctness are UNVERIFIED/);
  for (const unsupported of ["Pyth", "other", null, "<img src=x onerror=bad()>"]) {
    const invalid = views(); invalid.get_oracle_kind = unsupported;
    await assert.rejects(readAuditSnapshot(config, fixture({ values: invalid })), /Invalid oracle provider/);
  }
  const wrongFeeds = views(); wrongFeeds.get_oracle_kind = "pyth";
  await assert.rejects(readAuditSnapshot(config, fixture({ values: wrongFeeds })), /Invalid Pyth feed IDs/);
  values.get_pending_mandate = { mandate: rules(), effective_sec: NOW + 86400 };
  await assert.rejects(readAuditSnapshot(config, fixture({ values })), /Invalid queued Pyth feed IDs/);
});
test("an undeployed account is rejected", async () => {
  await assert.rejects(readAuditSnapshot({ ...config, expectedCodeHash: "" }, fixture({ code: "1".repeat(32) })), /no deployed contract/);
});
test("a mixed-block view is not combined with the snapshot", async () => {
  const f = fixture({ alter(reply, request) { if (request.params?.method_name === "get_owner") reply.result.block_hash = OTHER; } });
  await assert.rejects(readAuditSnapshot(config, f), /Mixed-block/);
});
test("account metadata and ledger must have the same block height as the anchor", async () => {
  for (const target of ["view_account", "mt_batch_balance_of"]) {
    const f = fixture({ alter(reply, request) {
      if (request.params?.request_type === target || request.params?.method_name === target) reply.result.block_height = 43;
    } });
    await assert.rejects(readAuditSnapshot(config, f), /Mixed-block/);
  }
});
test("one failed member invalidates the whole read, never producing partial balances", async () => {
  const f = fixture({ alter(reply, request) {
    if (request.params?.method_name === "get_pending_agent") { delete reply.result; reply.error = { message: "untrusted error" }; }
  } });
  await assert.rejects(readAuditSnapshot(config, f), /RPC rejected/);
});
test("large token values format exactly across 6, 24 and 30 decimals", () => {
  assert.equal(formatUnits("1234567", 6), "1.234567");
  assert.equal(formatUnits("1", 24), "0.000000000000000000000001");
  assert.equal(formatUnits("1000000000000000000000000000001", 30), "1.000000000000000000000000000001");
  assert.equal(formatUnits("340282366920938463463374607431768211455", 0), "340282366920938463463374607431768211455");
  for (const value of ["-1", "1.1", "1e24", "01", "340282366920938463463374607431768211456", 1000000000000000000000000]) {
    assert.throws(() => formatUnits(value, 24), /Invalid/);
  }
});
test("deadline equality is expired, not open", () => {
  assert.deepEqual(deadlineState(100, 101), { label: "Open", remaining: 1n });
  assert.deepEqual(deadlineState(100n, 100n), { label: "Expired", remaining: 0n });
  assert.equal(deadlineState(101, 100).label, "Expired");
});
test("unknown latest history never invents rejection or settlement", async () => {
  const snapshot = await readAuditSnapshot(config, fixture());
  const container = new FakeElement("div"); renderSnapshot(container, snapshot, fakeDocument);
  const display = container.textContent;
  assert.match(display, /Proposal history unknown/); assert.match(display, /Withdrawal history unknown/);
  assert.match(display, /Settlement UNVERIFIED/); assert.doesNotMatch(display, /Proposal 0.*Rejected/);
});
test("signatures and debit observations remain distinct from settlement and delivery", async () => {
  const values = views();
  values.get_last_proposal_outcome = { proposal_id: "8", outcome: { Executed: { proposal_id: "8", expires_sec: String(NOW) } } };
  values.get_signed_intent = { proposal_id: "8", expires_sec: NOW, payload: "not-for-display", signature: "not-for-display", public_key: "not-for-display" };
  values.get_last_withdrawal_outcome = { DebitObserved: { withdrawal_id: "9" } };
  const container = new FakeElement("div");
  renderSnapshot(container, await readAuditSnapshot(config, fixture({ values })), fakeDocument);
  assert.match(container.textContent, /Signature recorded/); assert.match(container.textContent, /Stored signature · proposal 8 · expired/);
  assert.match(container.textContent, /Terminal FT delivery or refund may still be outstanding/); assert.match(container.textContent, /Settlement UNVERIFIED/);
  assert.doesNotMatch(container.textContent, /not-for-display/);
});
test("untrusted RPC strings are literal text and never become markup", async () => {
  const hostile = '<img src=x onerror="globalThis.compromised=true"><script>bad()</script>';
  const values = views();
  values.get_mandate.assets[0].price_feed_id = hostile;
  values.get_last_proposal_outcome = { proposal_id: "3", outcome: { Rejected: { code: hostile, reason: hostile } } };
  values.get_last_withdrawal_outcome = { Failed: { withdrawal_id: "4", reason: hostile } };
  const container = new FakeElement("div");
  renderSnapshot(container, await readAuditSnapshot(config, fixture({ values })), fakeDocument);
  assert.ok(container.textContent.includes(hostile)); assert.equal(container.find("img").length, 0); assert.equal(container.find("script").length, 0);
});
test("schema refuses malformed balances, unsafe numbers and incoherent outcomes", async () => {
  const mutations = [
    (v) => { v.mt_batch_balance_of = ["1"]; },
    (v) => { v.mt_batch_balance_of[0] = 1; },
    (v) => { v.get_chain_time_sec = Number.MAX_SAFE_INTEGER + 1; },
    (v) => { v.get_mandate.assets[0].decimals = 31; },
    (v) => { v.get_owner = '<svg onload="bad()">'; },
    (v) => { v.get_last_proposal_outcome = { proposal_id: "3", outcome: { Executed: { proposal_id: "4", expires_sec: "100" } } }; },
    (v) => { v.get_mandate.stable_assets = ["nep141:unknown.near"]; },
  ];
  for (const mutate of mutations) { const values = views(); mutate(values); await assert.rejects(readAuditSnapshot(config, fixture({ values })), /Invalid/); }
});
test("queued rules are shown separately and do not replace current rules", async () => {
  const values = views(), queued = rules();
  queued.max_trade_usd = "75000000"; queued.max_price_age_sec = 120;
  values.get_pending_mandate = { mandate: queued, effective_sec: NOW + 86400 };
  const snapshot = await readAuditSnapshot(config, fixture({ values })), container = new FakeElement("div");
  renderSnapshot(container, snapshot, fakeDocument);
  assert.equal(snapshot.mandate.max_trade_usd, 50000000n);
  assert.match(container.textContent, /Inspect queued mandate · not yet applied/);
  assert.match(container.textContent, /\$50/); assert.match(container.textContent, /\$75/);
  assert.match(container.textContent, /Waiting 86400s/);
});
test("withdrawal phase uses recorded baseline; maturity equality does not bypass guards", async () => {
  for (const balance of [null, "100"]) {
    const values = views(), request = { asset_id: "nep141:usdc.near", receiver: "owner.near", amount: "20", effective_sec: NOW };
    values.get_pending_withdrawal = request;
    values.get_withdrawal_status = { id: "3", request_seq: "4", request, started_sec: NOW - 300, balance_before: balance };
    values.get_withdrawal_stage = balance === null ? "Preparing" : "Dispatched";
    values.is_in_flight = true;
    const snapshot = await readAuditSnapshot(config, fixture({ values })), container = new FakeElement("div");
    renderSnapshot(container, snapshot, fakeDocument);
    assert.match(container.textContent, /Delay elapsed/); assert.match(container.textContent, /guard active/);
    assert.ok(container.textContent.includes(balance === null ? "Preparing receipt / not sent" : "Dispatched / unresolved"));
  }
});
test("incoherent receipt phase, authority or sink observations are refused", async () => {
  for (const change of [
    (v) => { v.get_withdrawal_stage = "Dispatched"; },
    (v) => { v.get_withdrawal_protocol = "unknown"; },
    (v) => { v.is_withdrawal_retry_armed = true; },
    (v) => { v.get_withdrawal_recovery_accounts = ["other.near", "c.vault.near"]; },
  ]) {
    const values = views(); change(values);
    await assert.rejects(readAuditSnapshot(config, fixture({ values })), /Invalid/);
  }
});

test("a safely requeued withdrawal shows that owner authorization is still required", async () => {
  const values = views();
  values.get_pending_withdrawal = { asset_id: "nep141:usdc.near", receiver: "owner.near",
    amount: "20000000", effective_sec: NOW };
  const snapshot = await readAuditSnapshot(config, fixture({ values }));
  const container = new FakeElement("div"); renderSnapshot(container, snapshot, fakeDocument);
  assert.match(container.textContent, /Delay elapsed/);
  assert.match(container.textContent, /Unarmed; a safe retry requires owner authorization/);
  assert.equal(snapshot.withdrawalRetryArmed, false);
});

test("account, RPC and receipt links reject executable URLs, credentials and wrong explorers", () => {
  assert.equal(validateAccount("agent-v1.near"), "agent-v1.near");
  for (const value of ["a", "a..near", "a_-b.near", "<img>", "Agent.near", "a".repeat(65)]) assert.throws(() => validateAccount(value), /Invalid/);
  for (const value of ["javascript:alert(1)", "http://example.com", "https://user:password@example.com", "https://example.com?credential=value"]) assert.throws(() => validateRpcUrl(value, "mainnet"), /Invalid/);
  assert.equal(validateRpcUrl("http://127.0.0.1:8124", "local-fixture"), "http://127.0.0.1:8124/");
  assert.throws(() => validateRpcUrl("http://example.com", "local-fixture"), /Invalid/);
  const receipt = `https://nearblocks.io/txns/${BLOCK}`;
  assert.equal(validateReceiptUrl(receipt, "mainnet"), receipt);
  for (const value of ["javascript:alert(1)", receipt + "?redirect=evil", receipt.replace("nearblocks.io", "nearblocks.io.evil.example"), receipt.replace("https", "http")]) assert.throws(() => validateReceiptUrl(value, "mainnet"), /Invalid/);
  assert.throws(() => validateReceiptUrl(receipt, "testnet"), /Invalid/);
});
test("receipt references are links with no settlement inference", async () => {
  const snapshot = await readAuditSnapshot({ ...config, receiptUrl: `https://nearblocks.io/txns/${BLOCK}` }, fixture());
  const container = new FakeElement("div"); renderSnapshot(container, snapshot, fakeDocument);
  assert.equal(container.find("a")[0].href, snapshot.receipt);
  assert.equal(container.find("a")[0].rel, "noopener noreferrer");
  assert.match(container.textContent, /receipt reference — unverified/);
});
test("RPC transport rejects invalid envelopes, malformed bytes and oversized responses", async () => {
  for (const mode of ["id", "bytes", "large"]) {
    const f = fixture({ alter(reply, request) {
      if (mode === "id") reply.id = 999;
      if (mode === "bytes" && request.params?.method_name === "get_chain_time_sec") reply.result.result = [256];
      if (mode === "large") reply.padding = "x".repeat(MAX_RESPONSE_BYTES);
    } });
    await assert.rejects(readAuditSnapshot(config, f), /invalid envelope|Invalid|size limit/);
  }
});
test("timeout aborts the RPC and no snapshot is accepted", async () => {
  const fetchImpl = (_url, { signal }) => new Promise((_resolve, reject) => signal.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")), { once: true }));
  await assert.rejects(readAuditSnapshot(config, { fetchImpl, timeoutMs: 5 }), /timed out/);
});
test("read-only client cannot request mutation or account access-key lists", async () => {
  const read = createRpcClient({ ...config, fetchImpl: async () => { throw new Error("must not fetch"); } });
  await assert.rejects(read("broadcast_tx_commit", {}), /read-only/);
  await assert.rejects(read("query", { request_type: "view_access_key_list" }), /read-only/);
});
test("out-of-order responses and cancelled reads never replace newer state", async () => {
  const pending = [], rendered = [], errors = [];
  const loader = createSnapshotLoader(() => new Promise((resolve, reject) => pending.push({ resolve, reject })), {
    success: (value) => rendered.push(value), error: (error) => errors.push(error.message),
  });
  const first = loader.run("first"), second = loader.run("second");
  pending[1].resolve("new"); await second; pending[0].resolve("old"); await first;
  assert.deepEqual(rendered, ["new"]);
  const third = loader.run("third"); loader.cancel(); pending[2].reject(new Error("obsolete")); await third;
  assert.deepEqual(errors, []);
});

// The test DOM deliberately throws on HTML assignment. A hostile string can
// become text, but creating elements from it is impossible through this API.
class FakeElement {
  constructor(tag) { this.tag = tag; this.children = []; this.value = ""; }
  set textContent(value) { this.value = String(value); this.children = []; }
  get textContent() { return this.value + this.children.map((child) => child.textContent).join(" "); }
  set innerHTML(_value) { throw new Error("HTML assignment is forbidden for observed data"); }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = children; this.value = ""; }
  find(tag) { return [...(this.tag === tag ? [this] : []), ...this.children.flatMap((child) => child.find(tag))]; }
}
const fakeDocument = { createElement: (tag) => new FakeElement(tag) };
