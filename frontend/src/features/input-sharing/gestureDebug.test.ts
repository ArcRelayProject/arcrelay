import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { captureActions, gestureName, gesturePhase, gestureReport, isDockSwipe } from "./gestureDebug.ts";
import type { GestureDebugBatchReport, GestureDebugReport } from "../../ipc/generated";

function batch(id = 1): GestureDebugBatchReport {
  return {
    batch: { id, label: captureActions[id - 1], mode: "intercept", elapsedMs: 10000, finished: true, stopReason: "超时停止",
      totalSamples: 101, markerSamples: 100, suppressedMarkers: 50, droppedSamples: 0, capacityDroppedSamples: 0, retainedSamples: 1, triggers: [] },
    samples: [{ batchId: id, id: 51, elapsedMs: 300, source: "tap", eventType: 30, subtype: 23, motion: 1, phase: 4,
      progress: 0.251190185546875, velocityX: 0, velocityY: 0, magnification: 0, rotation: 0, swipeMask: 0, suppressed: true }],
  };
}

function report(batches = [batch()]): GestureDebugReport { return { osVersion: "26.5.2", batches }; }

test("gesture laboratory stays implemented but is hidden from the product UI", () => {
  const page = readFileSync(new URL("./InputSharingPage.svelte", import.meta.url), "utf8");
  assert.doesNotMatch(page, /import\.meta\.env\.(DEV|PROD|MODE)/);
  assert.match(page, /const gestureLabVisible = false/);
  assert.match(page, /gestureLabVisible && new URLSearchParams/);
  assert.match(page, /\{#if gestureLabVisible\}[\s\S]*手势实验（本机）[\s\S]*\{\/if\}/);
  assert.match(page, /import GestureDebugPage/);
  assert.match(page, /手势实验（本机）/);
});

test("gesture labels never invent a detected finger count", () => {
  assert.equal(gestureName({ eventType: 30, subtype: 23, motion: 1 }), "系统水平滑动");
  assert.equal(gestureName({ eventType: 30, subtype: 23, motion: 3 }), "系统捏合／张开");
  assert.equal(gestureName({ eventType: 29, subtype: 8, motion: 0 }), "缩放");
  assert.equal(gestureName({ eventType: 30, subtype: 99, motion: 1 }), "其他原生手势");
  assert.equal(gesturePhase(8), "取消");
  assert.equal(gesturePhase(77), "未知 (77)");
  assert.equal(isDockSwipe({ eventType: 29, subtype: 6 }), false);
  assert.equal(isDockSwipe({ eventType: 30, subtype: 23 }), true);
});

test("TXT export keeps all eight numbered actions and metadata without repeated JSON keys", () => {
  const data = report(captureActions.map((_, index) => batch(index + 1)));
  const text = gestureReport(data, "8 个动作依次完成");
  assert.match(text, /arcrelay-gesture-debug-v2 \(TXT; tables=CSV\)/);
  assert.match(text, /observations,8 个动作依次完成/);
  assert.match(text, /不代表动作成功/);
  for (let id = 1; id <= 8; id++) {
    assert.ok(text.includes(`${id},${captureActions[id - 1]},intercept,10000,1,101,1,100,50,0,0,0,超时停止`));
    assert.ok(text.includes(`${id},51,300,t,30,23,1,4,0.251190186,0,0,0,0,0,1`));
  }
  assert.doesNotMatch(text, /"elapsedMs"|"velocityX"|\[triggers\]/);
  assert.ok(text.length < JSON.stringify(data, null, 2).length / 2);
});

test("CSV sections escape quotes, commas and newlines in observations and action labels", () => {
  const data = report();
  data.batches[0].batch.label = '四指,向上"测试"';
  const text = gestureReport(data, '第一行\r\n第二行,"未触发"');
  assert.ok(text.includes('1,"四指,向上""测试""",intercept'));
  assert.ok(text.includes('observations,"第一行\r\n第二行,""未触发"""'));
});

test("unrecognized native fields retain large integers and tiny floats without axis claims", () => {
  const data = report();
  Object.assign(data.batches[0].samples[0], { eventType: 29, subtype: 6, motion: 3225419776, progress: 1.401298464324817e-45, swipeMask: 2147483648 });
  const text = gestureReport(data, "");
  assert.match(text, /29,6,3225419776,4,1\.40129846e-45,0,0,0,0,2147483648,1/);
  assert.match(text, /仅 type=30,subtype=23/);
});

test("reports actual injection requests separately and preserves zero-event actions and losses", () => {
  const data = report();
  data.batches[0].samples = [];
  Object.assign(data.batches[0].batch, { retainedSamples: 0, totalSamples: 3, markerSamples: 0, suppressedMarkers: 0, droppedSamples: 2, capacityDroppedSamples: 1 });
  data.batches[0].batch.triggers.push({ elapsedMs: 1500, request: { action: "swipeUp", amount: 0.5, durationMs: 500, delayMs: 0, reverse: false, cancelAtEnd: true } });
  const text = gestureReport(data, "未触发");
  assert.match(text, /1,三指向右,intercept,10000,1,3,0,0,0,2,1,0,/);
  assert.match(text, /\[triggers\]\nbatch,ms,action,durationMs,delayMs,amount,reverse,cancelAtEnd\n1,1500,swipeUp,500,0,0.5,0,1/);
  assert.equal(text.split("[events]\n")[1].split("\n").length, 1);
});

test("export UI requests full stopped report instead of copying preview samples", () => {
  const page = readFileSync(new URL("./GestureDebugPage.svelte", import.meta.url), "utf8");
  assert.match(page, /invoke\("get_gesture_debug_report"\)/);
  assert.match(page, /复制 TXT 报告/);
  assert.match(page, /下载 \.txt/);
  assert.doesNotMatch(page, /JSON 报告|gestureReport\(snapshot/);
  assert.match(page, /suppress, label: testLabel/);
});
