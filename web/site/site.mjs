import { NETWORKS, validateAccount, validateHash } from './audit.mjs';

// Share only public account identity. RPC paths, receipt references and every
// unrelated query parameter are excluded from the copied link.
export function vaultLink(page, { network, account, expectedCodeHash = '' }) {
  if (!Object.hasOwn(NETWORKS, network) || network === 'local-fixture') {
    throw new Error('Choose mainnet or testnet before sharing a vault.');
  }
  const url = new URL(page);
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) {
    throw new Error('Open the website through HTTP(S) before sharing.');
  }
  validateAccount(account);
  if (expectedCodeHash) validateHash(expectedCodeHash, 'code hash');
  url.search = '';
  url.searchParams.set('network', network);
  url.searchParams.set('vault', account);
  if (expectedCodeHash) url.searchParams.set('code', expectedCodeHash);
  url.hash = 'vault';
  return url.href;
}

export function sharedVault(page) {
  const params = new URL(page).searchParams;
  if (!params.has('vault')) return null;
  for (const name of ['vault', 'network', 'code']) {
    if (params.getAll(name).length > 1) throw new Error('The vault link has duplicate fields.');
  }
  const network = params.get('network') || 'mainnet';
  if (!Object.hasOwn(NETWORKS, network) || network === 'local-fixture') {
    throw new Error('The vault link has an unsupported network.');
  }
  const account = validateAccount(params.get('vault'));
  const expectedCodeHash = params.get('code') || '';
  if (expectedCodeHash) validateHash(expectedCodeHash, 'code hash');
  return { network, account, expectedCodeHash };
}

export async function copyText(clipboard, value) {
  if (typeof clipboard?.writeText !== 'function') {
    throw new Error('Clipboard unavailable. Select and copy the text manually.');
  }
  try { await clipboard.writeText(value); }
  catch { throw new Error('Copy failed. Select and copy the text manually.'); }
}

export function mountSite(doc, locationRef, clipboard) {
  const field = (id) => doc.getElementById(id);
  const form = field('audit-form');
  if (!form) return;
  field('read-vault').disabled = false;
  field('share-vault').disabled = false;
  field('copy-verify').disabled = false;
  const status = field('share-status');
  try {
    const saved = sharedVault(locationRef.href);
    if (saved) {
      field('audit-network').value = saved.network;
      field('audit-rpc').value = NETWORKS[saved.network].rpc;
      field('audit-account').value = saved.account;
      field('audit-code').value = saved.expectedCodeHash;
      field('audit-status').textContent = 'Vault link loaded. Select Read vault for a fresh final-block snapshot.';
    }
  } catch (error) { status.textContent = error.message; }
  // The demo vault is read as soon as the page loads, so a visitor sees live state without a click.
  if (typeof form.requestSubmit === 'function') form.requestSubmit();

  field('share-vault').addEventListener('click', async () => {
    try {
      const url = vaultLink(locationRef.href, {
        network: field('audit-network').value,
        account: field('audit-account').value.trim(),
        expectedCodeHash: field('audit-code').value.trim(),
      });
      await copyText(clipboard, url);
      status.textContent = 'Vault link copied. It uses the selected network’s standard RPC.';
    } catch (error) { status.textContent = error.message; }
  });
  field('copy-verify').addEventListener('click', async () => {
    const feedback = field('copy-status');
    try {
      await copyText(clipboard, './scripts/verify.sh --integration');
      feedback.textContent = 'Verification command copied.';
    } catch (error) { feedback.textContent = error.message; }
  });

  if (typeof MutationObserver !== 'undefined') {
    new MutationObserver(() => {
      const loading = field('audit-status').textContent.startsWith('Reading one final-block');
      field('read-vault').disabled = loading;
      field('read-vault').setAttribute('aria-busy', String(loading));
    }).observe(field('audit-status'), { childList: true, characterData: true, subtree: true });
  }
  if (typeof IntersectionObserver !== 'undefined') {
    const links = [...doc.querySelectorAll('.nav a')];
    const observer = new IntersectionObserver((entries) => {
      const visible = entries.find((entry) => entry.isIntersecting);
      if (!visible) return;
      for (const link of links) {
        if (link.hash === `#${visible.target.id}`) link.setAttribute('aria-current', 'location');
        else link.removeAttribute('aria-current');
      }
    }, { rootMargin: '-20% 0px -60% 0px' });
    for (const link of links) {
      const section = doc.getElementById(link.hash.slice(1));
      if (section) observer.observe(section);
    }
  }
}

if (typeof document !== 'undefined') mountSite(document, location, navigator.clipboard);
