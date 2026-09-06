import assert from "node:assert/strict";
import test from "node:test";
import {
  newDefinition,
  triggerSummary,
  variables,
  generatedName,
  identity,
  triggerChoices,
  weekSummary,
  conditionSummary,
  copyDefinition,
  matchesAutomation,
  referencedVariables,
  samplesFor,
  isActive,
  stepSummary,
} from "./automation.ts";
import type { AutomationActivity } from "./ipc/generated.ts";

test("multiple application targets use stable identities and readable summaries", () => {
  const apps = [
    identity({
      name: "Teams",
      path: "/Applications/Teams.app",
      identifier: "com.microsoft.teams2",
    }),
    identity({
      name: "Zoom",
      path: "/Applications/zoom.us.app",
      identifier: "us.zoom.xos",
    }),
  ];
  const d = newDefinition({ type: "application", event: "foreground", apps });
  assert.equal(d.trigger.type, "application");
  assert.equal(apps[0].id, "com.microsoft.teams2");
  assert.match(triggerSummary(d.trigger), /Teams 或 Zoom/);
  assert.match(generatedName(d, []), /新自动化/);
});

test("weekday and overnight summaries explain the actual AND condition", () => {
  assert.equal(weekSummary([5, 4, 3, 2, 1]), "工作日");
  assert.equal(weekSummary([6, 7]), "周末");
  assert.equal(weekSummary([]), "未选择日期");
  assert.match(
    conditionSummary({
      type: "timeRange",
      weekdays: [1],
      start: "22:00",
      end: "06:00",
      timezone: "Asia/Shanghai",
    }),
    /次日 06:00/,
  );
});

test("duplicates are independent, disabled drafts with a fresh identity", () => {
  const original = newDefinition({ type: "manual" });
  original.name = "会议模式";
  original.revision = 9;
  original.legacyId = "old-id";
  original.steps = [{ type: "delay", durationSeconds: 2 }];
  const copy = copyDefinition(original);
  assert.notEqual(copy.id, original.id);
  assert.equal(copy.enabled, false);
  assert.equal(copy.revision, 0);
  assert.equal(copy.legacyId, null);
  assert.equal(copy.name, "会议模式 副本");
  copy.steps.push({ type: "delay", durationSeconds: 5 });
  assert.equal(original.steps.length, 1);
});

test("search includes actions and conditions without changing definitions", () => {
  const d = newDefinition({ type: "manual" });
  d.name = "Meetings";
  d.steps = [
    { type: "quickAction", actionId: "builtin-automation-privacy-on" },
  ];
  assert.equal(matchesAutomation(d, " meetings ", []), true);
  assert.equal(matchesAutomation(d, "投屏隐私", []), true);
  assert.equal(matchesAutomation(d, "不存在", []), false);
  assert.equal(stepSummary(d.steps[0], []), "开启投屏隐私");
});

test("real samples must match event kind and contain every referenced variable", () => {
  const d = newDefinition({
    type: "transfer",
    received: true,
    deviceIds: [],
    fileKinds: [],
  });
  d.steps = [
    {
      type: "notification",
      title: "{{ event.device.name }}",
      body: "{{event.file.directory}} {{ event.device.name }}",
      sendToConnectedDevices: false,
    },
  ];
  assert.deepEqual(referencedVariables(d), [
    "event.device.name",
    "event.file.directory",
  ]);
  const sample = (
    kind: string,
    values: Record<string, string>,
  ): AutomationActivity => ({
    id: crypto.randomUUID(),
    automationId: d.id,
    definition: d,
    event: {
      id: crypto.randomUUID(),
      kind,
      variables: values,
      occurredAt: d.createdAt,
      originAutomationId: null,
    },
    status: "succeeded",
    reason: null,
    createdAt: d.createdAt,
    finishedAt: d.createdAt,
    steps: [],
    confirmedSteps: [],
    runConfirmed: false,
  });
  const good = sample("transfer.received", {
    "event.device.name": "Phone",
    "event.file.directory": "/Downloads",
  });
  const missing = sample("transfer.received", { "event.device.name": "Phone" });
  const wrong = sample("transfer.sent", good.event.variables);
  assert.deepEqual(samplesFor(d, [wrong, missing, good]), [good]);
});

test("only pending activities block duplicate running or survive completed cleanup", () => {
  for (const state of ["queued", "running", "awaitingConfirmation"] as const)
    assert.equal(isActive(state), true);
  for (const state of [
    "succeeded",
    "failed",
    "skipped",
    "canceled",
    "interrupted",
  ] as const)
    assert.equal(isActive(state), false);
});

test("only variables available for the selected trigger are offered", () => {
  const keys = (received: boolean) =>
    variables({ type: "transfer", received, deviceIds: [], fileKinds: [] }).map(
      (v) => v[1],
    );
  assert.ok(keys(true).includes("event.file.directory"));
  assert.ok(!keys(false).includes("event.file.directory"));
  assert.deepEqual(variables({ type: "manual" }), []);
  assert.equal(new Set(triggerChoices.map((c) => c.category)).size, 5);
});
