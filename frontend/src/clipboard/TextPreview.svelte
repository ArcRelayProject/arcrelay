<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { translate as tr } from "../i18n";
  import type { LanguagePreference } from "../types";
  import type { ClipboardTextPreview, ClipboardTextFormat } from "../ipc/generated";
  import type { ClipboardItem } from "./types";
  import AppSelect from "../components/AppSelect.svelte";
  import { clipboardBridge } from "./bridge";
  import DragHandle from "./DragHandle.svelte";
  import type { ClipboardDragMode } from "./dragSession";

  export let model: ClipboardTextPreview;
  export let item: ClipboardItem;
  export let language: LanguagePreference;
  export let busy = false;
  export let errorMessage = "";
  export let onCopy: (content: string) => Promise<void> | void;
  export let onPaste: (content: string) => Promise<void> | void;
  export let onCopyAll: () => Promise<void> | void;
  export let onPasteAll: () => Promise<void> | void;
  export let onDragAll: (event: PointerEvent, mode: ClipboardDragMode) => void = () => {};
  export let onDragSelection: (
    event: PointerEvent,
    text: string,
    mode: ClipboardDragMode,
    nativeSelection: boolean,
  ) => void = () => {};
  export let onNativeDragStart: () => void = () => {};
  let textDragMode: ClipboardDragMode = "auto";

  let sourceMode = false;
  let selectedText = "";
  let root: HTMLElement;
  let contentRoot: HTMLElement;
  let loading = false;
  let localError = "";
  let request = 0;
  let actionBusy = false;
  $: disabled = busy || loading || actionBusy;
  $: showSource = sourceMode || model.renderLimited;
  onDestroy(() => {
    request++;
  });

  function clearSelection() {
    selectedText = "";
    const selection = window.getSelection();
    if (selection?.anchorNode && root?.contains(selection.anchorNode)) selection.removeAllRanges();
  }

  function captureSelection() {
    const selection = window.getSelection();
    if (
      selection?.anchorNode &&
      selection.focusNode &&
      contentRoot?.contains(selection.anchorNode) &&
      contentRoot.contains(selection.focusNode)
    ) {
      selectedText = selection.isCollapsed ? "" : selection.toString();
    } else if (!selection?.isCollapsed || !root?.contains(document.activeElement)) {
      selectedText = "";
    }
  }

  function switchView(source: boolean) {
    clearSelection();
    sourceMode = source;
  }

  async function setFormat(format: ClipboardTextFormat) {
    clearSelection();
    const generation = ++request;
    loading = true;
    localError = "";
    try {
      const next = await clipboardBridge.textPreview(item, format);
      if (generation === request) model = next;
    } catch (error) {
      if (generation === request) localError = String(error);
    } finally {
      if (generation === request) loading = false;
    }
  }

  async function act(paste: boolean) {
    if (disabled) return;
    const selection = selectedText;
    actionBusy = true;
    localError = "";
    try {
      if (selection) await (paste ? onPaste(selection) : onCopy(selection));
      else if (showSource) await (paste ? onPaste(model.source) : onCopy(model.source));
      else await (paste ? onPasteAll() : onCopyAll());
    } catch (error) {
      localError = String(error);
    } finally {
      actionBusy = false;
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.isComposing || !(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey)
      return;
    if (event.code === "KeyA" && contentRoot?.contains(event.target as Node)) {
      event.preventDefault();
      event.stopPropagation();
      const range = document.createRange();
      range.selectNodeContents(contentRoot);
      const selection = window.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(range);
      captureSelection();
    }
  }

  function prepareSelectionDrag(event: PointerEvent) {
    captureSelection();
    if (selectedText) onDragSelection(event, selectedText, "plain_text", true);
  }

  function startSelectionDrag(event: DragEvent) {
    event.preventDefault();
    event.stopPropagation();
    if (selectedText) onNativeDragStart();
  }

  function dragFromHandle(event: PointerEvent, mode: ClipboardDragMode) {
    if (selectedText)
      onDragSelection(event, selectedText, mode === "text_file" ? mode : "plain_text", false);
    else onDragAll(event, mode);
  }

  onMount(() => {
    document.addEventListener("selectionchange", captureSelection);
    window.addEventListener("keydown", handleKeydown);
    return () => {
      document.removeEventListener("selectionchange", captureSelection);
      window.removeEventListener("keydown", handleKeydown);
    };
  });
</script>

<div class="text-selection-preview" bind:this={root}>
  <div class="text-preview-toolbar">
    <div role="group" aria-label={tr("显示方式", language)}>
      <button type="button" aria-pressed={!sourceMode} {disabled} on:click={() => switchView(false)}
        >{tr("预览", language)}</button
      >
      <button type="button" aria-pressed={sourceMode} {disabled} on:click={() => switchView(true)}
        >{tr("源文本", language)}</button
      >
    </div>
    <AppSelect
      aria-label={tr("内容格式", language)}
      value={model.format}
      {disabled}
      options={[
        { value: "text", label: tr("普通文本", language) },
        { value: "html", label: "HTML" },
        { value: "markdown", label: "Markdown" },
      ]}
      onValueChange={(format) => setFormat(format)}
    />
  </div>
  {#if model.renderLimited}
    <p class="text-preview-notice" role="status">
      {tr("内容较长，已显示完整源文本。插入全部仍使用完整内容。", language)}
    </p>
  {/if}
  {#if errorMessage || localError}<p class="text-preview-notice" role="alert">
      {errorMessage || localError}
    </p>{/if}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="text-preview-content"
    class:loading
    bind:this={contentRoot}
    tabindex="0"
    role="document"
    on:pointerdown={prepareSelectionDrag}
    on:dragstart={startSelectionDrag}
    aria-label={tr("拖动选择内容，双击选择词汇", language)}
    aria-busy={loading}
  >
    {#if !showSource && model.safeHtml !== null}
      <div class="rendered-text">{@html model.safeHtml}</div>
    {:else}
      <pre>{model.source}</pre>
    {/if}
  </div>
  <div class="dialog-actions text-preview-actions">
    <span class="selection-hint" aria-live="polite"
      >{tr(selectedText ? "已选择部分内容" : "拖动选择内容，双击选择词汇", language)}</span
    >
    <AppSelect
      class="text-drag-format"
      aria-label={tr("拖出格式", language)}
      bind:value={textDragMode}
      {disabled}
      options={[
        { value: "auto", label: tr("自动格式", language) },
        { value: "plain_text", label: tr("纯文本", language) },
        ...(item.kind === "html" ? [{ value: "rich_text", label: tr("富文本", language) }] : []),
        { value: "text_file", label: "TXT" },
      ]}
    />
    <DragHandle
      label={tr(selectedText ? "拖出选择" : "拖出", language)}
      disabled={disabled || !item.available}
      onPress={(event) => dragFromHandle(event, textDragMode)}
    />
    <button
      class="dialog-button"
      type="button"
      disabled={disabled || !model.source}
      on:mousedown|preventDefault
      on:click={() => act(false)}>{tr(selectedText ? "复制选择" : "复制全部", language)}</button
    >
    <button
      class="dialog-button primary"
      type="button"
      disabled={disabled || !model.source}
      on:mousedown|preventDefault
      on:click={() => act(true)}>{tr(selectedText ? "插入选择" : "插入全部", language)}</button
    >
  </div>
</div>

<style>
  .text-selection-preview {
    display: flex;
    flex: 1;
    min-width: 0;
    min-height: 0;
    flex-direction: column;
    background: var(--surface-raised);
  }
  .text-preview-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 20px;
    border-bottom: 1px solid var(--border-color);
  }
  .text-preview-toolbar button {
    padding: 6px 10px;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    background: var(--surface-raised);
    color: var(--text-primary);
    font: inherit;
    font-size: 12px;
  }
  .text-preview-toolbar button[aria-pressed="true"] {
    background: var(--surface-hover);
    font-weight: 650;
  }
  .text-preview-toolbar [role="group"] {
    display: flex;
    gap: 5px;
  }
  .text-preview-content {
    min-height: 140px;
    flex: 1;
    overflow: auto;
    padding: 18px 20px;
    color: var(--text-primary);
    font-size: 14px;
    line-height: 1.7;
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
  }
  .text-preview-content :global(*) {
    user-select: text;
    -webkit-user-select: text;
  }
  .text-preview-content pre {
    white-space: pre-wrap;
    margin: 0;
    tab-size: 4;
    font:
      13px/1.7 ui-monospace,
      monospace;
  }
  .text-preview-content.loading {
    opacity: 0.5;
    pointer-events: none;
  }
  .text-preview-notice {
    margin: 0;
    padding: 10px 20px;
    color: var(--text-secondary);
    font-size: 12px;
  }
  .text-preview-actions {
    align-items: center;
    flex-wrap: wrap;
  }
  .text-preview-actions :global(.text-drag-format) {
    min-height: 30px;
    min-width: 76px;
    max-width: 110px;
    padding: 5px;
    background: var(--surface-raised);
    color: var(--text-secondary);
    border: 1px solid var(--border-color);
    border-radius: 6px;
    font: inherit;
    font-size: 12px;
  }
  .selection-hint {
    margin-right: auto;
    color: var(--text-secondary);
    font-size: 11px;
  }
  .rendered-text :global(:first-child) {
    margin-top: 0;
  }
  .rendered-text :global(:last-child) {
    margin-bottom: 0;
  }
  .rendered-text :global(pre) {
    overflow-x: auto;
    padding: 12px;
    background: var(--surface-sunken);
    border-radius: 8px;
    tab-size: 4;
  }
  .rendered-text :global(code) {
    font-family: ui-monospace, monospace;
    font-size: 13px;
  }
  .rendered-text :global(blockquote) {
    margin-left: 0;
    padding-left: 14px;
    border-left: 3px solid var(--border-color);
    color: var(--text-secondary);
  }
  .rendered-text :global(table) {
    border-collapse: collapse;
    max-width: 100%;
  }
  .rendered-text :global(th),
  .rendered-text :global(td) {
    border: 1px solid var(--border-color);
    padding: 6px 10px;
  }
</style>
