<script lang="ts">
  import AppSelect from "../../components/AppSelect.svelte";
  import { translate as uiTranslate, language as uiLanguage } from "../../i18n";
  import { onMount } from "svelte";
  import {
    ArrowRight,
    ArrowsClockwise,
    Bug,
    CaretDown,
    Clock,
    DotsThree,
    Check,
    Copy,
    DesktopTower,
    DownloadSimple,
    FolderOpen,
    Gauge,
    Monitor,
    Pulse,
    ShieldCheck,
    WifiHigh,
  } from "phosphor-svelte";
  import { bridge } from "../../bridge";
  import type { RuntimeSnapshot } from "../../types";
  import type { LogStatus } from "../../types";
  import { activePortalCount, friendlyDeviceName, inputEventRate, percentile, portalEdgeTestRequest } from "./inputSharingUi";

  import { diagnosticHealth, latencyChart, latencyTone } from "./diagnosticsModel";

  export let snapshot: RuntimeSnapshot;
  export let onRefresh: () => Promise<void> = async () => {};
  export let notify: (message: string, error?: boolean) => void = () => {};

  let timeRange = "15";
  let copied = false;
  let refreshing = false;
  let showAllEvents = false;
  let testingPortalId: string | null = null;
  let logStatus: LogStatus | null = null;
  let exportingLogs = false;
  let changingLogLevel = false;
  let now = Date.now();
  $: rangeStart = now - Number(timeRange) * 60_000;
  $: rangedDiagnostics = snapshot.diagnostics.filter((record) => record.timestampMs >= rangeStart && record.timestampMs <= now);
  $: chart = latencyChart(rangedDiagnostics, rangeStart, now);
  $: latencies = chart.values;
  $: eventRate = inputEventRate(rangedDiagnostics);
  $: p50 = percentile(latencies, .5);
  $: p95 = percentile(latencies, .95);
  $: maximum = latencies.length ? Math.max(...latencies) : null;
  $: connected = snapshot.connectedPeers.length > 0;
  $: displays = snapshot.configuration.layout?.displays ?? {};
  $: portals = snapshot.configuration.layout?.portals ?? [];
  $: remoteId = snapshot.connectedPeers[0] ?? Object.values(displays).find((display) => display.deviceId !== snapshot.serviceInstanceId)?.deviceId;
  $: remotePeer = remoteId ? snapshot.nearbyPeers.find((peer) => peer.serviceInstanceId === remoteId) : null;
  $: remoteInjectionReady = Boolean(remotePeer?.capabilities?.canInjectAppPointer || (remotePeer?.capabilities?.canInjectAbsolutePointer && remotePeer?.capabilities?.canInjectKeyboard));
  $: captureReady = snapshot.capabilities.canCapturePointer && snapshot.capabilities.canCaptureKeyboard && snapshot.capabilities.canSuppressLocalInput;
  $: health = diagnosticHealth({
    enabled: snapshot.configuration.inputSharingEnabled,
    connected, captureReady, injectionReady: remoteInjectionReady,
    activePortals: activePortalCount(snapshot), p95,
  });
  $: detailedLogging = Boolean(logStatus?.detailedUntilMs && logStatus.detailedUntilMs > now);
  $: visibleEvents = rangedDiagnostics.slice(showAllEvents ? -20 : -4).reverse();
  $: latencyLabel = !latencies.length || chart.endToEnd ? "端到端延迟" : "控制器管线延迟";

  async function copyReport() {
    const report = [
      `Arc Input: ${health.title}`,
      `connected peers: ${snapshot.connectedPeers.length}`,
      `active portals: ${activePortalCount(snapshot)}`,
      `latency p95: ${p95?.toFixed(2) ?? "no samples"} ms`,
      ...rangedDiagnostics.slice(-20).map((event) => `${new Date(event.timestampMs).toISOString()} [${event.category}] ${event.message}`),
    ].join("\n");
    try {
      await navigator.clipboard.writeText(report);
    } catch {
      const textarea = document.createElement("textarea");
      textarea.value = report;
      textarea.style.position = "fixed";
      textarea.style.opacity = "0";
      document.body.appendChild(textarea);
      textarea.select();
      document.execCommand("copy");
      textarea.remove();
    }
    copied = true;
    notify("诊断报告已复制");
    window.setTimeout(() => copied = false, 1600);
  }

  async function refreshDiagnostics() {
    refreshing = true;
    try {
      await onRefresh();
      now = Date.now();
      notify("诊断数据已刷新");
    } catch (error) {
      notify(`诊断刷新失败：${String(error)}`, true);
    } finally {
      refreshing = false;
    }
  }

  async function testPortal(portalId: string) {
    const portal = portals.find((value) => value.portalId === portalId);
    const display = portal ? displays[portal.sourceDisplay] : null;
    if (!portal || !display) {
      notify("找不到这条边缘对应的来源屏幕", true);
      return;
    }
    const request = portalEdgeTestRequest(portal, displays);
    if (!request) return notify("找不到这条边缘对应的来源屏幕", true);
    testingPortalId = portalId;
    try {
      const result = await bridge.testEdge(request);
      notify(result.portalIds.includes(portalId)
        ? `边缘测试成功，已路由到${displays[result.displayId]?.name ?? "目标屏幕"}`
        : "测试移动未穿越该边缘，请检查方向、区段或激活策略", !result.portalIds.includes(portalId));
    } catch (error) {
      notify(`边缘测试失败：${String(error)}`, true);
    } finally {
      testingPortalId = null;
    }
  }

  async function openLogs() {
    try {
      await bridge.openLogDirectory();
      notify("已打开日志目录");
    } catch (error) {
      notify(`打开日志目录失败：${String(error)}`, true);
    }
  }

  async function exportLogs() {
    exportingLogs = true;
    try {
      const bundle = await bridge.exportDiagnosticBundle();
      notify(`诊断包已导出：${bundle.fileName}`);
      logStatus = await bridge.getLogStatus();
    } catch (error) {
      notify(`导出诊断包失败：${String(error)}`, true);
    } finally {
      exportingLogs = false;
    }
  }

  async function toggleDetailedLogging() {
    changingLogLevel = true;
    try {
      const active = Boolean(logStatus?.detailedUntilMs && logStatus.detailedUntilMs > Date.now());
      logStatus = await bridge.setDetailedLogging(!active);
      notify(active ? "详细日志已关闭" : "已开启 15 分钟详细日志");
    } catch (error) {
      notify(`切换详细日志失败：${String(error)}`, true);
    } finally {
      changingLogLevel = false;
    }
  }

  onMount(() => {
    void bridge.getLogStatus().then((status) => logStatus = status).catch(() => {});
    const timer = window.setInterval(() => now = Date.now(), 5_000);
    return () => window.clearInterval(timer);
  });
</script>

<section class="page" aria-label={uiTranslate("连接诊断", $uiLanguage)}>
  <div class="diagnostic-toolbar">
    <div class="range-control">
      <Clock size={16} />
      <AppSelect bind:value={timeRange} aria-label={uiTranslate("诊断时间范围", $uiLanguage)}
        options={[
          { value: "15", label: uiTranslate("最近 15 分钟", $uiLanguage) },
          { value: "60", label: uiTranslate("最近 1 小时", $uiLanguage) },
          { value: "1440", label: uiTranslate("最近 24 小时", $uiLanguage) },
        ]}
      />
    </div>
    <div class="toolbar-actions">
      <button class="quiet-action" on:click={copyReport}>
        {#if copied}<Check size={15} />{:else}<Copy size={15} />{/if}
        {uiTranslate(copied ? "已复制" : "复制报告", $uiLanguage)}
      </button>
      <button class="refresh-action" disabled={refreshing} on:click={refreshDiagnostics}>
        <ArrowsClockwise class={refreshing ? "spinning" : ""} size={15} />
        {uiTranslate(refreshing ? "检测中" : "重新检测", $uiLanguage)}
      </button>
    </div>
  </div>

  <div class="health-card" data-tone={health.tone}>
    <div class="health-heading">
      <span class="health-icon">
        {#if health.tone === "success"}<Check size={23} weight="bold" />
        {:else if health.tone === "warning"}<Gauge size={23} />
        {:else}<Pulse size={23} />{/if}
      </span>
      <div class="health-copy">
        <h2>{uiTranslate(health.title, $uiLanguage)}</h2>
        <p>{uiTranslate(health.detail, $uiLanguage)}</p>
      </div>
      <span class="health-status"><i></i>{uiTranslate(health.tone === "success" ? "运行正常" : health.tone === "warning" ? "需要关注" : "等待就绪", $uiLanguage)}</span>
    </div>
    <div class="metric-grid">
      <article>
        <span class="metric-label"><Pulse size={15} />{uiTranslate("延迟", $uiLanguage)}<small>P95</small></span>
        <strong class="metric-value" data-tone={latencyTone(p95)}>{p95 == null ? "—" : p95.toFixed(1)}{#if p95 != null}<span>ms</span>{/if}</strong>
        <small>{uiTranslate(p95 == null ? "等待输入样本" : p95 > 16 ? "高于 16 ms 目标" : "目标 ≤ 16 ms", $uiLanguage)}</small>
      </article>
      <article>
        <span class="metric-label"><Gauge size={15} />{uiTranslate("输入频率", $uiLanguage)}</span>
        <strong class="metric-value">{eventRate == null ? "—" : eventRate < 1 ? "< 1" : eventRate.toFixed(0)}{#if eventRate != null}<span>Hz</span>{/if}</strong>
        <small>{uiTranslate("根据当前输入样本估算", $uiLanguage)}</small>
      </article>
      <article>
        <span class="metric-label"><WifiHigh size={15} />{uiTranslate("丢包", $uiLanguage)}</span>
        <strong class="metric-value neutral">—</strong>
        <small>{uiTranslate("未采样", $uiLanguage)}</small>
      </article>
      <article>
        <span class="metric-label"><DesktopTower size={15} />{uiTranslate("连接状态", $uiLanguage)}</span>
        <strong class="metric-value text-value" data-tone={!connected ? "neutral" : remoteInjectionReady ? "success" : "warning"}>{uiTranslate(connected ? remoteInjectionReady ? "可用" : "能力受限" : "未连接", $uiLanguage)}</strong>
        <small>{activePortalCount(snapshot)} {uiTranslate("个自动边段可用", $uiLanguage)}</small>
      </article>
    </div>
  </div>

  <div class="diagnostic-grid">
    <section class="panel quality-card" aria-labelledby="quality-title">
      <div class="panel-heading">
        <div><h3 id="quality-title">{uiTranslate("连接质量", $uiLanguage)}</h3><p><i class="legend-dot"></i>{uiTranslate(latencyLabel, $uiLanguage)}</p></div>
        <span class="peer-label" title={remoteId ? friendlyDeviceName(remoteId, snapshot) : undefined}><DesktopTower size={14} /><span>{uiTranslate(remoteId ? friendlyDeviceName(remoteId, snapshot) : "目标设备", $uiLanguage)}</span></span>
      </div>
      <div class="chart-wrap">
        <div class="chart-scale" aria-hidden="true"><span>{chart.maximum} ms</span><span>0</span></div>
        <div class="chart">
          <svg viewBox="0 0 520 150" preserveAspectRatio="none" role="img" aria-label={uiTranslate(latencyLabel, $uiLanguage)}>
            <line x1="0" y1="12" x2="520" y2="12" /><line x1="0" y1="74" x2="520" y2="74" /><line x1="0" y1="136" x2="520" y2="136" />
            <line x1="0" y1={chart.targetY} x2="520" y2={chart.targetY} class="target" />
            {#if chart.path}<path d={chart.path} />{/if}
            {#if chart.points.length === 1}<circle cx={chart.points[0].x} cy={chart.points[0].y} r="3" />{/if}
          </svg>
          <span class="target-label" style:top={`${chart.targetY / 150 * 100}%`}>{uiTranslate("目标 ≤ 16 ms", $uiLanguage)}</span>
          {#if !chart.path}<div class="chart-empty"><Pulse size={24} /><span>{uiTranslate("发生跨屏输入后显示延迟", $uiLanguage)}</span></div>{/if}
        </div>
      </div>
      <div class="chart-axis"><span>{uiTranslate(timeRange === "15" ? "15 分钟前" : timeRange === "60" ? "1 小时前" : "24 小时前", $uiLanguage)}</span><span>{uiTranslate("现在", $uiLanguage)}</span></div>
      <div class="quality-stats">
        <div><small>{uiTranslate("p50（中位）", $uiLanguage)}</small><strong data-tone={latencyTone(p50)}>{p50 == null ? "—" : p50.toFixed(1)}{#if p50 != null}<span>ms</span>{/if}</strong></div>
        <div><small>P95</small><strong data-tone={latencyTone(p95)}>{p95 == null ? "—" : p95.toFixed(1)}{#if p95 != null}<span>ms</span>{/if}</strong></div>
        <div><small>{uiTranslate("最大", $uiLanguage)}</small><strong data-tone={latencyTone(maximum)}>{maximum == null ? "—" : maximum.toFixed(1)}{#if maximum != null}<span>ms</span>{/if}</strong></div>
      </div>
    </section>

    <section class="panel path-card" aria-labelledby="path-title">
      <div class="panel-heading"><div><h3 id="path-title">{uiTranslate("连接路径", $uiLanguage)}</h3><p>{uiTranslate("从本机捕获到远程输入", $uiLanguage)}</p></div><ShieldCheck size={18} /></div>
      <ol class="path-list">
        <li class="path-step">
          <span class="path-icon"><Monitor size={20} /></span>
          <div><strong>{uiTranslate("这台设备", $uiLanguage)}</strong><small>{uiTranslate(captureReady ? "鼠标与键盘捕获可用" : "请检查输入捕获权限", $uiLanguage)}</small></div>
          <em data-tone={captureReady ? "success" : "warning"}><i></i>{uiTranslate(captureReady ? "正常" : "受限", $uiLanguage)}</em>
        </li>
        <li class="path-step">
          <span class="path-icon"><WifiHigh size={20} /></span>
          <div><strong>{uiTranslate("本地网络", $uiLanguage)}</strong><small>{uiTranslate(connected ? "加密连接" : "等待连接", $uiLanguage)}</small></div>
          <em data-tone={connected ? "success" : "neutral"}><i></i>{uiTranslate(connected ? "已连接" : "未连接", $uiLanguage)}</em>
        </li>
        <li class="path-step">
          <span class="path-icon"><DesktopTower size={20} /></span>
          <div><strong>{uiTranslate(remoteId ? friendlyDeviceName(remoteId, snapshot) : "目标设备", $uiLanguage)}</strong><small>{uiTranslate(!connected ? "尚未连接" : remoteInjectionReady ? "鼠标与键盘注入可用" : remotePeer?.capabilities ? remotePeer.capabilities.limitation ?? "输入注入能力不完整" : "等待目标能力信息", $uiLanguage)}</small></div>
          <em data-tone={!connected ? "neutral" : remoteInjectionReady ? "success" : "warning"}><i></i>{uiTranslate(!connected ? "未连接" : remoteInjectionReady ? "正常" : "受限", $uiLanguage)}</em>
        </li>
      </ol>
      <div class="path-footer"><ShieldCheck size={14} />{uiTranslate(connected ? "设备间通信已加密" : "连接后显示完整路径", $uiLanguage)}</div>
    </section>

    <section class="panel portals-card" aria-labelledby="portals-title">
      <div class="panel-heading"><div><h3 id="portals-title">{uiTranslate("跨屏边缘", $uiLanguage)}<span class="count">{portals.length}</span></h3><p>{uiTranslate("检查屏幕之间的穿越通道", $uiLanguage)}</p></div></div>
      {#each portals as portal}
        <div class="portal-row">
          <span class="portal-icon"><Monitor size={17} /></span>
          <span class="portal-name"><span>{uiTranslate(displays[portal.sourceDisplay]?.name ?? "屏幕", $uiLanguage)}</span><span><ArrowRight size={12} />{uiTranslate(displays[portal.targetDisplay]?.name ?? "屏幕", $uiLanguage)}</span></span>
          <em data-tone={portal.status === "Active" ? "success" : "warning"}><i></i>{uiTranslate(portal.status === "Active" ? "可用" : "已暂停", $uiLanguage)}</em>
          <button class="test-action" disabled={portal.status !== "Active" || testingPortalId === portal.portalId} on:click={() => testPortal(portal.portalId)}>{uiTranslate(testingPortalId === portal.portalId ? "测试中" : "测试", $uiLanguage)}<ArrowRight size={13} /></button>
        </div>
      {/each}
      {#if portals.length === 0}<div class="empty-row"><Monitor size={24} />{uiTranslate("当前没有相邻且重叠的跨屏边段。", $uiLanguage)}</div>{/if}
    </section>

    <section class="panel timeline-card" aria-labelledby="timeline-title">
      <div class="panel-heading"><div><h3 id="timeline-title">{uiTranslate("最近事件", $uiLanguage)}</h3><p>{uiTranslate("所选时间范围内的活动", $uiLanguage)}</p></div>
        {#if rangedDiagnostics.length > 4}<button class="text-action" aria-expanded={showAllEvents} on:click={() => showAllEvents = !showAllEvents}>{uiTranslate(showAllEvents ? "收起" : "更多", $uiLanguage)}<CaretDown class={showAllEvents ? "expanded" : ""} size={14} /></button>{/if}
      </div>
      <div class="timeline-events">
        {#each visibleEvents as record, index}
          <div class="timeline-row"><i class:accent={index === 0}></i><time>{new Date(record.timestampMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })}</time><span>{record.message}</span></div>
        {/each}
        {#if rangedDiagnostics.length === 0}<div class="empty-row"><Clock size={24} />{uiTranslate("所选时间范围内暂无事件", $uiLanguage)}</div>{/if}
      </div>
    </section>
  </div>

  <details class="log-tools">
    <summary><span><DotsThree size={20} />{uiTranslate("日志与诊断工具", $uiLanguage)}{#if detailedLogging}<i class="logging-dot"></i>{/if}</span><CaretDown size={15} /></summary>
    <div class="log-tools-content">
      <p>{uiTranslate("需要进一步排查时，可开启详细日志或导出诊断包。", $uiLanguage)}</p>
      <div class="log-actions">
        <button class="secondary-action" class:logging={detailedLogging} aria-pressed={detailedLogging} disabled={changingLogLevel} on:click={toggleDetailedLogging}><Bug size={15} />{uiTranslate(detailedLogging ? "关闭详细日志" : "详细日志 15 分钟", $uiLanguage)}</button>
        <button class="secondary-action" on:click={openLogs}><FolderOpen size={15} />{uiTranslate("日志目录", $uiLanguage)}</button>
        <button class="secondary-action" disabled={exportingLogs} on:click={exportLogs}><DownloadSimple size={15} />{uiTranslate(exportingLogs ? "导出中…" : "导出诊断包", $uiLanguage)}</button>
      </div>
    </div>
  </details>
  <div class="privacy-note"><ShieldCheck size={14} /><span>{uiTranslate(logStatus?.enabled ? `日志已写盘 · 保留 ${logStatus.retentionDays} 天` : "磁盘日志暂不可用", $uiLanguage)} {uiTranslate("· 不记录具体按键、文字或剪贴板内容", $uiLanguage)}</span></div>
</section>

<style>
  .page { --diagnostic-success: color-mix(in srgb, var(--success) 76%, var(--text)); display: flex; min-width: 0; flex-direction: column; gap: 16px; padding: 2px 2px 24px; container-type: inline-size; }
  .diagnostic-toolbar, .toolbar-actions, .range-control { display: flex; align-items: center; gap: 8px; }
  .diagnostic-toolbar { justify-content: space-between; flex-wrap: wrap; gap: 10px; }
  .range-control { color: var(--text-muted); }
  .range-control :global(.app-select-trigger) { min-height: 32px; border-color: transparent; background: transparent; padding: 5px 8px; font-size: 12px; }
  .toolbar-actions { flex-wrap: wrap; }
  button, summary { -webkit-tap-highlight-color: transparent; }
  button { display: inline-flex; min-height: 32px; align-items: center; justify-content: center; gap: 6px; border: 1px solid transparent; border-radius: 7px; padding: 6px 11px; font-family: inherit; font-size: 12px; line-height: 1.4; font-weight: 550; white-space: nowrap; transition: background .15s, border-color .15s; }
  button:disabled { opacity: .5; }
  button:focus-visible, summary:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }
  .quiet-action, .text-action { color: var(--text-secondary); background: transparent; }
  .quiet-action:hover, .text-action:hover { color: var(--accent-strong); background: var(--accent-soft); }
  .refresh-action { color: var(--text-inverse); background: var(--accent); border-color: var(--accent); box-shadow: 0 2px 3px color-mix(in srgb, var(--accent) 14%, transparent); }
  .refresh-action:hover { background: var(--accent-strong); }
  .health-card { --health-color: var(--text-muted); --health-soft: var(--surface-soft); border: 1px solid var(--border); border-radius: 12px; overflow: hidden; background: var(--surface-raised); }
  .health-card[data-tone="success"] { --health-color: var(--diagnostic-success); --health-soft: var(--success-soft); }
  .health-card[data-tone="warning"] { --health-color: var(--warning); --health-soft: var(--warning-soft); }
  .health-heading { display: flex; align-items: center; gap: 14px; padding: 18px 22px; background: color-mix(in srgb, var(--health-soft) 48%, var(--surface-raised)); }
  .health-icon { display: grid; place-items: center; flex: none; width: 44px; height: 44px; border: 1px solid color-mix(in srgb, var(--health-color) 16%, transparent); border-radius: 12px; color: var(--health-color); background: var(--surface-raised); }
  .health-copy { min-width: 0; flex: 1; }
  .health-copy h2 { margin: 0; color: var(--text); font-size: 19px; line-height: 1.4; letter-spacing: -.025em; font-weight: 650; }
  .health-copy p { margin: 5px 0 0; color: var(--text-secondary); font-size: 11px; line-height: 1.6; }
  .health-status { display: inline-flex; flex: none; align-items: center; gap: 6px; padding: 5px 9px; border-radius: 6px; color: var(--health-color); background: var(--surface-raised); font-size: 10px; }
  .health-status i, em i { width: 5px; height: 5px; flex: none; border-radius: 50%; background: currentColor; }
  .metric-grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); padding: 16px 0; border-top: 1px solid var(--border); }
  .metric-grid article { display: flex; min-width: 0; flex-direction: column; align-items: flex-start; gap: 8px; padding: 0 22px; border-right: 1px solid var(--border); }
  .metric-grid article:last-child { border-right: 0; }
  .metric-label { display: flex; align-items: center; gap: 7px; color: var(--text-secondary); font-size: 11px; }
  .metric-label small { margin-left: 2px; font-size: 9px; }
  .metric-grid article > small { color: var(--text-muted); font-size: 10px; line-height: 1.5; }
  .metric-value { display: flex; align-items: baseline; gap: 5px; color: var(--text); font-size: 27px; line-height: 1.2; letter-spacing: -.045em; font-weight: 620; font-variant-numeric: tabular-nums; }
  .metric-value span { color: var(--text-muted); font-size: 12px; letter-spacing: 0; font-weight: 450; }
  .metric-value.text-value { min-height: 33px; align-items: center; font-size: 22px; letter-spacing: -.025em; }
  .metric-value.neutral, [data-tone="neutral"]:is(strong, em) { color: var(--text-muted); }
  [data-tone="warning"]:is(strong, em) { color: var(--warning); }
  [data-tone="success"]:is(strong, em) { color: var(--diagnostic-success); }
  .diagnostic-grid { display: grid; grid-template-columns: minmax(0, 1.25fr) minmax(0, 1fr); gap: 16px; align-items: stretch; }
  .panel { min-width: 0; border: 1px solid var(--border); border-radius: 12px; padding: 20px; background: var(--surface-raised); }
  .quality-card, .path-card { align-self: stretch; }
  .panel-heading { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; margin-bottom: 16px; color: var(--text-muted); }
  .panel-heading h3 { display: flex; align-items: center; gap: 8px; margin: 0; color: var(--text); font-size: 14px; line-height: 1.5; font-weight: 650; }
  .panel-heading p { display: flex; align-items: center; gap: 5px; margin: 5px 0 0; font-size: 10px; line-height: 1.5; }
  .legend-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
  .peer-label { display: inline-flex; min-width: 0; max-width: 52%; align-items: center; gap: 6px; padding: 5px 7px; border-radius: 5px; background: var(--surface-soft); font-size: 10px; }
  .peer-label span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .peer-label :global(svg) { flex-shrink: 0; }
  .chart-wrap { display: grid; grid-template-columns: 42px minmax(0, 1fr); gap: 6px; }
  .chart-scale { display: flex; flex-direction: column; justify-content: space-between; padding: 9px 0 10px; color: var(--text-muted); font-size: 9px; font-variant-numeric: tabular-nums; }
  .chart { position: relative; height: 150px; }
  .chart svg { display: block; width: 100%; height: 100%; overflow: visible; }
  .chart line { stroke: var(--border); vector-effect: non-scaling-stroke; }
  .chart line.target { stroke: var(--diagnostic-success); stroke-dasharray: 4 5; opacity: .55; }
  .chart path { fill: none; stroke: var(--accent); stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; vector-effect: non-scaling-stroke; }
  .chart circle { fill: var(--accent); }
  .target-label { position: absolute; right: 0; transform: translateY(calc(-100% - 4px)); padding: 1px 3px; color: var(--diagnostic-success); background: var(--surface-raised); font-size: 9px; pointer-events: none; }
  .chart-empty { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; color: var(--text-muted); background: color-mix(in srgb, var(--surface-raised) 85%, transparent); font-size: 11px; text-align: center; }
  .chart-axis { display: flex; justify-content: space-between; margin: 3px 0 0 48px; color: var(--text-muted); font-size: 9px; }
  .quality-stats { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; margin-top: 19px; padding-top: 14px; border-top: 1px solid var(--border); }
  .quality-stats > div { display: grid; gap: 5px; }
  .quality-stats small { color: var(--text-muted); font-size: 10px; }
  .quality-stats strong { font-size: 15px; font-weight: 600; font-variant-numeric: tabular-nums; }
  .quality-stats strong span { margin-left: 4px; color: var(--text-muted); font-size: 10px; font-weight: 450; }
  .path-card { display: flex; flex-direction: column; }
  .path-list { display: flex; flex: 1; flex-direction: column; justify-content: space-between; margin: 0; padding: 0; list-style: none; }
  .path-step { position: relative; display: grid; grid-template-columns: 36px minmax(0, 1fr) auto; align-items: center; column-gap: 11px; min-height: 68px; padding: 7px 0; }
  .path-step:not(:last-child)::after { position: absolute; top: calc(50% + 21px); bottom: calc(-50% + 21px); left: 17px; border-left: 1px dashed var(--border-strong); content: ""; }
  .path-icon { display: grid; place-items: center; width: 36px; height: 36px; border: 1px solid var(--border); border-radius: 10px; color: var(--accent); background: var(--surface-soft); }
  .path-step > div { display: grid; min-width: 0; gap: 4px; }
  .path-step strong { color: var(--text); font-size: 12px; line-height: 1.5; font-weight: 600; overflow-wrap: anywhere; }
  .path-step small { color: var(--text-muted); font-size: 10px; line-height: 1.5; overflow-wrap: anywhere; }
  em { display: inline-flex; align-items: center; gap: 5px; font-size: 10px; font-style: normal; white-space: nowrap; }
  .path-footer { display: flex; align-items: center; gap: 6px; margin-top: 14px; padding-top: 14px; border-top: 1px solid var(--border); color: var(--text-muted); font-size: 10px; }
  .count { display: inline-grid; place-items: center; min-width: 20px; height: 20px; padding: 0 4px; border-radius: 5px; color: var(--text-secondary); background: var(--surface-sunken); font-size: 10px; font-weight: 500; }
  .portal-row { display: grid; grid-template-columns: 30px minmax(0, 1fr) auto auto; align-items: center; gap: 10px; padding: 12px 0; border-top: 1px solid var(--border); }
  .portal-icon { display: grid; place-items: center; width: 30px; height: 32px; border-radius: 7px; color: var(--text-muted); background: var(--surface-soft); }
  .portal-name { display: grid; gap: 4px; font-size: 11px; line-height: 1.5; overflow-wrap: anywhere; }
  .portal-name > span:last-child { display: flex; align-items: center; gap: 5px; color: var(--text-secondary); }
  .portal-name :global(svg) { flex-shrink: 0; }
  .test-action { min-height: 28px; padding: 4px 7px; color: var(--accent-strong); background: var(--accent-soft); font-size: 10px; }
  .test-action:hover:enabled { border-color: var(--accent); }
  .text-action { min-height: 25px; padding: 2px 0 2px 6px; font-size: 10px; }
  .timeline-events { max-height: 330px; overflow: auto; }
  .timeline-row { display: grid; grid-template-columns: 6px 56px minmax(0, 1fr); align-items: baseline; gap: 9px; padding: 12px 0; border-top: 1px solid var(--border); font-size: 11px; line-height: 1.5; }
  .timeline-row > i { width: 5px; height: 5px; align-self: start; margin-top: 6px; border-radius: 50%; background: var(--border-strong); }
  .timeline-row > i.accent { background: var(--accent); }
  .timeline-row time { color: var(--text-muted); font-size: 10px; font-variant-numeric: tabular-nums; }
  .timeline-row > span { overflow-wrap: anywhere; }
  .empty-row { display: flex; min-height: 105px; flex-direction: column; justify-content: center; align-items: center; gap: 10px; color: var(--text-muted); font-size: 11px; line-height: 1.6; text-align: center; }
  .log-tools { border: 1px solid var(--border); border-radius: 9px; background: var(--surface-soft); }
  .log-tools summary { display: flex; align-items: center; justify-content: space-between; padding: 11px 14px; color: var(--text-secondary); font-size: 11px; list-style: none; cursor: pointer; }
  .log-tools summary::-webkit-details-marker { display: none; }
  .log-tools summary > span { display: flex; align-items: center; gap: 7px; }
  .log-tools[open] summary > :global(svg), :global(.expanded) { transform: rotate(180deg); }
  .log-tools-content { padding: 0 14px 14px; }
  .log-tools-content p { margin: 0 0 12px; color: var(--text-muted); font-size: 11px; line-height: 1.6; }
  .log-actions { display: flex; flex-wrap: wrap; gap: 8px; }
  .secondary-action { border-color: var(--border-strong); color: var(--text-secondary); background: var(--surface); font-size: 11px; }
  .secondary-action:hover, .secondary-action.logging { border-color: var(--accent); color: var(--accent-strong); }
  .logging-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
  .privacy-note { display: flex; align-items: flex-start; justify-content: center; gap: 6px; color: var(--text-muted); font-size: 10px; line-height: 1.6; text-align: center; }
  .privacy-note :global(svg) { flex: none; margin-top: 1px; }
  :global(.spinning) { animation: spin .8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @container (max-width: 760px) { .diagnostic-grid { grid-template-columns: minmax(0, 1fr); }.path-list { gap: 0; }.path-step { min-height: 64px; }.health-status { display: none; }.metric-grid article { padding-inline: 16px; }.metric-value { font-size: 24px; } }
  @container (max-width: 510px) { .metric-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 20px 0; }.metric-grid article:nth-child(2) { border-right: 0; }.health-heading { padding: 16px; align-items: flex-start; }.health-copy h2 { font-size: 17px; }.health-icon { width: 36px; height: 36px; border-radius: 10px; }.panel { padding: 16px; }.portal-row { gap: 7px; }.toolbar-actions { margin-left: auto; } }
  @media (prefers-reduced-motion: reduce) { button { transition: none; }:global(.spinning) { animation: none; } }
</style>
