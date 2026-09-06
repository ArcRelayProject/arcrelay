<script lang="ts">
  import { onMount } from 'svelte';
  import { ArrowClockwise, Check, CheckCircle, Clock, DeviceMobile, Laptop, PaperPlaneTilt, Plus, WarningCircle, X } from 'phosphor-svelte';
  import BrandLogo from '../BrandLogo.svelte';
  import TransferFileVisual from '../TransferFileVisual.svelte';
  import { TransferDraft, DraftError, isActiveTransfer } from '../transferDraft';
  import { SubscriptionScope, createInvalidationLoader } from '../subscriptions';
  import { language, setLanguage, t } from '../i18n';
  import { trayTransferBytes as bytes, trayTransferProgress, trayTransferStatus } from '../trayTransferState';
  import type { AppSettings, TransferSnapshot } from '../types';
  import type { TrayTransferPort } from './port';

  export let port: TrayTransferPort;
  const draft = new TransferDraft({ inspect: paths => port.inspect(paths), send: (peerId, paths) => port.send(peerId, paths) });
  const scope = new SubscriptionScope();
  let snapshot: TransferSnapshot = { revision: -1, deviceId: '', deviceName: '', receiveDirectory: '', discoverable: true, peers: [], transfers: [] };
  let hovering = true;
  let selectedPeerId = '';
  let transferId = port.initialTransferId ?? '';
  let error = '';
  let networkReady = false;
  let refreshing = false;
  let commandBusy = false;
  let ready = false;
  let frame: HTMLElement;
  let settingsRevision = -1;
  let theme: AppSettings['theme'] = 'system';
  const media = window.matchMedia('(prefers-color-scheme: dark)');
  $: tr = (source: string, values: Record<string, string | number> = {}) => t(source, $language, values);
  $: task = snapshot.transfers.find(item => item.id === transferId);
  $: batch = $draft.batches[transferId];
  $: active = Boolean(transferId && (!task || isActiveTransfer(task.status)));
  $: completed = task?.status === 'completed';
  $: terminal = Boolean(task && !isActiveTransfer(task.status));
  $: files = transferId ? task?.files ?? batch?.files ?? [] : $draft.files;
  $: total = transferId ? task?.totalBytes ?? batch?.files.reduce((sum, file) => sum + (file.size ?? 0), 0) ?? 0 : $draft.files.reduce((sum, file) => sum + (file.size ?? 0), 0);
  $: peer = snapshot.peers.find(item => item.id === selectedPeerId);
  $: sending = Boolean($draft.submitting);
  $: canSend = Boolean(peer && networkReady && $draft.files.length && !$draft.files.some(file => file.preparing || file.error) && !sending && !draft.targetBusy(peer.id));
  $: hasContent = Boolean(transferId || $draft.files.length || sending);
  $: mode = transferId ? 'task' : $draft.files.length ? 'choose' : 'hover';
  $: title = mode === 'hover' ? '附近传输' : completed ? '发送完成' : transferId ? (terminal ? '发送结果' : '正在发送') : '发送文件';
  $: if (ready) void port.updatePanel(Math.min(640, Math.ceil(frame?.getBoundingClientRect().height ?? 220)), hasContent).catch(report);

  function report(cause: unknown) {
    const messages: Partial<Record<DraftError['code'], string>> = {
      limit: '最多添加 256 个文件', empty: '请先添加文件', preparing: '正在读取文件信息',
      invalid: '请移除无法读取的文件后重试', offline: '设备已离线，请重新选择',
      busy: '该设备正在接收文件，请稍后再试', changed: '文件已更改，请确认后再次发送', unavailable: '请重新添加文件后重试',
    };
    error = cause instanceof DraftError ? tr(messages[cause.code] ?? '操作未完成') : String(cause instanceof Error ? cause.message : cause);
  }
  function applySettings(settings: AppSettings) {
    if (settings.revision < settingsRevision) return;
    settingsRevision = settings.revision;
    theme = settings.theme;
    setLanguage(settings.language);
    applyTheme();
  }
  function applyTheme() { document.documentElement.dataset.theme = theme === 'system' ? media.matches ? 'dark' : 'light' : theme; }
  function applySnapshot(next: TransferSnapshot) {
    if (next.revision < snapshot.revision) return;
    snapshot = next; networkReady = true; draft.updateNetwork(next);
    // Keep the user's target ID when it goes offline; never silently choose another.
  }
  async function add(paths: string[]) {
    if (!paths.length) return;
    error = ''; hovering = false;
    // A fresh drop after a terminal result starts a new draft. Keep only an
    // in-flight task on screen, so its cancellation/progress controls survive.
    if (task && !isActiveTransfer(task.status)) { transferId = ''; selectedPeerId = ''; }
    try { await draft.add(paths); } catch (cause) { report(cause); }
  }
  const drops = createInvalidationLoader(async () => {
    const drop = await port.takeDrop();
    if (scope.disposed) return;
    hovering = drop.hovering;
    await add(drop.paths);
  }, report);
  async function refresh() {
    if (refreshing) return;
    refreshing = true;
    try { applySnapshot(await port.refresh()); } catch (cause) { report(cause); }
    finally { refreshing = false; }
  }
  async function pick() {
    try { await add(await port.pick()); } catch (cause) { report(cause); }
  }
  function clearFiles() {
    if (sending) return;
    error = ''; selectedPeerId = ''; draft.clear();
  }
  async function send() {
    if (!canSend || !peer) return;
    error = '';
    try { transferId = await draft.sendTo(peer.id); }
    catch (cause) { report(cause); }
  }
  async function run(action: () => Promise<unknown>) {
    if (commandBusy) return;
    commandBusy = true; error = '';
    try { await action(); } catch (cause) { report(cause); }
    finally { commandBusy = false; }
  }
  async function hide() { try { await port.hide(); } catch (cause) { report(cause); } }
  async function done() {
    transferId = ''; error = ''; selectedPeerId = '';
    if (!$draft.files.length) { await port.updatePanel(190, false); await hide(); }
  }
  async function retry() {
    if (!task) return;
    // Restore for review and explicit selection, since the device may have changed.
    try { await draft.restore(task.id); transferId = ''; error = ''; }
    catch (cause) { report(cause); }
  }
  onMount(() => {
    ready = true;
    const resize = new ResizeObserver(() => {
      if (!scope.disposed) void port.updatePanel(Math.ceil(frame.getBoundingClientRect().height), hasContent).catch(report);
    });
    resize.observe(frame);
    media.addEventListener('change', applyTheme);
    void (async () => {
      await scope.add(port.onDrop(() => { void drops.refresh(); }));
      await drops.refresh();
    })().catch(report);
    void scope.add(port.onTransfer(applySnapshot)).catch(report);
    void (async () => { await scope.add(port.onSettings(applySettings)); applySettings(await port.getSettings()); })().catch(report);
    return () => { scope.dispose(); drops.dispose(); resize.disconnect(); media.removeEventListener('change', applyTheme); };
  });
</script>

<svelte:window onkeydown={event => { if (event.key === 'Escape') { event.preventDefault(); void hide(); } }} />

<section class="tray-panel" class:hover-panel={mode === 'hover'} class:choose-panel={mode === 'choose'} bind:this={frame} aria-label={tr('附近传输')}>
  <header class="panel-header">
    <BrandLogo size={26} />
    <strong class="brand">ArcRelay</strong>
    <span class="header-divider"></span>
    <h1>{tr(title)}</h1>
    <button class="icon-button close" aria-label={tr('关闭面板')} title={tr('关闭面板')} onclick={hide}><X size={20} /></button>
  </header>

  {#if mode === 'hover'}
    <div class="drop-target" class:dragging={hovering}>
      <PaperPlaneTilt size={36} weight="light" />
      <div><h2>{tr(hovering ? '松开后选择接收设备' : '拖入文件即可发送')}</h2><p>{tr('支持多文件，文件夹请先压缩')}</p></div>
    </div>
    <p class="panel-footnote">{tr('通过本地网络传输')}</p>
  {:else}
    {#if completed}
      <div class="result-hero" role="status">
        <span class="success-mark"><Check size={38} weight="bold" /></span>
        <h2>{tr('文件已送达')}</h2><p>{tr('{device} 已接收', { device: task?.peerName ?? '' })}</p>
      </div>
    {:else if terminal}
      <div class="result-hero" role="status">
        <WarningCircle size={44} class="warning-mark" />
        <h2>{tr(trayTransferStatus(task!.status))}</h2>
        <p>{task?.errorMessage ?? tr('文件保留在本机，可重新选择设备发送')}</p>
      </div>
    {/if}
    <div class="file-list" aria-label={tr('待发送文件')}>
      {#each files as file}
        <div class="file-row">
          <TransferFileVisual kind={file.kind} name={file.name} thumbnail={file.thumbnailDataUrl} />
          <div class="file-copy"><strong title={file.name}>{file.name}</strong>
            <p>{'preparing' in file && file.preparing ? tr('正在读取文件信息') : file.size === null ? '—' : bytes(file.size)}</p>
            {#if 'error' in file && file.error}<p class="error-text">{tr(file.error)}</p>{/if}
          </div>
          {#if !transferId && 'preparing' in file}
            <button class="icon-button" disabled={sending} aria-label={tr('移除 {name}', { name: file.name })} onclick={() => draft.remove(file.id)}><X size={18} /></button>
          {:else if completed}<CheckCircle size={22} class="success-icon" />{/if}
        </div>
      {/each}
    </div>
    {#if !transferId}
      <div class="file-summary">
        <p>{tr('{count} 个文件 · {size}', { count: files.length, size: bytes(total) })}</p>
        <button class="text-button clear-files" disabled={sending} title={tr('清空')} onclick={clearFiles}>{tr('清空')}</button>
      </div>
    {:else if files.length > 1}<p class="file-total">{tr('{count} 个文件 · {size}', { count: files.length, size: bytes(total) })}</p>{/if}

    {#if mode === 'choose'}
      <section class="device-section">
        <div class="section-heading"><div><h2>{tr('发送到')}</h2><p>{tr('选择一台附近设备')}</p></div>
          <button class="icon-button" disabled={refreshing} aria-label={tr('刷新设备')} onclick={refresh}><ArrowClockwise size={19} class={refreshing ? 'spinning' : ''} /></button>
        </div>
        {#if !networkReady || !snapshot.peers.length}
          <div class="empty-devices"><Laptop size={30} weight="light" /><strong>{tr(networkReady ? '暂无附近设备' : '正在查找附近设备')}</strong><p>{tr('请在接收设备上打开 ArcRelay，并连接同一网络')}</p></div>
        {:else}
          <div class="device-list" role="radiogroup" aria-label={tr('接收设备')}>
            {#each snapshot.peers as device}
              <label class="device-row" class:selected={selectedPeerId === device.id} class:busy={draft.targetBusy(device.id)}>
                <input type="radio" name="recipient" value={device.id} bind:group={selectedPeerId} disabled={sending || draft.targetBusy(device.id)} />
                {#if /ios|android/i.test(device.platform)}<DeviceMobile size={30} weight="light" />{:else}<Laptop size={32} weight="light" />{/if}
                <span class="device-copy"><strong title={device.name}>{device.name}</strong><small><span class="online-dot"></span>{tr(draft.targetBusy(device.id) ? '正在接收文件' : device.paired ? '在线 · 已配对' : '在线 · 需对方确认')}</small></span>
                <span class="radio-mark" aria-hidden="true">{#if selectedPeerId === device.id}<Check size={13} weight="bold" />{/if}</span>
              </label>
            {/each}
          </div>
        {/if}
        {#if selectedPeerId && !peer}<p class="error-text" role="status">{tr('设备已离线，请重新选择')}</p>{/if}
      </section>
      <button class="text-button add-files" disabled={sending} onclick={pick}><Plus size={18} />{tr('添加文件')}</button>
      <button class="primary-button" disabled={!canSend} onclick={send}>{sending ? tr('正在准备') : peer ? tr('发送到 {device}', { device: peer.name }) : tr('选择接收设备')}</button>
      <p class="panel-footnote">{tr('通过本地网络传输')}</p>
    {:else if active}
      <div class="recipient-summary"><Laptop size={30} weight="light" /><div><strong>{task?.peerName ?? batch?.peerName ?? tr('接收设备')}</strong><p>{tr(task ? trayTransferStatus(task.status) : '正在准备')}</p></div></div>
      <div class="progress-section" aria-live="polite">
        <div class="progress-heading"><strong>{tr(task ? trayTransferStatus(task.status) : '正在准备')}</strong><span>{task && ['transferring', 'paused'].includes(task.status) ? `${trayTransferProgress(task)}%` : ''}</span></div>
        {#if task && ['transferring', 'paused'].includes(task.status)}
          <progress max="100" value={trayTransferProgress(task)} aria-label={tr('发送进度')}></progress>
          <div class="progress-meta"><span>{bytes(task.completedBytes)} / {bytes(task.totalBytes)}</span><span>{task.remainingSeconds !== null ? tr('约剩 {seconds} 秒', { seconds: task.remainingSeconds }) : tr('正在估算剩余时间')}</span></div>
          <p class="speed">{bytes(task.speedBytesPerSecond)}/s</p>
        {:else}<div class="waiting-note"><Clock size={20} /><span>{tr(task?.status === 'awaitingApproval' ? '请在接收设备上确认本次传输' : '准备完成后将开始传输')}</span></div>{/if}
      </div>
      {#if task?.status === 'paused'}<button class="primary-button" disabled={commandBusy} onclick={() => run(() => port.resume(transferId))}>{tr('继续发送')}</button>{/if}
      <button class="secondary-button" onclick={hide}>{tr('收起到菜单栏')}</button>
      <button class="text-button centered" disabled={commandBusy || !task} onclick={() => run(() => port.cancel(transferId))}>{tr('取消发送')}</button>
      <p class="panel-footnote">{tr('关闭面板后，传输仍会继续')}</p>
    {:else}
      {#if !completed && batch}<button class="primary-button" disabled={commandBusy} onclick={() => run(retry)}>{tr('重新选择设备')}</button>{/if}
      <button class:primary-button={completed || !batch} class:secondary-button={!completed && Boolean(batch)} onclick={() => run(done)}>{tr($draft.files.length ? '发送下一批文件' : '完成')}</button>
      <button class="text-button centered" onclick={() => run(() => port.history())}>{tr('查看传输记录')}</button>
    {/if}
    {#if transferId && $draft.files.length}<p class="queued-note">{tr('另有 {count} 个文件待发送', { count: $draft.files.length })}</p>{/if}
  {/if}
  {#if error}<div class="error-banner" role="alert"><WarningCircle size={18} /><span>{error}</span><button class="icon-button" aria-label={tr('关闭提示')} onclick={() => error = ''}><X size={16} /></button></div>{/if}
</section>
{#if port.preview && mode === 'hover'}<button class="preview-action" onclick={pick}>预览：投放示例文件</button>{/if}
