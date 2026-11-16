<script lang="ts">
  import { DropdownMenu } from "bits-ui";
  import { DotsThree, CaretRight } from "phosphor-svelte";
  export let label: string;
  export let items: {
    label: string;
    action: () => void;
    icon?: typeof DotsThree;
    danger?: boolean;
    disabled?: boolean;
    separator?: boolean;
    navigate?: boolean;
  }[];
</script>

<DropdownMenu.Root>
  <DropdownMenu.Trigger class="input-menu-trigger" aria-label={label} title={label}
    ><DotsThree size={23} weight="bold" /></DropdownMenu.Trigger
  >
  <DropdownMenu.Portal>
    <DropdownMenu.Content
      class="input-menu-content"
      sideOffset={8}
      align="end"
      collisionPadding={12}
    >
      {#each items as item}
        {#if item.separator}<DropdownMenu.Separator class="input-menu-separator" />{/if}
        <DropdownMenu.Item
          class={`input-menu-item ${item.danger ? "danger" : ""}`}
          disabled={item.disabled}
          onSelect={item.action}
        >
          {#if item.icon}<svelte:component this={item.icon} size={19} />{/if}
          <span>{item.label}</span>{#if item.navigate}<CaretRight size={16} />{/if}
        </DropdownMenu.Item>
      {/each}
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>

<style>
  :global(.input-menu-trigger) {
    display: inline-grid;
    width: 36px;
    height: 36px;
    place-items: center;
    flex: none;
    border: 0;
    border-radius: 9px;
    color: var(--text-secondary);
    background: transparent;
    cursor: pointer;
  }
  :global(.input-menu-trigger:hover),
  :global(.input-menu-trigger[data-state="open"]) {
    color: var(--accent);
    background: var(--accent-soft);
  }
  :global(.input-menu-content) {
    z-index: 1001;
    min-width: 220px;
    max-width: calc(100vw - 24px);
    padding: 5px;
    border: 1px solid var(--border);
    border-radius: 12px;
    color: var(--text);
    background: var(--surface);
    box-shadow: 0 8px 28px #10182820;
    outline: none;
    font: 14px var(--font-family-ui);
  }
  :global(.input-menu-item) {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 42px;
    padding: 8px 11px;
    border-radius: 7px;
    outline: none;
    cursor: pointer;
  }
  :global(.input-menu-item > span) {
    flex: 1;
  }
  :global(.input-menu-item[data-highlighted]) {
    color: var(--accent-strong);
    background: var(--accent-soft);
  }
  :global(.input-menu-item.danger) {
    color: var(--danger);
  }
  :global(.input-menu-item[data-disabled]) {
    opacity: 0.45;
    cursor: default;
  }
  :global(.input-menu-separator) {
    height: 1px;
    margin: 5px 9px;
    background: var(--border);
  }
</style>
