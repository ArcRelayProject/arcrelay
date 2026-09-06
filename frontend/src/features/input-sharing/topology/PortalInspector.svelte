<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../../../i18n";
  import { ArrowsLeftRight, ArrowRight, CaretRight, Clock, Command, Monitor, TestTube } from "phosphor-svelte";
  import type { WorkspaceLayout } from "../../../types";

  export let layout: WorkspaceLayout;
  export let portalId: string | null;
  export let onChange: () => void;
  export let onFinish: () => void = () => {};
  export let onTest: () => void = () => {};

  let showAdvanced = false;
  $: portal = layout.portals.find((value) => value.portalId === portalId);
  $: source = portal ? layout.displays[portal.sourceDisplay] : null;
  $: target = portal ? layout.displays[portal.targetDisplay] : null;
  $: activationKind = !portal || portal.activationPolicy === "Immediate" ? "Immediate" : "RequireModifier" in portal.activationPolicy ? "RequireModifier" : "Dwell";
  $: segmentLengthMm = portal ? Math.max(0, portal.sourceSegment.endUm - portal.sourceSegment.startUm) / 1000 : 0;

  function setActivation(kind: "Immediate" | "RequireModifier" | "Dwell") {
    if (!portal) return;
    portal.activationPolicy = kind === "RequireModifier" ? { RequireModifier: { hid_usage: 0xe1 } } : kind === "Dwell" ? { Dwell: { milliseconds: 350 } } : "Immediate";
    onChange();
  }

</script>

<aside class="inspector card">
  {#if portal}
    <div class="title-row"><div><span class="eyebrow">{uiTranslate("自动跨屏通道", $uiLanguage)}</span><h3>{uiTranslate(source?.name ?? "来源屏幕", $uiLanguage)} ↔ {uiTranslate(target?.name ?? "目标屏幕", $uiLanguage)}</h3></div><span class:warning={portal.status !== "Active"} class="status">{uiTranslate(portal.status === "Active" ? "可用" : "已暂停", $uiLanguage)}</span></div>

    <div class="route-preview"><span><Monitor size={21} /><small>{source?.name}</small></span><ArrowRight size={18} /><span><Monitor size={21} /><small>{target?.name}</small></span></div>

    <section class="automatic-range"><div><h4>{uiTranslate("自动穿越范围", $uiLanguage)}</h4><strong>{segmentLengthMm.toFixed(0)} mm</strong></div><p>{uiTranslate("由两块屏幕相邻边的实际重叠部分决定；移动或校准屏幕后会自动更新。", $uiLanguage)}</p></section>

    <section>
      <h4>{uiTranslate("触发方式", $uiLanguage)}</h4>
      <div class="activation-grid">
        <button class:active={activationKind === "Immediate"} on:click={() => setActivation("Immediate")}><ArrowRight size={19} weight="bold" /><strong>{uiTranslate("直接穿越", $uiLanguage)}</strong><small>{uiTranslate("移动到边缘立即进入", $uiLanguage)}</small></button>
        <button class:active={activationKind === "Dwell"} on:click={() => setActivation("Dwell")}><Clock size={19} /><strong>{uiTranslate("边缘停留", $uiLanguage)}</strong><small>{uiTranslate("停留片刻后进入", $uiLanguage)}</small></button>
        <button class:active={activationKind === "RequireModifier"} on:click={() => setActivation("RequireModifier")}><Command size={19} /><strong>{uiTranslate("按住修饰键", $uiLanguage)}</strong><small>{uiTranslate("按键时跨越边缘", $uiLanguage)}</small></button>
      </div>
    </section>

    <label class="toggle-row"><span><strong>{uiTranslate("拖动文件时允许穿越", $uiLanguage)}</strong><small>{uiTranslate("按住鼠标按钮时也可以进入目标屏幕", $uiLanguage)}</small></span><input type="checkbox" bind:checked={portal.allowWhileDragging} on:change={onChange} /><i></i></label>

    <button class="advanced-row" on:click={() => showAdvanced = !showAdvanced}>{uiTranslate("手感微调", $uiLanguage)}<CaretRight class={showAdvanced ? "expanded" : ""} size={17} /></button>
    {#if showAdvanced}
      <div class="advanced-grid">
        <label>{uiTranslate("进入内缩", $uiLanguage)}<input type="number" min="0" value={portal.insetUm / 1000} on:change={(event) => { if (portal) portal.insetUm = Number(event.currentTarget.value) * 1000; onChange(); }} /><span>mm</span></label>
        <label>{uiTranslate("防回弹", $uiLanguage)}<input type="number" min="0" value={portal.hysteresisUm / 1000} on:change={(event) => { if (portal) portal.hysteresisUm = Number(event.currentTarget.value) * 1000; onChange(); }} /><span>mm</span></label>
        {#if portal.activationPolicy !== "Immediate" && "Dwell" in portal.activationPolicy}<label class="wide">{uiTranslate("停留时间", $uiLanguage)}<input type="number" min="50" max="3000" value={portal.activationPolicy.Dwell.milliseconds} on:change={(event) => { if (portal && portal.activationPolicy !== "Immediate" && "Dwell" in portal.activationPolicy) portal.activationPolicy.Dwell.milliseconds = Number(event.currentTarget.value); onChange(); }} /><span>ms</span></label>{/if}
      </div>
    {/if}

    <div class="portal-hint">{uiTranslate("从", $uiLanguage)} {source?.name} {uiTranslate("的", $uiLanguage)}{uiTranslate(portal.sourceEdge === "Right" ? "右" : portal.sourceEdge === "Left" ? "左" : portal.sourceEdge === "Top" ? "上" : "下", $uiLanguage)}{uiTranslate("侧高亮边段穿越，即可按对应位置进入", $uiLanguage)} {target?.name}。</div>
    <button class="primary-action" on:click={onFinish}>{uiTranslate("完成", $uiLanguage)}</button>
    <button class="test-action" on:click={onTest}><TestTube size={17} />{uiTranslate("测试这个边段", $uiLanguage)}</button>
  {:else}
    <div class="empty"><span><ArrowsLeftRight size={30} weight="duotone" /></span><h3>{uiTranslate("自动建立跨屏通道", $uiLanguage)}</h3><p>{uiTranslate("将两块屏幕按真实位置拖动靠近；相邻且有重叠的边段会自动连通。", $uiLanguage)}</p></div>
  {/if}
</aside>

<style>
  .inspector { width: 322px; padding: 22px; display: grid; align-content: start; gap: 17px; overflow: auto; }
  .title-row { display: flex; justify-content: space-between; gap: 10px; }.eyebrow { color: var(--accent-strong); font-size: 10px; font-weight: 800; letter-spacing: .08em; }.title-row h3 { margin: 6px 0 0; font-size: 15px; line-height: 1.4; }.status { height: fit-content; padding: 4px 8px; border-radius: 999px; color: var(--success); background: var(--success-soft); font-size: 10px; font-weight: 700; }.status.warning { color: var(--warning); background: var(--warning-soft); }
  .route-preview { display: grid; grid-template-columns: 1fr auto 1fr; align-items: center; gap: 8px; padding: 11px; border-radius: 10px; color: var(--accent); background: var(--surface-soft); }.route-preview span { display: grid; justify-items: center; gap: 4px; min-width: 0; }.route-preview small { max-width: 105px; overflow: hidden; color: var(--text-secondary); font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  section { display: grid; gap: 9px; }h4 { margin: 0; font-size: 12px; }.automatic-range { padding: 11px; border: 1px solid var(--border); border-radius: 10px; background: var(--surface-soft); }.automatic-range > div { display: flex; align-items: center; justify-content: space-between; }.automatic-range strong { color: var(--accent-strong); font-size: 11px; }.automatic-range p { margin: 0; color: var(--text-muted); font-size: 9px; line-height: 1.55; }
  .activation-grid { display: grid; grid-template-columns: repeat(3,1fr); gap: 7px; }.activation-grid button { display: grid; justify-items: start; gap: 5px; min-height: 98px; padding: 10px; border: 1px solid var(--border); border-radius: 10px; color: var(--text-secondary); background: var(--surface); text-align: left; }.activation-grid button.active { border-color: var(--accent); color: var(--accent-strong); background: var(--accent-soft); }.activation-grid strong { color: var(--text); font-size: 10px; }.activation-grid small { color: var(--text-muted); font-size: 8px; line-height: 1.4; }
  .toggle-row { display: grid; grid-template-columns: 1fr 42px; align-items: center; gap: 12px; }.toggle-row span { display: grid; gap: 3px; }.toggle-row strong { font-size: 11px; }.toggle-row small { color: var(--text-muted); font-size: 9px; line-height: 1.4; }.toggle-row input { position: absolute; opacity: 0; }.toggle-row i { position: relative; width: 40px; height: 23px; border-radius: 999px; background: var(--border-strong); }.toggle-row i::after { position: absolute; top: 3px; left: 3px; width: 17px; height: 17px; border-radius: 50%; background: #fff; box-shadow: 0 1px 3px #0002; content: ""; transition: 120ms ease; }.toggle-row input:checked + i { background: var(--accent); }.toggle-row input:checked + i::after { transform: translateX(17px); }.toggle-row input:focus-visible + i { outline: 3px solid var(--focus-ring); }
  .advanced-row { display: flex; min-height: 42px; align-items: center; justify-content: space-between; border: 1px solid var(--border); border-radius: 9px; padding: 0 11px; color: var(--text-secondary); background: var(--surface-soft); }.advanced-row :global(svg) { transition: transform 120ms ease; }.advanced-row :global(svg.expanded) { transform: rotate(90deg); }.advanced-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; padding: 10px; border-radius: 10px; background: var(--surface-soft); }.advanced-grid label { display: grid; grid-template-columns: 1fr 30px; gap: 5px; color: var(--text-muted); font-size: 9px; }.advanced-grid label.wide { grid-column: 1 / -1; }.advanced-grid input { grid-column: 1; min-width: 0; border: 1px solid var(--border); border-radius: 7px; padding: 6px; color: var(--text); background: var(--surface); }.advanced-grid span { align-self: end; padding-bottom: 6px; }
  .portal-hint { padding: 10px; border-radius: 9px; color: var(--accent-strong); background: var(--accent-soft); font-size: 10px; line-height: 1.5; }.primary-action, .test-action { display: flex; min-height: 40px; align-items: center; justify-content: center; gap: 7px; border-radius: 9px; font-weight: 650; }.primary-action { border: 1px solid var(--accent); color: #fff; background: var(--accent); }.test-action { border: 1px solid var(--accent); color: var(--accent-strong); background: var(--surface); }
  .empty { display: grid; min-height: 340px; place-content: center; justify-items: center; gap: 10px; text-align: center; }.empty > span { display: grid; width: 56px; height: 56px; place-items: center; border-radius: 15px; color: var(--accent); background: var(--accent-soft); }.empty h3 { margin: 0; }.empty p { max-width: 230px; margin: 0; color: var(--text-secondary); font-size: 12px; line-height: 1.55; }
</style>
