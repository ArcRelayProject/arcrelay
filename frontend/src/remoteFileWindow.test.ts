import test from "node:test";
import assert from "node:assert/strict";
import { fileWindow, RequestOwner } from "./remoteFileWindow.ts";

void test("directory rendering remains viewport bounded for 100,000 items in each view", () => {
  for (const mode of ["details", "compact", "grid"] as const) {
    const window = fileWindow(100_000, 100_000, 800, 1200, mode);
    assert.ok(window.end - window.start < 120);
    assert.equal(window.start % window.columns, 0);
    assert.ok(window.top > 0 && window.bottom > 0);
    const empty = fileWindow(0, 10, 800, 1200, mode);
    assert.equal(empty.start, 0);
    assert.equal(empty.end, 0);
  }
});
void test("a tree reply loses ownership after a device change, reload, or disposal", async () => {
  const owner = new RequestOwner();
  const old = owner.capture("a");
  assert.equal(old("a"), true);
  assert.equal(old("b"), false);
  owner.advance();
  assert.equal(old("a"), false);
  const current = owner.capture("a");
  owner.dispose();
  await Promise.resolve();
  assert.equal(current("a"), false);
});
