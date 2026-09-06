<script lang="ts">
  import AppSelect from "../../../components/AppSelect.svelte";
  import { translate as uiTranslate, language as uiLanguage } from "../../../i18n";
  import { CaretRight, DesktopTower, Monitor, Ruler, MinusCircle } from "phosphor-svelte";
  import type { WorkspaceLayout } from "../../../types";

  export let layout: WorkspaceLayout;
  export let displayId: string | null;
  export let deviceName = "这台 Mac";
  export let online = true;
  export let statusLabel = "在线";
  export let onRemove: (id: string) => void;
  export let onMove: (id: string, x: number, y: number) => void;
  export let onChange: () => void;

  let showAdvanced = false;
  $: display = displayId ? layout.displays[displayId] : null;
  $: diagonal = display ? Math.hypot(display.physicalSizeUm.width, display.physicalSizeUm.height) / 25_400 : 0;

  function setDiagonal(inches: number) {
    if (!display || inches <= 0) return;
    const aspect = display.pixelSize.width / display.pixelSize.height;
    const diagonalMm = inches * 25.4;
    const heightMm = diagonalMm / Math.sqrt(aspect * aspect + 1);
    display.physicalSizeUm.width = Math.round(heightMm * aspect * 1000);
    display.physicalSizeUm.height = Math.round(heightMm * 1000);
    display.deskRectUm.width = display.physicalSizeUm.width;
    display.deskRectUm.height = display.physicalSizeUm.height;
    display.geometryConfidence = "UserCalibrated";
    onChange();
  }
</script>

<aside class="inspector card">
  {#if display}
    <div class="inspector-title"><span class="display-icon"><Monitor size={24} weight="duotone" /></span><div><h3>{display.name}</h3><p><DesktopTower size={14} />{deviceName}</p></div><em class:offline={!online}>{uiTranslate(statusLabel, $uiLanguage)}</em></div>

    <section>
      <button class="advanced-row" on:click={() => onRemove(display.displayId)}><span>{uiTranslate("从布局移除屏幕", $uiLanguage)}</span><MinusCircle size={17} /></button>
      <small>{uiTranslate("仅移出跨屏布局，不解除配对。保存后生效，重连不会自动加回。", $uiLanguage)}</small>
    </section>

    <section>
      <h4>{uiTranslate("位置", $uiLanguage)}</h4>
      <div class="position-grid"><label>X<input type="number" value={display.deskRectUm.x} on:change={(event) => onMove(display.displayId, Number(event.currentTarget.value), display.deskRectUm.y)} /></label><label>Y<input type="number" value={display.deskRectUm.y} on:change={(event) => onMove(display.displayId, display.deskRectUm.x, Number(event.currentTarget.value))} /></label></div>
      <small>{uiTranslate("同一设备上的屏幕保持系统排列，移动任一屏幕会整体移动该设备。", $uiLanguage)}</small>
    </section>

    <section>
      <h4>{uiTranslate("旋转", $uiLanguage)}</h4>
      <div class="segmented">
        {#each [["Degrees0","0°"],["Degrees90","90°"],["Degrees180","180°"],["Degrees270","270°"]] as [value,label]}
          <button disabled class:active={display.rotation === value}>{uiTranslate(label, $uiLanguage)}</button>
        {/each}
      </div>
      <small>{uiTranslate("旋转和缩放由系统显示设置管理。", $uiLanguage)}</small>
    </section>

    <section><label class="select-field"><span>{uiTranslate("缩放", $uiLanguage)}</span><AppSelect bind:value={display.scaleFactor} disabled aria-label={uiTranslate("缩放", $uiLanguage)}
      options={[
        { value: 1, label: "100%" },
        { value: 1.25, label: "125%" },
        { value: 1.5, label: "150%" },
        { value: 2, label: "200%" },
      ]}
    /></label></section>

    <section>
      <div class="calibration-row"><div><h4>{uiTranslate("实际尺寸", $uiLanguage)}</h4><p><Ruler size={15} />{diagonal.toFixed(1)} {uiTranslate("英寸", $uiLanguage)}</p></div><button on:click={() => setDiagonal(Number(prompt("显示器对角线（英寸）", diagonal.toFixed(1)) ?? diagonal))}>{uiTranslate("重新校准", $uiLanguage)}</button></div>
    </section>

    <button class="advanced-row" on:click={() => showAdvanced = !showAdvanced}>{uiTranslate("显示高级信息", $uiLanguage)}<CaretRight class={showAdvanced ? "expanded" : ""} size={17} /></button>
    {#if showAdvanced}
      <dl class="advanced"><dt>{uiTranslate("像素尺寸", $uiLanguage)}</dt><dd>{display.pixelSize.width} × {display.pixelSize.height}</dd><dt>{uiTranslate("尺寸来源", $uiLanguage)}</dt><dd>{display.geometryConfidence}</dd><dt>{uiTranslate("显示器标识", $uiLanguage)}</dt><dd class="mono">{display.displayId}</dd><dt>{uiTranslate("库存版本", $uiLanguage)}</dt><dd>{display.inventoryRevision}</dd></dl>
    {/if}

  {:else}
    <div class="empty"><span><Monitor size={30} weight="duotone" /></span><h3>{uiTranslate("选择一块屏幕", $uiLanguage)}</h3><p>{uiTranslate("查看位置、旋转和物理尺寸；将它拖近另一块屏幕后会自动建立跨屏通道。", $uiLanguage)}</p></div>
  {/if}
</aside>

<style>
  .inspector { width: 322px; padding: 22px; display: grid; align-content: start; gap: 18px; overflow: auto; }
  .inspector-title { display: grid; grid-template-columns: 44px minmax(0,1fr) auto; align-items: center; gap: 11px; }.display-icon { display: grid; width: 42px; height: 42px; place-items: center; border-radius: 11px; color: var(--accent); background: var(--accent-soft); }.inspector-title h3 { margin: 0 0 5px; font-size: 17px; }.inspector-title p { display: flex; align-items: center; gap: 5px; margin: 0; color: var(--text-secondary); font-size: 11px; }.inspector-title em { padding: 4px 8px; border-radius: 999px; color: var(--success); background: var(--success-soft); font-size: 10px; font-style: normal; font-weight: 700; }.inspector-title em.offline { color: var(--text-muted); background: var(--surface-sunken); }
  section { display: grid; gap: 9px; }h4 { margin: 0; font-size: 12px; }.position-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 9px; }.position-grid label { display: flex; min-height: 40px; align-items: center; gap: 8px; padding: 0 10px; border: 1px solid var(--border); border-radius: 9px; color: var(--text-muted); font-size: 11px; }.position-grid input { min-width: 0; width: 100%; border: 0; color: var(--text); background: transparent; outline: none; }
  .segmented { display: grid; grid-template-columns: repeat(4,1fr); overflow: hidden; border: 1px solid var(--border); border-radius: 9px; }.segmented button { min-height: 37px; border: 0; border-right: 1px solid var(--border); color: var(--text-secondary); background: var(--surface); }.segmented button:last-child { border-right: 0; }.segmented button.active { color: var(--accent-strong); background: var(--accent-soft); box-shadow: inset 0 0 0 1px var(--accent); }
  .select-field { display: grid; gap: 8px; color: var(--text); font-size: 12px; font-weight: 650; }.select-field :global(.app-select-trigger) { min-height: 40px; border: 1px solid var(--border); border-radius: 9px; padding: 0 10px; color: var(--text); background: var(--surface); }
  .calibration-row { display: flex; align-items: flex-end; justify-content: space-between; gap: 12px; }.calibration-row p { display: flex; align-items: center; gap: 6px; margin: 8px 0 0; color: var(--text-secondary); font-size: 12px; }.calibration-row button { border: 0; color: var(--accent-strong); background: transparent; font-size: 11px; font-weight: 650; }
  .advanced-row { display: flex; min-height: 44px; align-items: center; justify-content: space-between; border: 1px solid var(--border); border-radius: 10px; padding: 0 12px; color: var(--text-secondary); background: var(--surface-soft); text-align: left; }.advanced-row :global(svg) { transition: transform 120ms ease; }.advanced-row :global(svg.expanded) { transform: rotate(90deg); }.advanced { display: grid; grid-template-columns: 100px minmax(0,1fr); gap: 9px; margin: -8px 0 0; padding: 12px; border-radius: 10px; background: var(--surface-soft); font-size: 10px; }.advanced dt { color: var(--text-muted); }.advanced dd { margin: 0; overflow-wrap: anywhere; }
  .empty { display: grid; place-content: center; justify-items: center; min-height: 330px; gap: 10px; text-align: center; }.empty > span { display: grid; width: 56px; height: 56px; place-items: center; border-radius: 15px; color: var(--accent); background: var(--accent-soft); }.empty h3 { margin: 0; }.empty p { max-width: 230px; margin: 0; color: var(--text-secondary); font-size: 12px; line-height: 1.55; }
</style>
