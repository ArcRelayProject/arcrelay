import { SubscriptionScope } from "./subscriptions";
import type { DesktopStateUpdate } from "./desktopProjection";
import { invoke, listen } from './ipc/client';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { AppSettings, AppUpdateCheckResult, AppUpdateProgress, BootstrapState, LogStatus, DiagnosticBundleInfo, McpConfig, NearbyDesktop, NotificationView, PrivacySettings, PrivacySnapshot, LocalSharedDirectory, EdgeTestResult, WorkspaceConfiguration } from "./types";
import { observeFrontendOperation } from "./logging";
const inTauri = () => "__TAURI_INTERNALS__" in window;
export const systemBridge = {
    isTauri: inTauri,
    async testEdge(request: {
        displayId: string;
        pointXUm: number;
        pointYUm: number;
        deltaXUm: number;
        deltaYUm: number;
    }, configuration?: WorkspaceConfiguration): Promise<EdgeTestResult> {
        return observeFrontendOperation("input.portal.test", () => invoke("test_input_edge", { request, configuration: configuration ?? null }));
    },
    async getLogStatus(): Promise<LogStatus> {
        return invoke("get_log_status");
    },
    async setDetailedLogging(enabled: boolean): Promise<LogStatus> {
        return invoke("set_detailed_logging", { enabled });
    },
    async openLogDirectory(): Promise<void> {
        await invoke("open_log_directory");
    },
    async exportDiagnosticBundle(): Promise<DiagnosticBundleInfo> {
        return observeFrontendOperation("diagnostics.bundle.export", () => invoke("export_diagnostic_bundle"));
    },
    async getRuntimeModules(): Promise<import("./ipc/generated").ModuleStatus[]> {
        return invoke("get_runtime_modules");
    },
    async onRuntimeModules(listener: (modules: import("./ipc/generated").ModuleStatus[]) => void): Promise<UnlistenFn> {
        return listen("runtime-modules", ({ payload }) => listener(payload));
    },
    async getBootstrapState(): Promise<BootstrapState> {
        return invoke("get_bootstrap_state");
    },
    takePendingTrayNavigation: () => invoke("take_pending_tray_navigation"),
    onTrayNavigationPending: (listener: () => void) => listen("tray-navigation-pending", listener),
    async getAppSettings(): Promise<AppSettings> {
        return invoke("get_app_settings");
    },
    async updateAppSettings(patch: import("./ipc/generated").AppSettingsPatch): Promise<AppSettings> {
        return invoke("update_app_settings", { patch });
    },
    async onAppSettingsChanged(listener: (settings: AppSettings) => void): Promise<UnlistenFn> {
        return listen("app-settings-changed", ({ payload }) => listener(payload));
    },
    previewSound: (event: import("./ipc/generated").SoundEvent) => invoke("preview_sound", { event }),
    getSoundMuteUntil: () => invoke("get_sound_mute_until"),
    setSoundTemporaryMute: (muted: boolean) => invoke("set_sound_temporary_mute", { muted }),
    resetSoundPreferences: () => invoke("reset_sound_preferences"),
    onSoundMuteChanged: (listener: (until: number | null) => void) => listen("sound-mute-changed", ({ payload }) => listener(payload)),
    onDesktopNotification: (listener: (notification: { title: string; body: string; error: boolean }) => void) =>
        listen("desktop-notification", ({ payload }) => listener(payload)),
    async showTestSystemNotification(): Promise<string> {
        return invoke("show_test_system_notification");
    },
    async checkForAppUpdate(): Promise<AppUpdateCheckResult> {
        return invoke("check_for_app_update");
    },
    async installAppUpdate(): Promise<void> {
        await invoke("install_app_update");
    },
    async onAppUpdateChecked(listener: (result: AppUpdateCheckResult) => void): Promise<UnlistenFn> {
        return listen("app-update-checked", (event) => listener(event.payload));
    },
    async onAppUpdateProgress(listener: (progress: AppUpdateProgress) => void): Promise<UnlistenFn> {
        return listen("app-update-progress", (event) => listener(event.payload));
    },
    async openSniptraSettings(): Promise<string> {
        return invoke("open_sniptra_settings");
    },
    async startScreenshotCapture(): Promise<string> {
        return invoke("start_screenshot_capture");
    },
    async mergeClipboardDevices(): Promise<number> {
        return invoke("clipboard_merge_devices");
    },
    async clearClipboardHistory(): Promise<void> {
        await invoke("clipboard_clear_history");
    },
    async onNotificationCount(listener: (count: number) => void): Promise<UnlistenFn> {
        return listen("notification-count", (event) => listener(event.payload));
    },
    async getLocalFileShares(): Promise<LocalSharedDirectory[]> {
        return invoke("get_local_file_shares");
    },
    async getApplicationDragIconPath(): Promise<string> {
        return invoke("get_application_drag_icon_path");
    },
    async revokeWebSessions(shareId?: string): Promise<number> {
        return invoke("revoke_web_sessions", { shareId: shareId || null });
    },
    async webGatewayQrCode(url: string): Promise<string> {
        return invoke("web_gateway_qr_code", { url });
    },
    async onState(listener: (state: DesktopStateUpdate) => void): Promise<UnlistenFn> {
        const scope = new SubscriptionScope();
        try {
            await Promise.all([
                scope.add(listen("desktop-state", ({ payload }) => listener(payload))),
                scope.add(listen("desktop-runtime", ({ payload }) => listener(payload))),
            ]);
            return () => scope.dispose();
        }
        catch (error) {
            scope.dispose();
            throw error;
        }
    },
    async onPrivacyState(listener: (state: PrivacySnapshot) => void): Promise<UnlistenFn> {
        return listen("privacy-state", (event) => listener(event.payload));
    },
    async setPrivacyEnabled(enabled: boolean): Promise<PrivacySnapshot> {
        return invoke("set_privacy_enabled", { enabled });
    },
    async updatePrivacySettings(settings: PrivacySettings): Promise<PrivacySnapshot> {
        return invoke("update_privacy_settings", { settings });
    },
    async onNotificationsChanged(listener: () => void): Promise<UnlistenFn> {
        return listen("notifications-changed", () => listener());
    },
    async listNotifications(includeRead = true, limit = 200): Promise<NotificationView[]> {
        return invoke("list_notifications", { includeRead, limit });
    },
    async createTestNotification(): Promise<NotificationView[]> {
        return invoke("create_test_notification");
    },
    async markNotificationRead(notificationId: string): Promise<NotificationView[]> {
        return invoke("mark_notification_read", { notificationId });
    },
    async deleteNotification(notificationId: string): Promise<NotificationView[]> {
        return invoke("delete_notification", { notificationId });
    },
    async getMcpConfig(): Promise<McpConfig> {
        return invoke("get_mcp_config");
    },
    async respondPairing(accepted: boolean, approvedGrantIds?: string[]): Promise<void> {
        await invoke("respond_pairing", { accepted, approvedGrantIds: approvedGrantIds ?? null });
    },
    async discoverDesktopDevices(): Promise<NearbyDesktop[]> {
        return invoke("discover_desktop_devices");
    },
    async connectDesktopDevice(deviceId: string): Promise<string> {
        return invoke("connect_desktop_device", { deviceId });
    },
    async connectDesktopAddress(host: string, port?: number): Promise<string> {
        return invoke("connect_desktop_address", { host, port: port ?? null });
    },
    async forgetPairedDevice(deviceId: string): Promise<void> {
        await invoke("forget_paired_device", { deviceId });
    },
    async disconnectDevice(deviceId: string): Promise<void> {
        await invoke("disconnect_device", { deviceId });
    },
    async setDeviceAutoConnect(deviceId: string, enabled: boolean): Promise<void> {
        await invoke("set_device_auto_connect", { deviceId, enabled });
    },
    async closeWindow(): Promise<void> {
        await getCurrentWebviewWindow().close();
    },
    async minimizeWindow(): Promise<void> {
        await getCurrentWebviewWindow().minimize();
    },
    async toggleMaximizeWindow(): Promise<boolean> {
        const window = getCurrentWebviewWindow();
        await window.toggleMaximize();
        return window.isMaximized();
    },
    async isWindowMaximized(): Promise<boolean> {
        return getCurrentWebviewWindow().isMaximized();
    },
    async quitApp(): Promise<void> {
        await invoke("quit_app");
    }
};
