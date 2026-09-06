import assert from "node:assert/strict";
import test from "node:test";

import {
  hitTestImageCharacter,
  imageTextBoundaryAtPoint,
  imageTextLineRange,
  imageTextSelectionBands,
  imageTextWordRange,
  selectableImageCharacters,
  selectedImageText,
} from "./imageTextSelection.ts";
import type { ClipboardImageOcr } from "./types.ts";

const ocr: ClipboardImageOcr = {
  text: "AB\n中",
  modelVersion: "test",
  updatedAtMs: 1,
  blocks: [
    {
      text: "AB",
      confidence: 0.9,
      left: 10,
      top: 20,
      width: 100,
      height: 20,
      points: null,
      characters: [],
    },
    {
      text: "中",
      confidence: 0.95,
      left: 20,
      top: 60,
      width: 30,
      height: 30,
      points: null,
      characters: [{
        text: "中",
        confidence: 0.95,
        points: [
          { x: 20, y: 60 },
          { x: 50, y: 65 },
          { x: 48, y: 95 },
          { x: 18, y: 90 },
        ],
      }],
    },
  ],
};

test("falls back to equal block interpolation for historical OCR results", () => {
  const characters = selectableImageCharacters(ocr);

  assert.equal(characters.length, 3);
  assert.deepEqual(characters[0].points, [
    { x: 10, y: 20 },
    { x: 60, y: 20 },
    { x: 60, y: 40 },
    { x: 10, y: 40 },
  ]);
  assert.deepEqual(characters[1].points[0], { x: 60, y: 20 });
});

test("hit tests source-image quadrilaterals", () => {
  const characters = selectableImageCharacters(ocr);

  assert.equal(hitTestImageCharacter(characters, { x: 35, y: 30 }), 0);
  assert.equal(hitTestImageCharacter(characters, { x: 80, y: 30 }), 1);
  assert.equal(hitTestImageCharacter(characters, { x: 34, y: 78 }), 2);
  assert.equal(hitTestImageCharacter(characters, { x: 5, y: 5 }), null);
});

test("maps each character half to a native-style text boundary", () => {
  const characters = selectableImageCharacters(ocr);

  assert.equal(imageTextBoundaryAtPoint(characters, { x: 20, y: 30 }), 0);
  assert.equal(imageTextBoundaryAtPoint(characters, { x: 50, y: 30 }), 1);
  assert.equal(imageTextBoundaryAtPoint(characters, { x: 95, y: 30 }), 2);
});

test("merges adjacent selected characters into one highlight per text line", () => {
  const characters = selectableImageCharacters(ocr);
  const bands = imageTextSelectionBands(characters, 0, 3);

  assert.equal(bands.length, 2);
  assert.deepEqual(bands[0], {
    blockIndex: 0,
    start: 0,
    end: 2,
    points: [
      { x: 10, y: 20 },
      { x: 110, y: 20 },
      { x: 110, y: 40 },
      { x: 10, y: 40 },
    ],
  });
  assert.deepEqual(bands[1].points, ocr.blocks[1].characters[0].points);
});

test("supports native double-click word and triple-click line ranges", () => {
  const characters = selectableImageCharacters(ocr);

  assert.deepEqual(imageTextWordRange(characters, 0, "en-US"), [0, 2]);
  assert.deepEqual(imageTextLineRange(characters, 1), [0, 2]);
  assert.deepEqual(imageTextLineRange(characters, 2), [2, 3]);
});

test("preserves OCR reading order and inserts line breaks between blocks", () => {
  const characters = selectableImageCharacters(ocr);

  assert.equal(selectedImageText(characters, 3, 0), "AB\n中");
  assert.equal(selectedImageText(characters, 1, 2), "B");
  assert.equal(selectedImageText(characters, 1, 1), "");
});
