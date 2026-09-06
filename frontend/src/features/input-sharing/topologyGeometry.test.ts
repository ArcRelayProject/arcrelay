import assert from "node:assert/strict";
import test from "node:test";

import type { DisplaySurface } from "../../types.ts";
import { moveDisplayGroup, resizeDisplay, snapDisplayPosition } from "./topologyGeometry.ts";

function display(id: string, x: number, y: number, width: number, height: number): DisplaySurface {
  return {
    displayId: id,
    deviceId: `device-${id}`,
    fingerprint: id,
    name: id,
    pixelSize: { width: 1920, height: 1080 },
    logicalBounds: { x: 0, y: 0, width: 1920, height: 1080 },
    scaleFactor: 1,
    physicalSizeUm: { width, height },
    rotation: "Degrees0",
    deskRectUm: { x, y, width, height },
    geometryConfidence: "UserCalibrated",
    inventoryRevision: 1,
  };
}

// Portal derivation vectors are covered by arcrelay-input/src/topology_tests.rs.
test("snaps a nearby screen edge into exact contact", () => {
  const a = display("a", 0, 0, 500_000, 300_000);
  const b = display("b", 540_000, 50_000, 400_000, 200_000);
  assert.deepEqual(snapDisplayPosition("b", 512_000, 50_000, { a, b }, 15_000), {
    x: 500_000,
    y: 50_000,
    snapped: true,
  });
});

test("moves every screen owned by the same device as one system-layout group", () => {
  const a = display("a", 0, 0, 500_000, 300_000);
  const b = display("b", 500_000, 40_000, 400_000, 200_000);
  const remote = display("remote", 1_200_000, 0, 450_000, 280_000);
  b.deviceId = a.deviceId;
  const displays = { a, b, remote };

  moveDisplayGroup("b", 700_000, 90_000, displays);

  assert.deepEqual(a.deskRectUm, { x: 200_000, y: 50_000, width: 500_000, height: 300_000 });
  assert.deepEqual(b.deskRectUm, { x: 700_000, y: 90_000, width: 400_000, height: 200_000 });
  assert.equal(remote.deskRectUm.x, 1_200_000);
});

test("snaps a system-layout group by the nearest member edge", () => {
  const a = display("a", 0, 0, 500_000, 300_000);
  const b = display("b", 500_000, 0, 400_000, 300_000);
  const remote = display("remote", 950_000, 0, 450_000, 300_000);
  b.deviceId = a.deviceId;

  assert.deepEqual(snapDisplayPosition("a", 48_000, 0, { a, b, remote }, 10_000), {
    x: 50_000,
    y: 0,
    snapped: true,
  });
});

test("resizes one display proportionally and persists the calibrated physical size", () => {
  const a = display("a", 10_000, 20_000, 500_000, 300_000);

  assert.deepEqual(resizeDisplay("a", 750_000, { a }), { width: 750_000, height: 450_000 });
  assert.deepEqual(a.deskRectUm, { x: 10_000, y: 20_000, width: 750_000, height: 450_000 });
  assert.deepEqual(a.physicalSizeUm, { width: 750_000, height: 450_000 });
  assert.equal(a.geometryConfidence, "UserCalibrated");
});

test("keeps manual display resizing within useful physical limits", () => {
  const a = display("a", 0, 0, 500_000, 250_000);

  assert.deepEqual(resizeDisplay("a", 10_000, { a }), { width: 160_000, height: 80_000 });
  assert.deepEqual(resizeDisplay("a", 10_000_000, { a }), { width: 3_000_000, height: 1_500_000 });
});
