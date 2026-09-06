import assert from "node:assert/strict";
import test from "node:test";

import { testVersion, validateStableVersion, validateUpdaterManifest } from "./release-channel.mjs";

test("test releases use an ordered platform-safe version range", () => {
  assert.equal(testVersion("0.1.1", "42"), "0.1.10042");
  assert.throws(() => testVersion("0.1.1-beta.1", "42"));
  assert.throws(() => testVersion("0.1.1", "0"));
  assert.throws(() => testVersion("0.1.10000", "42"));
  assert.equal(validateStableVersion("0.1.9999"), "0.1.9999");
  assert.throws(() => validateStableVersion("0.1.10000"));
});

test("updater manifests require every signed release target", () => {
  const manifest = {
    version: "0.1.10042",
    platforms: {
      "darwin-aarch64": {
        url: "https://api.github.com/repos/ArcRelayProject/arcrelay/releases/assets/123456",
        signature: "a".repeat(64),
      },
      "windows-x86_64": {
        url: "https://github.com/ArcRelayProject/arcrelay/releases/download/test-v0.1.10042/ArcRelay.exe",
        signature: "b".repeat(64),
      },
    },
  };
  assert.equal(
    validateUpdaterManifest(manifest, manifest.version, ["darwin-aarch64", "windows-x86_64"]),
    manifest,
  );
  assert.throws(() => validateUpdaterManifest(manifest, "0.1.1", ["darwin-aarch64"]));
  assert.throws(() => validateUpdaterManifest(manifest, manifest.version, ["darwin-x86_64"]));
  manifest.platforms["darwin-aarch64"].url =
    "https://example.com/ArcRelayProject/arcrelay/releases/assets/123456";
  assert.throws(() => validateUpdaterManifest(manifest, manifest.version, ["darwin-aarch64"]));
});
