import assert from "node:assert/strict";
import test from "node:test";

import { imagePreviewHeight, imagePreviewLayout } from "./imagePreviewGeometry.ts";

test("zooming in always grows the displayed image", () => {
  const base = imagePreviewLayout({
    imageWidth: 2939,
    imageHeight: 1418,
    canvasWidth: 760,
    canvasHeight: 520,
    scale: 1,
    rotation: 0,
  });
  const zoomed = imagePreviewLayout({
    imageWidth: 2939,
    imageHeight: 1418,
    canvasWidth: 760,
    canvasHeight: 520,
    scale: 1.75,
    rotation: 0,
  });
  assert.ok(base && zoomed);
  assert.ok(zoomed.imageWidth > base.imageWidth);
  assert.ok(zoomed.imageHeight > base.imageHeight);
  assert.ok(zoomed.stageWidth > base.stageWidth);
  assert.ok(Math.abs(zoomed.imageWidth / base.imageWidth - 1.75) < 0.000_001);
});

test("quarter-turn rotation fits against swapped image bounds", () => {
  const layout = imagePreviewLayout({
    imageWidth: 1600,
    imageHeight: 900,
    canvasWidth: 760,
    canvasHeight: 520,
    scale: 1,
    rotation: 90,
  });
  assert.ok(layout);
  assert.ok(layout.imageHeight <= 712);
  assert.ok(layout.imageWidth <= 496);
  assert.equal(layout.stageWidth, 760);
  assert.equal(layout.stageHeight, 520);
});

test("landscape previews shrink-wrap their fitted image instead of reserving a fixed canvas", () => {
  assert.equal(imagePreviewHeight({
    imageWidth: 1600,
    imageHeight: 900,
    containerWidth: 760,
    rotation: 0,
  }), 438);
});

test("portrait and rotated previews retain a bounded working area", () => {
  assert.equal(imagePreviewHeight({
    imageWidth: 900,
    imageHeight: 1600,
    containerWidth: 760,
    rotation: 0,
  }), 560);
  assert.equal(imagePreviewHeight({
    imageWidth: 1600,
    imageHeight: 900,
    containerWidth: 760,
    rotation: 90,
  }), 560);
});
