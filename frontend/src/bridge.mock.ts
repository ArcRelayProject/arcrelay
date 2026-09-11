import { defaultSoundPreferences } from "./soundDefaults";
let mockSoundMuteUntil: number | null = null;
import { mcpMockBridge } from './bridge.mcp.mock';
import { loadBrowserLanguage, saveBrowserLanguage } from "./browserLanguage";
import { type UnlistenFn } from "@tauri-apps/api/event";
import type { ActionPreset, AppSettings, AppUpdateCheckResult, AppUpdateProgress, ActionOutputEvent, BootstrapState, ImportActionsResponse, InputMetrics, InputRuntimeSnapshot, LogStatus, DiagnosticBundleInfo, InstalledApp, McpConfig, NearbyDesktop, NearbyInputPeer, NotificationView, PrinterSharingSnapshot, PrintJobActivitySnapshot, PrivacySettings, PrivacySnapshot, QuickAction, RemoteFileEntry, RemoteFileDirectoryPage, RemoteFileDragPreparation, RemoteFileOpenResult, RemoteFileShare, RemoteFileState, RemoteFileTransferSession, LocalSharedDirectory, WebAccessMode, WebGatewayStatus, TransferSnapshot, EdgeTestResult, WorkspaceConfiguration } from "./types";
import { mockData } from "./bridgeMockData";
import { automationMock } from './bridge.automations.mock';
import { transfersMock } from './bridge.transfers.mock';
import { sortRemoteFileEntries, type RemoteFileSortDirection, type RemoteFileSortKey } from "./remoteFileSort";
import { visualPreviewEnabled, visualPreviewLanguage, visualPreviewTheme } from "./visualPreview";
const inTauri = () => "__TAURI_INTERNALS__" in window;
const stateListeners = new Set<(state: BootstrapState) => void>();
const outputListeners = new Set<(event: ActionOutputEvent) => void>();
const privacyListeners = new Set<(state: PrivacySnapshot) => void>();
const outputs = new Map<string, string[]>();
const cloneState = () => structuredClone({ ...mockData.state, revision: ++mockData.state.revision });
function emitMockState() {
    const state = cloneState();
    stateListeners.forEach((listener) => listener(state));
}
function mockShortcutIdentity(value: string) {
    const mac = /Mac/i.test(navigator.platform);
    const aliases: Record<string, string> = {
        control: "ctrl", cmd: "super", command: "super", option: "alt",
        cmdorctrl: mac ? "super" : "ctrl", commandorcontrol: mac ? "super" : "ctrl",
    };
    return value.toLowerCase().split("+").map(part => {
        const token = part.trim().replace(/^key([a-z])$/, "$1").replace(/^digit([0-9])$/, "$1");
        return aliases[token] ?? token;
    }).sort().join("+");
}
function actionLabel(action: QuickAction) {
    switch (action.action_type.type) {
        case "LaunchApp":
            return "启动应用";
        case "OpenPath":
            return "打开路径";
        case "OpenUrl":
            return "打开 URL";
        case "ShellCommand":
            return "Shell 脚本";
        case "Hotkey":
            return "快捷键";
        case "AppleScript":
            return "AppleScript";
        case "System":
            return "系统操作";
        case "Media":
            return "媒体控制";
        case "SetSystemVolume":
            return "设置音量";
        case "SetSystemMuted":
            return action.action_type.muted ? "系统静音" : "取消静音";
        case "SetMicrophone":
            return action.action_type.active ? "开启麦克风" : "麦克风静音";
        case "ToggleShellCommand":
            return "开关命令";
    }
}
const mockRecoveredInputPeers = new Set<string>();
function browserInputRuntimeSnapshot(): InputRuntimeSnapshot {
    const snapshot = structuredClone(mockData.inputRuntime);
    const scenario = typeof window === "undefined"
        ? null
        : new URLSearchParams(window.location.search).get("inputScenario");
    if (scenario === "recovery" && !mockRecoveredInputPeers.has("remote-office-mac")) {
        const peer = snapshot.nearbyPeers.find((value) => value.serviceInstanceId === "remote-office-mac");
        if (peer) {
            peer.paired = true;
            peer.connected = false;
        }
        snapshot.connectedPeers = snapshot.connectedPeers.filter((value) => value !== "remote-office-mac");
        snapshot.displayAvailability["remote-display"] = "Offline";
        snapshot.controller = null;
        snapshot.controlEpoch = null;
        if (snapshot.configuration.layout) {
            snapshot.configuration.layout.portals = snapshot.configuration.layout.portals.map((portal) => ({ ...portal, status: "SuspendedOffline" }));
        }
    }
    else if (scenario === "onboarding") {
        snapshot.configuration.layout = null;
        snapshot.connectedPeers = [];
        snapshot.controller = null;
        snapshot.controlEpoch = null;
    }
    else if (scenario === "diagnostics-latency") {
        const peer = snapshot.nearbyPeers.find((value) => value.serviceInstanceId === "remote-office-mac");
        if (peer) peer.displayName = "设计工作室的 MacBook Air";
        snapshot.controller = "remote-office-mac";
        snapshot.diagnostics = snapshot.diagnostics.map((record, index) => ({
            ...record,
            latencyMicros: [2_640_000, 2_641_000, 2_650_500, 2_632_000][index % 4],
            message: "输入延迟超过目标范围",
        }));
    }
    else if (scenario === "diagnostics-empty") {
        snapshot.diagnostics = [];
    }
    else if (scenario === "diagnostics-permissions") {
        snapshot.capabilities.canCaptureKeyboard = false;
    }
    return snapshot;
}
const mockDirectoryKey = (shareId: string, relativePath: string) => `${shareId}:${relativePath}`;
function mockDirectory(shareId: string, relativePath: string) {
    return mockData.remoteDirectories.get(mockDirectoryKey(shareId, relativePath)) ?? [];
}
function mockRemoteFileTransfer(direction: "upload" | "download", peerId: string, shareId: string, directoryPath: string, name: string): RemoteFileTransferSession {
    const now = Date.now();
    return {
        id: crypto.randomUUID(), direction, status: "transferring", name, peerId, shareId, directoryPath,
        bytesTransferred: 0, totalBytes: 0, filesTransferred: 0, totalFiles: 0,
        currentName: "", error: null, startedAtMs: now, updatedAtMs: now,
    };
}
export const bridge = {
    ...mcpMockBridge,
    ...automationMock,
    async onPrintJobActivityError(_listener: (message: string) => void): Promise<UnlistenFn> { return () => { }; },
    async stopRemoteEdit(_peerId: string, _shareId: string, _relativePath: string): Promise<void> { },
    async getRuntimeModules(): Promise<import("./ipc/generated").ModuleStatus[]> { return []; },
    async onRuntimeModules(_listener: (modules: import("./ipc/generated").ModuleStatus[]) => void): Promise<UnlistenFn> { return () => { }; },
    isTauri: inTauri,
    async getInputRuntimeSnapshot(): Promise<InputRuntimeSnapshot> {
        return browserInputRuntimeSnapshot();
    },
    async previewWorkspace(configuration: WorkspaceConfiguration): Promise<WorkspaceConfiguration> {
        return structuredClone(configuration);
    },
    async arrangeWorkspace(configuration: WorkspaceConfiguration): Promise<WorkspaceConfiguration> {
        return structuredClone(configuration);
    },
    async saveWorkspace(configuration: WorkspaceConfiguration): Promise<InputRuntimeSnapshot> {
        mockData.inputRuntime.configuration = structuredClone(configuration);
        mockData.inputRuntime.revision += 1;
        return structuredClone(mockData.inputRuntime);
    },
    async setInputSharingEnabled(enabled: boolean): Promise<InputRuntimeSnapshot> {
        mockData.inputRuntime.configuration.inputSharingEnabled = enabled;
        for (const display of Object.values(mockData.inputRuntime.configuration.layout?.displays ?? {})) {
            if (display.deviceId === mockData.inputRuntime.serviceInstanceId) {
                mockData.inputRuntime.displayAvailability[display.displayId] = enabled ? "Ready" : "SharingDisabled";
            }
        }
        mockData.inputRuntime.captureActive = enabled;
        if (!enabled) {
            mockData.inputRuntime.controller = null;
            mockData.inputRuntime.controlEpoch = null;
        }
        mockData.inputRuntime.revision += 1;
        return structuredClone(mockData.inputRuntime);
    },
    async takeInputControl(): Promise<InputRuntimeSnapshot> {
        mockData.inputRuntime.controller = mockData.inputRuntime.serviceInstanceId;
        mockData.inputRuntime.controlEpoch = (mockData.inputRuntime.controlEpoch ?? 0) + 1;
        mockData.inputRuntime.revision += 1;
        return structuredClone(mockData.inputRuntime);
    },
    async releaseInputControl(): Promise<InputRuntimeSnapshot> {
        mockData.inputRuntime.controller = null;
        mockData.inputRuntime.controlEpoch = null;
        mockData.inputRuntime.revision += 1;
        return structuredClone(mockData.inputRuntime);
    },
    async connectInputPeer(peer: Pick<NearbyInputPeer, "serviceInstanceId" | "addresses" | "port">): Promise<InputRuntimeSnapshot> {
        const existing = mockData.inputRuntime.nearbyPeers.find((value) => value.serviceInstanceId === peer.serviceInstanceId);
        if (existing) {
            existing.connected = true;
            mockRecoveredInputPeers.add(existing.serviceInstanceId);
            if (!mockData.inputRuntime.connectedPeers.includes(existing.serviceInstanceId)) {
                mockData.inputRuntime.connectedPeers.push(existing.serviceInstanceId);
            }
        }
        mockData.inputRuntime.revision += 1;
        return structuredClone(mockData.inputRuntime);
    },
    async forgetInputPeer(serviceInstanceId: string): Promise<InputRuntimeSnapshot> {
        mockData.inputRuntime.connectedPeers = mockData.inputRuntime.connectedPeers.filter((peer) => peer !== serviceInstanceId);
        mockRecoveredInputPeers.delete(serviceInstanceId);
        mockData.inputRuntime.nearbyPeers = mockData.inputRuntime.nearbyPeers.map((peer) => peer.serviceInstanceId === serviceInstanceId ? { ...peer, paired: false, connected: false } : peer);
        delete mockData.inputRuntime.remoteOperatingSystems[serviceInstanceId];
        if (mockData.inputRuntime.configuration.layout) {
            const displayIds = Object.values(mockData.inputRuntime.configuration.layout.displays)
                .filter((display) => display.deviceId === serviceInstanceId)
                .map((display) => display.displayId);
            for (const displayId of displayIds)
                delete mockData.inputRuntime.configuration.layout.displays[displayId];
            mockData.inputRuntime.configuration.layout.portals = mockData.inputRuntime.configuration.layout.portals.filter((portal) => !displayIds.includes(portal.sourceDisplay) && !displayIds.includes(portal.targetDisplay));
        }
        mockData.inputRuntime.revision += 1;
        return structuredClone(mockData.inputRuntime);
    },
    async testEdge(request: {
        displayId: string;
        pointXUm: number;
        pointYUm: number;
        deltaXUm: number;
        deltaYUm: number;
    }, configuration?: WorkspaceConfiguration): Promise<EdgeTestResult> {
        const portal = mockData.inputRuntime.configuration.layout?.portals.find((candidate) => {
            if (candidate.status !== "Active")
                return false;
            const edgeMatches = (edge: "Left" | "Right" | "Top" | "Bottom") => edge === "Left" ? request.deltaXUm < 0
                : edge === "Right" ? request.deltaXUm > 0
                    : edge === "Top" ? request.deltaYUm < 0
                        : request.deltaYUm > 0;
            return (candidate.sourceDisplay === request.displayId && edgeMatches(candidate.sourceEdge))
                || (candidate.direction === "Bidirectional" && candidate.targetDisplay === request.displayId && edgeMatches(candidate.targetEdge));
        });
        const reverse = portal?.targetDisplay === request.displayId;
        return {
            displayId: portal ? reverse ? portal.sourceDisplay : portal.targetDisplay : request.displayId,
            pointXUm: request.pointXUm + request.deltaXUm,
            pointYUm: request.pointYUm + request.deltaYUm,
            portalIds: portal ? [portal.portalId] : [],
        };
    },
    async getLogStatus(): Promise<LogStatus> {
        return {
            enabled: true,
            runId: "browser-preview",
            startedAtMs: Date.now(),
            logDirectory: "/tmp/ArcRelay/logs/desktop",
            currentLogFile: "/tmp/ArcRelay/logs/desktop/desktop-preview.jsonl",
            droppedLines: 0,
            schemaVersion: 1,
            maxFileBytes: 10 * 1024 * 1024,
            maxTotalBytes: 100 * 1024 * 1024,
            retentionDays: 7,
            currentFilter: "info",
            detailedUntilMs: null,
        };
    },
    async setDetailedLogging(enabled: boolean): Promise<LogStatus> {
        const status = await bridge.getLogStatus();
        return {
            ...status,
            currentFilter: enabled ? "info,arcrelay_desktop::arc_input=debug" : "info",
            detailedUntilMs: enabled ? Date.now() + 15 * 60000 : null,
        };
    },
    async openLogDirectory(): Promise<void> {
    },
    async exportDiagnosticBundle(): Promise<DiagnosticBundleInfo> {
        return {
            path: "/tmp/ArcRelay-diagnostics-preview.zip",
            fileName: "ArcRelay-diagnostics-preview.zip",
            logFileCount: 1,
            sizeBytes: 1024,
        };
    },
    async getBootstrapState(): Promise<BootstrapState> {
        return cloneState();
    },
    async takePendingTrayNavigation(): Promise<string | null> { return null; },
    async onTrayNavigationPending(_listener: () => void): Promise<UnlistenFn> { return () => {}; },
    async getAppSettings(): Promise<AppSettings> {
        mockData.appSettings.language = visualPreviewEnabled()
            ? visualPreviewLanguage() ?? "zhCn"
            : loadBrowserLanguage();
        if (visualPreviewEnabled()) mockData.appSettings.theme = visualPreviewTheme() ?? "light";
        return structuredClone(mockData.appSettings);
    },
    async updateAppSettings(patch: import("./ipc/generated").AppSettingsPatch): Promise<AppSettings> {
        if (patch.language) saveBrowserLanguage(patch.language);
        mockData.appSettings = { ...mockData.appSettings, ...structuredClone(patch), notifications: { ...mockData.appSettings.notifications, ...patch.notifications }, sounds: { ...mockData.appSettings.sounds, ...patch.sounds }, webFiles: { ...mockData.appSettings.webFiles, ...patch.webFiles }, revision: mockData.appSettings.revision + 1 };
        return structuredClone(mockData.appSettings);
    },
    async onAppSettingsChanged(listener: (settings: AppSettings) => void): Promise<UnlistenFn> {
        return () => { };
    },
    async previewSound(_event: import("./ipc/generated").SoundEvent): Promise<void> { throw new Error("Sound preview is available in the desktop app."); },
    async getSoundMuteUntil(): Promise<number | null> { return mockSoundMuteUntil; },
    async setSoundTemporaryMute(muted: boolean): Promise<number | null> { return mockSoundMuteUntil = muted ? Date.now() + 3_600_000 : null; },
    async resetSoundPreferences(): Promise<AppSettings> { mockData.appSettings.sounds = defaultSoundPreferences(); return structuredClone(mockData.appSettings); },
    async onSoundMuteChanged(_listener: (until: number | null) => void): Promise<UnlistenFn> { return () => {}; },
    async onDesktopNotification(_listener: (notification: { title: string; body: string; error: boolean }) => void): Promise<UnlistenFn> { return () => {}; },
    async showTestSystemNotification(): Promise<string> {
        if (!mockData.appSettings.notifications.enabled)
            throw new Error("enable desktop notifications first");
        return "测试通知已发送";
    },
    async onOpenTransferRequest(listener: (transferId: string) => void): Promise<UnlistenFn> {
        return () => undefined;
    },
    async takePendingTransferRequest(): Promise<string | null> {
        return null;
    },
    async onOpenSystemShareRequest(_listener: (request: import('./types').SystemShareRequest) => void): Promise<UnlistenFn> {
        return () => undefined;
    },
    async listSystemShareRequests(): Promise<import('./types').SystemShareRequest[]> {
        return [];
    },
    async submitSystemShareRequest(_requestId: string, _transferId: string, _peerId: string): Promise<void> {
    },
    async discardSystemShareRequest(_requestId: string): Promise<void> {
    },
    async checkForAppUpdate(): Promise<AppUpdateCheckResult> {
        return { currentVersion: "0.1.2", channel: mockData.appSettings.updateChannel, update: null };
    },
    async installAppUpdate(): Promise<void> {
    },
    async onAppUpdateChecked(listener: (result: AppUpdateCheckResult) => void): Promise<UnlistenFn> {
        return () => { };
    },
    async onAppUpdateProgress(listener: (progress: AppUpdateProgress) => void): Promise<UnlistenFn> {
        return () => undefined;
    },
    async openSniptraSettings(): Promise<string> {
        return "Sniptra screenshot settings opened";
    },
    async startScreenshotCapture(): Promise<string> {
        return "Screenshot capture opened";
    },
    async mergeClipboardDevices(): Promise<import("./ipc/generated").ClipboardMergeSummary> {
      return { devices: 2, received: 3, sent: 0, labelsReceived: 0, labelsSent: 0, failed: 0, totalRecords: 3, complete: true, failures: [] };
    },
    async clearClipboardHistory(): Promise<void> {
    },
    async getTransferState(): Promise<TransferSnapshot> {
        return { ...await transfersMock.getTransferState(), discoverable: mockData.appSettings.nearbyDiscoverable };
    },
    async getPrinterSharingState(): Promise<PrinterSharingSnapshot> {
        return { hostingSupported: true, localPrinters: [], shares: [], remotePrinters: [], queueBindings: [] };
    },
    async onPrintJobActivity(listener: (value: PrintJobActivitySnapshot) => void): Promise<UnlistenFn> {
        return () => { };
    },
    async observePrintJobs(): Promise<UnlistenFn> {
        return () => { };
    },
    async getPrintJobActivity(): Promise<PrintJobActivitySnapshot> {
        return { revision: 0, received: [], sent: [] };
    },
    async refreshPrinterSharingState(): Promise<PrinterSharingSnapshot> {
        return { hostingSupported: true, localPrinters: [], shares: [], remotePrinters: [], queueBindings: [] };
    },
    async publishPrinter(localPrinterId: string): Promise<PrinterSharingSnapshot> {
        return { hostingSupported: true, localPrinters: [], shares: [], remotePrinters: [], queueBindings: [] };
    },
    async suspendPrinterShare(shareId: string): Promise<PrinterSharingSnapshot> {
        return { hostingSupported: true, localPrinters: [], shares: [], remotePrinters: [], queueBindings: [] };
    },
    async installRemotePrinter(sourceDeviceId: string, shareId: string): Promise<PrinterSharingSnapshot> {
        return { hostingSupported: true, localPrinters: [], shares: [], remotePrinters: [], queueBindings: [] };
    },
    async removeRemotePrinter(bindingId: string): Promise<PrinterSharingSnapshot> {
        return { hostingSupported: true, localPrinters: [], shares: [], remotePrinters: [], queueBindings: [] };
    },
    async onTransferState(listener: (state: TransferSnapshot) => void): Promise<UnlistenFn> {
        return transfersMock.onTransferState(listener);
    },
    async onNotificationCount(listener: (count: number) => void): Promise<UnlistenFn> {
        return () => undefined;
    },
    async refreshTransferDevices(): Promise<TransferSnapshot> {
        return transfersMock.refreshTransferDevices();
    },
    async pickTransferFiles(): Promise<string[]> {
        return transfersMock.pickTransferFiles();
    },
    async inspectTransferFiles(paths: string[]): Promise<import('./types').TransferDraftFile[]> {
        return transfersMock.inspectTransferFiles(paths);
    },
    async sendTransfer(peerId: string, paths: string[], requestIds: string[] = []): Promise<string> {
        return transfersMock.sendTransfer(peerId, paths, requestIds);
    },
    async respondTransfer(transferId: string, accepted: boolean, automaticReceive: boolean): Promise<void> {
        return transfersMock.respondTransfer(transferId, accepted, automaticReceive);
    },
    async pauseTransfer(transferId: string): Promise<void> {
        return transfersMock.pauseTransfer(transferId);
    },
    async resumeTransfer(transferId: string): Promise<void> {
        return transfersMock.resumeTransfer(transferId);
    },
    async cancelTransfer(transferId: string): Promise<void> {
        return transfersMock.cancelTransfer(transferId);
    },
    async chooseTransferReceiveDirectory(): Promise<TransferSnapshot> {
        return transfersMock.chooseTransferReceiveDirectory();
    },
    async openTransferReceiveDirectory(): Promise<void> {
        return transfersMock.openTransferReceiveDirectory();
    },
    async setTransferReceivePolicy(peerId: string, automatic: boolean): Promise<TransferSnapshot> {
        return transfersMock.setTransferReceivePolicy(peerId, automatic);
    },
    async listSystemFolders(): Promise<import('./ipc/generated').SystemFolder[]> { return []; },
    async addSystemFolder(_peerId: string, _shareId: string): Promise<import('./ipc/generated').SystemFolder> { throw new Error("System folder integration requires the installed desktop app."); },
    async openSystemFolder(_id: string): Promise<void> {},
    async removeSystemFolder(_id: string): Promise<void> {},
    async openSystemFolderRecovery(): Promise<void> {},
    async getRemoteFileState(): Promise<RemoteFileState> {
        return {
            devices: [{ id: "chen-windows", name: "Chen 的 Windows 工作站" }],
            localShares: structuredClone(mockData.localShares),
        };
    },
    async getLocalFileShares(): Promise<LocalSharedDirectory[]> {
        return structuredClone(mockData.localShares);
    },
    async listRemoteFileShares(peerId: string): Promise<RemoteFileShare[]> {
        return structuredClone(mockData.remoteShares);
    },
    async listRemoteDirectory(peerId: string, shareId: string, relativePath: string, cursor: string | null = null, search = "", sortKey: RemoteFileSortKey = "modified", sortDirection: RemoteFileSortDirection = "descending"): Promise<RemoteFileDirectoryPage> {
        const offset = Number.parseInt(cursor ?? "0", 10);
        const query = search.trim().toLocaleLowerCase("zh-CN");
        const all = sortRemoteFileEntries(mockDirectory(shareId, relativePath).filter((entry) => entry.name.toLocaleLowerCase("zh-CN").includes(query)), sortKey, sortDirection, "zhCn");
        const entries = all.slice(offset, offset + 100);
        return structuredClone({ entries, nextCursor: offset + entries.length < all.length ? String(offset + entries.length) : null });
    },
    async createRemoteDirectory(peerId: string, shareId: string, relativePath: string, name: string): Promise<RemoteFileEntry> {
        const entry: RemoteFileEntry = {
            name,
            relativePath: [relativePath, name].filter(Boolean).join("/"),
            kind: "folder",
            size: 0,
            modifiedAtMs: Date.now(),
        };
        mockData.remoteDirectories.set(mockDirectoryKey(shareId, relativePath), [...mockDirectory(shareId, relativePath), entry]);
        mockData.remoteDirectories.set(mockDirectoryKey(shareId, entry.relativePath), []);
        return structuredClone(entry);
    },
    async renameRemoteEntry(peerId: string, shareId: string, relativePath: string, newName: string): Promise<RemoteFileEntry> {
        const parentPath = relativePath.split("/").slice(0, -1).join("/");
        const entries = mockDirectory(shareId, parentPath);
        const entry = entries.find((item) => item.relativePath === relativePath);
        if (!entry)
            throw new Error("item to rename was not found");
        const next = { ...entry, name: newName, relativePath: [parentPath, newName].filter(Boolean).join("/") };
        mockData.remoteDirectories.set(mockDirectoryKey(shareId, parentPath), entries.map((item) => item === entry ? next : item));
        return structuredClone(next);
    },
    async deleteRemoteEntries(peerId: string, shareId: string, paths: string[]): Promise<Array<{
        path: string;
        error: string | null;
    }>> {
        return paths.map((path) => ({ path, error: null }));
    },
    async deleteRemoteEntry(peerId: string, shareId: string, relativePath: string): Promise<void> {
        const parentPath = relativePath.split("/").slice(0, -1).join("/");
        mockData.remoteDirectories.set(mockDirectoryKey(shareId, parentPath), mockDirectory(shareId, parentPath).filter((item) => item.relativePath !== relativePath));
        return;
    },
    async pickRemoteUpload(peerId: string, shareId: string, relativePath: string, folder: boolean): Promise<number> {
        return folder ? 1 : 2;
    },
    async uploadRemotePaths(peerId: string, shareId: string, relativePath: string, paths: string[]): Promise<number> {
        return paths.length;
    },
    async startRemoteUpload(peerId: string, shareId: string, relativePath: string, folder: boolean): Promise<RemoteFileTransferSession | null> {
        return mockRemoteFileTransfer("upload", peerId, shareId, relativePath, folder ? "示例文件夹" : "示例文件");
    },
    async startRemoteUploadPaths(peerId: string, shareId: string, relativePath: string, paths: string[]): Promise<RemoteFileTransferSession> {
        return mockRemoteFileTransfer("upload", peerId, shareId, relativePath, paths.at(0)?.split("/").at(-1) ?? "上传项目");
    },
    async downloadRemoteEntry(peerId: string, shareId: string, relativePath: string): Promise<string | null> {
        return `/Users/demo/Downloads/${relativePath.split("/").at(-1) ?? "download"}`;
    },
    async downloadRemoteEntries(peerId: string, shareId: string, relativePaths: string[]): Promise<string[] | null> {
        return relativePaths.map((relativePath) => `/Users/demo/Downloads/${relativePath.split("/").at(-1) ?? "download"}`);
    },
    async startRemoteDownloadEntries(peerId: string, shareId: string, relativePaths: string[]): Promise<RemoteFileTransferSession[] | null> {
        return relativePaths.map((path) => mockRemoteFileTransfer("download", peerId, shareId, path.split("/").slice(0, -1).join("/"), path.split("/").at(-1) ?? "下载项目"));
    },
    async listRemoteFileTransfers(): Promise<RemoteFileTransferSession[]> {
        return [];
    },
    async getRemoteFileThumbnail(peerId: string, shareId: string, relativePath: string, modifiedAtMs: number): Promise<string | null> {
        return null;
    },
    async openRemoteEntry(peerId: string, shareId: string, relativePath: string): Promise<RemoteFileOpenResult> {
        return {
            localPath: `/tmp/ArcRelay/remote-open/${relativePath.split("/").at(-1) ?? "file"}`,
            editable: /\.(?:txt|md|json|ya?ml|toml|csv|log|xml|html?|css|js|ts|rs|py|sh)$/i.test(relativePath),
        };
    },
    async prepareRemoteDrag(peerId: string, shareId: string, relativePath: string): Promise<RemoteFileDragPreparation> {
        const localPath = `/tmp/${relativePath.split("/").at(-1) ?? "download"}`;
        return { localPath, iconPath: localPath };
    },
    async startRemoteFilePromiseDrag(peerId: string, shareId: string, relativePath: string, kind: RemoteFileEntry["kind"], size: number): Promise<void> {
        return;
    },
    async getApplicationDragIconPath(): Promise<string> {
        return "";
    },
    async addRemoteFileShare(): Promise<LocalSharedDirectory | null> {
        const share: LocalSharedDirectory = {
            id: `local-${Date.now()}`,
            name: "新共享目录",
            path: "/Users/demo/Shared/新共享目录",
            writable: false,
            web: {
                mode: "disabled",
                listed: true,
                allowPreview: true,
                allowDownload: true,
                slug: crypto.randomUUID().replaceAll("-", "").slice(0, 20),
                hasPassword: false,
                credentialRevision: 0,
            },
        };
        mockData.localShares = [...mockData.localShares, share];
        return structuredClone(share);
    },
    async openFullDiskAccessSettings(): Promise<void> {
    },
    async removeRemoteFileShare(shareId: string): Promise<LocalSharedDirectory[]> {
        mockData.localShares = mockData.localShares.filter((share) => share.id !== shareId);
        return structuredClone(mockData.localShares);
    },
    async setRemoteFileShareWritable(shareId: string, writable: boolean): Promise<LocalSharedDirectory> {
        const share = mockData.localShares.find((item) => item.id === shareId);
        if (!share)
            throw new Error("shared directory not found");
        share.writable = writable;
        return structuredClone(share);
    },
    async setRemoteFileShareWebPolicy(shareId: string, policy: {
        mode: WebAccessMode;
        listed: boolean;
        allowPreview: boolean;
        allowDownload: boolean;
    }, newPassword?: string): Promise<LocalSharedDirectory> {
        const share = mockData.localShares.find((item) => item.id === shareId);
        if (!share)
            throw new Error("shared directory not found");
        share.web = {
            ...share.web,
            ...policy,
            hasPassword: policy.mode === "password" ? Boolean(newPassword || share.web.hasPassword) : false,
            credentialRevision: share.web.credentialRevision + 1,
        };
        return structuredClone(share);
    },
    async getWebGatewayStatus(): Promise<WebGatewayStatus> {
        const enabled = mockData.appSettings.webFiles.enabled;
        return {
            enabled,
            running: enabled,
            port: mockData.appSettings.webFiles.port,
            siteName: mockData.appSettings.webFiles.siteName || "演示电脑的 ArcRelay",
            addresses: enabled ? [`http://192.168.1.20:${mockData.appSettings.webFiles.port}`, `http://arcrelay-demo.local:${mockData.appSettings.webFiles.port}`] : [],
            sessionCount: enabled ? 2 : 0,
            lastError: null,
        };
    },
    async onWebGatewayStatus(listener: (status: WebGatewayStatus) => void): Promise<UnlistenFn> {
        return () => undefined;
    },
    async revokeWebSessions(shareId?: string): Promise<number> {
        return shareId ? 1 : 2;
    },
    async webGatewayQrCode(url: string): Promise<string> {
        return "";
    },
    async openWebGatewayUrl(url: string): Promise<void> {
        window.open(url, "_blank", "noopener,noreferrer");
        return;
    },
    async getActionPresets(): Promise<ActionPreset[]> {
        return [];
    },
    async installActionPreset(presetId: string): Promise<BootstrapState> {
        return cloneState();
    },
    async listInstalledApps(refresh = false): Promise<InstalledApp[]> {
        return [
            { name: "Visual Studio Code", path: "/Applications/Visual Studio Code.app", identifier: "com.microsoft.VSCode", version: null, iconDataUrl: null },
            { name: "腾讯会议", path: "/Applications/TencentMeeting.app", identifier: "com.tencent.meeting", version: null, iconDataUrl: null },
            { name: "Zoom", path: "/Applications/zoom.us.app", identifier: "us.zoom.xos", version: null, iconDataUrl: null },
            {
                name: "Finder",
                path: "/System/Library/CoreServices/Finder.app",
                identifier: "com.apple.finder",
                version: null,
                iconDataUrl: null,
            },
        ];
    },
    async getInstalledAppIcon(path: string): Promise<string | null> {
        return null;
    },
    async exportActionsText(): Promise<string> {
        return JSON.stringify({ format: "arcrelay-actions", version: 1, actions: mockData.state.actions }, null, 2);
    },
    async importActionsText(text: string): Promise<ImportActionsResponse> {
        const parsed = JSON.parse(text) as unknown;
        let imported: QuickAction[];
        if (Array.isArray(parsed)) {
            imported = parsed as QuickAction[];
        }
        else if (parsed && typeof parsed === "object" && "actions" in parsed) {
            imported = ((parsed as {
                actions?: QuickAction[];
            }).actions ?? []);
        }
        else {
            imported = [parsed as QuickAction];
        }
        for (const action of imported) {
            await bridge.saveAction({
                ...action,
                id: crypto.randomUUID(),
                revision: 0,
                source_preset_id: null,
                global_shortcut: null,
            });
        }
        return {
            result: { importedCount: imported.length, names: imported.map((action) => action.name) },
            state: cloneState(),
        };
    },
    async onState(listener: (state: BootstrapState) => void): Promise<UnlistenFn> {
        stateListeners.add(listener);
        return () => stateListeners.delete(listener);
    },
    async onPrivacyState(listener: (state: PrivacySnapshot) => void): Promise<UnlistenFn> {
        privacyListeners.add(listener);
        return () => privacyListeners.delete(listener);
    },
    async setPrivacyEnabled(enabled: boolean): Promise<PrivacySnapshot> {
        mockData.state.privacy = {
            ...mockData.state.privacy,
            active: enabled,
            manualEnabled: enabled,
            activationSource: enabled ? "manual" : "off",
        };
        const state = structuredClone(mockData.state.privacy);
        privacyListeners.forEach((listener) => listener(state));
        emitMockState();
        return state;
    },
    async updatePrivacySettings(settings: PrivacySettings): Promise<PrivacySnapshot> {
        mockData.state.privacy = { ...mockData.state.privacy, settings: structuredClone(settings) };
        const state = structuredClone(mockData.state.privacy);
        privacyListeners.forEach((listener) => listener(state));
        emitMockState();
        return state;
    },
    async onInputMetrics(listener: (metrics: InputMetrics | null) => void): Promise<UnlistenFn> {
        return () => undefined;
    },
    async onActionOutput(listener: (event: ActionOutputEvent) => void): Promise<UnlistenFn> {
        outputListeners.add(listener);
        return () => outputListeners.delete(listener);
    },
    async onNotificationsChanged(listener: () => void): Promise<UnlistenFn> {
        return () => { };
    },
    async validateActionShortcut(actionId: string, shortcut: string): Promise<void> {
        if (!shortcut.trim()) return;
        const key = mockShortcutIdentity(shortcut);
        const other = mockData.state.actions.find(action => action.id !== actionId
            && action.global_shortcut && mockShortcutIdentity(action.global_shortcut) === key);
        if (other) throw new Error(`global shortcut is already used by action \`${other.name}\``);
        const settings = mockData.appSettings;
        const reserved: [boolean, string, string][] = [
            [true, "CmdOrCtrl+Shift+U", "隐私解锁"],
            [true, "CmdOrCtrl+Alt+Shift+Escape", "跨屏输入紧急释放"],
            [true, "CmdOrCtrl+Alt+Shift+M", "跨屏输入指针居中"],
            [settings.clipboardEnabled, settings.clipboardShortcut, "剪贴板"],
            [settings.enhancedScreenshotEnabled, settings.screenshotShortcut, "截图"],
        ];
        for (const [enabled, value, name] of reserved) {
            if (enabled && mockShortcutIdentity(value) === key) throw new Error(`global shortcut is already used by ${name}`);
        }
    },
    async saveAction(action: QuickAction): Promise<BootstrapState> {
        await bridge.validateActionShortcut(action.id, action.global_shortcut ?? "");
        const previous = mockData.state.actions.find(item => item.id === action.id);
        if (previous && previous.revision !== action.revision) throw new Error("Action changed; reopen it before saving");
        const view = {
            ...action,
            revision: (action.revision ?? 0) + 1,
            actionTypeLabel: actionLabel(action),
            globalShortcutError: null,
            isToggle: action.action_type.type === "ToggleShellCommand",
            isRunning: false,
            requiresConfirmation: action.confirm_before_run,
        };
        const index = mockData.state.actions.findIndex((item) => item.id === action.id);
        if (index >= 0)
            mockData.state.actions[index] = view;
        else
            mockData.state.actions.push(view);
        emitMockState();
        return cloneState();
    },
    async deleteAction(actionId: string): Promise<BootstrapState> {
        mockData.state.actions = mockData.state.actions.filter((action) => action.id !== actionId);
        emitMockState();
        return cloneState();
    },
    async executeAction(actionId: string): Promise<string> {
        const action = mockData.state.actions.find((item) => item.id === actionId);
        const line = `${new Date().toLocaleTimeString("zh-CN", { hour12: false })}  ${action?.name ?? "动作"} 执行完成`;
        outputs.set(actionId, [...(outputs.get(actionId) ?? []), line]);
        outputListeners.forEach((listener) => listener({ actionId, revision: outputs.get(actionId)?.length ?? 0, lines: outputs.get(actionId) ?? [] }));
        return "执行完成";
    },
    async getActionOutput(actionId: string): Promise<ActionOutputEvent> {
        return { actionId, revision: outputs.get(actionId)?.length ?? 0, lines: [...(outputs.get(actionId) ?? [])] };
    },
    async listNotifications(includeRead = true, limit = 200): Promise<NotificationView[]> {
        return structuredClone(mockData.notifications.filter((item) => includeRead || item.readAtMs === null).slice(0, limit));
    },
    async createTestNotification(): Promise<NotificationView[]> {
        mockData.notifications = [
            {
                id: crypto.randomUUID(),
                title: "测试通知：Host 队列工作正常",
                body: "这条通知由桌面端写入 Host 持久队列。手机离线时会保留，任一已配对设备阅读后会在 Host 标记为全局已读。",
                source: "ArcRelay Desktop",
                kind: "info",
                reference: "desktop-test",
                createdAtMs: Date.now(),
                readAtMs: null,
                readByDeviceName: null,
                deliveryState: mockData.state.connectedDevices.length ? "unread" : "waitingForDevice",
            },
            ...mockData.notifications,
        ];
        mockData.state.unreadNotificationCount += 1;
        emitMockState();
        return structuredClone(mockData.notifications);
    },
    async markNotificationRead(notificationId: string): Promise<NotificationView[]> {
        mockData.notifications = mockData.notifications.map((item) => item.id === notificationId
            ? {
                ...item,
                readAtMs: item.readAtMs ?? Date.now(),
                readByDeviceName: item.readByDeviceName ?? "ArcRelay Desktop",
                deliveryState: "read",
            }
            : item);
        mockData.state.unreadNotificationCount = mockData.notifications.filter((item) => item.readAtMs === null).length;
        emitMockState();
        return structuredClone(mockData.notifications);
    },
    async deleteNotification(notificationId: string): Promise<NotificationView[]> {
        mockData.notifications = mockData.notifications.filter((item) => item.id !== notificationId);
        mockData.state.unreadNotificationCount = mockData.notifications.filter((item) => item.readAtMs === null).length;
        emitMockState();
        return structuredClone(mockData.notifications);
    },
    async getMcpConfig(): Promise<McpConfig> {
        const endpoint = "http://127.0.0.1:8766/mcp";
        const token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        return {
            endpoint,
            token,
            configText: `[mcp_servers.arcrelay]\nurl = "${endpoint}"\nhttp_headers = { Authorization = "Bearer ${token}" }`,
        };
    },

    async respondPairing(accepted: boolean, approvedGrantIds?: string[]): Promise<void> {
        mockData.state.pendingPairing = null;
        emitMockState();
    },
    async discoverDesktopDevices(): Promise<NearbyDesktop[]> {
        return [{
                deviceId: "device-macbook-air",
                deviceName: "Chen 的 MacBook Air",
                host: "macbook-air.local.",
                paired: false,
                connecting: false,
            }];
    },
    async connectDesktopDevice(deviceId: string): Promise<string> {
        const device = (await bridge.discoverDesktopDevices()).find((item) => item.deviceId === deviceId);
        if (!device)
            throw new Error("desktop device not found");
        mockData.state.outgoingPairings = [{
                deviceId: device.deviceId,
                deviceName: device.deviceName,
                pairingCode: "824193",
            }];
        emitMockState();
        return device.deviceName;
    },
    async connectDesktopAddress(host: string, port?: number): Promise<string> {
        return `ArcRelay (${host}${port ? `:${port}` : ""})`;
    },
    async forgetPairedDevice(deviceId: string): Promise<void> {
        mockData.state.pairedDevices = mockData.state.pairedDevices.filter((item) => item.id !== deviceId);
        emitMockState();
    },
    async disconnectDevice(deviceId: string): Promise<void> {
        mockData.state.connectedDevices = mockData.state.connectedDevices.filter((device) => device.id !== deviceId);
        emitMockState();
    },
    async setDeviceAutoConnect(deviceId: string, enabled: boolean): Promise<void> {
        mockData.state.pairedDevices = mockData.state.pairedDevices.map((item) => item.id === deviceId ? { ...item, autoConnect: enabled } : item);
        mockData.state.connectedDevices = mockData.state.connectedDevices.map((item) => item.id === deviceId ? { ...item, autoConnect: enabled } : item);
        emitMockState();
    },
    async openInputPermissionSettings(): Promise<void> {
    },
    async closeWindow(): Promise<void> {
    },
    async minimizeWindow(): Promise<void> {
    },
    async toggleMaximizeWindow(): Promise<boolean> {
        return false;
    },
    async isWindowMaximized(): Promise<boolean> {
        return false;
    },
    async quitApp(): Promise<void> {
    }
};
