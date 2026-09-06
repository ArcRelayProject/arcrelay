<script lang="ts">
  import { t } from "./localization";
  import { translate as uiTranslate, language as uiLanguage } from "./i18n";
  import { onDestroy, onMount } from "svelte";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import {
    Archive,
    ArrowClockwise,
    Check,
    DesktopTower,
    DeviceMobile,
    File as FileIcon,
    FilePdf,
    FolderOpen,
    Image as ImageIcon,
    Info,
    Pause,
    PaperPlaneTilt,
    Play,
    ShieldCheck,
    SlidersHorizontal,
    WarningCircle,
    X,
  } from "phosphor-svelte";

  import { SubscriptionScope } from "./subscriptions";
  import { bridge } from "./bridge";
  import { language, translate } from "./i18n";
  import type { TransferFileKind, TransferFileView, TransferPeer, TransferSnapshot, TransferView } from "./types";

  export let notify: (message: string, kind?: "success" | "error") => void = () => {};

  interface PreviewFile {
    id: number;
    name: string;
    size: string;
    kind: TransferFileKind;
    previewUrl?: string;
  }

  const emptySnapshot: TransferSnapshot = {
    revision: -1,
    deviceId: "",
    deviceName: "ArcRelay",
    receiveDirectory: "~/Downloads",
    discoverable: true,
    peers: [],
    transfers: [],
  };
  const emptyPeer: TransferPeer = {
    id: "", name: "未选择设备", platform: "", model: "等待发现附近设备", address: "", addresses: [], port: 0,
    publicKey: "", certificateSha256: "", paired: false, automaticReceive: false, lastSeenAtMs: 0,
  };

  let snapshot = emptySnapshot;
  let selectedDeviceId = "";
  let scanning = false;
  let dragging = false;
  let automaticallyReceiveFromSender = false;
  let previewFile: PreviewFile | null = null;
  const scope = new SubscriptionScope();
  let discoverabilitySaving = false;
  let currentLanguage: import("./types").LanguagePreference = "system";
  const unsubscribeLanguage = language.subscribe((value) => (currentLanguage = value));

  $: devices = snapshot.peers;
  $: selectedDevice = devices.find((device) => device.id === selectedDeviceId) ?? devices[0] ?? emptyPeer;
  $: incomingTransfer = snapshot.transfers.find((transfer) =>
    transfer.direction === "receiving" && transfer.status === "awaitingApproval" &&
    (!selectedDeviceId || transfer.peerId === selectedDeviceId),
  ) ?? snapshot.transfers.find((transfer) => transfer.direction === "receiving" && transfer.status === "awaitingApproval") ?? null;
  $: incomingPending = incomingTransfer !== null && incomingTransfer.peerId === selectedDevice.id;
  $: incomingFiles = incomingTransfer ? incomingTransfer.files.map(previewFileFromTransfer) : [];
  $: activeTransferValue = activeTransferForPeer(selectedDevice.id)
    ?? snapshot.transfers.find((transfer) => isActiveStatus(transfer.status))
    ?? null;
  $: activeTransfer = activeTransferValue !== null;
  $: activeTargetId = activeTransferValue?.peerId ?? "";
  $: activeFileName = activeTransferValue ? transferDisplayName(activeTransferValue) : "";
  $: activeFileSize = formatBytes(activeTransferValue?.totalBytes ?? 0);
  $: transferProgress = activeTransferValue?.totalBytes
    ? Math.min(100, Math.round(activeTransferValue.completedBytes * 100 / activeTransferValue.totalBytes))
    : 0;
  $: transferPaused = activeTransferValue?.status === "paused";
  $: remainingSeconds = activeTransferValue?.remainingSeconds ?? 0;

  onMount(() => {
    void (async () => { try {
      await scope.add(bridge.onTransferState((next) => { if (!scope.disposed) applySnapshot(next, true); }));
      const initial = await bridge.refreshTransferDevices();
      if (scope.disposed) return;
      applySnapshot(initial);
      if (bridge.isTauri()) {
        await scope.add(getCurrentWebviewWindow().onDragDropEvent((event) => {
          if (event.payload.type === "over") dragging = true;
          if (event.payload.type === "leave") dragging = false;
          if (event.payload.type === "drop") {
            dragging = false;
            void startFiles(event.payload.paths);
          }
        }));
      }
    } catch (error) {
      notify(errorMessage(error), "error");
    } })();
    return scope.dispose;
  });

  onDestroy(() => {
    unsubscribeLanguage();
  });

  function tr(source: string) { return translate(source, currentLanguage); }

  function applySnapshot(next: TransferSnapshot, projected = false) {
    if (!projected && next.revision <= snapshot.revision) return;
    const pending = next.transfers.find((transfer) => transfer.direction === "receiving" && transfer.status === "awaitingApproval");
    snapshot = next;
    if (pending) selectedDeviceId = pending.peerId;
    else if (!next.peers.some((peer) => peer.id === selectedDeviceId)) selectedDeviceId = next.peers[0]?.id ?? "";
  }

  function isPaired(device: TransferPeer) {
    return device.paired;
  }

  function selectDevice(deviceId: string) {
    selectedDeviceId = deviceId;
  }

  async function openFilePicker(deviceId = selectedDeviceId) {
    selectedDeviceId = deviceId;
    try {
      const paths = await bridge.pickTransferFiles();
      if (paths.length) await startFiles(paths);
    } catch (error) {
      notify(errorMessage(error), "error");
    }
  }

  function onDrop(event: DragEvent) {
    event.preventDefault();
    dragging = false;
    if (!bridge.isTauri() && event.dataTransfer?.files.length) notify("浏览器无法读取本地文件路径，请在桌面应用中选择文件", "error");
  }

  async function startFiles(paths: string[]) {
    if (!selectedDeviceId) return notify(uiTranslate("请先选择一台附近设备", $uiLanguage), "error");
    try {
      await bridge.sendTransfer(selectedDeviceId, paths);
      notify(t("已开始发送到 {name}", currentLanguage, { name: selectedDevice.name }));
    } catch (error) {
      notify(errorMessage(error), "error");
    }
  }

  async function togglePause() {
    if (!activeTransferValue) return;
    try {
      if (transferPaused) await bridge.resumeTransfer(activeTransferValue.id);
      else await bridge.pauseTransfer(activeTransferValue.id);
      notify(tr(transferPaused ? "传输已继续" : "传输已暂停"));
    } catch (error) { notify(errorMessage(error), "error"); }
  }

  async function cancelTransfer() {
    if (!activeTransferValue) return;
    try { await bridge.cancelTransfer(activeTransferValue.id); notify(tr("传输已取消"), "error"); }
    catch (error) { notify(errorMessage(error), "error"); }
  }

  async function refreshDevices() {
    if (scanning) return;
    scanning = true;
    try { applySnapshot(await bridge.refreshTransferDevices()); notify(uiTranslate("附近设备列表已更新", $uiLanguage)); }
    catch (error) { notify(errorMessage(error), "error"); }
    finally { scanning = false; }
  }

  async function toggleDiscoverability() {
    if (discoverabilitySaving) return;
    discoverabilitySaving = true;
    try {
      const saved = await bridge.updateAppSettings({ nearbyDiscoverable: !snapshot.discoverable });
      snapshot = { ...snapshot, discoverable: saved.nearbyDiscoverable };
      notify(tr(saved.nearbyDiscoverable ? "此设备现在可被附近设备发现" : "已关闭附近设备发现"));
    } catch (error) {
      notify(errorMessage(error), "error");
    } finally {
      discoverabilitySaving = false;
    }
  }

  async function acceptIncoming() {
    if (!incomingTransfer) return;
    try {
      await bridge.respondTransfer(incomingTransfer.id, true, automaticallyReceiveFromSender);
      notify(t(automaticallyReceiveFromSender ? "已接受；以后将自动接收 {name} 的文件" : "已接受来自 {name} 的文件", currentLanguage, { name: incomingTransfer.peerName }));
      automaticallyReceiveFromSender = false;
    } catch (error) { notify(errorMessage(error), "error"); }
  }

  async function rejectIncoming() {
    if (!incomingTransfer) return;
    try { await bridge.respondTransfer(incomingTransfer.id, false, false); notify(t("已拒绝来自 {name} 的传输", currentLanguage, { name: incomingTransfer.peerName }), "error"); }
    catch (error) { notify(errorMessage(error), "error"); }
  }

  async function chooseReceiveDirectory() {
    try { applySnapshot(await bridge.chooseTransferReceiveDirectory()); }
    catch (error) { notify(errorMessage(error), "error"); }
  }

  async function openReceiveDirectory() {
    try { await bridge.openTransferReceiveDirectory(); }
    catch (error) { notify(errorMessage(error), "error"); }
  }

  async function toggleAutomaticReceive() {
    if (!selectedDevice.paired) return notify(uiTranslate("请先配对此设备", $uiLanguage), "error");
    try { applySnapshot(await bridge.setTransferReceivePolicy(selectedDevice.id, !selectedDevice.automaticReceive)); }
    catch (error) { notify(errorMessage(error), "error"); }
  }

  function activeTransferForPeer(peerId: string): TransferView | null {
    return snapshot.transfers.find((transfer) => transfer.peerId === peerId && isActiveStatus(transfer.status)) ?? null;
  }

  function isActiveStatus(status: TransferView["status"]) {
    return ["preparing", "awaitingApproval", "connecting", "transferring", "paused"].includes(status);
  }

  function transferDisplayName(transfer: TransferView) {
    return transfer.files.length === 1 ? transfer.files[0].name : t("{count} 个文件", currentLanguage, { count: transfer.files.length });
  }

  function previewFileFromTransfer(file: TransferFileView): PreviewFile {
    return { id: file.id, name: file.name, size: formatBytes(file.size), kind: file.kind, previewUrl: file.thumbnailDataUrl ?? undefined };
  }

  function peerKind(device: TransferPeer) {
    return /ios|android|iphone|ipad|mobile/i.test(`${device.platform} ${device.model}`) ? "mobile" : "desktop";
  }

  function speedLabel(bytesPerSecond: number) {
    return bytesPerSecond ? `${formatBytes(bytesPerSecond)}/s` : uiTranslate("正在协商", $uiLanguage);
  }

  function errorMessage(error: unknown) {
    return error instanceof Error ? error.message : String(error);
  }

  function formatBytes(bytes: number) {
    if (bytes < 1024) return `${bytes} B`;
    const units = ["KB", "MB", "GB", "TB"];
    let value = bytes / 1024;
    let index = 0;
    while (value >= 1024 && index < units.length - 1) {
      value /= 1024;
      index += 1;
    }
    return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
  }
</script>

<div class="transfer-layout">
  <main class="transfer-main">
    <header class="transfer-header">
      <div>
        <h1>{uiTranslate("附近传输", $uiLanguage)}</h1>
        <p>{uiTranslate("设备只需统一配对一次，即可在同一局域网内安全发送文件。", $uiLanguage)}</p>
      </div>
      <div class="header-actions">
        <button
          class:active={snapshot.discoverable}
          class="discoverable-button"
          role="switch"
          aria-checked={snapshot.discoverable}
          disabled={discoverabilitySaving}
          on:click={toggleDiscoverability}
        >
          <span class:online={snapshot.discoverable} class="visibility-dot"></span>
          {uiTranslate(snapshot.discoverable ? "可被发现" : "不可被发现", $uiLanguage)}
        </button>
        <button
          class="refresh-button"
          class:scanning
          aria-label={uiTranslate("刷新附近设备", $uiLanguage)}
          title={uiTranslate("刷新附近设备", $uiLanguage)}
          on:click={refreshDevices}
        >
          <ArrowClockwise size={20} />
        </button>
        <button class="send-button" on:click={() => openFilePicker()}>
          <PaperPlaneTilt size={19} weight="fill" />
          {uiTranslate("发送文件", $uiLanguage)}
        </button>
      </div>
    </header>

    <div
      class="drop-zone"
      class:dragging
      role="button"
      tabindex="0"
      aria-label={uiTranslate("拖放或选择文件", $uiLanguage)}
      on:click={() => openFilePicker()}
      on:keydown={(event) => (event.key === "Enter" || event.key === " ") && openFilePicker()}
      on:dragenter={(event) => {
        event.preventDefault();
        dragging = true;
      }}
      on:dragover={(event) => event.preventDefault()}
      on:dragleave={(event) => {
        if (event.currentTarget === event.target) dragging = false;
      }}
      on:drop={onDrop}
    >
      <PaperPlaneTilt size={42} weight="regular" />
      <strong>{uiTranslate(dragging ? "松开即可发送" : "拖放文件到这里", $uiLanguage)}</strong>
      <span>{uiTranslate("或点击“发送文件”选择文件", $uiLanguage)}</span>
    </div>

    <div class="device-section-heading">
      <div>
        <h2>{uiTranslate("附近设备", $uiLanguage)}</h2>
        <span>{uiTranslate(scanning ? "正在扫描…" : `${devices.length} 台可用`, $uiLanguage)}</span>
      </div>
      <span class="encrypted-note"><ShieldCheck size={15} weight="fill" /> {uiTranslate("端到端加密", $uiLanguage)}</span>
    </div>

    <section class="device-list" aria-label={uiTranslate("附近设备列表", $uiLanguage)}>
      {#each devices as device (device.id)}
        <div
          class="device-row"
          class:selected={selectedDeviceId === device.id}
          role="button"
          tabindex="0"
          on:click={() => selectDevice(device.id)}
          on:keydown={(event) => (event.key === "Enter" || event.key === " ") && selectDevice(device.id)}
        >
          <span class="device-icon">
            {#if peerKind(device) === "desktop"}
              <DesktopTower size={25} />
            {:else}
              <DeviceMobile size={25} />
            {/if}
          </span>

          <span class="device-copy">
            <strong>{device.name}</strong>
            <small>{device.model || device.platform}</small>
          </span>

          <span class="device-state">
            {#if device.id === activeTargetId && activeTransfer}
              <span class="transfer-state">{uiTranslate(activeTransferValue?.direction === "receiving" ? "正在接收" : "正在传输", $uiLanguage)}</span>
            {:else if device.id === incomingTransfer?.peerId}
              <span class="confirm-state"><WarningCircle size={15} weight="fill" /> {uiTranslate("需要确认", $uiLanguage)}</span>
            {:else if isPaired(device)}
              <span class="paired-state"><ShieldCheck size={15} weight="fill" /> {uiTranslate("已配对", $uiLanguage)}</span>
            {:else}
              <span class="muted-state">{uiTranslate("未配对", $uiLanguage)}</span>
            {/if}
          </span>

          {#if device.id === activeTargetId && activeTransfer}
            <span class="inline-transfer" aria-label={uiTranslate((`传输进度 ${transferProgress}%`), $uiLanguage)}>
              <span class="inline-file">
                <FileIcon size={17} />
                <span><strong>{activeFileName}</strong><small>{activeFileSize}</small></span>
              </span>
              <span class="inline-progress">
                <span class="progress-copy"><strong>{transferProgress}%</strong><small>{uiTranslate(transferPaused ? "已暂停" : speedLabel(activeTransferValue?.speedBytesPerSecond ?? 0) + " · 剩余约 " + remainingSeconds + " 秒", $uiLanguage)}</small></span>
                <span class="progress-track"><i style={`width:${transferProgress}%`}></i></span>
              </span>
              <span class="transfer-controls">
                {#if activeTransferValue?.status === "transferring" || activeTransferValue?.status === "paused"}
                  <button aria-label={uiTranslate((transferPaused ? "继续传输" : "暂停传输"), $uiLanguage)} on:click|stopPropagation={togglePause}>
                    {#if transferPaused}<Play size={15} weight="fill" />{:else}<Pause size={15} weight="fill" />{/if}
                  </button>
                {/if}
                <button aria-label={uiTranslate("取消传输", $uiLanguage)} on:click|stopPropagation={cancelTransfer}><X size={16} /></button>
              </span>
            </span>
          {:else}
            <button
              class="device-action"
              aria-label={uiTranslate((device.id === incomingTransfer?.peerId ? "查看接收请求" : `向 ${device.name} 发送文件`), $uiLanguage)}
              on:click|stopPropagation={() =>
                device.id === incomingTransfer?.peerId ? selectDevice(device.id) : openFilePicker(device.id)}
            >
              {#if device.id === incomingTransfer?.peerId}
                <Info size={18} weight="bold" />
              {:else}
                <Play size={16} weight="fill" />
              {/if}
            </button>
          {/if}
        </div>
      {/each}
    </section>

    <p class="network-hint"><Info size={14} /> {uiTranslate("确保设备连接在同一局域网内并已打开 ArcRelay。", $uiLanguage)}</p>
  </main>

  <aside class="transfer-inspector">
    <div class="inspector-title">
      <strong>{uiTranslate(incomingPending ? "接收请求" : selectedDevice.name, $uiLanguage)}</strong>
      <button aria-label={uiTranslate("关闭设备详情", $uiLanguage)} on:click={() => (selectedDeviceId = devices[0]?.id ?? "")}><X size={18} /></button>
    </div>

    <div class="inspector-body">
      {#if incomingPending && incomingTransfer}
        <section class="request-sender-card">
          <span class="large-device-icon"><DeviceMobile size={30} /></span>
          <div>
            <strong>{incomingTransfer.peerName}</strong>
            <span class="unpaired-chip">{uiTranslate(selectedDevice.paired ? "已配对设备" : "未配对设备", $uiLanguage)}</span>
            <small>{uiTranslate("正在请求发送", $uiLanguage)} {incomingTransfer.files.length} {uiTranslate("个文件", $uiLanguage)}</small>
          </div>
        </section>

        <section class="inspector-card incoming-files-card">
          <div class="card-title-row">
            <h2>{uiTranslate("文件预览", $uiLanguage)}</h2>
            <span>{formatBytes(incomingTransfer.totalBytes)}</span>
          </div>
          <div class="incoming-files">
            {#each incomingFiles as file (file.id)}
              <div class="incoming-file-row">
                {#if file.kind === "image" && file.previewUrl}
                  <button class="image-thumbnail" aria-label={uiTranslate((`预览 ${file.name}`), $uiLanguage)} on:click={() => (previewFile = file)}>
                    <img src={file.previewUrl} alt={uiTranslate("ArcRelay 图片缩略图", $uiLanguage)} />
                    <span><ImageIcon size={12} weight="fill" /></span>
                  </button>
                {:else}
                  <span class:pdf={file.kind === "pdf"} class:archive={file.kind === "archive"} class="file-type-icon">
                    {#if file.kind === "pdf"}
                      <FilePdf size={23} weight="fill" />
                    {:else if file.kind === "archive"}
                      <Archive size={23} />
                    {:else}
                      <FileIcon size={23} />
                    {/if}
                  </span>
                {/if}
                <span class="incoming-file-copy"><strong>{file.name}</strong><small>{file.size}</small></span>
              </div>
            {/each}
          </div>
        </section>

        <section class="inspector-card">
          <div class="card-title-row"><h2>{uiTranslate("接收设置", $uiLanguage)}</h2></div>
          <dl class="request-details">
            <div><dt>{uiTranslate("保存到", $uiLanguage)}</dt><dd><button class="receive-directory-link" title={uiTranslate("在文件管理器中打开", $uiLanguage)} on:click={openReceiveDirectory}>{snapshot.receiveDirectory}</button></dd></div>
          </dl>
          <button class="change-folder-button" on:click={chooseReceiveDirectory}>{uiTranslate("更改接收目录", $uiLanguage)}</button>
        </section>

        {#if selectedDevice.paired}
          <label class="automatic-receive-option">
            <input type="checkbox" bind:checked={automaticallyReceiveFromSender} />
            <span><strong>{uiTranslate("以后自动接收此设备的文件", $uiLanguage)}</strong><small>{uiTranslate("设备已经过统一配对；此项只改变接收策略。", $uiLanguage)}</small></span>
          </label>
        {:else}
          <p class="safety-note"><ShieldCheck size={16} weight="fill" /> {uiTranslate("未配对设备只能单次确认接收，不会获得持续信任。", $uiLanguage)}</p>
        {/if}

        <p class="safety-note"><ShieldCheck size={16} weight="fill" /> {uiTranslate("未开启自动接收时，文件只会在你确认后写入接收目录。", $uiLanguage)}</p>
      {:else}
        <section class="device-profile-card">
          <span class="large-device-icon">
            {#if peerKind(selectedDevice) === "desktop"}<DesktopTower size={34} />{:else}<DeviceMobile size={34} />{/if}
          </span>
          <div>
            <span class="paired-chip" class:unpaired={!isPaired(selectedDevice)}><ShieldCheck size={14} weight="fill" /> {uiTranslate(isPaired(selectedDevice) ? "已配对" : "未配对", $uiLanguage)}</span>
            <strong>{selectedDevice.model || selectedDevice.platform}</strong>
            <small>{uiTranslate(selectedDevice.address ? `${selectedDevice.address}:${selectedDevice.port}` : "等待局域网设备", $uiLanguage)}</small>
          </div>
        </section>

        <section class="inspector-card">
          <div class="card-title-row"><h2>{uiTranslate("接收设置", $uiLanguage)}</h2></div>
          <dl class="device-details">
            <div><dt>{uiTranslate("接收方式", $uiLanguage)}</dt><dd>{uiTranslate(selectedDevice.automaticReceive ? "自动接收" : "每次确认", $uiLanguage)}</dd></div>
            <div><dt>{uiTranslate("接收目录", $uiLanguage)}</dt><dd><button class="receive-directory-link" title={uiTranslate("在文件管理器中打开", $uiLanguage)} on:click={openReceiveDirectory}>{snapshot.receiveDirectory}</button></dd></div>
          </dl>
          <p>{uiTranslate(selectedDevice.automaticReceive ? "来自这台已配对设备的文件将自动保存到接收目录。" : "收到文件时会先请求你的确认。", $uiLanguage)}</p>
          <button class="change-folder-button" on:click={chooseReceiveDirectory}>{uiTranslate("更改", $uiLanguage)}</button>
        </section>

        {#if selectedDevice.id === activeTargetId && activeTransfer}
          <section class="inspector-card">
            <div class="card-title-row"><h2>{uiTranslate("传输详情", $uiLanguage)}</h2></div>
            <dl class="device-details transfer-details">
              <div><dt>{uiTranslate("文件名", $uiLanguage)}</dt><dd>{activeFileName}</dd></div>
              <div><dt>{uiTranslate("文件大小", $uiLanguage)}</dt><dd>{activeFileSize}</dd></div>
              <div><dt>{uiTranslate("已传输", $uiLanguage)}</dt><dd>{transferProgress}%</dd></div>
              <div><dt>{uiTranslate("传输速度", $uiLanguage)}</dt><dd>{uiTranslate(transferPaused ? "已暂停" : speedLabel(activeTransferValue?.speedBytesPerSecond ?? 0), $uiLanguage)}</dd></div>
              <div><dt>{uiTranslate("剩余时间", $uiLanguage)}</dt><dd>{remainingSeconds} {uiTranslate("秒", $uiLanguage)}</dd></div>
            </dl>
          </section>
        {:else}
          <section class="inspector-card empty-transfer-card">
            <FolderOpen size={25} />
            <strong>{uiTranslate("可以发送文件", $uiLanguage)}</strong>
            <span>{uiTranslate("选择文件后将发送到这台设备。", $uiLanguage)}</span>
          </section>
        {/if}
      {/if}
    </div>

    <div class="inspector-actions">
      {#if incomingPending}
        <button class="accept-button" on:click={acceptIncoming}><Check size={18} weight="bold" /> {uiTranslate("接受", $uiLanguage)}</button>
        <button class="reject-button" on:click={rejectIncoming}><X size={18} /> {uiTranslate("拒绝", $uiLanguage)}</button>
      {:else if selectedDevice.id === activeTargetId && activeTransfer}
        <button class="cancel-button" on:click={cancelTransfer}><X size={17} /> {uiTranslate("取消传输", $uiLanguage)}</button>
      {:else}
        <button class="device-settings-button" on:click={toggleAutomaticReceive}><SlidersHorizontal size={18} /> {uiTranslate(selectedDevice.automaticReceive ? "改为每次确认" : "开启自动接收", $uiLanguage)}</button>
      {/if}
    </div>
  </aside>
</div>

{#if previewFile?.previewUrl}
  <div class="preview-backdrop" role="presentation" on:click={() => (previewFile = null)}>
    <div class="preview-dialog" role="dialog" aria-modal="true" aria-label={uiTranslate("图片预览", $uiLanguage)} tabindex="-1" on:click|stopPropagation on:keydown|stopPropagation>
      <header><div><strong>{previewFile.name}</strong><span>{previewFile.size}</span></div><button aria-label={uiTranslate("关闭图片预览", $uiLanguage)} on:click={() => (previewFile = null)}><X size={19} /></button></header>
      <div class="preview-canvas"><img src={previewFile.previewUrl} alt={previewFile.name} /></div>
    </div>
  </div>
{/if}

<style>
  @import "./transfer.css";
</style>
