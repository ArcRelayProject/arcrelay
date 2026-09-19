<script lang="ts">
  import { DotsSixVertical } from "phosphor-svelte";
  export let label: string;
  export let disabled = false;
  export let onPress: (event: PointerEvent) => void;
</script>

<button
  type="button"
  class="clipboard-drag-handle"
  {disabled}
  aria-label={label}
  title={label}
  draggable="false"
  on:pointerdown={(event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    onPress(event);
  }}
  on:dragstart|preventDefault
  on:click|preventDefault|stopPropagation
>
  <DotsSixVertical size={16} /><span>{label}</span>
</button>

<style>
  .clipboard-drag-handle {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    width: auto;
    min-height: 30px;
    padding: 4px 8px;
    flex-shrink: 0;
    white-space: nowrap;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    background: var(--surface-raised);
    color: var(--text-secondary);
    font: inherit;
    font-size: 12px;
    cursor: grab;
    user-select: none;
  }
  .clipboard-drag-handle:active {
    cursor: grabbing;
  }
  .clipboard-drag-handle:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .clipboard-drag-handle:focus-visible {
    outline: 2px solid var(--accent-color, #6563ff);
    outline-offset: 2px;
  }
</style>
