<script lang="ts">
  import { onMount } from "svelte";
  import { Camera, Crosshair, Eye, Play, Stop, Trash } from "phosphor-svelte";

  import { bridge } from "../../bridge";
  import AppSelect from "../../components/AppSelect.svelte";
  import { SubscriptionScope, observeSnapshot } from "../../subscriptions";
  import type { DisplaySurface, GazeCamera, GazeStatus, RuntimeSnapshot } from "../../types";

  export let snapshot: RuntimeSnapshot;
  export let notify: (message: string, error?: boolean) => void;

  type CalibrationTarget = { display: DisplaySurface; u: number; v: number; label: string };
  let cameras: GazeCamera[] = [];
  let selectedCamera = "";
  let status: GazeStatus | null = null;
  let busy = false;
  let calibrationTargets: CalibrationTarget[] = [];
  let calibrationIndex = -1;

  $: running = Boolean(status && !["idle", "stopped", "failed"].includes(status.state));
  $: cameraOptions = cameras.map((camera) => ({ value: camera.id, label: camera.name }));
  $: target = calibrationIndex >= 0 ? calibrationTargets[calibrationIndex] : null;
  $: progress = calibrationTargets.length ? Math.round(Math.max(0, calibrationIndex) / calibrationTargets.length * 100) : 0;

  function buildTargets(): CalibrationTarget[] {
    const positions = [
      [0.1, 0.1, "左上"], [0.5, 0.1, "上方"], [0.9, 0.1, "右上"],
      [0.1, 0.5, "左侧"], [0.5, 0.5, "中央"], [0.9, 0.5, "右侧"],
      [0.1, 0.9, "左下"], [0.5, 0.9, "下方"], [0.9, 0.9, "右下"],
    ] as const;
    return Object.values(snapshot.configuration.layout?.displays ?? {}).flatMap((display) =>
      positions.map(([u, v, label]) => ({ display, u, v, label })),
    );
  }

  async function refreshCameras() {
    busy = true;
    try {
      cameras = await bridge.listGazeCameras();
      if (!selectedCamera || !cameras.some((camera) => camera.id === selectedCamera)) {
        selectedCamera = cameras[0]?.id ?? "";
      }
      if (!cameras.length) notify("camera-rs 没有枚举到摄像头，请检查系统摄像头权限。", true);
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
      status = await bridge.startGazeTracking(selectedCamera);
      notify("眼动追踪已启动；图像仅在本机内存中处理。");
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
      status = await bridge.stopGazeTracking();
      cancelCalibration();
    } catch (error) {
      notify(String(error), true);
    } finally {
      busy = false;
    }
  }

  async function beginCalibration() {
    if (!selectedCamera || !snapshot.configuration.layout) return;
    if (!running && !(await start())) return;
    calibrationTargets = buildTargets();
    if (!calibrationTargets.length) {
      notify("屏幕布局为空，无法标定。", true);
      return;
    }
    try {
      status = await bridge.beginGazeCalibration(selectedCamera);
      calibrationIndex = 0;
      notify("标定已开始：看向提示位置，保持头部自然，然后按空格采样。");
    } catch (error) {
      notify(String(error), true);
    }
  }

  async function captureTarget() {
    if (!target || busy) return;
    busy = true;
    try {
      const rect = target.display.deskRectUm;
      const x = Math.round(rect.x + rect.width * target.u);
      const y = Math.round(rect.y + rect.height * target.v);
      await bridge.captureGazeCalibrationSample(x, y);
      if (calibrationIndex + 1 >= calibrationTargets.length) {
        status = await bridge.finishGazeCalibration();
        calibrationIndex = -1;
        calibrationTargets = [];
        notify("眼动标定已保存，并已绑定当前屏幕布局。");
      } else {
        calibrationIndex += 1;
      }
    } catch (error) {
      notify(`采样失败：${String(error)}`, true);
    } finally {
      busy = false;
    }
  }

  function cancelCalibration() {
    calibrationIndex = -1;
    calibrationTargets = [];
  }

  async function clearCalibration() {
    if (!window.confirm("删除当前摄像头和屏幕布局的眼动标定？")) return;
    try {
      status = await bridge.clearGazeCalibration();
      cancelCalibration();
    } catch (error) {
      notify(String(error), true);
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.code === "Space" && target && !event.repeat) {
      event.preventDefault();
      void captureTarget();
    }
  }

  onMount(() => {
    const scope = new SubscriptionScope();
    void observeSnapshot(scope,
      (accept) => bridge.onGazeState(accept),
      () => bridge.getGazeStatus(),
      (next) => { status = next; },
    ).catch((error) => notify(String(error), true));
    void refreshCameras();
    window.addEventListener("keydown", handleKeydown);
    return () => { window.removeEventListener("keydown", handleKeydown); scope.dispose(); };
  });
</script>

<section class="gaze-page">
  <div class="gaze-grid">
    <article class="card setup-card">
      <div class="card-title"><Camera size={20} /><div><h2>摄像头与推理</h2><p>camera-rs RGB 输入 · 五个 Intel OMZ 模型 · MNN</p></div></div>
      <label class="field">摄像头
        <AppSelect bind:value={selectedCamera} options={cameraOptions} disabled={running || busy} placeholder="未发现摄像头" aria-label="摄像头" />
      </label>
      <div class="actions">
        <button class="button secondary" disabled={busy || running} on:click={refreshCameras}>刷新</button>
        {#if running}
          <button class="button danger" disabled={busy} on:click={stop}><Stop size={16} />停止</button>
        {:else}
          <button class="button" disabled={busy || !selectedCamera} on:click={start}><Play size={16} />启动</button>
        {/if}
      </div>
      {#if !cameras.length}<button class="permission" on:click={() => bridge.openCameraPermissionSettings()}>打开摄像头权限设置</button>{/if}
    </article>

    <article class="card status-card">
      <div class="card-title"><Eye size={20} /><div><h2>实时状态</h2><p>不会保存或上传人脸画面</p></div></div>
      <div class="status-line"><span class:live={running}></span><strong>{status?.state ?? "idle"}</strong><small>{status?.cameraName ?? "尚未启动"}</small></div>
      <dl>
        <div><dt>推理延迟</dt><dd>{status?.inferenceMs?.toFixed(1) ?? "—"} ms</dd></div>
        <div><dt>人脸置信度</dt><dd>{status?.faceConfidence?.toFixed(3) ?? "—"}</dd></div>
        <div><dt>采集 / 推理 / 丢帧</dt><dd>{status?.capturedFrames ?? 0} / {status?.inferredFrames ?? 0} / {status?.droppedFrames ?? 0}</dd></div>
        <div><dt>注视目标</dt><dd>{status?.target ? `${status.target.displayId} · ${Math.round(status.target.logicalX)}, ${Math.round(status.target.logicalY)}` : "—"}</dd></div>
      </dl>
      {#if status?.error}<p class="error">{status.error}</p>{/if}
    </article>
  </div>

  <article class="card calibration-card">
    <div class="card-title"><Crosshair size={20} /><div><h2>多屏标定</h2><p>每块屏幕 9 点；布局、摄像头或位置变化后应重新标定</p></div></div>
    {#if target}
      <div class="calibration-progress"><span style={`width:${progress}%`}></span></div>
      <div class="calibration-task">
        <div class="screen-map">
          <span class="target-dot" style={`left:${target.u * 100}%;top:${target.v * 100}%`}></span>
        </div>
        <div>
          <strong>看向“{target.display.name}”的{target.label}</strong>
          <p>保持注视约半秒，然后按空格。远端屏幕可按示意位置目测；本机屏幕可将 ArcRelay 移到该屏后采样。</p>
          <small>{calibrationIndex + 1} / {calibrationTargets.length}</small>
        </div>
        <button class="button" disabled={busy} on:click={captureTarget}>采集此点（空格）</button>
        <button class="button secondary" on:click={cancelCalibration}>取消</button>
      </div>
    {:else}
      <div class="calibration-ready">
        <div><strong>{status?.calibrated ? "当前布局已标定" : "尚未标定"}</strong><p>未标定时仍会显示人脸、眼睛状态和视线向量，但不会生成跨屏目标。</p></div>
        <div class="actions">
          {#if status?.calibrated}<button class="button secondary" on:click={clearCalibration}><Trash size={15} />删除标定</button>{/if}
          <button class="button" disabled={!running || !snapshot.configuration.layout} on:click={beginCalibration}><Crosshair size={16} />{status?.calibrated ? "重新标定" : "开始标定"}</button>
        </div>
      </div>
    {/if}
  </article>

  <div class="safety-note"><strong>安全默认：</strong>稳定注视只预选屏幕和位置；只有随后明确移动实体鼠标才切换 Arc Input 目标，注视本身不会点击、输入或抢占控制权。</div>
</section>

<style>
  .gaze-page { display: grid; gap: 12px; padding-bottom: 20px; }.gaze-grid { display: grid; grid-template-columns: minmax(0, .9fr) minmax(0, 1.1fr); gap: 12px; }.card { border: 1px solid var(--border); border-radius: 13px; padding: 16px; }.card-title { display: flex; align-items: flex-start; gap: 10px; margin-bottom: 15px; color: var(--accent-strong); }.card-title h2 { margin: 0; color: var(--text); font-size: 15px; }.card-title p, .calibration-ready p, .calibration-task p { margin: 3px 0 0; color: var(--text-secondary); font-size: 11px; line-height: 1.5; }.actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 12px; }.actions button, .calibration-task button { display: inline-flex; align-items: center; gap: 6px; }.permission { margin-top: 10px; border: 0; color: var(--accent-strong); background: transparent; font-size: 11px; }.status-line { display: grid; grid-template-columns: 8px auto 1fr; align-items: center; gap: 8px; }.status-line > span { width: 8px; height: 8px; border-radius: 50%; background: var(--text-muted); }.status-line > span.live { background: var(--success); box-shadow: 0 0 0 3px color-mix(in srgb,var(--success) 15%,transparent); }.status-line strong { text-transform: capitalize; }.status-line small { overflow: hidden; color: var(--text-secondary); text-align: right; text-overflow: ellipsis; white-space: nowrap; }dl { display: grid; gap: 7px; margin: 14px 0 0; }dl div { display: flex; justify-content: space-between; gap: 12px; font-size: 11px; }dt { color: var(--text-secondary); }dd { margin: 0; font-variant-numeric: tabular-nums; }.error { color: var(--danger); font-size: 11px; }.calibration-ready { display: flex; align-items: center; justify-content: space-between; gap: 20px; }.calibration-ready strong, .calibration-task strong { font-size: 13px; }.calibration-progress { height: 4px; margin-bottom: 14px; overflow: hidden; border-radius: 99px; background: var(--surface-sunken); }.calibration-progress span { display: block; height: 100%; background: var(--accent); }.calibration-task { display: grid; grid-template-columns: 150px minmax(180px,1fr) auto auto; align-items: center; gap: 14px; }.screen-map { position: relative; aspect-ratio: 16/9; border: 2px solid var(--border-strong); border-radius: 7px; background: var(--surface-sunken); }.target-dot { position: absolute; width: 15px; height: 15px; border: 3px solid #fff; border-radius: 50%; background: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb,var(--accent) 25%,transparent); transform: translate(-50%,-50%); }.calibration-task small { color: var(--text-muted); }.safety-note { border: 1px solid color-mix(in srgb,var(--accent) 24%,var(--border)); border-radius: 10px; padding: 10px 13px; color: var(--text-secondary); background: color-mix(in srgb,var(--accent-soft) 55%,var(--surface)); font-size: 11px; }.safety-note strong { color: var(--text); }@media (max-width: 950px) { .gaze-grid { grid-template-columns: 1fr; }.calibration-task { grid-template-columns: 120px 1fr; }.calibration-task button { grid-column: span 1; }.calibration-ready { align-items: flex-start; flex-direction: column; } }
</style>
