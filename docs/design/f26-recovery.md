# F26: a stuck withdrawal guard

Status: design only, 7 November 2026. No contract code changes with this document.
Implementation needs the owner's written approval of one option, then review.

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

The 7 November testnet run showed the practical side. A withdrawal request is refused
while a signed intent is open, and a queued withdrawal holds for 24 hours. A stuck guard
would hold the vault much longer than that.

## What any fix must keep

1. No double payment. An unknown transfer is never resent automatically.
2. No instant owner override. The owner is a DAO, but a compromised owner must not be
   able to clear the guard and redirect funds at once.
3. The 24-hour delay on withdrawals stays.
4. A fix must not weaken the proof path. When authenticated proof exists, it still wins.
5. Every state change is covered by tests, and any new stored field comes with a Borsh
   migration and a golden fixture.

## Options

### A. Owner-initiated abandon after an extra delay (recommended)

The owner (the DAO) may call an `abandon_withdrawal_guard` method after a long extra
delay, for example 7 days from the moment the guard was set. The call states that the
funds may already have moved. It clears the guard, records an `Abandoned` outcome for that
withdrawal, and **consumes the authorization**: nothing is re-queued automatically.

- Safety: no double payment from the vault side. The owner must check the receiver out of
  band and queue a new request deliberately, which waits its own 24 hours.
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

## Recommendation

Choose **A**, with **B** as a later improvement. Pair it with the withdrawal ceiling below.
Do not change the contract before the testnet run is finished and the audit scope is
agreed, because every change here reopens the audit.

## Separate item: a withdrawal ceiling

The vault has no maximum withdrawal cap. An owner-set limit would shrink the damage of a
stolen owner key and of an abandoned guard.

- Fields in the mandate: a per-request ceiling and a per-day ceiling, per asset.
- A limit may be tightened at once and loosened only through the existing 24-hour delay,
  the same rule as every other mandate field.
- It is checked when a request is queued and again when it is applied.

## Tests a fix needs

- Unit: the guard set, the extra delay not reached (refused), reached (cleared), abandon
  consumes the authorization, and no automatic re-queue.
- Sandbox: a dispatched withdrawal with a lost final callback, an abandon after the delay,
  and a later deliberate request.
- Migration: the new layout against a golden fixture, and refusal of every unsafe old state.
- Gas: the added check stays inside the prepaid gas for the withdrawal callbacks.

## Impact on audit

Option A and the ceiling both change funds-critical code. Do them before the external audit
starts, or schedule a second review. Either way, hand the auditors this design with the code.
