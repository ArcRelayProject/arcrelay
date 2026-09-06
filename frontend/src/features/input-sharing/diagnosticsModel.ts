import type { DiagnosticRecord } from "../../types";

export const INPUT_LATENCY_TARGET_MS = 16;

export function latencyTone(value: number | null) {
  return value == null ? "neutral" : value > INPUT_LATENCY_TARGET_MS ? "warning" : "success";
}

export function diagnosticHealth(state: {
  enabled: boolean;
  connected: boolean;
  captureReady: boolean;
  injectionReady: boolean;
  activePortals: number;
  p95: number | null;
}) {
  if (!state.enabled) return { tone: "neutral", title: "跨屏输入已关闭", detail: "启用跨屏输入后，查看连接与输入状态。" };
  if (!state.connected) return { tone: "neutral", title: "等待远程设备连接", detail: "请确认另一台设备已开启 ArcRelay，并连接到同一网络。" };
  if (!state.captureReady) return { tone: "warning", title: "本机输入权限需要处理", detail: "请检查本机的鼠标、键盘与辅助功能权限。" };
  if (!state.injectionReady) return { tone: "warning", title: "目标设备输入能力受限", detail: "请检查目标设备的输入权限与能力信息。" };
  if (state.p95 != null && state.p95 > INPUT_LATENCY_TARGET_MS) return { tone: "warning", title: "连接可用，但输入延迟偏高", detail: "延迟超过 16 ms 目标，建议检查网络与设备负载。" };
  if (state.activePortals === 0) return { tone: "warning", title: "连接可用，仍需排列屏幕位置", detail: "请在布局与通道中排列屏幕，建立相邻的跨屏边缘。" };
  if (state.p95 == null) return { tone: "neutral", title: "连接已就绪，等待输入样本", detail: "移动鼠标穿越屏幕边缘后，即可查看输入延迟。" };
  return { tone: "success", title: "跨屏输入运行正常", detail: "连接与输入能力可用，延迟在目标范围内。" };
}

export function latencyChart(records: DiagnosticRecord[], startMs: number, endMs: number) {
  const valid = records.filter((record) => record.timestampMs >= startMs && record.timestampMs <= endMs
    && record.latencyMicros != null && Number.isFinite(record.latencyMicros) && record.latencyMicros >= 0);
  const endToEnd = valid.filter((record) => record.category === "input-e2e");
  const source = endToEnd.length ? endToEnd : valid.filter((record) => record.category === "input-latency");
  const samples = [...source].sort((a, b) => a.timestampMs - b.timestampMs);
  const values = samples.map((record) => record.latencyMicros! / 1000);
  const peak = Math.max(20, ...values);
  const step = 10 ** Math.floor(Math.log10(peak));
  const maximum = Math.ceil(peak / step) * step;
  const y = (value: number) => 136 - value / maximum * 124;
  const points = samples.map((record) => ({
    x: (record.timestampMs - startMs) / Math.max(1, endMs - startMs) * 520,
    y: y(record.latencyMicros! / 1000),
  }));
  return {
    values, maximum, points,
    endToEnd: endToEnd.length > 0,
    targetY: y(INPUT_LATENCY_TARGET_MS),
    path: points.map((point, index) => `${index === 0 ? "M" : "L"} ${point.x.toFixed(2)} ${point.y.toFixed(2)}`).join(" "),
  };
}
