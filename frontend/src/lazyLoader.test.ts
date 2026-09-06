import assert from "node:assert/strict";
import test from "node:test";

import { createCachedLoader } from "./lazyLoader.ts";

test("cached loader shares one import and keeps the resolved module", async () => {
  let calls = 0;
  const load = createCachedLoader(async () => {
    calls += 1;
    return { name: "page" };
  });

  const first = load();
  const second = load();

  assert.equal(first, second);
  assert.deepEqual(await first, { name: "page" });
  assert.deepEqual(await load(), { name: "page" });
  assert.equal(calls, 1);
});

test("cached loader permits retry after an import failure", async () => {
  let calls = 0;
  const load = createCachedLoader(async () => {
    calls += 1;
    if (calls === 1) throw new Error("temporary import failure");
    return "ready";
  });

  await assert.rejects(load(), /temporary import failure/);
  assert.equal(await load(), "ready");
  assert.equal(calls, 2);
});
