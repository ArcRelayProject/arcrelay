<script lang="ts">
  import { ArrowRight, DownloadSimple, Eye, FolderOpen, LockKey, ShieldCheck } from "phosphor-svelte";

  import type { ShareView } from "./types";

  export let shares: ShareView[] = [];
  export let openShare: (share: ShareView) => void;

  function accessLabel(share: ShareView) {
    if (share.mode === "password") return share.unlocked ? "密码保护 · 已解锁" : "需要密码";
    return "局域网公开";
  }
</script>

<section class="share-home" aria-labelledby="shares-heading">
  <div class="section-heading">
    <div><h2 id="shares-heading">共享文件夹</h2><p>选择一个由这台 Mac 明确开放的位置</p></div>
    <span>{shares.length} 个可见共享</span>
  </div>

  {#if shares.length === 0}
    <div class="share-list empty-share-list">
      <span class="empty-state-icon"><FolderOpen size={30} /></span>
      <strong>目前没有可访问的共享文件夹</strong>
      <p>请让主机在 ArcRelay 的“文件共享”设置中开放一个位置。</p>
    </div>
  {:else}
    <div class="share-list">
      <div class="share-list-heading" aria-hidden="true"><span>文件夹</span><span>访问方式</span><span>预览</span><span>下载</span><span>操作</span></div>
      {#each shares as share (share.slug)}
        <div class="share-row">
          <div class="share-identity"><span class="folder-icon"><FolderOpen size={24} weight="duotone" /></span><span><strong>{share.name}</strong><small>本机共享位置</small></span></div>
          <span class:protected={share.mode === "password"} class="access-state">{#if share.mode === "password"}<LockKey size={17} />{:else}<ShieldCheck size={17} />{/if}<span><strong>{accessLabel(share)}</strong><small>{share.mode === "password" ? "仅本次浏览器会话" : "同一局域网可访问"}</small></span></span>
          <span class:enabled={share.allowPreview} class="capability-state"><Eye size={17} />{share.allowPreview ? "可预览" : "不可预览"}</span>
          <span class:enabled={share.allowDownload} class="capability-state"><DownloadSimple size={17} />{share.allowDownload ? "可下载" : "不可下载"}</span>
          <button class="enter-share" on:click={() => openShare(share)}><span>{share.mode === "password" && !share.unlocked ? "解锁" : "进入"}</span><ArrowRight size={18} /></button>
        </div>
      {/each}
    </div>
  {/if}
</section>
