import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

test("release versions round-trip without a production frontend fallback", (t) => {
  const root = fileURLToPath(new URL("..", import.meta.url));
  const fixture = mkdtempSync(join(tmpdir(), "arcrelay-version-"));
  t.after(() => rmSync(fixture, { recursive: true, force: true }));
  for (const path of [
    "Cargo.toml",
    "Cargo.lock",
    "package.json",
    "package-lock.json",
    "tauri.conf.json",
    "gen/apple-macos/project.yml",
    "gen/apple-macos/ArcRelay.xcodeproj/project.pbxproj",
    "frontend/src/bridge.mock.ts",
  ]) {
    const destination = join(fixture, path);
    mkdirSync(dirname(destination), { recursive: true });
    copyFileSync(join(root, path), destination);
  }
  const original = JSON.parse(readFileSync(join(fixture, "package.json"), "utf8")).version;
  const [major, minor] = original.split(".");
  const next = `${major}.${minor}.54321`;
  const run = (...args) =>
    execFileSync(process.execPath, [join(root, "scripts/version.mjs"), ...args], {
      cwd: fixture,
      encoding: "utf8",
    });

  assert.equal(existsSync(join(fixture, "frontend/src/App.svelte")), false);
  run("--check", original);
  run(next);
  run("--check", next);
  assert.equal(JSON.parse(readFileSync(join(fixture, "package.json"), "utf8")).version, next);
  assert.ok(readFileSync(join(fixture, "frontend/src/bridge.mock.ts"), "utf8").includes(next));
  run(original);
  run("--check", original);
});
