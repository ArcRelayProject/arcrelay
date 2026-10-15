<script lang="ts">
  import { t } from "../localization";
  import type { LanguagePreference } from "../types";
  import { tr } from "./i18n";
  import type { ClipboardItem } from "./types";
  import FileIcon from "./FileIcon.svelte";
  import { cachedFileNames, filePreviewKey, loadFileNames } from "./filePreviewCache";

  export let item: ClipboardItem;
  export let language: LanguagePreference;
  export let active = true;
  export let detailed = false;
  export let onPreview: () => void = () => {};

  let names: string[] | undefined;
  let error = "";
  let version: string | null = null;
  let request = 0;

  $: {
    const next = active && !item.sensitive ? filePreviewKey(item) : null;
    if (next !== version) {
      version = next;
      request += 1;
      names = cachedFileNames(item);
      error = "";
      if (next && names === undefined) void load(item, next);
    }
  }
  $: visibleNames = detailed ? (names ?? []) : (names ?? []).slice(0, 2);
  $: remaining = Math.max(0, item.itemCount - visibleNames.length);

  async function load(snapshot: ClipboardItem, key: string) {
    const currentRequest = ++request;
    error = "";
    try {
      const result = await loadFileNames(snapshot);
      if (currentRequest === request && version === key) names = result;
    } catch {
      if (currentRequest === request && version === key) error = tr("无法读取文件列表", language);
    }
  }

  function fileType(name: string) {
    const extension = name.includes(".") ? name.split(".").pop()! : "";
    return extension
      ? t("{type} 文件", language, { type: extension.toUpperCase() })
      : tr("文件", language);
  }

  function formatSize(bytes: number) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(2)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
  }
</script>

<div
  class="file-preview"
  class:detailed
  class:multiple={item.itemCount > 1}
  aria-busy={names === undefined && !error}
>
  <div class="file-preview-summary">
    <strong>{t("{count} 个文件", language, { count: item.itemCount })}</strong>
    {#if detailed}<span>· {formatSize(item.sizeBytes)}</span>{/if}
  </div>
  {#if error}
    <div class="file-preview-state" role="status">
      <span>{error}</span>
      <button
        type="button"
        aria-disabled={false}
        on:pointerdown|stopPropagation
        on:dblclick|stopPropagation
        on:click|stopPropagation={() => {
          if (version) void load(item, version);
        }}>{tr("重试", language)}</button
      >
    </div>
  {:else if names === undefined}
    <div class="file-preview-state" role="status">{tr("正在读取文件信息", language)}</div>
  {:else if names.length === 0}
    <div class="file-preview-state" role="status">{tr("文件列表为空", language)}</div>
  {:else}
    <ul class="file-preview-list" aria-label={tr("文件列表", language)}>
      {#each visibleNames as name}
        <li class="file-preview-entry">
          <FileIcon {name} compact={!detailed} />
          <div class="file-preview-name" title={name}>
            <span class="file-name">{name}</span>
            {#if detailed}<span class="file-type">{fileType(name)}</span>{/if}
          </div>
        </li>
      {/each}
    </ul>
    {#if !detailed && remaining > 0}
      <button
        class="file-preview-more"
        type="button"
        aria-disabled={false}
        aria-label={t("展开其余 {count} 个文件", language, { count: remaining })}
        on:pointerdown|stopPropagation
        on:dblclick|stopPropagation
        on:click|stopPropagation={() => {
          onPreview();
        }}>+{t("{count} 个文件", language, { count: remaining })}</button
      >
    {/if}
  {/if}
</div>

<style>
  .file-preview {
    min-width: 0;
    padding: 12px 14px;
    color: var(--text-primary);
  }
  .file-preview-summary {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 5px;
    margin-bottom: 8px;
    font-size: 14px;
    line-height: 1.4;
  }
  .file-preview-summary strong {
    font-weight: 650;
  }
  .file-preview-list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .file-preview-entry {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .file-preview-entry + .file-preview-entry {
    margin-top: 5px;
  }
  .file-preview-name {
    min-width: 0;
    flex: 1;
  }
  .file-name {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    line-height: 1.5;
  }
  .file-type {
    display: block;
    margin-top: 2px;
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 1.4;
  }
  .file-preview-more {
    pointer-events: auto;
    display: block;
    margin: 7px 0 0 32px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--accent-strong);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .file-preview-more:hover {
    text-decoration: underline;
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 3px;
    border-radius: 3px;
  }
  .file-preview-state {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    min-height: 24px;
    color: var(--text-secondary);
    font-size: 12px;
  }
  .multiple .file-preview-state {
    min-height: 53px;
  }
  .file-preview-state button {
    pointer-events: auto;
    padding: 2px 5px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface-raised);
    color: var(--accent-strong);
    font: inherit;
    cursor: pointer;
  }
  .detailed {
    padding: 0;
  }
  .detailed .file-preview-summary {
    margin: 0;
    padding: 14px 20px;
    border-bottom: 1px solid var(--border);
    background: var(--surface-soft);
    font-size: 14px;
  }
  .detailed .file-preview-entry {
    gap: 12px;
    padding: 8px 20px;
  }
  .detailed .file-preview-entry + .file-preview-entry {
    margin: 0;
    border-top: 1px solid var(--border);
  }
  .detailed .file-name {
    overflow: visible;
    white-space: normal;
    overflow-wrap: anywhere;
    font-size: 14px;
  }
  .detailed .file-preview-state {
    min-height: 90px;
    padding: 16px 20px;
  }
  @media (max-width: 380px) {
    .detailed .file-preview-entry {
      gap: 10px;
      padding: 12px 14px;
    }
    .detailed .file-preview-summary {
      padding-inline: 14px;
    }
  }
</style>
