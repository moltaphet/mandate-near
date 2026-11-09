# F26: durable withdrawal journal at the custody executor

Status: source implementation and review specification, 8 November 2026.
The shared protocol crate, native custody-executor overlay, fresh vault adapter
and keeper recovery path are implemented locally. The final source-v4
source/lock/artifact binding and scoped native checks are recorded in the
[checkpoint](../audit/F26-DURABLE-2026-11-08.md). Actual-WASM asynchronous coverage,
intended-chain evidence and independent review remain separate gates. No deployed journal or universal recovery is claimed.

Existing vault deployments use `unique_voucher_v1` with the pinned external Intents
WASM. That binary has no per-withdrawal terminal-journal query among its
exports. The preserved upstream inspection references commit
`32cd1f9788a347908164fbcc56525dc6542f158f`; its exact correspondence to the
pinned/deployed WASM remains unverified. The supplied source archive is available
and pinned at SHA256 `0c9073d58953d06850b408800047a0f061194e925c9ea1c2259e3910f18c24d0`
(1,324,342 bytes). The [preparation package](../../integrations/intents-executor/README.md)
verifies and stages the exact upstream, journal/shared sources and allocator
manifest change independently of Cargo or deployment. See the
[fixture provenance](../../contracts/vault/tests/fixtures/README.md) and
[existing recovery research](../evidence/recovery-design.txt).

`new_with_journal(owner, mandate, journal)` explicitly selects the new protocol
for a fresh vault; `journal` binds signer, Intents, oracle, oracle kind and ledger
domain. Existing initializers retain voucher behavior. The executor is separately
initialized with `new_with_withdrawal_journal(config, ledger_domain, authorized_vaults)`.
Neither mode selection nor a capability response authenticates deployed code.

## 1. Custody boundary and initial scope

The journal must live in the contract that owns the relevant ledger, commits
the withdrawal debit and executes the terminal withdrawal resolver. A proxy
that only records a call to the existing Intents `ft_withdraw` remains dependent
on the result reaching that proxy. It cannot reconstruct a missing terminal
result from its own `Pending` record.

Initial scope is an ordinary NEP141 `ft_transfer` withdrawal, with a positive
amount and an exact token and receiver. There is no arbitrary `msg`, caller
supplied memo, `ft_transfer_call`, multi-asset batch, NFT or native-token path
in this version. Token and receiver validation must preserve the vault's
existing asset and unsafe-receiver restrictions. The executor must reject zero
amounts. Extending the asset or transfer semantics requires another review and
an explicitly versioned schema.

The authenticated predecessor is the vault whose ledger is debited. A caller
cannot select another vault by putting its account in JSON. Keepers invoke the
vault; only the vault invokes executor mutations for its own operations.

## 2. Implemented source ABI and immutable binding

The following methods are provided by the source overlay; they are absent from
the preserved legacy fixture:

```text
withdraw_with_id(request: JournalWithdrawalRequest)
    -> PromiseOrValue<JournalWithdrawalRecord>
cancel_withdrawal_with_id(request: JournalWithdrawalRequest)
    -> JournalWithdrawalRecord
get_journal_withdrawal_status(vault_id: AccountId, withdrawal_id: U64)
    -> Option<JournalWithdrawalRecord>
get_withdrawal_journal_protocol() -> Option<JournalProtocolDescriptor>
get_journal_authorized_vaults() -> Option<Vec<AccountId>>
```

Both mutations require the authenticated vault predecessor and exactly one
yoctoNEAR. Query methods are public, read-only and bounded to one operation.
The first withdrawal may return its terminal resolver result; duplicate calls
return the existing record without creating another transfer promise.

`JournalWithdrawalRequest` contains exactly:

| Field | Meaning |
| --- | --- |
| `withdrawal_id: U64` | A never-reused attempt ID in this vault's executor ledger domain |
| `request_seq: U64` | The owner-authorization generation captured by the vault |
| `token: AccountId` | Exact NEP141 token contract |
| `receiver_id: AccountId` | Exact external receiver |
| `amount: U128` | Positive amount in token base units |

`JournalWithdrawalRecord` binds all of the following, in addition to its state:

```text
protocol_version, ledger_domain, executor_id, vault_id,
withdrawal_id, request_seq, token, receiver_id, amount, request_hash
```

The operation key is `(authenticated vault_id, withdrawal_id)` within an
immutable executor `ledger_domain`. The domain and protocol version must be
preserved through compatible upgrades. An upgrade cannot create a fresh domain
to make an old key reusable while old receipts or authorizations remain valid.

The protocol version is `durable_withdrawal_journal_v1`.
`JournalProtocolDescriptor` identifies that version, the executor account,
immutable 32-byte ledger domain and request-hash scheme. Its values must agree
with the independently reviewed deployment manifest. A descriptor or method's
presence alone is not evidence that the implementation obeys this spec.

## 3. Canonical request commitment

Executor and vault must consume the same pinned, reviewed shared Rust crate
for request types, validation and commitment generation. The shared implementation
is `crates/withdrawal-journal`, with independently generated golden vectors and
strict request/status schemas. Executor preparation pins its exact source files;
both sides consume the same encoding implementation.

The v1 commitment is:

```text
SHA256(
  UTF8("mandate-withdrawal-journal-request-v1") || 0x00 ||
  Borsh((
    protocol_version: String,
    ledger_domain: [u8; 32],
    executor_id: AccountId,
    authenticated_vault_id: AccountId,
    withdrawal_id: u64,
    request_seq: u64,
    token: AccountId,
    receiver_id: AccountId,
    amount: u128
  ))
)
```

The tuple order above is fixed. Borsh strings use their length-prefixed UTF-8
encoding, account IDs use the shared crate's validated string encoding, the
32-byte array has no variable-length prefix, and integer fields use the stated
fixed-width little-endian encoding. The shared crate must freeze those choices
and pin the serialization dependency; changing them changes the hash scheme.

JSON member order, whitespace and the textual spelling of a number are not
commitment inputs. On the JSON ABI, U64/U128 values are decimal strings and
32-byte digests/domains are validated lowercase 64-hex strings. The executor
computes the commitment from authenticated context and validated typed fields;
it does not accept a caller's hash as a replacement for those fields.

## 4. Journal states and atomic dispatch

The initial state set is deliberately limited:

| State | Required meaning |
| --- | --- |
| `Pending` | The bound request was accepted, its exact amount debited and its one transfer scheduled; terminal outcome is unresolved |
| `Delivered { delivered_amount: U128 }` | The reviewed ordinary-transfer resolver established full delivery; amount must equal the request |
| `Refunded { refunded_amount: U128 }` | The resolver established full failure/refund and atomically restored the exact request amount to the same vault ledger |
| `Cancelled` | A bound tombstone prevents this operation from ever reaching debit or transfer dispatch |

A missing record is not a state and is not proof that an original call cannot
still arrive. Partial, malformed, contradictory or unsupported outcomes remain
unresolved; they must not be exposed as `Delivered`, `Refunded` or `Cancelled`.

For a previously absent valid key, acceptance must commit these effects in one
executor receipt: the immutable request binding, exact ledger debit, `Pending`
record and scheduling of one transfer with its identified resolver. A failure
of validation, funding, storage or promise construction must roll back all of
those effects. There must be no committed debit without its record and no
scheduled transfer without its committed debit and binding.

Before any mutation schedules work, it must look up the key. An existing key
with any different bound field or commitment is refused. A matching existing
key returns its current record and never schedules another transfer, including
while `Pending` and after a terminal state. Concurrent original and duplicate
receipts must serialize through this lookup and acceptance transaction.

A synchronous refusal that leaves no record does not permit the vault to infer
safe absence. If an earlier original receipt could still arrive, recovery must
first establish the irrevocable cancellation below. A rejected zero/invalid
request is not an accepted journal operation.

## 5. Terminal resolver and refunds

The resolver must be private to the executor and bound to the one accepted
operation and its exact transfer promise. It must use the reviewed NEP141
transfer-result semantics, not receiver balance changes, nonce consumption,
relay reports or caller-supplied JSON.

The only normal transitions from `Pending` are full `Delivered` or full
`Refunded`. For a refund, restoration to the original vault ledger and the
terminal journal update must occur atomically in the same resolver receipt.
The journal must never advertise `Refunded` before the corresponding credit
commits, or credit again after a terminal record already exists.

Duplicate or late resolvers for a matching terminal operation are no-ops.
Mismatched IDs, bindings or promise-result counts must not mutate another
operation. A malformed or partial result retains `Pending`. If the executor's
resolver fails, runs out of gas or does not complete, the operation stays
unresolved even if the token transfer itself completed.

The vault callback is only a notification. If it is lost after the executor
commits a terminal journal entry, a later authenticated query can obtain that
entry. If the executor never commits a terminal result, this query cannot
create one.

## 6. Irrevocable pre-dispatch cancellation

Cancellation of an absent key must atomically store the complete bound request
as `Cancelled`, without a debit or transfer. A delayed original with that key
then returns the tombstone and cannot schedule work. Cancellation of an already
matching `Cancelled` key is idempotent; a conflicting binding is refused.

If the original acceptance has already committed `Pending`, cancellation must
return that state and leave it unresolved. No timeout, owner override or
replacement request may turn a dispatched operation into `Cancelled`. Matching
terminal records also remain unchanged. The race is decided by which executor
receipt commits first; elapsed wall time is not its evidence.

The vault may release a guard from exact `Cancelled` proof and preserve a still
authorized request as unarmed. A later owner rearm preserves the required
maturity and allocates a new attempt ID. Cancellation never resurrects an owner
request that was cancelled or replaced under another `request_seq`.

## 7. Strict reconciliation and trust

The vault must query its configured executor through an authenticated
cross-contract promise. Its private response handler must match the protocol
version, domain, executor, authenticated vault, attempt ID, request sequence,
token, receiver, amount and canonical hash against the immutable local active
operation before changing any state. Bounded schema validation must reject
unknown versions, malformed fields and terminal amounts unequal to the request.

`Pending`, `null`, a failed query or any inconsistent tuple retains the guard.
Initial debit, a sent voucher or a lower scalar balance is not terminal proof
in this mode. Terminal accounting for the captured operation must remain
possible after policy tightening, while replacement/cancellation generations
must never be requeued or rearmed by an old response.

The executor account, code/source correspondence, upgrade and ledger-admin
authorities, token behavior and deployment domain are explicit trust inputs.
An account reporting this protocol is insufficient; the exact code and authority
paths must be independently reviewed. Off-chain RPC observations, keeper
opinions and capability self-reports are not on-chain outcome proofs.

## 8. Missing resolver: only an audited final-receipt proof path

If `Pending` persists because the executor resolver is missing, the only proposed
escape is an independently audited on-chain verifier of finalized receipt and
execution-outcome evidence. This path is not presently implemented.

The verifier must authenticate finality and inclusion; identify the executor's
accepted operation and the exact generated token-transfer receipt; prove its
token, predecessor, receiver, amount and relationship to that operation; and
authenticate the terminal outcome under the reviewed transfer semantics.
An outcome hash alone or an unverified action receipt is insufficient. The
proof format must explain how that linkage is proved with NEAR's actual
receipt/outcome commitments before an implementation is accepted.

Applying valid proof must atomically finalize only the matching still-`Pending`
entry, including any required ledger refund. A later resolver must then be a
no-op, preventing duplicate credit or conflicting terminal states. Proof replay
for another operation, domain, executor or chain must fail. Without that reviewed
verifier and exact proof, the guard remains; no manual receipt assertion,
balance-only heuristic or timer is an alternative.

## 9. Storage, tombstones and gas funding

Journal entries and cancellation tombstones need funded storage. The fresh-only
executor initializer admits 1–20 distinct vaults immutably; other predecessors
cannot allocate entries or tombstones. Each request,
response and lookup must be bounded, but total retained history grows with
accepted owner-authorized attempts. The funding source and reservation rules
must be explicit and measured. One yoctoNEAR is authentication, not sufficient
storage funding. The implementation reserves a padded terminal slot and separate
refund storage before debit. Insufficient funding must stop before acceptance/dispatch;
failed cancellation storage funding must retain the vault guard.

IDs cannot wrap, reset or be reused. Deleting a terminal entry or cancellation
tombstone would permit a delayed duplicate to appear new. No garbage collection
is allowed in v1. Any future compaction must retain an audited permanent rejection
of retired keys and explain how exact historical status remains available;
presence of a counter alone does not prove a request's terminal tuple.

Gas must cover acceptance, token transfer, executor resolver, journal/refund
writes, vault notification and subsequent status-query reconciliation at the
supported maximum field sizes. The source requires 100 TGas for executor
withdrawal and 30 TGas for cancellation, assigning 30 TGas to the ordinary
token transfer and 40 TGas to its resolver. Vault apply/recovery use 280/140 TGas.
These configured budgets and storage reserves still require actual-WASM
boundary measurements for the final artifact and selected intended token.

## 10. Keeper and current voucher compatibility

A keeper persists the observed attempt ID, invokes an
ID-bound vault recovery method and stop or reload on an active-ID change. A
stale keeper cannot act on an unrelated replacement by calling a method that
implicitly selects whichever withdrawal is current. Recovery polling is bounded
with backoff; repeated queries/cancellation attempts cannot create new IDs,
debits, transfer dispatches or owner authorizations.

Only explicit owner authorization may arm a fresh attempt. Unavailable,
malformed or `Pending` status causes continued guard retention and operational
escalation, not a blind resend. Keeper storage or relay status is not executor
journal evidence. Before any paid broadcast, the keeper durably records the
exact public transaction hash with single-writer exclusion. An ambiguous outcome
retains the restart fence and requires operator reconciliation; an unknown or
not-found transaction is not evidence that it is safe to pay again. The journal
contains no private or signed transaction bytes.

Existing `unique_voucher_v1` deployments remain explicitly that protocol.
Their preparation, sent/cancelled voucher proof and `DebitObserved` behavior
must not be relabeled terminal journal evidence. A journal integration requires
an explicit reviewed protocol selection; descriptor detection cannot silently
switch modes or fall back to an unjournaled withdrawal. The pinned fixture and
its existing regression targets must be preserved separately from any new
source-owned executor prototype.

## 11. Migration and old unknown operations

Migration must preserve monotonic IDs and owner-request generations, and retain
the current paused/quiescent migration restrictions. Unknown pre-journal sends
must be refused, not imported as `Cancelled`, `Refunded` or new `Pending`
records. Neither an empty local active record nor a current balance proves the
absence of old external transfer/resolver/refund receipts.

An old operation can only be resolved by authenticated existing-protocol proof
that actually establishes its required outcome, or by a separately reviewed
final-receipt proof path. A journal added later cannot retroactively prove what
an old unjournaled executor did. If external history cannot establish safe
quiescence, use a separately reviewed fresh deployment and custody transition;
do not use migration to bypass the unresolved-send guard.

## 12. Implementation and release gates

1. Obtain and pin the executor source, dependencies and build inputs; establish
   the source-to-WASM mapping and exact administrative authority boundary.
2. Implement the shared ABI/commitment crate and executor journal in the actual
   custody/debit/resolver path. A stand-in or proxy is not evidence for the
   existing production Intents implementation.
3. Test same-ID concurrent duplicates, conflicting tuples, cancellation races,
   zero amounts, storage/gas refusal, atomic debit/dispatch, full delivery/refund,
   duplicate resolvers, lost vault notifications, lost executor resolvers,
   malformed/partial outcomes and stale keeper/callback IDs. Compare ledger
   changes with journal states and verify no duplicate debit, transfer or refund.
4. Test migration refusal for unknown historical sends and preserve all legacy
   voucher fixtures and tests. Measure maximum-case storage and gas for the
   exact new artifact; complete CI and independent source review.
5. If a final-receipt escape is included, separately audit its NEAR finality,
   inclusion, action/outcome binding and replay handling. Otherwise document
   indefinite `Pending` as the unresolved case.
6. Independently audit the exact executor, vault adapter, shared crate, upgrade
   rules and external token/ledger assumptions before any real-fund release.
7. Rehearse the selected intended-chain deployment, genuinely elapsed owner
   withdrawal, terminal receiver/ledger outcomes and failure recovery with exact
   source/WASM evidence and separately authorized operations.

The pre-journal hardening results establish only that earlier candidate's
recorded scope. They do not close these new protocol gates. Mainnet remains
**NO-GO**; this spec adds no deployment or transaction authorization.
