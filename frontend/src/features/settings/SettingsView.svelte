<script lang="ts">
  import AppSelect from "../../components/AppSelect.svelte";
  import { translate as uiTranslate, language as uiLanguage, languageOptions } from "../../i18n";
  import {
    Camera,
    Bell,
    CaretDown,
    CaretRight,
    CaretUp,
    ClipboardText,
    ClockCounterClockwise,
    Copy,
    DesktopTower,
    DeviceMobile,
    FileText,
    FolderOpen,
    GlobeHemisphereWest,
    HandTap,
    Keyboard,
    LinkSimpleHorizontal,
    MagnifyingGlass,
    Palette,
    PencilSimple,
    Plus,
    Power,
    RocketLaunch,
    ShieldCheck,
    Star,
    Trash,
    WifiHigh,
  } from "phosphor-svelte";

  import BrandLogo from "../../BrandLogo.svelte";
  import { bridge } from "../../bridge";
  import type {
    AppSettings,
    AppUpdateCheckResult,
    AppUpdateProgress,
    BootstrapState,
    ConnectedDevice,
    LocalSharedDirectory,
    NearbyDesktop,
    WebAccessMode,
    WebGatewayStatus,
  } from "../../types";
  import { permissionText, type SettingsTab } from "../../app_helpers";
  import McpSettings from "./McpSettings.svelte";
  import FileSharingSettings from "./FileSharingSettings.svelte";
  import SoundSettings from "./SoundSettings.svelte";

  export let snapshot: BootstrapState | null = null;
  export let settingsTab: SettingsTab;
  export let appSettings: AppSettings;
  export let settingsSaving = false;
  export let appUpdateStatus: AppUpdateCheckResult;
  export let appUpdateProgress: AppUpdateProgress | null = null;
  export let appUpdateChecking = false;
  export let appUpdateInstalling = false;
  export let localShares: LocalSharedDirectory[] = [];
  export let localSharesLoaded = false;
  export let localShareBusy = "";
  export let webGatewayStatus: WebGatewayStatus | null = null;
  export let clipboardMergeBusy = false;
  export let nearbyDesktops: NearbyDesktop[] = [];
  export let nearbyDiscoveryStarted = false;
  export let nearbyDiscoveryBusy = false;
  export let connectingDesktopId = "";
  export let pairingResponseBusy = false;
  export let addDeviceGuideOpen = false;

  export let loadLocalShares: () => void | Promise<void>;
  export let addLocalShare: () => void | Promise<void>;
  export let toggleLocalShareWriteAccess: (share: LocalSharedDirectory) => void | Promise<void>;
  export let updateLocalShareWebAccess: (
    share: LocalSharedDirectory,
    policy: { mode: WebAccessMode; listed: boolean; allowPreview: boolean; allowDownload: boolean },
    newPassword?: string,
  ) => void | Promise<void>;
  export let refreshWebGatewayStatus: () => void | Promise<void>;
  export let revokeWebAccess: (shareId?: string) => void | Promise<void>;
  export let removeLocalShare: (share: LocalSharedDirectory) => void | Promise<void>;
  export let patchAppSettings: (patch: import("../../ipc/generated").AppSettingsPatch, message?: string) => void | Promise<void>;
  export let testSystemNotification: () => void | Promise<void>;
  export let checkForAppUpdate: (silent?: boolean) => void | Promise<void>;
  export let confirmInstallAppUpdate: () => void;
  export let captureClipboardShortcut: (event: KeyboardEvent) => void;
  export let captureScreenshotShortcut: (event: KeyboardEvent) => void;
  export let testScreenshotCapture: () => void | Promise<void>;
  export let mergeClipboardDeviceHistory: () => void | Promise<void>;
  export let confirmClearClipboardHistory: () => void;
  export let copyConnectionInfo: () => void | Promise<void>;
  export let refreshNearbyDesktops: () => void | Promise<void>;
  export let connectDesktop: (device: NearbyDesktop) => void | Promise<void>;
  export let connectDesktopAddress: (host: string, port?: number) => void | Promise<void>;
  export let respondToPairing: (accepted: boolean, approvedGrantIds?: string[]) => void | Promise<void>;
  export let setDeviceAutoConnect: (device: ConnectedDevice, enabled: boolean) => void | Promise<void>;
  export let confirmForgetPairedDevice: (device: ConnectedDevice) => void;
  export let confirmQuitApp: () => void;

  let manualHost = "";
  let manualPort = "";
  let manualValidationError = "";
  const isMacPlatform = /Mac|iPhone|iPad/.test(navigator.platform);
  let selectedDeviceId = "";
  let autoConnectBusyId = "";
  let remoteDevices: ConnectedDevice[] = [];
  let selectedDevice: ConnectedDevice | undefined;
  let onlineDeviceIds = new Set<string>();
  let pairingSelectionDeviceId = "";
  let approvedPairingGrantIds = new Set<string>();
  $: if (snapshot?.pendingPairing?.deviceId !== pairingSelectionDeviceId) {
    pairingSelectionDeviceId = snapshot?.pendingPairing?.deviceId ?? "";
    approvedPairingGrantIds = new Set(snapshot?.pendingPairing?.grants.map((grant) => grant.id) ?? []);
  }
  function setPairingGrant(id: string, checked: boolean) {
    const next = new Set(approvedPairingGrantIds);
    if (checked) next.add(id);
    else next.delete(id);
    approvedPairingGrantIds = next;
  }
  $: onlineDeviceIds = new Set(snapshot?.connectedDevices.map((device) => device.id) ?? []);
  $: remoteDevices = [
    ...(snapshot?.pairedDevices ?? []),
    ...(snapshot?.connectedDevices ?? []).filter(
      (connected) => !(snapshot?.pairedDevices ?? []).some((paired) => paired.id === connected.id),
    ),
  ];
  $: if (
    !selectedDeviceId
    || (selectedDeviceId !== "__local__" && !remoteDevices.some((device) => device.id === selectedDeviceId))
  ) {
    selectedDeviceId = remoteDevices.find((device) => onlineDeviceIds.has(device.id))?.id
      ?? remoteDevices[0]?.id
      ?? "__local__";
  }
  $: selectedDevice = remoteDevices.find((device) => device.id === selectedDeviceId);
  function desktopConnected(deviceId: string) {
    return snapshot?.connectedDevices.some((device) => device.id === deviceId) ?? false;
  }

  function submitManualConnection() {
    const host = manualHost.trim();
    const port = manualPort.trim() === "" ? undefined : Number(manualPort);
    if (!host || /\s/.test(host) || host.length > 253 || (port !== undefined && (!Number.isInteger(port) || port < 1 || port > 65535))) {
      manualValidationError = "请输入有效的 IP 或主机名；端口可留空。";
      return;
    }
    manualValidationError = "";
    void connectDesktopAddress(host, port);
  }

  async function toggleSelectedAutoConnect() {
    if (!selectedDevice || autoConnectBusyId) return;
    autoConnectBusyId = selectedDevice.id;
    try {
      await setDeviceAutoConnect(selectedDevice, !selectedDevice.autoConnect);
    } finally {
      autoConnectBusyId = "";
    }
  }
</script>
    <main class="content full-content settings-page">
      <header class="settings-header">
        <h1>{uiTranslate("设置", $uiLanguage)}</h1>
        <nav class="settings-tabs" aria-label={uiTranslate("设置分类", $uiLanguage)}>
          <button class:active={settingsTab === "general"} on:click={() => (settingsTab = "general")}>{uiTranslate("通用", $uiLanguage)}</button>
          <button class:active={settingsTab === "connection"} on:click={() => (settingsTab = "connection")}>{uiTranslate("设备与连接", $uiLanguage)}</button>
          <button class:active={settingsTab === "files"} on:click={() => { settingsTab = "files"; void loadLocalShares(); }}>{uiTranslate("文件共享", $uiLanguage)}</button>
          <button class:active={settingsTab === "clipboard"} on:click={() => (settingsTab = "clipboard")}>{uiTranslate("剪贴板", $uiLanguage)}</button>
          <button class:active={settingsTab === "screenshot"} on:click={() => (settingsTab = "screenshot")}>{uiTranslate("截图", $uiLanguage)}</button>
          <button class:active={settingsTab === "notifications"} on:click={() => (settingsTab = "notifications")}>{uiTranslate("声音与通知", $uiLanguage)}</button>
          <button class:active={settingsTab === "agent"} on:click={() => (settingsTab = "agent")}>{uiTranslate("Agent 接入", $uiLanguage)}</button>
        </nav>
      </header>

      {#if settingsTab === "connection"}
        <section class="connection-settings connection-settings-v3" aria-labelledby="connection-settings-title-v3">
          <div class="connection-toolbar">
            <div class="connection-toolbar-copy">
              <strong id="connection-settings-title-v3"><span>{remoteDevices.length} {uiTranslate("台已配对设备", $uiLanguage)}</span><b>·</b><em>{snapshot?.connectedDevices.length ?? 0} {uiTranslate("台在线", $uiLanguage)}</em></strong>
            </div>
            <button
              class="primary-button add-device-button-v3"
              disabled={!snapshot?.serverRunning}
              aria-expanded={addDeviceGuideOpen}
              on:click={() => (addDeviceGuideOpen = !addDeviceGuideOpen)}
            >
              {#if addDeviceGuideOpen}<CaretUp size={17} /> {uiTranslate("收起", $uiLanguage)}{:else}<Plus size={17} /> {uiTranslate("添加设备", $uiLanguage)}{/if}
            </button>
          </div>

          {#if addDeviceGuideOpen}
            <section class="add-device-sheet" aria-label={uiTranslate("添加设备", $uiLanguage)}>
              <header>
                <div><strong>{uiTranslate("添加新设备", $uiLanguage)}</strong><span>{uiTranslate("先在另一台设备打开 ArcRelay，并连接到同一网络。", $uiLanguage)}</span></div>
              </header>
              <div class="add-device-sheet-grid">
                <div class="add-device-discovery">
                  <span class="sheet-method-icon"><WifiHigh size={20} /></span>
                  <span><strong>{uiTranslate("自动发现", $uiLanguage)}</strong><small>{uiTranslate("查找同一局域网中的 ArcRelay 设备", $uiLanguage)}</small></span>
                  <button class="secondary-button" disabled={nearbyDiscoveryBusy} on:click={refreshNearbyDesktops}>{uiTranslate(nearbyDiscoveryBusy ? "正在查找…" : "查找附近设备", $uiLanguage)}</button>
                </div>
                <form class="add-device-manual" on:submit|preventDefault={submitManualConnection}>
                  <label><span>{uiTranslate("IP 地址或主机名", $uiLanguage)}</span><input bind:value={manualHost} aria-label={uiTranslate("对端 IP 或主机名", $uiLanguage)} placeholder={uiTranslate("192.168.1.10 或 hostname.local", $uiLanguage)} /></label>
                  <label class="manual-port-field"><span>{uiTranslate("端口（可选）", $uiLanguage)}</span><input bind:value={manualPort} aria-label={uiTranslate("连接端口", $uiLanguage)} inputmode="numeric" placeholder="8765–8775" /></label>
                  <button class="primary-button" type="submit" disabled={connectingDesktopId === "manual"}>{uiTranslate(connectingDesktopId === "manual" ? "连接中…" : "连接", $uiLanguage)}</button>
                  {#if manualValidationError}<small class="field-error">{manualValidationError}</small>{/if}
                </form>
              </div>
              {#if nearbyDesktops.length > 0}
                <div class="sheet-nearby-list">
                  {#each nearbyDesktops as device (device.deviceId)}
                    <div>
                      <span class="device-name-cell"><span class="device-row-icon"><DesktopTower size={18} /></span><span><strong>{device.deviceName}</strong><small>{uiTranslate(device.paired ? "已配对" : "新设备", $uiLanguage)}</small></span></span>
                      <button class="primary-button" disabled={desktopConnected(device.deviceId) || device.connecting || Boolean(connectingDesktopId)} on:click={() => connectDesktop(device)}>{uiTranslate(desktopConnected(device.deviceId) ? "已连接" : device.connecting ? "连接中…" : device.paired ? "连接" : "配对", $uiLanguage)}</button>
                    </div>
                  {/each}
                </div>
              {:else if nearbyDiscoveryStarted && !nearbyDiscoveryBusy}
                <div class="sheet-empty-state"><strong>{uiTranslate("没有发现附近电脑", $uiLanguage)}</strong><span>{uiTranslate("请确认另一台电脑已打开 ArcRelay，并连接同一网络。", $uiLanguage)}</span></div>
              {/if}
            </section>
          {/if}

          {#if snapshot?.pendingPairing}
            <section class="pairing-banner settings-pairing-banner">
              <div class="pairing-icon"><DeviceMobile size={26} /></div>
              <div class="pairing-copy"><strong>{uiTranslate("新设备请求连接", $uiLanguage)}</strong><span>{snapshot.pendingPairing.deviceName}</span><small>{uiTranslate("仅勾选需要授权的能力：", $uiLanguage)}</small><div class="pairing-grants">{#each snapshot.pendingPairing.grants as grant (grant.id)}<label><input type="checkbox" checked={approvedPairingGrantIds.has(grant.id)} on:change={(event) => setPairingGrant(grant.id, event.currentTarget.checked)} /> <span>{grant.label}</span></label>{/each}</div></div>
              <div class="pairing-code-block"><span>{uiTranslate("确认配对码一致", $uiLanguage)}</span><code>{snapshot.pendingPairing.pairingCode}</code></div>
              <div class="pairing-actions"><button class="primary-button" disabled={pairingResponseBusy} on:click={() => respondToPairing(true, Array.from(approvedPairingGrantIds))}>{uiTranslate(pairingResponseBusy ? "处理中…" : "允许", $uiLanguage)}</button><button class="secondary-button" disabled={pairingResponseBusy} on:click={() => respondToPairing(false, [])}>{uiTranslate("拒绝", $uiLanguage)}</button></div>
            </section>
          {/if}
          {#each snapshot?.outgoingPairings ?? [] as pairing (pairing.deviceId)}
            <section class="pairing-banner settings-pairing-banner">
              <div class="pairing-icon"><DesktopTower size={26} /></div>
              <div class="pairing-copy"><strong>{uiTranslate("等待对方确认", $uiLanguage)}</strong><span>{pairing.deviceName}</span><small>{uiTranslate("请确认另一台设备显示相同配对码。", $uiLanguage)}</small></div>
              <div class="pairing-code-block"><span>{uiTranslate("配对码", $uiLanguage)}</span><code>{pairing.pairingCode}</code></div>
            </section>
          {/each}

          <div class="device-master-detail">
            <aside class="device-master-pane" aria-label={uiTranslate("我的设备", $uiLanguage)}>
              <header class="device-master-header"><h2>{uiTranslate("我的设备", $uiLanguage)}</h2><span>{remoteDevices.length + 1}</span></header>
              <div class="device-master-list">
                <button class:selected={selectedDeviceId === "__local__"} type="button" on:click={() => (selectedDeviceId = "__local__")}>
                  <span class="master-device-icon"><DesktopTower size={20} /></span>
                  <span class="master-device-copy"><strong>{uiTranslate(appSettings.deviceName || "这台 Mac", $uiLanguage)}</strong><small><i class:offline={!snapshot?.serverRunning} class="device-status-dot"></i>{uiTranslate(snapshot?.serverRunning ? "可连接" : "服务停止", $uiLanguage)}</small></span>
                  <span class="local-device-badge">{uiTranslate("本机", $uiLanguage)}</span><CaretRight class="master-device-caret" size={16} />
                </button>
                {#each remoteDevices as device (device.id)}
                  <button class:selected={selectedDeviceId === device.id} type="button" on:click={() => (selectedDeviceId = device.id)}>
                    <span class="master-device-icon"><DeviceMobile size={20} /></span>
                    <span class="master-device-copy"><strong>{device.name}</strong><small class:offline={!onlineDeviceIds.has(device.id)}><i class:offline={!onlineDeviceIds.has(device.id)} class="device-status-dot"></i>{uiTranslate(onlineDeviceIds.has(device.id) ? "在线" : "离线", $uiLanguage)}</small></span>
                    {#if !device.autoConnect}<span class="manual-connect-badge">{uiTranslate("手动", $uiLanguage)}</span>{/if}<CaretRight class="master-device-caret" size={16} />
                  </button>
                {/each}
              </div>
            </aside>

            <article class="device-inspector" aria-live="polite">
              {#if selectedDevice}
                <header class="device-inspector-header">
                  <span class="inspector-device-icon"><DeviceMobile size={29} /></span>
                  <div><h2>{selectedDevice.name}</h2><span class:offline={!onlineDeviceIds.has(selectedDevice.id)} class="inspector-online-state"><i></i>{uiTranslate(onlineDeviceIds.has(selectedDevice.id) ? "在线" : "离线", $uiLanguage)}</span></div>
                </header>

                <dl class="device-connection-facts">
                  <div><dt>{uiTranslate("连接方式", $uiLanguage)}</dt><dd>{uiTranslate("局域网 · QUIC 安全连接", $uiLanguage)}</dd></div>
                  <div><dt>{uiTranslate("设备身份", $uiLanguage)}</dt><dd title={selectedDevice.id}>{selectedDevice.id}</dd></div>
                </dl>

                <section class="inspector-section">
                  <span class="inspector-section-icon"><HandTap size={20} /></span>
                  <div><strong>{uiTranslate("当前控制", $uiLanguage)}</strong><small>{uiTranslate(snapshot?.activeInputDevice?.id === selectedDevice.id ? "这台设备正在控制本机" : snapshot?.activeInputDevice ? `${snapshot.activeInputDevice.name} 正在控制本机` : "无设备正在控制", $uiLanguage)}</small></div>
                  <span class:active={snapshot?.activeInputDevice?.id === selectedDevice.id} class="inspector-state-pill">{uiTranslate(snapshot?.activeInputDevice?.id === selectedDevice.id ? "控制中" : "空闲", $uiLanguage)}</span>
                </section>

                <button
                  type="button"
                  class="inspector-section inspector-toggle-row"
                  role="switch"
                  aria-checked={selectedDevice.autoConnect}
                  disabled={autoConnectBusyId === selectedDevice.id}
                  on:click={toggleSelectedAutoConnect}
                >
                  <span class="inspector-section-icon"><LinkSimpleHorizontal size={20} /></span>
                  <div><strong>{uiTranslate("自动连接", $uiLanguage)}</strong><small>{uiTranslate(selectedDevice.autoConnect ? "设备可用时自动重新连接" : "仅在你点击连接时建立连接", $uiLanguage)}</small></div>
                  <span class:checked={selectedDevice.autoConnect} class="switch-control"><span></span></span>
                </button>

                <div class="device-inspector-actions">
                  {#if onlineDeviceIds.has(selectedDevice.id)}
                    <button class="secondary-button" on:click={() => bridge.disconnectDevice(selectedDevice?.id ?? "")}><Power size={17} /> {uiTranslate("断开", $uiLanguage)}</button>
                  {:else}
                    <button class="secondary-button" disabled={Boolean(connectingDesktopId)} on:click={() => connectDesktop({ deviceId: selectedDevice?.id ?? "", deviceName: selectedDevice?.name ?? "", host: "", paired: true, connecting: false })}><LinkSimpleHorizontal size={17} /> {uiTranslate(connectingDesktopId === selectedDevice.id ? "连接中…" : "重新连接", $uiLanguage)}</button>
                  {/if}
                  <button class="secondary-button inspector-forget-button" on:click={() => selectedDevice && confirmForgetPairedDevice(selectedDevice)}><Trash size={17} /> {uiTranslate("忘记设备", $uiLanguage)}</button>
                </div>
              {:else}
                <header class="device-inspector-header local-inspector-header">
                  <span class="inspector-device-icon"><DesktopTower size={29} /></span>
                  <div><h2>{uiTranslate(appSettings.deviceName || "这台 Mac", $uiLanguage)}</h2><span class:offline={!snapshot?.serverRunning} class="inspector-online-state"><i></i>{uiTranslate(snapshot?.serverRunning ? "本机可连接" : "连接服务已停止", $uiLanguage)}</span></div>
                </header>
                <dl class="device-connection-facts"><div><dt>{uiTranslate("网络端口", $uiLanguage)}</dt><dd>{snapshot?.port}</dd></div><div><dt>{uiTranslate("安全连接", $uiLanguage)}</dt><dd>QUIC · TLS 1.3</dd></div></dl>
                <section class="inspector-section"><span class="inspector-section-icon"><HandTap size={20} /></span><div><strong>{uiTranslate("当前控制", $uiLanguage)}</strong><small>{uiTranslate(snapshot?.activeInputDevice ? `${snapshot.activeInputDevice.name} 正在控制本机` : "无设备正在控制", $uiLanguage)}</small></div><span class:active={Boolean(snapshot?.activeInputDevice)} class="inspector-state-pill">{uiTranslate(snapshot?.activeInputDevice ? "控制中" : "空闲", $uiLanguage)}</span></section>
                <div class="local-inspector-cta"><strong>{uiTranslate("连接另一台设备", $uiLanguage)}</strong><span>{uiTranslate("添加设备后，可在这里管理连接与控制状态。", $uiLanguage)}</span><button class="primary-button" on:click={() => (addDeviceGuideOpen = true)}><Plus size={16} /> {uiTranslate("添加设备", $uiLanguage)}</button></div>
              {/if}

              <details class="inspector-settings-disclosure">
                <summary>{uiTranslate("连接设置", $uiLanguage)} <CaretDown size={17} /></summary>
                <div class="inspector-settings-body">
                  <label class="inspector-setting-field"><span><strong>{uiTranslate("本机设备名称", $uiLanguage)}</strong><small>{uiTranslate("其他设备会通过这个名称识别本机", $uiLanguage)}</small></span><input class="device-name-input" value={appSettings.deviceName} maxlength="128" disabled={settingsSaving} placeholder={uiTranslate("使用系统主机名", $uiLanguage)} aria-label={uiTranslate("设备名称", $uiLanguage)} on:change={(event) => patchAppSettings({ deviceName: event.currentTarget.value.trim() }, "设备名称已更新，其他设备将自动同步")} /></label>
                  <div class:permission-warning={snapshot?.inputPermission !== "Granted"} class="inspector-setting-row"><span><strong>{uiTranslate("控制权限", $uiLanguage)}</strong><small>{uiTranslate(snapshot?.inputPermission === "Granted" ? "已开启，连接的设备可以控制键盘和指针" : permissionText(snapshot?.inputPermission), $uiLanguage)}</small></span><button class="text-button" on:click={() => bridge.openInputPermissionSettings()}>{uiTranslate(snapshot?.inputPermission === "Granted" ? "查看" : "去开启", $uiLanguage)}</button></div>
                  <button class="inspector-setting-row inspector-setting-toggle" role="switch" aria-checked={appSettings.nearbyDiscoverable} disabled={settingsSaving} on:click={() => patchAppSettings({ nearbyDiscoverable: !appSettings.nearbyDiscoverable }, appSettings.nearbyDiscoverable ? "已关闭附近设备发现" : "此设备现在可被附近设备发现")}><span><strong>{uiTranslate("附近设备发现", $uiLanguage)}</strong><small>{uiTranslate("允许同一网络中的设备找到这台电脑", $uiLanguage)}</small></span><span class:checked={appSettings.nearbyDiscoverable} class="switch-control"><span></span></span></button>
                  <details class="inspector-advanced-settings"><summary>{uiTranslate("高级连接设置", $uiLanguage)} <CaretDown size={16} /></summary><div><p><span>{uiTranslate("后台服务", $uiLanguage)}</span><strong>{uiTranslate(snapshot?.serverRunning ? "运行中" : "已停止", $uiLanguage)}</strong></p><p><span>{uiTranslate("网络端口", $uiLanguage)}</span><strong>{snapshot?.port}</strong></p><p><span>{uiTranslate("安全连接", $uiLanguage)}</span><strong>TLS 1.3</strong></p><button class="secondary-button" on:click={copyConnectionInfo}><Copy size={16} /> {uiTranslate("复制诊断信息", $uiLanguage)}</button></div></details>
                </div>
              </details>
            </article>
          </div>
        </section>

        <section hidden class="connection-settings legacy-connection-settings" aria-labelledby="connection-settings-title">
          <div class:unavailable={!snapshot?.serverRunning} class="consumer-connection-hero">
            <span class="consumer-hero-icon"><LinkSimpleHorizontal size={23} /></span>
            <div class="consumer-hero-copy">
              <span class:offline={!snapshot?.serverRunning} class="connection-service-label">
                {uiTranslate(snapshot?.serverRunning ? "连接服务运行中" : "连接服务不可用", $uiLanguage)}
              </span>
              <strong id="connection-settings-title">{uiTranslate(snapshot?.serverRunning ? "本机可连接" : "连接服务已停止", $uiLanguage)}</strong>
              <small>{uiTranslate(snapshot?.serverRunning ? "其他设备可以在同一网络中发现并连接这台电脑" : "请重新启动 ArcRelay 后再连接设备", $uiLanguage)}</small>
            </div>
            <button class="primary-button add-device-button" disabled={!snapshot?.serverRunning} on:click={() => (addDeviceGuideOpen = !addDeviceGuideOpen)}>
              {#if addDeviceGuideOpen}<CaretUp size={17} /> {uiTranslate("收起", $uiLanguage)}{:else}<Plus size={17} /> {uiTranslate("添加设备", $uiLanguage)}{/if}
            </button>
          </div>

          {#if addDeviceGuideOpen}
            <section class="add-device-guide" aria-label={uiTranslate("添加设备", $uiLanguage)}>
              <header class="add-device-guide-header">
                <div>
                  <span>{uiTranslate("添加新设备", $uiLanguage)}</span>
                  <strong>{uiTranslate("选择一种连接方式", $uiLanguage)}</strong>
                </div>
                <small>{uiTranslate("请先在另一台设备上打开 ArcRelay，并确保两台设备处于同一网络。", $uiLanguage)}</small>
              </header>
              <div class="connection-method-grid">
                <div class="connection-method-card discovery-method-card">
                  <span class="connection-method-icon"><WifiHigh size={20} /></span>
                  <span class="connection-method-copy">
                    <strong>{uiTranslate("自动发现", $uiLanguage)}</strong>
                    <small>{uiTranslate("推荐用于同一局域网内的设备", $uiLanguage)}</small>
                  </span>
                  <button class="secondary-button" disabled={nearbyDiscoveryBusy} on:click={refreshNearbyDesktops}>
                    {uiTranslate(nearbyDiscoveryBusy ? "正在查找…" : "查找附近设备", $uiLanguage)}
                  </button>
                </div>
                <div class="connection-method-card manual-method-card">
                  <div class="manual-method-heading">
                    <strong>{uiTranslate("手动连接", $uiLanguage)}</strong>
                    <small>{uiTranslate("适用于自动发现不可用时", $uiLanguage)}</small>
                  </div>
                  <form class="manual-connect-form" on:submit|preventDefault={submitManualConnection}>
                    <label>
                      <span>{uiTranslate("IP 地址或主机名", $uiLanguage)}</span>
                      <input bind:value={manualHost} aria-label={uiTranslate("对端 IP 或主机名", $uiLanguage)} placeholder={uiTranslate("192.168.1.10 或 hostname.local", $uiLanguage)} />
                    </label>
                    <label class="manual-port-field">
                      <span>{uiTranslate("端口（可选）", $uiLanguage)}</span>
                      <input bind:value={manualPort} aria-label={uiTranslate("连接端口", $uiLanguage)} inputmode="numeric" placeholder="8765–8775" />
                    </label>
                    <button class="primary-button manual-connect-button" type="submit" disabled={connectingDesktopId === "manual"}>
                      {uiTranslate(connectingDesktopId === "manual" ? "连接中…" : "连接", $uiLanguage)}
                    </button>
                    {#if manualValidationError}<small class="field-error">{manualValidationError}</small>{/if}
                  </form>
                </div>
              </div>
            </section>
          {/if}

          {#if snapshot?.pendingPairing}
            <section class="pairing-banner settings-pairing-banner">
              <div class="pairing-icon"><DeviceMobile size={26} /></div>
              <div class="pairing-copy">
                <strong>{uiTranslate("新设备请求连接", $uiLanguage)}</strong>
                <span>{snapshot.pendingPairing.deviceName}</span>
                <small>{uiTranslate("仅勾选需要授权的能力：", $uiLanguage)}</small>
                <div class="pairing-grants">{#each snapshot.pendingPairing.grants as grant (grant.id)}<label><input type="checkbox" checked={approvedPairingGrantIds.has(grant.id)} on:change={(event) => setPairingGrant(grant.id, event.currentTarget.checked)} /> <span>{grant.label}</span></label>{/each}</div>
              </div>
              <div class="pairing-code-block"><span>{uiTranslate("确认配对码一致", $uiLanguage)}</span><code>{snapshot.pendingPairing.pairingCode}</code></div>
              <div class="pairing-actions">
                <button class="primary-button" disabled={pairingResponseBusy} on:click={() => respondToPairing(true, Array.from(approvedPairingGrantIds))}>{uiTranslate(pairingResponseBusy ? "处理中…" : "允许", $uiLanguage)}</button>
                <button class="secondary-button" disabled={pairingResponseBusy} on:click={() => respondToPairing(false, [])}>{uiTranslate("拒绝", $uiLanguage)}</button>
              </div>
            </section>
          {/if}
          {#each snapshot?.outgoingPairings ?? [] as pairing (pairing.deviceId)}
            <section class="pairing-banner settings-pairing-banner">
              <div class="pairing-icon"><DesktopTower size={26} /></div>
              <div class="pairing-copy"><strong>{uiTranslate("等待对方确认", $uiLanguage)}</strong><span>{pairing.deviceName}</span><small>{uiTranslate("请确认另一台设备显示相同配对码。", $uiLanguage)}</small></div>
              <div class="pairing-code-block"><span>{uiTranslate("配对码", $uiLanguage)}</span><code>{pairing.pairingCode}</code></div>
            </section>
          {/each}

          {#if nearbyDesktops.length > 0}
            <div class="nearby-desktop-list">
              {#each nearbyDesktops as device (device.deviceId)}
                <div class="nearby-desktop-row">
                  <span class="device-name-cell"><span class="device-row-icon"><DesktopTower size={19} /></span><span><strong>{device.deviceName}</strong><small>{uiTranslate(device.paired ? "已配对" : "新设备", $uiLanguage)}</small></span></span>
                  <span></span>
                  <button class="primary-button" disabled={desktopConnected(device.deviceId) || device.connecting || Boolean(connectingDesktopId)} on:click={() => connectDesktop(device)}>{uiTranslate(desktopConnected(device.deviceId) ? "已连接" : device.connecting ? "连接中…" : device.paired ? "连接并同步" : "配对并同步", $uiLanguage)}</button>
                </div>
              {/each}
            </div>
          {:else if nearbyDiscoveryStarted && !nearbyDiscoveryBusy}
            <div class="compact-empty-state"><strong>{uiTranslate("没有发现附近电脑", $uiLanguage)}</strong><span>{uiTranslate("请确认另一台电脑已打开 ArcRelay，并连接同一网络。", $uiLanguage)}</span></div>
          {/if}

          <div class="connection-section-title">
            <div class="section-heading-copy"><h2>{uiTranslate("当前控制", $uiLanguage)}</h2><span>{uiTranslate("查看正在操作这台电脑的设备", $uiLanguage)}</span></div>
          </div>
          <div class="settings-list consumer-settings-list">
            <div class:has-active-control={Boolean(snapshot?.activeInputDevice)} class="settings-row compact-status-row control-status-row">
              <span class="row-icon"><HandTap size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate(snapshot?.activeInputDevice?.name ?? "无设备正在控制", $uiLanguage)}</strong><small>{uiTranslate(snapshot?.activeInputDevice ? "正在操作这台电脑" : "连接设备后，可在这里查看控制状态", $uiLanguage)}</small></span>
              <span class="control-row-action">
                <span class:active={Boolean(snapshot?.activeInputDevice)} class="control-state-pill">{uiTranslate(snapshot?.activeInputDevice ? "控制中" : "空闲", $uiLanguage)}</span>
                {#if snapshot?.activeInputDevice}<button class="secondary-button compact-setting-button" on:click={() => bridge.disconnectDevice(snapshot?.activeInputDevice?.id ?? "")}>{uiTranslate("立即断开", $uiLanguage)}</button>{/if}
              </span>
            </div>
          </div>

          <div class="connection-section-title">
            <div class="section-heading-copy"><h2>{uiTranslate("我的设备", $uiLanguage)}</h2><span>{uiTranslate("已配对的设备会在可用时自动连接", $uiLanguage)}</span></div>
            <span class="section-count">{snapshot?.pairedDevices.length ?? 0} {uiTranslate("台设备", $uiLanguage)}</span>
          </div>
          <div class="settings-list consumer-settings-list device-consumer-list">
            {#each snapshot?.connectedDevices ?? [] as device (device.id)}
              <div class="settings-row device-consumer-row">
                <span class="device-name-cell"><span class="device-row-icon"><DeviceMobile size={19} /></span><span class="device-identity"><strong>{device.name}</strong><small>{uiTranslate("已配对设备", $uiLanguage)}</small></span></span>
                <span class="device-consumer-state"><strong><i class="online-dot"></i>{uiTranslate("在线", $uiLanguage)}</strong><small>{uiTranslate("自动连接已开启", $uiLanguage)}</small></span>
                <span class="device-row-actions">
                  <button class="secondary-button device-menu-button" on:click={() => bridge.disconnectDevice(device.id)}>{uiTranslate("断开", $uiLanguage)}</button>
                  <button class="secondary-button device-menu-button danger-device-button" on:click={() => confirmForgetPairedDevice(device)}>{uiTranslate("忘记", $uiLanguage)}</button>
                </span>
              </div>
            {/each}
            {#each (snapshot?.pairedDevices ?? []).filter((pairedDevice) => !(snapshot?.connectedDevices ?? []).some((connectedDevice) => connectedDevice.id === pairedDevice.id)) as device (device.id)}
              <div class="settings-row device-consumer-row">
                <span class="device-name-cell"><span class="device-row-icon"><DeviceMobile size={19} /></span><span class="device-identity"><strong>{device.name}</strong><small>{uiTranslate("已配对设备", $uiLanguage)}</small></span></span>
                <span class="device-consumer-state muted"><strong><i class="online-dot"></i>{uiTranslate("离线", $uiLanguage)}</strong><small>{uiTranslate("自动连接已开启", $uiLanguage)}</small></span>
                <span class="device-row-actions">
                  <button class="secondary-button device-menu-button" disabled={Boolean(connectingDesktopId)} on:click={() => connectDesktop({ deviceId: device.id, deviceName: device.name, host: "", paired: true, connecting: false })}>{uiTranslate("重连", $uiLanguage)}</button>
                  <button class="secondary-button device-menu-button danger-device-button" on:click={() => confirmForgetPairedDevice(device)}>{uiTranslate("忘记", $uiLanguage)}</button>
                </span>
              </div>
            {/each}
            {#if !(snapshot?.connectedDevices.length || snapshot?.pairedDevices.length)}
              <div class="consumer-inline-empty"><DeviceMobile size={22} /><span>{uiTranslate("还没有添加设备", $uiLanguage)}</span><button class="text-button" on:click={() => (addDeviceGuideOpen = true)}>{uiTranslate("添加第一台设备", $uiLanguage)}</button></div>
            {/if}
          </div>

          <div class="connection-section-title"><h2>{uiTranslate("连接设置", $uiLanguage)}</h2></div>
          <div class="settings-list consumer-settings-list">
            <div class="settings-row compact-status-row device-name-setting-row">
              <span class="row-icon"><DesktopTower size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("设备名称", $uiLanguage)}</strong><small>{uiTranslate("其他设备会通过这个名称识别本机；留空则使用系统主机名", $uiLanguage)}</small></span>
              <input
                class="device-name-input"
                value={appSettings.deviceName}
                maxlength="128"
                disabled={settingsSaving}
                placeholder={uiTranslate("使用系统主机名", $uiLanguage)}
                aria-label={uiTranslate("设备名称", $uiLanguage)}
                on:change={(event) => patchAppSettings({ deviceName: event.currentTarget.value.trim() }, "设备名称已更新，其他设备将自动同步")}
              />
            </div>
            <div class:permission-warning={snapshot?.inputPermission !== "Granted"} class="settings-row compact-status-row">
              <span class="row-icon"><Keyboard size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("控制权限", $uiLanguage)}</strong><small>{uiTranslate(snapshot?.inputPermission === "Granted" ? "已开启，连接的设备可以控制键盘和指针" : permissionText(snapshot?.inputPermission), $uiLanguage)}</small></span>
              <button class="text-button" on:click={() => bridge.openInputPermissionSettings()}>{uiTranslate(snapshot?.inputPermission === "Granted" ? "查看" : "去开启", $uiLanguage)}</button>
            </div>
            <button class="settings-row settings-toggle-row" role="switch" aria-checked={appSettings.nearbyDiscoverable} disabled={settingsSaving} on:click={() => patchAppSettings({ nearbyDiscoverable: !appSettings.nearbyDiscoverable }, appSettings.nearbyDiscoverable ? "已关闭附近设备发现" : "此设备现在可被附近设备发现") }>
              <span class="row-icon"><WifiHigh size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("附近设备发现", $uiLanguage)}</strong><small>{uiTranslate("允许附近设备在同一网络中找到这台电脑", $uiLanguage)}</small></span>
              <span class:checked={appSettings.nearbyDiscoverable} class="switch-control"><span></span></span>
            </button>
          </div>

          <details class="advanced-settings-disclosure">
            <summary>{uiTranslate("高级连接设置", $uiLanguage)} <CaretDown size={17} /></summary>
            <div class="advanced-settings-body">
              <div class="advanced-status-grid">
                <div><span>{uiTranslate("后台服务", $uiLanguage)}</span><strong>{uiTranslate(snapshot?.serverRunning ? "运行中" : "已停止", $uiLanguage)}</strong></div>
                <div><span>{uiTranslate("网络端口", $uiLanguage)}</span><strong>{snapshot?.port}</strong></div>
                <div><span>{uiTranslate("安全连接", $uiLanguage)}</span><strong>TLS 1.3</strong></div>
              </div>
              {#if snapshot?.activeInputDevice && snapshot.inputMetrics}
                <div class="advanced-diagnostic"><strong>{uiTranslate("输入诊断 ·", $uiLanguage)} {snapshot.activeInputDevice.name}</strong><span>{uiTranslate("接收", $uiLanguage)} {snapshot.inputMetrics.receiveHz.toFixed(1)} {uiTranslate("Hz · 系统注入", $uiLanguage)} {snapshot.inputMetrics.quartzHz.toFixed(1)} {uiTranslate("Hz · 最大间隔", $uiLanguage)} {(snapshot.inputMetrics.maximumGapUs / 1000).toFixed(2)} ms</span></div>
              {/if}
              <details class="nested-advanced-disclosure">
                <summary>{uiTranslate("连接诊断记录", $uiLanguage)}</summary>
                <div class="connection-log-list">{#each [...(snapshot?.activity ?? [])].reverse().slice(0, 12) as item}<p><span class="activity-dot"></span>{item}</p>{/each}</div>
              </details>
              <button class="secondary-button" on:click={copyConnectionInfo}><Copy size={16} /> {uiTranslate("复制诊断信息", $uiLanguage)}</button>
              <details class="nested-advanced-disclosure developer-disclosure">
                <summary>{uiTranslate("开发者设置", $uiLanguage)}</summary>
                <button class="secondary-button" on:click={() => (settingsTab = "agent")}>{uiTranslate("管理 Agent 接入", $uiLanguage)}</button>
              </details>
            </div>
          </details>
        </section>
      {:else if settingsTab === "agent"}
        <section class="settings-tab-content"><McpSettings mcpRunning={snapshot?.mcpRunning ?? false} /></section>
      {:else if settingsTab === "files"}
        <FileSharingSettings
          {appSettings}
          {settingsSaving}
          {localShares}
          {localSharesLoaded}
          {localShareBusy}
          {webGatewayStatus}
          {addLocalShare}
          {toggleLocalShareWriteAccess}
          {updateLocalShareWebAccess}
          {refreshWebGatewayStatus}
          {revokeWebAccess}
          {removeLocalShare}
          {patchAppSettings}
        />
      {:else if settingsTab === "clipboard"}
        <section class="settings-tab-content">
          <div class="connection-section-title"><h2>{uiTranslate("剪贴板", $uiLanguage)}</h2><span>{uiTranslate("保存与跨设备同步", $uiLanguage)}</span></div>
          <div class="settings-list settings-card-list">
            <button
              class="settings-row settings-toggle-row"
              role="switch"
              aria-checked={appSettings.clipboardEnabled}
              disabled={settingsSaving}
              on:click={() => patchAppSettings({ clipboardEnabled: !appSettings.clipboardEnabled }, appSettings.clipboardEnabled ? "剪贴板记录已关闭" : "剪贴板记录已开启")}
            >
              <span class="row-icon accent-icon"><ClipboardText size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("保存剪贴板历史", $uiLanguage)}</strong><small>{uiTranslate("保存复制过的文本、图片和文件，方便稍后再次使用。", $uiLanguage)}</small></span>
              <span class:checked={appSettings.clipboardEnabled} class="switch-control"><span></span></span>
            </button>
            <label class:disabled={!appSettings.clipboardEnabled} class="settings-row setting-field-row">
              <span class="row-icon"><Keyboard size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("打开剪贴板的快捷键", $uiLanguage)}</strong><small>{uiTranslate("点击输入框后，直接按下新的组合键。", $uiLanguage)}</small></span>
              <input class="shortcut-input" value={appSettings.clipboardShortcut} disabled={!appSettings.clipboardEnabled || settingsSaving} aria-label={uiTranslate("剪贴板全局快捷键", $uiLanguage)} readonly on:keydown={captureClipboardShortcut} />
            </label>
            <button
              class="settings-row settings-toggle-row"
              role="switch"
              aria-checked={appSettings.clipboardSyncEnabled}
              disabled={settingsSaving}
              on:click={() => patchAppSettings({ clipboardSyncEnabled: !appSettings.clipboardSyncEnabled }, "设置已保存")}
            >
              <span class="row-icon accent-icon"><LinkSimpleHorizontal size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("在设备间同步剪贴板", $uiLanguage)}</strong><small>{uiTranslate("在已配对的电脑之间同步复制的文本和图片。", $uiLanguage)}</small></span>
              <span class:checked={appSettings.clipboardSyncEnabled} class="switch-control"><span></span></span>
            </button>
            <button
              class="settings-row settings-toggle-row"
              role="switch"
              aria-checked={appSettings.clipboardSyncUpdateSystemClipboard}
              disabled={settingsSaving || !appSettings.clipboardSyncEnabled}
              on:click={() => patchAppSettings({ clipboardSyncUpdateSystemClipboard: !appSettings.clipboardSyncUpdateSystemClipboard }, "设置已保存")}
            >
              <span class="row-icon"><ClipboardText size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("自动复制收到的内容", $uiLanguage)}</strong><small>{uiTranslate("其他设备复制后，可直接在这台电脑粘贴。", $uiLanguage)}</small></span>
              <span class:checked={appSettings.clipboardSyncUpdateSystemClipboard} class="switch-control"><span></span></span>
            </button>
          </div>
          <div class="connection-section-title"><h2>{uiTranslate("使用方式", $uiLanguage)}</h2><span>{uiTranslate("面板、记录与同步细节", $uiLanguage)}</span></div>
          <div class="settings-list settings-card-list">
            <button class="settings-row settings-toggle-row" role="switch" aria-checked={appSettings.clipboardAutoFocusSearch} disabled={settingsSaving || !appSettings.clipboardEnabled} on:click={() => patchAppSettings({ clipboardAutoFocusSearch: !appSettings.clipboardAutoFocusSearch }, "设置已保存")}>
              <span class="row-icon"><MagnifyingGlass size={21} /></span><span class="row-copy"><strong>{uiTranslate("唤起后聚焦搜索框", $uiLanguage)}</strong><small>{uiTranslate("打开剪贴板窗口后可直接输入搜索；关闭后优先使用方向键选择。", $uiLanguage)}</small></span><span class:checked={appSettings.clipboardAutoFocusSearch} class="switch-control"><span></span></span>
            </button>
            <div class="settings-row setting-field-row">
              <span class="row-icon"><ClockCounterClockwise size={21} /></span><span class="row-copy"><strong>{uiTranslate("历史记录顺序", $uiLanguage)}</strong><small>{uiTranslate("选择最近复制或最早创建的内容优先。", $uiLanguage)}</small></span>
              <AppSelect value={appSettings.clipboardSortBy} disabled={!appSettings.clipboardEnabled || settingsSaving} aria-label={uiTranslate("剪贴板历史记录排序", $uiLanguage)} onValueChange={(clipboardSortBy) => patchAppSettings({ clipboardSortBy }, "剪贴板排序已更新")}
                options={[
                  { value: "updatedAt", label: uiTranslate("最近使用优先", $uiLanguage) },
                  { value: "createdAt", label: uiTranslate("创建时间优先", $uiLanguage) },
                ]}
              />
            </div>
            <button class="settings-row settings-toggle-row" role="switch" aria-checked={appSettings.clipboardSyncEditsAndDeletes} disabled={settingsSaving || !appSettings.clipboardSyncEnabled} on:click={() => patchAppSettings({ clipboardSyncEditsAndDeletes: !appSettings.clipboardSyncEditsAndDeletes }, "设置已保存")}>
              <span class="row-icon"><PencilSimple size={21} /></span><span class="row-copy"><strong>{uiTranslate("同步修改与删除", $uiLanguage)}</strong><small>{uiTranslate("让已同步记录在各台设备保持一致。", $uiLanguage)}</small></span><span class:checked={appSettings.clipboardSyncEditsAndDeletes} class="switch-control"><span></span></span>
            </button>
            <button class="settings-row settings-toggle-row" role="switch" aria-checked={appSettings.clipboardSyncFavorites} disabled={settingsSaving || !appSettings.clipboardSyncEnabled} on:click={() => patchAppSettings({ clipboardSyncFavorites: !appSettings.clipboardSyncFavorites }, "设置已保存")}>
              <span class="row-icon"><Star size={21} /></span><span class="row-copy"><strong>{uiTranslate("同步收藏", $uiLanguage)}</strong><small>{uiTranslate("在其他设备保留相同的收藏状态。", $uiLanguage)}</small></span><span class:checked={appSettings.clipboardSyncFavorites} class="switch-control"><span></span></span>
            </button>
          </div>
          <div class="connection-section-title clipboard-maintenance-title"><h2>{uiTranslate("记录维护", $uiLanguage)}</h2><span>{uiTranslate("整理或清除本机保存的剪贴板历史", $uiLanguage)}</span></div>
          <div class="settings-list settings-card-list">
            <button class="settings-row settings-action-row" disabled={clipboardMergeBusy || !appSettings.clipboardSyncEnabled} on:click={mergeClipboardDeviceHistory}>
              <span class="row-icon"><ClockCounterClockwise size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("合并设备历史", $uiLanguage)}</strong><small>{uiTranslate("重新汇总已配对设备的剪贴板记录并去除重复项。", $uiLanguage)}</small></span>
              <span class="row-action-label">{uiTranslate(clipboardMergeBusy ? "正在合并…" : "合并", $uiLanguage)}</span>
            </button>
            <button class="settings-row settings-action-row clipboard-danger-row" on:click={confirmClearClipboardHistory}>
              <span class="row-icon"><Trash size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("清空记录", $uiLanguage)}</strong><small>{uiTranslate("删除全部本地剪贴板记录，包括收藏和标签归类。", $uiLanguage)}</small></span>
              <span class="row-action-label">{uiTranslate("清空", $uiLanguage)}</span>
            </button>
          </div>
        </section>
      {:else if settingsTab === "notifications"}
        <section class="settings-tab-content">
          <SoundSettings bind:settings={appSettings} {settingsSaving} {patchAppSettings} {testSystemNotification} />
        </section>
      {:else if settingsTab === "screenshot"}
        <section class="settings-tab-content">
          <div class="connection-section-title"><h2>{uiTranslate("截图", $uiLanguage)}</h2><span>{uiTranslate("快捷键、捕获与输出", $uiLanguage)}</span></div>
          <div class="settings-list settings-card-list">
            <button
              class="settings-row settings-toggle-row"
              role="switch"
              aria-checked={appSettings.enhancedScreenshotEnabled}
              disabled={settingsSaving}
              on:click={() => patchAppSettings(
                { enhancedScreenshotEnabled: !appSettings.enhancedScreenshotEnabled },
                appSettings.enhancedScreenshotEnabled ? "增强截图已关闭" : "增强截图已启用",
              )}
            >
              <span class="row-icon accent-icon"><Camera size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("增强截图", $uiLanguage)}</strong><small>{uiTranslate("由 ArcRelay 常驻监听快捷键，截图界面仅在需要时启动。", $uiLanguage)}</small></span>
              <span class:checked={appSettings.enhancedScreenshotEnabled} class="switch-control"><span></span></span>
            </button>
            <label class:disabled={!appSettings.enhancedScreenshotEnabled} class="settings-row setting-field-row">
              <span class="row-icon"><Keyboard size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("区域截图快捷键", $uiLanguage)}</strong><small>{uiTranslate("点击输入框后，直接按下新的组合键。", $uiLanguage)}</small></span>
              <input class="shortcut-input" value={appSettings.screenshotShortcut} disabled={!appSettings.enhancedScreenshotEnabled || settingsSaving} aria-label={uiTranslate("区域截图全局快捷键", $uiLanguage)} readonly on:keydown={captureScreenshotShortcut} />
            </label>
            <button class="settings-row settings-action-row" disabled={!appSettings.enhancedScreenshotEnabled || settingsSaving} on:click={testScreenshotCapture}>
              <span class="row-icon"><Camera size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("测试截图", $uiLanguage)}</strong><small>{uiTranslate("立即打开一次区域截图，确认权限与快捷键组件可用。", $uiLanguage)}</small></span>
              <span class="row-action-label">{uiTranslate("开始", $uiLanguage)}</span>
            </button>
          </div>

          <div class="connection-section-title"><h2>{uiTranslate("捕获", $uiLanguage)}</h2><span>{uiTranslate("区域选择行为", $uiLanguage)}</span></div>
          <div class="settings-list settings-card-list">
            <button class="settings-row settings-toggle-row" role="switch" aria-checked={appSettings.screenshotIncludeCursor} disabled={settingsSaving || !appSettings.enhancedScreenshotEnabled} on:click={() => patchAppSettings({ screenshotIncludeCursor: !appSettings.screenshotIncludeCursor })}>
              <span class="row-icon"><HandTap size={21} /></span><span class="row-copy"><strong>{uiTranslate("包含鼠标指针", $uiLanguage)}</strong><small>{uiTranslate("截图结果中保留当前指针。", $uiLanguage)}</small></span><span class:checked={appSettings.screenshotIncludeCursor} class="switch-control"><span></span></span>
            </button>
          </div>

          <div class="connection-section-title"><h2>{uiTranslate("输出", $uiLanguage)}</h2><span>{uiTranslate("保存截图时使用", $uiLanguage)}</span></div>
          <div class="settings-list settings-card-list">
            <div class="settings-row setting-field-row">
              <span class="row-icon"><FileText size={21} /></span><span class="row-copy"><strong>{uiTranslate("文件格式", $uiLanguage)}</strong><small>{uiTranslate("PNG 更适合界面截图。", $uiLanguage)}</small></span>
              <AppSelect value={appSettings.screenshotFormat} disabled={settingsSaving || !appSettings.enhancedScreenshotEnabled} aria-label={uiTranslate("文件格式", $uiLanguage)} onValueChange={(screenshotFormat) => patchAppSettings({ screenshotFormat })}
                options={[
                  { value: "png", label: "PNG" },
                  { value: "jpeg", label: "JPEG" },
                  { value: "webp", label: "WebP" },
                ]}
              />
            </div>
            <label class:disabled={!appSettings.enhancedScreenshotEnabled} class="settings-row setting-field-row">
              <span class="row-icon"><PencilSimple size={21} /></span><span class="row-copy"><strong>{uiTranslate("文件名模板", $uiLanguage)}</strong><small>{uiTranslate("支持", $uiLanguage)} {'{date}'} {uiTranslate("与", $uiLanguage)} {'{time}'} {uiTranslate("占位符。", $uiLanguage)}</small></span>
              <input class="device-name-input" value={appSettings.screenshotFileNameTemplate} disabled={settingsSaving || !appSettings.enhancedScreenshotEnabled} aria-label={uiTranslate("截图文件名模板", $uiLanguage)} on:change={(event) => patchAppSettings({ screenshotFileNameTemplate: event.currentTarget.value.trim() })} />
            </label>
          </div>
        </section>
      {:else}
        <section class="settings-tab-content">
          <div class="connection-section-title"><h2>{uiTranslate("通用", $uiLanguage)}</h2><span>{uiTranslate("外观与启动", $uiLanguage)}</span></div>
          <div class="settings-list settings-card-list">
            <div class="settings-row setting-field-row">
              <span class="row-icon accent-icon"><Palette size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("主题", $uiLanguage)}</strong><small>{uiTranslate("选择界面的显示方式。", $uiLanguage)}</small></span>
              <AppSelect value={appSettings.theme} disabled={settingsSaving} aria-label={uiTranslate("主题", $uiLanguage)} onValueChange={(theme) => patchAppSettings({ theme }, "主题已更新")}
                options={[
                  { value: "system", label: uiTranslate("跟随系统", $uiLanguage) },
                  { value: "light", label: uiTranslate("浅色", $uiLanguage) },
                  { value: "dark", label: uiTranslate("深色", $uiLanguage) },
                ]}
              />
            </div>
            <div class="settings-row setting-field-row">
              <span class="row-icon"><GlobeHemisphereWest size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("语言", $uiLanguage)}</strong><small>{uiTranslate("选择 ArcRelay 的界面显示语言。", $uiLanguage)}</small></span>
              <AppSelect aria-label={uiTranslate("语言", $uiLanguage)} value={appSettings.language} disabled={settingsSaving} onValueChange={(language) => patchAppSettings({ language }, "语言偏好已更新")}
                options={[
                  { value: "system", label: uiTranslate("跟随系统", $uiLanguage) },
                  ...languageOptions.map((option) => ({ value: option.value, lang: option.locale, label: option.label })),
                ]}
              />
            </div>
            <button
              class="settings-row settings-toggle-row"
              role="switch"
              aria-checked={appSettings.launchAtStartup}
              disabled={settingsSaving}
              on:click={() => patchAppSettings({ launchAtStartup: !appSettings.launchAtStartup }, appSettings.launchAtStartup ? "已关闭开机启动" : "已开启开机启动")}
            >
              <span class="row-icon"><RocketLaunch size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("登录时自动启动", $uiLanguage)}</strong><small>{uiTranslate("自动保持设备连接和附近传输可用。", $uiLanguage)}</small></span>
              <span class:checked={appSettings.launchAtStartup} class="switch-control"><span></span></span>
            </button>
            <button
              class="settings-row settings-toggle-row"
              role="switch"
              aria-checked={appSettings.launchSilently}
              disabled={settingsSaving}
              on:click={() => patchAppSettings({ launchSilently: !appSettings.launchSilently }, appSettings.launchSilently ? "已关闭静默启动" : "已开启静默启动，下次启动生效")}
            >
              <span class="row-icon"><DesktopTower size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("静默启动", $uiLanguage)}</strong><small>{uiTranslate("下次启动时仅在后台运行，可从托盘打开主页面。", $uiLanguage)}</small></span>
              <span class:checked={appSettings.launchSilently} class="switch-control"><span></span></span>
            </button>
            <button
              class="settings-row settings-toggle-row"
              role="switch"
              aria-checked={appSettings.autoUpdateEnabled}
              disabled={settingsSaving}
              on:click={() => patchAppSettings({ autoUpdateEnabled: !appSettings.autoUpdateEnabled }, appSettings.autoUpdateEnabled ? "已关闭自动检查更新" : "已开启自动检查更新")}
            >
              <span class="row-icon"><ClockCounterClockwise size={21} /></span>
              <span class="row-copy"><strong>{uiTranslate("自动检查更新", $uiLanguage)}</strong><small>{uiTranslate("启动后及每 6 小时检查一次 main 分支的最新成功构建。", $uiLanguage)}</small></span>
              <span class:checked={appSettings.autoUpdateEnabled} class="switch-control"><span></span></span>
            </button>
            {#if isMacPlatform}
              <div class="settings-row setting-field-row">
                <span class="row-icon"><ShieldCheck size={21} /></span>
                <span class="row-copy"><strong>{uiTranslate("完全磁盘访问", $uiLanguage)}</strong><small>{uiTranslate("可一次授权 ArcRelay 读取你选择共享的文件夹；也可只在添加文件夹时按需授权。", $uiLanguage)}</small></span>
                <button class="secondary-button" on:click={() => bridge.openFullDiskAccessSettings()}>{uiTranslate("前往系统设置", $uiLanguage)}</button>
              </div>
            {/if}
            <div class="settings-row setting-field-row app-update-row">
              <span class="row-icon accent-icon"><ClockCounterClockwise size={21} /></span>
              <span class="row-copy">
                <strong>{uiTranslate("应用更新", $uiLanguage)}</strong>
                {#if appUpdateInstalling && appUpdateProgress?.phase === "downloading"}
                  <small>
                    {uiTranslate("正在下载更新…", $uiLanguage)}
                    {#if appUpdateProgress.totalBytes}
                      {Math.min(100, Math.round(appUpdateProgress.downloadedBytes / appUpdateProgress.totalBytes * 100))}%
                    {/if}
                  </small>
                  {#if appUpdateProgress.totalBytes}
                    <progress max={appUpdateProgress.totalBytes} value={appUpdateProgress.downloadedBytes}></progress>
                  {/if}
                {:else if appUpdateStatus.update}
                  <small>{uiTranslate("可更新到", $uiLanguage)} {appUpdateStatus.update.version}{uiTranslate("。更新包会在安装前验证签名。", $uiLanguage)}</small>
                {:else}
                  <small>{uiTranslate(`当前版本 ${appUpdateStatus.currentVersion}，更新通道为 main。`, $uiLanguage)}</small>
                {/if}
              </span>
              <span class="app-update-actions">
                <button class="secondary-button" disabled={appUpdateChecking || appUpdateInstalling} on:click={() => checkForAppUpdate(false)}>{uiTranslate(appUpdateChecking ? "正在检查…" : "检查更新", $uiLanguage)}</button>
                {#if appUpdateStatus.update}
                  <button class="primary-button" disabled={appUpdateInstalling} on:click={confirmInstallAppUpdate}>{uiTranslate(appUpdateInstalling ? "正在安装…" : "安装并重启", $uiLanguage)}</button>
                {/if}
              </span>
            </div>
          </div>
          <div class="settings-about-footer">
            <div><BrandLogo size={22} /><span><strong>ArcRelay</strong><small>{uiTranslate("版本", $uiLanguage)} {appUpdateStatus.currentVersion}</small></span></div>
            <button class="text-button danger-text-button" on:click={confirmQuitApp}><Power size={16} /> {uiTranslate("完全退出", $uiLanguage)}</button>
          </div>
        </section>
      {/if}
    </main>
