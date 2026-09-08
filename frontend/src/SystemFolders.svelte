<script lang="ts">
  import { onMount } from 'svelte';
  import { FolderOpen, CaretDown, X, WarningCircle } from 'phosphor-svelte';
  import { Dialog, DropdownMenu } from 'bits-ui';
  import { bridge } from './bridge';
  import type { SystemFolder } from './ipc/generated';
  import type { LanguagePreference, RemoteFileDevice, RemoteFileShare } from './types';

  export let peerId: string;
  export let shareId: string;
  export let devices: RemoteFileDevice[] = [];
  export let shares: RemoteFileShare[] = [];
  export let language: LanguagePreference;
  export let notify: (message: string, kind?: 'success' | 'error') => void;
  let folders: SystemFolder[] = [];
  let busy = false;
  let removing: SystemFolder | null = null;
  let disposed = false;
  let settingsOpen = false;
  let loadError = "";
  $: if (!settingsOpen) removing = null;
  $: availableShares = shares.filter(share => !folders.some(folder => folder.peerId === peerId && folder.shareId === share.id));
  $: deviceGroups = [...new Set(folders.map(folder => folder.peerId))].map(id => ({ id, name: devices.find(device => device.id === id)?.name ?? `${zh ? "设备" : "Device"} · ${id.slice(0, 8)}`, folders: folders.filter(folder => folder.peerId === id) }));
  $: recoveryCount = folders.reduce((count, folder) => count + folder.recoveryCount, 0);
  const mac = /Mac/i.test(navigator.platform);
  const windows = /Win/i.test(navigator.platform);
  const supported = mac || windows;
  $: fileManager = mac ? 'Finder' : (zh ? '资源管理器' : 'File Explorer');
  $: zh = language === 'zhCn' || (language === 'system' && navigator.language.startsWith('zh'));
  $: selected = folders.find(f => f.peerId === peerId && f.shareId === shareId);

  async function refresh() {
    try { const result = await bridge.listSystemFolders(); if (!disposed) { folders = result; loadError = ''; } }
    catch (error) { if (!disposed) loadError = String(error); }
  }
  onMount(() => {
    if (!supported || !bridge.isTauri()) return;
    void refresh();
    const timer = setInterval(() => { if (!busy) void refresh(); }, 10000);
    return () => { disposed = true; clearInterval(timer); };
  });
  async function act(work: () => Promise<unknown>) {
    if (busy) return;
    busy = true;
    try { await work(); await refresh(); }
    catch (error) { notify(String(error), 'error'); await refresh(); }
    finally { busy = false; }
  }
  async function add() {
    const folder = await bridge.addSystemFolder(peerId, shareId);
    await refresh();
    await bridge.openSystemFolder(folder.id);
  }
</script>

{#if supported}
  <div class="finder-actions">
    <button class="finder-open" title={selected?.registered ? (zh ? `在 ${fileManager} 中打开共享根目录` : `Open shared root in ${fileManager}`) : (zh ? `添加到 ${fileManager}` : `Add to ${fileManager}`)}
      disabled={busy || !peerId || !shareId || !bridge.isTauri() || Boolean(loadError)}
      on:click={() => act(selected?.registered ? () => bridge.openSystemFolder(selected.id) : add)}>
      <FolderOpen size={17} /><span>{busy ? (zh ? '处理中…' : 'Working…') : selected?.registered ? (zh ? `在 ${fileManager} 中打开` : `Open in ${fileManager}`) : (zh ? `添加到 ${fileManager}` : `Add to ${fileManager}`)}</span>
    </button>
    <DropdownMenu.Root>
      <DropdownMenu.Trigger class="finder-menu-trigger" aria-label={zh ? `${fileManager} 接入选项` : `${fileManager} integration options`} title={zh ? `${fileManager} 接入选项` : `${fileManager} integration options`}><CaretDown size={12} /></DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content class="finder-menu" sideOffset={6} align="end">
          <DropdownMenu.Item class="finder-menu-item" onSelect={() => { settingsOpen = true; if (bridge.isTauri()) void refresh(); }}>{zh ? `${fileManager} 接入设置` : `${fileManager} settings`}</DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
    {#if recoveryCount > 0}
      <button class="recovery-notice" title={zh ? '有文件未能写回远程设备，查看恢复副本' : 'Some files could not be saved remotely. View recovery copies'} disabled={busy} on:click={() => act(() => bridge.openSystemFolderRecovery())}><WarningCircle size={16} /><span>{zh ? '查看恢复副本' : 'View recovery copies'} ({recoveryCount})</span></button>
    {:else if selected?.error || loadError}
      <button class="recovery-notice" on:click={() => settingsOpen = true} title={zh ? `查看 ${fileManager} 接入问题` : `View ${fileManager} issue`}><WarningCircle size={16} /><span>{zh ? '接入异常' : `${fileManager} issue`}</span></button>
    {/if}
  </div>
  <Dialog.Root bind:open={settingsOpen}>
    <Dialog.Portal>
      <Dialog.Overlay class="finder-overlay" />
      <Dialog.Content class="finder-settings">
        <div class="heading">
          <FolderOpen size={20} />
          <Dialog.Title class="dialog-title">{zh ? `${fileManager} 接入设置` : `${fileManager} settings`}</Dialog.Title>
          <Dialog.Close class="finder-close" aria-label={zh ? '关闭' : 'Close'}><X size={18} /></Dialog.Close>
        </div>
        <Dialog.Description class="finder-description">{zh ? `将共享文件夹添加到本机 ${fileManager}。应用内文件浏览始终可用。` : `Add shared folders to ${fileManager} on this computer. In-app browsing remains available.`}</Dialog.Description>
        {#if mac}<p>{zh ? '需要 macOS 13+ 安装版。已下载文件可离线使用；本地修改联网后校验再上传。未下载文件需要对端在线，写入需对端授权。' : 'Requires the installed macOS 13+ app. Downloaded files remain available offline; edits upload after a version check when reconnected. Uncached files need an online peer; writing requires its permission.'}</p>{:else}<p>{zh ? '通过 Windows WebClient 添加到“此电脑”的网络位置。需要本机 ArcRelay 与对端在线，不提供离线同步。写入需对端授权；不支持文件锁或覆盖移动。系统 WebClient 的文件大小限制仍然适用。' : 'Adds a network location under This PC using Windows WebClient. ArcRelay and the peer must be online; offline sync is unavailable. Writing requires permission. File locks and overwrite moves are unsupported. Windows WebClient file-size limits apply.'}</p>{/if}
        {#if loadError}<div class="finder-error" role="status">{loadError}<button disabled={busy} on:click={() => void refresh()}>{zh ? '重试' : 'Retry'}</button></div>{/if}
        <div class="folder-list">
          {#each deviceGroups as group (group.id)}
            <h3>{group.name}</h3>
            {#each group.folders as folder (folder.id)}
            <div class="folder">
              <div class="folder-copy"><strong>{folder.name}</strong><span>{!folder.registered ? (zh ? '尚未接入' : 'Not connected') : folder.online ? (zh ? '在线' : 'Online') : (mac ? (zh ? '离线 · 保留本地副本' : 'Offline · local copies retained') : (zh ? '离线 · 暂不可访问' : 'Offline · unavailable'))}</span></div>
              <div class="folder-actions">
                {#if folder.registered}<button disabled={busy} on:click={() => act(() => bridge.openSystemFolder(folder.id))}>{zh ? '打开' : 'Open'}</button>{/if}
                {#if folder.error || !folder.registered}<button disabled={busy} on:click={() => act(() => bridge.addSystemFolder(folder.peerId, folder.shareId))}>{zh ? '重试接入' : 'Retry setup'}</button>{/if}
                <button disabled={busy} on:click={() => removing = folder}>{zh ? '移除接入' : 'Disconnect'}</button>
              </div>
              {#if folder.error}<small class="finder-error" role="status">{folder.error}</small>{/if}
            </div>
            {/each}
          {:else}{#if !loadError}<p>{zh ? `尚未添加文件夹。每个共享文件夹会在 ${fileManager} 中显示为独立入口。` : `No folders added. Each shared folder appears as a separate ${fileManager} location.`}</p>{/if}{/each}
        </div>
        {#if availableShares.length && !loadError}
          <div class="available-shares">
            <h3>{zh ? '可添加的共享文件夹' : 'Available shared folders'} · {devices.find(device => device.id === peerId)?.name ?? peerId}</h3>
            <p>{zh ? `逐项添加到 ${fileManager}；其他设备的共享文件夹可在切换设备后添加。` : 'Add folders individually. Switch devices to add shared folders from another device.'}</p>
            {#each availableShares as share (share.id)}
              <div class="folder"><div class="folder-copy"><strong>{share.name}</strong><span>{share.writable ? (zh ? '可读写' : 'Read and write') : (zh ? '只读' : 'Read only')}</span></div><button disabled={busy || !bridge.isTauri()} on:click={() => act(() => bridge.addSystemFolder(peerId, share.id))}>{zh ? `添加到 ${fileManager}` : `Add to ${fileManager}`}</button></div>
            {/each}
          </div>
        {/if}
        {#if recoveryCount > 0}<div class="recovery-box" role="status"><p>{zh ? `有 ${recoveryCount} 份未完成写回的本地副本。请检查文件内容后自行恢复。` : `${recoveryCount} local copies were not saved remotely. Inspect their contents before recovery.`}</p><button disabled={busy} on:click={() => act(() => bridge.openSystemFolderRecovery())}>{zh ? '查看恢复副本' : 'View recovery copies'}</button></div>{/if}
        {#if removing}
          <div class="confirmation" role="alert">
            <p>{zh ? `移除“${removing.name}”的 ${fileManager} 接入？不会删除对端文件，已另存到本机的文件会保留。请先保存正在编辑的文件。` : `Disconnect “${removing.name}” from ${fileManager}? Remote files are not deleted and files saved locally are retained. Save open documents first.`}</p>
            <button disabled={busy} on:click={() => removing = null}>{zh ? '取消' : 'Cancel'}</button>
            <button disabled={busy} on:click={() => act(async () => { if (removing) await bridge.removeSystemFolder(removing.id); removing = null; })}>{zh ? '确认移除' : 'Disconnect'}</button>
          </div>
        {/if}
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
{/if}

<style>
  .finder-actions { display: flex; align-items: center; flex: 0 0 auto; gap: 2px; }
  button { display: inline-flex; align-items: center; justify-content: center; gap: 6px; padding: 5px 9px; border: 1px solid var(--border-strong); border-radius: 6px; color: var(--text); background: var(--control-bg); cursor: pointer; font-size: 12px; white-space: nowrap; }
  button:disabled { opacity: .5; cursor: default; }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .finder-open { height: 34px; border-color: transparent; background: transparent; }
  .finder-open:hover:not(:disabled) { background: var(--surface-soft); }
  :global(.finder-menu-trigger), :global(.finder-close) { display: inline-flex; align-items: center; justify-content: center; width: 28px; height: 34px; border: 0; border-radius: 6px; background: transparent; color: var(--text); cursor: pointer; }
  :global(.finder-menu-trigger:focus-visible), :global(.finder-close:focus-visible) { outline: 2px solid var(--accent); }
  :global(.finder-menu) { z-index: 100; padding: 5px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-raised); color: var(--text); box-shadow: var(--shadow-floating); }
  :global(.finder-menu-item) { padding: 8px 12px; border-radius: 5px; font-size: 12px; cursor: pointer; outline: none; }
  :global(.finder-menu-item[data-highlighted]) { background: var(--surface-soft); }
  :global(.finder-overlay) { position: fixed; inset: 0; z-index: 100; background: rgb(0 0 0 / .25); }
  :global(.finder-settings) { position: fixed; z-index: 101; top: 50%; left: 50%; transform: translate(-50%, -50%); box-sizing: border-box; padding: 24px; border: 1px solid var(--border-strong); border-radius: 14px; background: var(--surface-raised); color: var(--text); box-shadow: var(--shadow-floating); width: min(600px, calc(100vw - 40px)); max-height: calc(100vh - 64px); overflow: auto; }
  :global(.finder-settings .dialog-title) { margin: 0; font-size: 18px; font-weight: 650; }
  .heading { display: flex; align-items: center; gap: 10px; }
  :global(.finder-close) { margin-left: auto; }
  p, small, .folder span, :global(.finder-description) { font-size: 12px; color: var(--text-muted); line-height: 1.6; }
  p { margin: 10px 0; }
  :global(.finder-description) { margin-top: 14px; }
  h3 { font-size: 13px; margin: 18px 0 8px; color: var(--text-secondary); }
  .available-shares { margin-top: 18px; }
  .folder-list { margin-top: 20px; }
  .folder { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; padding: 14px 0; border-top: 1px solid var(--border); }
  .folder-copy { flex: 1 1 200px; min-width: 0; display: grid; gap: 5px; }
  .folder-copy strong { font-size: 13px; overflow-wrap: anywhere; }
  .folder-actions { display: flex; gap: 6px; }
  .folder small { flex-basis: 100%; overflow-wrap: anywhere; }
  .finder-error { color: var(--danger); overflow-wrap: anywhere; }
  .recovery-notice { border: 0; color: var(--danger); background: transparent; }
  .recovery-box, .confirmation { border-top: 1px solid var(--border); padding-top: 10px; margin-top: 10px; }
  @media (max-width: 1240px) { .finder-open span, .recovery-notice span { display: none; } }
</style>
