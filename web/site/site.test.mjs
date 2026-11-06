import test from 'node:test';
import assert from 'node:assert/strict';
import { vaultLink, sharedVault, copyText } from './site.mjs';

const hash = '5'.repeat(44);

test('vault sharing drops RPC, receipt and unrelated URL data', () => {
  const result = vaultLink('https://mandatenear.xyz/?rpc=custom&receipt=old&other=value#old', {
    network: 'testnet', account: 'vault.testnet', expectedCodeHash: hash,
  });
  const url = new URL(result);
  assert.equal(url.origin, 'https://mandatenear.xyz');
  assert.deepEqual([...url.searchParams.keys()], ['network', 'vault', 'code']);
  assert.equal(url.hash, '#vault');
  assert.deepEqual(sharedVault(result), { network: 'testnet', account: 'vault.testnet', expectedCodeHash: hash });
});

test('invalid account text and unsupported or fixture networks cannot be shared', () => {
  for (const account of ['<img>', 'a..near', 'A.near', 'a']) {
    assert.throws(() => vaultLink('https://mandatenear.xyz/', { network: 'mainnet', account }));
  }
  for (const network of ['local-fixture', 'other', '__proto__']) {
    assert.throws(() => vaultLink('https://mandatenear.xyz/', { network, account: 'vault.near' }));
  }
});

test('share link refuses executable/file URLs, credentials and invalid code hashes', () => {
  for (const page of ['file:///project/index.html', 'javascript:alert(1)', 'https://user:password@example.com/']) {
    assert.throws(() => vaultLink(page, { network: 'mainnet', account: 'vault.near' }));
  }
  for (const expectedCodeHash of ['wrong', '1'.repeat(44), '0'.repeat(44)]) {
    assert.throws(() => vaultLink('https://mandatenear.xyz/', { network: 'mainnet', account: 'vault.near', expectedCodeHash }));
  }
});

test('shared links are bounded identity prefill, never a custom RPC instruction', () => {
  assert.equal(sharedVault('https://mandatenear.xyz/?other=value'), null);
  assert.deepEqual(sharedVault('https://mandatenear.xyz/?vault=vault.near&rpc=https://other.example'), {
    network: 'mainnet', account: 'vault.near', expectedCodeHash: '',
  });
  for (const search of ['vault=a..near', 'vault=vault.near&network=local-fixture', 'vault=vault.near&code=bad', 'vault=vault.near&vault=other.near', 'vault=vault.near&network=mainnet&network=testnet']) {
    assert.throws(() => sharedVault(`https://mandatenear.xyz/?${search}`));
  }
});

test('clipboard success is only returned after the real write resolves', async () => {
  let finish, copied = false;
  const pending = copyText({ writeText: () => new Promise((resolve) => { finish = resolve; }) }, 'public text')
    .then(() => { copied = true; });
  await Promise.resolve();
  assert.equal(copied, false);
  finish();
  await pending;
  assert.equal(copied, true);
});

test('clipboard unavailable or rejected reports failure without echoing raw errors', async () => {
  await assert.rejects(copyText(undefined, 'public text'), /Clipboard unavailable/);
  await assert.rejects(copyText({ writeText: async () => { throw new Error('internal detail'); } }, 'public text'), (error) => {
    assert.match(error.message, /Copy failed/);
    assert.ok(!error.message.includes('internal detail'));
    return true;
  });
});
