<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../../../i18n";
  import { CornersOut } from "phosphor-svelte";
  import type { DisplaySurface } from "../../../types";

  export let display: DisplaySurface;
  export let number = 1;
  export let label = display.name;
  export let x = 0;
  export let y = 0;
  export let width = 100;
  export let height = 60;
  export let selected = false;
  export let controller = false;
  export let online = true;
  export let statusLabel = "在线";
  export let onpointerdown: (event: PointerEvent) => void;
  export let onresizepointerdown: (event: PointerEvent) => void;
  export let onselect: () => void;
</script>

<g
  class:selected
  class:offline={!online}
  role="button"
  tabindex="0"
  aria-label={`${number} · ${label} · ${display.name} · ${statusLabel}`}
  aria-pressed={selected}
  on:pointerdown={onpointerdown}
  on:click|stopPropagation={onselect}
  on:keydown={(event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onselect();
    }
  }}
>
  <title
    >{label} · {display.name} · {display.pixelSize.width}×{display.pixelSize.height} · {Math.round(
      display.scaleFactor * 100,
    )}% · {statusLabel}</title
  >
  <rect {x} {y} {width} {height} rx="7" class="surface" />
  <svg {x} {y} {width} {height} overflow="hidden" aria-hidden="true">
    <text
      x={width / 2}
      y={height / 2 + (height < 50 ? 6 : -2)}
      text-anchor="middle"
      class="number"
      style:font-size={`${Math.max(12, Math.min(44, height * 0.3))}px`}>{number}</text
    >
    {#if width > 82 && height > 50}<text
        x={width / 2}
        y={height / 2 + 23}
        text-anchor="middle"
        class="name"
        >{label.length > Math.floor(width / 8)
          ? `${label.slice(0, Math.max(3, Math.floor(width / 8) - 1))}…`
          : label}</text
      >{/if}
    {#if !online && width > 75 && height > 55}<text
        x={width - 10}
        y="18"
        text-anchor="end"
        class="status">{uiTranslate(statusLabel, $uiLanguage)}</text
      >{/if}
  </svg>
  {#if controller && online}<circle cx={x + width - 23} cy={y + 11} r="3" class="controller" />{/if}
</g>
{#if selected}
  <foreignObject x={x + width - 23} y={y + height - 23} width="24" height="24">
    <button
      class="resize-handle"
      aria-label={uiTranslate("校准尺寸", $uiLanguage)}
      title={uiTranslate("校准尺寸", $uiLanguage)}
      on:pointerdown={(event) => {
        event.stopPropagation();
        onresizepointerdown(event);
      }}
      on:click|stopPropagation><CornersOut size={12} /></button
    >
  </foreignObject>
{/if}

<style>
  g {
    cursor: grab;
    outline: none;
  }
  g:active {
    cursor: grabbing;
  }
  .surface {
    fill: var(--accent-soft, #efeeff);
    stroke: var(--accent, #5b5ff0);
    stroke-width: 1.3;
  }
  .selected .surface,
  g:focus-visible .surface {
    stroke-width: 3;
  }
  .offline .surface {
    fill: var(--surface-soft, #f7f8fc);
    stroke: var(--text-muted, #989aa6);
    stroke-dasharray: 5 4;
  }
  .selected.offline .surface {
    stroke: var(--accent);
  }
  .number {
    fill: var(--text);
    font-weight: 650;
    pointer-events: none;
  }
  .name {
    fill: var(--text-secondary);
    font-size: 13px;
    pointer-events: none;
  }
  .status {
    fill: var(--text-muted);
    font-size: 9px;
    pointer-events: none;
  }
  .controller {
    fill: var(--success);
    stroke: var(--surface);
    stroke-width: 1.5;
  }
  .resize-handle {
    display: grid;
    width: 21px;
    height: 21px;
    place-items: center;
    border: 1px solid var(--accent);
    border-radius: 5px;
    color: var(--accent);
    background: var(--surface);
    cursor: nwse-resize;
  }
  .resize-handle:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
