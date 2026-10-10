import test from "node:test";
import assert from "node:assert/strict";
import { mergeAppPinnedEntries } from "./appPins.ts";

test("old pins outside the history page lead the list without duplicates or metadata loss", () => {
  const pins = [
    { id: 80, source: "Notes" },
    { id: 3, source: "Browser" },
  ];
  const page = [
    { id: 5, source: "Editor" },
    { id: 3, source: "old" },
  ];
  const result = mergeAppPinnedEntries(page, pins);
  assert.deepEqual(
    result.map((item) => item.id),
    [80, 3, 5],
  );
  assert.equal(result[1], pins[1]);
  assert.deepEqual(
    mergeAppPinnedEntries(
      [...result, { id: 3, source: "Browser" }, { id: 2, source: "Editor" }],
      pins,
    ).map((item) => item.id),
    [80, 3, 5, 2],
  );
});

test("switching application replaces the pin prefix while retaining chronological order", () => {
  const history = [{ id: 5 }, { id: 4 }, { id: 3 }];
  assert.deepEqual(
    mergeAppPinnedEntries(history, [{ id: 3 }]).map((item) => item.id),
    [3, 5, 4],
  );
  assert.deepEqual(
    mergeAppPinnedEntries(history, [{ id: 4 }]).map((item) => item.id),
    [4, 5, 3],
  );
  assert.deepEqual(mergeAppPinnedEntries(history, []), history);
});
