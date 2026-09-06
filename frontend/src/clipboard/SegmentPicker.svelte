<script lang="ts">
  import { t } from "../localization";
  import { translate as uiTranslate, language as uiLanguage } from "../i18n";
  import { onDestroy } from "svelte";
  import { clipboardBridge } from "./bridge";
  import type { LanguagePreference } from "../types";
  import {
    textSliceParts,
    type TextSlice,
  } from "./textSegments";

  type PickerGranularity = "word" | "phrase";

  export let model: import("./textSegments").TextSliceModel;
  export let itemId: number;
  $: text = model.text;
  export let language: LanguagePreference;
  export let busy = false;
  export let errorMessage = "";
  export let sourceWarning = "";
  export let onCopy: (content: string) => Promise<void> | void;
  export let onPaste: (content: string) => Promise<void> | void;
  export let onPasteAll: () => Promise<void> | void;

  let modeledText = "";
  let selectedText = "";
  let selectionGeneration = 0;
  onDestroy(() => { selectionGeneration++; });
  let granularity: PickerGranularity = "phrase";
  let selectedIds = new Set<string>();
  let lastSelectedIndex: number | null = null;
  let focusedIndex = 0;
  let pickerRoot: HTMLElement;

  $: if (text !== modeledText) reset(text);
  $: slices = model.levels[granularity];
  $: renderParts = textSliceParts(text, slices);
  $: selectedSlices = slices.filter((slice) => selectedIds.has(slice.id));
  $: selectedOrderById = new Map(selectedSlices.map((slice, index) => [slice.id, index + 1]));
  $: void joinSelection(selectedSlices);
  $: unitLabel = granularity === "word"
    ? (uiTranslate("词", $uiLanguage))
    : (uiTranslate("段", $uiLanguage));

  function reset(source: string) {
    modeledText = source;
    selectedIds = new Set();
    lastSelectedIndex = null;
    focusedIndex = 0;
    requestAnimationFrame(() => focusSlice(0));
  }

  async function joinSelection(slices: TextSlice[]) {
    const generation = ++selectionGeneration;
    selectedText = "";
    if (!slices.length) return;
    try {
      const content = await clipboardBridge.joinSegments(itemId, model.version, slices.map((slice) => slice.id));
      if (generation === selectionGeneration) selectedText = content;
    } catch (reason) { if (generation === selectionGeneration) errorMessage = String(reason); }
  }

  function setGranularity(next: PickerGranularity) {
    if (granularity === next) return;
    granularity = next;
    selectedIds = new Set();
    lastSelectedIndex = null;
    focusedIndex = 0;
  }

  function replaceSelection(next: Set<string>) {
    selectedIds = next;
  }

  function toggleSlice(event: MouseEvent, slice: TextSlice, index: number) {
    if (busy || event.detail > 1) return;
    const next = new Set(selectedIds);
    if (event.shiftKey && lastSelectedIndex !== null) {
      const start = Math.min(lastSelectedIndex, index);
      const end = Math.max(lastSelectedIndex, index);
      for (let current = start; current <= end; current += 1) next.add(slices[current].id);
    } else if (next.has(slice.id)) {
      next.delete(slice.id);
    } else {
      next.add(slice.id);
    }
    focusedIndex = index;
    lastSelectedIndex = index;
    replaceSelection(next);
  }

  function insertSlice(event: MouseEvent, slice: TextSlice) {
    event.preventDefault();
    event.stopPropagation();
    if (!busy) void onPaste(slice.text);
  }

  function selectAll() {
    replaceSelection(new Set(slices.map((slice) => slice.id)));
    lastSelectedIndex = slices.length ? slices.length - 1 : null;
  }

  function clearSelection() {
    replaceSelection(new Set());
    lastSelectedIndex = null;
  }

  function focusSlice(index: number) {
    const buttons = pickerRoot?.querySelectorAll<HTMLButtonElement>(".segment-inline-unit");
    if (!buttons?.length) return;
    focusedIndex = Math.max(0, Math.min(index, buttons.length - 1));
    buttons[focusedIndex]?.focus({ preventScroll: true });
    buttons[focusedIndex]?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }

  function handleSliceKeyDown(event: KeyboardEvent, slice: TextSlice, index: number) {
    if (event.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      if (!busy) void onPaste(slice.text);
    } else if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      event.preventDefault();
      focusSlice(index + 1);
    } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      event.preventDefault();
      focusSlice(index - 1);
    } else if (event.key === "Home") {
      event.preventDefault();
      focusSlice(0);
    } else if (event.key === "End") {
      event.preventDefault();
      focusSlice(slices.length - 1);
    }
  }

  function handleKeyDown(event: KeyboardEvent) {
    if (event.isComposing) return;
    const primaryModifier = event.metaKey || event.ctrlKey;
    if (primaryModifier && event.code === "KeyA") {
      event.preventDefault();
      event.stopPropagation();
      selectAll();
    } else if (primaryModifier && event.code === "KeyC") {
      event.preventDefault();
      event.stopPropagation();
      if (!busy && selectedText) void onCopy(selectedText);
    }
  }

  function keyboardScope(node: HTMLElement) {
    node.addEventListener("keydown", handleKeyDown);
    return { destroy: () => node.removeEventListener("keydown", handleKeyDown) };
  }
</script>

<div class="segment-picker" bind:this={pickerRoot} use:keyboardScope>
  <div class="segment-picker-main">
    <div class="segment-inline-toolbar">
      <span class="segment-inline-label">{uiTranslate("选择单位", $uiLanguage)}</span>
      <div class="segment-granularity" role="group" aria-label={uiTranslate("选择单位", $uiLanguage)}>
        <button
          class:active={granularity === "word"}
          type="button"
          aria-pressed={granularity === "word"}
          on:click={() => setGranularity("word")}
        >{uiTranslate("词", $uiLanguage)}</button>
        <button
          class:active={granularity === "phrase"}
          type="button"
          aria-pressed={granularity === "phrase"}
          on:click={() => setGranularity("phrase")}
        >{uiTranslate("段", $uiLanguage)}</button>
      </div>
      <span class="segment-character-count">{text.length} {uiTranslate("字符", $uiLanguage)}</span>
    </div>

    <p class="segment-inline-hint" id="segment-inline-instructions">
      {uiTranslate("悬浮预览边界 · 双击直接插入 · 单击加入选择", $uiLanguage)}
    </p>

    <div
      class="segment-reading-canvas"
      role="group"
      aria-label={uiTranslate("可选择的剪贴板正文", $uiLanguage)}
      aria-describedby="segment-inline-instructions"
      dir="auto"
    >
      {#each renderParts as part}
        {#if part.kind === "text"}
          <span>{part.text}</span>
        {:else}
          {@const index = part.index}
          {@const order = selectedOrderById.get(part.slice.id) ?? 0}
          <button
            class:selected={order > 0}
            class="segment-inline-unit"
            type="button"
            disabled={busy}
            aria-pressed={order > 0}
            aria-label={`${part.slice.text}${order > 0 ? (t("，已选第 {count} 项", language, { count: order })) : ""}`}
            data-hint={t("双击插入此{unit}", language, { unit: unitLabel })}
            tabindex={focusedIndex === index ? 0 : -1}
            on:focus={() => (focusedIndex = index)}
            on:click={(event) => toggleSlice(event, part.slice, index)}
            on:dblclick={(event) => insertSlice(event, part.slice)}
            on:keydown={(event) => handleSliceKeyDown(event, part.slice, index)}
          >
            {#if order > 0}<span class="segment-selection-index" aria-hidden="true">{order}</span>{/if}
            <span>{part.slice.text}</span>
          </button>
        {/if}
      {/each}
    </div>

    {#if model.truncated}
      <p class="segment-limit-note">
        {uiTranslate("文本较长：详细边界已限量，以保证预览操作流畅。", $uiLanguage)}
      </p>
    {/if}

    {#if sourceWarning}
      <p class="segment-source-warning" role="status">{sourceWarning}</p>
    {/if}

    {#if errorMessage}
      <p class="segment-error" role="alert">{errorMessage}</p>
    {/if}
  </div>

  <div class="segment-inline-actions">
    <div class="segment-selection-status" aria-live="polite">
      {#if selectedSlices.length}
        <strong>{t("已选 {count} {unit}", language, { count: selectedSlices.length, unit: unitLabel })}</strong>
        <span>{uiTranslate("按原文顺序", $uiLanguage)}</span>
      {:else}
        <strong>{t("未选择{unit}", language, { unit: unitLabel })}</strong>
        <span>{uiTranslate("双击可直接插入", $uiLanguage)}</span>
      {/if}
    </div>
    <button class="dialog-button" type="button" disabled={busy || !selectedSlices.length} on:click={clearSelection}>
      {uiTranslate("清空", $uiLanguage)}
    </button>
    <button class="dialog-button primary" type="button" disabled={busy || !selectedText} on:click={() => onPaste(selectedText)}>
      {busy
        ? (uiTranslate("正在插入…", $uiLanguage))
        : (t("插入所选 {count} {unit}", language, { count: selectedSlices.length, unit: unitLabel }))}
    </button>
    <button class="dialog-button" type="button" disabled={busy} on:click={onPasteAll}>
      {uiTranslate("插入全文", $uiLanguage)}
    </button>
  </div>
</div>
