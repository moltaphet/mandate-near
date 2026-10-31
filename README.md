# Mandate

An autonomous trading agent that can never break its rules: the strategy runs
sealed in a TEE, and a NEAR smart contract is the only key that can move the
treasury, so every trade is checked on-chain against an owner-defined mandate.

> **Status: work in progress** for NEAR HACKATHON (2026-11-01 → 2026-11-14).
> This README lists only what exists in the repository today.

## Problem

Handing money to an AI agent today means trusting the agent, its operator,
and whoever can reach its keys. A prompt injection, a bad model output or a
leaked key can drain the treasury. Spending limits enforced off-chain can be
bypassed by whoever runs that code.

## Approach

- Funds sit on NEAR Intents under the vault contract's own account.
- The only signing key for that account is derived via NEAR Chain
  Signatures and controlled by the vault contract.
- The contract signs a swap only after checking it against the mandate:
  allowed assets, trade size, position weight, slippage against an oracle,
  turnover, cooldown and a drawdown circuit breaker.
- The agent, keeper and UI can stop trading but cannot move funds outside the
  mandate.

Full design, trust model and risk register: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Try it

Requirements: [rustup](https://rustup.rs). The pinned toolchain (Rust 1.95.0)
installs automatically from `rust-toolchain.toml`.

1. Run `./scripts/verify.sh`. You should see formatting, clippy and all unit
   tests pass, ending with `All checks passed.`
2. Run `cargo test -p mandate-vault mandate::`. You should see the mandate
   validation rules exercised one by one (invalid asset ids, duplicate assets,
   slippage cap, price-age bounds, and so on).

## What exists today

| Area | State |
|---|---|
| `contracts/vault` | Contract state, mandate config with validation, owner controls (`set_mandate` needs 1 yocto; `pause` works with a function-call key; `unpause` needs 1 yocto). Unit-tested. |
| Trading flow (OutLayer decision → mandate check → MPC sign → Intents) | Not implemented yet |
| Agent, keeper, dashboard | Not implemented yet |

## Repository layout

```
contracts/vault/   NEAR contract (Rust, near-sdk 5.30)
docs/              architecture and risk register
scripts/verify.sh  one-command check
```

## Hackathon disclosure

- All code in this repository was written during the NEAR HACKATHON window,
  starting 2026-10-31. There is no pre-hackathon baseline.
- Third-party libraries: `near-sdk` (MIT/Apache-2.0).
- Development is AI-assisted (Claude Code).

## License

[MIT](LICENSE)
