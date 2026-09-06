<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../../../i18n";
  import type { WorkspaceLayout } from "../../../types";
  export let layout: WorkspaceLayout | null;
</script>

<div class="card panel">
  <h3>{uiTranslate("边缘状态", $uiLanguage)}</h3>
  {#if !layout || layout.portals.length === 0}<p class="muted">{uiTranslate("尚未创建 Portal。", $uiLanguage)}</p>{/if}
  {#each layout?.portals ?? [] as portal}
    <div class="portal"><span class:active={portal.status === "Active"}></span><div><strong>{portal.portalId}</strong><small>{portal.sourceDisplay} → {portal.targetDisplay}</small></div><em>{portal.status}</em></div>
  {/each}
</div>

<style>
  .panel { padding: 20px; }
  h3 { margin: 0 0 14px; }
  p { font-size: 13px; }
  .portal { display: grid; grid-template-columns: 8px 1fr auto; gap: 10px; align-items: center; padding: 9px 0; border-top: 1px solid #edf0f5; }
  .portal > span { width: 8px; height: 8px; border-radius: 50%; background: #e1a11a; }
  .portal > span.active { background: #22b66d; }
  .portal div { display: grid; gap: 3px; font-size: 12px; }
  small, em { color: #7d8798; font-size: 10px; font-style: normal; }
</style>
