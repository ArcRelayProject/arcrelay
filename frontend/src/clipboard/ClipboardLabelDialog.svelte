<script lang="ts">
  import { t } from "../localization";
  import { translate as uiTranslate, language as uiLanguage } from "../i18n";
  import { Dialog } from "bits-ui";
  import { Check, Plus, Tag, Trash } from "phosphor-svelte";

  import type { LanguagePreference } from "../types";
  import { tr } from "./i18n";
  import type { ClipboardLabel } from "./types";

  export let open = false;
  export let labels: ClipboardLabel[] = [];
  export let selectedLabelIds: string[] = [];
  export let newLabelName = "";
  export let newLabelColor = "#5B5FF0";
  export let newLabelInput: HTMLInputElement;
  export let language: LanguagePreference;
  export let manageOnly = false;
  export let onSave: () => void | Promise<void>;
  export let onCreate: () => void | Promise<void>;
  export let onDelete: (labelId: string) => void | Promise<void>;
  export let onToggle: (labelId: string) => void;
  export let onNewLabelKeyDown: (event: KeyboardEvent) => void;
</script>

<Dialog.Root bind:open>
  <Dialog.Portal>
    <Dialog.Overlay class="dialog-overlay" />
    <Dialog.Content class="dialog-content label-dialog-content">
      <div class="label-dialog-header">
        <span class="label-dialog-icon"><Tag size={18} weight="fill" /></span>
        <div>
          <Dialog.Title class="dialog-title">{manageOnly ? uiTranslate("管理标签", $uiLanguage) : tr("添加到标签", language)}</Dialog.Title>
          <Dialog.Description class="label-dialog-description">
            {manageOnly
              ? uiTranslate("创建或删除用于整理剪贴板记录的标签。", $uiLanguage)
              : uiTranslate("选择已有标签，或在这里直接新建。", $uiLanguage)}
          </Dialog.Description>
        </div>
      </div>
      <div class="label-section-heading">
        <span>{uiTranslate("已有标签", $uiLanguage)}</span>
        {#if labels.length}<span>{manageOnly ? labels.length : `${selectedLabelIds.length} / ${labels.length}`}</span>{/if}
      </div>
      <div class="label-list">
        {#each labels as label (label.id)}
          <div class:selected={!manageOnly && selectedLabelIds.includes(label.id)} class:manage-only={manageOnly} class="label-option">
            {#if manageOnly}
              <div class="label-toggle">
                <span class="label-dot" style:--label-color={label.color}></span>
                <span class="label-name">{label.name}</span>
              </div>
            {:else}
              <label class="label-toggle">
                <input class="label-checkbox" type="checkbox" checked={selectedLabelIds.includes(label.id)} on:change={() => onToggle(label.id)} />
                <span class="label-dot" style:--label-color={label.color}></span>
                <span class="label-name">{label.name}</span>
              </label>
            {/if}
            <button
              class="label-delete"
              type="button"
              aria-label={t("删除标签 {name}", language, { name: label.name })}
              title={uiTranslate("删除标签", $uiLanguage)}
              on:click={() => onDelete(label.id)}
            ><Trash size={14} /></button>
          </div>
        {:else}
          <div class="label-empty-state">
            <Tag size={22} />
            <strong>{uiTranslate("还没有标签", $uiLanguage)}</strong>
            <span>{uiTranslate("在下方创建第一个标签。", $uiLanguage)}</span>
          </div>
        {/each}
      </div>
      <div class="label-create-panel">
        <label class="label-create-title" for="new-label-name">{uiTranslate("新建标签", $uiLanguage)}</label>
        <div class="label-create-row">
          <div class="label-name-input">
            <Tag size={15} />
            <input
              id="new-label-name"
              bind:this={newLabelInput}
              bind:value={newLabelName}
              placeholder={tr("标签名称", language)}
              maxlength="32"
              on:keydown={onNewLabelKeyDown}
            />
          </div>
          <input
            class="label-color-input"
            bind:value={newLabelColor}
            type="color"
            aria-label={uiTranslate("标签颜色", $uiLanguage)}
            title={uiTranslate("选择颜色", $uiLanguage)}
          />
          <button class="label-create-button" type="button" disabled={!newLabelName.trim()} on:click={onCreate}>
            <Plus size={15} weight="bold" /> {uiTranslate("添加", $uiLanguage)}
          </button>
        </div>
      </div>
      <div class="dialog-actions label-dialog-actions">
        {#if manageOnly}
          <button class="dialog-button primary" type="button" on:click={onSave}><Check size={16} weight="bold" /> {uiTranslate("完成", $uiLanguage)}</button>
        {:else}
          <Dialog.Close class="dialog-button">{tr("取消", language)}</Dialog.Close>
          <button class="dialog-button primary" type="button" on:click={onSave}><Check size={16} weight="bold" /> {tr("保存", language)}</button>
        {/if}
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
