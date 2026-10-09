<script lang="ts">
  import {
    ArrowRight,
    Check,
    CaretRight,
    CaretLeft,
    CopySimple,
    PencilSimple,
    PushPin,
    Star,
    Tag,
    Trash,
  } from "phosphor-svelte";
  const icons = {
    insert: ArrowRight,
    copy: CopySimple,
    edit: PencilSimple,
    pin: PushPin,
    favorite: Star,
    label: Tag,
    delete: Trash,
  };
  import { tick } from "svelte";
  import {
    inlineContextMenu,
    closeInlineContextMenu,
    selectInlineMenu,
    backInlineMenu,
    hoverInlineMenu,
    handleInlineMenuKey,
  } from "./inlineContextMenu";
  import { dismissibleDropdown } from "../dismissibleDropdown";
  let node: HTMLDivElement;
  let x = 0;
  let y = 0;
  $: menu = $inlineContextMenu;
  $: level = menu?.levels[menu.levels.length - 1];
  $: if (menu && level) void place(menu.x, menu.y, level.active);
  async function place(left: number, top: number, active: number) {
    await tick();
    if (!node) return;
    const bounds = node.getBoundingClientRect();
    x = Math.max(8, Math.min(left, window.innerWidth - bounds.width - 8));
    y = Math.max(8, Math.min(top, window.innerHeight - bounds.height - 8));
    node
      .querySelector<HTMLElement>(`[data-menu-index="${active}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }
</script>

{#if menu && level}
  <div
    bind:this={node}
    class="clipboard-inline-menu"
    role="menu"
    tabindex="-1"
    on:keydown={handleInlineMenuKey}
    aria-label={level.title || "Clipboard actions"}
    on:pointerdown={(event) => event.preventDefault()}
    on:contextmenu={(event) => event.preventDefault()}
    style:left={`${x}px`}
    style:top={`${y}px`}
    use:dismissibleDropdown={{ open: true, close: closeInlineContextMenu }}
  >
    {#if menu.levels.length > 1}
      <button class="menu-back" type="button" role="menuitem" on:click={backInlineMenu}
        ><CaretLeft size={15} /><span class="menu-label">{level.title}</span></button
      >
      <div class="menu-separator" role="separator"></div>
    {/if}
    {#each level.items as entry, index}
      {#if "item" in entry}
        <div class="menu-separator" role="separator"></div>
      {:else}
        <button
          type="button"
          role="menuitem"
          data-menu-index={index}
          disabled={entry.enabled === false}
          class:active={level.active === index}
          class:destructive={entry.destructive}
          aria-haspopup={entry.items ? "menu" : undefined}
          on:pointermove={() => hoverInlineMenu(index)}
          on:click={() => selectInlineMenu(index)}
        >
          {#if entry.icon}<svelte:component
              this={icons[entry.icon]}
              size={16}
            />{:else if entry.checked}<Check size={16} />{:else}<span
              class="icon-space"
              aria-hidden="true"
            ></span>{/if}
          <span class="menu-label">{entry.text}</span>{#if entry.items}<CaretRight size={14} />{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .clipboard-inline-menu {
    position: fixed;
    z-index: 1000;
    width: min(260px, calc(100vw - 16px));
    max-height: calc(100vh - 16px);
    overflow: auto;
    padding: 5px;
    border: 1px solid var(--border, #8884);
    border-radius: 10px;
    background: var(--surface, #fff);
    color: var(--text, #20212a);
    box-shadow: 0 10px 30px #0003;
  }
  button {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 12px;
    width: 100%;
    padding: 8px 10px;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: inherit;
    text-align: left;
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }
  button.active:not(:disabled),
  button:hover:not(:disabled) {
    background: var(--accent-soft, #7377ed20);
  }
  button.destructive {
    color: var(--danger, #d33b45);
  }
  .menu-label {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .icon-space {
    width: 16px;
    flex-shrink: 0;
  }
  button :global(svg) {
    flex-shrink: 0;
  }
  button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .menu-separator {
    margin: 4px;
    height: 1px;
    background: #8883;
  }
  .menu-back {
    font-weight: 600;
  }
  :global([data-theme="dark"]) .clipboard-inline-menu {
    background: #25262e;
    color: #eee;
  }
</style>
