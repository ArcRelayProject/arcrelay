import assert from "node:assert/strict";
import test from "node:test";
import {
  clipboardDragIds,
  createClipboardDragSession,
  type ClipboardDragPhase,
  type ClipboardDragRequest,
  type PreparedClipboardDrag,
} from "./dragSession.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const press = {
  button: 0,
  buttons: 1,
  clientX: 10,
  clientY: 10,
  pointerId: 1,
  pointerType: "mouse",
};
const move = { ...press, clientX: 25 };
const ready = (token = "first"): PreparedClipboardDrag => ({ token, kind: "files", count: 2 });
const request: ClipboardDragRequest = { ids: [2, 1], mode: "auto" };
const flush = async () => {
  await Promise.resolve();
  await Promise.resolve();
};

function harness() {
  const preparations: Array<ReturnType<typeof deferred<PreparedClipboardDrag>>> = [];
  const requests: ClipboardDragRequest[] = [];
  const started: string[] = [];
  const cancelled: string[] = [];
  const phases: ClipboardDragPhase[] = [];
  const errors: unknown[] = [];
  let dropped = 0;
  const session = createClipboardDragSession({
    prepare: (value) => {
      requests.push(value);
      const pending = deferred<PreparedClipboardDrag>();
      preparations.push(pending);
      return pending.promise;
    },
    start: async (token) => {
      started.push(token);
    },
    cancel: async (token) => {
      cancelled.push(token);
    },
    state: (phase) => {
      phases.push(phase);
    },
    error: (error) => {
      errors.push(error);
    },
    dropped: () => {
      dropped++;
    },
  });
  return {
    session,
    preparations,
    requests,
    started,
    cancelled,
    phases,
    errors,
    dropped: () => dropped,
  };
}

test("multi-record payload preserves selection order, dragging an unselected record is singular", () => {
  assert.deepEqual(clipboardDragIds(3, [8, 3, 2, 8]), [8, 3, 2]);
  assert.deepEqual(clipboardDragIds(5, [8, 3, 2]), [5]);
});

test("release during preparation cancels eventual token without ever starting", async () => {
  const h = harness();
  h.session.begin(request, press);
  h.session.move(move);
  h.session.release(1);
  h.preparations[0].resolve(ready());
  await flush();
  assert.deepEqual(h.started, []);
  assert.deepEqual(h.cancelled, ["first"]);
  assert.equal(h.phases.at(-1), "idle");
  assert.equal(h.session.suppressClick(), true);
});

test("ordinary clicks and subthreshold movement perform no preparation, IO, or error reporting", () => {
  const h = harness();
  h.session.begin(request, press);
  h.session.move({ ...press, clientX: 13 });
  h.session.release(1);
  assert.deepEqual(h.requests, []);
  assert.deepEqual(h.started, []);
  assert.deepEqual(h.cancelled, []);
  assert.deepEqual(h.errors, []);
  assert.equal(h.session.suppressClick(), false);
});

test("threshold waits for preparation, command resolution and mouseup do not complete native drag", async () => {
  const h = harness();
  h.session.begin(request, press);
  h.session.move(move);
  h.preparations[0].resolve(ready());
  await flush();
  assert.deepEqual(h.started, ["first"]);
  assert.equal(h.phases.at(-1), "dragging");
  h.session.release(1);
  assert.equal(h.phases.at(-1), "dragging");
  h.session.ended({ token: "unrelated", outcome: "dropped" });
  assert.equal(h.phases.at(-1), "dragging");
  h.session.ended({ token: "first", outcome: "dropped" });
  assert.equal(h.phases.at(-1), "idle");
  assert.equal(h.dropped(), 1);
});

test("superseded and disposed preparations cancel their own tokens", async () => {
  const h = harness();
  h.session.begin(request, press, false, true);
  h.session.begin({ ids: [7], mode: "text_file" }, press, false, true);
  h.preparations[0].resolve(ready("old"));
  await flush();
  assert.deepEqual(h.cancelled, ["old"]);
  h.session.destroy();
  h.preparations[1].resolve(ready("new"));
  await flush();
  assert.deepEqual(h.cancelled, ["old", "new"]);
  assert.deepEqual(h.started, []);
});

test("sensitive record preparation waits for actual drag intent", async () => {
  const h = harness();
  h.session.begin(request, press, false, false);
  assert.equal(h.requests.length, 0);
  h.session.move(move);
  assert.equal(h.requests.length, 1);
  h.session.release(1);
  h.preparations[0].resolve(ready());
  await flush();
  assert.deepEqual(h.started, []);
});

test("text selection only starts at DOM dragstart, selecting more text does not export", async () => {
  const h = harness();
  const selected: ClipboardDragRequest = {
    ids: [4],
    mode: "plain_text",
    selection: { id: 4, text: "完整选择\nsecond line" },
  };
  h.session.begin(selected, press, true);
  h.session.move(move);
  assert.deepEqual(h.requests, []);
  assert.deepEqual(h.started, []);
  h.session.nativeDragStart();
  h.preparations[0].resolve(ready());
  await flush();
  assert.deepEqual(h.started, ["first"]);
  assert.deepEqual(h.requests, [selected]);
});

test("lost button state cancels preparation and ignores other pointers", async () => {
  const h = harness();
  h.session.begin(request, press);
  h.session.move(move);
  h.session.release(2);
  h.session.move({ ...move, buttons: 0 });
  h.preparations[0].resolve(ready());
  await flush();
  assert.deepEqual(h.started, []);
  assert.deepEqual(h.cancelled, ["first"]);
});

test("failed completion clears busy state and reports the native error", async () => {
  const h = harness();
  h.session.begin(request, press);
  h.session.move(move);
  h.preparations[0].resolve(ready());
  await flush();
  h.session.ended({ token: "first", outcome: "failed", error: "Source file disappeared" });
  assert.equal(h.phases.at(-1), "idle");
  assert.deepEqual(h.errors, ["Source file disappeared"]);
});
