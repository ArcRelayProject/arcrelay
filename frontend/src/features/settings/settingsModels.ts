import type {
  AppSettings,
  McpPermissions,
  NotificationPreferencesPatch,
  SoundPreferencesPatch,
} from "../../ipc/generated";

export type ReminderPreset = "essential" | "balanced" | "all";
export type PermissionPreset = "readOnly" | "daily" | "full";

type NotificationEventKey = Exclude<
  keyof AppSettings["notifications"],
  "enabled" | "onlyWhenInactive" | "showPreviews"
>;
type SoundEventKey = Exclude<
  keyof AppSettings["sounds"],
  "enabled" | "volume" | "muteDuringPrivacy" | "mutedAutomationIds"
>;

const notificationEventKeys: NotificationEventKey[] = [
  "pairingRequests",
  "transferRequests",
  "transferCompleted",
  "transferFailed",
  "remoteFileCompleted",
  "remoteFileFailed",
  "printCompleted",
  "printFailed",
  "deviceConnections",
  "workflowActionRequired",
  "workflowCompleted",
  "workflowFailed",
  "inputPermissionRequired",
  "agentNotifications",
  "updateAvailable",
];

const soundEventKeys: SoundEventKey[] = [
  "clipboardAdded",
  "clipboardReceived",
  "clipboardUsed",
  "transferRequest",
  "transferSent",
  "transferReceived",
  "transferFailed",
  "actionStarted",
  "actionSucceeded",
  "actionFailed",
  "automationStarted",
  "automationConfirmation",
  "automationSucceeded",
  "automationFailed",
  "automationInterrupted",
];

export const notificationPresetPatches: Record<ReminderPreset, NotificationPreferencesPatch> = {
  essential: {
    pairingRequests: true,
    transferRequests: true,
    transferCompleted: false,
    transferFailed: true,
    remoteFileCompleted: false,
    remoteFileFailed: true,
    printCompleted: false,
    printFailed: true,
    deviceConnections: false,
    workflowActionRequired: true,
    workflowCompleted: false,
    workflowFailed: true,
    inputPermissionRequired: true,
    agentNotifications: true,
    updateAvailable: true,
  },
  balanced: {
    pairingRequests: true,
    transferRequests: true,
    transferCompleted: true,
    transferFailed: true,
    remoteFileCompleted: true,
    remoteFileFailed: true,
    printCompleted: false,
    printFailed: true,
    deviceConnections: false,
    workflowActionRequired: true,
    workflowCompleted: false,
    workflowFailed: true,
    inputPermissionRequired: true,
    agentNotifications: true,
    updateAvailable: true,
  },
  all: Object.fromEntries(notificationEventKeys.map((key) => [key, true])) as NotificationPreferencesPatch,
};

export const soundPresetPatches: Record<ReminderPreset, SoundPreferencesPatch> = {
  essential: {
    clipboardAdded: false,
    clipboardReceived: false,
    clipboardUsed: false,
    transferRequest: true,
    transferSent: false,
    transferReceived: false,
    transferFailed: true,
    actionStarted: false,
    actionSucceeded: false,
    actionFailed: true,
    automationStarted: false,
    automationConfirmation: true,
    automationSucceeded: false,
    automationFailed: true,
    automationInterrupted: true,
  },
  balanced: {
    clipboardAdded: true,
    clipboardReceived: false,
    clipboardUsed: false,
    transferRequest: true,
    transferSent: true,
    transferReceived: true,
    transferFailed: true,
    actionStarted: false,
    actionSucceeded: true,
    actionFailed: true,
    automationStarted: false,
    automationConfirmation: true,
    automationSucceeded: false,
    automationFailed: true,
    automationInterrupted: true,
  },
  all: Object.fromEntries(soundEventKeys.map((key) => [key, true])) as SoundPreferencesPatch,
};

export function detectReminderPreset(settings: AppSettings): ReminderPreset | "custom" {
  for (const preset of ["essential", "balanced", "all"] as ReminderPreset[]) {
    const notificationsMatch = notificationEventKeys.every(
      (key) => settings.notifications[key] === notificationPresetPatches[preset][key],
    );
    const soundsMatch = soundEventKeys.every(
      (key) => settings.sounds[key] === soundPresetPatches[preset][key],
    );
    if (notificationsMatch && soundsMatch) return preset;
  }
  return "custom";
}

export const permissionPresets: Record<PermissionPreset, McpPermissions> = {
  readOnly: {
    notifications: true,
    read: true,
    manage: false,
    enable: false,
    execute: false,
    scripts: false,
  },
  daily: {
    notifications: true,
    read: true,
    manage: true,
    enable: true,
    execute: false,
    scripts: false,
  },
  full: {
    notifications: true,
    read: true,
    manage: true,
    enable: true,
    execute: true,
    scripts: true,
  },
};

export function detectPermissionPreset(permissions: McpPermissions): PermissionPreset | "custom" {
  for (const preset of ["readOnly", "daily", "full"] as PermissionPreset[]) {
    if (
      (Object.keys(permissions) as (keyof McpPermissions)[]).every(
        (key) => permissions[key] === permissionPresets[preset][key],
      )
    ) return preset;
  }
  return "custom";
}
