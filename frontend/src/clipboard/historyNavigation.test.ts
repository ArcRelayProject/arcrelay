import assert from "node:assert/strict";
import test from "node:test";
import { captureScrollAnchor, historyQueryKey, mergeTimelineEntries, scrollTopForAnchor } from "./historyNavigation.ts";
import type { ClipboardItem, ClipboardTimeline } from "./types";

const item = (id: number) => ({ id } as ClipboardItem);
const page = (...ids: number[]) => ({ entries: ids.map(item) } as ClipboardTimeline);

test("timeline prepends and appends without repeating recopied or duplicate records", () => {
  const current = [item(4), item(3), item(2)];
  assert.deepEqual(mergeTimelineEntries(current, page(6, 5, 4, 5), true).map(x => x.id), [6, 5, 4, 3, 2]);
  assert.deepEqual(mergeTimelineEntries(current, page(3, 2, 1, 1), false).map(x => x.id), [4, 3, 2, 1]);
  assert.equal(mergeTimelineEntries(current, page(4), true)[0], current[0]);
  assert.equal(current.length, 3);
});

test("preserves the visible row and pixel offset after prepend and image resize", () => {
  const entries = [{ id: 3, height: 100 }, { id: 2, height: 200 }, { id: 1, height: 90 }];
  const height = (item: typeof entries[number]) => item.height;
  const anchor = captureScrollAnchor(entries, 145, height)!;
  assert.deepEqual(anchor, { id: 2, offset: 45 });
  const prepended = [{ id: 4, height: 120 }, ...entries];
  assert.equal(scrollTopForAnchor(prepended, anchor, height), 265);
  prepended[1] = { id: 3, height: 160 };
  assert.equal(scrollTopForAnchor(prepended, anchor, height), 325);
  assert.equal(scrollTopForAnchor([], anchor, height), null);
});

test("handles exact row boundaries, empty histories and below-viewport anchors", () => {
  const entries = [item(2), item(1)];
  assert.deepEqual(captureScrollAnchor(entries, 100, () => 100), { id: 1, offset: 0 });
  assert.equal(captureScrollAnchor([], 0, () => 100), null);
  assert.equal(scrollTopForAnchor(entries, { id: 1, offset: -40 }, () => 100), 60);
});

test("query identity includes all filters and safely handles arbitrary keywords", () => {
  assert.notEqual(historyQueryKey("ssh", "all", null), historyQueryKey("ssh", "text", null));
  assert.notEqual(historyQueryKey("ssh", "all", null), historyQueryKey("ssh", "all", "work"));
  assert.deepEqual(JSON.parse(historyQueryKey('ssh "工作"', "favorites", "label")), ['ssh "工作"', "favorites", "label"]);
});
