// Public, read-only observations. No wallet, transaction or access-key RPCs.
const U64_MAX = (1n << 64n) - 1n;
const U128_MAX = (1n << 128n) - 1n;
const ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const HASH = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/;
export const MAX_RESPONSE_BYTES = 65536;
export const NETWORKS = Object.freeze({
  mainnet: { rpc: "https://free.rpc.fastnear.com", explorer: "https://nearblocks.io", chain: "mainnet" },
  testnet: { rpc: "https://test.rpc.fastnear.com", explorer: "https://testnet.nearblocks.io", chain: "testnet" },
  "local-fixture": { rpc: "http://127.0.0.1:8124", explorer: null, chain: "local-fixture" },
});

function invalid(field) { throw new Error(`Invalid ${field}; this observation was not accepted.`); }
function object(value, field) {
  if (!value || typeof value !== "object" || Array.isArray(value)) invalid(field);
  return value;
}
function text(value, field, max = 2048) {
  if (typeof value !== "string" || value.length > max) invalid(field);
  return value;
}
function boolean(value, field) { if (typeof value !== "boolean") invalid(field); return value; }
function uint(value, field, max = U64_MAX) {
  if (typeof value === "bigint") {
    if (value < 0n || value > max) invalid(field);
    return value;
  }
  if (typeof value === "number") {
    if (!Number.isSafeInteger(value) || value < 0) invalid(field);
    value = String(value);
  }
  if (typeof value !== "string" || !/^(0|[1-9][0-9]{0,38})$/.test(value)) invalid(field);
  const number = BigInt(value);
  if (number > max) invalid(field);
  return number;
}
function amount(value, field) {
  // JSON wrappers for token amounts must be decimal strings, never lossy numbers.
  if (typeof value !== "string") invalid(field);
  return uint(value, field, U128_MAX);
}
function optional(value, parse) { return value === null ? null : parse(value); }
function modelId(value) {
  if (typeof value !== "string" || !value.trim() || new TextEncoder().encode(value).length > 64) invalid("declared source ID");
  return value;
}
function modelIds(value) {
  if (!Array.isArray(value) || value.length > 8) invalid("declared source list");
  const ids = value.map(modelId);
  if (new Set(ids).size !== ids.length) invalid("duplicate declared source ID");
  return ids;
}
function withdrawalLimits(value, identifiers) {
  if (!Array.isArray(value) || value.length > identifiers.size) invalid("withdrawal ceilings");
  const seen = new Set();
  return value.map((entry) => {
    const limit = object(entry, "withdrawal ceiling"), id = asset(limit.asset_id);
    const maximum = amount(limit.max_per_request, "per-request withdrawal ceiling");
    if (!identifiers.has(id) || seen.has(id) || maximum === 0n) invalid("withdrawal ceiling asset or amount");
    seen.add(id);
    return { asset_id: id, max_per_request: maximum };
  });
}

export function validateAccount(value) {
  if (typeof value !== "string" || value.length < 2 || value.length > 64 ||
      !/^[a-z0-9]+(?:[._-][a-z0-9]+)*$/.test(value)) invalid("account ID");
  return value;
}
function asset(value) {
  if (typeof value !== "string" || !value.startsWith("nep141:")) invalid("asset ID");
  validateAccount(value.slice(7));
  return value;
}
function hash(value, field = "block hash") {
  if (typeof value !== "string" || !HASH.test(value)) invalid(field);
  let decoded = 0n;
  for (const char of value) decoded = decoded * 58n + BigInt(ALPHABET.indexOf(char));
  let bytes = 0;
  while (decoded) { bytes++; decoded >>= 8n; }
  if (bytes + value.match(/^1*/)[0].length !== 32) invalid(field);
  return value;
}
export { hash as validateHash };
export function validateRpcUrl(value, network) {
  if (!Object.hasOwn(NETWORKS, network)) invalid("network");
  if (typeof value !== "string" || value.length > 512) invalid("RPC URL");
  let url;
  try { url = new URL(value); } catch { invalid("RPC URL"); }
  if (url.username || url.password || url.search || url.hash) invalid("RPC URL");
  if (network === "local-fixture") {
    if (url.protocol !== "http:" || !["localhost", "127.0.0.1", "[::1]"].includes(url.hostname)) invalid("local fixture URL");
  } else if (url.protocol !== "https:" || ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname)) {
    invalid("public RPC URL");
  }
  return url.href;
}
export function validateReceiptUrl(value, network) {
  if (!value) return null;
  const base = NETWORKS[network]?.explorer;
  if (!base || typeof value !== "string" || value.length > 256) invalid("receipt reference");
  let url;
  try { url = new URL(value); } catch { invalid("receipt reference"); }
  if (url.origin !== base || url.username || url.password || url.search || url.hash ||
      !/^\/txns\/[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(url.pathname)) invalid("receipt reference");
  hash(url.pathname.slice(6), "receipt hash");
  return url.href;
}

export function formatUnits(value, decimals) {
  const units = typeof value === "bigint" ? value : amount(value, "amount");
  if (units < 0n || units > U128_MAX || !Number.isInteger(decimals) || decimals < 0 || decimals > 30) invalid("units");
  const digits = units.toString().padStart(decimals + 1, "0");
  if (!decimals) return digits;
  const fraction = digits.slice(-decimals).replace(/0+$/, "");
  return digits.slice(0, -decimals) + (fraction ? `.${fraction}` : "");
}
function usd(value) { return `$${formatUnits(value, 6)}`; }
function date(value) {
  if (value > 253402300799n) return `${value} Unix seconds`;
  return new Date(Number(value) * 1000).toISOString().replace("T", " ").replace(".000Z", " UTC");
}
export function deadlineState(now, deadline) {
  const chain = uint(now, "chain time"), expiry = uint(deadline, "deadline");
  return chain < expiry ? { label: "Open", remaining: expiry - chain } : { label: "Expired", remaining: 0n };
}

function mandate(raw) {
  const m = object(raw, "mandate");
  if (!Array.isArray(m.assets) || m.assets.length < 1 || m.assets.length > 20) invalid("mandate assets");
  const assets = m.assets.map((entry) => {
    const a = object(entry, "asset configuration");
    return { asset_id: asset(a.asset_id), decimals: Number(uint(a.decimals, "decimals", 30n)),
      price_feed_id: text(a.price_feed_id, "price feed", 128) };
  });
  const identifiers = new Set(assets.map((a) => a.asset_id));
  if (identifiers.size !== assets.length || !Array.isArray(m.stable_assets) || m.stable_assets.length > assets.length) invalid("stable assets");
  const stable = m.stable_assets.map(asset);
  if (new Set(stable).size !== stable.length || stable.some((id) => !identifiers.has(id))) invalid("stable subset");
  const maxTrade = uint(m.max_trade_usd, "trade cap"), daily = uint(m.max_daily_turnover_usd, "daily cap");
  if (maxTrade === 0n || maxTrade > daily) invalid("trade limits");
  return { assets, stable_assets: stable,
    allowed_model_ids: Object.hasOwn(m, "allowed_model_ids") ? modelIds(m.allowed_model_ids) : null,
    withdrawal_limits: Object.hasOwn(m, "withdrawal_limits") ? withdrawalLimits(m.withdrawal_limits, identifiers) : null,
    max_trade_usd: maxTrade, max_daily_turnover_usd: daily,
    max_position_bps: uint(m.max_position_bps, "position cap", 10000n),
    max_slippage_bps: uint(m.max_slippage_bps, "slippage cap", 500n),
    max_drawdown_bps: uint(m.max_drawdown_bps, "drawdown cap", 10000n),
    min_trade_interval_sec: uint(m.min_trade_interval_sec, "cooldown", (1n << 32n) - 1n),
    max_price_age_sec: uint(m.max_price_age_sec, "price age", 300n) };
}
function trading(raw) {
  const state = object(raw, "trading state");
  return { last_trade_at_sec: optional(state.last_trade_at_sec, (v) => uint(v, "last signing reservation")),
    day_start_sec: uint(state.day_start_sec, "turnover day"),
    day_turnover_usd: amount(state.day_turnover_usd, "reserved turnover"),
    high_water_mark_usd: amount(state.high_water_mark_usd, "high-water snapshot"),
    signed_intent_expires_at_sec: uint(state.signed_intent_expires_at_sec, "reserved intent deadline") };
}
function proposal(raw) {
  const p = object(raw, "proposal");
  const reason = Object.hasOwn(p, "reason_hash") ? text(p.reason_hash, "reason hash", 64) : null;
  if (reason !== null && !/^[0-9a-fA-F]{64}$/.test(reason)) invalid("reason hash");
  return { id: uint(p.id, "proposal ID"), asset_in: asset(p.asset_in), asset_out: asset(p.asset_out),
    amount_in: amount(p.amount_in, "proposal input"), min_amount_out: amount(p.min_amount_out, "proposal output"),
    model_id: Object.hasOwn(p, "model_id") ? modelId(p.model_id) : null, reason_hash: reason,
    created_sec: uint(p.created_sec, "proposal creation"), expires_sec: uint(p.expires_sec, "proposal expiry") };
}
function withdrawal(raw) {
  const p = object(raw, "withdrawal request");
  return { asset_id: asset(p.asset_id), receiver: validateAccount(p.receiver), amount: amount(p.amount, "withdrawal amount"),
    effective_sec: uint(p.effective_sec, "withdrawal maturity") };
}
function activeWithdrawal(raw) {
  const a = object(raw, "active withdrawal");
  return { id: uint(a.id, "withdrawal ID"), request_seq: uint(a.request_seq, "withdrawal generation"),
    request: withdrawal(a.request), started_sec: uint(a.started_sec, "withdrawal start"),
    balance_before: optional(a.balance_before, (v) => amount(v, "withdrawal baseline")) };
}
function journalBinding(raw, intents) {
  const binding = object(raw, "withdrawal journal binding");
  const fields = ["protocol_version", "ledger_domain", "executor_id", "request_hash_scheme"];
  if (Object.keys(binding).length !== fields.length || fields.some((field) => !Object.hasOwn(binding, field)) ||
      binding.protocol_version !== "durable_withdrawal_journal_v1" ||
      binding.request_hash_scheme !== "sha256-borsh-v1" ||
      typeof binding.ledger_domain !== "string" || binding.ledger_domain.length !== 64 || !/^[0-9a-f]{64}$/.test(binding.ledger_domain) ||
      /^0+$/.test(binding.ledger_domain) || validateAccount(binding.executor_id) !== intents) invalid("withdrawal journal binding");
  return { protocol_version: binding.protocol_version, ledger_domain: binding.ledger_domain,
    executor_id: binding.executor_id, request_hash_scheme: binding.request_hash_scheme };
}
function variant(raw, variants, field) {
  const data = object(raw, field), entries = Object.keys(data);
  if (entries.length !== 1 || !variants.includes(entries[0])) invalid(field);
  return [entries[0], object(data[entries[0]], field)];
}
function proposalOutcome(raw) {
  const o = object(raw, "proposal outcome"), [kind, value] = variant(o.outcome, ["Executed", "Rejected", "Failed"], "execution outcome");
  const outcome = { proposal_id: uint(o.proposal_id, "outcome proposal ID"), kind };
  if (kind === "Executed") {
    if (uint(value.proposal_id, "executed proposal ID") !== outcome.proposal_id) invalid("outcome association");
    outcome.expires_sec = uint(value.expires_sec, "signed expiry");
  } else {
    outcome.reason = text(value.reason, "execution reason");
    if (kind === "Rejected") outcome.code = text(value.code, "gate code", 128);
  }
  return outcome;
}
function withdrawalOutcome(raw) {
  const [kind, value] = variant(raw, ["Delivered", "Failed", "DebitObserved"], "withdrawal outcome");
  const outcome = { kind, withdrawal_id: uint(value.withdrawal_id, "outcome withdrawal ID") };
  if (kind === "Delivered") outcome.amount = amount(value.amount, "delivered amount");
  if (kind === "Failed") outcome.reason = text(value.reason, "withdrawal reason");
  return outcome;
}
function signed(raw) {
  const s = object(raw, "signed intent");
  // Never expose the payload, signature or any key value in this dashboard.
  return { proposal_id: uint(s.proposal_id, "signed proposal ID"), expires_sec: uint(s.expires_sec, "signed expiry") };
}

async function readBounded(response, limit) {
  const length = response.headers?.get("content-length");
  if (length && (!/^[0-9]+$/.test(length) || Number(length) > limit)) throw new Error("RPC response exceeded the size limit.");
  if (!response.body?.getReader) throw new Error("Streaming response limits are unavailable in this browser.");
  const reader = response.body.getReader(), chunks = [];
  let size = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > limit) throw new Error("RPC response exceeded the size limit.");
      chunks.push(value);
    }
  } catch (error) { await reader.cancel().catch(() => {}); throw error; }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
  try { return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)); }
  catch { throw new Error("RPC returned invalid JSON."); }
}
function decodeView(result, method) {
  const bytes = result.result;
  if (!Array.isArray(bytes) || bytes.length > MAX_RESPONSE_BYTES ||
      bytes.some((byte) => !Number.isInteger(byte) || byte < 0 || byte > 255)) invalid(`${method} response`);
  try { return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(new Uint8Array(bytes))); }
  catch { invalid(`${method} JSON`); }
}
function base64(value) {
  const bytes = new TextEncoder().encode(JSON.stringify(value));
  if (bytes.length > 8192) invalid("query arguments");
  return btoa(String.fromCharCode(...bytes));
}

export function createRpcClient({ rpc, network, fetchImpl = globalThis.fetch, timeoutMs = 8000, signal }) {
  const url = validateRpcUrl(rpc, network);
  if (!Number.isInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 15000) invalid("timeout");
  let nextId = 0;
  return async function rpcRead(method, params) {
    if (method !== "status" && (method !== "query" || !["call_function", "view_account"].includes(params?.request_type))) invalid("read-only RPC method");
    const id = ++nextId, controller = new AbortController();
    const cancel = () => controller.abort();
    signal?.addEventListener("abort", cancel, { once: true });
    if (signal?.aborted) controller.abort();
    const timer = setTimeout(cancel, timeoutMs);
    try {
      const body = JSON.stringify({ jsonrpc: "2.0", id, method, params });
      if (new TextEncoder().encode(body).length > 16384) invalid("RPC request size");
      const response = await fetchImpl(url, { method: "POST", headers: { "content-type": "application/json" },
        body, signal: controller.signal, credentials: "omit", referrerPolicy: "no-referrer", redirect: "error" });
      if (!response.ok) throw new Error("RPC HTTP request failed; check the endpoint and try again.");
      const reply = await readBounded(response, MAX_RESPONSE_BYTES);
      if (reply?.jsonrpc !== "2.0" || reply.id !== id || !Object.hasOwn(reply, "result") || Object.hasOwn(reply, "error")) {
        throw new Error("RPC rejected the read or returned an invalid envelope.");
      }
      return reply.result;
    } catch (error) {
      if (controller.signal.aborted) throw new Error(signal?.aborted ? "Read superseded." : "RPC read timed out; no snapshot was accepted.");
      if (error instanceof TypeError) throw new Error("RPC connection failed; check browser access and the endpoint.");
      throw error;
    } finally { clearTimeout(timer); signal?.removeEventListener("abort", cancel); }
  };
}

const VIEWS = ["get_owner", "get_mandate", "get_agent", "get_externals", "get_oracle_kind", "get_trading_state", "is_paused", "is_halted",
  "get_strikes", "is_key_registered", "is_in_flight", "get_pending_proposal", "get_signed_intent",
  "get_last_proposal_outcome", "get_pending_mandate", "get_pending_agent", "get_pending_withdrawal",
  "get_withdrawal_status", "get_last_withdrawal_outcome", "get_withdrawal_stage",
  "is_withdrawal_retry_armed", "get_withdrawal_protocol", "get_withdrawal_recovery_accounts"];

export async function readAuditSnapshot(config, dependencies = {}) {
  const account = validateAccount(config.account), network = config.network;
  const expectedHash = config.expectedCodeHash ? hash(config.expectedCodeHash, "expected code hash") : null;
  const receipt = validateReceiptUrl(config.receiptUrl, network);
  const read = createRpcClient({ rpc: config.rpc, network, ...dependencies });
  const status = object(await read("status", []), "network status");
  if (status.chain_id !== NETWORKS[network].chain) throw new Error("Wrong chain: the RPC does not match the selected network.");
  const query = async (target, method, args, block) => {
    const response = object(await read("query", { request_type: "call_function", account_id: target, method_name: method,
      args_base64: base64(args), ...(block ? { block_id: block.hash } : { finality: "final" }) }), "view response");
    const observedHash = hash(response.block_hash), height = uint(response.block_height, "block height");
    if (block && (observedHash !== block.hash || height !== block.height)) throw new Error("Mixed-block response: no atomic snapshot was accepted.");
    return { value: decodeView(response, method), hash: observedHash, height };
  };
  const clock = await query(account, "get_chain_time_sec", {}, null);
  const block = { hash: clock.hash, height: clock.height }, now = uint(clock.value, "chain time");
  const meta = object(await read("query", { request_type: "view_account", account_id: account, block_id: block.hash }), "account metadata");
  if (meta.block_hash !== block.hash || uint(meta.block_height, "account block height") !== block.height) throw new Error("Mixed-block account metadata: no snapshot was accepted.");
  const codeHash = hash(meta.code_hash, "deployed code hash");
  if (/^1+$/.test(codeHash)) throw new Error("The account has no deployed contract code.");
  if (expectedHash && expectedHash !== codeHash) throw new Error("Code mismatch: the deployed vault does not match the supplied expectation.");
  const responses = await Promise.all(VIEWS.map(async (method) => [method, (await query(account, method, {}, block)).value]));
  const raw = Object.fromEntries(responses);
  const rules = mandate(raw.get_mandate), externals = raw.get_externals;
  const oracleKind = raw.get_oracle_kind;
  if (oracleKind !== "price_oracle" && oracleKind !== "pyth") invalid("oracle provider");
  if (oracleKind === "pyth" && rules.assets.some((asset) => !/^[a-f0-9]{64}$/.test(asset.price_feed_id))) invalid("Pyth feed IDs");
  if (!Array.isArray(externals) || externals.length !== 3) invalid("external contracts");
  const [signer, intents, oracle] = externals.map(validateAccount);
  const withdrawalProtocol = text(raw.get_withdrawal_protocol, "withdrawal protocol", 64);
  if (!["unique_voucher_v1", "durable_withdrawal_journal_v1"].includes(withdrawalProtocol)) invalid("withdrawal protocol");
  const withdrawalJournalBinding = withdrawalProtocol === "durable_withdrawal_journal_v1"
    ? journalBinding((await query(account, "get_withdrawal_journal_binding", {}, block)).value, intents) : null;
  const balanceRaw = (await query(intents, "mt_batch_balance_of", { account_id: account,
    token_ids: rules.assets.map((a) => a.asset_id) }, block)).value;
  if (!Array.isArray(balanceRaw) || balanceRaw.length !== rules.assets.length) invalid("Intents balances");
  const snapshot = { account, network, rpc: validateRpcUrl(config.rpc, network), block, now, codeHash,
    identity: expectedHash ? "Matches supplied hash" : "UNVERIFIED — no expected code hash supplied",
    receipt, owner: validateAccount(raw.get_owner), agent: optional(raw.get_agent, validateAccount),
    externals: { signer, intents, oracle }, oracleKind, mandate: rules, trading: trading(raw.get_trading_state),
    paused: boolean(raw.is_paused, "pause state"), halted: boolean(raw.is_halted, "halt state"),
    strikes: uint(raw.get_strikes, "strike count", (1n << 32n) - 1n),
    registered: boolean(raw.is_key_registered, "key registration"), inFlight: boolean(raw.is_in_flight, "in-flight guard"),
    proposal: optional(raw.get_pending_proposal, proposal), signed: optional(raw.get_signed_intent, signed),
    proposalOutcome: optional(raw.get_last_proposal_outcome, proposalOutcome),
    pendingMandate: optional(raw.get_pending_mandate, (v) => {
      object(v, "pending mandate");
      const queued = mandate(v.mandate);
      if (oracleKind === "pyth" && queued.assets.some((asset) => !/^[a-f0-9]{64}$/.test(asset.price_feed_id))) invalid("queued Pyth feed IDs");
      return { mandate: queued, effective_sec: uint(v.effective_sec, "mandate maturity") };
    }),
    pendingAgent: optional(raw.get_pending_agent, (v) => {
      object(v, "pending agent"); return { agent: validateAccount(v.agent), effective_sec: uint(v.effective_sec, "agent maturity") };
    }),
    pendingWithdrawal: optional(raw.get_pending_withdrawal, withdrawal),
    withdrawal: optional(raw.get_withdrawal_status, activeWithdrawal),
    withdrawalStage: optional(raw.get_withdrawal_stage, (value) => {
      if (!["Preparing", "Dispatched"].includes(value)) invalid("withdrawal stage");
      return value;
    }),
    withdrawalRetryArmed: boolean(raw.is_withdrawal_retry_armed, "withdrawal retry arm"),
    withdrawalProtocol, withdrawalJournalBinding,
    recoveryAccounts: (() => {
      // This legacy tuple remains exported in journal mode but proves nothing
      // about that protocol's executor records or terminal outcomes.
      if (withdrawalJournalBinding) return null;
      const accounts = raw.get_withdrawal_recovery_accounts;
      if (!Array.isArray(accounts) || accounts.length !== 2 ||
          accounts[0] !== `s.${account}` || accounts[1] !== `c.${account}`) invalid("receipt sink accounts");
      return accounts.map(validateAccount);
    })(),
    withdrawalOutcome: optional(raw.get_last_withdrawal_outcome, withdrawalOutcome),
    balances: rules.assets.map((a, i) => ({ ...a, units: amount(balanceRaw[i], "ledger balance") })) };
  if ((snapshot.withdrawal === null) !== (snapshot.withdrawalStage === null) ||
      (snapshot.withdrawalRetryArmed && !snapshot.pendingWithdrawal) ||
      (snapshot.withdrawal && (withdrawalJournalBinding
        ? snapshot.withdrawal.balance_before !== null
        : (snapshot.withdrawal.balance_before === null) !== (snapshot.withdrawalStage === "Preparing")))) invalid("withdrawal protocol coherence");
  snapshot.readiness = deriveReadiness(snapshot);
  return snapshot;
}

export function deriveReadiness(snapshot) {
  const reasons = [];
  if (snapshot.owner === snapshot.account) reasons.push("Owner equals vault account");
  if (snapshot.paused) reasons.push("Paused");
  if (snapshot.halted) reasons.push("Proposal circuit breaker halted");
  if (!snapshot.agent) reasons.push("Agent not configured");
  if (!snapshot.registered) reasons.push("Intents key registration not recorded");
  if (snapshot.mandate.allowed_model_ids === null) reasons.push("Declared-source admission unavailable in this contract schema");
  else if (!snapshot.mandate.allowed_model_ids.length) reasons.push("All declared decision sources disabled");
  else if (snapshot.proposal?.model_id && !snapshot.mandate.allowed_model_ids.includes(snapshot.proposal.model_id)) reasons.push("Pending proposal source no longer allowed");
  if (snapshot.inFlight) reasons.push("Execution or withdrawal guard active");
  if (snapshot.proposal && snapshot.now < snapshot.proposal.expires_sec) reasons.push("Unexpired proposal pending");
  if (snapshot.now < snapshot.trading.signed_intent_expires_at_sec) reasons.push("Signing reservation deadline open");
  if (snapshot.trading.last_trade_at_sec !== null && snapshot.now < snapshot.trading.last_trade_at_sec + snapshot.mandate.min_trade_interval_sec) reasons.push("Cooldown active");
  return reasons.length ? reasons : ["No observed proposal blocker; oracle, custody and execution readiness UNVERIFIED"];
}

export function createSnapshotLoader(read, handlers) {
  let generation = 0, controller;
  return {
    cancel() { generation++; controller?.abort(); },
    async run(config) {
      controller?.abort(); controller = new AbortController();
      const current = ++generation;
      handlers.loading?.();
      try {
        const result = await read(config, { signal: controller.signal });
        if (generation === current) handlers.success?.(result);
      } catch (error) {
        if (generation === current) handlers.error?.(error);
      }
    },
  };
}

export function renderSnapshot(container, snapshot, documentRef = globalThis.document) {
  const element = (tag, value, className) => {
    const el = documentRef.createElement(tag);
    if (value !== undefined) el.textContent = String(value);
    if (className) el.className = className;
    return el;
  };
  const row = (list, label, value) => {
    const wrapper = element("div", undefined, "audit-row");
    wrapper.append(element("dt", label), element("dd", value)); list.append(wrapper);
  };
  const card = (title) => {
    const section = element("section", undefined, "audit-card");
    section.append(element("h3", title)); return section;
  };
  const ruleFields = (list, rules) => {
    row(list, "Trade cap", usd(rules.max_trade_usd)); row(list, "Daily turnover allowance", usd(rules.max_daily_turnover_usd));
    row(list, "Position / slippage", `${rules.max_position_bps} / ${rules.max_slippage_bps} bps`);
    row(list, "Drawdown", `${rules.max_drawdown_bps} bps`);
    row(list, "Cooldown / price age", `${rules.min_trade_interval_sec}s / ${rules.max_price_age_sec}s`);
    row(list, "Stable subset", rules.stable_assets.join(", ") || "None");
    row(list, "Allowed declared sources", rules.allowed_model_ids === null ? "Unavailable · legacy schema" : rules.allowed_model_ids.join(", ") || "None · proposals disabled");
    for (const asset of rules.assets) {
      const ceiling = rules.withdrawal_limits?.find((limit) => limit.asset_id === asset.asset_id);
      const value = rules.withdrawal_limits === null
        ? "Uncapped · legacy schema"
        : ceiling ? `${ceiling.max_per_request} base units per request` : "Uncapped · no ceiling configured";
      row(list, "Withdrawal ceiling", `${asset.asset_id}: ${value}`);
    }
    row(list, "Withdrawal scope", "Per request only; no aggregate or daily withdrawal cap");
  };
  const assetFields = (rules) => {
    const table = element("table"), head = element("thead"), headers = element("tr"), body = element("tbody");
    ["Asset", "Decimals", "Price feed"].forEach((value) => headers.append(element("th", value))); head.append(headers);
    rules.assets.forEach((asset) => {
      const tr = element("tr"); [asset.asset_id, asset.decimals, asset.price_feed_id].forEach((value) => tr.append(element("td", value))); body.append(tr);
    });
    table.append(head, body); const scroll = element("div", undefined, "audit-scroll"); scroll.append(table); return scroll;
  };
  const grid = element("div", undefined, "audit-grid");
  const anchor = element("section", undefined, "block-anchor");
  anchor.append(element("span", snapshot.network === "local-fixture" ? "LOCAL RPC FIXTURE · NO LIVE EVIDENCE" : `${snapshot.network.toUpperCase()} · FINAL BLOCK`, "audit-kicker"),
    element("strong", `#${snapshot.block.height}`), element("span", date(snapshot.now)),
    element("code", snapshot.block.hash), element("p", snapshot.identity));
  const controls = card("Authority and observed blockers"), controlList = element("dl");
  row(controlList, "Vault", snapshot.account); row(controlList, "Owner", snapshot.owner); row(controlList, "Agent", snapshot.agent ?? "Not configured");
  row(controlList, "Oracle provider", snapshot.oracleKind === "pyth" ? "Pyth" : "PriceOracle · aggregate/report adapter");
  row(controlList, "Controls", `${snapshot.paused ? "Paused" : "Unpaused"} · ${snapshot.halted ? "Halted" : "Not halted"} · ${snapshot.strikes} strikes`);
  row(controlList, "Registration", snapshot.registered ? "Intents registration recorded; authority not independently checked" : "Not recorded");
  row(controlList, "Guard", snapshot.inFlight ? "Active" : "No active guard reported");
  controls.append(controlList);
  const blockers = element("ul", undefined, "audit-blockers");
  snapshot.readiness.forEach((value) => blockers.append(element("li", value))); controls.append(blockers);

  const rules = card("Current mandate"), ruleList = element("dl");
  const m = snapshot.mandate;
  ruleFields(ruleList, m); rules.append(ruleList);
  const assetDetails = element("details", undefined, "audit-details");
  assetDetails.append(element("summary", "Inspect listed assets and price-feed mapping"), assetFields(m)); rules.append(assetDetails);
  rules.append(element("p", "USD fields are contract-recorded rule amounts. No current market valuation or oracle correctness is inferred.", "audit-note"));
  rules.append(element("p", "Source IDs are declarations. Their presence does not authenticate a model response or prove model origin.", "audit-note"));

  const accounting = card("Signing reservations"), accountList = element("dl");
  row(accountList, "Reserved turnover", `${usd(snapshot.trading.day_turnover_usd)} · day ${date(snapshot.trading.day_start_sec)}`);
  row(accountList, "Last reservation", snapshot.trading.last_trade_at_sec === null ? "None recorded" : date(snapshot.trading.last_trade_at_sec));
  row(accountList, "High-water snapshot", usd(snapshot.trading.high_water_mark_usd)); accounting.append(accountList);
  accounting.append(element("p", "Reserved before signing dispatch. Failed, lost or unfilled signing has no automatic quota refund. Approval-day accounting does not prove settlement-day turnover.", "audit-note"));

  const queues = card("Queued owner changes"), queueList = element("dl");
  const maturity = (value) => `${snapshot.now >= value ? "Delay elapsed" : `Waiting ${value - snapshot.now}s`} · ${date(value)}`;
  row(queueList, "Mandate", snapshot.pendingMandate ? `${snapshot.pendingMandate.mandate.assets.length} assets · ${maturity(snapshot.pendingMandate.effective_sec)}` : "None observed");
  row(queueList, "Agent", snapshot.pendingAgent ? `${snapshot.pendingAgent.agent} · ${maturity(snapshot.pendingAgent.effective_sec)}` : "None observed");
  const describeWithdrawal = (w) => `${w.amount} base units of ${w.asset_id} → ${w.receiver}`;
  row(queueList, "Withdrawal", snapshot.pendingWithdrawal ? `${describeWithdrawal(snapshot.pendingWithdrawal)} · ${maturity(snapshot.pendingWithdrawal.effective_sec)}` : "None observed");
  row(queueList, "Withdrawal attempt", snapshot.pendingWithdrawal ? (snapshot.withdrawalRetryArmed ? "One authorized attempt armed" : "Unarmed; a safe retry requires owner authorization") : (snapshot.withdrawal ? "Attempt consumed" : "No withdrawal queued"));
  queues.append(queueList);
  if (snapshot.pendingMandate) {
    const details = element("details", undefined, "audit-details"), fields = element("dl");
    ruleFields(fields, snapshot.pendingMandate.mandate);
    details.append(element("summary", "Inspect queued mandate · not yet applied"), fields, assetFields(snapshot.pendingMandate.mandate)); queues.append(details);
  }

  const timeline = card("Latest observed outcomes"), events = element("ol", undefined, "audit-timeline");
  const event = (title, detail) => {
    const item = element("li"); item.append(element("strong", title), element("p", detail)); events.append(item);
  };
  if (snapshot.proposal) {
    const p = snapshot.proposal, expiry = deadlineState(snapshot.now, p.expires_sec);
    event(`Pending proposal ${p.id} · ${expiry.label.toLowerCase()}`, `${p.amount_in} base units ${p.asset_in} → minimum ${p.min_amount_out} base units ${p.asset_out}; deadline ${date(p.expires_sec)}`);
    event("Decision evidence", `Declared source: ${p.model_id ?? "unavailable · legacy schema"}; reason hash: ${p.reason_hash ?? "unavailable"}. The hash requires the off-chain record and source-specific verifier; this page does not authenticate it.`);
  } else event("No pending proposal observed", "This does not prove there were no previous proposals.");
  if (snapshot.proposalOutcome) {
    const o = snapshot.proposalOutcome;
    event(`Proposal ${o.proposal_id} · ${o.kind === "Executed" ? "Signature recorded" : o.kind}`, o.kind === "Executed"
      ? `Recorded signature deadline ${date(o.expires_sec)}. Executed is the contract's signing outcome; settlement is UNVERIFIED.`
      : `${o.code ? `${o.code}: ` : ""}${o.reason}`);
  } else event("Proposal history unknown", "No latest outcome stored. Older history is not available through this view.");
  if (snapshot.signed) {
    const s = snapshot.signed, expiry = deadlineState(snapshot.now, s.expires_sec);
    event(`Stored signature · proposal ${s.proposal_id} · ${expiry.label.toLowerCase()}`, `Deadline ${date(s.expires_sec)}. Expiry does not prove that a trade settled or was never filled.`);
  } else event("No signature stored", "Signature absence does not prove an earlier signing request failed or that an earlier intent did not settle.");
  if (snapshot.withdrawal) {
    const w = snapshot.withdrawal;
    const preparing = snapshot.withdrawalJournalBinding ? "Checking executor / not sent" : "Preparing receipt / not sent";
    event(`Withdrawal ${w.id} · ${snapshot.withdrawalStage === "Preparing" ? preparing : "Dispatched / unresolved"}`,
      `${describeWithdrawal(w.request)}. Generation ${w.request_seq}; started ${date(w.started_sec)}. An unresolved sent result must not be blindly retried.`);
  } else event("No withdrawal guard observed", "This is current state, not complete withdrawal history.");
  if (snapshot.withdrawalOutcome) {
    const o = snapshot.withdrawalOutcome;
    event(`Withdrawal ${o.withdrawal_id} · ${o.kind === "Delivered" ? "Delivery reported" : o.kind === "DebitObserved" ? "Ledger debit observed" : "Failed"}`,
      o.kind === "Delivered" ? `Contract callback reported ${o.amount} base units delivered.` : o.kind === "DebitObserved" ? "The operation receipt proves initial ledger debit. Terminal FT delivery or refund may still be outstanding." : o.reason);
  } else event("Withdrawal history unknown", "No latest confirmed withdrawal outcome stored.");
  event("Settlement UNVERIFIED", "Neither a stored signature nor an expired intent establishes settlement. This dashboard does not authenticate execution receipts.");
  if (snapshot.receipt) {
    const item = element("li"), link = element("a", "Open receipt reference — unverified");
    link.href = snapshot.receipt; link.target = "_blank"; link.rel = "noopener noreferrer";
    item.append(link); events.append(item);
  }
  timeline.append(events);

  const ledger = card("Intents ledger · exact token amounts"), table = element("table"), head = element("thead"), headers = element("tr");
  ["Asset", "Amount", "Base units", "Decimals"].forEach((value) => headers.append(element("th", value))); head.append(headers);
  const body = element("tbody");
  snapshot.balances.forEach((balance) => {
    const tr = element("tr");
    [balance.asset_id, formatUnits(balance.units, balance.decimals), balance.units, balance.decimals].forEach((value) => tr.append(element("td", value)));
    body.append(tr);
  });
  table.append(head, body); const scroll = element("div", undefined, "audit-scroll"); scroll.append(table); ledger.append(scroll);
  ledger.append(element("p", "Only listed assets are read. These are Intents ledger balances, not native FT balances or current USD NAV.", "audit-note"));
  const identity = card("Deployment observation"), identityList = element("dl");
  row(identityList, "Vault code hash", snapshot.codeHash);
  row(identityList, "Signer", snapshot.externals.signer); row(identityList, "Intents", snapshot.externals.intents); row(identityList, "Oracle", snapshot.externals.oracle);
  row(identityList, "Withdrawal protocol", snapshot.withdrawalProtocol);
  if (snapshot.withdrawalJournalBinding) {
    row(identityList, "Journal executor", snapshot.withdrawalJournalBinding.executor_id);
    row(identityList, "Ledger domain", snapshot.withdrawalJournalBinding.ledger_domain);
    row(identityList, "Request hash scheme", snapshot.withdrawalJournalBinding.request_hash_scheme);
  } else row(identityList, "Receipt sinks · existence/authority unverified", snapshot.recoveryAccounts.join(", "));
  row(identityList, "RPC", snapshot.rpc); identity.append(identityList);
  identity.append(element("p", snapshot.withdrawalJournalBinding
    ? "This binding is reported by the vault. Executor admission, authoritative journal outcomes, completed refunds, account permissions, external code/admin roles, source/audit correspondence and live withdrawal results are UNVERIFIED. A matching supplied vault hash alone is not custody readiness."
    : "Account permissions, permanently inert receipt sinks, external code/admin roles, source/audit correspondence and oracle freshness/correctness are UNVERIFIED. A matching supplied vault hash alone is not custody readiness.", "audit-note"));
  grid.append(controls, rules, accounting, queues, timeline, ledger, identity);
  container.replaceChildren(anchor, grid);
}

export function mountLiveDashboard(documentRef = globalThis.document) {
  const form = documentRef.getElementById("audit-form");
  if (!form) return;
  const output = documentRef.getElementById("audit-output"), status = documentRef.getElementById("audit-status");
  let retried = false;
  const loader = createSnapshotLoader(readAuditSnapshot, {
    loading() { output.replaceChildren(); status.textContent = "Reading one final-block snapshot…"; status.className = "audit-status"; },
    success(snapshot) {
      renderSnapshot(output, snapshot, documentRef);
      status.textContent = `Snapshot observed ${new Date().toISOString().replace("T", " ").replace(/\.\d+Z$/, " UTC")}. Refresh to read newer state.`;
      status.className = "audit-status observed";
    },
    error(error) {
      // One automatic retry for a slow RPC: the first read after page load can time out while the endpoint warms up.
      if (!retried && /timed out/i.test(error.message)) {
        retried = true;
        output.replaceChildren();
        status.textContent = "The RPC was slow to answer. Trying once more…";
        status.className = "audit-status";
        setTimeout(() => run(), 800);
        return;
      }
      output.replaceChildren(); status.textContent = error.message; status.className = "audit-status error";
    },
  });
  const field = (id) => documentRef.getElementById(id);
  const network = field("audit-network"), rpc = field("audit-rpc");
  network.addEventListener("change", () => { rpc.value = NETWORKS[network.value].rpc; });
  form.addEventListener("input", () => { loader.cancel(); output.replaceChildren(); status.textContent = "Inputs changed. Read the account again for a new snapshot."; });
  const run = () => loader.run({ network: network.value, rpc: rpc.value.trim(), account: field("audit-account").value.trim(),
    expectedCodeHash: field("audit-code").value.trim(), receiptUrl: field("audit-receipt").value.trim() });
  form.addEventListener("submit", (e) => {
    e.preventDefault();
    retried = false;
    run();
  });
  const setMode = (mode) => {
    const sample = mode === "sample";
    field("sample-view").hidden = !sample; field("live-view").hidden = sample;
    documentRef.querySelectorAll("[data-audit-mode]").forEach((button) => button.setAttribute("aria-pressed", String(button.dataset.auditMode === mode)));
  };
  documentRef.querySelectorAll("[data-audit-mode]").forEach((button) => button.addEventListener("click", () => setMode(button.dataset.auditMode)));
  documentRef.querySelectorAll('a[href="#about"], a[href="#faq"]').forEach((link) => link.addEventListener("click", () => setMode("sample")));
}

if (typeof document !== "undefined") mountLiveDashboard(document);
