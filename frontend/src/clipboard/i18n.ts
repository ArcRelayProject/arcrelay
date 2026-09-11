import { defaultSoundPreferences } from "../soundDefaults";
import { loadBrowserLanguage, browserLanguageKey } from "../browserLanguage";
import { invoke } from '../ipc/client';
import { listen } from "@tauri-apps/api/event";

import type { AppSettings, LanguagePreference } from "../types";
import { visualPreviewEnabled, visualPreviewLanguage, visualPreviewTheme } from "../visualPreview";

export { translate as tr, localeFor } from "../i18n";

export async function loadAppSettings(): Promise<AppSettings> {
  if (!("__TAURI_INTERNALS__" in window)) {
    return {
      revision: 0,
      deviceName: "ArcRelay",
      theme: visualPreviewEnabled() ? visualPreviewTheme() ?? "light" : "system",
      language: visualPreviewEnabled() ? visualPreviewLanguage() ?? "zhCn" : loadBrowserLanguage(),
      launchAtStartup: false,
      launchSilently: false,
      autoUpdateEnabled: true,
      updateChannel: "stable",
      sounds: defaultSoundPreferences(),
      notifications: {
        enabled: true,
        onlyWhenInactive: true,
        showPreviews: false,
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
      clipboardEnabled: true,
      clipboardShortcut: "CmdOrCtrl+Shift+V",
      clipboardAutoFocusSearch: true,
      clipboardSortBy: "updatedAt",
      clipboardSyncEnabled: true,
      clipboardSyncUpdateSystemClipboard: true,
      clipboardSyncEditsAndDeletes: true,
      clipboardSyncFavorites: true,
      nearbyDiscoverable: true,
      enhancedScreenshotEnabled: false,
      screenshotShortcut: "CmdOrCtrl+Shift+A",
      screenshotIncludeCursor: false,
      screenshotFormat: "png",
      screenshotFileNameTemplate: "Screenshot_{date}_{time}",
      webFiles: { enabled: false, port: 8767, bindMode: "lanOnly", siteName: "", sessionIdleMinutes: 720, allowVpnPrivate: false },
    };
  }
  return invoke("get_app_settings");
}

export async function onAppSettingsChanged(handler: (settings: AppSettings) => void) {
  if (!("__TAURI_INTERNALS__" in window)) {
    const update = (event: StorageEvent) => { if (event.key === browserLanguageKey) void loadAppSettings().then(handler); };
    window.addEventListener("storage", update);
    return () => window.removeEventListener("storage", update);
  }
  return listen<AppSettings>("app-settings-changed", (event) => handler(event.payload));
}
