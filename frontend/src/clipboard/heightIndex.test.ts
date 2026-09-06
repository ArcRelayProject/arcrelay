import assert from "node:assert/strict";
import test from "node:test";
import { HeightIndex } from "./heightIndex.ts";

test("10k rows retain viewport and anchor correctness across resizes", () => {
  const heights = Array.from({ length: 10_000 }, (_, i) => 80 + i % 90);
  const index = new HeightIndex(heights.map((height, id) => ({ id, height })));
  for (let id = 0; id < heights.length; id += 37) {
    heights[id] += 54;
    index.set(id, heights[id]);
  }
  let offset = 0;
  for (let id = 0; id < heights.length; id++) {
    assert.equal(index.offset(id), offset);
    assert.equal(index.indexAt(offset), id);
    assert.equal(index.indexAt(offset + heights[id] - 0.5), id);
    assert.deepEqual(index.anchor(offset + 3), { id, offset: 3 });
    assert.equal(index.anchorTop({ id, offset: 3 }), offset + 3);
    offset += heights[id];
  }
  assert.equal(index.total, offset);
  assert.equal(index.indexAt(offset), heights.length);
  assert.equal(index.anchorTop({ id: -1, offset: 0 }), null);
});

test("empty lists and invalid measurements", () => {
  const index = new HeightIndex([]);
  assert.equal(index.anchor(0), null);
  assert.equal(index.offset(10), 0);
  assert.equal(index.indexAt(0), 0);
  assert.throws(() => new HeightIndex([{ id: 1, height: Number.NaN }]), RangeError);
});
