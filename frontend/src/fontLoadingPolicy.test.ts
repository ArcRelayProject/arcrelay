import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const css = readFileSync(new URL("./fonts.css", import.meta.url), "utf8");

test("startup UI uses system fonts without a blocking webfont", () => {
  assert.doesNotMatch(css, /@font-face/);
  assert.doesNotMatch(css, /font-display:\s*block/);
  assert.match(css, /-apple-system/);
  assert.match(css, /"Segoe UI"/);
});
