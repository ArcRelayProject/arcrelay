import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  buildUpdaterManifest,
  REQUIRED_UPDATER_TARGETS,
} from "./generate-updater-manifest.mjs";

const version = "0.1.2";
const payloads = [
  `ArcRelay_${version}_aarch64.app.tar.gz`,
  `ArcRelay_${version}_x64.app.tar.gz`,
  `ArcRelay_${version}_x64-setup.exe`,
  `ArcRelay_${version}_arm64-setup.exe`,
];

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "arcrelay-updater-manifest-"));
  const signatures = join(root, "signatures");
  mkdirSync(signatures);
  const assets = [];
  let id = 100;
  for (const payload of payloads) {
    assets.push({ id: id++, name: payload, label: payload });
    const signature = `${payload}.sig`;
    assets.push({ id: id++, name: signature, label: signature });
    writeFileSync(join(signatures, signature), `signed-${payload}\n`);
  }
  return { root, signatures, assets };
}

test("builds one atomic manifest for every supported desktop architecture", () => {
  const { root, signatures, assets } = fixture();
  try {
    const manifest = buildUpdaterManifest({
      release: {
        body: "Release notes",
        created_at: "2026-09-07T00:00:00Z",
        published_at: null,
      },
      assets,
      version,
      signatureDirectory: signatures,
    });
    assert.equal(manifest.version, version);
    assert.equal(manifest.notes, "Release notes");
    assert.equal(manifest.pub_date, "2026-09-07T00:00:00.000Z");
    assert.deepEqual(Object.keys(manifest.platforms), REQUIRED_UPDATER_TARGETS);
    assert.match(manifest.platforms["windows-aarch64"].url, /\/assets\/106$/);
    assert.equal(
      manifest.platforms["darwin-x86_64"],
      manifest.platforms["darwin-x86_64-app"],
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("fails closed when a required architecture is missing", () => {
  const { root, signatures, assets } = fixture();
  try {
    assert.throws(
      () =>
        buildUpdaterManifest({
          release: { created_at: "2026-09-07T00:00:00Z" },
          assets: assets.filter(
            ({ name }) => name !== `ArcRelay_${version}_arm64-setup.exe`,
          ),
          version,
          signatureDirectory: signatures,
        }),
      /Expected exactly one release asset named ArcRelay_0\.1\.2_arm64-setup\.exe, found 0/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
