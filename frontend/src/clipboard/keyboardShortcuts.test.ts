import assert from "node:assert/strict";
import test from "node:test";

import {
  resolveClipboardKeyboardAction,
  resolvePreviewKeyboardAction,
  type ClipboardKeyboardMode,
  type ClipboardShortcutEvent,
} from "./keyboardShortcuts.ts";

function key(
  code: string,
  options: Partial<ClipboardShortcutEvent> = {},
): ClipboardShortcutEvent {
  return {
    key: options.key ?? code.replace(/^Key/, "").replace(/^Digit/, ""),
    code,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    isComposing: false,
    ...options,
  };
}

function action(event: ClipboardShortcutEvent, mode: ClipboardKeyboardMode) {
  return resolveClipboardKeyboardAction(event, mode);
}

test("leaves printable search characters untouched", () => {
  for (const [code, value] of [
    ["Minus", "-"],
    ["BracketLeft", "["],
    ["Equal", "="],
    ["BracketRight", "]"],
    ["Digit1", "1"],
    ["KeyF", "f"],
  ]) {
    assert.equal(action(key(code, { key: value }), "search"), null);
  }
});

test("uses Command or Ctrl plus a fixed rank while search is focused", () => {
  assert.deepEqual(action(key("Digit1", { key: "1", metaKey: true }), "search"), { type: "pasteRank", index: 0 });
  assert.deepEqual(action(key("Digit5", { key: "5", ctrlKey: true }), "search"), { type: "pasteRank", index: 4 });
  assert.equal(action(key("Digit6", { key: "6", metaKey: true }), "search"), null);
});

test("enables one-key commands only in results mode", () => {
  assert.deepEqual(action(key("Digit1", { key: "1" }), "results"), { type: "pasteRank", index: 0 });
  assert.deepEqual(action(key("KeyC", { key: "c" }), "results"), { type: "copy" });
  assert.deepEqual(action(key("KeyF", { key: "f" }), "results"), { type: "favorite" });
  assert.deepEqual(action(key("KeyJ", { key: "j" }), "results"), { type: "move", delta: 1 });
  assert.deepEqual(action(key("Space", { key: " " }), "results"), { type: "preview" });
});

test("uses Space as a stable preview close shortcut", () => {
  assert.equal(resolvePreviewKeyboardAction(key("Space", { key: " " })), "close");
  assert.equal(resolvePreviewKeyboardAction(key("Space", { key: " ", repeat: true })), "suppressRepeat");
  assert.equal(resolvePreviewKeyboardAction(key("Space", { key: " ", metaKey: true })), null);
  assert.equal(resolvePreviewKeyboardAction(key("Space", { key: " ", isComposing: true })), null);
});

test("keeps shared navigation and paste actions available in both modes", () => {
  for (const mode of ["search", "results"] as const) {
    assert.deepEqual(action(key("ArrowDown", { key: "ArrowDown" }), mode), { type: "move", delta: 1 });
    assert.deepEqual(action(key("Enter", { key: "Enter" }), mode), { type: "paste", plainText: false });
    assert.deepEqual(action(key("Enter", { key: "Enter", shiftKey: true }), mode), { type: "paste", plainText: true });
  }
});

test("switches modes explicitly and ignores input-method composition", () => {
  assert.deepEqual(action(key("Tab", { key: "Tab" }), "search"), { type: "focusResults" });
  assert.deepEqual(action(key("Tab", { key: "Tab" }), "results"), { type: "move", delta: 1 });
  assert.deepEqual(action(key("Tab", { key: "Tab", shiftKey: true }), "results"), { type: "move", delta: -1 });
  assert.deepEqual(action(key("Slash", { key: "/" }), "results"), { type: "focusSearch" });
  assert.equal(action(key("KeyJ", { key: "j", isComposing: true }), "results"), null);
});

test("preserves standard modified editing shortcuts", () => {
  assert.equal(action(key("KeyC", { key: "c", metaKey: true }), "search"), null);
  assert.equal(action(key("KeyA", { key: "a", ctrlKey: true }), "search"), null);
  assert.deepEqual(action(key("KeyF", { key: "f", metaKey: true }), "results"), { type: "focusSearch" });
  assert.deepEqual(action(key("KeyF", { key: "f", metaKey: true }), "search"), { type: "cancelSearch" });
  assert.deepEqual(action(key("KeyF", { key: "f", ctrlKey: true }), "search"), { type: "cancelSearch" });
  assert.deepEqual(action(key("KeyK", { key: "k", metaKey: true }), "search"), { type: "actions" });
});
