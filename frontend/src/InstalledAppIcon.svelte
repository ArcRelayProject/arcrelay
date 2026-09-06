<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "./i18n";
  import { AppWindow } from "phosphor-svelte";
  import { onMount } from "svelte";

  import { bridge } from "./bridge";

  export let path: string;
  export let name: string;
  export let initialIcon: string | null = null;
  export let size = 22;

  let root: HTMLSpanElement;
  let icon = initialIcon;
  let requested = false;

  async function load() {
    if (requested || icon) return;
    requested = true;
    try {
      icon = await bridge.getInstalledAppIcon(path);
    } catch {
      icon = null;
    }
  }

  onMount(() => {
    if (!("IntersectionObserver" in window)) {
      void load();
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          observer.disconnect();
          void load();
        }
      },
      { rootMargin: "120px" },
    );
    observer.observe(root);
    return () => observer.disconnect();
  });
</script>

<span class="installed-app-icon" bind:this={root} style={`--installed-icon-size:${size}px`}>
  {#if icon}
    <img src={icon} alt={uiTranslate((`${name} 图标`), $uiLanguage)} />
  {:else}
    <AppWindow {size} />
  {/if}
</span>

<style>
  .installed-app-icon {
    display: inline-grid;
    width: var(--installed-icon-size);
    height: var(--installed-icon-size);
    flex: 0 0 auto;
    place-items: center;
  }

  img {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
</style>
