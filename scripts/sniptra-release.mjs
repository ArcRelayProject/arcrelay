import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, mkdirSync, appendFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = 'ArcRelayProject/sniptra';
const api = `https://api.github.com/repos/${repository}`;
const platforms = ['macos-universal', 'windows-x86_64'];
export function validateManifest(m, tag) {
  if (m.schema_version !== 1 || m.protocol_version !== 1 || m.release !== tag ||
      !/^v\d+\.\d+\.\d+-ci\.\d+$/.test(tag)) throw new Error('Incompatible Sniptra manifest');
  for (const platform of platforms) {
    const a = m.assets?.[platform];
    const name = `sniptra-${tag}-integration-${platform}.zip`;
    if (!a || a.name !== name || a.url !== `https://github.com/${repository}/releases/download/${tag}/${name}` ||
        !/^[a-f0-9]{64}$/.test(a.sha256)) throw new Error(`Invalid Sniptra asset: ${platform}`);
  }
  return m;
}
async function request(url) {
  const headers = { 'User-Agent': 'ArcRelay-Sniptra' };
  if (url.startsWith('https://api.github.com/') && process.env.GH_TOKEN) headers.Authorization = `Bearer ${process.env.GH_TOKEN}`;
  const response = await fetch(url, { headers, signal: AbortSignal.timeout(120000) });
  if (!response.ok) throw new Error(`Sniptra download failed (${response.status}): ${url}`);
  return response;
}
export async function resolveRelease(tag = '', build = '') {
  if (build && !/^\d+$/.test(build)) throw new Error('Invalid legacy Sniptra build number');
  if (tag && !/^v\d+\.\d+\.\d+-ci\.\d+$/.test(tag)) throw new Error('Invalid Sniptra release tag');
  for (let page = 1; page <= 20; page++) {
    const result = await (await request(tag ? `${api}/releases/tags/${tag}` : `${api}/releases?per_page=100&page=${page}`)).json();
    const releases = tag ? [result] : result;
    if (!Array.isArray(releases)) throw new Error('Invalid release list');
    for (const r of releases) {
      if (r.draft || r.prerelease || !/^v\d+\.\d+\.\d+-ci\.\d+$/.test(r.tag_name)) continue;
      const a = r.assets.find(a => a.name === 'release-manifest.json');
      if (!a) { if (tag) throw new Error('Pinned release has no manifest'); else continue; }
      const m = await (await request(a.browser_download_url)).json();
      if (m.protocol_version !== 1) { if (tag) throw new Error('Pinned release is incompatible'); else continue; }
      if (build && m.jenkins_build !== Number(build)) continue;
      return validateManifest(m, r.tag_name);
    }
    if (tag || releases.length < 100) break;
  }
  throw new Error('No compatible published Sniptra release');
}
async function main() {
  const [command, lockFile, target, outputDirectory] = process.argv.slice(2);
  if (command === 'resolve') {
    const m = await resolveRelease(process.env.SNIPTRA_RELEASE_TAG || '', process.env.SNIPTRA_BUILD_NUMBER || '');
    const lock = JSON.stringify(m);
    writeFileSync(lockFile, `${lock}\n`);
    if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `lock=${lock}\nrelease=${m.release}\n`);
    console.log(`Locked Sniptra ${m.release}`);
  } else if (command === 'download') {
    const m = JSON.parse(readFileSync(lockFile, 'utf8'));
    validateManifest(m, m.release);
    const platform = target.endsWith('-apple-darwin') ? 'macos-universal' :
      ['x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc'].includes(target) ? 'windows-x86_64' : null;
    if (!platform) throw new Error(`Unsupported Sniptra target: ${target}`);
    const asset = m.assets[platform];
    const bytes = Buffer.from(await (await request(asset.url)).arrayBuffer());
    if (createHash('sha256').update(bytes).digest('hex') !== asset.sha256) throw new Error('Sniptra archive checksum mismatch');
    const directory = outputDirectory ? path.resolve(outputDirectory) : path.resolve('.sniptra', m.release, platform);
    mkdirSync(directory, { recursive: true });
    const archive = path.join(directory, 'component.zip');
    writeFileSync(archive, bytes);
    // The verified official archive has a fixed allowlist. Extract only these names.
    const ext = platform.startsWith('windows') ? '.exe' : '';
    const names = [`sniptra${ext}`, `sniptra-ocr-worker${ext}`, 'integration-info.json', 'SHA256SUMS.txt', 'BINARY-LICENSE.txt', 'README.md'];
    execFileSync('tar', ['-xf', archive, '-C', directory, ...names], { stdio: 'inherit' });
    if (process.env.GITHUB_ENV) appendFileSync(process.env.GITHUB_ENV,
      `SNIPTRA_ARTIFACT_DIR=${directory}\nSNIPTRA_SIDECAR_TARGET=${target}\nARCRELAY_REQUIRE_SNIPTRA=1\n`);
    console.log(`Component directory: ${directory}`);
    console.log(`Verified ${m.release}: ${platform} for ${target}${target.startsWith('aarch64-pc-windows') ? ' (Windows 11 x64 emulation)' : ''}`);
  } else throw new Error('Usage: sniptra-release.mjs resolve LOCK | download LOCK TARGET');
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
