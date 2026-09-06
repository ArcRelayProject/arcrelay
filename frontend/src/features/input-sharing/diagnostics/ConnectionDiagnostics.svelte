<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../../../i18n";
  export let discovered: string[] = [];
  export let connected: string[] = [];
</script>

<div class="card panel">
  <div class="panel-title"><h3>{uiTranslate("Peer 连接", $uiLanguage)}</h3><span class="badge">{connected.length} {uiTranslate("在线", $uiLanguage)}</span></div>
  {#if discovered.length === 0}<p class="muted">{uiTranslate("尚未发现同一局域网中的 Arc Input 设备。", $uiLanguage)}</p>{/if}
  {#each discovered as id}
    <div class="peer"><span class:online={connected.includes(id)} class="dot"></span><span class="mono">{id}</span><small>{connected.includes(id) ? "WorkspaceReady" : "Discovered"}</small></div>
  {/each}
</div>

<style>
  .panel { padding: 20px; }
  .panel-title { display: flex; justify-content: space-between; align-items: center; }
  h3 { margin: 0 0 14px; }
  p { font-size: 13px; }
  .peer { display: grid; grid-template-columns: 10px 1fr auto; align-items: center; gap: 9px; padding: 10px 0; border-top: 1px solid #edf0f5; font-size: 12px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: #c3cad5; }
  .dot.online { background: #22b66d; box-shadow: 0 0 0 4px #dcfae6; }
  small { color: #7d8798; }
</style>
