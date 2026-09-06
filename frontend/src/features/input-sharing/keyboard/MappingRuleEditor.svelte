<script lang="ts">
  import AppSelect from "../../../components/AppSelect.svelte";
  import { translate as uiTranslate, language as uiLanguage } from "../../../i18n";
  import type { KeyboardMappingRule } from "../../../types";
  export let source = "Windows Ctrl+C";
  export let target = "macOS Command+C";
  export let description = "复制";
  export let rule: KeyboardMappingRule | null = null;
  export let onChange: () => void = () => {};
  export let onDelete: (() => void) | null = null;

  const actions: KeyboardMappingRule["action"][] = ["SelectAll", "Copy", "Paste", "Cut", "Undo", "Redo", "ApplicationSwitch"];
</script>

<div class="mapping">
  {#if rule}
    <div class="custom-row"><label>{uiTranslate("修饰键", $uiLanguage)}<AppSelect multiple bind:value={rule.source.modifiers} onValueChange={onChange} aria-label={uiTranslate("修饰键", $uiLanguage)}
      options={[
        { value: 224, label: "Ctrl" },
        { value: 225, label: "Shift" },
        { value: 226, label: "Alt/Option" },
        { value: 227, label: "Meta/Command" },
      ]}
    /></label><label>{uiTranslate("主键 HID", $uiLanguage)}<input type="number" min="4" max="231" bind:value={rule.source.key} on:change={onChange} /></label></div>
    <div class="custom-row"><label>{uiTranslate("语义", $uiLanguage)}<AppSelect bind:value={rule.action} onValueChange={onChange} aria-label={uiTranslate("语义", $uiLanguage)}
      options={[
        ...actions.map((action) => ({ value: action, label: action })),
      ]}
    /></label>{#if onDelete}<button class="delete" on:click={onDelete}>{uiTranslate("删除", $uiLanguage)}</button>{/if}</div>
  {:else}
    <div><span class="label">{uiTranslate("来源", $uiLanguage)}</span><strong>{source}</strong></div>
    <div class="arrow">↓</div>
    <div><span class="label">{uiTranslate("目标", $uiLanguage)}</span><strong>{target}</strong></div>
    <span class="semantic">{uiTranslate(description, $uiLanguage)}</span>
  {/if}
</div>

<style>
  .mapping { position: relative; display: grid; gap: 8px; padding: 15px; border: 1px solid #e2e7f0; border-radius: 12px; background: #fafbfe; }
  .mapping > div:not(.arrow) { display: flex; justify-content: space-between; gap: 20px; font-size: 13px; }
  .arrow { position: absolute; left: 50%; top: 38px; color: #635bdf; font-weight: 800; }
  .label { color: #667085; }
  .semantic { position: absolute; right: 12px; top: 42px; padding: 2px 7px; border-radius: 999px; color: #5148c6; background: #eeecff; font-size: 10px; }
  .custom-row { display: grid !important; grid-template-columns: 1fr 1fr; align-items: end; gap: 9px !important; }
  .custom-row label { display: grid; gap: 5px; color: #667085; font-size: 11px; }
  .custom-row input, .custom-row :global(.app-select-trigger) { min-width: 0; padding: 7px; border: 1px solid #d7dce5; border-radius: 8px; background: inherit; color: inherit; }
  .delete { padding: 8px; border: 0; border-radius: 8px; color: #b4232f; background: #feecee; }
  @media (prefers-color-scheme: dark) { .mapping { border-color: #354053; background: #171e2a; } }
</style>
