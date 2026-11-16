<script lang="ts">
  import { Portal } from "bits-ui";
  export let actionsTarget: HTMLElement | undefined = undefined;
  import { t, locale, translate as uiTranslate, language as uiLanguage } from "../../i18n";
  import { diagnosticMessage } from "../../diagnosticMessages";
  import InputMenu from "./InputMenu.svelte";
  import AppSelect from "../../components/AppSelect.svelte";
  import { onMount } from "svelte";
  import {
    ArrowRight,
    ArrowsClockwise,
    Bug,
    CaretDown,
    Copy,
    DesktopTower,
    DownloadSimple,
    FolderOpen,
    Monitor,
    Pulse,
    ShieldCheck,
    WifiHigh,
  } from "phosphor-svelte";
  import { bridge } from "../../bridge";
  import type { RuntimeSnapshot } from "../../types";
  import type { LogStatus } from "../../types";
  import {
    activePortalCount,
    friendlyDeviceName,
    inputEventRate,
    percentile,
    portalEdgeTestRequest,
  } from "./inputSharingUi";

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
  $: rangedDiagnostics = snapshot.diagnostics.filter(
    (record) => record.timestampMs >= rangeStart && record.timestampMs <= now,
  );
  $: chart = latencyChart(rangedDiagnostics, rangeStart, now);
  $: latencies = chart.values;
  $: eventRate = inputEventRate(rangedDiagnostics);
  $: p50 = percentile(latencies, 0.5);
  $: p95 = percentile(latencies, 0.95);
  $: maximum = latencies.length ? Math.max(...latencies) : null;
  $: connected = snapshot.connectedPeers.length > 0;
  $: displays = snapshot.configuration.layout?.displays ?? {};
  $: portals = snapshot.configuration.layout?.portals ?? [];
  $: remoteId =
    snapshot.connectedPeers[0] ??
    Object.values(displays).find((display) => display.deviceId !== snapshot.serviceInstanceId)
      ?.deviceId;
  $: remotePeer = remoteId
    ? snapshot.nearbyPeers.find((peer) => peer.serviceInstanceId === remoteId)
    : null;
  $: remoteInjectionReady = Boolean(
    remotePeer?.capabilities?.canInjectAppPointer ||
    (remotePeer?.capabilities?.canInjectAbsolutePointer &&
      remotePeer?.capabilities?.canInjectKeyboard),
  );
  $: captureReady =
    snapshot.capabilities.canCapturePointer &&
    snapshot.capabilities.canCaptureKeyboard &&
    snapshot.capabilities.canSuppressLocalInput;
  $: health = diagnosticHealth({
    enabled: snapshot.configuration.inputSharingEnabled,
    connected,
    captureReady,
    injectionReady: remoteInjectionReady,
    activePortals: activePortalCount(snapshot),
    p95,
  });
  $: detailedLogging = Boolean(logStatus?.detailedUntilMs && logStatus.detailedUntilMs > now);
  $: visibleEvents = rangedDiagnostics.slice(showAllEvents ? -20 : -3).reverse();
  $: latencyLabel = chart.scope === "receive" ? "本机接收处理耗时" : "本机采集处理耗时";

  async function copyReport() {
    const report = [
      `Arc Input: ${health.title}`,
      `connected peers: ${snapshot.connectedPeers.length}`,
      `active portals: ${activePortalCount(snapshot)}`,
      `local ${chart.scope} sampled latency p95: ${p95?.toFixed(2) ?? "no samples"} ms`,
      ...rangedDiagnostics
        .slice(-20)
        .map(
          (event) =>
            `${new Date(event.timestampMs).toISOString()} [${event.category}] ${diagnosticMessage(event.message, $uiLanguage)}`,
        ),
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
    notify(uiTranslate("诊断报告已复制", $uiLanguage));
    window.setTimeout(() => (copied = false), 1600);
  }

  async function refreshDiagnostics() {
    refreshing = true;
    try {
      await onRefresh();
      now = Date.now();
      notify(uiTranslate("诊断数据已刷新", $uiLanguage));
    } catch (error) {
      notify(
        t("诊断刷新失败：{part0}", $uiLanguage, { part0: diagnosticMessage(error, $uiLanguage) }),
        true,
      );
    } finally {
      refreshing = false;
    }
  }

  async function testPortal(portalId: string) {
    const portal = portals.find((value) => value.portalId === portalId);
    const display = portal ? displays[portal.sourceDisplay] : null;
    if (!portal || !display) {
      notify(uiTranslate("找不到这条边缘对应的来源屏幕", $uiLanguage), true);
      return;
    }
    const request = portalEdgeTestRequest(portal, displays);
    if (!request) return notify(uiTranslate("找不到这条边缘对应的来源屏幕", $uiLanguage), true);
    testingPortalId = portalId;
    try {
      const result = await bridge.testEdge(request);
      notify(
        result.portalIds.includes(portalId)
          ? t("边缘测试成功，已路由到{part0}", $uiLanguage, {
              part0: displays[result.displayId]?.name ?? "目标屏幕",
            })
          : uiTranslate("测试移动未穿越该边缘，请检查方向、区段或激活策略", $uiLanguage),
        !result.portalIds.includes(portalId),
      );
    } catch (error) {
      notify(
        t("边缘测试失败：{part0}", $uiLanguage, { part0: diagnosticMessage(error, $uiLanguage) }),
        true,
      );
    } finally {
      testingPortalId = null;
    }
  }

  async function openLogs() {
    try {
      await bridge.openLogDirectory();
      notify(uiTranslate("已打开日志目录", $uiLanguage));
    } catch (error) {
      notify(
        t("打开日志目录失败：{part0}", $uiLanguage, {
          part0: diagnosticMessage(error, $uiLanguage),
        }),
        true,
      );
    }
  }

  async function exportLogs() {
    exportingLogs = true;
    try {
      const bundle = await bridge.exportDiagnosticBundle();
      notify(t("诊断包已导出：{part0}", $uiLanguage, { part0: bundle.fileName }));
      logStatus = await bridge.getLogStatus();
    } catch (error) {
      notify(
        t("导出诊断包失败：{part0}", $uiLanguage, { part0: diagnosticMessage(error, $uiLanguage) }),
        true,
      );
    } finally {
      exportingLogs = false;
    }
  }

  async function toggleDetailedLogging() {
    changingLogLevel = true;
    try {
      const active = Boolean(logStatus?.detailedUntilMs && logStatus.detailedUntilMs > Date.now());
      logStatus = await bridge.setDetailedLogging(!active);
      notify(
        active
          ? uiTranslate("详细日志已关闭", $uiLanguage)
          : uiTranslate("已开启 15 分钟详细日志", $uiLanguage),
      );
    } catch (error) {
      notify(
        t("切换详细日志失败：{part0}", $uiLanguage, {
          part0: diagnosticMessage(error, $uiLanguage),
        }),
        true,
      );
    } finally {
      changingLogLevel = false;
    }
  }

  onMount(() => {
    void bridge
      .getLogStatus()
      .then((status) => (logStatus = status))
      .catch(() => {});
    const timer = window.setInterval(() => (now = Date.now()), 5_000);
    return () => window.clearInterval(timer);
  });
</script>

<section class="page" aria-label={uiTranslate("连接诊断", $uiLanguage)}>
  <Portal to={actionsTarget} disabled={!actionsTarget}
    ><div class="diagnostic-toolbar">
      <button class="input-secondary" disabled={refreshing} on:click={refreshDiagnostics}
        ><ArrowsClockwise size={16} />{uiTranslate(
          refreshing ? "检测中" : "重新检测",
          $uiLanguage,
        )}</button
      ><button class="input-primary" disabled={exportingLogs} on:click={exportLogs}
        ><DownloadSimple size={16} />{uiTranslate(
          exportingLogs ? "导出中…" : "导出诊断包",
          $uiLanguage,
        )}</button
      ><InputMenu
        label={uiTranslate("诊断更多选项", $uiLanguage)}
        items={[
          {
            label: uiTranslate(copied ? "已复制" : "复制报告", $uiLanguage),
            icon: Copy,
            action: () => {
              void copyReport();
            },
          },
        ]}
      />
    </div></Portal
  >
  <section class="diagnostic-section">
    <header>
      <h2>{uiTranslate("连接检查", $uiLanguage)}</h2>
      <span data-tone={health.tone}>{uiTranslate(health.title, $uiLanguage)}</span>
    </header>
    {#if health.tone !== "success"}<p class="health-detail">
        {uiTranslate(health.detail, $uiLanguage)}
      </p>{/if}
    <div class="check-row">
      <Monitor size={18} /><strong>{uiTranslate("本机输入权限", $uiLanguage)}</strong><span
        >{uiTranslate(captureReady ? "可捕获鼠标与键盘" : "请检查输入捕获权限", $uiLanguage)}</span
      ><em data-tone={captureReady ? "success" : "warning"}
        >{uiTranslate(captureReady ? "正常" : "受限", $uiLanguage)}</em
      >
    </div>
    <div class="check-row">
      <DesktopTower size={18} /><strong>{uiTranslate("远程设备", $uiLanguage)}</strong><span
        >{remoteId
          ? friendlyDeviceName(remoteId, snapshot, $uiLanguage)
          : uiTranslate("目标设备", $uiLanguage)}{#if connected}<small>
            · {uiTranslate("加密连接", $uiLanguage)}</small
          >{/if}</span
      ><em data-tone={!connected ? "neutral" : remoteInjectionReady ? "success" : "warning"}
        >{uiTranslate(
          !connected ? "未连接" : remoteInjectionReady ? "可用" : "能力受限",
          $uiLanguage,
        )}</em
      >
    </div>
    <div class="edges">
      <h3>{uiTranslate("跨屏边缘", $uiLanguage)}</h3>
      {#each portals as portal}
        <div class="portal-row">
          <span class="portal-icon"><Monitor size={17} /></span>
          <span class="portal-name"
            ><span>{uiTranslate(displays[portal.sourceDisplay]?.name ?? "屏幕", $uiLanguage)}</span
            ><span
              ><ArrowRight size={12} />{uiTranslate(
                displays[portal.targetDisplay]?.name ?? "屏幕",
                $uiLanguage,
              )}</span
            ></span
          >
          <em data-tone={portal.status === "Active" ? "success" : "warning"}
            ><i></i>{uiTranslate(portal.status === "Active" ? "可用" : "已暂停", $uiLanguage)}</em
          >
          <button
            class="test-action"
            disabled={portal.status !== "Active" || testingPortalId === portal.portalId}
            on:click={() => testPortal(portal.portalId)}
            >{uiTranslate(
              testingPortalId === portal.portalId ? "测试中" : "测试",
              $uiLanguage,
            )}<ArrowRight size={13} /></button
          >
        </div>
      {/each}
      {#if portals.length === 0}<div class="empty-row">
          <Monitor size={24} />{uiTranslate("当前没有相邻且重叠的跨屏边段。", $uiLanguage)}
        </div>{/if}
    </div>
  </section>
  <section class="diagnostic-section">
    <h2>{uiTranslate("本机输入处理", $uiLanguage)}</h2>
    <div class="metric-grid">
      <article>
        <span>{uiTranslate(latencyLabel, $uiLanguage)} P95</span><strong
          data-tone={latencyTone(p95)}>{p95 == null ? "—" : `${p95.toFixed(1)} ms`}</strong
        >
      </article>
      <article>
        <span>{uiTranslate("处理速率（估算）", $uiLanguage)}</span><strong
          >{eventRate == null ? "—" : `${eventRate < 1 ? "< 1" : eventRate.toFixed(0)} Hz`}</strong
        >
      </article>
      <article>
        <span>{uiTranslate("丢包", $uiLanguage)}</span><strong
          >{uiTranslate("未采样", $uiLanguage)}</strong
        >
      </article>
    </div>
    <p class="measurement-note">{uiTranslate("本机耗时，不含网络传输", $uiLanguage)}</p>
    <details class="performance-details">
      <summary>{uiTranslate("性能详情", $uiLanguage)}<CaretDown size={15} /></summary>
      <div class="chart-wrap">
        <div class="chart-scale" aria-hidden="true">
          <span>{chart.maximum} ms</span><span>0</span>
        </div>
        <div class="chart">
          <svg
            viewBox="0 0 520 150"
            preserveAspectRatio="none"
            role="img"
            aria-label={uiTranslate(latencyLabel, $uiLanguage)}
          >
            <line x1="0" y1="12" x2="520" y2="12" /><line x1="0" y1="74" x2="520" y2="74" /><line
              x1="0"
              y1="136"
              x2="520"
              y2="136"
            />
            <line x1="0" y1={chart.targetY} x2="520" y2={chart.targetY} class="target" />
            {#if chart.path}<path d={chart.path} />{/if}
            {#if chart.points.length === 1}<circle
                cx={chart.points[0].x}
                cy={chart.points[0].y}
                r="3"
              />{/if}
          </svg>
          <span class="target-label" style:top={`${(chart.targetY / 150) * 100}%`}
            >{uiTranslate("本机参考线 16 ms", $uiLanguage)}</span
          >
          {#if !chart.path}<div class="chart-empty">
              <Pulse size={24} /><span>{uiTranslate("发生跨屏输入后显示延迟", $uiLanguage)}</span>
            </div>{/if}
        </div>
      </div>
      <div class="chart-axis">
        <span
          >{uiTranslate(
            timeRange === "15" ? "15 分钟前" : timeRange === "60" ? "1 小时前" : "24 小时前",
            $uiLanguage,
          )}</span
        ><span>{uiTranslate("现在", $uiLanguage)}</span>
      </div>
      <div class="quality-stats">
        <div>
          <small>{uiTranslate("p50（中位）", $uiLanguage)}</small><strong
            data-tone={latencyTone(p50)}
            >{p50 == null ? "—" : p50.toFixed(1)}{#if p50 != null}<span>ms</span>{/if}</strong
          >
        </div>
        <div>
          <small>{uiTranslate("样本 P95", $uiLanguage)}</small><strong data-tone={latencyTone(p95)}
            >{p95 == null ? "—" : p95.toFixed(1)}{#if p95 != null}<span>ms</span>{/if}</strong
          >
        </div>
        <div>
          <small>{uiTranslate("最大", $uiLanguage)}</small><strong data-tone={latencyTone(maximum)}
            >{maximum == null ? "—" : maximum.toFixed(1)}{#if maximum != null}<span>ms</span
              >{/if}</strong
          >
        </div>
      </div>
    </details>
  </section>
  <section class="diagnostic-section">
    <h2>{uiTranslate("日志与工具", $uiLanguage)}</h2>
    <div class="logging-row">
      <span
        >{uiTranslate("详细日志", $uiLanguage)}<small
          >{uiTranslate("开启后记录 15 分钟", $uiLanguage)}</small
        ></span
      ><button
        class="logging-switch"
        role="switch"
        aria-label={uiTranslate("详细日志", $uiLanguage)}
        aria-checked={detailedLogging}
        disabled={changingLogLevel}
        class:enabled={detailedLogging}
        on:click={toggleDetailedLogging}><i></i></button
      ><button class="input-secondary" on:click={openLogs}
        ><FolderOpen size={16} />{uiTranslate("日志目录", $uiLanguage)}</button
      >
    </div>
    <header class="events-heading">
      <h3>{uiTranslate("最近事件", $uiLanguage)}</h3>
      <AppSelect
        bind:value={timeRange}
        aria-label={uiTranslate("诊断时间范围", $uiLanguage)}
        options={[
          { value: "15", label: uiTranslate("最近 15 分钟", $uiLanguage) },
          { value: "60", label: uiTranslate("最近 1 小时", $uiLanguage) },
          { value: "1440", label: uiTranslate("最近 24 小时", $uiLanguage) },
        ]}
      /><button class="input-secondary" on:click={copyReport}
        ><Copy size={15} />{uiTranslate(copied ? "已复制" : "复制报告", $uiLanguage)}</button
      >
    </header>
    <div class="event-list">
      {#each visibleEvents as record}<div>
          <time
            >{new Date(record.timestampMs).toLocaleTimeString($locale, {
              hour: "2-digit",
              minute: "2-digit",
              second: "2-digit",
            })}</time
          ><code>{record.category}</code><span
            >{diagnosticMessage(record.message, $uiLanguage)}</span
          >
        </div>{/each}{#if !visibleEvents.length}<p>
          {uiTranslate("所选时间范围内暂无事件", $uiLanguage)}
        </p>{/if}
    </div>
    {#if rangedDiagnostics.length > 3}<button
        class="input-text-button"
        aria-expanded={showAllEvents}
        on:click={() => (showAllEvents = !showAllEvents)}
        >{uiTranslate(showAllEvents ? "收起" : "更多", $uiLanguage)}</button
      >{/if}
  </section>
  <p class="privacy-note">
    <ShieldCheck size={14} />{uiTranslate(
      logStatus?.enabled
        ? t("日志已写盘 · 保留 {part0} 天", $uiLanguage, { part0: logStatus.retentionDays })
        : "磁盘日志暂不可用",
      $uiLanguage,
    )}{uiTranslate("· 不记录具体按键、文字或剪贴板内容", $uiLanguage)}
  </p>
</section>

<style>
  .page {
    --diagnostic-success: color-mix(in srgb, var(--success) 76%, var(--text));
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 16px;
    padding: 2px 2px 24px;
    container-type: inline-size;
  }
  .diagnostic-toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .diagnostic-toolbar {
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 10px;
  }
  button,
  summary {
    -webkit-tap-highlight-color: transparent;
  }
  button {
    display: inline-flex;
    min-height: 32px;
    align-items: center;
    justify-content: center;
    gap: 6px;
    border: 1px solid transparent;
    border-radius: 7px;
    padding: 6px 11px;
    font-family: inherit;
    font-size: 12px;
    line-height: 1.4;
    font-weight: 550;
    white-space: nowrap;
    transition:
      background 0.15s,
      border-color 0.15s;
  }
  button:disabled {
    opacity: 0.5;
  }
  button:focus-visible,
  summary:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 3px;
  }
  em i {
    width: 5px;
    height: 5px;
    flex: none;
    border-radius: 50%;
    background: currentColor;
  }
  .metric-grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    padding: 0;
    border-top: 1px solid var(--border);
  }
  .metric-grid article {
    display: flex;
    min-width: 0;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    padding: 0 22px;
    border-right: 1px solid var(--border);
  }
  .metric-grid article:last-child {
    border-right: 0;
  }
  [data-tone="neutral"]:is(strong, em) {
    color: var(--text-muted);
  }
  [data-tone="warning"]:is(strong, em) {
    color: var(--warning);
  }
  [data-tone="success"]:is(strong, em) {
    color: var(--diagnostic-success);
  }
  .chart-wrap {
    display: grid;
    grid-template-columns: 42px minmax(0, 1fr);
    gap: 6px;
  }
  .chart-scale {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    padding: 9px 0 10px;
    color: var(--text-muted);
    font-size: 9px;
    font-variant-numeric: tabular-nums;
  }
  .chart {
    position: relative;
    height: 150px;
  }
  .chart svg {
    display: block;
    width: 100%;
    height: 100%;
    overflow: visible;
  }
  .chart line {
    stroke: var(--border);
    vector-effect: non-scaling-stroke;
  }
  .chart line.target {
    stroke: var(--diagnostic-success);
    stroke-dasharray: 4 5;
    opacity: 0.55;
  }
  .chart path {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }
  .chart circle {
    fill: var(--accent);
  }
  .target-label {
    position: absolute;
    right: 0;
    transform: translateY(calc(-100% - 4px));
    padding: 1px 3px;
    color: var(--diagnostic-success);
    background: var(--surface-raised);
    font-size: 9px;
    pointer-events: none;
  }
  .chart-empty {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--text-muted);
    background: color-mix(in srgb, var(--surface-raised) 85%, transparent);
    font-size: 11px;
    text-align: center;
  }
  .chart-axis {
    display: flex;
    justify-content: space-between;
    margin: 3px 0 0 48px;
    color: var(--text-muted);
    font-size: 9px;
  }
  .quality-stats {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 12px;
    margin-top: 19px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
  }
  .quality-stats > div {
    display: grid;
    gap: 5px;
  }
  .quality-stats small {
    color: var(--text-muted);
    font-size: 10px;
  }
  .quality-stats strong {
    font-size: 15px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .quality-stats strong span {
    margin-left: 4px;
    color: var(--text-muted);
    font-size: 10px;
    font-weight: 450;
  }
  em {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 10px;
    font-style: normal;
    white-space: nowrap;
  }
  .portal-row {
    display: grid;
    grid-template-columns: 30px minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 10px;
    padding: 6px 0;
    border-top: 1px solid var(--border);
  }
  .portal-icon {
    display: grid;
    place-items: center;
    width: 30px;
    height: 32px;
    border-radius: 7px;
    color: var(--text-muted);
    background: var(--surface-soft);
  }
  .portal-name {
    display: grid;
    gap: 4px;
    font-size: 11px;
    line-height: 1.5;
    overflow-wrap: anywhere;
  }
  .portal-name > span:last-child {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--text-secondary);
  }
  .portal-name :global(svg) {
    flex-shrink: 0;
  }
  .test-action {
    min-height: 28px;
    padding: 4px 7px;
    color: var(--accent-strong);
    background: var(--accent-soft);
    font-size: 10px;
  }
  .test-action:hover:enabled {
    border-color: var(--accent);
  }
  .empty-row {
    display: flex;
    min-height: 105px;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    gap: 10px;
    color: var(--text-muted);
    font-size: 11px;
    line-height: 1.6;
    text-align: center;
  }
  :global(.expanded) {
    transform: rotate(180deg);
  }
  .privacy-note {
    display: flex;
    align-items: flex-start;
    justify-content: center;
    gap: 6px;
    color: var(--text-muted);
    font-size: 10px;
    line-height: 1.6;
    text-align: center;
  }
  .privacy-note :global(svg) {
    flex: none;
    margin-top: 1px;
  }
  :global(.spinning) {
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @container (max-width: 760px) {
    .metric-grid article {
      padding-inline: 16px;
    }
  }
  @container (max-width: 510px) {
    .metric-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 20px 0;
    }
    .metric-grid article:nth-child(2) {
      border-right: 0;
    }
    .portal-row {
      gap: 7px;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    button {
      transition: none;
    }
    :global(.spinning) {
      animation: none;
    }
  }

  .page {
    display: block;
    width: min(100%, 1080px);
    padding-bottom: 24px;
  }
  .diagnostic-toolbar {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 8px;
    margin: 0;
  }
  .diagnostic-section {
    padding: 16px 0;
    border-bottom: 1px solid var(--border);
  }
  .diagnostic-section:first-of-type {
    padding-top: 0;
  }
  .diagnostic-section header {
    display: flex;
    align-items: center;
    gap: 15px;
    margin-bottom: 12px;
  }
  .diagnostic-section h2 {
    margin: 0 0 12px;
    font-size: 16px;
  }
  .diagnostic-section header h2 {
    margin: 0;
  }
  .diagnostic-section h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
  }
  .diagnostic-section header > span {
    font-size: 12px;
  }
  .health-detail {
    margin: 8px 0 12px;
    color: var(--text-secondary);
    font-size: 12px;
  }
  .check-row {
    display: grid;
    grid-template-columns: 22px 160px minmax(0, 1fr) 80px;
    align-items: center;
    gap: 10px;
    min-height: 47px;
    border-top: 1px solid var(--border);
    font-size: 13px;
  }
  .check-row strong {
    font-weight: 500;
  }
  .check-row > span {
    overflow-wrap: anywhere;
    color: var(--text-secondary);
  }
  .check-row em {
    font-style: normal;
    font-size: 12px;
    text-align: right;
  }
  .edges {
    margin-top: 12px;
  }
  .edges h3 {
    margin-bottom: 4px;
    color: var(--text-secondary);
  }
  .portal-row {
    min-height: 46px;
    padding-inline: 0;
  }
  .metric-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    margin-top: 18px;
    border: 0;
  }
  .metric-grid > article {
    display: grid;
    gap: 10px;
    padding: 0 20px;
    border: 0;
    border-left: 1px solid var(--border);
    background: none;
  }
  .metric-grid > article:first-child {
    padding-left: 0;
    border-left: 0;
  }
  .metric-grid article > span {
    font-size: 12px;
    color: var(--text-secondary);
  }
  .metric-grid strong {
    font-size: 21px;
    font-weight: 650;
  }
  .measurement-note {
    color: var(--text-muted);
    font-size: 11px;
    margin: 15px 0 4px;
  }
  .performance-details summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-top: 8px;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
    list-style: none;
  }
  .logging-row {
    display: flex;
    align-items: center;
    gap: 14px;
    min-height: 50px;
  }
  .logging-row > span {
    display: flex;
    align-items: center;
    gap: 14px;
    flex: 1;
    font-size: 13px;
  }
  .logging-row small {
    color: var(--text-muted);
    font-size: 12px;
  }
  .logging-switch {
    width: 40px;
    height: 23px;
    border: 0;
    padding: 3px;
    border-radius: 99px;
    background: var(--border-strong);
  }
  .logging-switch i {
    display: block;
    width: 17px;
    height: 17px;
    border-radius: 50%;
    background: white;
  }
  .logging-switch.enabled {
    background: var(--accent);
  }
  .logging-switch.enabled i {
    transform: translateX(17px);
  }
  .events-heading {
    margin-top: 16px;
  }
  .events-heading h3 {
    flex: 1;
  }
  .event-list {
    overflow: auto;
    max-height: 280px;
    border: 1px solid var(--border);
    border-radius: 8px;
    font-size: 12px;
  }
  .event-list > div {
    display: grid;
    grid-template-columns: 75px 110px minmax(0, 1fr);
    align-items: start;
    gap: 10px;
    padding: 9px 12px;
    border-bottom: 1px solid var(--border);
  }
  .event-list > div:last-child {
    border-bottom: 0;
  }
  .event-list time {
    color: var(--text-muted);
  }
  .event-list code {
    overflow-wrap: anywhere;
    font-size: 11px;
  }
  .event-list span {
    overflow-wrap: anywhere;
  }
  .event-list > p {
    padding: 12px;
    color: var(--text-muted);
  }
  .privacy-note {
    font-size: 11px;
    margin-top: 15px;
  }
  @media (max-width: 980px) {
    .diagnostic-toolbar {
      margin-top: 0;
    }
    .check-row {
      grid-template-columns: 20px 110px minmax(0, 1fr) 60px;
    }
    .logging-row > span {
      flex-wrap: wrap;
      gap: 4px;
    }
  }
</style>
