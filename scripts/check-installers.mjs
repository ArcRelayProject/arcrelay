import assert from 'node:assert/strict';
import { readFileSync, statSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = (path) => readFileSync(resolve(root, path));
const config = JSON.parse(read('tauri.conf.json'));
const pkg = JSON.parse(read('package.json'));
// Custom templates do not inherit upstream fixes automatically. Make upgrading
// the CLI an explicit template review instead of silently shipping stale code.
assert.equal(pkg.devDependencies['@tauri-apps/cli'], '2.11.4', 'Review the NSIS template against the new Tauri CLI');
const nsis = config.bundle.windows.nsis;
const template = read(nsis.template).toString('utf8');
const hooks = read(nsis.installerHooks).toString('utf8');
// NSIS accepts mixed quote delimiters, but $" is not an escape. It leaves
// literal dollar signs in Explorer's command even when the installer builds.
assert.doesNotMatch(hooks, /\$"/, 'Use literal double quotes inside single-quoted NSIS strings, or the NSIS $\\" escape');
const shareCommand = hooks.split('\n').find((line) => line.includes('WriteRegStr') && line.includes('ArcRelay.Share\\command'));
assert.ok(shareCommand, 'Explorer must have a Share with ArcRelay command');
const commandValue = shareCommand.match(/'([^']+)'\s*$/)?.[1];
assert.equal(commandValue,
  '"$INSTDIR\\${MAINBINARYNAME}.exe" --arcrelay-share-source windows-context-menu --arcrelay-share "%1"',
  'Explorer must quote both the installed executable and the selected file');
assert.ok(
  statSync(resolve(root, 'binaries/windows-share')).isDirectory(),
  'The generated Windows Share Target resource directory must exist in clean checkouts',
);
assert.ok(
  statSync(resolve(root, 'binaries/macos-share')).isDirectory(),
  'The generated macOS Share Extension directory must exist in clean checkouts',
);
assert.equal(
  config.bundle.macOS.files['PlugIns/ArcRelayShare.appex'],
  'binaries/macos-share/ArcRelayShare.appex',
  'The macOS Share Extension must be copied into the application PlugIns directory',
);
assert.equal(nsis.installMode, 'currentUser', 'Changing install scope requires an upgrade/migration review');
assert.equal(config.identifier, 'com.arcrelay.desktop', 'Keep the existing application/data identity');

for (const path of ['Info.plist', 'gen/apple-macos/ArcRelay/Info.plist']) {
  const infoPlist = read(path).toString('utf8');
  assert.match(infoPlist, /<key>NSLocalNetworkUsageDescription<\/key>\s*<string>[^<]+<\/string>/,
    `${path} must explain why ArcRelay needs local-network access`);
  assert.match(infoPlist, /<key>NSBonjourServices<\/key>\s*<array>\s*<string>_arcrelay\._udp<\/string>\s*<\/array>/,
    `${path} must declare the ArcRelay Bonjour service`);
}
const xcodeProject = read('gen/apple-macos/project.yml').toString('utf8');
assert.match(xcodeProject, /NSLocalNetworkUsageDescription:\s*\S/,
  'The generated macOS project must preserve the local-network usage description');
assert.match(xcodeProject, /NSBonjourServices:\s*\n\s*- _arcrelay\._udp/,
  'The generated macOS project must preserve the ArcRelay Bonjour service');

function bitmapSize(path) {
  const bytes = read(path);
  assert.equal(bytes.toString('ascii', 0, 2), 'BM', `${path} is not a Windows bitmap`);
  // Match NSIS's LoadImageCanLoadFile restrictions. A valid top-down BMP
  // otherwise passes dimensions/bpp checks but is omitted from the installer.
  assert.equal(bytes.readUInt32LE(14), 40, `${path} must use BITMAPINFOHEADER`);
  assert.ok(bytes.readInt32LE(22) > 0, `${path} must store rows bottom-up; run normalize-installer-bitmaps.mjs`);
  assert.equal(bytes.readUInt16LE(26), 1, `${path} must have one color plane`);
  assert.equal(bytes.readUInt16LE(28), 24, `${path} must be a 24-bit bitmap`);
  assert.equal(bytes.readUInt32LE(30), 0, `${path} must be uncompressed RGB`);
  return [bytes.readInt32LE(18), bytes.readInt32LE(22)];
}
function pngSize(path) {
  const bytes = read(path);
  assert.equal(bytes.toString('hex', 0, 8), '89504e470d0a1a0a', `${path} is not a PNG`);
  return [bytes.readUInt32BE(16), bytes.readUInt32BE(20)];
}
assert.deepEqual(bitmapSize(nsis.headerImage), [300, 114]);
assert.deepEqual(bitmapSize(nsis.sidebarImage), [328, 628]);
for (const path of [nsis.installerIcon, nsis.uninstallerIcon, nsis.installerHooks]) read(path);

let keys;
for (const language of nsis.languages) {
  const contents = read(nsis.customLanguageFiles[language]).toString('utf8');
  const translated = [...contents.matchAll(/^LangString (\w+) /gm)].map((match) => match[1]);
  assert.equal(new Set(translated).size, translated.length, `${language} has duplicate translations`);
  const sorted = translated.sort();
  if (keys) assert.deepEqual(sorted, keys, 'Installer languages must expose the same messages');
  keys = sorted;
  for (const [, key] of template.matchAll(/\$\(([A-Za-z]\w*)\)/g)) {
    assert.ok(translated.includes(key), `${language} is missing ${key}`);
  }
}

const dmg = config.bundle.macOS.dmg;
const size = [dmg.windowSize.width, dmg.windowSize.height];
assert.deepEqual(pngSize(dmg.background), size, 'DMG artwork must fit the Finder window');
assert.deepEqual(pngSize(dmg.background.replace(/\.png$/, '@2x.png')), size.map((v) => v * 2));
for (const point of [dmg.appPosition, dmg.applicationFolderPosition]) {
  assert.ok(point.x >= 64 && point.x <= size[0] - 64 && point.y >= 240 && point.y <= 340,
    'DMG icons must stay inside the artwork’s clear interaction area');
}
console.log('Installer assets, translations, identity and template version are valid.');
