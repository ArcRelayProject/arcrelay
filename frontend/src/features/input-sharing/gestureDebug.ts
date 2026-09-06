import type { GestureDebugReport, GestureDebugSample } from "../../ipc/generated";

export const captureActions = ["三指向右", "三指向左", "三指向上", "三指向下", "四指向右", "四指向左", "四指向上", "四指向下"];

export function isDockSwipe(sample: Pick<GestureDebugSample, "eventType" | "subtype">): boolean {
  return sample.eventType === 30 && sample.subtype === 23;
}

export function gestureName(sample: Pick<GestureDebugSample, "eventType" | "subtype" | "motion">): string {
  if (sample.eventType === 30 && sample.subtype === 23) {
    return sample.motion === 1 ? "系统水平滑动" : sample.motion === 2 ? "系统垂直滑动" : sample.motion === 3 ? "系统捏合／张开" : "DockSwipe（未知轴）";
  }
  if (sample.eventType === 29) {
    if (sample.subtype === 0) return "手势伴随标记";
    if (sample.subtype === 5) return "旋转";
    if (sample.subtype === 8) return "缩放";
    if (sample.subtype === 22) return "智能缩放";
  }
  return "其他原生手势";
}

export function gesturePhase(phase: number): string {
  return ({ 0: "—", 1: "开始", 2: "变化", 4: "结束", 8: "取消", 128: "可能开始" } as Record<number, string>)[phase] ?? `未知 (${phase})`;
}

// Nine significant digits retain float32 precision, including tiny encoded
// values. Do not toFixed() unknown native fields into misleading zeroes.
function cell(value: string | number | boolean): string {
  const text = typeof value === "number"
    ? Number.isFinite(value) ? String(Number.isInteger(value) ? value : Number(value.toPrecision(9))) : ""
    : typeof value === "boolean" ? value ? "1" : "0" : value;
  return /[",\r\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
}

function row(...values: (string | number | boolean)[]): string {
  return values.map(cell).join(",");
}

export function gestureReport(report: GestureDebugReport, notes: string): string {
  const lines = [
    "arcrelay-gesture-debug-v2 (TXT; tables=CSV)",
    row("os", report.osVersion),
    row("observations", notes),
    "说明: label/手指数为人工填写；一个 batch 是一次点击开始，并非自动识别的手势。",
    "t=tap(可能含合成); p=posted(不代表动作成功); ms=距该试次开始; blocked=已拦截。",
    "i/d列为原生字段编号；仅 type=30,subtype=23 可将 i123/i132/d124/d129/d130 解释为轴/阶段/进度/速度。",
    "标记29/0只统计；busyLost=回调争锁丢失; capacityLost=容量拒收; overwritten=0(不覆盖); 任一丢失>0不保证序列完整。",
    "整数原样，浮点最多9位有效数字；triggers只列实际请求，不使用页面当前选项推断已执行。",
    "",
    "[batches]",
    "batch,label,mode,ms,finished,total,retained,markers,blockedMarkers,busyLost,capacityLost,overwritten,stop",
  ];
  for (const { batch } of report.batches) {
    lines.push(row(batch.id, batch.label, batch.mode, batch.elapsedMs, batch.finished, batch.totalSamples,
      batch.retainedSamples, batch.markerSamples, batch.suppressedMarkers, batch.droppedSamples,
      batch.capacityDroppedSamples, 0, batch.stopReason));
  }
  const triggers = report.batches.flatMap(({ batch }) => batch.triggers.map((trigger) => ({ batch: batch.id, ...trigger })));
  if (triggers.length) {
    lines.push("", "[triggers]", "batch,ms,action,durationMs,delayMs,amount,reverse,cancelAtEnd");
    for (const { batch, elapsedMs, request } of triggers) {
      lines.push(row(batch, elapsedMs, request.action, request.durationMs, request.delayMs, request.amount, request.reverse, request.cancelAtEnd));
    }
  }
  lines.push("", "[events]", "batch,id,ms,src,type,subtype,i123,i132,d124,d129,d130,d113,d114,i115,blocked");
  for (const { samples } of report.batches) {
    for (const sample of samples) {
      lines.push(row(sample.batchId, sample.id, sample.elapsedMs, sample.source === "tap" ? "t" : sample.source === "posted" ? "p" : sample.source,
        sample.eventType, sample.subtype, sample.motion, sample.phase, sample.progress, sample.velocityX,
        sample.velocityY, sample.magnification, sample.rotation, sample.swipeMask, sample.suppressed));
    }
  }
  return lines.join("\n");
}
