import test from "node:test";
import assert from "node:assert/strict";
import { diagnosticMessage } from "./diagnosticMessages.ts";
void test("English diagnostics localize at the UI boundary while preserving embedded details", () => {
  assert.equal(
    diagnosticMessage("download the gaze model components first", "zhCn"),
    "请先下载眼动模型组件",
  );
  assert.equal(
    diagnosticMessage("failed to receive gaze models: offline", "zhCn"),
    "接收眼动模型失败: offline",
  );
  assert.equal(
    diagnosticMessage("failed to receive gaze models: offline", "enUs"),
    "failed to receive gaze models: offline",
  );
});

void test("input activity localizes known events and keeps display names intact", () => {
  assert.equal(diagnosticMessage("capture/router/enqueue sample", "zhCn"), "输入采集、路由与发送队列样本");
  assert.equal(diagnosticMessage("控制会话开始", "enUs"), "Control session started");
  assert.equal(diagnosticMessage("穿越到 我的屏幕 {name}", "enUs"), "Crossed to 我的屏幕 {name}");
  assert.equal(diagnosticMessage("书房 Mac mini 自动重连", "deDe"), "书房 Mac mini automatisch wieder verbunden");
});
