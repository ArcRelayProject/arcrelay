// Browser-only preview adapter. Never invokes native actions, scripts, or system APIs.
import type { automationsBridge } from "./bridge.automations";
import type {
  AutomationDefinition,
  AutomationActivity,
  AutomationIssue,
  ActionType,
  AutomationEvent,
  ActivityStatus,
} from "./ipc/generated";
import {
  builtinNames,
  isActive,
  newDefinition,
  referencedVariables,
  triggerChoices,
  variables,
} from "./automation";
import { mockData } from "./bridgeMockData";
import { iconSvgFor } from "./quickActionIcons";
const definitions: AutomationDefinition[] = [];
const activities: AutomationActivity[] = [];
const listeners = new Set<(activity: AutomationActivity) => void>();
const timers = new Map<string, ReturnType<typeof setTimeout>>();
const result = () => ({ exitCode: null, stdout: "", stderr: "", error: null });
const event = (kind: string): AutomationEvent => ({
  id: crypto.randomUUID(),
  kind,
  occurredAt: new Date().toISOString(),
  variables: {},
  originAutomationId: null,
});
const scenario =
  typeof window !== "undefined"
    ? new URLSearchParams(window.location.search).get("automationScenario")
    : null;
let permissionGranted = scenario !== "permission";
function action(id: string) {
  return mockData.state.actions.find((a) => a.id === id);
}
function emit(a: AutomationActivity) {
  listeners.forEach((fn) => fn(structuredClone(a)));
}
function snapshot(
  d: AutomationDefinition,
  e: AutomationEvent,
): AutomationActivity {
  return {
    id: crypto.randomUUID(),
    automationId: d.id,
    definition: structuredClone(d),
    event: e,
    status: "queued",
    reason: null,
    createdAt: new Date().toISOString(),
    finishedAt: null,
    confirmedSteps: [],
    runConfirmed: false,
    steps: d.steps.map((step, index) => {
      const source = step.type === "quickAction" ? action(step.actionId) : null;
      return {
        index,
        step: structuredClone(step),
        action:
          step.type === "quickAction"
            ? {
                name:
                  builtinNames[step.actionId] ?? source?.name ?? "动作已删除",
                definition: source
                  ? structuredClone(source)
                  : {
                      automationBuiltin: "privacy",
                      enabled: step.actionId.endsWith("-on"),
                    },
                requiresConfirmation:
                  step.actionId.endsWith("-off") ||
                  Boolean(source?.confirm_before_run) ||
                  (source?.action_type.type === "System" &&
                    ["shutdown", "restart"].includes(
                      source.action_type.operation,
                    )),
                requiresUnlockedSession: false,
                requirements: [],
              }
            : null,
        status: "queued",
        startedAt: null,
        finishedAt: null,
        result: result(),
      };
    }),
  };
}
function finish(
  a: AutomationActivity,
  status: ActivityStatus,
  reason: string | null = null,
) {
  a.status = status;
  a.reason = reason;
  a.finishedAt = new Date().toISOString();
  emit(a);
}
function execute(a: AutomationActivity) {
  if (!isActive(a.status)) return;
  if (a.definition.runMode === "askBeforeRun" && !a.runConfirmed) {
    a.status = "awaitingConfirmation";
    a.reason = "预览：自动化设置为运行前询问";
    emit(a);
    return;
  }
  const step = a.steps.find((s) => s.status !== "succeeded");
  if (!step) {
    finish(a, "succeeded", "预览演示已完成；没有执行任何系统动作。");
    return;
  }
  if (
    step.action?.requiresConfirmation &&
    !a.confirmedSteps.includes(step.index)
  ) {
    step.status = "awaitingConfirmation";
    a.status = "awaitingConfirmation";
    a.reason = "预览：需要在本机确认此动作";
    emit(a);
    return;
  }
  a.status = "running";
  a.reason = "仅模拟运行进度，不执行真实动作。";
  step.status = "running";
  step.startedAt = new Date().toISOString();
  emit(a);
  timers.set(
    a.id,
    setTimeout(
      () => {
        timers.delete(a.id);
        if (!isActive(a.status)) return;
        step.status = "succeeded";
        step.finishedAt = new Date().toISOString();
        step.result.stdout = "预览：此步骤已模拟完成，未执行系统操作。";
        emit(a);
        execute(a);
      },
      step.step.type === "delay"
        ? Math.min(6000, Math.max(300, step.step.durationSeconds * 1000))
        : 800,
    ),
  );
}
function start(d: AutomationDefinition, e: AutomationEvent) {
  const a = snapshot(d, e);
  activities.unshift(a);
  if (
    activities.some(
      (v) => v.id !== a.id && v.automationId === d.id && isActive(v.status),
    )
  )
    finish(a, "skipped", "已有同一自动化正在运行");
  else execute(a);
  return a.id;
}
function validation(d: AutomationDefinition): AutomationIssue[] {
  const issues: AutomationIssue[] = [];
  const add = (
    message: string,
    stepIndex: number | null = null,
    code = "configuration",
    remedy = "edit",
  ) => issues.push({ message, stepIndex, code, remedy });
  if (!d.steps.length) add("请添加至少一个动作");
  if (d.trigger.type === "application" && !d.trigger.apps.length)
    add("请从已安装应用中选择应用");
  if (d.trigger.type === "hotkey" && !d.trigger.shortcut.trim())
    add("请录入全局快捷键");
  if (
    d.trigger.type === "schedule" &&
    (!d.trigger.weekdays.length || !/^\d{2}:\d{2}$/.test(d.trigger.time))
  )
    add("请选择有效运行时间和重复日期");
  for (const c of d.conditions) {
    if (c.type === "applicationRunning" && !c.app.id) add("请选择条件应用");
    if (c.type === "deviceConnected" && !c.deviceId) add("请选择条件设备");
    if (c.type === "timeRange" && !c.weekdays.length) add("请选择条件日期");
  }
  d.steps.forEach((s, i) => {
    if (
      s.type === "quickAction" &&
      !builtinNames[s.actionId] &&
      !action(s.actionId)
    )
      add("快捷动作已被删除，请重新选择动作", i, "action.missing", "actions");
    if (
      s.type === "shell" &&
      (!s.script.trim() ||
        !Number.isInteger(s.timeoutSeconds) ||
        s.timeoutSeconds < 1 ||
        s.timeoutSeconds > 3600)
    )
      add("脚本不能为空，超时应为 1–3600 秒", i);
    if (
      s.type === "delay" &&
      (!Number.isInteger(s.durationSeconds) ||
        s.durationSeconds < 0 ||
        s.durationSeconds > 86400)
    )
      add("等待时间应为 0–86400 秒", i);
    if (s.type === "notification" && !s.title.trim()) add("请填写通知标题", i);
    if (
      s.type === "quickAction" &&
      action(s.actionId)?.action_type.type === "System" &&
      !permissionGranted
    )
      add("截图缺少屏幕录制权限", i, "screen.capture", "screenPermission");
  });
  const allowed = variables(d.trigger).map((v) => v[1]);
  for (const key of referencedVariables(d))
    if (!allowed.includes(key)) add("当前触发类型不提供变量：" + key);
  return issues;
}
if (scenario) {
  const addAction = (
    id: string,
    name: string,
    type: ActionType,
    icon: string,
  ) => {
    if (!action(id))
      mockData.state.actions.push({
        id,
        name,
        action_type: type,
        icon_id: icon,
        icon_svg: iconSvgFor(icon),
        color: "#5b5ff0",
        group: "日常",
        sort_order: mockData.state.actions.length,
        source_preset_id: null,
        revision: 0,
      confirm_before_run: false,
        actionTypeLabel: name,
        isToggle: false,
        isRunning: false,
        requiresConfirmation: false,
        globalShortcutError: null,
      });
  };
  addAction(
    "automation-music",
    "暂停音乐",
    { type: "Media", operation: "pause" },
    "radio",
  );
  addAction(
    "automation-folder",
    "打开工作目录",
    { type: "OpenPath", path: "/Users/demo/Projects" },
    "folder",
  );
  addAction(
    "automation-mic",
    "麦克风静音",
    { type: "SetMicrophone", active: false },
    "radio",
  );
  addAction(
    "automation-capture",
    "全屏截图",
    { type: "System", operation: "screenshot_full" },
    "crosshair",
  );
  const meeting = newDefinition({
    type: "application",
    event: "foreground",
    apps: [
      {
        id: "com.tencent.meeting",
        name: "腾讯会议",
        path: "/Applications/TencentMeeting.app",
      },
    ],
  });
  meeting.id = "demo-meeting";
  meeting.name = "会议模式";
  meeting.conditions = [
    {
      type: "timeRange",
      weekdays: [1, 2, 3, 4, 5],
      start: "09:00",
      end: "18:00",
      timezone: "Asia/Shanghai",
    },
  ];
  meeting.steps = [
    { type: "quickAction", actionId: "automation-music" },
    { type: "delay", durationSeconds: 2 },
    { type: "quickAction", actionId: "builtin-automation-privacy-on" },
  ];
  const leave = newDefinition({
    type: "application",
    event: "exited",
    apps: meeting.trigger.type === "application" ? meeting.trigger.apps : [],
  });
  leave.name = "离开会议";
  leave.steps = [
    { type: "quickAction", actionId: "builtin-automation-privacy-off" },
  ];
  const work = newDefinition({
    type: "schedule",
    time: "09:00",
    weekdays: [1, 2, 3, 4, 5],
    timezone: "Asia/Shanghai",
    catchUp: false,
  });
  work.name = "工作日开工";
  work.steps = [{ type: "quickAction", actionId: "automation-folder" }];
  const file = newDefinition({
    type: "transfer",
    received: true,
    deviceIds: [],
    fileKinds: [],
  });
  file.name = "收到文件";
  file.steps = [
    {
      type: "notification",
      title: "收到文件",
      body: "来自 {{ event.device.name }} 的 {{ event.file.count }} 个文件",
      sendToConnectedDevices: false,
    },
    {
      type: "shell",
      shell: "system",
      script: 'open -- "{{ event.file.directory }}"',
      workingDirectory: "/Users/demo/Downloads",
      timeoutSeconds: 30,
    },
  ];
  const lock = newDefinition({ type: "system", event: "locked" });
  lock.name = "锁屏安静模式";
  lock.enabled = false;
  lock.steps = [
    { type: "quickAction", actionId: "automation-music" },
    { type: "quickAction", actionId: "automation-mic" },
  ];
  definitions.push(meeting, leave, work, file, lock);
  for (const [i, d] of definitions.entries()) {
    const e = event(
      d === file
        ? "transfer.received"
        : d === meeting
          ? "application.foreground"
          : "manual",
    );
    if (d === file)
      e.variables = {
        "event.device.name": "我的 iPhone",
        "event.file.count": "3",
        "event.file.directory": "/Users/demo/Downloads",
        "event.file.firstPath": "/Users/demo/Downloads/report.pdf",
      };
    if (d === meeting)
      e.variables = {
        "event.app.name": "腾讯会议",
        "event.app.id": "com.tencent.meeting",
        "event.app.path": "/Applications/TencentMeeting.app",
      };
    const a = snapshot(d, e);
    a.createdAt = new Date(Date.now() - (i + 1) * 60 * 60 * 1000).toISOString();
    a.finishedAt = a.createdAt;
    a.status = "succeeded";
    a.steps.forEach((s) => {
      s.status = "succeeded";
      s.startedAt = a.createdAt;
      s.finishedAt = a.createdAt;
      s.result.stdout = "预览示例";
    });
    if (d === file) {
      a.status = "failed";
      a.reason = "工作目录不存在，可能已被移动或删除";
      a.steps[1].status = "failed";
      a.steps[1].result.error = a.reason;
      a.steps[1].result.exitCode = 1;
    }
    if (d === leave) {
      a.status = "awaitingConfirmation";
      a.finishedAt = null;
      a.reason = "关闭投屏隐私前，请确认当前没有正在进行的屏幕共享。";
      a.steps[0].status = "awaitingConfirmation";
      a.steps[0].startedAt = null;
      a.steps[0].finishedAt = null;
    }
    activities.push(a);
  }
  if (scenario === "permission") {
    meeting.name = "会议截图";
    meeting.trigger = { type: "hotkey", shortcut: "CmdOrCtrl+Shift+9" };
    meeting.steps = [{ type: "quickAction", actionId: "automation-capture" }];
  }
}
export const automationMock: typeof automationsBridge = {
  async pickAutomationPath() {
    return "/Users/demo/Projects";
  },
  async previewAutomationSchedule() {
    return null;
  }, // Production preview uses the Rust scheduler, never an approximation.
  async openAutomationScreenPermission() {
    permissionGranted = true;
  },
  async listAutomations() {
    return structuredClone(definitions);
  },
  async saveAutomation(d) {
    const issues = validation(d);
    if (issues.some((i) => d.enabled || i.code === "configuration"))
      throw Error(issues[0].message);
    const saved = {
      ...structuredClone(d),
      revision: d.revision + 1,
      updatedAt: new Date().toISOString(),
    };
    const i = definitions.findIndex((v) => v.id === d.id);
    if (i < 0) definitions.push(saved);
    else definitions[i] = saved;
    return structuredClone(saved);
  },
  async setAutomationEnabled(id, enabled) {
    const d = definitions.find((d) => d.id === id);
    if (!d) throw Error("automation does not exist");
    if (enabled && validation(d).length) throw Error(validation(d)[0].message);
    d.enabled = enabled;
  },
  async deleteAutomation(id) {
    const i = definitions.findIndex((d) => d.id === id);
    if (i >= 0) definitions.splice(i, 1);
  },
  async runAutomation(id) {
    const d = definitions.find((d) => d.id === id);
    if (!d) throw Error("automation does not exist");
    return start(d, event("manual"));
  },
  async testAutomation(d, e) {
    const issues = validation(d);
    if (issues.length) throw Error(issues[0].message);
    if (referencedVariables(d).some((key) => !e || !(key in e.variables)))
      throw Error("real event variables are missing");
    return start(d, e ?? event("test"));
  },
  async automationCapabilities() {
    return [
      ...triggerChoices.map((c) => ({
        id: c.capability,
        available: true,
        reason: null,
        remedy: null,
      })),
      ...["system", "sh", "bash", "zsh"].map((s) => ({
        id: `shell.${s}`,
        available: true,
        reason: null,
        remedy: null,
      })),
      {
        id: "screen.capture",
        available: permissionGranted,
        reason: permissionGranted ? null : "需要屏幕录制权限",
        remedy: "screenPermission",
      },
    ];
  },
  async checkAutomation(d) {
    return validation(d);
  },
  async listAutomationActivities(id = null, limit = 100) {
    return structuredClone(
      activities.filter((a) => !id || a.automationId === id).slice(0, limit),
    );
  },
  async confirmAutomationActivity(id) {
    const a = activities.find((a) => a.id === id);
    if (!a || a.status !== "awaitingConfirmation") throw Error("activity was already handled");
    if (a.definition.runMode === "askBeforeRun" && !a.runConfirmed)
      a.runConfirmed = true;
    else {
      const step = a.steps.find((s) => s.status === "awaitingConfirmation");
      if (step) a.confirmedSteps.push(step.index);
    }
    execute(a);
  },
  async cancelAutomationActivity(id) {
    const a = activities.find((a) => a.id === id);
    if (!a) return;
    clearTimeout(timers.get(id));
    timers.delete(id);
    for (const s of a.steps)
      if (s.status === "running" || s.status === "awaitingConfirmation") {
        s.status = "canceled";
        s.finishedAt = new Date().toISOString();
      }
    finish(a, "canceled", "预览运行已停止；没有执行系统动作。");
  },
  async clearAutomationActivities() {
    for (let i = activities.length - 1; i >= 0; i--)
      if (!isActive(activities[i].status)) activities.splice(i, 1);
  },
  async onAutomationConfiguration(_listener) { return () => {}; },
  async onAutomationActivity(listener) {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
};
