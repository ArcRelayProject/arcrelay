import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

test("connection settings render the backend-resolved local identity", () => {
  const page = readFileSync(
    new URL("./features/settings/SettingsView.svelte", import.meta.url),
    "utf8",
  );

  assert.match(page, /snapshot\?\.localDeviceName/);
  assert.match(page, /snapshot\?\.localPlatform === "windows"/);
  assert.doesNotMatch(page, /appSettings\.deviceName \|\| "这台 Mac"/);
});
