import assert from "node:assert/strict";
import test from "node:test";
import { SubscriptionScope, observeSnapshot, createInvalidationLoader, type Stop } from "./subscriptions.ts";
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>((r) => resolve = r); return { promise, resolve }; }
test("unmount while listener registration is pending releases the eventual listener", async () => {
  const registration = deferred<Stop>(); const scope = new SubscriptionScope(); let stopped = 0;
  const pending = scope.add(registration.promise); scope.dispose(); registration.resolve(() => stopped++); await pending;
  assert.equal(stopped, 1);
});
test("an initial query cannot overwrite a newer event", async () => {
  const initial = deferred<{revision: number}>(); const scope = new SubscriptionScope(); const applied: number[] = [];
  let event!: (value: {revision: number}) => void;
  const pending = observeSnapshot(scope, async (accept) => { event=accept; return () => {}; }, () => initial.promise, (value) => applied.push(value.revision));
  await Promise.resolve(); event({revision: 2}); initial.resolve({revision: 1}); await pending;
  event({revision: 2}); scope.dispose(); event({revision: 3}); assert.deepEqual(applied, [2]);
});
test("invalidation during a query causes one follow-up query", async () => {
  const first=deferred<void>(); let calls=0;
  const loader=createInvalidationLoader(async () => { calls++; if(calls===1) await first.promise; }, (error) => assert.fail(String(error)));
  const pending=loader.refresh(); void loader.refresh(); void loader.refresh(); first.resolve(); await pending;
  assert.equal(calls, 2); loader.dispose(); await loader.refresh(); assert.equal(calls, 2);
});
