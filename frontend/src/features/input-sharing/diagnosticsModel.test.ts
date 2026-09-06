import assert from "node:assert/strict";
import test from "node:test";
import { diagnosticHealth, latencyChart, latencyTone } from "./diagnosticsModel.ts";

const ready = { enabled: true, connected: true, captureReady: true, injectionReady: true, activePortals: 3, p95: 8 };
const record = (timestampMs: number, latencyMicros: number | null, category = "input-e2e") => ({ timestampMs, latencyMicros, category, message: "sample" });

test("high latency is identified even with active screen edges", () => {
  assert.equal(diagnosticHealth({ ...ready, p95: 2641 }).title, "连接可用，但输入延迟偏高");
  assert.equal(latencyTone(2641), "warning");
  assert.equal(latencyTone(16), "success");
  assert.equal(latencyTone(null), "neutral");
});

test("missing samples and disconnected or disabled input cannot report healthy", () => {
  assert.equal(diagnosticHealth(ready).tone, "success");
  assert.equal(diagnosticHealth({ ...ready, p95: null }).tone, "neutral");
  assert.equal(diagnosticHealth({ ...ready, connected: false }).title, "等待远程设备连接");
  assert.equal(diagnosticHealth({ ...ready, enabled: false }).title, "跨屏输入已关闭");
  assert.equal(diagnosticHealth({ ...ready, activePortals: 0 }).title, "连接可用，仍需排列屏幕位置");
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

test("chart excludes invalid and out-of-window samples and prefers measured end-to-end latency", () => {
  const chart = latencyChart([
    record(-1, 99000), record(1001, 99000), record(10, -1000), record(20, NaN),
    record(30, null), record(40, 7000), record(50, 1000, "input-latency"),
  ], 0, 1000);
  assert.deepEqual(chart.values, [7]);
  assert.equal(chart.endToEnd, true);
  assert.equal(chart.points.length, 1);
});

test("controller-only and empty charts preserve their actual sampling state", () => {
  const fallback = latencyChart([record(10, null), record(20, 3000, "input-latency")], 0, 1000);
  assert.equal(fallback.endToEnd, false);
  assert.deepEqual(fallback.values, [3]);
  assert.equal(latencyChart([], 0, 1000).path, "");
});
