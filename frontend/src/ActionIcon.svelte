<script lang="ts">
  import { Lightning } from "phosphor-svelte";

  export let iconId: string;
  export let iconSvg: string;
  export let size = 24;
  export let color = "currentColor";
</script>

{#if iconSvg}
  <span
    class="action-svg"
    class:custom={iconId === "custom"}
    style:--action-svg-size={`${size}px`}
    style:--action-svg-color={color}
    aria-hidden="true"
  >{@html iconSvg}</span>
{:else}
  <Lightning {size} {color} weight="duotone" aria-hidden="true" />
{/if}

<style>
  .action-svg {
    display: inline-grid;
    width: var(--action-svg-size);
    height: var(--action-svg-size);
    place-items: center;
    color: var(--action-svg-color);
    line-height: 0;
  }

  .action-svg :global(svg) {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
