<script lang="ts">
  import { onMount } from "svelte";
  import { ClockCounterClockwise } from "phosphor-svelte";
  import AppSelect from "../../components/AppSelect.svelte";
  import { bridge } from "../../bridge";
  import { translate, t, language } from "../../i18n";

  let days = 30;
  let savedDays = 30;
  let loaded = false;
  let busy = false;
  let error = "";
  let saved = false;
  $: choices = [...new Set([7, 30, 90, 0, savedDays])].map((value) => ({
    value,
    label:
      value === 0
        ? translate("永久", $language)
        : t("{part0} 天未使用", $language, { part0: value }),
  }));

  async function load() {
    busy = true;
    error = "";
    try {
      days = savedDays = await bridge.getClipboardRetentionDays();
      loaded = true;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
  async function save(value: number) {
    if (!loaded || busy) return;
    busy = true;
    error = "";
    saved = false;
    try {
      days = savedDays = await bridge.setClipboardRetentionDays(value);
      saved = true;
    } catch (e) {
      days = savedDays;
      error = String(e);
    } finally {
      busy = false;
    }
  }
  onMount(() => {
    void load();
  });
</script>

<div class="settings-row setting-field-row">
  <span class="row-icon"><ClockCounterClockwise size={21} /></span>
  <span class="row-copy">
    <strong>{translate("未使用保留时间", $language)}</strong>
    <small
      >{translate(
        "从最后一次复制或使用起计算；收藏和带标签的内容不会自动清理。记录条数和总容量不限。",
        $language,
      )}</small
    >
    {#if error}<small role="alert">{error}</small>{/if}
    {#if saved}<small role="status">{translate("设置已保存", $language)}</small>{/if}
  </span>
  {#if !loaded && error}
    <button disabled={busy} on:click={load}>{translate("重试", $language)}</button>
  {:else}
    <AppSelect
      bind:value={days}
      options={choices}
      disabled={!loaded || busy}
      aria-label={translate("未使用保留时间", $language)}
      onValueChange={(value) => void save(Number(value))}
    />
  {/if}
</div>

<style>
  .row-copy small {
    white-space: normal;
    overflow: visible;
    text-overflow: clip;
    line-height: 1.5;
  }
</style>
