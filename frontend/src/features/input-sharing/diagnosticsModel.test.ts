import assert from "node:assert/strict";
import test from "node:test";
import { inputEventRate } from "./inputSharingUi.ts";
import { diagnosticHealth, latencyChart, latencyTone } from "./diagnosticsModel.ts";

const ready = {
  enabled: true,
  connected: true,
  captureReady: true,
  injectionReady: true,
  activePortals: 3,
  p95: 8,
};
const record = (
  timestampMs: number,
  latencyMicros: number | null,
  category = "input-receive-local",
) => ({ timestampMs, latencyMicros, category, message: "sample" });

test("high latency is identified even with active screen edges", () => {
  assert.equal(diagnosticHealth({ ...ready, p95: 2641 }).title, "本机输入处理耗时偏高");
  assert.equal(latencyTone(2641), "warning");
  assert.equal(latencyTone(16), "success");
  assert.equal(latencyTone(null), "neutral");
});

test("missing samples and disconnected or disabled input cannot report healthy", () => {
  assert.equal(diagnosticHealth(ready).tone, "neutral");
  assert.equal(diagnosticHealth({ ...ready, p95: null }).tone, "neutral");
  assert.equal(diagnosticHealth({ ...ready, connected: false }).title, "等待远程设备连接");
  assert.equal(diagnosticHealth({ ...ready, enabled: false }).title, "跨屏输入已关闭");
  assert.equal(
    diagnosticHealth({ ...ready, activePortals: 0 }).title,
    "连接可用，仍需排列屏幕位置",
  );
});

test("permission failures describe the affected device", () => {
  assert.equal(diagnosticHealth({ ...ready, captureReady: false }).title, "本机输入权限需要处理");
  assert.equal(diagnosticHealth({ ...ready, injectionReady: false }).title, "目标设备输入能力受限");
});

test("chart target shares the latency scale and timestamps determine horizontal position", () => {
  const chart = latencyChart([record(900, 2641000), record(100, 16000)], 0, 1000);
  assert.equal(chart.maximum, 3000);
  assert.equal(chart.points[0].x, 52);
  assert.equal(chart.points[1].x, 468);
  assert.equal(chart.points[0].y, chart.targetY);
  assert.ok(chart.points[1].y < chart.targetY);
  assert.ok(chart.targetY < 136);
});

test("chart excludes invalid and out-of-window samples and keeps receive and capture scopes separate", () => {
  const chart = latencyChart(
    [
      record(-1, 99000),
      record(1001, 99000),
      record(10, -1000),
      record(20, NaN),
      record(30, null),
      record(40, 7000),
      record(50, 1000, "input-capture-local"),
    ],
    0,
    1000,
  );
  assert.deepEqual(chart.values, [7]);
  assert.equal(chart.scope, "receive");
  assert.equal(chart.points.length, 1);
});

test("controller-only and empty charts preserve their actual sampling state", () => {
  const fallback = latencyChart(
    [record(10, null), record(20, 3000, "input-capture-local")],
    0,
    1000,
  );
  assert.equal(fallback.scope, "capture");
  assert.deepEqual(fallback.values, [3]);
  assert.equal(latencyChart([], 0, 1000).path, "");
});

test("legacy wall-clock and biased handler samples cannot enter local latency statistics", () => {
  const chart = latencyChart(
    [record(10, 2000, "input-e2e"), record(20, 5000, "input-latency")],
    0,
    1000,
  );
  assert.deepEqual(chart.values, []);
  assert.match(diagnosticHealth(ready).detail, /无法据此判断端到端延迟/);
});

test("processed event rate accounts for fixed sampling without mixing paths", () => {
  assert.equal(inputEventRate([record(0, 1000), record(1000, 1000)]), 128);
  assert.equal(inputEventRate([record(0, 1000), record(1000, 1000, "input-capture-local")]), null);
  assert.equal(
    inputEventRate([record(0, 1000, "input-e2e"), record(1000, 1000, "input-latency")]),
    null,
  );
});
