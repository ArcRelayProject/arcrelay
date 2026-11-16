<script lang="ts">
  import { t, translate as uiTranslate, language as uiLanguage } from "../../../i18n";
  import type { DisplaySurface, Portal } from "../../../types";

  export let portal: Portal;
  export let displays: Record<string, DisplaySurface>;
  export let project: (x: number, y: number) => { x: number; y: number };
  export let selected = false;
  export let onselect: () => void;

  function edgeSegment(
    display: DisplaySurface,
    edge: Portal["sourceEdge"],
    segment: { startUm: number; endUm: number },
    projector: typeof project,
  ) {
    const rect = display.deskRectUm;
    if (edge === "Left" || edge === "Right") {
      const x = edge === "Left" ? rect.x : rect.x + rect.width;
      return {
        start: projector(x, rect.y + segment.startUm),
        end: projector(x, rect.y + segment.endUm),
      };
    }
    const y = edge === "Top" ? rect.y : rect.y + rect.height;
    return {
      start: projector(rect.x + segment.startUm, y),
      end: projector(rect.x + segment.endUm, y),
    };
  }

  $: source = displays[portal.sourceDisplay];
  $: target = displays[portal.targetDisplay];
  $: sourceSegment = source
    ? edgeSegment(source, portal.sourceEdge, portal.sourceSegment, project)
    : null;
  $: targetSegment = target
    ? edgeSegment(target, portal.targetEdge, portal.targetSegment, project)
    : null;
</script>

{#if source && target && sourceSegment && targetSegment}
  <g
    class:selected
    class:inactive={portal.status !== "Active"}
    role="button"
    tabindex="0"
    aria-label={t("{source} 与 {target} 的自动跨屏边段", $uiLanguage, {
      source: source.name,
      target: target.name,
    })}
    on:pointerdown|stopPropagation
    on:click|stopPropagation={onselect}
    on:keydown={(event) => (event.key === "Enter" || event.key === " ") && onselect()}
  >
    <line
      class="hit-area"
      x1={sourceSegment.start.x}
      y1={sourceSegment.start.y}
      x2={sourceSegment.end.x}
      y2={sourceSegment.end.y}
    />
    <line
      class="hit-area"
      x1={targetSegment.start.x}
      y1={targetSegment.start.y}
      x2={targetSegment.end.x}
      y2={targetSegment.end.y}
    />
    <line
      class="segment"
      x1={sourceSegment.start.x}
      y1={sourceSegment.start.y}
      x2={sourceSegment.end.x}
      y2={sourceSegment.end.y}
    />
    <line
      class="segment"
      x1={targetSegment.start.x}
      y1={targetSegment.start.y}
      x2={targetSegment.end.x}
      y2={targetSegment.end.y}
    />
  </g>
{/if}

<style>
  g {
    cursor: pointer;
  }
  .segment {
    stroke: #665deb;
    stroke-width: 3;
    stroke-linecap: round;
  }
  .hit-area {
    stroke: transparent;
    stroke-width: 20;
  }
  .selected .segment {
    stroke: var(--accent-strong);
    stroke-width: 5;
  }
  .inactive .segment {
    opacity: 0.36;
    stroke-dasharray: 7 5;
  }
</style>
