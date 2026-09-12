import { translate, t, localeFor } from "./localization.ts";
import type { LanguagePreference } from "./types";
import type {
  AutomationDefinition,
  AutomationTrigger,
  AutomationStep,
  AutomationCondition,
  ApplicationIdentity,
  ActivityStatus,
  ActionView,
  AutomationActivity,
  PresenceEvent,
} from "./ipc/generated";
export const weekdays = [1, 2, 3, 4, 5, 6, 7];
export const dayNames = ["", "一", "二", "三", "四", "五", "六", "日"];
export const statusLabels: Record<ActivityStatus, string> = {
  awaitingConfirmation: "等待确认",
  skipped: "未运行",
  queued: "排队中",
  running: "运行中",
  succeeded: "成功",
  failed: "失败",
  canceled: "已取消",
  interrupted: "已中断",
};
export const applicationLabels = {
  started: "启动",
  exited: "退出",
  foreground: "成为前台",
  background: "离开前台",
};
export const systemLabels = {
  locked: "系统锁定",
  unlocked: "系统解锁",
  sleeping: "进入睡眠",
  resumed: "从睡眠恢复",
  started: "ArcRelay 启动",
};
export const presenceLabels: Record<PresenceEvent, string> = {
  ownerPresent: "本人在场",
  absent: "无人",
  unknownPresent: "陌生人在场",
  multiplePeople: "多人在场",
  uncertain: "无法确认身份",
};
export function newDefinition(
  trigger: AutomationTrigger,
): AutomationDefinition {
  const now = new Date().toISOString();
  return {
    id: crypto.randomUUID(),
    name: "",
    enabled: true,
    trigger,
    conditions: [],
    steps: [],
    runMode: "automatic",
    revision: 0,
    createdAt: now,
    updatedAt: now,
    legacyId: null,
  };
}
export function triggerSummary(trigger: AutomationTrigger, language: LanguagePreference = "zhCn") {
  switch (trigger.type) {
    case "manual":
      return translate("手动运行", language);
    case "schedule":
      return `${weekSummary(trigger.weekdays, language)} ${trigger.time}`;
    case "application":
      return t("当 {apps} {event}时", language, { apps: trigger.apps.length ? (language === "zhCn" ? trigger.apps.map(a => a.name).join(" 或 ") : new Intl.ListFormat(localeFor(language), { type: "disjunction" }).format(trigger.apps.map(a => a.name))) : translate("所选应用", language), event: translate(applicationLabels[trigger.event], language) });
    case "system":
      return t("当{event}时", language, { event: translate(systemLabels[trigger.event], language) });
    case "device":
      return t("当{devices}{event}时", language, { devices: translate(trigger.deviceIds.length ? "所选设备" : "任意已配对设备", language), event: translate(trigger.connected ? "连接" : "断开", language) });
    case "transfer":
      return t("当文件{event}完成时", language, { event: translate(trigger.received ? "接收" : "发送", language) });
    case "hotkey":
      return t("当按下 {shortcut} 时", language, { shortcut: trigger.shortcut || translate("快捷键", language) });
    case "presence":
      return t("当{state}时", language, { state: translate(presenceLabels[trigger.state], language) });
  }
}
export function conditionSummary(condition: AutomationCondition, language: LanguagePreference = "zhCn") {
  switch (condition.type) {
    case "timeRange":
      return `${weekSummary(condition.weekdays, language)} ${condition.start}–${condition.end <= condition.start ? translate("次日 ", language) : ""}${condition.end}`;
    case "applicationRunning":
      return t("{app} {state}", language, { app: condition.app.name, state: translate(condition.running ? "正在运行" : "未运行", language) });
    case "deviceConnected":
      return translate(condition.connected ? "所选设备已连接" : "所选设备未连接", language);
    case "presence":
      return t("在场状态为{state}", language, { state: translate(presenceLabels[condition.state], language) });
  }
}
export function stepSummary(step: AutomationStep, actions: ActionView[], language: LanguagePreference = "zhCn") {
  switch (step.type) {
    case "quickAction":
      return (
        (builtinNames[step.actionId] ? translate(builtinNames[step.actionId], language) : undefined) ??
        actions.find((a) => a.id === step.actionId)?.name ??
        translate("快捷动作已被删除", language)
      );
    case "shell":
      return translate("运行 Shell 脚本", language);
    case "delay":
      return step.durationSeconds >= 60 && step.durationSeconds % 60 === 0 ? t("等待 {count} 分钟", language, { count: step.durationSeconds / 60 }) : t("等待 {count} 秒", language, { count: step.durationSeconds });
    case "notification":
      return t("显示“{title}”", language, { title: step.title });
  }
}
export function generatedName(d: AutomationDefinition, actions: ActionView[], language: LanguagePreference = "zhCn") {
  return `${triggerSummary(d.trigger, language)} · ${d.steps[0] ? stepSummary(d.steps[0], actions, language) : translate("新自动化", language)}`;
}
export function variables(trigger: AutomationTrigger): [string, string][] {
  if (trigger.type === "application")
    return [
      ["应用名称", "event.app.name"],
      ["应用路径", "event.app.path"],
      ["应用 ID", "event.app.id"],
    ];
  if (trigger.type === "device")
    return [
      ["设备名称", "event.device.name"],
      ["设备 ID", "event.device.id"],
    ];
  if (trigger.type === "transfer")
    return trigger.received
      ? [
          ["发送设备名称", "event.device.name"],
          ["接收目录", "event.file.directory"],
          ["文件数量", "event.file.count"],
          ["第一个文件路径", "event.file.firstPath"],
        ]
      : [
          ["接收设备名称", "event.device.name"],
          ["文件数量", "event.file.count"],
        ];
  if (trigger.type === "schedule")
    return [
      ["计划时间", "event.schedule.plannedAt"],
      ["实际触发时间", "event.schedule.actualAt"],
    ];
  if (trigger.type === "presence")
    return [
      ["在场状态", "event.presence.state"],
      ["人脸数量", "event.presence.faceCount"],
      ["本人相似度", "event.presence.ownerSimilarity"],
    ];
  return [];
}
export function identity(app: {
  name: string;
  path: string;
  identifier: string | null;
}): ApplicationIdentity {
  return {
    id: app.identifier || app.path.replaceAll("\\", "/").toLowerCase(),
    name: app.name,
    path: app.path,
  };
}
export const triggerChoices: {
  category: string;
  label: string;
  capability: string;
  trigger: () => AutomationTrigger;
}[] = [
  ...(["foreground", "started", "exited", "background"] as const).map(
    (event) => ({
      category: "应用",
      label: `应用${applicationLabels[event]}`,
      capability: `application.${event}`,
      trigger: () => ({ type: "application" as const, event, apps: [] }),
    }),
  ),
  ...(["locked", "unlocked", "sleeping", "resumed", "started"] as const).map(
    (event) => ({
      category: "系统",
      label: systemLabels[event],
      capability: `system.${event}`,
      trigger: () => ({ type: "system" as const, event }),
    }),
  ),
  ...(["ownerPresent", "absent", "unknownPresent", "multiplePeople", "uncertain"] as const).map(
    (state) => ({
      category: "在场",
      label: presenceLabels[state],
      capability: `presence.${state}`,
      trigger: () => ({ type: "presence" as const, state }),
    }),
  ),
  {
    category: "时间",
    label: "到达指定时间",
    capability: "schedule",
    trigger: () => ({
      type: "schedule",
      time: "09:00",
      weekdays: [1, 2, 3, 4, 5],
      timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      catchUp: false,
    }),
  },
  ...[true, false].map((connected) => ({
    category: "ArcRelay",
    label: connected ? "设备连接" : "设备断开",
    capability: `device.${connected ? "connected" : "disconnected"}`,
    trigger: () => ({ type: "device" as const, connected, deviceIds: [] }),
  })),
  ...[true, false].map((received) => ({
    category: "ArcRelay",
    label: received ? "文件接收完成" : "文件发送完成",
    capability: `transfer.${received ? "received" : "sent"}`,
    trigger: () => ({
      type: "transfer" as const,
      received,
      deviceIds: [],
      fileKinds: [],
    }),
  })),
  {
    category: "手动",
    label: "全局快捷键",
    capability: "hotkey",
    trigger: () => ({ type: "hotkey", shortcut: "" }),
  },
  {
    category: "手动",
    label: "手动运行",
    capability: "manual",
    trigger: () => ({ type: "manual" }),
  },
];

export const builtinNames: Record<string, string> = {
  "builtin-automation-privacy-on": "开启投屏隐私",
  "builtin-automation-privacy-off": "关闭投屏隐私",
};
export function weekSummary(days: number[], language: LanguagePreference = "zhCn") {
  const sorted = [...new Set(days)].sort((a, b) => a - b).join(",");
  if (sorted === "1,2,3,4,5") return translate("工作日", language);
  if (sorted === "1,2,3,4,5,6,7") return translate("每天", language);
  if (sorted === "6,7") return translate("周末", language);
  return days.length ? t("每周{days}", language, { days: [...days].sort((a, b) => a - b).map(day => language === "zhCn" ? dayNames[day] : weekdayName(day, language)).join(language === "zhCn" ? "、" : ", ") }) : translate("未选择日期", language);
}
export function isActive(status: ActivityStatus) {
  return ["queued", "running", "awaitingConfirmation"].includes(status);
}
export function formatDate(value: string | null, timezone?: string) {
  if (!value) return "尚未运行";
  try {
    return new Intl.DateTimeFormat(typeof document === "undefined" ? "en-US" : document.documentElement.lang || "en-US", {
      month: "numeric",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      ...(timezone ? { timeZone: timezone } : {}),
    }).format(new Date(value));
  } catch {
    return value;
  }
}
export function ruleSummary(d: AutomationDefinition, actions: ActionView[], language: LanguagePreference = "zhCn") {
  return `${triggerSummary(d.trigger, language)} → ${d.steps.map((s) => stepSummary(s, actions, language)).join(" → ") || translate("尚未添加动作", language)}`;
}
export function matchesAutomation(
  d: AutomationDefinition,
  query: string,
  actions: ActionView[],
  language: LanguagePreference = "zhCn",
) {
  return `${d.name} ${ruleSummary(d, actions, language)} ${d.conditions.map(c => conditionSummary(c, language)).join(" ")}`
    .toLocaleLowerCase()
    .includes(query.trim().toLocaleLowerCase());
}
export function copyDefinition(d: AutomationDefinition, language: LanguagePreference = "zhCn"): AutomationDefinition {
  return {
    ...structuredClone(d),
    id: crypto.randomUUID(),
    name: t("{name} 副本", language, { name: d.name }),
    revision: 0,
    enabled: false,
    legacyId: null,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };
}
export function referencedVariables(d: AutomationDefinition): string[] {
  const text = d.steps
    .map((s) =>
      s.type === "shell"
        ? s.script
        : s.type === "notification"
          ? `${s.title}\n${s.body}`
          : "",
    )
    .join("\n");
  return [
    ...new Set([...text.matchAll(/\{\{\s*([\w.]+)\s*\}\}/g)].map((m) => m[1])),
  ];
}
export function samplesFor(
  d: AutomationDefinition,
  activities: AutomationActivity[],
) {
  const keys = referencedVariables(d);
  const prefix =
    d.trigger.type === "application"
      ? `application.${d.trigger.event}`
      : d.trigger.type === "device"
        ? `device.${d.trigger.connected ? "connected" : "disconnected"}`
        : d.trigger.type === "transfer"
          ? `transfer.${d.trigger.received ? "received" : "sent"}`
          : d.trigger.type === "system"
            ? `system.${d.trigger.event}`
            : d.trigger.type;
  return activities.filter(
    (a) =>
      a.event.kind === prefix && keys.every((key) => key in a.event.variables),
  );
}

export function weekdayName(day: number, language: LanguagePreference, width: "short" | "long" = "short") {
  return new Intl.DateTimeFormat(localeFor(language), { weekday: width, timeZone: "UTC" }).format(new Date(Date.UTC(2024, 0, day)));
}
