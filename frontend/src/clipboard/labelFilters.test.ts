import assert from "node:assert/strict";
import test from "node:test";

import { filterClipboardLabels, parseRecentLabelIds, rememberRecentLabel, selectQuickLabels } from "./labelFilters.ts";
import type { ClipboardLabel } from "./types.ts";

const labels: ClipboardLabel[] = [
  { id: "a", name: "工作", color: "#3b82f6", revision: 1, updated_by_device_id: "local", deleted: false },
  { id: "b", name: "项目 Alpha", color: "#22c55e", revision: 1, updated_by_device_id: "local", deleted: false },
  { id: "c", name: "临时", color: "#f59e0b", revision: 1, updated_by_device_id: "local", deleted: false },
  { id: "d", name: "代码", color: "#8b5cf6", revision: 1, updated_by_device_id: "local", deleted: false },
];

test("quick labels prioritize the selected and recently used labels", () => {
  assert.deepEqual(selectQuickLabels(labels, ["c", "a"], "d").map((label) => label.id), ["d", "c", "a"]);
});

test("quick labels ignore missing and duplicate ids", () => {
  assert.deepEqual(selectQuickLabels(labels, ["missing", "a", "a"], null).map((label) => label.id), ["a", "b", "c"]);
});

test("label search is trimmed and case insensitive", () => {
  assert.deepEqual(filterClipboardLabels(labels, " alpha ").map((label) => label.id), ["b"]);
  assert.deepEqual(filterClipboardLabels(labels, "代码").map((label) => label.id), ["d"]);
});

test("recent label storage is bounded and safely parsed", () => {
  assert.deepEqual(rememberRecentLabel(["a", "b", "c"], "b", 3), ["b", "a", "c"]);
  assert.deepEqual(parseRecentLabelIds('["a",2,"b"]'), ["a", "b"]);
  assert.deepEqual(parseRecentLabelIds("invalid"), []);
});
