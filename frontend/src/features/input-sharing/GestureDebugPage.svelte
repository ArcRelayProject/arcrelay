<script lang="ts">
  import AppSelect from "../../components/AppSelect.svelte";
  import { translate as uiTranslate, language as uiLanguage } from "../../i18n";
  import { onMount } from "svelte";
  import { bridge } from "../../bridge";
  import { invoke } from "../../ipc/client";
  import type { GestureDebugAction, GestureDebugReport, GestureDebugSnapshot, GestureDebugTrigger } from "../../ipc/generated";
  import { captureActions, gestureName, gesturePhase, gestureReport, isDockSwipe } from "./gestureDebug";

  export let inputSharingEnabled = false;

  const native = bridge.isTauri();
  let snapshot: GestureDebugSnapshot | null = null;
  let error = "";
  let busy = false;
  let stopping = false;
  let disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let durationMs = 500;
  let delayMs = 1500;
  let amount = 0.5;
  let reverse = false;
  let cancelAtEnd = false;
  let interceptConfirmed = false;
  let actionIndex = 0;
  let testLabel = captureActions[0];
  let notes = "";
  let copied = false;
  let exporting = false;
  let reportData: GestureDebugReport | null = null;

  const actions: { action: GestureDebugAction; label: string }[] = [
    { action: "swipeLeft", label: "← 向左滑动" },
    { action: "swipeRight", label: "向右滑动 →" },
    { action: "swipeUp", label: "↑ 向上滑动" },
    { action: "swipeDown", label: "↓ 向下滑动" },
    { action: "pinchIn", label: "捏合" },
    { action: "pinchOut", label: "张开" },
  ];

  $: unavailable = !native || !snapshot?.available;
  $: active = Boolean(snapshot?.capturing || snapshot?.injecting || snapshot?.batches.some((batch) => !batch.finished));
  $: cannotStart = unavailable || !snapshot?.accessibilityGranted || inputSharingEnabled || busy || stopping || exporting || Boolean(snapshot?.injecting);
  $: atBatchLimit = Boolean(snapshot && snapshot.batches.length >= snapshot.batchLimit);
  $: latest = snapshot?.samples.slice().reverse() ?? [];
  $: report = reportData ? gestureReport(reportData, notes) : "";

  async function refresh() {
    try {
      const next = await invoke("get_gesture_debug_snapshot");
      if (!disposed) snapshot = next;
    } catch (cause) { if (!disposed) error = String(cause); }
    finally { if (!disposed) timer = setTimeout(refresh, 300); }
  }

  async function run(operation: () => Promise<GestureDebugSnapshot>) {
    busy = true;
    reportData = null;
    copied = false;
    error = "";
    try {
      const next = await operation();
      if (!disposed) snapshot = next;
    } catch (cause) { if (!disposed) error = String(cause); }
    finally {
      busy = false;
      if (disposed && native) void invoke("stop_gesture_debug").catch(() => {});
    }
  }

  function start(suppress: boolean) {
    if (suppress && !interceptConfirmed) return;
    void run(() => invoke("start_gesture_debug_capture", { suppress, label: testLabel }));
  }

  function trigger(action: GestureDebugAction) {
    const value: GestureDebugTrigger = { action, durationMs, delayMs, amount, reverse, cancelAtEnd };
    void run(() => invoke("trigger_gesture_debug", { request: value }));
  }

  async function stop() {
    stopping = true;
    try { snapshot = await invoke("stop_gesture_debug"); }
    catch (cause) { error = String(cause); }
    finally { stopping = false; }
  }

  function selectAction(index: number) {
    actionIndex = index;
    testLabel = captureActions[index];
  }

  async function exportReport(download: boolean) {
    exporting = true;
    copied = false;
    error = "";
    try {
      const data = await invoke("get_gesture_debug_report");
      if (disposed) return;
      reportData = data;
      const text = gestureReport(data, notes);
      if (download) {
        const url = URL.createObjectURL(new Blob([text], { type: "text/plain;charset=utf-8" }));
        const link = document.createElement("a");
        link.href = url;
        link.download = "arcrelay-gesture-debug.txt";
        link.click();
        setTimeout(() => URL.revokeObjectURL(url), 1000);
      } else {
        try { await navigator.clipboard.writeText(text); copied = true; }
        catch { error = "无法自动复制，报告已生成，请展开下方 TXT 报告手动复制，或下载文件。"; }
      }
    } catch (cause) { if (!disposed) error = String(cause); }
    finally { exporting = false; }
  }

  onMount(() => {
    if (native) void refresh();
    return () => {
      disposed = true;
      clearTimeout(timer);
      if (native) void invoke("stop_gesture_debug").catch(() => {});
    };
  });
</script>

<section class="gesture-lab" aria-label={uiTranslate("本机多指手势实验", $uiLanguage)}>
  <div class="lab-warning">
    <strong>{uiTranslate("实验接口 · 仅本机 · 不接入跨设备传输", $uiLanguage)}</strong>
    <p>{uiTranslate("使用未公开的 macOS 系统手势事件，可能无效或方向相反。先关闭通用控制及其他手势工具，准备至少两个桌面。这里不检测原始触点，也不能可靠区分三指和四指。", $uiLanguage)}</p>
    <p>{uiTranslate("只读观测不会阻止本机动作；拦截最多 10 秒。离开本页会停止。紧急释放：", $uiLanguage)}<kbd>⌘⌥⇧ Esc</kbd>。</p>
  </div>

  {#if !native}
    <div class="lab-notice">{uiTranslate("当前是浏览器预览，只展示界面。请运行 macOS 应用进行真实采集与合成，debug／release 均可；这里不会显示模拟成功结果。", $uiLanguage)}</div>
  {:else if snapshot && !snapshot.available}
    <div class="lab-notice">{uiTranslate("此实验仅支持 macOS 应用，当前平台不可用；不限制 debug／release 构建。", $uiLanguage)}</div>
  {:else if !snapshot}
    <div class="lab-notice">{uiTranslate("正在读取本机实验状态…", $uiLanguage)}</div>
  {/if}
  {#if inputSharingEnabled}<div class="lab-notice">{uiTranslate("请返回并关闭正式跨屏输入，再开始本机实验。无需取消配对或删除布局。", $uiLanguage)}</div>{/if}
  {#if snapshot?.available && !snapshot.accessibilityGranted}
    <div class="lab-notice">{uiTranslate("当前 ArcRelay 应用尚未获得辅助功能权限。", $uiLanguage)}
      <button class="button secondary" on:click={() => bridge.openInputPermissionSettings().catch((cause) => error = String(cause))}>{uiTranslate("打开权限设置", $uiLanguage)}</button>
    </div>
  {/if}
  {#if error}<div class="lab-error" role="alert">{error}</div>{/if}

  <div class="lab-status">
    <span>macOS <strong>{snapshot?.osVersion || "—"}</strong></span>
    <span>{uiTranslate("采集", $uiLanguage)} <strong>{snapshot?.capturing ? `${snapshot.suppressing ? "拦截" : "只读"} · ${Math.ceil(snapshot.remainingMs / 1000)}s` : "已停止"}</strong></span>
    <span>{uiTranslate("合成", $uiLanguage)} <strong>{snapshot?.injecting ? "等待／执行中" : "空闲"}</strong></span>
    <button class="button danger" disabled={!native || stopping} on:click={stop}>{stopping ? "正在停止…" : "立即停止全部实验"}</button>
  </div>

  <div class="lab-panels">
    <section class="lab-card">
      <h2>{uiTranslate("1 · 观测真实手势", $uiLanguage)}</h2>
      <p>{uiTranslate("在触控板上做三指／四指手势，观察是否收到 type 29 / 30。未收到事件也请记录；它不等于触控板没有检测到手指。", $uiLanguage)}</p>
      <label class="lab-field">{uiTranslate("八动作快捷标签（人工指定）", $uiLanguage)}
        <AppSelect value={actionIndex} disabled={active || busy || stopping} onValueChange={(event) => selectAction(Number(event))} aria-label={uiTranslate("八动作快捷标签（人工指定）", $uiLanguage)}
          options={[
            ...captureActions.map((label, index) => ({ value: index, label: [(index + 1), " · ", (label)].join("") })),
          ]}
        />
      </label>
      <label class="lab-field">{uiTranslate("本次动作标签（开始时固定保存）", $uiLanguage)}<input bind:value={testLabel} disabled={active || busy || stopping} maxlength="160" placeholder={uiTranslate("例如：四指上滑，Mission Control", $uiLanguage)} /></label>
      <div class="lab-buttons">
        <button class="button" disabled={cannotStart || active || atBatchLimit || !testLabel.trim()} on:click={() => start(false)}>{uiTranslate("只读观测 · 120 秒", $uiLanguage)}</button>
        <button class="button secondary" disabled={cannotStart || active || atBatchLimit || !interceptConfirmed || !testLabel.trim()} on:click={() => start(true)}>{uiTranslate("拦截系统手势 · 10 秒", $uiLanguage)}</button>
        <button class="button secondary" disabled={active || busy || stopping || actionIndex >= captureActions.length - 1} on:click={() => selectAction(actionIndex + 1)}>{uiTranslate("下一动作 →", $uiLanguage)}</button>
      </div>
      <label class="lab-check"><input type="checkbox" bind:checked={interceptConfirmed} />{uiTranslate("我理解拦截会暂时阻止本机系统滑动／捏合", $uiLanguage)}</label>
      <p>{uiTranslate("每次开始都会新增一个编号试次并保留旧记录。建议每次只做标签对应的一个动作，停止后点「下一动作」再开始；八次完成后一起导出。不自动识别手指数或动作个数。", $uiLanguage)}</p>
      <small>{uiTranslate("可以切到其他应用测试全局采集。窗口失焦不会停止；关闭窗口或离开本页会停止。标签仅标记该试次的意图。", $uiLanguage)}</small>
    </section>

    <section class="lab-card">
      <h2>{uiTranslate("2 · 手动合成系统手势", $uiLanguage)}</h2>
      <p>{uiTranslate("点击后在本机投递 DockSwipe 序列。左右用于测试桌面切换，上下用于 Mission Control／App Exposé，捏合用于系统级动作，不是应用内缩放。", $uiLanguage)}</p>
      <div class="lab-fields">
        <label class="lab-field">{uiTranslate("时长（ms）", $uiLanguage)}<input type="number" min="150" max="2000" step="50" bind:value={durationMs} /></label>
        <label class="lab-field">{uiTranslate("延迟（ms）", $uiLanguage)}<input type="number" min="0" max="3000" step="500" bind:value={delayMs} /></label>
        <label class="lab-field">{uiTranslate("幅度", $uiLanguage)}<input type="number" min="0.05" max="1" step="0.05" bind:value={amount} /></label>
      </div>
      <div class="lab-buttons">
        <label class="lab-check"><input type="checkbox" bind:checked={reverse} />{uiTranslate("反转方向", $uiLanguage)}</label>
        <label class="lab-check"><input type="checkbox" bind:checked={cancelAtEnd} />{uiTranslate("结束时取消（测试回退）", $uiLanguage)}</label>
      </div>
      <div class="lab-action-grid">
        {#each actions as item}<button class="button secondary" disabled={cannotStart || snapshot?.suppressing || (atBatchLimit && !snapshot?.capturing)} on:click={() => trigger(item.action)}>{item.label}</button>{/each}
      </div>
      <small>{uiTranslate("只实现 macOS ≤ 26 的 legacy 格式；方向和具体动作受系统设置影响。合成与拦截不能同时运行，可与只读观测并行。", $uiLanguage)}</small>
    </section>
  </div>

  <section class="lab-card lab-events">
    <div class="lab-card-heading">
      <div><h2>{uiTranslate("试次与事件记录", $uiLanguage)}</h2><p>{uiTranslate("已记录", $uiLanguage)} {snapshot?.batches.length ?? 0} / {snapshot?.batchLimit ?? 32} {uiTranslate("次 · 共", $uiLanguage)} {snapshot?.totalSamples ?? 0} {uiTranslate("条 · 标记汇总", $uiLanguage)} {snapshot?.markerSamples ?? 0} {uiTranslate("条 · 争锁丢失", $uiLanguage)} {snapshot?.droppedSamples ?? 0} {uiTranslate("条 · 容量拒收", $uiLanguage)} {snapshot?.capacityDroppedSamples ?? 0} {uiTranslate("条 · 覆盖 0 条", $uiLanguage)}</p></div>
      <button class="button secondary" disabled={unavailable || busy || stopping || active || exporting} on:click={() => run(() => invoke("clear_gesture_debug"))}>{uiTranslate("清空全部试次", $uiLanguage)}</button>
    </div>
    <p>{uiTranslate("每个试次保留最多", $uiLanguage)} {snapshot?.sampleLimit ?? 8192} {uiTranslate("条非标记事件，达到上限会停止，不覆盖已有记录。导出包含所有试次；下方实时事件仅展示最近试次的最后 60 条。", $uiLanguage)}</p>
    {#if atBatchLimit}<div class="lab-notice">{uiTranslate("试次已满，请先导出，再清空记录以继续。", $uiLanguage)}</div>{/if}
    {#if snapshot && (snapshot.droppedSamples > 0 || snapshot.capacityDroppedSamples > 0)}<div class="lab-error" role="alert">{uiTranslate("存在丢失事件，不能保证序列完整。报告将保留丢失计数，请勿将其视为完整采集。", $uiLanguage)}</div>{/if}
    <div class="lab-table-wrap">
      <table aria-label={uiTranslate("已记录的动作试次", $uiLanguage)}>
        <thead><tr><th>{uiTranslate("试次 / 手动标签", $uiLanguage)}</th><th>{uiTranslate("模式", $uiLanguage)}</th><th>{uiTranslate("事件 / 标记", $uiLanguage)}</th><th>{uiTranslate("争锁丢失 / 容量拒收", $uiLanguage)}</th><th>{uiTranslate("状态", $uiLanguage)}</th></tr></thead>
        <tbody>
          {#each snapshot?.batches ?? [] as batch (batch.id)}
            <tr><td>#{batch.id} · {batch.label}</td><td>{batch.mode === "intercept" ? "拦截" : batch.mode === "observe" ? "只读" : "合成"}</td><td>{batch.retainedSamples} / {batch.markerSamples}</td><td>{batch.droppedSamples} / {batch.capacityDroppedSamples}</td><td>{batch.finished ? "已结束" : "进行中"}<small>{batch.stopReason || "—"}</small></td></tr>
          {:else}<tr><td colspan="5" class="lab-empty">{uiTranslate("尚无试次。每次开始采集会自动编号，即使没有事件也会保留该试次。", $uiLanguage)}</td></tr>{/each}
        </tbody>
      </table>
    </div>
    <p class="lab-message" role="status">{snapshot?.message || "尚未开始采集。"}</p>
    <p><code>tap</code> {uiTranslate("= 已观测（可能包含合成）；", $uiLanguage)}<code>posted</code> {uiTranslate("= 仅已投递。后者不证明动作执行成功。", $uiLanguage)}</p>
    <div class="lab-table-wrap">
      <table>
        <thead><tr><th>{uiTranslate("试次 / # / ms", $uiLanguage)}</th><th>{uiTranslate("来源", $uiLanguage)}</th><th>{uiTranslate("手势 / 类型", $uiLanguage)}</th><th>{uiTranslate("阶段", $uiLanguage)}</th><th>{uiTranslate("进度", $uiLanguage)}</th><th>{uiTranslate("速度 X / Y", $uiLanguage)}</th><th>{uiTranslate("结果", $uiLanguage)}</th></tr></thead>
        <tbody>
          {#each latest as sample (`${sample.batchId}:${sample.id}`)}
            <tr><td>{sample.batchId} / {sample.id}<small>{sample.elapsedMs} ms</small></td><td><code>{sample.source}</code></td><td>{gestureName(sample)}<small>{sample.eventType} / {sample.subtype}{isDockSwipe(sample) ? ` / axis ${sample.motion}` : " / 原始字段见报告"}</small></td><td>{isDockSwipe(sample) ? gesturePhase(sample.phase) : "未解码"}<small>f132: {sample.phase}</small></td><td>{isDockSwipe(sample) ? sample.progress.toFixed(4) : "—"}</td><td>{isDockSwipe(sample) ? `${sample.velocityX.toFixed(3)} / ${sample.velocityY.toFixed(3)}` : "—"}</td><td>{sample.suppressed ? "已拦截" : sample.source === "posted" ? "待人工确认" : "未拦截"}</td></tr>
          {:else}<tr><td colspan="7" class="lab-empty">{uiTranslate("暂无事件。开始观测后，在触控板上做系统手势。", $uiLanguage)}</td></tr>{/each}
        </tbody>
      </table>
    </div>
  </section>

  <section class="lab-card">
    <h2>{uiTranslate("测试结论与报告", $uiLanguage)}</h2>
    <label class="lab-field">{uiTranslate("实际观察", $uiLanguage)}<textarea bind:value={notes} maxlength="4000" rows="3" placeholder={uiTranslate("实际手指数／系统设置；是否采到；是否拦截；是否切换；方向是否正确；动画是否跟随；取消后是否恢复。", $uiLanguage)}></textarea></label>
    <div class="lab-buttons">
      <button class="button secondary" disabled={!snapshot?.batches.length || active || busy || stopping || exporting} on:click={() => exportReport(false)}>{exporting ? "正在导出…" : copied ? "已复制（可再次复制）" : "复制 TXT 报告"}</button>
      <button class="button secondary" disabled={!snapshot?.batches.length || active || busy || stopping || exporting} on:click={() => exportReport(true)}>{uiTranslate("下载 .txt", $uiLanguage)}</button>
      <small>{uiTranslate("先停止实验再导出；包含全部编号试次和事件，表格使用紧凑 CSV，不再重复 JSON 字段名。仅在点击时复制／下载，不自动保存或上传。", $uiLanguage)}</small>
    </div>
    <details><summary>{uiTranslate("查看已生成的 TXT 报告／手动复制", $uiLanguage)}</summary><textarea class="lab-report" readonly value={report} placeholder={uiTranslate("停止实验后，点击复制或下载生成完整报告。", $uiLanguage)} rows="8" aria-label={uiTranslate("手势实验 TXT 报告", $uiLanguage)}></textarea></details>
  </section>
</section>

<style>
  .gesture-lab { display: grid; gap: 16px; padding-bottom: 28px; font-size: 13px; }
  .lab-warning, .lab-notice, .lab-error { border: 1px solid var(--border); border-radius: 12px; padding: 14px 16px; background: var(--warning-soft); }
  .lab-warning strong { color: var(--warning); }.lab-error { color: var(--danger); background: var(--danger-soft); }
  p { margin: 7px 0 12px; color: var(--text-secondary); line-height: 1.65; }h2 { margin: 0; font-size: 16px; }small { color: var(--text-muted); font-size: 11px; line-height: 1.6; }
  .lab-status { display: flex; flex-wrap: wrap; align-items: center; gap: 18px; }.lab-status .button { margin-left: auto; }
  .lab-panels { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 16px; }
  .lab-card { min-width: 0; padding: 18px; border: 1px solid var(--border); border-radius: 12px; background: var(--surface-raised); }
  .lab-buttons { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; margin-block: 12px; }
  .lab-fields { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; }
  .lab-field { display: grid; gap: 6px; margin-top: 12px; color: var(--text-secondary); }.lab-field input, .lab-field :global(.app-select-trigger), textarea { box-sizing: border-box; width: 100%; min-width: 0; border: 1px solid var(--border-strong); border-radius: 8px; padding: 8px; color: var(--text); background: var(--control-bg); font: inherit; }
  .lab-check { display: inline-flex; align-items: center; gap: 7px; line-height: 1.6; }.lab-check input { accent-color: var(--accent); }
  .lab-action-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; margin-bottom: 12px; }
  .lab-card-heading { display: flex; justify-content: space-between; align-items: start; gap: 12px; }.lab-card-heading .button { flex-shrink: 0; }
  .lab-message { color: var(--text); }code, .lab-report { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }.lab-report { margin-top: 10px; font-size: 11px; }
  .lab-table-wrap { overflow: auto; max-height: 330px; }table { width: 100%; border-collapse: collapse; white-space: nowrap; text-align: left; font-size: 11px; }th, td { border-bottom: 1px solid var(--border); padding: 9px 10px; }th { position: sticky; top: 0; background: var(--surface-raised); color: var(--text-secondary); }td small { display: block; }.lab-empty { text-align: center; padding: 28px; color: var(--text-muted); }
  button:disabled { opacity: .45; }details { margin-top: 12px; }summary { cursor: pointer; color: var(--text-secondary); }kbd { font-family: inherit; }
  @media (max-width: 1000px) { .lab-panels { grid-template-columns: 1fr; } }
</style>
