import assert from "node:assert/strict";
import test from "node:test";
import { captureActionShortcut } from "./actionShortcutCapture.ts";

const keyEvent = (overrides: Partial<KeyboardEvent> = {}) => ({
  key: "l", code: "KeyL", ctrlKey: false, metaKey: false, altKey: false,
  shiftKey: false, repeat: false, isComposing: false, ...overrides,
});

test("records Control and Command independently with all held modifiers", () => {
  assert.deepEqual(captureActionShortcut(keyEvent({ ctrlKey: true, metaKey: true, altKey: true, shiftKey: true })),
    { type: "shortcut", value: "Ctrl+Super+Alt+Shift+KeyL" });
});

test("preserves physical keys for shifted punctuation, non-Latin input and numpad plus", () => {
  for (const [key, code, expected] of [["+", "Equal", "Equal"], ["ж", "Semicolon", "Semicolon"], ["+", "NumpadAdd", "NumpadAdd"]]) {
    assert.deepEqual(captureActionShortcut(keyEvent({ key, code, ctrlKey: true, shiftKey: true })),
      { type: "shortcut", value: `Ctrl+Shift+${expected}` });
  }
});

test("keeps dialog navigation available and distinguishes clearing from modified deletion keys", () => {
  for (const key of ["Tab", "Escape"]) assert.deepEqual(captureActionShortcut(keyEvent({ key })), { type: "navigate" });
  assert.deepEqual(captureActionShortcut(keyEvent({ key: "Tab", shiftKey: true })), { type: "navigate" });
  for (const key of ["Backspace", "Delete"]) {
    assert.deepEqual(captureActionShortcut(keyEvent({ key, code: key })), { type: "clear" });
    assert.deepEqual(captureActionShortcut(keyEvent({ key, code: key, ctrlKey: true })), { type: "shortcut", value: `Ctrl+${key}` });
  }
  assert.deepEqual(captureActionShortcut(keyEvent({ key: "Escape", code: "Escape", ctrlKey: true })), { type: "shortcut", value: "Ctrl+Escape" });
});

test("rejects text and Shift-only input, without accepting an unknown key", () => {
  assert.equal(captureActionShortcut(keyEvent()).type, "error");
  assert.equal(captureActionShortcut(keyEvent({ shiftKey: true })).type, "error");
  assert.equal(captureActionShortcut(keyEvent({ key: "Unidentified", code: "Unidentified", ctrlKey: true })).type, "error");
});

test("ignores modifier-only presses, held-key repeats and IME composition", () => {
  for (const key of ["Meta", "Control", "Alt", "Shift", "AltGraph"]) {
    assert.deepEqual(captureActionShortcut(keyEvent({ key })), { type: "ignore" });
  }
  assert.deepEqual(captureActionShortcut(keyEvent({ ctrlKey: true, repeat: true })), { type: "ignore" });
  assert.deepEqual(captureActionShortcut(keyEvent({ isComposing: true })), { type: "ignore" });
});
