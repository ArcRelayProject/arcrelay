<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "./i18n";
  import { onMount } from "svelte";
  import { ArrowClockwise, ArrowDownLeft, ArrowUpRight, CheckCircle, ListChecks, Printer, ShareNetwork, WifiHigh, X } from "phosphor-svelte";

  import { SubscriptionScope } from "./subscriptions";
  import { bridge } from "./bridge";
  import type { LocalPrinter, PrintJobActivitySnapshot, PrintJobState, PrinterShare, PrinterSharingSnapshot } from "./types";

  export let notify: (message: string, kind?: "success" | "error") => void;

  let snapshot: PrinterSharingSnapshot = { hostingSupported: true, localPrinters: [], shares: [], remotePrinters: [], queueBindings: [] };
  let loading = true;
  let busyPrinterId = "";
  let jobsOpen = false;
  let jobsLoading = false;
  let jobTab: "received" | "sent" = "received";
  let jobActivity: PrintJobActivitySnapshot = { revision: 0, received: [], sent: [] };

  $: activeJobCount = [
    ...jobActivity.received.map((job) => job.state),
    ...jobActivity.sent.map((job) => job.state),
  ].filter((state) => state && !["completed", "cancelled", "failed"].includes(state)).length;

  const activeShare = (printer: LocalPrinter): PrinterShare | undefined =>
    snapshot.shares.find(
      (share) => share.localPrinterId === printer.id && share.state === "published",
    );

  const queueBinding = (sourceDeviceId: string, shareId: string) =>
    snapshot.queueBindings.find(
      (binding) => binding.remoteDeviceId === sourceDeviceId && binding.printerShareId === shareId,
    );

  async function toggleRemotePrinter(sourceDeviceId: string, shareId: string) {
    busyPrinterId = `${sourceDeviceId}:${shareId}`;
    try {
      const binding = queueBinding(sourceDeviceId, shareId);
      snapshot = binding
        ? await bridge.removeRemotePrinter(binding.id)
        : await bridge.installRemotePrinter(sourceDeviceId, shareId);
      const updated = queueBinding(sourceDeviceId, shareId);
      if (updated?.state === "failed") {
        notify(updated.failureMessage ?? "系统打印机安装失败", "error");
      } else {
        notify(binding ? "已从系统移除打印机" : "打印机已安装到系统");
      }
    } catch (error) {
      notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      busyPrinterId = "";
    }
  }

  async function refresh() {
    loading = true;
    try {
      snapshot = await bridge.refreshPrinterSharingState();
    } catch (error) {
      notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      loading = false;
    }
  }

  async function loadInitialState() {
    loading = true;
    try {
      // Local inventory and cached LAN state should render without waiting
      // for every discovered peer to answer a network refresh.
      snapshot = await bridge.getPrinterSharingState();
    } catch (error) {
      notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      loading = false;
    }
    void bridge.refreshPrinterSharingState()
      .then((next) => { snapshot = next; })
      .catch((error) => notify(error instanceof Error ? error.message : String(error), "error"));
  }

  async function toggleShare(printer: LocalPrinter) {
    busyPrinterId = printer.id;
    try {
      const share = activeShare(printer);
      snapshot = share
        ? await bridge.suspendPrinterShare(share.id)
        : await bridge.publishPrinter(printer.id);
      notify(share ? "已停止共享打印机" : "打印机已在局域网公开共享");
    } catch (error) {
      notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      busyPrinterId = "";
    }
  }

  async function loadJobs(showLoading = true) {
    if (showLoading) jobsLoading = true;
    try {
      applyJobs(await bridge.getPrintJobActivity());
    } catch (error) {
      notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      jobsLoading = false;
    }
  }

  async function openJobs() {
    jobsOpen = true;
    await loadJobs();
    if (jobActivity.received.length === 0 && jobActivity.sent.length > 0) jobTab = "sent";
  }

  const jobStateLabel = (state: PrintJobState | null, sourceAvailable = true) => {
    if (!state) return sourceAvailable ? "等待状态" : "共享设备离线";
    return {
      offered: "等待接收",
      receiving: "正在接收",
      validating: "正在校验",
      ready: "准备打印",
      submitting: "正在提交到系统打印队列",
      queued: "已进入队列",
      printing: "正在打印",
      completed: "打印完成",
      held: "已暂停",
      cancelled: "已取消",
      failed: "打印失败",
      ambiguous: "提交结果待确认",
    }[state];
  };

  const jobProgress = (state: PrintJobState | null) => ({
    offered: 8,
    receiving: 24,
    validating: 38,
    ready: 50,
    submitting: 58,
    queued: 64,
    printing: 84,
    completed: 100,
    held: 64,
    cancelled: 100,
    failed: 100,
    ambiguous: 58,
  }[state ?? "offered"]);

  const jobTone = (state: PrintJobState | null) =>
    state === "completed" ? "success" : state === "failed" || state === "cancelled" ? "danger" : state === "held" || state === "ambiguous" ? "warning" : "active";

  const formatBytes = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  };

  const formatTime = (timestamp: number) =>
    timestamp > 0
      ? new Intl.DateTimeFormat("zh-CN", { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(timestamp)
      : "时间未知";

  function applyJobs(next: PrintJobActivitySnapshot) {
    if (next.revision >= jobActivity.revision) jobActivity = next;
  }
  onMount(() => {
    const scope = new SubscriptionScope();
    void scope.add(bridge.onPrintJobActivityError((message) => { if (!scope.disposed) notify(message, "error"); })).catch(console.error);
    void scope.add(bridge.onPrintJobActivity((next) => { if (!scope.disposed) applyJobs(next); }))
      .then(() => { if (!scope.disposed) return scope.add(bridge.observePrintJobs()); })
      .catch((error) => notify(String(error), "error"));
    void loadInitialState();
    return scope.dispose;
  });
</script>

<svelte:window on:keydown={(event) => event.key === "Escape" && (jobsOpen = false)} />

<main class="printer-page">
  <header class="page-heading">
    <div>
      <h1>{uiTranslate("打印机共享", $uiLanguage)}</h1>
      <p>{uiTranslate("将本机打印机公开给同一局域网中的 ArcRelay 设备，无需逐台授权。", $uiLanguage)}</p>
    </div>
    <div class="heading-actions">
      <button class="jobs-button" on:click={openJobs}>
        <ListChecks size={18} /> {uiTranslate("打印任务", $uiLanguage)}
        {#if activeJobCount > 0}<span class="active-count">{activeJobCount}</span>{/if}
      </button>
      <button class="refresh-button" on:click={refresh} disabled={loading}>
        <span class:spinning={loading}><ArrowClockwise size={17} /></span> {uiTranslate("刷新", $uiLanguage)}
      </button>
    </div>
  </header>

  {#if !snapshot.hostingSupported}
    <section class="unsupported-notice">
      {uiTranslate("当前平台暂不支持共享本机打印机；你仍可发现并安装其他设备共享的打印机。", $uiLanguage)}
    </section>
  {/if}

  <section class="printer-section">
    <div class="section-heading">
      <div><h2>{uiTranslate("这台电脑的打印机", $uiLanguage)}</h2><span>{snapshot.localPrinters.length} {uiTranslate("台", $uiLanguage)}</span></div>
      <small>{uiTranslate("ArcRelay 使用系统已安装的打印驱动", $uiLanguage)}</small>
    </div>

    {#if loading && snapshot.localPrinters.length === 0}
      <div class="empty-state"><span class="loading-dot"></span>{uiTranslate("正在读取系统打印机…", $uiLanguage)}</div>
    {:else if snapshot.localPrinters.length === 0}
      <div class="empty-state">
        <Printer size={30} />
        <strong>{uiTranslate("没有找到系统打印机", $uiLanguage)}</strong>
        <span>{uiTranslate("请先在系统设置中添加打印机，然后刷新此页面。", $uiLanguage)}</span>
      </div>
    {:else}
      <div class="printer-list">
        {#each snapshot.localPrinters as printer (printer.id)}
          {@const share = activeShare(printer)}
          <div class:shared={Boolean(share)} class="printer-row">
            <span class="printer-icon"><Printer size={24} weight="duotone" /></span>
            <span class="printer-copy">
              <strong>{printer.displayName}</strong>
              <small>{uiTranslate(printer.isDefault ? "默认打印机 · " : "", $uiLanguage)}{uiTranslate(printer.status === "ready" ? "就绪" : printer.status === "busy" ? "正在打印" : printer.status === "offline" ? "离线" : "状态未知", $uiLanguage)}</small>
            </span>
            {#if share}
              <span class="shared-state"><CheckCircle size={16} weight="fill" /> {uiTranslate("局域网共享中", $uiLanguage)}</span>
            {/if}
            <button
              class:danger={Boolean(share)}
              class="share-button"
              disabled={!snapshot.hostingSupported || busyPrinterId === printer.id}
              on:click={() => toggleShare(printer)}
            >
              <ShareNetwork size={17} />
              {uiTranslate(busyPrinterId === printer.id ? "处理中…" : share ? "停止共享" : "共享", $uiLanguage)}
            </button>
          </div>
        {/each}
      </div>
    {/if}
  </section>

  <section class="printer-section remote-section">
    <div class="section-heading">
      <div><h2>{uiTranslate("局域网打印机", $uiLanguage)}</h2><span>{snapshot.remotePrinters.length} {uiTranslate("台", $uiLanguage)}</span></div>
      <small>{uiTranslate("发现后可一键注册到系统打印机", $uiLanguage)}</small>
    </div>
    {#if snapshot.remotePrinters.length === 0}
      <div class="empty-state compact">
        <ShareNetwork size={27} />
        <strong>{uiTranslate("正在等待其他 ArcRelay 设备共享打印机", $uiLanguage)}</strong>
        <span>{uiTranslate("请确认设备处于同一局域网，并已在 ArcRelay 中开启打印机共享。", $uiLanguage)}</span>
      </div>
    {:else}
      <div class="printer-list">
        {#each snapshot.remotePrinters as remote (`${remote.printer.sourceDeviceId}:${remote.printer.shareId}`)}
          {@const binding = queueBinding(remote.printer.sourceDeviceId, remote.printer.shareId)}
          {@const remoteKey = `${remote.printer.sourceDeviceId}:${remote.printer.shareId}`}
          <div class="printer-row">
            <span class="printer-icon"><Printer size={24} weight="duotone" /></span>
            <span class="printer-copy">
              <strong>{remote.printer.displayName}</strong>
              <small>{remote.printer.sourceDeviceName} · {uiTranslate(remote.printer.status === "ready" ? "就绪" : remote.printer.status === "busy" ? "正在打印" : "暂不可用", $uiLanguage)}</small>
            </span>
            <span class="shared-state"><WifiHigh size={16} /> {uiTranslate(binding?.state === "ready" ? "已安装" : binding?.state === "failed" ? "安装失败" : "已发现", $uiLanguage)}</span>
            <button
              class:danger={Boolean(binding)}
              class="share-button"
              disabled={busyPrinterId === remoteKey}
              title={binding?.failureMessage ?? ""}
              on:click={() => toggleRemotePrinter(remote.printer.sourceDeviceId, remote.printer.shareId)}
            >{uiTranslate(busyPrinterId === remoteKey ? "处理中…" : binding ? "移除" : "安装到系统", $uiLanguage)}</button>
          </div>
        {/each}
      </div>
    {/if}
  </section>
</main>

{#if jobsOpen}
  <div class="jobs-overlay">
    <button class="jobs-backdrop" aria-label={uiTranslate("关闭打印任务", $uiLanguage)} on:click={() => (jobsOpen = false)}></button>
    <dialog open class="jobs-panel" aria-label={uiTranslate("打印任务", $uiLanguage)}>
      <header class="jobs-header">
        <div>
          <h2>{uiTranslate("打印任务", $uiLanguage)}</h2>
          <p>{uiTranslate("查看共享打印机收到的任务，以及发送到其他设备的打印进度。", $uiLanguage)}</p>
        </div>
        <div class="jobs-header-actions">
          <button class="icon-button" aria-label={uiTranslate("刷新打印任务", $uiLanguage)} title={uiTranslate("刷新", $uiLanguage)} on:click={() => loadJobs()} disabled={jobsLoading}>
            <span class:spinning={jobsLoading}><ArrowClockwise size={18} /></span>
          </button>
          <button class="icon-button" aria-label={uiTranslate("关闭打印任务", $uiLanguage)} on:click={() => (jobsOpen = false)}><X size={19} /></button>
        </div>
      </header>

      <div class="job-tabs" role="tablist" aria-label={uiTranslate("任务方向", $uiLanguage)}>
        <button class:active={jobTab === "received"} role="tab" aria-selected={jobTab === "received"} on:click={() => (jobTab = "received")}>
          <ArrowDownLeft size={17} weight="bold" /> {uiTranslate("收到的任务", $uiLanguage)} <span>{jobActivity.received.length}</span>
        </button>
        <button class:active={jobTab === "sent"} role="tab" aria-selected={jobTab === "sent"} on:click={() => (jobTab = "sent")}>
          <ArrowUpRight size={17} weight="bold" /> {uiTranslate("发出的任务", $uiLanguage)} <span>{jobActivity.sent.length}</span>
        </button>
      </div>

      <div class="jobs-content">
        {#if jobsLoading && jobActivity.received.length === 0 && jobActivity.sent.length === 0}
          <div class="jobs-empty"><span class="loading-dot"></span>{uiTranslate("正在读取打印任务…", $uiLanguage)}</div>
        {:else if jobTab === "received"}
          {#if jobActivity.received.length === 0}
            <div class="jobs-empty"><span class="jobs-empty-icon"><ArrowDownLeft size={28} weight="duotone" /></span><strong>{uiTranslate("还没有收到打印任务", $uiLanguage)}</strong><span>{uiTranslate("其他设备使用本机共享打印机后，任务会显示在这里。", $uiLanguage)}</span></div>
          {:else}
            <div class="job-list">
              {#each jobActivity.received as job (job.id)}
                <article class="job-card">
                  <div class="job-main">
                    <span class="printer-icon small"><Printer size={20} weight="duotone" /></span>
                    <div class="job-copy">
                      <strong>{job.documentName}</strong>
                      <span>{job.printerName} {uiTranslate("· 来自", $uiLanguage)} {job.sourceDeviceId}</span>
                      <small>{formatBytes(job.documentSizeBytes)} · {formatTime(job.createdAtMs)}</small>
                    </div>
                    <span class="job-state {jobTone(job.state)}">{uiTranslate(jobStateLabel(job.state), $uiLanguage)}</span>
                  </div>
                  <div class="progress-track"><span class="{jobTone(job.state)}" style={`width: ${jobProgress(job.state)}%`}></span></div>
                  {#if job.failureMessage}<p class="job-error">{job.failureMessage}</p>{/if}
                </article>
              {/each}
            </div>
          {/if}
        {:else if jobActivity.sent.length === 0}
          <div class="jobs-empty"><span class="jobs-empty-icon"><ArrowUpRight size={28} weight="duotone" /></span><strong>{uiTranslate("还没有发出打印任务", $uiLanguage)}</strong><span>{uiTranslate("通过已安装的局域网打印机打印后，任务会显示在这里。", $uiLanguage)}</span></div>
        {:else}
          <div class="job-list">
            {#each jobActivity.sent as job (job.localJobId)}
              <article class="job-card">
                <div class="job-main">
                  <span class="printer-icon small"><Printer size={20} weight="duotone" /></span>
                  <div class="job-copy">
                    <strong>{job.documentName}</strong>
                    <span>{job.printerName} · {job.remoteDeviceName}</span>
                    <small>{formatTime(job.createdAtMs)}{uiTranslate(job.sourceAvailable ? "" : " · 共享设备当前离线", $uiLanguage)}</small>
                  </div>
                  <span class="job-state {jobTone(job.state)}">{uiTranslate(jobStateLabel(job.state, job.sourceAvailable), $uiLanguage)}</span>
                </div>
                <div class="progress-track"><span class="{jobTone(job.state)}" style={`width: ${jobProgress(job.state)}%`}></span></div>
                {#if job.failureMessage}<p class="job-error">{job.failureMessage}</p>{/if}
              </article>
            {/each}
          </div>
        {/if}
      </div>
    </dialog>
  </div>
{/if}

<style>
  .printer-page { flex: 1; min-width: 0; overflow: auto; padding: 42px clamp(28px, 4vw, 64px) 64px; background: var(--surface); color: var(--text); }
  .page-heading, .section-heading, .printer-row { display: flex; align-items: center; }
  .page-heading { justify-content: space-between; gap: 24px; margin-bottom: 26px; }
  .heading-actions, .jobs-header-actions { display: flex; align-items: center; gap: 9px; }
  h1 { margin: 0 0 7px; color: var(--text); font-size: 31px; letter-spacing: -.04em; }
  h2 { margin: 0; color: var(--text); font-size: 18px; }
  p, small, .empty-state span { color: var(--text-secondary); }
  p { margin: 0; font-size: 14px; }
  button { font: inherit; }
  .refresh-button, .jobs-button, .share-button { display: inline-flex; align-items: center; justify-content: center; gap: 7px; border: 1px solid var(--border-strong); border-radius: var(--control-radius); background: var(--control-bg); color: var(--text); padding: 9px 14px; cursor: pointer; transition: border-color .16s ease, background .16s ease, color .16s ease, transform .16s ease; }
  .refresh-button:hover, .share-button:hover { border-color: color-mix(in srgb, var(--accent) 35%, var(--border-strong)); background: var(--control-hover); }
  .jobs-button { border-color: color-mix(in srgb, var(--accent) 42%, var(--border)); background: var(--accent-soft); color: var(--accent-strong); font-weight: 650; }
  .jobs-button:hover { border-color: color-mix(in srgb, var(--accent) 68%, var(--border)); background: color-mix(in srgb, var(--accent) 14%, var(--surface-raised)); }
  .active-count { display: grid; min-width: 18px; height: 18px; place-items: center; padding: 0 4px; border-radius: 999px; background: var(--accent); color: var(--text-inverse); font-size: 10px; }
  button:focus-visible { outline: 3px solid var(--focus-ring); outline-offset: 2px; }
  button:active:not(:disabled) { transform: translateY(1px); }
  button:disabled { opacity: .52; cursor: default; }
  .unsupported-notice { padding: 13px 16px; border: 1px solid color-mix(in srgb, var(--warning) 32%, var(--border)); border-radius: 12px; background: var(--warning-soft); color: var(--warning); font-size: 13px; }
  .printer-section { margin-top: 28px; }
  .section-heading { justify-content: space-between; margin-bottom: 12px; }
  .section-heading > div { display: flex; align-items: center; gap: 9px; }
  .section-heading > div span { padding: 2px 8px; border: 1px solid color-mix(in srgb, var(--accent) 20%, transparent); border-radius: 999px; background: var(--accent-soft); color: var(--accent-strong); font-size: 12px; }
  .section-heading small { font-size: 12px; }
  .printer-list { overflow: hidden; border: 1px solid var(--border); border-radius: var(--card-radius); background: var(--surface-raised); box-shadow: var(--shadow-card); }
  .printer-row { min-height: 78px; gap: 14px; padding: 0 18px; border-bottom: 1px solid var(--border); transition: background .16s ease; }
  .printer-row:last-child { border-bottom: 0; }
  .printer-row:hover { background: var(--surface-hover); }
  .printer-row.shared { background: color-mix(in srgb, var(--accent) 4%, var(--surface-raised)); }
  .printer-row.shared:hover { background: color-mix(in srgb, var(--accent) 7%, var(--surface-raised)); }
  .printer-icon { display: grid; place-items: center; width: 42px; height: 42px; flex: 0 0 auto; border: 1px solid color-mix(in srgb, var(--accent) 16%, transparent); border-radius: 11px; background: var(--accent-soft); color: var(--accent-strong); }
  .printer-copy { display: grid; gap: 4px; flex: 1; min-width: 0; }
  .printer-copy strong { overflow: hidden; color: var(--text); text-overflow: ellipsis; white-space: nowrap; }
  .printer-copy small { font-size: 12px; }
  .shared-state { display: inline-flex; align-items: center; gap: 5px; padding: 5px 9px; border-radius: 999px; background: var(--success-soft); color: var(--success); font-size: 12px; font-weight: 600; }
  .share-button { min-width: 106px; border-color: color-mix(in srgb, var(--accent) 54%, var(--border)); color: var(--accent-strong); background: var(--control-bg); }
  .share-button.danger { border-color: color-mix(in srgb, var(--danger) 35%, var(--border)); color: var(--danger); background: var(--danger-soft); }
  .share-button.danger:hover { border-color: color-mix(in srgb, var(--danger) 58%, var(--border)); background: color-mix(in srgb, var(--danger) 13%, var(--surface-raised)); }
  .empty-state { display: flex; min-height: 180px; flex-direction: column; align-items: center; justify-content: center; gap: 8px; border: 1px dashed var(--border-strong); border-radius: var(--card-radius); background: var(--surface-soft); color: var(--text-secondary); text-align: center; }
  .empty-state strong { color: var(--text); }
  .empty-state span { font-size: 13px; }
  .empty-state.compact { min-height: 140px; }
  .loading-dot { width: 9px; height: 9px; border-radius: 50%; background: var(--accent); animation: pulse 1s infinite alternate; }

  .jobs-overlay { position: fixed; z-index: 100; inset: 0; display: flex; align-items: center; justify-content: center; padding: 32px; }
  .jobs-backdrop { position: absolute; inset: 0; border: 0; border-radius: 0; background: var(--backdrop); cursor: default; }
  .jobs-panel { position: relative; display: flex; width: min(720px, calc(100vw - 64px)); min-height: 500px; max-height: min(720px, calc(100vh - 64px)); flex-direction: column; overflow: hidden; margin: 0; padding: 0; border: 1px solid var(--border-strong); border-radius: 20px; background: var(--surface-raised); box-shadow: var(--shadow-floating); color: var(--text); }
  .jobs-header { display: flex; align-items: flex-start; justify-content: space-between; gap: 20px; padding: 24px 26px 18px; background: var(--surface-raised); }
  .jobs-header h2 { margin-bottom: 6px; color: var(--text); font-size: 24px; letter-spacing: -.025em; }
  .jobs-header p { max-width: 490px; color: var(--text-secondary); font-size: 13px; line-height: 1.55; }
  .icon-button { display: grid; width: 38px; height: 38px; flex: 0 0 auto; place-items: center; border: 1px solid var(--border); border-radius: 10px; background: var(--control-bg); color: var(--text-secondary); cursor: pointer; transition: background .16s ease, color .16s ease, border-color .16s ease; }
  .icon-button:hover { border-color: var(--border-strong); background: var(--control-hover); color: var(--text); }
  .job-tabs { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; margin: 0 26px 18px; padding: 5px; border: 1px solid var(--border); border-radius: 12px; background: var(--surface-sunken); }
  .job-tabs button { display: inline-flex; min-height: 42px; align-items: center; justify-content: center; gap: 8px; padding: 8px 14px; border: 1px solid transparent; border-radius: 9px; background: transparent; color: var(--text-secondary); cursor: pointer; font-weight: 600; transition: background .16s ease, color .16s ease, border-color .16s ease, box-shadow .16s ease; }
  .job-tabs button:hover { color: var(--text); }
  .job-tabs button.active { border-color: var(--border); background: var(--surface-raised); box-shadow: 0 1px 3px rgba(24, 27, 40, .08); color: var(--accent-strong); }
  .job-tabs span { display: grid; min-width: 21px; height: 21px; place-items: center; border-radius: 999px; background: color-mix(in srgb, currentColor 11%, transparent); font-size: 11px; }
  .jobs-content { flex: 1; min-height: 0; overflow: auto; padding: 22px 26px 26px; border-top: 1px solid var(--border); background: var(--surface-soft); }
  .jobs-empty { display: flex; min-height: 310px; flex-direction: column; align-items: center; justify-content: center; gap: 9px; border: 1px dashed var(--border-strong); border-radius: 16px; background: var(--surface-raised); color: var(--text-secondary); text-align: center; }
  .jobs-empty-icon { display: grid; width: 62px; height: 62px; place-items: center; margin-bottom: 5px; border: 1px solid color-mix(in srgb, var(--accent) 20%, transparent); border-radius: 18px; background: var(--accent-soft); color: var(--accent-strong); }
  .jobs-empty strong { color: var(--text); font-size: 16px; }
  .jobs-empty > span:not(.jobs-empty-icon):not(.loading-dot) { max-width: 360px; color: var(--text-secondary); font-size: 13px; line-height: 1.55; }
  .job-list { display: grid; gap: 11px; }
  .job-card { padding: 16px; border: 1px solid var(--border); border-radius: 14px; background: var(--surface-raised); box-shadow: 0 1px 2px rgba(24, 27, 40, .035); }
  .job-main { display: flex; align-items: center; gap: 12px; }
  .printer-icon.small { width: 38px; height: 38px; border-radius: 10px; }
  .job-copy { display: grid; flex: 1; min-width: 0; gap: 4px; }
  .job-copy strong, .job-copy span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .job-copy strong { color: var(--text); }
  .job-copy span, .job-copy small { color: var(--text-secondary); font-size: 12px; }
  .job-state { flex: 0 0 auto; padding: 5px 9px; border-radius: 999px; background: color-mix(in srgb, var(--accent) 13%, var(--surface-raised)); color: var(--accent-strong); font-size: 11px; font-weight: 650; }
  .job-state.success { background: var(--success-soft); color: var(--success); }
  .job-state.warning { background: var(--warning-soft); color: var(--warning); }
  .job-state.danger { background: var(--danger-soft); color: var(--danger); }
  .progress-track { height: 5px; overflow: hidden; margin: 13px 0 1px 50px; border-radius: 99px; background: var(--surface-sunken); }
  .progress-track span { display: block; height: 100%; border-radius: inherit; background: var(--accent); transition: width .25s ease; }
  .progress-track span.success { background: var(--success); }
  .progress-track span.warning { background: var(--warning); }
  .progress-track span.danger { background: var(--danger); }
  .job-error { margin: 9px 0 0 50px; color: var(--danger); font-size: 12px; }

  :global(:root[data-theme="dark"]) .job-tabs button.active { border-color: var(--border-strong); box-shadow: 0 1px 5px rgba(0, 0, 0, .28); }
  :global(:root[data-theme="dark"]) .job-card { box-shadow: 0 1px 3px rgba(0, 0, 0, .18); }
  :global(:root[data-theme="dark"]) .jobs-panel { border-color: #444854; }

  .spinning { animation: spin .8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @keyframes pulse { to { opacity: .3; } }
  @media (max-width: 900px) {
    .shared-state { display: none; }
    .printer-page { padding-inline: 24px; }
    .jobs-overlay { padding: 18px; }
    .jobs-panel { width: calc(100vw - 36px); min-height: min(500px, calc(100vh - 36px)); max-height: calc(100vh - 36px); }
  }
  @media (max-width: 620px) {
    .page-heading { align-items: flex-start; flex-direction: column; }
    .heading-actions { width: 100%; }
    .heading-actions button { flex: 1; }
    .jobs-header { padding-inline: 20px; }
    .job-tabs { margin-inline: 20px; }
    .jobs-content { padding: 18px 20px 20px; }
    .job-main { align-items: flex-start; }
    .job-state { max-width: 96px; white-space: nowrap; }
  }
</style>
