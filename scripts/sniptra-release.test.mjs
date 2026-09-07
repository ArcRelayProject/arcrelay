import test from 'node:test';
import assert from 'node:assert/strict';
import { validateManifest, resolveRelease } from './sniptra-release.mjs';
const tag = 'v0.1.0-ci.16';
function manifest() {
  return { schema_version: 1, protocol_version: 1, release: tag, assets: Object.fromEntries(
    ['macos-universal', 'windows-x86_64'].map(p => {
      const name = `sniptra-${tag}-integration-${p}.zip`;
      return [p, { name, url: `https://github.com/ArcRelayProject/sniptra-releases/releases/download/${tag}/${name}`, sha256: 'a'.repeat(64) }];
    })) };
}
test('requires both compatible platforms and exact release URLs', () => {
  assert.equal(validateManifest(manifest(), tag).release, tag);
  for (const mutate of [m => delete m.assets['windows-x86_64'], m => m.protocol_version = 2,
    m => m.assets['macos-universal'].sha256 = 'bad', m => m.assets['macos-universal'].url = 'https://example.com/payload',
    m => m.assets['macos-universal'].name = '../payload.zip']) {
    const m = manifest(); mutate(m); assert.throws(() => validateManifest(m, tag));
  }
});
test('skips incompatible releases, resolves once, and rejects incompatible pins', async () => {
  const oldFetch = globalThis.fetch;
  const calls = [];
  globalThis.fetch = async url => {
    calls.push(url);
    let data;
    if (url.includes('/releases?')) data = [
      { tag_name: 'v0.2.0-ci.17', assets: [{ name: 'release-manifest.json', browser_download_url: 'https://test/new' }] },
      { tag_name: tag, assets: [{ name: 'release-manifest.json', browser_download_url: 'https://test/old' }] },
    ];
    else if (url.includes('/tags/')) data = { tag_name: tag, assets: [{ name: 'release-manifest.json', browser_download_url: 'https://test/new' }] };
    else data = url.endsWith('/new') ? { protocol_version: 2 } : manifest();
    return { ok: true, json: async () => data };
  };
  try {
    assert.equal((await resolveRelease()).release, tag);
    assert.equal(calls.length, 3);
    await assert.rejects(resolveRelease(tag), /incompatible/);
  } finally { globalThis.fetch = oldFetch; }
});
