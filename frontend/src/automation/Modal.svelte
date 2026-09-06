<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../i18n";
  import { X } from "phosphor-svelte";
  export let title: string;
  export let close: () => void;
  export let busy = false;
  export let wide = false;
  function mount(node: HTMLDialogElement) {
    const previous = document.activeElement as HTMLElement | null;
    node.showModal();
    (
      node.querySelector<HTMLElement>("input, [data-select-trigger], textarea") ??
      node.querySelector<HTMLElement>("button")
    )?.focus();
    return {
      destroy() {
        node.close();
        previous?.focus();
      },
    };
  }
</script>

<dialog
  class="au-modal"
  class:wide
  aria-label={title}
  use:mount
  on:cancel|preventDefault={() => !busy && close()}
>
  <header>
    <h2>{title}</h2>
    <button
      class="au-icon"
      aria-label={uiTranslate("关闭弹窗", $uiLanguage)}
      disabled={busy}
      on:click={close}><X size={20} /></button
    >
  </header>
  <slot />
</dialog>
