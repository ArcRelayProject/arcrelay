<script lang="ts">
  import { t } from "../localization";
  import { DevicesIcon, Star } from "phosphor-svelte";

  import type { LanguagePreference } from "../types";
  import ContentPreview from "./ContentPreview.svelte";
  import { localeFor, tr } from "./i18n";
  import type { ClipboardItem } from "./types";

  export let item: ClipboardItem;
  export let shortcutIndex: number | null;
  export let selected: boolean;
  export let multiSelected = false;
  export let multiSelectActive = false;
  export let selectionOrder: number | null = null;
  export let language: LanguagePreference;
  export let onSelect: (event?: MouseEvent) => void;
  export let onFocus: () => void;
  export let onPaste: (plainText: boolean) => void;
  export let onContextMenu: (event: MouseEvent) => void;
  export let timestamp: number;
  export let shortcutModifier: string;
  export let previewActive = true;
  export let timelineTarget = false;
  export let exactTime = false;

  function handleContextMenu(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    onContextMenu(event);
  }

  function handleDoubleClick(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    onPaste(event.shiftKey);
  }

  function handleKeyDown(event: KeyboardEvent) {
    if (event.key === "Enter" || event.key === " ") onSelect();
  }

  function formatBytes(bytes: number) {
    if (item.kind === "text" || item.kind === "html") {
      const count = item.characterCount ?? 0;
      return t("{count} 个字符", language, { count: count });
    }
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(2)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
  }

  function formatTime(timestamp: number, useExactTime: boolean) {
    if (useExactTime) return new Intl.DateTimeFormat(localeFor(language), { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(timestamp);
    const delta = Date.now() - timestamp;
    if (delta < 60_000) return tr("刚刚", language);
    if (delta < 60 * 60_000) {
      const count = Math.floor(delta / 60_000);
      return new Intl.RelativeTimeFormat(localeFor(language), { numeric: "auto" }).format(-count, "minute");
    }
    if (delta < 24 * 60 * 60_000) {
      const count = Math.floor(delta / 60 / 60_000);
      return new Intl.RelativeTimeFormat(localeFor(language), { numeric: "auto" }).format(-count, "hour");
    }
    if (delta < 48 * 60 * 60_000) return tr("昨天", language);
    return new Intl.DateTimeFormat(localeFor(language), { month: "numeric", day: "numeric" }).format(timestamp);
  }
</script>

<div
  class:selected
  class:multi-selected={multiSelected}
  class:unavailable={!item.available}
  class="clipboard-row"
  role="button"
  tabindex="0"
  onclick={(event) => onSelect(event)}
  onfocus={onFocus}
  onkeydown={handleKeyDown}
  oncontextmenu={handleContextMenu}
  ondblclick={handleDoubleClick}
  aria-disabled={!item.available}
  aria-label={t("{name}，双击插入", language, { name: item.sourceApp ?? "剪贴板" })}
>
  {#if selectionOrder !== null}
    <span class="selection-order" aria-label={t("第 {count} 个选择", language, { count: selectionOrder })}>{selectionOrder}</span>
  {/if}
  <div class="clipboard-content">
    <ContentPreview {item} {language} active={previewActive} />
  </div>
  <div class="clipboard-meta">
    {#if timelineTarget}<span class="timeline-target-badge">{tr("定位记录", language)}</span>{/if}
    {#if !multiSelectActive}
      {#if shortcutIndex !== null}
        <span class="item-number" title={t("按 {shortcut} 快速插入", language, { shortcut: `${shortcutModifier}${shortcutIndex}` })}>{shortcutIndex}</span>
      {:else}
        <span class="item-number placeholder" aria-hidden="true"></span>
      {/if}
    {/if}
    <strong>{item.sourceApp ?? tr("此电脑", language)}</strong>
    {#if item.sourceDeviceName}
      <span
        class="remote-device-indicator"
        title={t("来自其他设备：{name}", language, { name: item.sourceDeviceName })}
        aria-label={t("来自其他设备：{name}", language, { name: item.sourceDeviceName })}
      ><DevicesIcon size={15} weight="bold" /></span>
    {/if}
    {#if item.favorite}<Star size={14} weight="fill" class="meta-pin" />{/if}
    {#each item.labels.slice(0, 2) as label (label.id)}
      <span class="label-chip" style:--label-color={label.color}>{label.name}</span>
    {/each}
    {#if item.labels.length > 2}<span class="label-chip">+{item.labels.length - 2}</span>{/if}
    <span class="meta-spacer"></span>
    <span>{formatBytes(item.sizeBytes)}</span>
    <span>{formatTime(timestamp, exactTime)}</span>
  </div>
</div>

<style>
  /* Keep the row as the hit target while asynchronous HTML/image previews
     replace their children between the first and second click. */
  .clipboard-content {
    pointer-events: none;
  }
</style>
