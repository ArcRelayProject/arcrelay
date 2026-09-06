<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../../../i18n";
  import type { DiagnosticRecord } from "../../../types";
  export let records: DiagnosticRecord[] = [];
  $: endToEnd = records.filter((record) => record.category === "input-e2e");
  $: samples = endToEnd.length ? endToEnd : records.filter((record) => record.category === "input-latency");
  $: latencies = samples.flatMap((record) => record.latencyMicros == null ? [] : [record.latencyMicros / 1000]);
  $: sorted = [...latencies].sort((a, b) => a - b);
  $: p95 = sorted.length ? sorted[Math.min(Math.floor(sorted.length * .95), sorted.length - 1)] : null;
</script>

<div class="card panel">
  <div class="panel-title"><h3>{uiTranslate("输入延迟", $uiLanguage)}</h3><span class:good={p95 != null && p95 <= 16} class="value">{uiTranslate(p95 == null ? "等待样本" : `${p95.toFixed(1)} ms p95`, $uiLanguage)}</span></div>
  <div class="chart">
    {#each latencies.slice(-24) as value}<i style={`height:${Math.min(value / 24 * 100, 100)}%`} title={`${value.toFixed(2)} ms`}></i>{/each}
  </div>
  <p class="muted">{uiTranslate("目标：正常局域网 capture-to-inject p95 ≤ 16 ms；尚无对端样本时显示控制器管线延迟。", $uiLanguage)}</p>
</div>

<style>
  .panel { padding: 20px; }
  .panel-title { display: flex; justify-content: space-between; align-items: center; }
  h3 { margin: 0; }
  .value { color: #b54708; font-size: 13px; font-weight: 700; }
  .value.good { color: #067647; }
  .chart { height: 90px; display: flex; align-items: end; gap: 4px; margin: 20px 0 10px; border-bottom: 1px solid #dbe0e8; }
  i { width: 100%; min-height: 2px; border-radius: 3px 3px 0 0; background: #635bdf; }
  p { margin: 0; font-size: 12px; }
</style>
