# Mandate — three-minute demo draft

Draft only, 7 November 2026. No demo results, live provider run or Phase4 sandbox
execution are claimed below. Rehearse after the final source and evidence freeze.

## Preparation

Show the exact commit, artifact hash and environment before starting. Use the
existing cream site and dashboard. Label every screen **local sandbox**,
**approved testnet** or **historical example**. Keep credentials and authentication
settings out of the recording. Replace planned results below with actual output;
leave any incomplete segment explicitly marked **NOT RUN**.

Prepare a captured public model record, the response-derived trade tuple, and
the canonical commitment read from the real proposal. Show unsigned captures as
**UNVERIFIED**. A public synthetic signature fixture must remain labelled
**synthetic**, including when it verifies against its public pin.

## Timed walkthrough

| Time | Screen and action | Narration / evidence requirement |
|---|---|---|
| 00:00–00:25 | Vault, owner and ledger | “The agent chooses a trade; the vault authorizes it through on-chain rules before asking Chain Signatures to sign.” Show the environment and selected DAO owner, with operational status. |
| 00:25–00:50 | Current mandate and controls | Show asset list, trade/daily/position/slippage limits, price timestamps, pause and declared-source list. Explain that declared IDs do not authenticate model origin. Enable the source-list segment only after final Phase3 verification. |
| 00:50–01:30 | Planned hostile decision test | Display a captured $60 trade against a $50 cap and hostile rationale. Deliberately bypass local preview in the adversarial sandbox test. Show exact `TradeTooLarge`, no signed intent, released execution lock, one strike and unchanged balances. **Phase4: NOT RUN.** |
| 01:30–02:05 | Planned valid proposal and response record | Show exact input/output IDs and amounts, raw request/response hashes, canonical version and matching on-chain `reason_hash`. Run the offline verifier on this record. State its actual result, keeping unsigned output **UNVERIFIED**. **Phase4: NOT RUN.** |
| 02:05–02:30 | Planned residual-risk trade | Show a large bad decision that fits all configured limits and is allowed. “The vault does not determine whether a strategy is profitable.” A signature is not settlement. **Phase4: NOT RUN.** |
| 02:30–03:00 | Evidence and release boundary | Show complete final command output when available. State what live signing, matched fill, elapsed withdrawal and recovery actually ran. Explain F26 guard retention and the unverified DAO/Pyth operational requirements. |

An unknown declared model ID must be refused before proposal state changes. A
new allowed ID waits the existing 24-hour loosening delay; removal tightens the
policy immediately. Present these as tested outcomes only after the final
allowlist and migration tests pass. An agent can still lie about its declared ID.

## Offline provenance segment

After a public record and proposal hash exist, the operator can run:

```sh
cargo run -p mandate-agent -- --verify-model PUBLIC_RECORD.json --reason-hash OBSERVED_REASON_HASH
```

For a stored or separately supplied signature, add an independently trusted
public `--signing-address` and, where needed, `--signature PUBLIC_PROOF.json`.
Use the CLI's actual `UNVERIFIED` or `VERIFIED_PINNED_SIGNER` result. These checks
do not authenticate TEE identity, the chain snapshot, proposal inclusion, gate
approval or settlement. Retrieve the proposal hash separately from the approved
environment and show the exact asset/amount tuple alongside it.

## Current limits to state aloud

The latest default fast baseline stopped with exit101 because protocol tests
could not bind loopback sockets. Formatting and strict Clippy passed before that
failure; later provenance targets were not reached by that script. Separate
focused passes are not a complete combined pass. Final integration and gas
measurement remain required.

The historical `mandate-r1-4715a0.testnet` account demonstrated an MPC spike,
not a fresh current custody deployment. Pyth feed/updater readiness and separate
DAO governance still need evidence. F26 can retain a withdrawal guard after an
already-dispatched transfer; no owner override exists. No mainnet readiness,
profitability or universal prompt-injection immunity is claimed.
