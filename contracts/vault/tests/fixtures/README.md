# Test fixtures

## `intents-near-mainnet.wasm`

The supplied verifier fixture was recorded by the author as an unmodified
`view_code` download from `intents.near`. Sandbox tests exercise this binary,
alongside stand-in MPC/oracle contracts. Its size and SHA-256 are checked;
correspondence to the current deployment, exact upstream source and audit
scope remains UNVERIFIED until independently compared.

| Field | Value |
|---|---|
| Author-recorded source account | `intents.near` (mainnet) |
| Fetched at block | 222833256 (2026-11-01) |
| On-chain code hash | `BtA1BEFNS619KkvXFsgcpQ21Tb5yMm32aaUkn7xNothk` |
| SHA-256 | `a1afab98916d9f213746544dcf8e4e6bdb22c0be6bc5dfbd8ea8814c9922b547` |
| Size | 1,297,551 bytes |
| Upstream | https://github.com/near/intents (MIT) |

Download separately for comparison (read-only, no credentials):

```bash
scripts/fetch-intents-fixture.sh /tmp/intents.wasm && shasum -a 256 /tmp/intents.wasm
```

The script checks downloaded bytes against the reported chain code hash.
Compare that download with this pinned fixture before relying on a live identity.
An upstream source/audit mapping and privileged-role review are separate checks.
Preserve the existing fixture and evidence when testing a changed deployment.

Historical vault STATE fixtures and their independent generator are documented
in [legacy-state-fixtures.md](legacy-state-fixtures.md).
