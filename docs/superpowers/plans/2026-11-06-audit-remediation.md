# Mandate audit remediation — 2026-11-06

Execution authorized by the owner in the current fix branch. Preserve existing edits and excluded private files. No deployment, commit, or push is part of this task.

1. Bind executions to immutable asset ordering, applied configuration revision and proposal identity. Reject stale callbacks, expire unsigned approvals and handle malformed salt safely.
2. Track withdrawals before any external call, reserve callback gas, parse results within callbacks and retain an identified recovery state. Never resend an unresolved transfer on elapsed time alone.
3. Bound supported asset identifiers to valid NEP141 accounts; add safe migration for the two known legacy layouts.
4. Fix aggregate price timestamp coherence, conservative client outcome classification and keeper readiness/backoff.
5. Add adversarial regression tests and funded Intents settlement evidence; align documentation, web claims, and verification scripts.
6. Run format, Clippy, unit, sandbox and gas checks. Produce an English text report separating verified fixes from external deployment and provider assumptions.

Withdrawal recovery must distinguish pre-send cancellation from post-send uncertainty. A post-send ledger debit can release the trading guard without claiming final receiver delivery. Unknown or deposit-masked outcomes remain fail closed and must never become an automatic duplicate withdrawal.
