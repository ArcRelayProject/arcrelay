<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import {
    ArrowClockwise, Camera, Check, CheckCircle, Crosshair, Eye, LockKey,
    Monitor, Play, Stop, Trash, WarningCircle, X,
  } from "phosphor-svelte";

  import { bridge } from "../../bridge";
  import AppSelect from "../../components/AppSelect.svelte";
  import calibrationMonitor from "../../assets/illustrations/gaze-calibration-monitor.png";
  import type { GazeCalibrationOverlayEvent } from "../../ipc/generated";
  import { SubscriptionScope, observeSnapshot } from "../../subscriptions";
  import type { DisplaySurface, GazeCamera, GazeStatus, RuntimeSnapshot } from "../../types";
  import {
    clampProgress, observationCanHeadCalibrate, samplesAreStable, toStabilitySample,
    type StabilitySample,
  } from "./gazeCalibration";

  export let snapshot: RuntimeSnapshot;
  export let notify: (message: string, error?: boolean) => void;

  type Phase = "ready" | "checking" | "calibrating" | "transition" | "complete";
  type Target = { display: DisplaySurface; screenIndex: number; u: number; v: number; label: string };

  const DWELL_MS = 180;
  const PREFLIGHT_MS = 600;
  const FRESH_MS = 700;
  const POINTS = Array.from({ length: 9 }, (_, index) => [0.5, 0.5, `样本 ${index + 1}`] as const);

  let cameras: GazeCamera[] = [];
  let selectedCamera = "";
  let status: GazeStatus | null = null;
  let phase: Phase = "ready";
  let busy = false;
  let captureBusy = false;
  let launchBusy = false;
  let targets: Target[] = [];
  let targetIndex = -1;
  let preflightProgress = 0;
  let dwellProgress = 0;
  let stableSince = 0;
  let preflightStableSince = 0;
  let lastObservationAt = 0;
  let lastInferenceFrame = -1;
  let samples: StabilitySample[] = [];
  let ticker: number | undefined;
  let transitionTimer: number | undefined;
  let calibrationSessionId = "";
  let completedScreens: string[] = [];
  let refineDisplayId: string | null = null;
  let flowSending = false;
  let lastFlowSentAt = 0;

  $: running = Boolean(status && !["idle", "stopped", "failed"].includes(status.state));
  $: cameraOptions = cameras.map((camera) => ({ value: camera.id, label: camera.name }));
  $: target = targetIndex >= 0 ? targets[targetIndex] : null;
  $: allDisplays = Object.values(snapshot.configuration.layout?.displays ?? {})
    .filter((display) => snapshot.displayAvailability[display.displayId] !== "Offline")
    .sort((a, b) => a.deskRectUm.y - b.deskRectUm.y || a.deskRectUm.x - b.deskRectUm.x);
  $: deviceCount = new Set(allDisplays.map((display) => display.deviceId)).size;
  $: overallProgress = targets.length
    ? Math.round((Math.max(0, targetIndex) + dwellProgress) / targets.length * 100)
    : 0;
  $: faceReady = observationCanHeadCalibrate(status?.observation, status?.faceConfidence);
  $: samplingMode = "滤波后的头部方向采样";
  $: liveTargetName = allDisplays.find((display) => display.displayId === status?.target?.displayId)?.name;
  $: calibratedDisplayIds = new Set(status?.calibratedDisplayIds ?? []);

  function screenIndex(display: DisplaySurface): number {
    return Object.values(snapshot.configuration.layout?.displays ?? {})
      .filter((candidate) => candidate.deviceId === display.deviceId)
      .sort((a, b) => a.logicalBounds.y - b.logicalBounds.y || a.logicalBounds.x - b.logicalBounds.x)
      .findIndex((candidate) => candidate.displayId === display.displayId);
  }

  function buildTargets(displayId: string | null = null): Target[] {
    return allDisplays
      .filter((display) => !displayId || display.displayId === displayId)
      .flatMap((display) => POINTS.map(([u, v, label]) => ({
      display,
      screenIndex: Math.max(0, screenIndex(display)),
      u,
      v,
      label,
    })));
  }

  function newSessionId(): string {
    return globalThis.crypto?.randomUUID?.() ?? `gaze-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  }

  function ingestStatus(next: GazeStatus) {
    status = next;
    if (["idle", "stopped", "failed"].includes(next.state)
      && next.cameraId && cameras.some((camera) => camera.id === next.cameraId)) {
      selectedCamera = next.cameraId;
    }
    if (next.inferredFrames === lastInferenceFrame) return;
    lastInferenceFrame = next.inferredFrames;
    if (observationCanHeadCalibrate(next.observation, next.faceConfidence)) {
      lastObservationAt = performance.now();
      samples = [...samples, toStabilitySample(next.observation)].slice(-6);
    } else {
      samples = [];
      stableSince = 0;
      preflightStableSince = 0;
    }
  }

  async function refreshCameras() {
    busy = true;
    try {
      cameras = await bridge.listGazeCameras();
      const savedCamera = status?.cameraId;
      if (savedCamera && cameras.some((camera) => camera.id === savedCamera)) selectedCamera = savedCamera;
      else if (!selectedCamera || !cameras.some((camera) => camera.id === selectedCamera)) selectedCamera = cameras[0]?.id ?? "";
      if (!cameras.length) notify("没有检测到摄像头，请检查系统摄像头权限。", true);
    } catch (error) {
      notify(`摄像头枚举失败：${String(error)}`, true);
    } finally {
      busy = false;
    }
  }

  async function start(): Promise<boolean> {
    if (!selectedCamera) return false;
    busy = true;
    try {
      ingestStatus(await bridge.startGazeTracking(selectedCamera));
      return true;
    } catch (error) {
      notify(`启动眼动追踪失败：${String(error)}`, true);
      return false;
    } finally {
      busy = false;
    }
  }

  async function stop() {
    busy = true;
    try {
      await cancelCalibration(false);
      ingestStatus(await bridge.stopGazeTracking());
    } catch (error) {
      notify(String(error), true);
    } finally {
      busy = false;
    }
  }

  async function resumeTracking() {
    if (await start()) notify("头部屏幕识别已启动，原有标定已恢复。");
  }

  async function beginCalibration(displayId: string | null = null) {
    if (busy || !selectedCamera || !snapshot.configuration.layout) return;
    if (!running && !(await start())) return;
    refineDisplayId = displayId;
    calibrationSessionId = newSessionId();
    phase = "checking";
    preflightProgress = 0;
    preflightStableSince = 0;
    samples = [];
    lastObservationAt = 0;
  }

  async function launchCalibration() {
    if (launchBusy || phase !== "checking") return;
    launchBusy = true;
    try {
      targets = buildTargets(refineDisplayId);
      if (!targets.length) throw new Error("屏幕布局中没有在线显示器");
      ingestStatus(await bridge.beginGazeCalibration(selectedCamera, refineDisplayId));
      targetIndex = 0;
      resetPointStability();
      phase = "calibrating";
      await sendFlow("calibrating", targets[0]);
    } catch (error) {
      await bridge.cancelGazeCalibration().catch(() => {});
      phase = "ready";
      notify(`无法开始标定：${String(error)}`, true);
    } finally {
      launchBusy = false;
    }
  }

  function flowEvent(stage: string, active: Target, next: Target | null = null): GazeCalibrationOverlayEvent {
    return {
      sessionId: calibrationSessionId,
      stage,
      sourceDeviceId: snapshot.serviceInstanceId,
      targetDeviceId: active.display.deviceId,
      displayId: active.display.displayId,
      screenIndex: active.screenIndex,
      nextScreenIndex: next?.screenIndex ?? null,
      screenName: active.display.name,
      nextScreenName: next?.display.name ?? null,
      targetU: active.u,
      targetV: active.v,
      dwellProgress,
      current: Math.max(0, targetIndex),
      total: targets.length,
    };
  }

  async function sendFlow(
    stage: "calibrating" | "paused" | "transition",
    active: Target,
    next: Target | null = null,
    screenNameOverride?: string,
  ) {
    if (flowSending || !calibrationSessionId) return;
    const now = performance.now();
    if (stage === "calibrating" && now - lastFlowSentAt < 80) return;
    flowSending = true;
    lastFlowSentAt = now;
    try {
      const event = flowEvent(stage, active, next);
      if (screenNameOverride) event.screenName = screenNameOverride;
      await bridge.sendGazeCalibrationOverlay(event);
    } catch (error) {
      notify(`无法在 ${active.display.name} 显示标定圆点：${String(error)}`, true);
      await cancelCalibration(false);
    } finally {
      flowSending = false;
    }
  }

  async function closeDevice(deviceId: string) {
    const display = allDisplays.find((item) => item.deviceId === deviceId);
    if (!display || !calibrationSessionId) return;
    await bridge.sendGazeCalibrationOverlay({
      ...flowEvent("close", { display, screenIndex: 0, u: .5, v: .5, label: "" }),
      targetDeviceId: deviceId,
      stage: "close",
      current: 0,
      total: 0,
    }).catch(() => {});
  }

  async function closeAllDevices() {
    await Promise.all([...new Set(allDisplays.map((display) => display.deviceId))].map(closeDevice));
  }

  function resetPointStability() {
    dwellProgress = 0;
    stableSince = 0;
    samples = [];
    lastObservationAt = 0;
  }

  async function captureTarget() {
    if (!target || captureBusy || phase !== "calibrating") return;
    captureBusy = true;
    try {
      const rect = target.display.deskRectUm;
      await bridge.captureGazeCalibrationSample(
        Math.round(rect.x + rect.width * target.u),
        Math.round(rect.y + rect.height * target.v),
      );
      if (targetIndex + 1 >= targets.length) {
        ingestStatus(await bridge.finishGazeCalibration());
        completedScreens = [...new Set(targets.map((item) => item.display.name))];
        await closeAllDevices();
        targetIndex = -1;
        phase = "complete";
        notify(refineDisplayId
          ? `${completedScreens[0]} 的头部方向标定已优化。`
          : "头部屏幕区域标定已完成。稳定看向另一块屏幕后会自动移动鼠标。");
        refineDisplayId = null;
        return;
      }

      const previous = target;
      targetIndex += 1;
      const next = targets[targetIndex];
      if (previous.display.displayId !== next.display.displayId) {
        resetPointStability();
        phase = "transition";
        if (previous.display.deviceId !== next.display.deviceId) {
          await closeDevice(previous.display.deviceId);
          await sendFlow("transition", next, next, previous.display.name);
        } else {
          await sendFlow("transition", previous, next);
        }
        transitionTimer = window.setTimeout(() => {
          phase = "calibrating";
          resetPointStability();
          void sendFlow("calibrating", next);
        }, 900);
      } else {
        dwellProgress = 0;
        stableSince = performance.now();
        await sendFlow("calibrating", next);
      }
    } catch (error) {
      stableSince = 0;
      dwellProgress = 0;
      notify(`自动采样失败：${String(error)}`, true);
      if (target) await sendFlow("paused", target);
    } finally {
      captureBusy = false;
    }
  }

  async function cancelCalibration(announce = true) {
    if (transitionTimer) clearTimeout(transitionTimer);
    transitionTimer = undefined;
    if (calibrationSessionId) await closeAllDevices();
    if (phase !== "ready" && phase !== "complete") await bridge.cancelGazeCalibration().catch(() => {});
    targets = [];
    targetIndex = -1;
    resetPointStability();
    phase = "ready";
    calibrationSessionId = "";
    refineDisplayId = null;
    if (announce) notify("已退出标定，临时采样没有保存。", false);
  }

  async function clearCalibration() {
    if (!window.confirm("删除当前摄像头和屏幕布局的眼动标定？")) return;
    try {
      ingestStatus(await bridge.clearGazeCalibration());
      phase = "ready";
      completedScreens = [];
    } catch (error) {
      notify(String(error), true);
    }
  }

  function tickCalibration() {
    const now = performance.now();
    const fresh = now - lastObservationAt <= FRESH_MS;
    const stable = fresh && samplesAreStable(samples);
    if (phase === "checking") {
      if (!stable) {
        preflightStableSince = 0;
        preflightProgress = 0;
      } else {
        if (!preflightStableSince) preflightStableSince = now;
        preflightProgress = clampProgress((now - preflightStableSince) / PREFLIGHT_MS);
        if (preflightProgress >= 1) void launchCalibration();
      }
      return;
    }
    if (phase !== "calibrating" || !target || captureBusy) return;
    if (!fresh) {
      stableSince = 0;
      dwellProgress = 0;
      void sendFlow("paused", target);
    } else if (!stable) {
      stableSince = 0;
      dwellProgress = 0;
      void sendFlow("calibrating", target);
    } else {
      if (!stableSince) stableSince = now;
      dwellProgress = clampProgress((now - stableSince) / DWELL_MS);
      void sendFlow("calibrating", target);
      if (dwellProgress >= 1) void captureTarget();
    }
  }

  onMount(() => {
    const scope = new SubscriptionScope();
    void observeSnapshot(scope,
      (accept) => bridge.onGazeState(accept),
      () => bridge.getGazeStatus(),
      ingestStatus,
    ).catch((error) => notify(String(error), true));
    void refreshCameras();
    if ("__TAURI_INTERNALS__" in window) {
      void scope.add(listen<string>("gaze-calibration-cancel", (event) => {
        if (!calibrationSessionId || event.payload === calibrationSessionId) void cancelCalibration();
      }));
    }
    ticker = window.setInterval(tickCalibration, 60);
    return () => {
      if (ticker) clearInterval(ticker);
      if (transitionTimer) clearTimeout(transitionTimer);
      if (calibrationSessionId) void closeAllDevices();
      scope.dispose();
    };
  });
</script>

<section class="gaze-page" aria-live="polite">
  <div class="stepper" aria-label="标定步骤">
    <span class:active={phase === "ready" || phase === "checking"} class:done={phase !== "ready" && phase !== "checking"}><i>{phase === "ready" || phase === "checking" ? "1" : "✓"}</i>准备</span><b></b>
    <span class:active={phase === "calibrating" || phase === "transition"} class:done={phase === "complete"}><i>{phase === "complete" ? "✓" : "2"}</i>标定</span><b></b>
    <span class:active={phase === "complete"}><i>3</i>完成</span>
  </div>

  {#if phase === "ready"}
    <article class="hero-card">
      <div class="hero-copy">
        <span class="eyebrow"><Crosshair size={15} weight="bold" />多设备头部区域标定</span>
        <h2>把头转向每块屏幕中央的圆点</h2>
        <p>ArcRelay 只学习你看向每块屏幕时的头部方向，不再依赖偏头时不稳定的眼动向量。标定后看向目标屏幕即可移动鼠标。</p>
        <label class="camera-field"><span>用于标定的摄像头</span><AppSelect bind:value={selectedCamera} options={cameraOptions} disabled={running || busy} placeholder="未发现摄像头" aria-label="用于标定的摄像头" /></label>
        <div class="privacy-line"><LockKey size={15} weight="fill" /><span><strong>完全本机处理</strong>　只同步圆点位置与进度，摄像头画面和人脸特征不会离开本机。</span></div>
        <div class="hero-actions">
          {#if status?.calibrated && !running}
            <button class="start-button" disabled={busy || !selectedCamera || !snapshot.configuration.layout} on:click={resumeTracking}><Play size={17} weight="fill" />{busy ? "正在启动…" : "启动识别"}</button>
            <button class="text-button" disabled={busy} on:click={() => beginCalibration()}><ArrowClockwise size={15} />全部重新标定</button>
          {:else if !status?.calibrated}
            <button class="start-button" disabled={busy || !selectedCamera || !snapshot.configuration.layout} on:click={() => beginCalibration()}><Play size={17} weight="fill" />{busy ? "正在启动…" : "开始标定"}</button>
          {:else}
            <button class="text-button" disabled={busy} on:click={() => beginCalibration()}><ArrowClockwise size={15} />全部重新标定</button>
          {/if}
          <button class="text-button" disabled={busy} on:click={refreshCameras}><ArrowClockwise size={15} />重新检测摄像头</button>
        </div>
      </div>
      <div class="preview-panel" aria-hidden="true">
        <img src={calibrationMonitor} alt="" />
        <div class="preview-meta"><span><Monitor size={15} />{allDisplays.length} 块屏幕</span><span>{deviceCount} 台设备</span><span><Eye size={15} />头部区域识别</span></div>
      </div>
    </article>
    <div class="readiness-grid">
      <section><span class:ok={Boolean(cameras.length)}>{#if cameras.length}<Check size={16} weight="bold" />{:else}<WarningCircle size={16} />{/if}</span><div><strong>摄像头</strong><p>{cameras.length ? `${cameras.length} 个设备可用` : "等待摄像头权限"}</p></div></section>
      <section><span class:ok={Boolean(allDisplays.length)}>{#if allDisplays.length}<Check size={16} weight="bold" />{:else}<WarningCircle size={16} />{/if}</span><div><strong>标定范围</strong><p>{allDisplays.length} 块屏幕 · {deviceCount} 台设备</p></div></section>
      <section><span class:ok={running}>{#if running}<Check size={16} weight="bold" />{:else}<Camera size={16} />{/if}</span><div><strong>眼动服务</strong><p>{running ? `${status?.cameraName ?? "摄像头"} 正在运行` : "开始标定时自动启动"}</p></div></section>
    </div>
    {#if status?.calibrated}
      <section class="existing-profile">
        <CheckCircle size={18} weight="fill" />
        <div class="profile-copy"><strong>标定已保存，重启后可直接恢复</strong><p>某块屏幕识别不准时，只优化这一块即可保留其他屏幕的数据。</p></div>
        <div class="profile-screens">
          {#each allDisplays as display}
            <span class:missing={!calibratedDisplayIds.has(display.displayId)}>
              <Monitor size={14} /><b>{display.name}</b>
              <button disabled={busy || !selectedCamera} on:click={() => beginCalibration(display.displayId)}>{calibratedDisplayIds.has(display.displayId) ? "优化标定" : "补充标定"}</button>
            </span>
          {/each}
        </div>
        <button class="delete-profile" on:click={clearCalibration}><Trash size={15} />删除全部</button>
      </section>
    {/if}
  {:else if phase === "checking"}
    <article class="state-card">
      <div class="check-visual" style={`--check-progress:${preflightProgress * 360}deg`}><Eye size={34} weight="duotone" /></div>
      <span class="eyebrow">自动环境检查</span><h2>{faceReady ? "很好，请保持这个姿势" : "请面向摄像头"}</h2>
      <p>{faceReady ? "正在确认滤波后的头部方向稳定，完成后会自动进入跨设备全屏标定。" : "让面部保持在画面中，坐姿自然，允许较大角度偏转。"}</p>
      <div class="check-list"><span class:ok={Boolean(status?.faceConfidence && status.faceConfidence >= .6)}><i></i>面部清晰</span><span class:ok={Boolean(status?.observation)}><i></i>头部方向有效</span><span class:ok={faceReady}><i></i>姿态稳定</span></div>
      <div class="bar"><span style={`width:${preflightProgress * 100}%`}></span></div><button class="cancel-button" on:click={() => cancelCalibration()}><X size={15} />取消</button>
    </article>
  {:else if phase === "calibrating" || phase === "transition"}
    <article class="state-card">
      <div class="running-icon"><Crosshair size={30} weight="duotone" /></div><span class="eyebrow">跨设备全屏标定</span>
      <h2>{phase === "transition" ? "正在切换到下一块屏幕" : `请看向 ${target?.display.name ?? "屏幕"} 上的圆点`}</h2>
      <p>{phase === "transition" ? "下一台设备会自动显示引导，不需要移动或点击窗口。" : `${samplingMode} · 保持头部朝向圆点；面部丢失时暂停，恢复后继续。`}</p>
      <div class="bar"><span style={`width:${overallProgress}%`}></span></div><strong class="progress-label">{overallProgress}% · {targetIndex + 1}/{targets.length}</strong>
      <button class="cancel-button" on:click={() => cancelCalibration()}><Stop size={15} />退出标定</button>
    </article>
  {:else}
    <article class="state-card complete-card">
      <div class="complete-icon"><Check size={38} weight="bold" /></div><span class="eyebrow">标定完成</span><h2>看向屏幕即可移动鼠标</h2>
      <p>ArcRelay 会根据滤波后的头部方向选择屏幕；稳定看向另一块屏幕后，鼠标会自动移动到该屏幕中央。</p>
      <div class="screen-results">{#each completedScreens as name}<span><Monitor size={16} /><strong>{name}</strong><i><Check size={13} weight="bold" /></i></span>{/each}</div>
      <div class="hero-actions"><button class="start-button" on:click={() => phase = "ready"}>返回眼动设置</button><button class="text-button" on:click={() => beginCalibration()}><ArrowClockwise size={15} />全部重新标定</button></div>
    </article>
  {/if}

  {#if phase === "ready"}
    <section class:recognized={Boolean(running && status?.target)} class="recognition-card" role="status">
      <div class="recognition-icon"><Eye size={22} weight={status?.target ? "fill" : "duotone"} /></div>
      <div class="recognition-copy">
        <span>{running ? "实时识别结果" : status?.calibrated ? "标定已就绪" : "等待标定"}</span>
        <strong>{status?.target ? `正在看向 ${liveTargetName ?? "已标定屏幕"}` : running ? "正在确认头部方向…" : status?.calibrated ? "启动后显示正在看的屏幕" : "完成标定后显示识别结果"}</strong>
      </div>
      <div class="recognition-screens" aria-label="屏幕识别状态">
        {#each allDisplays as display}
          <span class:active={status?.target?.displayId === display.displayId}><Monitor size={15} /><b>{display.name}</b>{#if status?.target?.displayId === display.displayId}<i>正在看</i>{/if}</span>
        {/each}
      </div>
      <dl><div><dt>推理延迟</dt><dd>{status?.inferenceMs?.toFixed(0) ?? "—"} ms</dd></div><div><dt>人脸置信度</dt><dd>{status?.faceConfidence?.toFixed(2) ?? "—"}</dd></div></dl>
      {#if running}<button class="stop-tracking" on:click={stop} disabled={busy}><Stop size={14} />停止</button>{/if}
    </section>
  {/if}
  {#if status?.error}<div class="error-banner"><WarningCircle size={17} /><span>{status.error}</span></div>{/if}
  <div class="safety-note"><strong>自动切屏：</strong>稳定看向另一块屏幕后，ArcRelay 会移动鼠标到屏幕中央；按 ⌘⌥⇧ Esc 可随时紧急释放，不会自动点击或输入。</div>
</section>

<style>
  .gaze-page{display:grid;gap:12px;padding:2px 0 22px}.stepper{display:grid;grid-template-columns:auto minmax(28px,82px) auto minmax(28px,82px) auto;align-items:center;justify-content:center;gap:10px;padding:2px 0 5px;color:var(--text-muted);font-size:11px;font-weight:650}.stepper span{display:inline-flex;align-items:center;gap:7px;white-space:nowrap}.stepper i{display:grid;width:22px;height:22px;place-items:center;border:1px solid var(--border-strong);border-radius:50%;background:var(--surface);font-style:normal;font-size:10px}.stepper b{height:1px;background:var(--border)}.stepper span.active{color:var(--accent-strong)}.stepper span.active i{border-color:var(--accent);color:#fff;background:var(--accent)}.stepper span.done i{border-color:var(--success);color:#fff;background:var(--success)}
  .hero-card{display:grid;grid-template-columns:minmax(0,1.12fr) minmax(310px,.88fr);gap:32px;overflow:hidden;border:1px solid var(--border);border-radius:18px;padding:26px 28px;background:var(--surface-raised);box-shadow:var(--shadow-card)}.hero-copy{align-self:center}.eyebrow{display:inline-flex;align-items:center;gap:6px;color:var(--accent-strong);font-size:11px;font-weight:750;letter-spacing:.02em}.hero-card h2,.state-card h2{margin:8px 0 7px;color:var(--text);font-size:23px;font-weight:760;line-height:1.18;letter-spacing:-.035em}.hero-copy>p,.state-card>p{max-width:590px;margin:0;color:var(--text-secondary);font-size:12px;line-height:1.7}.camera-field{display:grid;gap:7px;max-width:420px;margin-top:20px;color:var(--text-secondary);font-size:11px}.privacy-line{display:flex;align-items:flex-start;gap:8px;max-width:540px;margin-top:13px;color:var(--text-muted);font-size:10px;line-height:1.55}.privacy-line :global(svg){flex:none;margin-top:1px;color:var(--success)}.privacy-line strong{color:var(--text-secondary)}.hero-actions{display:flex;align-items:center;justify-content:center;gap:15px;margin-top:20px}.hero-copy .hero-actions{justify-content:flex-start}.start-button{display:inline-flex;min-height:39px;align-items:center;justify-content:center;gap:8px;border:1px solid var(--accent);border-radius:10px;padding:0 17px;color:#fff;background:var(--accent);font-size:12px;font-weight:720;box-shadow:0 8px 18px color-mix(in srgb,var(--accent) 20%,transparent)}.start-button:disabled{border-color:var(--border-strong);color:var(--text-muted);background:var(--surface-sunken);box-shadow:none}.text-button{display:inline-flex;align-items:center;gap:6px;border:0;padding:8px 0;color:var(--text-secondary);background:transparent;font-size:11px}
  .preview-panel{display:grid;align-content:center;gap:8px;padding:0 4px}.preview-panel img{display:block;width:100%;max-height:255px;object-fit:contain;filter:drop-shadow(0 16px 22px rgba(31,33,47,.12))}.preview-meta{display:flex;justify-content:space-between;gap:10px;color:var(--text-muted);font-size:10px}.preview-meta span{display:inline-flex;align-items:center;gap:5px}
  .readiness-grid{display:grid;grid-template-columns:repeat(3,1fr);gap:10px}.readiness-grid section{display:flex;align-items:center;gap:11px;border:1px solid var(--border);border-radius:12px;padding:12px 14px;background:var(--surface-raised)}.readiness-grid section>span{display:grid;width:28px;height:28px;flex:none;place-items:center;border-radius:9px;color:var(--text-muted);background:var(--surface-sunken)}.readiness-grid section>span.ok{color:var(--success);background:var(--success-soft)}.readiness-grid strong{display:block;color:var(--text);font-size:11px}.readiness-grid p{margin:2px 0 0;color:var(--text-muted);font-size:10px}
  .existing-profile{display:grid;grid-template-columns:auto minmax(170px,.75fr) minmax(300px,1.25fr) auto;align-items:center;gap:12px;border:1px solid color-mix(in srgb,var(--success) 28%,var(--border));border-radius:12px;padding:12px 13px;background:color-mix(in srgb,var(--success-soft) 55%,var(--surface))}.existing-profile>:global(svg){color:var(--success)}.profile-copy strong{display:block;color:var(--text);font-size:11px}.profile-copy p{margin:2px 0 0;color:var(--text-secondary);font-size:10px}.profile-screens{display:flex;flex-wrap:wrap;gap:6px}.profile-screens>span{display:inline-flex;align-items:center;gap:6px;border:1px solid color-mix(in srgb,var(--success) 22%,var(--border));border-radius:8px;padding:5px 6px 5px 8px;color:var(--text-secondary);background:var(--surface);font-size:9px}.profile-screens>span.missing{border-color:color-mix(in srgb,var(--warning) 32%,var(--border))}.profile-screens b{max-width:120px;overflow:hidden;color:var(--text);font-size:9px;text-overflow:ellipsis;white-space:nowrap}.profile-screens button,.delete-profile{display:inline-flex;align-items:center;gap:5px;border:1px solid var(--border-strong);border-radius:7px;padding:5px 7px;color:var(--text-secondary);background:var(--surface);font-size:9px}.profile-screens button{border:0;color:var(--accent-strong);background:var(--accent-soft)}
  .state-card{display:grid;min-height:390px;place-content:center;justify-items:center;border:1px solid var(--border);border-radius:18px;padding:34px;background:var(--surface-raised);box-shadow:var(--shadow-card);text-align:center}.check-visual{position:relative;display:grid;width:94px;height:94px;margin-bottom:20px;place-items:center;border-radius:50%;color:var(--accent);background:conic-gradient(var(--accent) var(--check-progress),var(--border) 0)}.check-visual:after{position:absolute;inset:7px;border-radius:50%;background:var(--surface-raised);content:""}.check-visual :global(svg){position:relative;z-index:1}.check-list{display:flex;gap:8px;margin-top:25px}.check-list span{display:inline-flex;align-items:center;gap:6px;border:1px solid var(--border);border-radius:999px;padding:7px 10px;color:var(--text-muted);background:var(--surface-soft);font-size:10px}.check-list i{width:7px;height:7px;border-radius:50%;background:var(--border-strong)}.check-list span.ok{color:var(--success);background:var(--success-soft)}.check-list span.ok i{background:var(--success)}.bar{width:min(390px,70vw);height:5px;margin-top:22px;overflow:hidden;border-radius:99px;background:var(--surface-sunken)}.bar span{display:block;height:100%;border-radius:inherit;background:var(--accent);transition:width .1s linear}.cancel-button{display:inline-flex;align-items:center;gap:6px;margin-top:20px;border:0;color:var(--text-muted);background:transparent;font-size:10px}.running-icon{display:grid;width:76px;height:76px;margin-bottom:18px;place-items:center;border-radius:50%;color:var(--accent);background:var(--accent-soft);box-shadow:0 0 0 12px color-mix(in srgb,var(--accent-soft) 55%,transparent)}.progress-label{margin-top:8px;color:var(--accent-strong);font-size:12px;font-variant-numeric:tabular-nums}.complete-icon{display:grid;width:84px;height:84px;margin-bottom:18px;place-items:center;border-radius:50%;color:#fff;background:var(--success);box-shadow:0 0 0 12px var(--success-soft)}.screen-results{display:flex;max-width:680px;flex-wrap:wrap;justify-content:center;gap:8px;margin-top:24px}.screen-results>span{display:flex;min-width:180px;align-items:center;gap:8px;border:1px solid var(--border);border-radius:10px;padding:10px 12px;color:var(--text-secondary);background:var(--surface-soft);font-size:10px}.screen-results strong{flex:1;color:var(--text);font-size:11px;text-align:left}.screen-results i{display:grid;width:20px;height:20px;place-items:center;border-radius:50%;color:#fff;background:var(--success)}
  .recognition-card{display:grid;grid-template-columns:auto minmax(155px,.6fr) minmax(280px,1.4fr) auto auto;align-items:center;gap:12px;border:1px solid var(--border);border-radius:14px;padding:13px 15px;background:var(--surface-raised);box-shadow:var(--shadow-card)}.recognition-card.recognized{border-color:color-mix(in srgb,var(--accent) 42%,var(--border));background:linear-gradient(110deg,color-mix(in srgb,var(--accent-soft) 62%,var(--surface-raised)),var(--surface-raised) 45%)}.recognition-icon{display:grid;width:38px;height:38px;place-items:center;border-radius:11px;color:var(--text-muted);background:var(--surface-sunken)}.recognized .recognition-icon{color:var(--accent);background:var(--accent-soft);box-shadow:0 0 0 4px color-mix(in srgb,var(--accent-soft) 45%,transparent)}.recognition-copy span{display:block;color:var(--text-muted);font-size:9px;font-weight:700}.recognition-copy strong{display:block;margin-top:2px;color:var(--text);font-size:13px}.recognition-screens{display:flex;min-width:0;flex-wrap:wrap;gap:6px}.recognition-screens>span{display:inline-flex;min-width:0;align-items:center;gap:5px;border:1px solid var(--border);border-radius:8px;padding:6px 8px;color:var(--text-muted);background:var(--surface-soft);font-size:9px}.recognition-screens>span.active{border-color:var(--accent);color:var(--accent-strong);background:var(--accent-soft)}.recognition-screens b{max-width:115px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.recognition-screens i{border-radius:99px;padding:2px 5px;color:#fff;background:var(--accent);font-style:normal;font-size:8px}.recognition-card dl{display:flex;gap:14px;margin:0}.recognition-card dl div{display:grid;gap:2px;font-size:8px}.recognition-card dt{color:var(--text-muted)}.recognition-card dd{margin:0;color:var(--text);font-size:9px;font-variant-numeric:tabular-nums}.stop-tracking{display:inline-flex;align-items:center;gap:5px;border:1px solid var(--border-strong);border-radius:8px;padding:7px 9px;color:var(--text-secondary);background:var(--surface);font-size:9px}.error-banner{display:flex;align-items:center;gap:8px;border:1px solid color-mix(in srgb,var(--danger) 25%,var(--border));border-radius:10px;padding:9px 12px;color:var(--danger);background:var(--danger-soft);font-size:10px}.safety-note{border:1px solid color-mix(in srgb,var(--accent) 22%,var(--border));border-radius:10px;padding:9px 12px;color:var(--text-secondary);background:color-mix(in srgb,var(--accent-soft) 48%,var(--surface));font-size:10px}.safety-note strong{color:var(--text)}
  @media(max-width:900px){.hero-card{grid-template-columns:1fr}.preview-panel{display:none}.readiness-grid{grid-template-columns:1fr}.hero-card h2,.state-card h2{font-size:20px}.stepper{grid-template-columns:auto 30px auto 30px auto}.existing-profile,.recognition-card{grid-template-columns:auto 1fr}.profile-screens,.recognition-screens{grid-column:1/-1}.recognition-card dl{display:none}}
</style>
