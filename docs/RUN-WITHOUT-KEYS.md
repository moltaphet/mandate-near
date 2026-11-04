# Run everything without keys

No step below needs a NEAR account, a private key or a funded wallet. The
integration tests start their own local NEAR sandbox, and the sandbox has its
own genesis account. Nothing is sent to a public network.

## What you need

| Tool | Why | Install |
|---|---|---|
| Rust via rustup | builds the workspace; `rust-toolchain.toml` pins 1.95.0 and installs it | <https://rustup.rs> |
| `cargo-near` 0.22 | the integration tests build the contract wasm with it | `cargo install cargo-near --locked` |
| Internet, first run only | the sandbox binary (`near-sandbox`) and crates are downloaded once | — |

The first integration run is slow: it compiles the contract several times and
downloads the sandbox. Later runs are faster.

## Steps

1. Clone the repo and enter it:

   ```bash
   git clone https://github.com/moltaphet/mandate-near
   cd mandate-near
   ```

2. Run the offline checks: formatting, clippy with warnings as errors, and
   unit and property tests. No network is needed once crates are cached.

   ```bash
   ./scripts/verify.sh
   ```

3. Run the integration tests on the local sandbox. This includes the eight attacks.

   ```bash
   ./scripts/verify.sh --integration
   ```

4. Record the attack output. The file is written to `docs/evidence/attacks.txt`.

   ```bash
   ./scripts/attack-demo.sh
   ```

5. Record gas per call. The file is written to `docs/evidence/gas.txt`.

   ```bash
   ./scripts/gas-report.sh
   ```

6. Check that every test and path cited in the docs still exists.

   ```bash
   ./scripts/check-claims.sh
   ```

## What this does not cover

- **The mainnet path.** Tests use a stand-in MPC signer and the real
  `intents.near` code. A live MPC call needs a funded account, and that is
  deliberately not part of this guide.
- **The agent and keeper binaries against a live chain.** They read keys from
  environment variables. The sandbox tests call the same library code without
  keys. See [TESTNET.md](TESTNET.md) for the testnet run and what it did not reach.
- **Dashboard and video.** Not built yet.

## If a step fails

- `cargo near: command not found`: install `cargo-near` (step above).
- The sandbox does not start: check the first-run download finished. Delete
  the partially downloaded sandbox directory under the `near-sandbox` cache and rerun.
- A count or timing in the docs differs from your run: the docs state what
  was measured on a given day. Trust your run and check the date in
  `docs/evidence/`.
