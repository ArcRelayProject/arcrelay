<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../../../i18n";
  import type { DiagnosticRecord } from "../../../types";
  import { localLatencySamples } from "../diagnosticsModel.ts";
  export let records: DiagnosticRecord[] = [];
  $: local = localLatencySamples(records);
  $: latencies = local.samples.map((record) => record.latencyMicros! / 1000);
  $: sorted = [...latencies].sort((a, b) => a - b);
  $: p95 = sorted.length
    ? sorted[Math.min(Math.floor(sorted.length * 0.95), sorted.length - 1)]
    : null;
</script>

<div class="card panel">
  <div class="panel-title">
    <h3>
      {uiTranslate(
        local.scope === "receive" ? "本机接收处理耗时" : "本机采集处理耗时",
        $uiLanguage,
      )}
    </h3>
    <span class:good={p95 != null && p95 <= 16} class="value"
      >{uiTranslate(p95 == null ? "等待样本" : `${p95.toFixed(1)} ms p95`, $uiLanguage)}</span
    >
  </div>
  <div class="chart">
    {#each latencies.slice(-24) as value}<i
        style={`height:${Math.min((value / 24) * 100, 100)}%`}
        title={`${value.toFixed(2)} ms`}
      ></i>{/each}
  </div>
  <p class="muted">
    {uiTranslate(
      "显示每 128 次处理的固定抽样，不含网络传输；完整分段统计和 QUIC 指标见诊断日志。",
      $uiLanguage,
    )}
  </p>
</div>

<style>
  .panel {
    padding: 20px;
  }
  .panel-title {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  h3 {
    margin: 0;
  }
  .value {
    color: #b54708;
    font-size: 13px;
    font-weight: 700;
  }
  .value.good {
    color: #067647;
  }
  .chart {
    height: 90px;
    display: flex;
    align-items: end;
    gap: 4px;
    margin: 20px 0 10px;
    border-bottom: 1px solid #dbe0e8;
  }
  i {
    width: 100%;
    min-height: 2px;
    border-radius: 3px 3px 0 0;
    background: #635bdf;
  }
  p {
    margin: 0;
    font-size: 12px;
  }
</style>
