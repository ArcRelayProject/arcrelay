import { spawnSync } from 'node:child_process';
import { cpSync, mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import path from 'node:path';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

if (process.platform === 'win32') {
  const script = path.join(root, 'native', 'windows-share', 'build.ps1');
  run('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', script]);
} else if (process.platform === 'darwin') {
  const temporaryRoot = mkdtempSync(path.join(tmpdir(), 'arcrelay-share-extension-'));
  const outputRoot = path.join(root, 'binaries', 'macos-share');
  const productRoot = path.join(temporaryRoot, 'products');
  const config = JSON.parse(readFileSync(path.join(root, 'tauri.conf.json'), 'utf8'));
  const version = String(config.version);

  try {
    mkdirSync(outputRoot, { recursive: true });
    for (const entry of readdirSync(outputRoot)) {
      if (entry !== '.gitignore') rmSync(path.join(outputRoot, entry), { recursive: true, force: true });
    }
    for (const target of ['ArcRelayShare', 'ArcRelayFiles']) {
    const outputExtension = path.join(outputRoot, `${target}.appex`);
    run('xcodebuild', [
      '-project', path.join(root, 'gen', 'apple-macos', 'ArcRelay.xcodeproj'),
      '-target', target,
      '-configuration', 'Release',
      '-sdk', 'macosx',
      '-quiet',
      `CONFIGURATION_BUILD_DIR=${productRoot}`,
      `OBJROOT=${path.join(temporaryRoot, 'objects')}`,
      `SYMROOT=${path.join(temporaryRoot, 'symbols')}`,
      'ARCHS=arm64 x86_64',
      'ONLY_ACTIVE_ARCH=NO',
      'CODE_SIGNING_ALLOWED=NO',
      'CODE_SIGNING_REQUIRED=NO',
      `MARKETING_VERSION=${version}`,
      `CURRENT_PROJECT_VERSION=${version}`,
    ]);
    cpSync(path.join(productRoot, `${target}.appex`), outputExtension, { recursive: true });

    const signingIdentity = [
      process.env.ARCRELAY_MACOS_SIGNING_IDENTITY,
      process.env.APPLE_SIGNING_IDENTITY,
      process.env.MACOS_SIGNING_IDENTITY,
    ].find((identity) => identity?.trim()) ?? '-';
    const signingArguments = [
      '--force',
      '--sign', signingIdentity,
      '--options', 'runtime',
      '--entitlements', path.join(root, 'gen', 'apple-macos', target, `${target}.entitlements`),
    ];
    if (signingIdentity !== '-') signingArguments.push('--timestamp');
    signingArguments.push(outputExtension);
    run('codesign', signingArguments);
    run('codesign', ['--verify', '--strict', '--verbose=2', outputExtension]);
    }
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
}
