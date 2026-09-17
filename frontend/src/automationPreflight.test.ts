import test from "node:test";
import assert from "node:assert/strict";
import { AutomationPreflightCache } from "./automationPreflight.ts";
import type { AutomationDefinition } from "./ipc/generated.ts";

void test("preflight batches only changed definitions and invalidates on capability or action changes", async () => {
  const cache = new AutomationPreflightCache();
  const a = { id: "a", name: "A" } as AutomationDefinition;
  const b = { id: "b", name: "B" } as AutomationDefinition;
  const batches: string[][] = [];
  const check = async (items: AutomationDefinition[]) => {
    batches.push(items.map((d) => d.id));
    return items.map(() => []);
  };
  await cache.check([a, b], [], "actions-v1", check);
  await cache.check([a, b], [], "actions-v1", check);
  await cache.check([a, { ...b, name: "changed" }], [], "actions-v1", check);
  await cache.check([a, b], [], "actions-v2", check);
  assert.deepEqual(batches, [["a", "b"], ["b"], ["a", "b"]]);
});
