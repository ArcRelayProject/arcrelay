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
