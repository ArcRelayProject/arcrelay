import test from "node:test";
import assert from "node:assert/strict";
import { dismissibleDropdown } from "./dismissibleDropdown.ts";

function fixture(open = true) {
  const document = new EventTarget();
  let closed = 0;
  let focused = 0;
  const node = {
    ownerDocument: document,
    querySelector: () => ({ focus: () => focused++ }),
  } as unknown as HTMLElement;
  const action = dismissibleDropdown(node, { open, close: () => closed++ });
  const send = (type: string, inside = false, key = "") => {
    const event = new Event(type, { cancelable: true, bubbles: true });
    Object.defineProperties(event, {
      composedPath: { value: () => inside ? [node, document] : [document] },
      key: { value: key },
      isComposing: { value: false },
    });
    document.dispatchEvent(event);
    return event;
  };
  return { action, send, node, counts: () => ({ closed, focused }), reopen: () => action.update({ open: true, close: () => closed++ }) };
}

test("inside pointer and focus preserve the popup; outside pointer closes without consuming the click", () => {
  const f = fixture();
  f.send("pointerdown", true);
  f.send("focusin", true);
  assert.equal(f.counts().closed, 0);
  const event = f.send("pointerdown");
  assert.deepEqual(f.counts(), { closed: 1, focused: 0 });
  assert.equal(event.defaultPrevented, false);
  f.send("focusin");
  assert.equal(f.counts().closed, 1);
  f.action.destroy();
});

test("Tab focus leaving a popup dismisses it without stealing focus", () => {
  const f = fixture();
  f.send("focusin");
  assert.deepEqual(f.counts(), { closed: 1, focused: 0 });
  f.action.destroy();
});

test("Escape closes and returns focus to trigger without invoking the page Escape action", () => {
  const f = fixture();
  assert.equal(f.send("keydown", true, "Enter").defaultPrevented, false);
  assert.equal(f.send("keydown", true, "Escape").defaultPrevented, true);
  assert.deepEqual(f.counts(), { closed: 1, focused: 1 });
  assert.equal(f.send("keydown", false, "Escape").defaultPrevented, false);
  f.action.destroy();
});

test("updates and teardown do not leave a closed or unmounted popup listening", () => {
  const f = fixture(false);
  f.send("pointerdown");
  assert.equal(f.counts().closed, 0);
  f.reopen();
  f.send("pointerdown");
  assert.equal(f.counts().closed, 1);
  f.reopen();
  f.action.destroy();
  f.send("pointerdown");
  assert.equal(f.counts().closed, 1);
});
