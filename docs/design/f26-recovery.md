# F26: a stuck withdrawal guard

Status: historical design exploration, 7 November 2026; reconciled 8 November.
The options below were not approved or implemented. The current implementation
retains an unknown-result guard until authenticated proof resolves it; elapsed
time, balances and owner authority cannot override it. That availability residual
is accepted only for the current prototype and testnet scope. A separate design
and independent review are required before changing this funds-critical protocol.

## The problem

A withdrawal sends funds out through the Intents `ft_withdraw` call. The vault keeps a
guard while the result is unknown. If the final callback is lost or malformed, or its
proof is missing or inconsistent, the guard stays. While it stays, the vault refuses new
withdrawal dispatches and new trade authorizations. Elapsed time, a larger balance and
owner authority do not clear it. The original withdrawal may already have moved funds,
so clearing the guard blindly could pay twice.

This is a liveness problem with a safety reason: availability is traded for never
resending an unknown transfer. See [SECURITY.md](../../SECURITY.md) (F26) and the
withdrawal section of the [README](../../README.md).

The 7 November testnet run recorded a queued withdrawal with a 24-hour maturity
and an early-apply refusal. Delivery after the actual delay and lost-callback
recovery were not exercised. A stuck guard can hold the vault indefinitely.

## What any fix must keep

1. No double payment. An unknown transfer is never resent automatically.
2. No instant owner override. The owner is a DAO, but a compromised owner must not be
   able to clear the guard and redirect funds at once.
3. The 24-hour delay on withdrawals stays.
4. A fix must not weaken the proof path. When authenticated proof exists, it still wins.
5. Every state change is covered by tests, and any new stored field comes with a Borsh
   migration and a golden fixture.

## Options

### A. Owner-initiated abandon after an extra delay (historical, unapproved)

The owner (the DAO) may call an `abandon_withdrawal_guard` method after a long extra
delay, for example 7 days from the moment the guard was set. The call states that the
funds may already have moved. It clears the guard, records an `Abandoned` outcome for that
withdrawal, and **consumes the authorization**: nothing is re-queued automatically.

- Proposed safety argument, not an established guarantee: the owner would check the
  receiver out of band and queue a new request deliberately, waiting another 24 hours.
  An unresolved original transfer means this argument needs independent review.
- Risk: if the original transfer completed late, a later refund could credit the vault
  twice in the ledger view. The abandon record makes this visible.
- Cost: one new state field and one method. A new Borsh layout and a migration golden.
- Attack surface: a compromised DAO can clear a guard after the extra delay. That power
  is bounded by the delay and by the ceiling in the section below.

### B. A second authenticated proof path

Allow the recovery proof to be read again later, or from another Intents view, when the
first read was missing or inconsistent. This only helps when the reason is a transient
read failure. It does nothing when the Intents state itself is inconsistent.

- Safety: strongest, because it never clears on an assumption.
- Cost: moderate. It adds retry and proof-source logic to the most complex module.
- Limit: it does not fix a guard that no proof can resolve.

### C. Limit what a stuck guard blocks

Keep the guard, but let trades in assets other than the withdrawn one continue.

- Safety: balance interplay between assets makes this risky. A trade that changes the
  vault's holdings can change whether the stuck withdrawal's balance check still means
  anything.
- Cost: high, and it touches the execution gates, which is the part auditors trust most.

## Current disposition

The original recommendation of A was an unapproved proposal. It is not the current
policy: no abandon method or delayed owner bypass exists. Unknown dispatched
withdrawals retain their guard until authenticated proof resolves the outcome.
Per-request ceilings do not make an assumption-based guard release safe. Any new
resolution design must be reviewed independently and reopen the audit scope.

The subsequent fresh-only [source-owned journal candidate](f26-durable-executor.md)
uses permanent executor records and cancellation tombstones, with terminal
reconciliation in the real custody/resolver path. It implements neither option
A nor a balance/time reset. A lost vault notification can be queried after a
terminal executor record commits; a missing executor resolver can still leave
Pending indefinitely. Final asynchronous verification and independent review
remain open, and unknown legacy sends gain no retroactive journal truth.

## Separate item: a withdrawal ceiling

The current hardening source implements optional per-asset `withdrawal_limits`
with positive token-base-unit ceilings per request. Omitted assets and migrated
old policies are uncapped. A per-day or aggregate ceiling remains a separate
proposal; it is not implemented. These ceilings do not resolve or abandon an
unknown-result guard: F26 remains unchanged.

- The implemented mandate field is a per-request ceiling per configured asset.
- A limit may be tightened at once and loosened only through the existing 24-hour delay,
  the same rule as every other mandate field.
- It is checked when a request is queued, when it is applied, and during preparation
  before dispatch. Increasing/removing a ceiling waits 24 hours, including asset removal.

## Historical test outline for option A

This outline belongs to the unapproved proposal above; it does not describe tests
or guarantees of the current implementation.

- Unit: the guard set, the extra delay not reached (refused), reached (cleared), abandon
  consumes the authorization, and no automatic re-queue.
- Sandbox: a dispatched withdrawal with a lost final callback, an abandon after the delay,
  and a later deliberate request.
- Migration: the new layout against a golden fixture, and refusal of every unsafe old state.
- Gas: the added check stays inside the prepaid gas for the withdrawal callbacks.

## Impact on audit

The implemented ceiling changes funds-critical code and belongs in the new audit
scope. Option A remains historical and unapproved. Any future F26 protocol change
requires a separate design, regression evidence and review; hand auditors both the
current guard rules and any proposed replacement.
