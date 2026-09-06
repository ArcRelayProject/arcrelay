<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../../../i18n";
  import { CornersOut } from "phosphor-svelte";
  import type { DisplaySurface } from "../../../types";

  export let display: DisplaySurface;
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
  on:pointerdown={onpointerdown}
  on:click={onselect}
  on:keydown={(event) => event.key === "Enter" && onselect()}
>
  <rect {x} {y} {width} {height} rx="9" class="surface" />
  <rect x={x + 8} y={y + 8} width={Math.max(width - 16, 0)} height={Math.max(height - 16, 0)} rx="5" class="screen" />
  <text x={x + 15} y={y + 27} class="name">{display.name}</text>
  <text x={x + 15} y={y + 46} class="details">
    {display.pixelSize.width}×{display.pixelSize.height} · {Math.round(display.scaleFactor * 100)}%
  </text>
  <text x={x + 15} y={y + height - 16} class="details">
    {uiTranslate(statusLabel, $uiLanguage)}{uiTranslate(display.logicalBounds.x === 0 && display.logicalBounds.y === 0 ? " · 主屏幕" : "", $uiLanguage)}
  </text>
  {#if controller}
    <circle cx={x + width - 16} cy={y + 16} r="6" class="controller" />
  {/if}
</g>
{#if selected}
  <foreignObject x={x + width - 15} y={y + height - 15} width="30" height="30">
    <button
      class="resize-handle"
      aria-label={uiTranslate("校准尺寸", $uiLanguage)}
      title={uiTranslate("校准尺寸", $uiLanguage)}
      on:pointerdown={(event) => { event.stopPropagation(); onresizepointerdown(event); }}
      on:click={(event) => event.stopPropagation()}
    ><CornersOut size={15} weight="bold" /></button>
  </foreignObject>
{/if}

<style>
  g { cursor: grab; outline: none; }
  g:active { cursor: grabbing; }
  .surface { fill: #252d3d; stroke: #4a556b; stroke-width: 2; }
  .screen { fill: #f8faff; stroke: #d5dbea; }
  .selected .surface { stroke: #6258ea; stroke-width: 4; }
  .offline { opacity: .4; stroke-dasharray: 7 5; }
  .name { fill: #202a3d; font-size: 14px; font-weight: 700; pointer-events: none; }
  .details { fill: #667085; font-size: 10px; pointer-events: none; }
  .controller { fill: #26b36a; stroke: #fff; stroke-width: 2; }
  .resize-handle { display: grid; width: 28px; height: 28px; place-items: center; border: 2px solid #fff; border-radius: 9px; color: #fff; background: #6258ea; box-shadow: 0 2px 8px #10182838; cursor: nwse-resize; }
  .resize-handle:focus-visible { outline: 3px solid var(--focus-ring); outline-offset: 2px; }
  @media (prefers-color-scheme: dark) {
    .screen { fill: #222a38; stroke: #465065; }
    .name { fill: #edf1f7; }
    .details { fill: #aeb8c9; }
  }
</style>
