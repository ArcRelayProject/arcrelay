<script lang="ts">
  import { t } from "../localization";
  import { FileText, LinkSimpleHorizontal, LockSimple } from "phosphor-svelte";

  import type { LanguagePreference } from "../types";
  import { cachedHtmlPreview, loadCachedHtmlPreview } from "./htmlPreviewCache";
  import ImagePreview from "./ImagePreview.svelte";
  import { tr } from "./i18n";
  import Thumbnail from "./Thumbnail.svelte";
  import type { ClipboardItem } from "./types";

  export let item: ClipboardItem;
  export let language: LanguagePreference;
  export let interactiveImage = false;
  export let active = true;
  export let imageSelectionBusy = false;
  export let onCopyImageText: (content: string) => Promise<void> | void = () => undefined;
  export let onPasteImageText: (content: string) => Promise<void> | void = () => undefined;

  let html: string | null = item.kind === "html" ? (cachedHtmlPreview(item) ?? null) : null;
  let htmlItemVersion: string | null = null;
  let request = 0;

  $: {
    const nextVersion = active && item.kind === "html"
      ? `${item.id}:${item.updatedAtMs}:${item.sizeBytes}:${item.preview}`
      : null;
    if (nextVersion !== htmlItemVersion) {
      htmlItemVersion = nextVersion;
      if (nextVersion) void loadHtml(item, nextVersion);
      else {
        request += 1;
        html = null;
      }
    }
  }

  const looksLikeUrl = (value: string) => /^https?:\/\//i.test(value.trim());
  const looksLikeCode = (value: string) =>
    value.includes("\n") || /^(git|npm|pnpm|yarn|cargo|cd)\s/m.test(value.trim());

  async function loadHtml(nextItem: ClipboardItem, version: string) {
    const currentRequest = ++request;
    const cached = cachedHtmlPreview(nextItem);
    if (cached !== undefined) {
      html = cached;
      return;
    }

    html = null;
    const value = await loadCachedHtmlPreview(nextItem);
    if (currentRequest === request && htmlItemVersion === version) html = value;
  }
</script>

{#if item.sensitive}
  <div class:image-sensitive-preview={item.kind === "image"} class="sensitive-preview">
    <LockSimple size={21} />
    <span>{tr("敏感内容已隐藏", language)}</span>
  </div>
{:else if item.kind === "image"}
  {#if interactiveImage}
    <ImagePreview
      {item}
      {language}
      busy={imageSelectionBusy}
      onCopy={onCopyImageText}
      onPaste={onPasteImageText}
    />
  {:else}
    <Thumbnail {item} {language} {active} />
  {/if}
{:else if item.kind === "html"}
  {#if html}
    <div class="html-preview" aria-label={item.preview}>{@html html}</div>
  {:else}
    <div class="html-preview html-preview-placeholder"><span>{item.preview}</span></div>
  {/if}
{:else if item.kind === "files"}
  <div class="file-preview">
    <FileText size={23} />
    <div>
      <strong>{item.preview}</strong>
      <span>{item.itemCount > 1 ? (t("{count} 个文件", language, { count: item.itemCount })) : tr("文件", language)}</span>
    </div>
  </div>
{:else if looksLikeCode(item.preview)}
  <pre class="code-preview">{item.preview}</pre>
{:else}
  <div class:link-preview={looksLikeUrl(item.preview)} class="text-preview">
    {#if looksLikeUrl(item.preview)}<LinkSimpleHorizontal size={19} />{/if}
    <span>{item.preview}</span>
  </div>
{/if}
