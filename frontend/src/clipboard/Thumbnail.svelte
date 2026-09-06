<script lang="ts">
  import { ImageSquare } from "phosphor-svelte";

  import type { LanguagePreference } from "../types";
  import { tr } from "./i18n";
  import { cachedThumbnail, loadCachedThumbnail } from "./thumbnailCache";
  import type { ClipboardItem } from "./types";

  export let item: ClipboardItem;
  export let language: LanguagePreference;
  export let active = true;

  let source: string | null = cachedThumbnail(item.id) ?? null;
  let request = 0;

  $: if (active) void loadThumbnail(item.id);

  async function loadThumbnail(id: number) {
    const currentRequest = ++request;
    const cached = cachedThumbnail(id);
    if (cached !== undefined) {
      source = cached;
      return;
    }

    const value = await loadCachedThumbnail(id);
    if (currentRequest === request) source = value;
  }
</script>

{#if source}
  <img class="clipboard-thumbnail" src={source} alt={tr("剪贴板图片预览", language)} draggable="false" />
{:else}
  <div class="thumbnail-fallback">
    <ImageSquare size={34} />
    <span>{item.width && item.height ? `${item.width} × ${item.height}` : tr("图片", language)}</span>
  </div>
{/if}
