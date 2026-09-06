import { iconSvgFor } from "./quickActionIcons";
import type {
  ActionType,
  ActionView,
  BootstrapState,
  NotificationView,
  QuickAction,
} from "./types";

export type Page = "actions" | "notifications" | "workflows" | "privacy" | "transfer" | "files" | "printers" | "input" | "settings";
export type SettingsTab = "connection" | "files" | "clipboard" | "screenshot" | "notifications" | "general" | "agent";
export type DraftType = ActionType["type"];

const shortcutKeysByCode: Record<string, string> = {
  Backquote: "`",
  Backslash: "\\",
  BracketLeft: "[",
  BracketRight: "]",
  Comma: ",",
  Equal: "=",
  Minus: "-",
  Period: ".",
  Quote: "'",
  Semicolon: ";",
  Slash: "/",
  Space: "Space",
};

export function shortcutKeyFromKeyboardEvent(event: Pick<KeyboardEvent, "code" | "key">) {
  if (/^Key[A-Z]$/.test(event.code)) return event.code.slice(3);
  if (/^Digit[0-9]$/.test(event.code)) return event.code.slice(5);
  return shortcutKeysByCode[event.code] ?? (event.key.length === 1 ? event.key.toUpperCase() : event.key);
}

export const actionTypeOptions: { value: DraftType; label: string }[] = [
  { value: "LaunchApp", label: "启动应用" },
  { value: "OpenPath", label: "打开文件或目录" },
  { value: "OpenUrl", label: "打开 URL" },
  { value: "Hotkey", label: "快捷键" },
  { value: "System", label: "系统操作" },
  { value: "Media", label: "媒体控制" },
  { value: "SetSystemVolume", label: "设置系统音量" },
  { value: "SetSystemMuted", label: "系统静音" },
  { value: "SetMicrophone", label: "麦克风" },
  { value: "ShellCommand", label: "Shell 命令（高级）" },
  { value: "ToggleShellCommand", label: "开关命令（高级）" },
  { value: "AppleScript", label: "AppleScript（macOS）" },
];

export type SystemOperation = Extract<ActionType, { type: "System" }>["operation"];
export type MediaOperation = Extract<ActionType, { type: "Media" }>["operation"];

export interface ActionDraft {
  revision: number;
  id: string;
  name: string;
  group: string;
  iconId: string;
  iconSvg: string;
  color: string;
  sortOrder: number;
  sourcePresetId: string | null;
  confirmBeforeRun: boolean;
  globalShortcut: string;
  type: DraftType;
  appName: string;
  appPath: string;
  path: string;
  url: string;
  command: string;
  workingDir: string;
  modifiers: string;
  key: string;
  script: string;
  startCommand: string;
  stopCommand: string;
  systemOperation: SystemOperation;
  mediaOperation: MediaOperation;
  volume: number;
  muted: boolean;
  microphoneActive: boolean;
}

export function notificationStatusLabel(notification: NotificationView) {
  switch (notification.deliveryState) {
    case "read":
      return "已读";
    case "waitingForDevice":
      return "等待上线";
    default:
      return "未读";
  }
}

export function notificationTime(timestamp: number, full = false) {
  const date = new Date(timestamp);
  const locale = typeof document === "undefined" ? "en-US" : document.documentElement.lang || "en-US";
  if (full) return date.toLocaleString(locale, { hour12: false });
  const today = new Date();
  if (date.toDateString() === today.toDateString()) {
    return date.toLocaleTimeString(locale, {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    });
  }
  return date.toLocaleDateString(locale, { month: "2-digit", day: "2-digit" });
}


export function emptyActionDraft(sortOrder = 0): ActionDraft {
  return {
    id: "",
    revision: 0,
    name: "",
    group: "常用",
    iconId: "zap",
    iconSvg: iconSvgFor("zap"),
    color: "#6366F1",
    sortOrder,
    sourcePresetId: null,
    confirmBeforeRun: false,
    globalShortcut: "",
    type: "LaunchApp",
    appName: "",
    appPath: "",
    path: "",
    url: "https://",
    command: "",
    workingDir: "",
    modifiers: "command, shift",
    key: "",
    script: "",
    startCommand: "",
    stopCommand: "",
    systemOperation: "lock_screen",
    mediaOperation: "toggle_play_pause",
    volume: 50,
    muted: true,
    microphoneActive: false,
  };
}

export function draftFromAction(action: ActionView): ActionDraft {
  const draft = emptyActionDraft();
  draft.id = action.id;
  draft.revision = action.revision;
  draft.name = action.name;
  draft.group = action.group;
  draft.iconId = action.icon_id;
  draft.iconSvg = action.icon_svg;
  draft.color = action.color;
  draft.sortOrder = action.sort_order;
  draft.sourcePresetId = action.source_preset_id;
  draft.confirmBeforeRun = action.confirm_before_run;
  draft.globalShortcut = action.global_shortcut ?? "";
  draft.type = action.action_type.type;

  switch (action.action_type.type) {
    case "LaunchApp":
      draft.appName = action.action_type.app_name;
      draft.appPath = action.action_type.app_path ?? "";
      break;
    case "OpenPath":
      draft.path = action.action_type.path;
      break;
    case "OpenUrl":
      draft.url = action.action_type.url;
      break;
    case "ShellCommand":
      draft.command = action.action_type.command;
      draft.workingDir = action.action_type.working_dir ?? "";
      break;
    case "Hotkey":
      draft.modifiers = action.action_type.modifiers.join(", ");
      draft.key = action.action_type.key;
      break;
    case "AppleScript":
      draft.script = action.action_type.script;
      break;
    case "System":
      draft.systemOperation = action.action_type.operation;
      break;
    case "Media":
      draft.mediaOperation = action.action_type.operation;
      break;
    case "SetSystemVolume":
      draft.volume = action.action_type.volume;
      break;
    case "SetSystemMuted":
      draft.muted = action.action_type.muted;
      break;
    case "SetMicrophone":
      draft.microphoneActive = action.action_type.active;
      break;
    case "ToggleShellCommand":
      draft.startCommand = action.action_type.start_command;
      draft.stopCommand = action.action_type.stop_command ?? "";
      draft.workingDir = action.action_type.working_dir ?? "";
      break;
  }
  return draft;
}

export function actionFromDraft(draft: ActionDraft): QuickAction {
  let actionType: ActionType;
  switch (draft.type) {
    case "LaunchApp":
      actionType = {
        type: "LaunchApp",
        app_name: draft.appName,
        app_path: draft.appPath || null,
      };
      break;
    case "OpenPath":
      actionType = { type: "OpenPath", path: draft.path };
      break;
    case "OpenUrl":
      actionType = { type: "OpenUrl", url: draft.url };
      break;
    case "Hotkey":
      actionType = {
        type: "Hotkey",
        modifiers: draft.modifiers
          .split(",")
          .map((item) => item.trim())
          .filter(Boolean),
        key: draft.key,
      };
      break;
    case "AppleScript":
      actionType = { type: "AppleScript", script: draft.script };
      break;
    case "System":
      actionType = { type: "System", operation: draft.systemOperation };
      break;
    case "Media":
      actionType = { type: "Media", operation: draft.mediaOperation };
      break;
    case "SetSystemVolume":
      actionType = { type: "SetSystemVolume", volume: Number(draft.volume) || 0 };
      break;
    case "SetSystemMuted":
      actionType = { type: "SetSystemMuted", muted: draft.muted };
      break;
    case "SetMicrophone":
      actionType = { type: "SetMicrophone", active: draft.microphoneActive };
      break;
    case "ToggleShellCommand":
      actionType = {
        type: "ToggleShellCommand",
        start_command: draft.startCommand,
        stop_command: draft.stopCommand || null,
        working_dir: draft.workingDir || null,
      };
      break;
    default:
      actionType = {
        type: "ShellCommand",
        command: draft.command,
        working_dir: draft.workingDir || null,
      };
  }

  return {
    id: draft.id || crypto.randomUUID(),
    revision: draft.revision,
    name: draft.name,
    icon_id: draft.iconId,
    icon_svg: draft.iconSvg,
    color: draft.color,
    group: draft.group,
    sort_order: draft.sortOrder,
    source_preset_id: draft.sourcePresetId,
    confirm_before_run: draft.confirmBeforeRun,
    global_shortcut: draft.globalShortcut.trim() || null,
    action_type: actionType,
  };
}


export function actionSummary(action: ActionView) {
  switch (action.action_type.type) {
    case "LaunchApp":
      return action.action_type.app_path || action.action_type.app_name;
    case "OpenPath":
      return action.action_type.path;
    case "OpenUrl":
      return action.action_type.url;
    case "ShellCommand":
      return action.action_type.command;
    case "Hotkey":
      return [...action.action_type.modifiers, action.action_type.key].join(" + ");
    case "AppleScript":
      return action.action_type.script;
    case "System":
      return systemOperationLabel(action.action_type.operation);
    case "Media":
      return mediaOperationLabel(action.action_type.operation);
    case "SetSystemVolume":
      return `${action.action_type.volume}%`;
    case "SetSystemMuted":
      return action.action_type.muted ? "静音" : "取消静音";
    case "SetMicrophone":
      return action.action_type.active ? "开启" : "静音";
    case "ToggleShellCommand":
      return action.action_type.start_command;
  }
}

export function actionContentTitle(action: ActionView) {
  switch (action.action_type.type) {
    case "ShellCommand":
    case "AppleScript":
      return "脚本内容";
    case "ToggleShellCommand":
      return "命令内容";
    case "Hotkey":
      return "快捷键";
    case "OpenPath":
      return "文件或目录";
    default:
      return "动作内容";
  }
}

export function shortcutLabel(action: Pick<ActionView, "global_shortcut">, commandLabel = "Super") {
  if (!action.global_shortcut) return "—";
  return action.global_shortcut.split("+")
    .map((item) => ({ super: commandLabel, control: "Ctrl", shift: "Shift", alt: "Alt" }[item.toLowerCase()]
      ?? shortcutKeysByCode[item] ?? item.replace(/^Key([A-Z])$/, "$1").replace(/^Digit([0-9])$/, "$1")))
    .join(" + ");
}

export function actionDescription(action: ActionView) {
  switch (action.action_type.type) {
    case "LaunchApp":
      return `快速打开 ${action.action_type.app_name}，无需离开当前工作流。`;
    case "OpenPath":
      return "使用系统文件管理器打开预设文件或目录。";
    case "OpenUrl":
      return "在系统默认浏览器中打开预设地址。";
    case "ShellCommand":
      return "在指定目录运行本地命令，并保留最近的输出记录。";
    case "Hotkey":
      return "向当前系统发送预设快捷键组合。";
    case "AppleScript":
      return "通过 AppleScript 自动操作本机应用。";
    case "System":
      return "调用当前操作系统的原生系统能力。";
    case "Media":
      return "控制当前系统媒体会话，不依赖特定播放器。";
    case "SetSystemVolume":
    case "SetSystemMuted":
      return "调整系统默认输出设备的音量状态。";
    case "SetMicrophone":
      return "调整系统默认麦克风的启用状态。";
    case "ToggleShellCommand":
      return "第一次运行启动进程，再次运行时安全停止。";
  }
}

export function systemOperationLabel(operation: SystemOperation) {
  const labels: Record<SystemOperation, string> = {
    lock_screen: "锁定电脑",
    sleep: "电脑睡眠",
    display_sleep: "关闭显示器",
    shutdown: "关闭电脑",
    restart: "重新启动",
    screenshot_full: "全屏截图",
    screenshot_region: "区域截图",
  };
  return labels[operation];
}

export function mediaOperationLabel(operation: MediaOperation) {
  const labels: Record<MediaOperation, string> = {
    toggle_play_pause: "播放 / 暂停",
    play: "播放",
    pause: "暂停",
    next: "下一曲",
    previous: "上一曲",
    seek_forward: "快进",
    seek_backward: "快退",
  };
  return labels[operation];
}

export function permissionText(permission: BootstrapState["inputPermission"] | undefined) {
  switch (permission) {
    case "Granted":
      return "已授权";
    case "Denied":
      return "已拒绝";
    case undefined:
      return "尚未设置";
    default:
      return "当前系统不支持";
  }
}


export function detectWindowPlatform(): "macos" | "windows" | "linux" {
  if (!("__TAURI_INTERNALS__" in window)) {
    const previewPlatform = new URLSearchParams(window.location.search).get("platform");
    if (previewPlatform === "macos" || previewPlatform === "windows" || previewPlatform === "linux") {
      return previewPlatform;
    }
  }
  const userAgent = navigator.userAgent.toLowerCase();
  if (userAgent.includes("macintosh") || userAgent.includes("mac os")) return "macos";
  if (userAgent.includes("windows")) return "windows";
  return "linux";
}


export function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
