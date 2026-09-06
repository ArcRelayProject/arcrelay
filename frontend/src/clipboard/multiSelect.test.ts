import assert from "node:assert/strict";
import test from "node:test";

import { appendSelectedIds, selectionOrder, toggleSelectedId } from "./multiSelect.ts";

test("preserves the order in which clipboard items are selected", () => {
  let selected: number[] = [];
  selected = toggleSelectedId(selected, 7);
  selected = toggleSelectedId(selected, 3);
  selected = toggleSelectedId(selected, 9);
  assert.deepEqual(selected, [7, 3, 9]);
  assert.equal(selectionOrder(selected, 3), 2);
});

test("removes a deselected item and closes the order gap", () => {
  const selected = toggleSelectedId([7, 3, 9], 3);
  assert.deepEqual(selected, [7, 9]);
  assert.equal(selectionOrder(selected, 9), 2);
});

test("appends bulk selection without duplicates", () => {
  assert.deepEqual(appendSelectedIds([3, 7], [7, 9, 3, 11]), [3, 7, 9, 11]);
});
