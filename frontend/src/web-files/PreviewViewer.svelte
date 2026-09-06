<script lang="ts">
  import { DownloadSimple, FileAudio, FileImage, FileText, FileVideo, ShieldCheck, X } from "phosphor-svelte";

  import { api, endpoint } from "./api";
  import type { FileEntry, ShareView, TextPreview } from "./types";

  export let share: ShareView;
  export let entry: FileEntry;
  export let close: () => void;

  let textPreview: TextPreview | null = null;
  let loading = false;
  let error = "";

  $: if (entry.previewKind === "text" && share.allowPreview) void loadText(entry.relativePath);

  async function loadText(path: string) {
    textPreview = null;
    error = "";
    loading = true;
    try { textPreview = await api.text(share.slug, path); }
    catch (value) { error = value instanceof Error ? value.message : String(value); }
    finally { loading = false; }
  }

  function contentUrl(download = false) {
    return endpoint(share.slug, "content", entry.relativePath, download ? { download: "true" } : undefined);
  }
  function formatSize(size: number) {
    if (size < 1024) return `${size} B`;
    if (size < 1024 ** 2) return `${(size / 1024).toFixed(1)} KB`;
    if (size < 1024 ** 3) return `${(size / 1024 ** 2).toFixed(1)} MB`;
    return `${(size / 1024 ** 3).toFixed(1)} GB`;
  }
</script>

<svelte:window on:keydown={(event) => event.key === "Escape" && close()} />

<div class="preview-backdrop" role="button" tabindex="0" aria-label="关闭预览" on:click|self={close} on:keydown={(event) => event.key === "Escape" && close()}>
  <div class="preview-panel" role="dialog" aria-modal="true" aria-label={`预览 ${entry.name}`}>
    <header>
      <span class={`preview-file-icon ${entry.previewKind}`}>{#if entry.previewKind === "image"}<FileImage size={27} />{:else if entry.previewKind === "video"}<FileVideo size={27} />{:else if entry.previewKind === "audio"}<FileAudio size={27} />{:else}<FileText size={27} />{/if}</span>
      <div><strong>{entry.name}</strong><small>{entry.mediaType} · {formatSize(entry.size)}</small></div>
      <button class="icon-button" aria-label="关闭预览" on:click={close}><X size={20} /></button>
    </header>

    <div class="preview-content">
      {#if !share.allowPreview}
        <div class="preview-message"><strong>此共享未启用网页预览</strong><p>如果允许下载，可以保存后使用本机应用打开。</p></div>
      {:else if loading}
        <div class="preview-message"><span class="spinner"></span><p>正在载入预览…</p></div>
      {:else if error}
        <div class="preview-message"><strong>无法预览</strong><p>{error}</p></div>
      {:else if entry.previewKind === "text" && textPreview}
        <div class="text-preview">{#if textPreview.truncated}<div class="truncate-note">文件较大，仅显示前 512 KiB。</div>{/if}<pre>{textPreview.text}</pre></div>
      {:else if entry.previewKind === "image"}
        <img class="image-preview" src={endpoint(share.slug, "thumbnail", entry.relativePath, { dimension: "2048" })} alt={entry.name} />
      {:else if entry.previewKind === "audio"}
        <div class="media-preview"><FileAudio size={68} weight="duotone" /><strong>{entry.name}</strong><audio controls preload="metadata" src={contentUrl()}></audio></div>
      {:else if entry.previewKind === "video"}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video class="video-preview" controls preload="metadata" playsinline src={contentUrl()}></video>
      {:else}
        <div class="preview-message"><strong>当前浏览器不支持直接预览</strong><p>可下载后使用本地应用打开。ArcRelay 不会在服务器端转码此文件。</p></div>
      {/if}
    </div>

    <footer>
      <span class="preview-trust"><ShieldCheck size={17} />预览内容直接来自 {share.name}，不会上传到云端。<kbd>Esc</kbd></span>
      <div><button class="secondary-button" on:click={close}>关闭</button>{#if share.allowDownload}<a class="primary-action" href={contentUrl(true)} download={entry.name}><DownloadSimple size={18} />下载原文件</a>{/if}</div>
    </footer>
  </div>
</div>
