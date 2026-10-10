import test from "node:test";
import assert from "node:assert/strict";
import { get } from "svelte/store";
import {
  inlineContextMenu,
  showInlineContextMenu,
  closeInlineContextMenu,
  selectInlineMenu,
  handleInlineMenuKey,
} from "./inlineContextMenu.ts";

test("selection closes the menu and returns its action for host cleanup before execution", async () => {
  let called = false;
  const done = showInlineContextMenu(
    [
      {
        text: "Paste",
        action: () => {
          called = true;
        },
      },
    ],
    10,
    20,
  );
  selectInlineMenu(0);
  assert.equal(get(inlineContextMenu), null);
  assert.equal(called, false);
  const selected = await done;
  selected?.();
  assert.equal(called, true);
});

test("keyboard skips disabled/separator items, enters submenus, backs out, and dismisses", async () => {
  const done = showInlineContextMenu(
    [
      { text: "Unavailable", enabled: false },
      { item: "Separator" },
      { text: "Formats", items: [{ text: "Plain text" }] },
      { text: "Delete" },
    ],
    10,
    20,
  );
  const key = (key: string) =>
    handleInlineMenuKey({ key, shiftKey: false, preventDefault() {}, stopPropagation() {} });
  assert.equal(get(inlineContextMenu)?.levels[0].active, 2);
  key("ArrowRight");
  assert.equal(get(inlineContextMenu)?.levels.length, 2);
  key("ArrowLeft");
  key("ArrowDown");
  assert.equal(get(inlineContextMenu)?.levels[0].active, 3);
  key("Escape");
  assert.equal(await done, null);
});

test("opening a replacement menu cancels the old menu without executing an action", async () => {
  const old = showInlineContextMenu([{ text: "Old" }], 0, 0);
  const current = showInlineContextMenu([{ text: "New" }], 0, 0);
  assert.equal(await old, null);
  const entry = get(inlineContextMenu)?.levels[0].items[0];
  assert.ok(entry && !("item" in entry));
  assert.equal(entry.text, "New");
  closeInlineContextMenu();
  assert.equal(await current, null);
});
