import assert from "node:assert/strict";
import test from "node:test";
import { containedSlice, containingSlice, textSliceParts, type TextSlice } from "./textSegments.ts";
const alice: TextSlice = { id: "word-11-16", level: "word", start: 11, end: 16, text: "Alice" };
const phrase: TextSlice = { id: "phrase-11-24", level: "phrase", start: 11, end: 24, text: "Alice Johnson" };
test("navigates between Rust-supplied parent and child ranges", () => {
  assert.equal(containingSlice([phrase], alice), phrase);
  assert.equal(containedSlice([alice], phrase, alice.start), alice);
});
test("reconstructs untouched text around UTF-16 selectable ranges", () => {
  const text = "👋 Alpha, beta";
  const word: TextSlice = { id: "word-3-8", level: "word", start: 3, end: 8, text: "Alpha" };
  assert.equal(textSliceParts(text, [word]).map((part) => part.kind === "slice" ? part.slice.text : part.text).join(""), text);
});
