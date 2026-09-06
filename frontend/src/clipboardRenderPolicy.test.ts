import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const css = readFileSync(new URL("./clipboard/styles/base.css", import.meta.url), "utf8");
const app = readFileSync(new URL("./clipboard/ClipboardApp.svelte", import.meta.url), "utf8");

test("virtualized clipboard rows do not use hidden-webview containment", () => {
  const itemRule = css.match(/\.clipboard-item\s*\{[^}]+\}/s)?.[0] ?? "";
  assert.doesNotMatch(itemRule, /content-visibility:/);
  assert.doesNotMatch(itemRule, /contain-intrinsic-size:/);
});

test("visible-range calculation updates the rendered clipboard rows atomically", () => {
  const rangeFunction = app.match(/function recalculateVisibleRange[\s\S]+?\n  }\n\n  function updateVisibleRange/)?.[0] ?? "";
  assert.match(rangeFunction, /renderedEntries = entries\s*\.slice\(virtualStartIndex, virtualEndIndex\)/);
});
