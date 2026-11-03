# Test fixtures

## `intents-near-mainnet.wasm`

The contract deployed at `intents.near` on NEAR mainnet, downloaded with
`view_code`, unmodified. Sandbox tests deploy it so the vault is exercised against
the real verifier, not a mock.

| Field | Value |
|---|---|
| Source account | `intents.near` (mainnet) |
| Fetched at block | 222833256 (2026-11-01) |
| On-chain code hash | `BtA1BEFNS619KkvXFsgcpQ21Tb5yMm32aaUkn7xNothk` |
| SHA-256 | `a1afab98916d9f213746544dcf8e4e6bdb22c0be6bc5dfbd8ea8814c9922b547` |
| Size | 1,297,551 bytes |
| Upstream | https://github.com/near/intents (MIT) |

Check the file against the chain yourself (read-only, no keys):

```bash
scripts/fetch-intents-fixture.sh /tmp/intents.wasm && shasum -a 256 /tmp/intents.wasm
```

The script fails if the downloaded bytes do not hash to the code hash currently on-chain.
If `intents.near` is upgraded, the live hash will differ from the table above. Refresh the
fixture with `scripts/fetch-intents-fixture.sh` and update the table.
