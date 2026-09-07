<script lang="ts">
  import { onMount } from 'svelte';
  import { FolderOpen } from 'phosphor-svelte';
  import { bridge } from './bridge';
  import type { SystemFolder } from './ipc/generated';
  import type { LanguagePreference } from './types';

  export let peerId: string;
  export let shareId: string;
  export let language: LanguagePreference;
  export let notify: (message: string, kind?: 'success' | 'error') => void;
  let folders: SystemFolder[] = [];
  let busy = false;
  let removing: SystemFolder | null = null;
  let disposed = false;
  const mac = /Mac/i.test(navigator.platform);
  $: zh = language === 'zhCn' || (language === 'system' && navigator.language.startsWith('zh'));
  $: selected = folders.find(f => f.peerId === peerId && f.shareId === shareId);

  async function refresh() {
    try { const result = await bridge.listSystemFolders(); if (!disposed) folders = result; }
    catch (error) { if (!disposed) notify(String(error), 'error'); }
  }
  onMount(() => {
    if (!mac || !bridge.isTauri()) return;
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

{#if mac}
  <section class="system-folders" aria-label={zh ? 'Finder 文件夹' : 'Finder folders'}>
    <div class="heading">
      <FolderOpen size={18} />
      <strong>{zh ? 'Finder 文件夹' : 'Finder folders'}</strong>
      <button disabled={busy || !peerId || !shareId || !bridge.isTauri()} on:click={() => act(selected?.registered ? () => bridge.openSystemFolder(selected.id) : add)}>
        {busy ? (zh ? '处理中…' : 'Working…') : selected?.registered ? (zh ? '在 Finder 中打开' : 'Open in Finder') : (zh ? '添加到 Finder' : 'Add to Finder')}
      </button>
      {#if selected?.error}<button disabled={busy} on:click={() => act(add)}>{zh ? '重试接入' : 'Retry setup'}</button>{/if}
    </div>
    <p>{zh ? '需要 macOS 13+ 安装版。已下载文件可离线使用；本地修改联网后校验再上传。未下载文件需要对端在线，写入需对端授权。' : 'Requires the installed macOS 13+ app. Downloaded files remain available offline; edits upload after a version check when reconnected. Uncached files need an online peer; writing requires its permission.'}</p>
    {#each folders as folder (folder.id)}
      <div class="folder">
        <button class="folder-name" disabled={busy || !folder.registered} on:click={() => act(() => bridge.openSystemFolder(folder.id))}>{folder.name}</button>
        <span class:offline={!folder.online}>{folder.online ? (zh ? '在线' : 'Online') : (zh ? '离线 · 保留本地副本' : 'Offline · local copies retained')}</span>
        <button disabled={busy} on:click={() => removing = folder}>{zh ? '移除接入' : 'Disconnect'}</button>
        {#if folder.error}<small role="status">{folder.error}</small>{/if}
      </div>
    {/each}
    {#if folders.length}<button class="recovery" disabled={busy} on:click={() => act(() => bridge.openSystemFolderRecovery())}>{zh ? '打开失败写入的恢复副本' : 'Open recovery copies of failed saves'}</button>{/if}
    {#if removing}
      <div class="confirmation" role="alert">
        <p>{zh ? `移除“${removing.name}”的 Finder 接入？不会删除对端文件，系统会保留已下载数据。请先保存正在编辑的文件。` : `Disconnect “${removing.name}” from Finder? Remote files are not deleted and the system preserves downloaded data. Save open documents first.`}</p>
        <button disabled={busy} on:click={() => removing = null}>{zh ? '取消' : 'Cancel'}</button>
        <button disabled={busy} on:click={() => act(async () => { if (removing) await bridge.removeSystemFolder(removing.id); removing = null; })}>{zh ? '确认移除' : 'Disconnect'}</button>
      </div>
    {/if}
  </section>
{/if}

<style>
  .system-folders { margin: 0 18px 12px; padding: 12px 14px; border: 1px solid var(--border); border-radius: 10px; background: var(--surface); }
  .heading, .folder { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .heading strong { margin-right: auto; font-size: 13px; }
  button { padding: 5px 9px; border: 1px solid var(--border-strong); border-radius: 6px; color: var(--text); background: var(--control-bg); cursor: pointer; font-size: 12px; }
  button:disabled { opacity: .5; cursor: default; }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  p, small, .folder span { font-size: 12px; color: var(--text-muted); line-height: 1.5; }
  p { margin: 8px 0 0; }
  .folder { margin-top: 9px; }
  .folder-name { margin-right: auto; max-width: 60%; overflow: hidden; text-overflow: ellipsis; }
  .folder small { flex-basis: 100%; overflow-wrap: anywhere; }
  .offline { color: var(--text-muted); }
  .recovery { margin-top: 10px; }
  .confirmation { border-top: 1px solid var(--border); margin-top: 10px; padding-top: 8px; }
  .confirmation p { margin-bottom: 8px; }
</style>
