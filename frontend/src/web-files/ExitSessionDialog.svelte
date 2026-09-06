<script lang="ts">
  import { FolderOpen, LockKey, SignOut, X } from "phosphor-svelte";

  export let busy = false;
  export let error = "";
  export let shareCount = 0;
  export let unlockedCount = 0;
  export let cancel: () => void;
  export let confirm: () => void | Promise<void>;
</script>

<svelte:window on:keydown={(event) => event.key === "Escape" && !busy && cancel()} />

<div class="dialog-backdrop" role="button" tabindex="0" aria-label="关闭退出确认" on:click|self={() => !busy && cancel()} on:keydown={(event) => event.key === "Escape" && !busy && cancel()}>
  <div class="session-dialog" role="dialog" aria-modal="true" aria-labelledby="exit-title">
    <header class="dialog-title-row">
      <span class="dialog-icon danger"><SignOut size={24} /></span>
      <button class="icon-button" aria-label="关闭" disabled={busy} on:click={cancel}><X size={19} /></button>
    </header>
    <div class="dialog-copy"><span class="dialog-eyebrow danger-text">退出浏览器会话</span><h2 id="exit-title">要退出当前会话吗？</h2><p>已解锁的密码共享将重新锁定。正在进行的下载不会被删除，但需要重新输入密码才能继续浏览。</p></div>
    <div class="impact-list">
      <span><FolderOpen size={18} /><strong>{shareCount}</strong> 个共享位置</span>
      <span><LockKey size={18} /><strong>{unlockedCount}</strong> 个已解锁文件夹</span>
    </div>
    {#if error}<div class="inline-error" role="alert">{error}</div>{/if}
    <footer class="dialog-actions"><button class="secondary-button" disabled={busy} on:click={cancel}>继续浏览</button><button class="danger-action" disabled={busy} on:click={confirm}>{busy ? "正在退出…" : "退出会话"}</button></footer>
  </div>
</div>
