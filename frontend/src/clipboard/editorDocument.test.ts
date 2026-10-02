import test from "node:test";
import assert from "node:assert/strict";
import { whitespaceDiff } from "./editor/textDiff.ts";
import { boundedRect, ImageHistory, type ImageDocument } from "./editor/imageDocument.ts";
test("ignoring spaces maps actual text changes back to the unchanged document", () => {
  assert.equal(whitespaceDiff("hello world\n", " hello  world \n").length, 0);
  const a = "hello  world\n",
    b = "hello  there\n";
  const changes = whitespaceDiff(a, b);
  assert.ok(changes.length);
  let result = a;
  for (const change of [...changes].reverse())
    result =
      result.slice(0, change.fromA) + b.slice(change.fromB, change.toB) + result.slice(change.toA);
  assert.equal(result.replaceAll(" ", ""), b.replaceAll(" ", ""));
});
test("crop stays inside original pixel bounds and honors the chosen ratio", () => {
  const crop = boundedRect({ x: 100, y: 100 }, { x: 1000, y: 800 }, 1200, 900, 16 / 9);
  assert.ok(crop.x + crop.width <= 1200);
  assert.ok(crop.y + crop.height <= 900);
  assert.ok(Math.abs(crop.width / crop.height - 16 / 9) < 0.01);
  const free = boundedRect({ x: 100, y: 100 }, { x: 400, y: 800 }, 1200, 900);
  assert.equal(free.width, 300);
  assert.equal(free.height, 700);
});
test("image undo restores original pixels and new changes discard redo", () => {
  const original: ImageDocument = { base: "original", width: 100, height: 60, marks: [] };
  const history = new ImageHistory(original);
  history.push({ ...original, base: "cropped", width: 50 });
  assert.equal(history.undo().base, "original");
  assert.equal(history.redo().width, 50);
  history.undo();
  history.push({ ...original, base: "rotated", width: 60, height: 100 });
  assert.equal(history.canRedo, false);
  assert.equal(history.undo().width, 100);
});
