<script lang="ts">
  import { onMount } from 'svelte';
  import { shouldAutoSendSystemShare } from './trayNavigation';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { ArrowClockwise, CheckCircle, DesktopTower, DeviceMobile, DotsThree, FolderOpen, Gear, Info, Pause, Play, Plus, ShieldCheck, UploadSimple, WarningCircle, X, CaretDown } from 'phosphor-svelte';
  import { bridge } from './bridge';
  import { language } from './i18n';
  import { transferText } from './transferMessages';
  import { SubscriptionScope } from './subscriptions';
  import { transferDraft } from './transferDraftStore';
  import { DraftError, isActiveTransfer, type DraftState } from './transferDraft';
  import TransferFileVisual from './TransferFileVisual.svelte';
  import type { SystemShareRequest, TransferFileKind, TransferPeer, TransferSnapshot, TransferView } from './types';

  export let notify: (message: string, kind?: 'success' | 'error') => void = () => {};
  export let openRequest: { transferId: string; sequence: number } | null = null;
  export let systemShareRequest: { request: SystemShareRequest; sequence: number } | null = null;
  type Panel = { type: 'settings' } | { type: 'device' | 'task' | 'incoming'; id: string };
  type Preview = { name: string; size: number | null; kind: TransferFileKind; thumbnailDataUrl: string | null };
  let snapshot: TransferSnapshot = { revision: -1, deviceId: '', deviceName: 'ArcRelay', receiveDirectory: '', discoverable: true, peers: [], transfers: [] };
  let knownPeers = new Map<string, TransferPeer>();
  let panel: Panel | null = null;
  let preview: Preview | null = null;
  let expanded = false;
  let dragging = false;
  let dragDepth = 0;
  let scanning = false;
  let discoverabilitySaving = false;
  let directorySaving = false;
  let policySaving = false;
  let confirmAutomatic = false;
  let automaticallyReceive = false;
  let busyTasks = new Set<string>();
  let draftNotice = '';
  let trayTargetPeerId: string | null = null;
  let undoTimer: ReturnType<typeof setTimeout> | undefined;
  let handledOpenSequence = 0;
  let handledSystemShareSequence = 0;
  let automaticSystemShares = new Map<string, string>();
  let automaticSystemShareBusy = new Set<string>();
  let networkReady = false;
  const scope = new SubscriptionScope();
  $: tr = (source: string) => transferText(source, $language);
  $: text = (source: string, values: Record<string, string | number>) => transferText(source, $language, values);
  $: devices = [...knownPeers.values()].map(device => ({ ...device, online: snapshot.peers.some(peer => peer.id === device.id) }));
  $: pending = snapshot.transfers.filter(task => task.direction === 'receiving' && task.status === 'awaitingApproval');
  $: tasks = [...snapshot.transfers].sort((a, b) => Number(isActiveTransfer(b.status)) - Number(isActiveTransfer(a.status)) || b.createdAtMs - a.createdAtMs);
  $: hasActive = tasks.some(task => isActiveTransfer(task.status));
  $: visibleFiles = expanded ? $transferDraft.files : $transferDraft.files.slice(0, 4);
  $: totalBytes = $transferDraft.files.reduce((sum, file) => sum + (file.size ?? 0), 0);
  $: preparing = $transferDraft.files.some(file => file.preparing);
  $: invalid = $transferDraft.files.some(file => file.error);
  $: panelId = panel && 'id' in panel ? panel.id : '';
  $: inspectedPeer = panel?.type === 'device' ? devices.find(device => device.id === panelId) : null;
  $: inspectedTask = panel && (panel.type === 'task' || panel.type === 'incoming') ? snapshot.transfers.find(task => task.id === panelId) : null;
  $: if (openRequest && openRequest.sequence !== handledOpenSequence) {
    handledOpenSequence = openRequest.sequence;
    openPanel({ type: 'incoming', id: openRequest.transferId });
  }
  $: if (systemShareRequest && systemShareRequest.sequence !== handledSystemShareSequence) {
    handledSystemShareSequence = systemShareRequest.sequence;
    void importSystemShare(systemShareRequest.request);
  }

  onMount(() => {
    void (async () => {
      try {
        await scope.add(bridge.onTransferState(next => { if (!scope.disposed) applySnapshot(next, true); }));
        if (!scope.disposed) applySnapshot(await bridge.refreshTransferDevices());
        const requests = await bridge.listSystemShareRequests();
        for (const request of requests) {
          if (!scope.disposed) await importSystemShare(request);
        }
      } catch (error) { if (!scope.disposed) report(error); }
    })();
    // Prepare files even when discovery is unavailable. Never send from a drop.
    if (bridge.isTauri()) void scope.add(getCurrentWebviewWindow().onDragDropEvent(event => {
      if (scope.disposed) return;
      if (event.payload.type === 'over' || event.payload.type === 'enter') dragging = true;
      if (event.payload.type === 'leave') dragging = false;
      if (event.payload.type === 'drop') { dragging = false; void addPaths(event.payload.paths); }
    })).catch(report);
    return () => {
      scope.dispose();
      if (undoTimer) {
        clearTimeout(undoTimer);
        const requestIds = [...new Set(transferDraft.state.undo.flatMap(file => file.systemShareRequestIds ?? []))];
        transferDraft.discardUndo();
        void discardSystemShares(requestIds);
      }
    };
  });

  function applySnapshot(next: TransferSnapshot, projected = false) {
    if (scope.disposed || (!projected && next.revision <= snapshot.revision)) return;
    if (panel?.type === 'incoming') {
      const previousRequest = snapshot.transfers.find(task => task.id === panelId);
      const nextRequest = next.transfers.find(task => task.id === panelId);
      if (previousRequest?.status === 'awaitingApproval' && nextRequest?.status !== 'awaitingApproval') panel = null;
    }
    snapshot = next;
    networkReady = true;
    knownPeers = new Map([...knownPeers, ...next.peers.map(peer => [peer.id, peer] as const)]);
    transferDraft.updateNetwork(next);
    queueMicrotask(() => void tryAutomaticSystemShares());
  }
  function report(error: unknown) {
    const messages: Record<DraftError['code'], string> = {
      limit: '一次最多添加 256 个文件', empty: '请先添加文件', preparing: '请等待文件准备完成', invalid: '请移除无法读取的文件后重试',
      offline: '原接收设备不在线，不会改发其他设备', busy: '已有提交或传输任务，请等待完成',
      changed: '文件已变更，请检查更新后的清单再发送；重试时请重新加入待发送', unavailable: '源文件记录不可用，请重新添加文件',
    };
    notify(error instanceof DraftError ? tr(messages[error.code]) : error instanceof Error ? error.message : String(error), 'error');
  }
  async function addPaths(paths: string[]) {
    try { const count = await transferDraft.add(paths); if (!scope.disposed) draftNotice = count ? text('已添加 {count} 个文件，尚未发送', { count }) : tr('这些文件已在待发送列表中'); }
    catch (error) { if (!scope.disposed) report(error); }
  }
  async function importSystemShare(request: SystemShareRequest) {
    try {
      if (!transferDraft.hasSystemRequest(request.id)) {
        await transferDraft.add(request.items.map(item => item.path), request.id);
      }
      draftNotice = text('已添加 {count} 个文件，尚未发送', { count: request.items.length });
      if (request.source === 'tray') trayTargetPeerId = request.targetPeerId;
      if (shouldAutoSendSystemShare(request) && request.targetPeerId) {
        automaticSystemShares = new Map(automaticSystemShares).set(request.id, request.targetPeerId);
        await tryAutomaticSystemShares();
      }
    } catch (error) { report(error); }
  }
  async function tryAutomaticSystemShares() {
    if (!networkReady) return;
    for (const [requestId, peerId] of automaticSystemShares) {
      if (automaticSystemShareBusy.has(requestId)) continue;
      const device = devices.find(candidate => candidate.id === peerId && candidate.online);
      if (!device || $transferDraft.submitting || transferDraft.targetBusy(peerId)) continue;
      const requestFiles = $transferDraft.files.filter(file => file.systemShareRequestIds?.includes(requestId));
      if (!requestFiles.length || requestFiles.some(file => file.preparing || file.error)) continue;
      automaticSystemShareBusy = new Set([...automaticSystemShareBusy, requestId]);
      try {
        await transferDraft.sendSystemRequestTo(requestId, peerId);
        automaticSystemShares = new Map([...automaticSystemShares].filter(([id]) => id !== requestId));
        notify(text('传输请求已创建：{name}', { name: device.name }));
        draftNotice = '';
      } catch (error) { report(error); }
      finally { automaticSystemShareBusy = new Set([...automaticSystemShareBusy].filter(id => id !== requestId)); }
    }
  }
  async function discardSystemShares(requestIds: string[]) {
    for (const requestId of requestIds) {
      try { await bridge.discardSystemShareRequest(requestId); }
      catch (error) { report(error); }
      automaticSystemShares = new Map([...automaticSystemShares].filter(([id]) => id !== requestId));
    }
  }
  async function pickFiles() {
    try { const paths = await bridge.pickTransferFiles(); if (paths.length) await addPaths(paths); }
    catch (error) { report(error); }
  }
  function browserDrop(event: DragEvent) {
    event.preventDefault(); dragging = false; dragDepth = 0;
    if (!bridge.isTauri()) notify(tr('浏览器预览无法读取真实文件路径，请在桌面应用中添加文件'), 'error');
  }
  function clearDraft() {
    const systemRequestIds = transferDraft.systemRequestIds();
    trayTargetPeerId = null;
    transferDraft.clear(); draftNotice = tr('已清空待发送列表，本地文件未删除');
    if (undoTimer) clearTimeout(undoTimer);
    undoTimer = setTimeout(() => { undoTimer = undefined; transferDraft.discardUndo(); void discardSystemShares(systemRequestIds); }, 8000);
  }
  function undoClear() {
    try {
      if (undoTimer) { clearTimeout(undoTimer); undoTimer = undefined; }
      transferDraft.undoClear();
      draftNotice = tr('已恢复待发送文件');
    } catch (error) { report(error); }
  }
  function sendLabel(device: TransferPeer & { online: boolean }, state: DraftState, translate: typeof tr) {
    if (!device.online) return translate('设备已离线');
    if (state.submitting) return translate('提交中…');
    if (transferDraft.targetBusy(device.id)) return translate('传输进行中');
    if (!state.files.length) return translate('请先添加文件');
    if (state.files.some(file => file.preparing)) return translate('读取中…');
    if (state.files.some(file => file.error)) return translate('请检查文件');
    return text('发送 {count} 个文件', { count: state.files.length });
  }
  async function sendTo(device: TransferPeer) {
    try {
      const transferId = await transferDraft.sendTo(device.id);
      for (const requestId of transferDraft.systemRequestIds(transferId)) {
        automaticSystemShares = new Map([...automaticSystemShares].filter(([id]) => id !== requestId));
      }
      draftNotice = '';
      trayTargetPeerId = null;
      notify(text('传输请求已创建：{name}', { name: device.name }));
    }
    catch (error) { report(error); }
  }
  async function removeDraftFile(id: number) {
    const requestIds = $transferDraft.files.find(file => file.id === id)?.systemShareRequestIds ?? [];
    transferDraft.remove(id);
    await discardSystemShares(requestIds.filter(requestId => !transferDraft.hasSystemRequest(requestId)));
  }
  async function retry(task: TransferView) { try { await transferDraft.retry(task.id); notify(text('传输请求已创建：{name}', { name: task.peerName })); } catch (error) { report(error); } }
  async function restore(task: TransferView) { try { await transferDraft.restore(task.id); panel = null; draftNotice = tr('文件仅添加到此处，点击设备后才会发送。'); } catch (error) { report(error); } }
  async function taskAction(task: TransferView, action: 'pause' | 'resume' | 'cancel') {
    if (busyTasks.has(task.id)) return;
    busyTasks = new Set([...busyTasks, task.id]);
    try {
      if (action === 'pause') await bridge.pauseTransfer(task.id);
      else if (action === 'resume') await bridge.resumeTransfer(task.id);
      else {
        await bridge.cancelTransfer(task.id);
        if (panel && 'id' in panel && panel.id === task.id) panel = null;
        notify(tr('传输已取消'));
      }
    } catch (error) { report(error); }
    finally { busyTasks = new Set([...busyTasks].filter(id => id !== task.id)); }
  }
  async function refreshDevices() {
    if (scanning) return; scanning = true;
    try { applySnapshot(await bridge.refreshTransferDevices()); } catch (error) { report(error); } finally { scanning = false; }
  }
  async function toggleDiscoverability() {
    if (discoverabilitySaving) return; discoverabilitySaving = true;
    try { const saved = await bridge.updateAppSettings({ nearbyDiscoverable: !snapshot.discoverable }); snapshot = { ...snapshot, discoverable: saved.nearbyDiscoverable }; }
    catch (error) { report(error); } finally { discoverabilitySaving = false; }
  }
  function openPanel(next: Panel) { confirmAutomatic = false; automaticallyReceive = false; panel = next; }
  function dialog(node: HTMLDialogElement) {
    const previous = document.activeElement as HTMLElement | null; node.showModal();
    return { destroy() { node.close(); if (previous?.isConnected) previous.focus(); } };
  }
  async function chooseDirectory() {
    if (directorySaving) return; directorySaving = true;
    try { applySnapshot(await bridge.chooseTransferReceiveDirectory()); } catch (error) { report(error); } finally { directorySaving = false; }
  }
  async function openDirectory() { try { await bridge.openTransferReceiveDirectory(); } catch (error) { report(error); } }
  async function setAutomatic(device: TransferPeer, automatic: boolean) {
    if (policySaving || !device.paired) return; policySaving = true;
    try { applySnapshot(await bridge.setTransferReceivePolicy(device.id, automatic)); confirmAutomatic = false; }
    catch (error) { report(error); } finally { policySaving = false; }
  }
  async function respond(task: TransferView, accepted: boolean) {
    if (busyTasks.has(task.id)) return; busyTasks = new Set([...busyTasks, task.id]);
    const automatic = accepted && automaticallyReceive && snapshot.peers.some(peer => peer.id === task.peerId && peer.paired);
    try {
      await bridge.respondTransfer(task.id, accepted, automatic);
      if (panel?.type === 'incoming' && panel.id === task.id) panel = null;
      automaticallyReceive = false;
      notify(text(accepted ? '已接受来自 {name} 的文件' : '已拒绝来自 {name} 的传输', { name: task.peerName }));
    } catch (error) { report(error); } finally { busyTasks = new Set([...busyTasks].filter(id => id !== task.id)); }
  }
  function formatBytes(bytes: number | null) {
    if (bytes === null) return tr('计算中…'); if (bytes < 1024) return `${bytes} B`;
    const units = ['KB', 'MB', 'GB', 'TB']; let value = bytes / 1024; let index = 0;
    while (value >= 1024 && index < units.length - 1) { value /= 1024; index += 1; }
    return `${Number(value.toFixed(1))} ${units[index]}`;
  }
  function statusLabel(task: TransferView, translate: typeof tr) {
    const labels = { preparing: '正在准备', awaitingApproval: task.direction === 'sending' ? '等待对方接收' : '等待你确认', connecting: '正在连接', transferring: task.direction === 'sending' ? '正在发送' : '正在接收', paused: '已暂停', completed: '已完成', rejected: task.direction === 'sending' ? '对方未接收' : '已拒绝', cancelled: '已取消', failed: task.direction === 'sending' ? '发送失败' : '接收失败' };
    return translate(labels[task.status]);
  }
  const progress = (task: TransferView) => task.totalBytes ? Math.min(100, Math.round(task.completedBytes / task.totalBytes * 100)) : 0;
  const mobile = (peer: TransferPeer) => /ios|android|iphone|ipad|mobile/i.test(`${peer.platform} ${peer.model}`);
  const recoverable = (task: TransferView) => task.direction === 'sending' && ['failed', 'rejected', 'cancelled'].includes(task.status);
</script>

<div class="transfer-layout">
  <main class="transfer-main" on:dragenter={(event) => { event.preventDefault(); if (event.dataTransfer?.types.includes('Files')) { dragDepth++; dragging = true; } }} on:dragover|preventDefault on:dragleave={() => { dragDepth = Math.max(0, dragDepth - 1); if (!dragDepth) dragging = false; }} on:drop={browserDrop}>
    <header class="transfer-header"><div><h1>{tr('附近传输')}</h1><p>{tr('添加文件后，选择一台设备发送。')}</p></div><div class="header-actions"><button class="discoverable-button" class:active={snapshot.discoverable} role="switch" aria-checked={snapshot.discoverable} disabled={discoverabilitySaving} on:click={toggleDiscoverability}><span class="visibility-dot"></span>{tr(snapshot.discoverable ? '本机可被发现' : '本机不可被发现')}</button><button class="secondary-button" on:click={() => openPanel({ type: 'settings' })}><Gear size={19} />{tr('接收设置')}</button></div></header>

    {#if pending.length}<section class="incoming-requests" aria-label={tr('接收请求')} aria-live="polite">{#each pending as request (request.id)}<div><Info size={19} /><span><strong>{request.peerName}</strong> · {text('请求接收 {count} 个文件', { count: request.files.length })}</span><button class="outline-button" on:click={() => openPanel({ type: 'incoming', id: request.id })}>{tr('查看请求')}</button><button class="icon-button dismiss-request" disabled={busyTasks.has(request.id)} aria-label={`${tr('取消传输')}：${request.peerName}`} title={tr('取消传输')} on:click={() => taskAction(request, 'cancel')}><X size={17} /></button></div>{/each}</section>{/if}

    <section class="draft-panel" class:dragging aria-labelledby="draft-heading">
      <header class="draft-header"><div><h2 id="draft-heading">{tr('待发送')}</h2><span>{$transferDraft.files.length ? `${text('{count} 个文件', { count: $transferDraft.files.length })} · ${preparing ? tr('计算中…') : formatBytes(totalBytes)}` : tr('尚未添加文件')}</span></div><div class="draft-actions">{#if $transferDraft.files.length}<button class="text-button" disabled={Boolean($transferDraft.submitting)} on:click={clearDraft}>{tr('清空')}</button>{/if}<button class="outline-button" on:click={pickFiles}><Plus size={18} />{tr('添加文件')}</button></div></header>
      {#if $transferDraft.files.length}
        <div class="draft-file-list" class:expanded>{#each visibleFiles as file (file.id)}<div class="draft-file-row" class:invalid={Boolean(file.error)}>
          {#if file.thumbnailDataUrl}<button class="preview-button" aria-label={text('预览 {name}', { name: file.name })} on:click={() => preview = file}><TransferFileVisual kind={file.kind} name={file.name} thumbnail={file.thumbnailDataUrl} /></button>{:else}<TransferFileVisual kind={file.kind} name={file.name} />{/if}
          <div class="file-copy"><strong title={file.path}>{file.name}</strong>{#if file.error}<small class="file-error" title={file.error}><WarningCircle size={14} />{tr('文件无法读取')} · {file.error}</small>{/if}</div><span class="file-size">{file.preparing ? tr('读取中…') : formatBytes(file.size)}</span><button class="icon-button" disabled={$transferDraft.submitting?.files.some(item => item.id === file.id)} aria-label={text('移除 {name}', { name: file.name })} on:click={() => removeDraftFile(file.id)}><X size={18} /></button>
        </div>{/each}</div>
        {#if $transferDraft.files.length > 4}<button class="expand-files text-button" aria-expanded={expanded} on:click={() => expanded = !expanded}><CaretDown size={16} />{expanded ? tr('收起文件清单') : text('展开其余 {count} 个文件', { count: $transferDraft.files.length - 4 })}</button>{/if}
        <button class="append-drop" on:click={pickFiles}><span><Plus size={18} />{tr('继续拖入文件')}</span><small>{tr('文件仅添加到此处，点击设备后才会发送。')}</small></button>
      {:else}<button class="empty-drop" on:click={pickFiles}><UploadSimple size={38} weight="light" /><strong>{tr('拖放文件到这里')}</strong><span>{tr(hasActive ? '正在传输的文件不受影响，可以继续准备下一批。' : '或点击“添加文件”，选择要发送的文件')}</span><small>{tr('先添加文件，再选择接收设备。')}</small></button>{/if}
      {#if dragging}<div class="drag-overlay"><Plus size={38} /><strong>{tr('松开以添加文件')}</strong><span>{tr('将追加到待发送列表，不会立即发送。')}</span></div>{/if}
    </section>
    {#if trayTargetPeerId && $transferDraft.files.length}<div class="draft-notice" role="status">{text('托盘指定接收设备：{name}。点击设备确认发送；离线时不会自动改发。', { name: devices.find(device => device.id === trayTargetPeerId)?.name ?? trayTargetPeerId })}</div>{/if}
    {#if draftNotice || $transferDraft.undo.length}<div class="draft-notice" role="status"><span>{draftNotice}</span>{#if $transferDraft.undo.length}<button class="text-button" on:click={undoClear}>{tr('撤销清空')}</button>{/if}</div>{/if}

    <section class="recipients" aria-labelledby="recipient-heading"><header class="section-heading"><div><h2 id="recipient-heading">{tr('发送到')}</h2><span>{text('{count} 台设备可用', { count: snapshot.peers.length })}</span></div><button class="text-button refresh-button" disabled={scanning} aria-label={tr('刷新附近设备')} on:click={refreshDevices}><ArrowClockwise size={18} class={scanning ? 'spinning' : ''} />{tr(scanning ? '正在扫描…' : '刷新')}</button></header>
      {#if devices.length}<div class="device-list">{#each devices as device (device.id)}<div class="device-row" class:offline={!device.online}>
        <button class="recipient-main" disabled={!device.online || Boolean($transferDraft.submitting) || !$transferDraft.files.length || preparing || invalid || transferDraft.targetBusy(device.id)} aria-label={text('向 {name} 发送 {count} 个文件', { name: device.name, count: $transferDraft.files.length })} on:click={() => sendTo(device)}><span class="device-icon">{#if mobile(device)}<DeviceMobile size={32} weight="light" />{:else}<DesktopTower size={32} weight="light" />{/if}</span><span class="device-copy"><strong>{device.name}</strong><small>{device.platform}{device.model ? ` · ${device.model}` : ''}</small></span><span class="paired-chip" class:muted={!device.paired || !device.online}>{tr(!device.online ? '设备已离线' : device.paired ? '已配对' : '未配对')}</span><span class="send-affordance">{sendLabel(device, $transferDraft, tr)}</span></button>
        <button class="icon-button details-button" aria-label={text('查看 {name} 的设备详情', { name: device.name })} on:click={() => openPanel({ type: 'device', id: device.id })}><DotsThree size={23} weight="bold" /></button>
      </div>{/each}</div>{:else}<div class="no-devices"><DesktopTower size={32} weight="light" /><strong>{tr('暂未发现附近设备')}</strong><p>{tr('确保设备连接在同一局域网内并已打开 ArcRelay。')}</p>{#if $transferDraft.files.length}<small>{tr('待发送文件已保留，可以继续添加。')}</small>{/if}</div>{/if}
      <div class="network-hint"><span><Info size={15} />{tr($transferDraft.files.length ? '请确认接收设备名称，点击后开始发送。' : '先添加文件，再选择接收设备。')}</span><span><ShieldCheck size={15} />{tr('端到端加密传输')}</span></div>
    </section>

    {#if !bridge.isTauri()}<p class="browser-preview-note">{tr('状态仅为浏览器演示，不会发送文件或修改设备设置。')}</p>{/if}

    <section class="transfer-history" aria-labelledby="history-heading"><header class="section-heading"><h2 id="history-heading">{tr(hasActive ? '传输任务' : '传输记录')}</h2></header>
      {#if tasks.length}<div class="task-list">{#each tasks as task (task.id)}<article class="task-row" class:failed={task.status === 'failed'}>
        <TransferFileVisual kind={task.files[0]?.kind ?? 'file'} name={task.files[0]?.name ?? ''} thumbnail={task.files[0]?.thumbnailDataUrl ?? null} />
        <div class="task-copy"><strong>{task.files.length === 1 ? task.files[0].name : text('{count} 个文件', { count: task.files.length })} · {text(task.direction === 'sending' ? '发送至 {name}' : '接收自 {name}', { name: task.peerName })}</strong><small>{formatBytes(task.totalBytes)}{#if task.status === 'awaitingApproval' && task.direction === 'sending'} · {tr('对方确认后才会开始传输')}{:else if task.errorMessage} · {task.errorMessage}{/if}</small>
          {#if task.status === 'transferring' || task.status === 'paused'}<div class="task-progress"><progress value={task.completedBytes} max={task.totalBytes || 1} aria-label={tr('传输进度')}></progress><span>{progress(task)}% · {formatBytes(task.completedBytes)} / {formatBytes(task.totalBytes)}{#if task.status === 'transferring' && task.speedBytesPerSecond} · {formatBytes(task.speedBytesPerSecond)}/s{/if}</span></div>{/if}
        </div><span class="task-status" class:complete={task.status === 'completed'} class:error={task.status === 'failed' || task.status === 'rejected'}>{#if task.status === 'completed'}<CheckCircle size={22} weight="fill" />{:else if task.status === 'failed'}<WarningCircle size={20} />{/if}{statusLabel(task, tr)}</span>
        <div class="task-actions">{#if task.status === 'transferring' || task.status === 'paused'}<button class="icon-button" disabled={busyTasks.has(task.id)} aria-label={tr(task.status === 'paused' ? '继续传输' : '暂停传输')} on:click={() => taskAction(task, task.status === 'paused' ? 'resume' : 'pause')}>{#if task.status === 'paused'}<Play size={17} />{:else}<Pause size={17} />{/if}</button>{/if}
          {#if isActiveTransfer(task.status)}<button class="icon-button" disabled={busyTasks.has(task.id)} aria-label={tr('取消传输')} on:click={() => taskAction(task, 'cancel')}><X size={17} /></button>{/if}
          {#if recoverable(task) && $transferDraft.batches[task.id]}<button class="outline-button" disabled={Boolean($transferDraft.submitting) || transferDraft.targetBusy(task.peerId)} on:click={() => retry(task)}>{tr('重试')}</button>{/if}<button class="text-button accent" on:click={() => openPanel({ type: task.direction === 'receiving' && task.status === 'awaitingApproval' ? 'incoming' : 'task', id: task.id })}>{tr('查看')}</button>
        </div>
      </article>{/each}</div>{:else}<p class="empty-history">{tr('暂无传输记录')}</p>{/if}
    </section>
  </main>
</div>

{#if panel}<dialog class="transfer-drawer" use:dialog aria-labelledby="transfer-drawer-title" on:cancel|preventDefault={() => panel = null} on:click={(event) => { if (event.target === event.currentTarget) panel = null; }}>
  <header class="drawer-header"><h2 id="transfer-drawer-title">{tr(panel.type === 'settings' ? '本机接收设置' : panel.type === 'device' ? '设备详情' : panel.type === 'incoming' ? '接收请求' : '传输详情')}</h2><button class="icon-button" aria-label={tr('关闭详情')} on:click={() => panel = null}><X size={20} /></button></header>
  <div class="drawer-body">
    {#if panel.type === 'settings'}
      <p>{tr('管理文件保存到本机的位置，以及接收确认方式。')}</p><section><h3>{tr('接收目录')}</h3><div class="directory-row"><FolderOpen size={23} /><button class="directory-link" title={snapshot.receiveDirectory} on:click={openDirectory}>{snapshot.receiveDirectory}</button><button class="text-button accent" disabled={directorySaving} on:click={chooseDirectory}>{tr(directorySaving ? '保存中…' : '更改')}</button></div><p>{tr('只影响本机收到的文件，不是对方的保存位置。')}</p></section><section><h3>{tr('接收确认')}</h3><strong>{tr('默认每次确认')}</strong><p>{tr('已配对不代表自动接收，可在设备详情中单独设置。')}</p></section>
    {:else if panel.type === 'device' && inspectedPeer}
      <div class="peer-profile"><span>{#if mobile(inspectedPeer)}<DeviceMobile size={36} />{:else}<DesktopTower size={36} />{/if}</span><h3>{inspectedPeer.name}</h3><small>{inspectedPeer.platform} · {inspectedPeer.model}</small><span class="paired-chip">{tr(!inspectedPeer.online ? '设备已离线' : inspectedPeer.paired ? '已配对' : '未配对')}</span></div>
      <section><h3>{tr('接收来自此设备的文件')}</h3><button class="secondary-button full-width" disabled={!inspectedPeer.paired || policySaving} on:click={() => inspectedPeer.automaticReceive ? setAutomatic(inspectedPeer, false) : confirmAutomatic = true}>{tr(inspectedPeer.automaticReceive ? '改为每次确认' : '开启自动接收')}</button>{#if !inspectedPeer.paired}<p>{tr('未配对设备不能开启自动接收。')}</p>{/if}
        {#if confirmAutomatic}<div class="automatic-confirmation"><p>{tr('开启后，来自此设备的文件会自动保存到本机，无需逐次确认。')}</p><button class="outline-button" disabled={policySaving} on:click={() => setAutomatic(inspectedPeer, true)}>{tr('仅对此设备开启')}</button><button class="text-button" disabled={policySaving} on:click={() => confirmAutomatic = false}>{tr('取消')}</button></div>{/if}<p>{tr('这不会自动发送文件，也不改变对方的接收设置。')}</p>
      </section><p class="drawer-footnote">{tr('查看设备不会选择发送目标，请回到主页面明确点击设备发送。')}</p>
    {:else if inspectedTask}
      <div class="task-summary"><span>{statusLabel(inspectedTask, tr)}</span><h3>{text(inspectedTask.direction === 'sending' ? '发送至 {name}' : '接收自 {name}', { name: inspectedTask.peerName })}</h3><p>{text('{count} 个文件', { count: inspectedTask.files.length })} · {formatBytes(inspectedTask.totalBytes)}</p>{#if inspectedTask.errorMessage}<p class="error-copy">{inspectedTask.errorMessage}</p>{/if}</div>
      <div class="detail-files">{#each inspectedTask.files as file (file.id)}<div>{#if file.thumbnailDataUrl}<button class="preview-button" aria-label={text('预览 {name}', { name: file.name })} on:click={() => preview = file}><TransferFileVisual kind={file.kind} name={file.name} thumbnail={file.thumbnailDataUrl} /></button>{:else}<TransferFileVisual kind={file.kind} name={file.name} />{/if}<span><strong>{file.name}</strong><small>{formatBytes(file.size)}</small></span></div>{/each}</div>
      {#if panel.type === 'incoming' && inspectedTask.status === 'awaitingApproval'}
        <section><h3>{tr('保存到本机')}</h3><p class="path-copy">{snapshot.receiveDirectory}</p>{#if snapshot.peers.some(peer => peer.id === inspectedTask.peerId && peer.paired)}<label class="automatic-label"><input type="checkbox" bind:checked={automaticallyReceive} />{tr('以后自动接收来自此设备的文件')}</label>{/if}</section><div class="drawer-actions"><button class="primary-button" disabled={busyTasks.has(inspectedTask.id)} on:click={() => respond(inspectedTask, true)}>{tr('接受')}</button><button class="secondary-button" disabled={busyTasks.has(inspectedTask.id)} on:click={() => respond(inspectedTask, false)}>{tr('拒绝')}</button></div>
      {:else if inspectedTask.direction === 'sending'}
        <p class="drawer-footnote">{tr('本批文件和接收设备已固定，新添加的文件不会加入本次传输。')}</p>
        {#if $transferDraft.batches[inspectedTask.id] && (recoverable(inspectedTask) || inspectedTask.status === 'completed')}<div class="drawer-actions">{#if recoverable(inspectedTask)}<button class="primary-button" disabled={Boolean($transferDraft.submitting) || transferDraft.targetBusy(inspectedTask.peerId)} on:click={() => retry(inspectedTask)}>{tr('重试发送到此设备')}</button>{/if}<button class="secondary-button" on:click={() => restore(inspectedTask)}>{tr(inspectedTask.status === 'completed' ? '再次发送这些文件' : '加入待发送，重新选择设备')}</button></div>{:else if !isActiveTransfer(inspectedTask.status)}<p>{tr('源文件记录不可用，请重新添加文件')}</p>{/if}
      {/if}
    {:else}<p>{tr('接收请求已处理或已过期')}</p>{/if}
  </div>
</dialog>{/if}
{#if preview?.thumbnailDataUrl}<dialog class="transfer-preview" use:dialog aria-label={tr('图片预览')} on:cancel|preventDefault={() => preview = null}><header class="drawer-header"><h2>{preview.name}</h2><button class="icon-button" aria-label={tr('关闭图片预览')} on:click={() => preview = null}><X size={20} /></button></header><img src={preview.thumbnailDataUrl} alt={preview.name} /><p>{formatBytes(preview.size)}</p></dialog>{/if}

<style>
  @import './transfer-workspace.css';
</style>
