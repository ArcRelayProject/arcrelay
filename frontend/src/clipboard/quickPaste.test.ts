import assert from "node:assert/strict";
import test from "node:test";
import { visibleQuickPasteIds } from "./quickPaste.ts";

const entries = Array.from({ length: 30 }, (_, index) => ({ id: 100 + index }));
const offset = (index: number) => index * 100;
test("numbers the viewport rather than the beginning or virtual overscan", () => {
  assert.deepEqual(visibleQuickPasteIds(entries, 17, 23, 1750, 400, offset), [118, 119, 120, 121]);
  assert.deepEqual(visibleQuickPasteIds(entries, 18, 24, 1850, 400, offset), [119, 120, 121, 122]);
});
test("includes tall cards and caps shortcuts at nine", () => {
  assert.deepEqual(visibleQuickPasteIds(entries, 0, 1, 200, 300, index => index * 1000), [100]);
  assert.equal(visibleQuickPasteIds(entries, 0, 30, 0, 3000, offset).length, 9);
  assert.deepEqual(visibleQuickPasteIds(entries, 0, 3, 0, 0, offset), []);
});
test("resolves reordered and filtered records by stable ID", () => {
  assert.deepEqual(visibleQuickPasteIds([entries[20], entries[3]], 0, 2, 0, 200, offset), [120, 103]);
  assert.deepEqual(visibleQuickPasteIds([], 0, 0, 0, 200, offset), []);
});
