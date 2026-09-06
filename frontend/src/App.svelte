<script lang="ts">
  import { defaultSoundPreferences } from "./soundDefaults";
  import { t } from "./localization";
  import { DesktopProjection, type DesktopStateUpdate } from "./desktopProjection";
  const desktopProjection = new DesktopProjection();
  let moduleStatuses: import("./ipc/generated").ModuleStatus[] = [];
  import { translate as uiTranslate, language as uiLanguage } from "./i18n";
  import { SubscriptionScope } from "./subscriptions";
  import { onMount, tick } from "svelte";
  import { parseTrayDestination } from "./trayNavigation";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { Checkbox, Dialog } from "bits-ui";
  import {
    AppWindow,
    Bell,
    Camera,
    CaretDown,
    Check,
    CheckCircle,
    ClockCounterClockwise,
    Code,
    Copy,
    CopySimple,
    DesktopTower,
    DeviceMobile,
    EyeSlash,
    FileText,
    FolderOpen,
    FolderSimplePlus,
    Gear,
    HardDrives,
    HandTap,
    Keyboard,
    Lightning,
    MagnifyingGlass,
    Minus,
    Monitor,
    Moon,
    Palette,
    PencilSimple,
    PaperPlaneTilt,
    Play,
    Plus,
    Power,
    Printer,
    PuzzlePiece,
    ShieldCheck,
    Shield,
    Star,
    ChatCircleDots,
    SquaresFour,
    Square,
    Trash,
    WarningCircle,
    WifiHigh,
    GlobeHemisphereWest,
    ClipboardText,
    RocketLaunch,
    X,
  } from "phosphor-svelte";

  import BrandLogo from "./BrandLogo.svelte";
  import ActionIcon from "./ActionIcon.svelte";
  import ActionsFeature from "./features/actions/ActionsFeature.svelte";
  import PrivacyFeature from "./features/privacy/PrivacyFeature.svelte";
  import SettingsView from "./features/settings/SettingsView.svelte";
  import InstalledAppIcon from "./InstalledAppIcon.svelte";
  import { bridge } from "./bridge";
  import { setLanguage, translate } from "./i18n";
  import { createCachedLoader } from "./lazyLoader";
  import { actionIconCatalog, sanitizeImportedSvg } from "./quickActionIcons";
  import type {
    ActionPreset,
    AppSettings,
    AppUpdateCheckResult,
    AppUpdateProgress,
    ActionView,
    BootstrapState,
    ConnectedDevice,
    InputMetrics,
    InstalledApp,
    LocalSharedDirectory,
    NearbyDesktop,
    PrivacySettings,
    ProtectedApp,
    TransferSnapshot,
    TransferStatus,
    SystemShareRequest,
  } from "./types";

  import {
    actionContentTitle,
    actionDescription,
    actionFromDraft,
    actionSummary,
    actionTypeOptions,
    detectWindowPlatform,
    draftFromAction,
    emptyActionDraft,
    errorMessage,
    mediaOperationLabel,
    permissionText,
    shortcutKeyFromKeyboardEvent,
    shortcutLabel,
    systemOperationLabel,
    type ActionDraft,
    type Page,
    type SettingsTab,
  } from "./app_helpers";

  type ConfirmationRequest = {
    title: string;
    description: string;
    confirmLabel: string;
    kind: "danger" | "primary";
    onConfirm: () => Promise<boolean | void> | boolean | void;
  };

  let snapshot: BootstrapState | null = null;
  const previewParams = new URLSearchParams(window.location.search);
  const requestedPage = previewParams.get("page");
  const requestedSettingsTab = previewParams.get("settingsTab");
  const settingsTabs = new Set<SettingsTab>(["connection", "files", "clipboard", "screenshot", "notifications", "general", "agent"]);
  let page: Page = requestedPage === "transfer"
    ? "transfer"
    : requestedPage === "files"
      ? "files"
      : requestedPage === "printers"
        ? "printers"
        : requestedPage === "input"
          ? "input"
        : requestedPage === "workflows" || requestedPage === "automations"
          ? "workflows"
        : requestedPage === "settings"
          ? "settings"
        : "actions";
  let settingsTab: SettingsTab = requestedSettingsTab && settingsTabs.has(requestedSettingsTab as SettingsTab)
    ? requestedSettingsTab as SettingsTab
    : "connection";
  let appSettings: AppSettings = {
    revision: 0,
    deviceName: "",
    theme: "system",
    language: "system",
    launchAtStartup: false,
    launchSilently: false,
    autoUpdateEnabled: true,
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
    webFiles: {
      enabled: false,
      port: 8767,
      bindMode: "lanOnly",
      siteName: "",
      sessionIdleMinutes: 720,
      allowVpnPrivate: false,
    },
  };
  let settingsSaving = false;
  let appUpdateStatus: AppUpdateCheckResult = { currentVersion: "0.1.0", update: null };
  let appUpdateProgress: AppUpdateProgress | null = null;
  let appUpdateChecking = false;
  let appUpdateInstalling = false;
  let localShares: LocalSharedDirectory[] = [];
  let localSharesLoaded = false;
  let localShareBusy = "";
  let webGatewayStatus: import("./types").WebGatewayStatus | null = null;
  let clipboardMergeBusy = false;
  let loading = true;
  let automationPage: { requestLeave(next: () => void): void } | undefined;
  let notificationsPage: { focusSearch(): void } | undefined;
  let NotificationsPage: typeof import("./NotificationsPage.svelte").default | undefined;
  let PrinterPage: typeof import("./PrinterPage.svelte").default | undefined;
  let RemoteFilesPage: typeof import("./RemoteFilesPage.svelte").default | undefined;
  let TransferPage: typeof import("./TransferWorkspace.svelte").default | undefined;
  let AutomationPage: typeof import("./AutomationPage.svelte").default | undefined;
  let InputSharingPage: typeof import("./features/input-sharing/InputSharingPage.svelte").default | undefined;
  let lazyPageErrors: Partial<Record<Page, string>> = {};
  const loadNotificationsPage = createCachedLoader(async () => (await import("./NotificationsPage.svelte")).default);
  const loadPrinterPage = createCachedLoader(async () => (await import("./PrinterPage.svelte")).default);
  const loadRemoteFilesPage = createCachedLoader(async () => (await import("./RemoteFilesPage.svelte")).default);
  const loadTransferPage = createCachedLoader(async () => (await import("./TransferWorkspace.svelte")).default);
  const loadAutomationPage = createCachedLoader(async () => (await import("./AutomationPage.svelte")).default);
  const loadInputSharingPage = createCachedLoader(async () => (await import("./features/input-sharing/InputSharingPage.svelte")).default);
  let confirmationRequest: ConfirmationRequest | null = null;
  let confirmationBusy = false;
  let toast = "";
  let toastKind: "success" | "error" = "success";
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let windowMaximized = false;
  let windowFocused = true;
  let transferStatuses = new Map<string, TransferStatus>();
  let transferOpenRequest: { transferId: string; sequence: number } | null = null;
  let remoteFileOpenSequence = 0;
  let remoteFileOpenRequest: { deviceId: string; sequence: number } | null = null;
  let systemShareOpenRequest: { request: SystemShareRequest; sequence: number } | null = null;
  let systemShareOpenSequence = 0;
  let transferOpenSequence = 0;
  let nearbyDesktops: NearbyDesktop[] = [];
  let nearbyDiscoveryStarted = false;
  let nearbyDiscoveryBusy = false;
  let connectingDesktopId = "";
  let pairingResponseBusy = false;
  let addDeviceGuideOpen = false;
  let appearanceMedia: MediaQueryList | undefined;
  let handleSystemThemeChange: (() => void) | undefined;
  const windowPlatform = detectWindowPlatform();
  const shortcutModifier = windowPlatform === "macos" ? "⌘" : "Ctrl";

  let actions: ActionView[] = [];
  $: actions = snapshot?.actions ?? [];
  $: void ensureLazyPage(page);

  onMount(() => {
    const scope = new SubscriptionScope();
    let disposed = false;
    let transferObserved = false;
    let modulesObserved = false;
    void (async () => {
      try {
        await scope.add(bridge.onAppSettingsChanged((settings) => {
          if (settings.revision >= appSettings.revision) { appSettings = settings; applyAppearance(settings); }
        }));
        await Promise.all([
          bridge.onState((state) => applySnapshot(state)),
          bridge.onRuntimeModules((modules) => { modulesObserved = true; moduleStatuses = modules; }),
          bridge.onInputMetrics((metrics: InputMetrics | null) => {
            if (snapshot) snapshot = { ...snapshot, inputMetrics: metrics };
          }),
          bridge.onPrivacyState((privacy) => {
            if (snapshot) snapshot = { ...snapshot, privacy };
          }),
          bridge.onTransferState((state) => { applyTransferNotifications(state, transferObserved); transferObserved = true; }),
          bridge.onOpenTransferRequest((transferId) => {
            openTransferRequest(transferId);
            void bridge.takePendingTransferRequest();
          }),
          bridge.onTrayNavigationPending(() => { void consumeTrayNavigation(); }),
          bridge.onOpenSystemShareRequest((request) => openSystemShareRequest(request)),
          bridge.onNotificationCount((count) => {
            if (snapshot) snapshot = { ...snapshot, unreadNotificationCount: count };
          }),
          bridge.onAppUpdateChecked((result) => { appUpdateStatus = result; }),
          bridge.onAppUpdateProgress((progress) => {
            appUpdateProgress = progress;
          }),
          bridge.onWebGatewayStatus((status) => {
            webGatewayStatus = status;
          }),
        ].map((registration) => scope.add(registration)));
        if (disposed) {
          return;
        }
        const [initialState, settings, maximized] = await Promise.all([
          bridge.getBootstrapState(), bridge.getAppSettings(), bridge.isWindowMaximized(),
        ]);
        void bridge.getRuntimeModules().then((modules) => { if (!disposed && !modulesObserved) moduleStatuses = modules; }).catch(console.error);
        void bridge.getWebGatewayStatus().then((status) => { if (!disposed) webGatewayStatus = status; }).catch((error) => showToast(errorMessage(error), "error"));
        if (disposed) return;
        applySnapshot(initialState);
        if (settings.revision >= appSettings.revision) appSettings = settings;
        applyAppearance(appSettings);
        appearanceMedia = window.matchMedia("(prefers-color-scheme: dark)");
        handleSystemThemeChange = () => appSettings.theme === "system" && applyAppearance(appSettings);
        appearanceMedia.addEventListener("change", handleSystemThemeChange);
        windowMaximized = maximized;
        await consumeTrayNavigation();
        const pendingTransferRequest = await bridge.takePendingTransferRequest();
        if (pendingTransferRequest) openTransferRequest(pendingTransferRequest);
        const pendingSystemShares = await bridge.listSystemShareRequests();
        for (const request of pendingSystemShares) openSystemShareRequest(request);
        if ("__TAURI_INTERNALS__" in window) {
          const currentWindow = getCurrentWebviewWindow();
          await scope.add(currentWindow.onResized(async () => {
            windowMaximized = await bridge.isWindowMaximized();
          }));
          await scope.add(currentWindow.onFocusChanged(({ payload }) => {
            windowFocused = payload;
          }));
        }
      } catch (error) {
        showToast(errorMessage(error), "error");
      } finally {
        if (!disposed) loading = false;
      }
    })();

    return () => {
      disposed = true;
      scope.dispose();
      if (appearanceMedia && handleSystemThemeChange) appearanceMedia.removeEventListener("change", handleSystemThemeChange);
    };
  });

  function applySnapshot(update: DesktopStateUpdate) {
    const state = desktopProjection.apply(update);
    if (!state) return;
    const pairingJustArrived = Boolean(state.pendingPairing && !snapshot?.pendingPairing);
    snapshot = state;
    const connectingDesktopIds = new Set(state.outgoingPairings.map((pairing) => pairing.deviceId));
    const connectedDesktopIds = new Set(state.connectedDevices.map((device) => device.id));
    nearbyDesktops = nearbyDesktops.map((device) => ({
      ...device,
      connecting: !connectedDesktopIds.has(device.deviceId) && connectingDesktopIds.has(device.deviceId),
    }));
    if (pairingJustArrived && !requestedPage && page !== "workflows") {
      page = "settings";
      settingsTab = "connection";
    }
  }

  function applyTransferNotifications(state: TransferSnapshot, announce: boolean) {
    const nextStatuses = new Map<string, TransferStatus>();
    for (const transfer of state.transfers) {
      nextStatuses.set(transfer.id, transfer.status);
      if (
        transfer.direction !== "receiving" ||
        transferStatuses.get(transfer.id) === transfer.status
      ) {
        continue;
      }

      const subject = transfer.files.length === 1
        ? `“${transfer.files[0].name}”`
        : t("{count} 个文件", appSettings.language, { count: transfer.files.length });
      if (transfer.status === "awaitingApproval") {
        if (announce) showToast(t("{name} 请求发送 {subject}", appSettings.language, { name: transfer.peerName, subject: subject }));
      } else if (transfer.status === "completed" && announce) {
        showToast(t("已接收来自 {name} 的{subject}", appSettings.language, { name: transfer.peerName, subject: subject }));
      }
    }
    transferStatuses = nextStatuses;
  }

  function navigatePage(next: Page, after: () => void = () => {}) {
    const change = () => { page = next; after(); };
    if (page === "workflows" && next !== page && automationPage) automationPage.requestLeave(change);
    else change();
  }

  function openTransferRequest(transferId: string) {
    transferOpenSequence += 1;
    transferOpenRequest = { transferId, sequence: transferOpenSequence };
    navigatePage("transfer");
  }

  function openSystemShareRequest(request: SystemShareRequest) {
    systemShareOpenSequence += 1;
    systemShareOpenRequest = { request, sequence: systemShareOpenSequence };
    navigatePage("transfer");
  }

  async function consumeTrayNavigation() {
    try {
      const destination = parseTrayDestination(await bridge.takePendingTrayNavigation());
      if (destination?.page === "settings") openSettings("general");
      else if (destination?.page === "devices") openSettings("connection");
      else if (destination?.page === "transfers") navigatePage("transfer", () => { transferOpenRequest = null; });
      else if (destination?.page === "files") navigatePage("files", () => {
        remoteFileOpenRequest = { deviceId: destination.peerId, sequence: ++remoteFileOpenSequence };
      });
    } catch (error) { showToast(errorMessage(error), "error"); }
  }

  function openSettings(tab: SettingsTab = settingsTab) {
    navigatePage("settings", () => {
      settingsTab = tab;
      if (tab === "files") void loadLocalShares();
    });
  }

  async function loadLocalShares() {
    if (localShareBusy === "load") return;
    localShareBusy = "load";
    try {
      localShares = await bridge.getLocalFileShares();
      localSharesLoaded = true;
    } catch (error) {
      showToast(errorMessage(error), "error");
    } finally {
      localShareBusy = "";
    }
  }

  async function addLocalShare() {
    if (localShareBusy) return;
    localShareBusy = "add";
    try {
      const share = await bridge.addRemoteFileShare();
      if (!share) return;
      localShares = [...localShares.filter((item) => item.id !== share.id), share];
      localSharesLoaded = true;
      showToast(`已共享“${share.name}”`);
    } catch (error) {
      showToast(errorMessage(error), "error");
    } finally {
      localShareBusy = "";
    }
  }

  async function toggleLocalShareWriteAccess(share: LocalSharedDirectory) {
    if (localShareBusy) return;
    localShareBusy = share.id;
    try {
      const updated = await bridge.setRemoteFileShareWritable(share.id, !share.writable);
      localShares = localShares.map((item) => item.id === updated.id ? updated : item);
      showToast(updated.writable ? "已允许远程写入" : "已改为只读");
    } catch (error) {
      showToast(errorMessage(error), "error");
    } finally {
      localShareBusy = "";
    }
  }

  async function updateLocalShareWebAccess(
    share: LocalSharedDirectory,
    policy: { mode: import("./types").WebAccessMode; listed: boolean; allowPreview: boolean; allowDownload: boolean },
    newPassword?: string,
  ) {
    if (localShareBusy) return;
    localShareBusy = share.id;
    try {
      const updated = await bridge.setRemoteFileShareWebPolicy(share.id, policy, newPassword);
      localShares = localShares.map((item) => item.id === updated.id ? updated : item);
      showToast(policy.mode === "disabled" ? "已关闭浏览器访问" : policy.mode === "public" ? "已设为局域网公开" : "密码访问已更新");
    } catch (error) {
      showToast(errorMessage(error), "error");
      throw error;
    } finally {
      localShareBusy = "";
    }
  }

  async function refreshWebGatewayStatus() {
    webGatewayStatus = await bridge.getWebGatewayStatus();
  }

  async function revokeWebAccess(shareId?: string) {
    try {
      const count = await bridge.revokeWebSessions(shareId);
      await refreshWebGatewayStatus();
      showToast(count > 0 ? `已撤销 ${count} 个网页会话` : "当前没有可撤销的网页会话");
    } catch (error) {
      showToast(errorMessage(error), "error");
    }
  }

  async function removeLocalShare(share: LocalSharedDirectory) {
    if (localShareBusy) return;
    localShareBusy = share.id;
    try {
      localShares = await bridge.removeRemoteFileShare(share.id);
      showToast("已停止共享");
    } catch (error) {
      showToast(errorMessage(error), "error");
      throw error;
    } finally {
      localShareBusy = "";
    }
  }

  function applyAppearance(settings: AppSettings) {
    const resolvedTheme = settings.theme === "system"
      ? (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light")
      : settings.theme;
    document.documentElement.dataset.theme = resolvedTheme;
    document.documentElement.style.colorScheme = resolvedTheme;
    const themeColor = resolvedTheme === "dark" ? "#1c1e24" : "#ffffff";
    document.querySelector<HTMLMetaElement>('meta[name="theme-color"]')?.setAttribute("content", themeColor);
    setLanguage(settings.language);
  }

  function tr(source: string) {
    return translate(source, appSettings.language);
  }

  async function patchAppSettings(patch: import("./ipc/generated").AppSettingsPatch, message = "设置已保存") {
    if (settingsSaving) return;
    settingsSaving = true;
    const previous = appSettings;
    const next = { ...appSettings, ...patch, notifications: { ...appSettings.notifications, ...patch.notifications }, sounds: { ...appSettings.sounds, ...patch.sounds }, webFiles: { ...appSettings.webFiles, ...patch.webFiles } };
    appSettings = next;
    applyAppearance(next);
    try {
      const saved = await bridge.updateAppSettings(patch);
      if (saved.revision >= appSettings.revision) appSettings = saved;
      if (patch.webFiles || patch.deviceName !== undefined) await refreshWebGatewayStatus();
      applyAppearance(appSettings);
      showToast(tr(message));
    } catch (error) {
      if (appSettings.revision === previous.revision) appSettings = previous;
      applyAppearance(appSettings);
      showToast(errorMessage(error), "error");
    } finally {
      settingsSaving = false;
    }
  }

  async function testSystemNotification() {
    try {
      showToast(await bridge.showTestSystemNotification());
    } catch (error) {
      showToast(errorMessage(error), "error");
    }
  }

  async function checkForAppUpdate(silent = false) {
    if (appUpdateChecking || appUpdateInstalling) return;
    appUpdateChecking = true;
    const previousVersion = appUpdateStatus.update?.version;
    try {
      appUpdateStatus = await bridge.checkForAppUpdate();
      if (appUpdateStatus.update) {
        appUpdateProgress = null;
        if (!silent || appUpdateStatus.update.version !== previousVersion) {
          showToast(`发现新版本 ${appUpdateStatus.update.version}`);
        }
      } else if (!silent) {
        showToast("当前已是最新版本");
      }
    } catch (error) {
      if (!silent) showToast(errorMessage(error), "error");
    } finally {
      appUpdateChecking = false;
    }
  }

  function confirmInstallAppUpdate() {
    const update = appUpdateStatus.update;
    if (!update || appUpdateInstalling) return;
    const transferActive = [...transferStatuses.values()].some((status) =>
      ["preparing", "awaitingApproval", "connecting", "transferring", "paused"].includes(status));
    if (transferActive) {
      showToast("请先完成或取消正在进行的文件传输", "error");
      return;
    }
    requestConfirmation({
      title: `安装 ArcRelay ${update.version}？`,
      description: "更新包会先经过签名验证。安装期间 ArcRelay 将退出并重新启动，请先保存正在编辑的工作。",
      confirmLabel: "安装并重启",
      kind: "primary",
      onConfirm: installAppUpdate,
    });
  }

  async function installAppUpdate() {
    if (appUpdateInstalling) return false;
    appUpdateInstalling = true;
    appUpdateProgress = { phase: "downloading", downloadedBytes: 0, totalBytes: null };
    try {
      await bridge.installAppUpdate();
      return true;
    } catch (error) {
      showToast(errorMessage(error), "error");
      return false;
    } finally {
      appUpdateInstalling = false;
    }
  }

  async function openSniptraSettings() {
    try {
      showToast(await bridge.openSniptraSettings());
    } catch (error) {
      showToast(errorMessage(error));
    }
  }

  function captureClipboardShortcut(event: KeyboardEvent) {
    event.preventDefault();
    const ignored = ["Meta", "Control", "Alt", "Shift"];
    if (ignored.includes(event.key)) return;
    const modifiers: string[] = [];
    if (event.ctrlKey) modifiers.push("Ctrl");
    if (event.metaKey) modifiers.push(windowPlatform === "macos" ? "Cmd" : "Super");
    if (event.altKey) modifiers.push("Alt");
    if (event.shiftKey) modifiers.push("Shift");
    const key = shortcutKeyFromKeyboardEvent(event);
    if (!modifiers.length) {
      showToast(tr("全局快捷键至少需要一个修饰键"), "error");
      return;
    }
    void patchAppSettings({ clipboardShortcut: [...modifiers, key].join("+") }, "剪贴板快捷键已更新");
  }

  function captureScreenshotShortcut(event: KeyboardEvent) {
    event.preventDefault();
    const ignored = ["Meta", "Control", "Alt", "Shift"];
    if (ignored.includes(event.key)) return;
    const modifiers: string[] = [];
    if (event.ctrlKey) modifiers.push("Ctrl");
    if (event.metaKey) modifiers.push(windowPlatform === "macos" ? "Cmd" : "Super");
    if (event.altKey) modifiers.push("Alt");
    if (event.shiftKey) modifiers.push("Shift");
    const key = shortcutKeyFromKeyboardEvent(event);
    if (!modifiers.length) {
      showToast(tr("全局快捷键至少需要一个修饰键"), "error");
      return;
    }
    void patchAppSettings({ screenshotShortcut: [...modifiers, key].join("+") }, "截图快捷键已更新");
  }

  async function testScreenshotCapture() {
    try {
      showToast(await bridge.startScreenshotCapture());
    } catch (error) {
      showToast(errorMessage(error), "error");
    }
  }

  async function mergeClipboardDeviceHistory() {
    if (clipboardMergeBusy) return;
    clipboardMergeBusy = true;
    try {
      await bridge.mergeClipboardDevices();
      showToast(tr("设备历史已合并"));
    } catch (error) {
      showToast(errorMessage(error), "error");
    } finally {
      clipboardMergeBusy = false;
    }
  }

  function confirmClearClipboardHistory() {
    requestConfirmation({
      title: "清空剪贴板记录？",
      description: "这会删除全部本地剪贴板记录，包括收藏和已归类内容，且无法撤销。",
      confirmLabel: "清空记录",
      kind: "danger",
      onConfirm: async () => {
        await bridge.clearClipboardHistory();
        showToast(tr("剪贴板记录已清空"));
      },
    });
  }

  async function copyConnectionInfo() {
    if (!snapshot) return;
    try {
      await navigator.clipboard.writeText(
        `ArcRelay\n端口：${snapshot.port}\n加密：TLS 1.3\n状态：${snapshot.serverRunning ? "已就绪" : "已停止"}`,
      );
      showToast(tr("诊断信息已复制"));
    } catch (error) {
      showToast(errorMessage(error), "error");
    }
  }

  async function refreshNearbyDesktops() {
    if (nearbyDiscoveryBusy) return;
    nearbyDiscoveryBusy = true;
    nearbyDiscoveryStarted = true;
    try {
      nearbyDesktops = await bridge.discoverDesktopDevices();
    } catch (error) {
      showToast(errorMessage(error), "error");
    } finally {
      nearbyDiscoveryBusy = false;
    }
  }

  async function connectDesktop(device: NearbyDesktop) {
    if (connectingDesktopId) return;
    connectingDesktopId = device.deviceId;
    nearbyDesktops = nearbyDesktops.map((item) =>
      item.deviceId === device.deviceId ? { ...item, connecting: true } : item,
    );
    try {
      const deviceName = await bridge.connectDesktopDevice(device.deviceId);
      showToast(t("已向 {name} 发起连接请求", appSettings.language, { name: deviceName }));
    } catch (error) {
      nearbyDesktops = nearbyDesktops.map((item) =>
        item.deviceId === device.deviceId ? { ...item, connecting: false } : item,
      );
      showToast(errorMessage(error), "error");
    } finally {
      connectingDesktopId = "";
    }
  }

  async function connectDesktopAddress(host: string, port?: number) {
    if (connectingDesktopId) return;
    connectingDesktopId = "manual";
    try {
      const deviceName = await bridge.connectDesktopAddress(host, port);
      showToast(t("已向 {name} 发起连接请求", appSettings.language, { name: deviceName }));
    } catch (error) {
      showToast(errorMessage(error), "error");
    } finally {
      connectingDesktopId = "";
    }
  }

  async function respondToPairing(accepted: boolean, approvedGrantIds?: string[]) {
    if (pairingResponseBusy) return;
    pairingResponseBusy = true;
    try {
      await bridge.respondPairing(accepted, approvedGrantIds);
      showToast(accepted ? "设备已允许连接" : "已拒绝连接请求");
    } catch (error) {
      showToast(errorMessage(error), "error");
    } finally {
      pairingResponseBusy = false;
    }
  }

  function confirmForgetPairedDevice(device: ConnectedDevice) {
    requestConfirmation({
      title: "忘记这台设备？",
      description: `忘记“${device.name}”后，下次连接时需要重新确认。`,
      confirmLabel: "忘记设备",
      kind: "danger",
      onConfirm: async () => {
        await bridge.forgetPairedDevice(device.id);
        showToast(`已忘记“${device.name}”`);
      },
    });
  }

  async function setDeviceAutoConnect(device: ConnectedDevice, enabled: boolean) {
    try {
      await bridge.setDeviceAutoConnect(device.id, enabled);
      showToast(enabled ? `已为“${device.name}”开启自动连接` : `已为“${device.name}”关闭自动连接`);
    } catch (error) {
      showToast(errorMessage(error), "error");
    }
  }

  function confirmQuitApp() {
    requestConfirmation({
      title: "完全退出 ArcRelay？",
      description: "退出后，设备连接、剪贴板同步和附近传输都会停止。",
      confirmLabel: "完全退出",
      kind: "danger",
      onConfirm: () => bridge.quitApp(),
    });
  }

  async function ensureLazyPage(target: Page) {
    try {
      switch (target) {
        case "notifications":
          NotificationsPage ??= await loadNotificationsPage();
          break;
        case "transfer":
          TransferPage ??= await loadTransferPage();
          break;
        case "files":
          RemoteFilesPage ??= await loadRemoteFilesPage();
          break;
        case "printers":
          PrinterPage ??= await loadPrinterPage();
          break;
        case "workflows":
          AutomationPage ??= await loadAutomationPage();
          break;
        case "input":
          InputSharingPage ??= await loadInputSharingPage();
          break;
      }
    } catch (error) {
      lazyPageErrors = { ...lazyPageErrors, [target]: errorMessage(error) };
      showToast(errorMessage(error), "error");
    }
  }

  function openWorkflowsPage() {
    navigatePage("workflows");
  }

  function openInputPage() {
    navigatePage("input");
  }

  function openPrivacyPage() {
    navigatePage("privacy");
  }


  function handleShortcut(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      if (page === "notifications") notificationsPage?.focusSearch();
    }
  }

  async function toggleWindowMaximize() {
    windowMaximized = await bridge.toggleMaximizeWindow();
  }

  function requestConfirmation(request: ConfirmationRequest) {
    if (confirmationBusy) return;
    confirmationRequest = request;
  }

  async function confirmRequestedAction() {
    if (!confirmationRequest || confirmationBusy) return;
    const request = confirmationRequest;
    confirmationBusy = true;
    try {
      const shouldClose = await request.onConfirm();
      if (shouldClose !== false) confirmationRequest = null;
    } catch (error) {
      showToast(errorMessage(error), "error");
    } finally {
      confirmationBusy = false;
    }
  }

  function showToast(message: string, kind: "success" | "error" = "success") {
    toast = message;
    toastKind = kind;
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = ""), 2600);
  }

  function showInputToast(message: string, error = false) {
    showToast(message, error ? "error" : "success");
  }

</script>

<svelte:window on:keydown={handleShortcut} />

<div
  class="window-frame"
  class:window-inactive={!windowFocused}
  class:window-maximized={windowMaximized}
  data-platform={windowPlatform}
>
  <header
    class="custom-titlebar"
    data-tauri-drag-region
    role="toolbar"
    aria-label={uiTranslate("窗口标题栏", $uiLanguage)}
    tabindex="-1"
    on:dblclick={toggleWindowMaximize}
  >
    <div class="titlebar-leading" data-tauri-drag-region>
      {#if windowPlatform !== "macos"}
        <div class="titlebar-app-identity" data-tauri-drag-region>
          <span class="titlebar-app-mark"><BrandLogo size={16} /></span>
          <span>ArcRelay</span>
        </div>
      {/if}
    </div>

    <div class="titlebar-main" data-tauri-drag-region>
      {#if windowPlatform !== "macos"}
        <div class="windows-window-controls" aria-label={uiTranslate("窗口控制", $uiLanguage)}>
          <button aria-label={uiTranslate("最小化窗口", $uiLanguage)} title={uiTranslate("最小化", $uiLanguage)} on:click={() => bridge.minimizeWindow()}><Minus size={16} /></button>
          <button aria-label={uiTranslate((windowMaximized ? "还原窗口" : "最大化窗口"), $uiLanguage)} title={uiTranslate((windowMaximized ? "还原" : "最大化"), $uiLanguage)} on:click={toggleWindowMaximize}>
            {#if windowMaximized}<CopySimple size={15} />{:else}<Square size={14} />{/if}
          </button>
          <button class="windows-close" aria-label={uiTranslate("关闭窗口", $uiLanguage)} title={uiTranslate("关闭", $uiLanguage)} on:click={() => bridge.closeWindow()}><X size={17} /></button>
        </div>
      {/if}
    </div>
  </header>

  <div class="app-shell">
  <aside class="sidebar">
    <div class="brand">
      <span class="brand-mark"><BrandLogo size={24} /></span>
      <span>ArcRelay</span>
    </div>

    <nav class="primary-nav" aria-label={uiTranslate("主要导航", $uiLanguage)}>
      <button class:active={page === "actions"} on:click={() => navigatePage("actions")}>
        <Lightning size={21} weight={page === "actions" ? "fill" : "regular"} />
        <span>{uiTranslate("快捷动作", $uiLanguage)}</span>
      </button>
      <button class:active={page === "notifications"} on:click={() => navigatePage("notifications")}>
        <Bell size={21} weight={page === "notifications" ? "fill" : "regular"} />
        <span>{uiTranslate("通知", $uiLanguage)}</span>
        {#if (snapshot?.unreadNotificationCount ?? 0) > 0}
          <span class="nav-count">{snapshot?.unreadNotificationCount}</span>
        {/if}
      </button>
      <button class:active={page === "workflows"} on:click={openWorkflowsPage}>
        <SquaresFour size={21} weight={page === "workflows" ? "fill" : "regular"} />
        <span>{uiTranslate("自动化", $uiLanguage)}</span>
      </button>
      <button class:active={page === "privacy"} on:click={openPrivacyPage}>
        <Shield size={21} weight={page === "privacy" ? "fill" : "regular"} />
        <span>{uiTranslate("投屏隐私", $uiLanguage)}</span>
        {#if snapshot?.privacy.active}
          <span class="privacy-nav-dot" aria-label={uiTranslate("已开启", $uiLanguage)}></span>
        {/if}
      </button>
      <button class:active={page === "transfer"} on:click={() => navigatePage("transfer")}>
        <PaperPlaneTilt size={21} weight={page === "transfer" ? "fill" : "regular"} />
        <span>{uiTranslate("附近传输", $uiLanguage)}</span>
      </button>
      <button class:active={page === "files"} on:click={() => navigatePage("files")}>
        <HardDrives size={21} weight={page === "files" ? "fill" : "regular"} />
        <span>{uiTranslate("远程文件", $uiLanguage)}</span>
      </button>
      <button class:active={page === "printers"} on:click={() => navigatePage("printers")}>
        <Printer size={21} weight={page === "printers" ? "fill" : "regular"} />
        <span>{uiTranslate("打印机共享", $uiLanguage)}</span>
      </button>
      <button class:active={page === "input"} on:click={openInputPage}>
        <Keyboard size={21} weight={page === "input" ? "fill" : "regular"} />
        <span>{uiTranslate("跨屏输入", $uiLanguage)}</span>
      </button>
      <button class:active={page === "settings"} on:click={() => openSettings()}>
        <Gear size={21} weight={page === "settings" ? "fill" : "regular"} />
        <span>{uiTranslate("设置", $uiLanguage)}</span>
        {#if snapshot?.pendingPairing}
          <span class="pairing-nav-dot" aria-label={uiTranslate("有新的配对请求", $uiLanguage)}></span>
        {/if}
      </button>
    </nav>

    <div class="sidebar-status">
      <div>
        <span class:online={snapshot?.serverRunning} class="status-dot"></span>
        <span>{uiTranslate(snapshot?.serverRunning ? "服务运行中" : "服务已停止", $uiLanguage)}</span>
        {#each moduleStatuses.filter((module) => module.state === "unavailable") as module}
          <span title={module.error ?? ""} role="status">{uiTranslate(module.name === "print" ? "打印服务暂不可用" : "文件传输正在重试", $uiLanguage)}</span>
        {/each}
      </div>
      <button
        type="button"
        class="sidebar-status-link"
        aria-label={uiTranslate((`打开设备与连接，当前 ${snapshot?.connectedDevices.length ?? 0} 台设备已连接`), $uiLanguage)}
        on:click={() => openSettings("connection")}
      >
        <DesktopTower size={17} />
        <span>{uiTranslate(`${snapshot?.connectedDevices.length ?? 0} 台设备已连接`, $uiLanguage)}</span>
      </button>
    </div>
  </aside>

  {#if loading}
    <main class="loading-state">
      <span class="loading-spinner"></span>
      <p>{uiTranslate("正在连接本地服务…", $uiLanguage)}</p>
    </main>
  {:else if page === "transfer"}
    {#if TransferPage}
      <svelte:component this={TransferPage} notify={showToast} openRequest={transferOpenRequest} systemShareRequest={systemShareOpenRequest} />
    {:else}
      <main class="loading-state">
        <span class="loading-spinner"></span>
        <p>{uiTranslate(lazyPageErrors.transfer ?? "正在加载附近传输…", $uiLanguage)}</p>
      </main>
    {/if}
  {:else if page === "files"}
    {#if RemoteFilesPage}
      <svelte:component this={RemoteFilesPage} notify={showToast} language={appSettings.language} openDeviceRequest={remoteFileOpenRequest} />
    {:else}
      <main class="loading-state">
        <span class="loading-spinner"></span>
        <p>{uiTranslate(lazyPageErrors.files ?? "正在加载远程文件…", $uiLanguage)}</p>
      </main>
    {/if}
  {:else if page === "printers"}
    {#if PrinterPage}
      <svelte:component this={PrinterPage} notify={showToast} />
    {:else}
      <main class="loading-state">
        <span class="loading-spinner"></span>
        <p>{uiTranslate(lazyPageErrors.printers ?? "正在加载打印机共享…", $uiLanguage)}</p>
      </main>
    {/if}
  {:else if page === "input"}
    {#if InputSharingPage}
      <svelte:component this={InputSharingPage} notify={showInputToast} />
    {:else}
      <main class="loading-state">
        <span class="loading-spinner"></span>
        <p>{uiTranslate(lazyPageErrors.input ?? "正在加载跨屏输入…", $uiLanguage)}</p>
      </main>
    {/if}
  {:else if page === "actions"}
    <ActionsFeature
      {snapshot}
      language={appSettings.language}
      {shortcutModifier}
      notify={showToast}
      confirm={requestConfirmation}
      onSnapshot={applySnapshot}
    />
  {:else if page === "notifications"}
    {#if NotificationsPage}
      <svelte:component
        this={NotificationsPage}
        bind:this={notificationsPage}
        language={appSettings.language}
        {shortcutModifier}
        notify={showToast}
        confirm={requestConfirmation}
      />
    {:else}
      <main class="loading-state">
        <span class="loading-spinner"></span>
        <p>{uiTranslate(lazyPageErrors.notifications ?? "正在加载通知…", $uiLanguage)}</p>
      </main>
    {/if}
  {:else if page === "workflows"}
    {#if AutomationPage}
      <svelte:component
        this={AutomationPage}
        bind:this={automationPage}
        openDevices={() => openSettings("connection")}
        {actions}
        language={appSettings.language}
        notify={showToast}
        confirm={requestConfirmation}
      />
    {:else}
      <main class="loading-state">
        <span class="loading-spinner"></span>
        <p>{uiTranslate(lazyPageErrors.workflows ?? "正在加载自动化…", $uiLanguage)}</p>
      </main>
    {/if}
  {:else if page === "privacy"}
    <PrivacyFeature
      {snapshot}
      language={appSettings.language}
      notify={showToast}
      confirm={requestConfirmation}
      onSnapshot={applySnapshot}
    />
  {:else}
    <SettingsView
      {snapshot}
      bind:settingsTab
      {appSettings}
      {settingsSaving}
      {appUpdateStatus}
      {appUpdateProgress}
      {appUpdateChecking}
      {appUpdateInstalling}
      {localShares}
      {localSharesLoaded}
      {localShareBusy}
      {webGatewayStatus}
      {clipboardMergeBusy}
      {nearbyDesktops}
      {nearbyDiscoveryStarted}
      {nearbyDiscoveryBusy}
      {connectingDesktopId}
      {pairingResponseBusy}
      bind:addDeviceGuideOpen
      {loadLocalShares}
      {addLocalShare}
      {toggleLocalShareWriteAccess}
      {updateLocalShareWebAccess}
      {refreshWebGatewayStatus}
      {revokeWebAccess}
      {removeLocalShare}
      {patchAppSettings}
      {testSystemNotification}
      {checkForAppUpdate}
      {confirmInstallAppUpdate}
      {captureClipboardShortcut}
      {captureScreenshotShortcut}
      {testScreenshotCapture}
      {mergeClipboardDeviceHistory}
      {confirmClearClipboardHistory}
      {copyConnectionInfo}
      {refreshNearbyDesktops}
      {connectDesktop}
      {connectDesktopAddress}
      {respondToPairing}
      {setDeviceAutoConnect}
      {confirmForgetPairedDevice}
      {confirmQuitApp}
    />
  {/if}
  </div>
</div>

<Dialog.Root
  open={confirmationRequest !== null}
  onOpenChange={(open) => {
    if (!open && !confirmationBusy) confirmationRequest = null;
  }}
>
  <Dialog.Portal>
    <Dialog.Overlay class="modal-backdrop" />
    <Dialog.Content class="modal confirmation-modal">
      <header class="modal-header">
        <div>
          <Dialog.Title class="modal-title" level={2}>{uiTranslate(confirmationRequest?.title ?? "确认操作", $uiLanguage)}</Dialog.Title>
          <Dialog.Description class="modal-description">
            {confirmationRequest?.description ?? ""}
          </Dialog.Description>
        </div>
        <Dialog.Close class="modal-close-button" aria-label={uiTranslate("关闭", $uiLanguage)} disabled={confirmationBusy}><X size={19} /></Dialog.Close>
      </header>
      <footer class="modal-footer">
        <Dialog.Close class="secondary-button" disabled={confirmationBusy}>{uiTranslate("取消", $uiLanguage)}</Dialog.Close>
        <button
          class={confirmationRequest?.kind === "danger" ? "danger-button" : "primary-button"}
          disabled={confirmationBusy}
          on:click={confirmRequestedAction}
        >
          {#if confirmationRequest?.kind === "danger"}<Trash size={17} />{:else}<Play size={17} weight="fill" />{/if}
          {uiTranslate(confirmationBusy ? "处理中…" : confirmationRequest?.confirmLabel ?? "确认", $uiLanguage)}
        </button>
      </footer>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>



{#if toast}
  <div class="toast" class:error={toastKind === "error"}>
    {#if toastKind === "success"}<CheckCircle size={18} weight="fill" />{:else}<WarningCircle size={18} weight="fill" />{/if}
    {toast}
  </div>
{/if}
