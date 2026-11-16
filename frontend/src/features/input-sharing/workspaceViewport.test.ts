import test from "node:test";
import assert from "node:assert/strict";
import { workspaceBounds, workspaceProjection } from "./workspaceViewport.ts";
import type { DisplaySurface } from "../../types.ts";

const display = (x: number, y: number, width: number, height: number) =>
  ({ deskRectUm: { x, y, width, height } }) as DisplaySurface;

test("fit contains the complete irregular workspace, including negative coordinates and an offline island", () => {
  const screens = [
    display(0, 0, 600000, 400000),
    display(300000, -170000, 300000, 170000),
    display(-300000, 0, 300000, 170000),
    display(720000, 0, 300000, 170000),
    display(1020000, 0, 160000, 110000),
  ];
  const bounds = workspaceBounds(screens);
  assert.deepEqual(bounds, { x: -300000, y: -170000, width: 1480000, height: 570000 });
  for (const [width, height] of [
    [1180, 600],
    [500, 300],
    [240, 180],
  ]) {
    const view = workspaceProjection(bounds, width, height);
    for (const screen of screens) {
      const rect = screen.deskRectUm;
      assert.ok(view.x + rect.x * view.scale >= 0);
      assert.ok(view.y + rect.y * view.scale >= 0);
      assert.ok(view.x + (rect.x + rect.width) * view.scale <= width);
      assert.ok(view.y + (rect.y + rect.height) * view.scale <= height);
    }
    // A connected upper screen shares the exact edge at every viewport size.
    assert.equal(
      view.y + (screens[1].deskRectUm.y + screens[1].deskRectUm.height) * view.scale,
      view.y,
    );
  }
});

test("fit centers translated workspaces without treating the origin as a phantom display", () => {
  const bounds = workspaceBounds([display(900000, 800000, 600000, 400000)]);
  const view = workspaceProjection(bounds, 1000, 700);
  assert.equal(Math.round(view.x + (bounds.x + bounds.width / 2) * view.scale), 500);
  assert.equal(Math.round(view.y + (bounds.y + bounds.height / 2) * view.scale), 350);
});

test("zoom keeps the workspace center fixed and panning changes only the screen-space offset", () => {
  const bounds = workspaceBounds([display(-300000, -100000, 900000, 500000)]);
  const fit = workspaceProjection(bounds, 1000, 700);
  const zoom = workspaceProjection(bounds, 1000, 700, 2);
  const pan = workspaceProjection(bounds, 1000, 700, 2, { x: 120, y: -40 });
  assert.equal(zoom.scale, fit.scale * 2);
  assert.equal(pan.scale, zoom.scale);
  assert.equal(pan.x - zoom.x, 120);
  assert.equal(pan.y - zoom.y, -40);
  assert.equal(Math.round(zoom.x + (bounds.x + bounds.width / 2) * zoom.scale), 500);
});

test("empty and unmeasured canvases produce finite coordinates", () => {
  const view = workspaceProjection(workspaceBounds([]), 0, 0);
  assert.ok(Object.values(view).every(Number.isFinite));
  assert.ok(view.scale > 0);
});
