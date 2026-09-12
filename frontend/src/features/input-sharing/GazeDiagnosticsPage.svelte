<script lang="ts">
  import { onMount } from "svelte";
  import {
    ArrowLeft, Camera, CheckCircle, Eye, LockKey, Monitor, Play, Stop, WarningCircle,
  } from "phosphor-svelte";

  import { bridge } from "../../bridge";
  import AppSelect from "../../components/AppSelect.svelte";
  import type { GazePreviewRectView, GazePreviewView } from "../../ipc/generated";
  import type { DisplaySurface, GazeCamera, GazeStatus } from "../../types";

  export let status: GazeStatus | null;
  export let cameras: GazeCamera[];
  export let selectedCamera: string;
  export let displays: DisplaySurface[];
  export let busy: boolean;
  export let onStart: (cameraId: string) => Promise<boolean>;
  export let onStop: () => Promise<void>;
  export let onClose: () => void;
  export let notify: (message: string, error?: boolean) => void;

  let preview: GazePreviewView | null = null;
  let cameraId = selectedCamera;
  let previewError = "";

  $: if (selectedCamera && !cameraId) cameraId = selectedCamera;
  $: running = Boolean(status && !["idle", "stopped", "failed"].includes(status.state));
  $: cameraOptions = cameras.map((camera) => ({ value: camera.id, label: camera.name }));
  $: activeDisplay = displays.find((display) => display.displayId === status?.target?.displayId);
  $: presenceLabel = status?.presenceState === "ownerPresent"
    ? "本人在场"
    : status?.presenceState === "absent"
      ? "无人"
      : status?.presenceState === "unknownPresent"
        ? "陌生人在场"
        : status?.presenceState === "multiplePeople"
          ? "多人在场"
          : "正在确认";

  function rectStyle(rect: GazePreviewRectView): string {
    if (!preview?.width || !preview.height) return "display:none";
    return [
      `left:${rect.x / preview.width * 100}%`,
      `top:${rect.y / preview.height * 100}%`,
      `width:${rect.width / preview.width * 100}%`,
      `height:${rect.height / preview.height * 100}%`,
    ].join(";");
  }

  async function start() {
    if (!cameraId) return;
    preview = null;
    previewError = "";
    await onStart(cameraId);
  }

  onMount(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void bridge.onGazePreview((next) => {
      preview = next;
      previewError = "";
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch((error) => {
      previewError = String(error);
      notify(`无法订阅摄像头预览：${previewError}`, true);
    });
    void bridge.setGazePreviewEnabled(true).catch((error) => {
      previewError = String(error);
      notify(`无法启用摄像头预览：${previewError}`, true);
    });
    return () => {
      disposed = true;
      unlisten?.();
      void bridge.setGazePreviewEnabled(false);
    };
  });
</script>

<section class="diagnostics-page" aria-label="本机视觉诊断">
  <header>
    <button class="back-button" on:click={onClose}><ArrowLeft size={16} />返回眼动设置</button>
    <div><span>本机视觉诊断</span><h2>摄像头与人脸识别</h2><p>实时确认摄像头画面、检测区域和本机用户匹配结果。</p></div>
    <span class:active={running} class="runtime-state"><i></i>{running ? "正在采集" : "未启动"}</span>
  </header>

  <div class="toolbar">
    <label><span>摄像头</span><AppSelect bind:value={cameraId} options={cameraOptions} disabled={running || busy} placeholder="未发现摄像头" aria-label="诊断摄像头" /></label>
    {#if running}
      <button class="stop-button" disabled={busy} on:click={onStop}><Stop size={15} />停止摄像头</button>
    {:else}
      <button class="start-button" disabled={busy || !cameraId} on:click={start}><Play size={16} weight="fill" />{busy ? "正在启动…" : "启动摄像头"}</button>
    {/if}
    <div class="privacy"><LockKey size={15} weight="fill" /><span>预览仅在本机内存中传递，关闭此页后立即停止生成，不保存、不上传。</span></div>
  </div>

  <div class="diagnostics-grid">
    <article class="camera-card">
      <div class="card-heading"><div><span>实时画面</span><strong>{status?.cameraName ?? "等待选择摄像头"}</strong></div><b>{preview ? `${preview.width} × ${preview.height}` : "—"}</b></div>
      <div class:empty={!preview} class="camera-stage" style={`aspect-ratio:${preview?.width ?? 16}/${preview?.height ?? 9}`}>
        {#if preview}
          <img src={preview.imageDataUrl} alt="摄像头实时诊断预览" />
          {#if preview.face}<span class="face-box" style={rectStyle(preview.face)}><i>人脸</i></span>{/if}
          {#if preview.leftEye}<span class="eye-box" style={rectStyle(preview.leftEye)}></span>{/if}
          {#if preview.rightEye}<span class="eye-box" style={rectStyle(preview.rightEye)}></span>{/if}
        {:else}
          <Camera size={38} weight="duotone" />
          <strong>{running ? "等待第一帧…" : "启动摄像头以查看实时画面"}</strong>
          <span>{previewError || "绿色框表示人脸，蓝色框表示双眼检测区域。"}</span>
        {/if}
      </div>
      <div class="legend"><span><i class="face-dot"></i>人脸区域</span><span><i class="eye-dot"></i>双眼区域</span><b>帧 #{preview?.sequence ?? "—"}</b></div>
    </article>

    <aside class="results">
      <article class:good={Boolean(status?.observation)} class="result-card primary">
        <div class="result-icon"><Eye size={22} weight="duotone" /></div>
        <div><span>视觉检测</span><strong>{status?.observation ? "人脸与眼睛可用" : running ? "没有检测到有效人脸" : "等待摄像头"}</strong></div>
        <em>{status?.faceConfidence?.toFixed(2) ?? "—"}</em>
      </article>

      <article class:good={status?.presenceState === "ownerPresent"} class="result-card">
        <div class="result-icon"><LockKey size={21} weight="duotone" /></div>
        <div><span>本机用户识别</span><strong>{presenceLabel}</strong><small>{status?.presenceProfileEnrolled ? status.presenceProfileName ?? "已录入用户" : "尚未录入本机用户"}</small></div>
        <em>{status?.presenceOwnerSimilarity?.toFixed(2) ?? "—"}</em>
      </article>

      <article class:good={Boolean(status?.target)} class="result-card">
        <div class="result-icon"><Monitor size={21} weight="duotone" /></div>
        <div><span>屏幕判断</span><strong>{status?.target ? activeDisplay?.name ?? status.target.displayId : "尚无稳定目标"}</strong><small>{status?.target?.source === "eye" ? "注视映射" : status?.target ? "头部区域" : "等待标定结果"}</small></div>
        <em>{status?.target?.confidence.toFixed(2) ?? "—"}</em>
      </article>

      <article class="metrics-card">
        <div><span>人脸数量</span><strong>{status?.presenceFaceCount ?? 0}</strong></div>
        <div><span>推理延迟</span><strong>{status?.inferenceMs?.toFixed(0) ?? "—"}<small> ms</small></strong></div>
        <div><span>采集帧</span><strong>{status?.capturedFrames ?? 0}</strong></div>
        <div><span>推理帧</span><strong>{status?.inferredFrames ?? 0}</strong></div>
        <div><span>丢弃帧</span><strong>{status?.droppedFrames ?? 0}</strong></div>
        <div><span>稳定时间</span><strong>{Math.round((status?.presenceStableForMs ?? 0) / 100) / 10}<small> s</small></strong></div>
      </article>

      <article class="vectors-card">
        <div class="card-heading"><div><span>姿态与注视向量</span><strong>用于分析上下屏误判</strong></div></div>
        <dl>
          <div><dt>头部 yaw</dt><dd>{status?.observation?.headYaw.toFixed(2) ?? "—"}°</dd></div>
          <div><dt>头部 pitch</dt><dd>{status?.observation?.headPitch.toFixed(2) ?? "—"}°</dd></div>
          <div><dt>注视 X</dt><dd>{status?.observation?.gazeX.toFixed(3) ?? "—"}</dd></div>
          <div><dt>注视 Y</dt><dd>{status?.observation?.gazeY.toFixed(3) ?? "—"}</dd></div>
        </dl>
        <p><CheckCircle size={14} />双眼状态：{status?.observation ? `${status.observation.leftEyeOpen ? "左眼睁开" : "左眼闭合"} · ${status.observation.rightEyeOpen ? "右眼睁开" : "右眼闭合"}` : "无有效观测"}</p>
      </article>
    </aside>
  </div>

  {#if status?.error}<div class="error-banner"><WarningCircle size={17} /><span>{status.error}</span></div>{/if}
</section>

<style>
  .diagnostics-page{display:grid;gap:14px;padding:2px 0 22px}.diagnostics-page>header{display:flex;align-items:center;gap:16px;border-bottom:1px solid var(--border);padding:2px 0 14px}.diagnostics-page>header>div{flex:1}.diagnostics-page header span,.card-heading span,.result-card span{display:block;color:var(--text-muted);font-size:9px;font-weight:720;letter-spacing:.035em}.diagnostics-page h2{margin:3px 0;color:var(--text);font-size:21px;letter-spacing:-.03em}.diagnostics-page header p{margin:0;color:var(--text-secondary);font-size:10px}.back-button{display:inline-flex;align-items:center;gap:6px;border:1px solid var(--border);border-radius:9px;padding:8px 10px;color:var(--text-secondary);background:var(--surface);font-size:10px}.runtime-state{display:inline-flex!important;align-items:center;gap:7px;border:1px solid var(--border);border-radius:999px;padding:7px 10px;background:var(--surface-raised)}.runtime-state i{width:7px;height:7px;border-radius:50%;background:var(--text-muted)}.runtime-state.active{color:var(--success)}.runtime-state.active i{background:var(--success);box-shadow:0 0 0 4px var(--success-soft)}
  .toolbar{display:flex;align-items:end;gap:10px;border:1px solid var(--border);border-radius:12px;padding:11px 12px;background:var(--surface-raised)}.toolbar label{display:grid;min-width:260px;gap:5px;color:var(--text-muted);font-size:9px}.start-button,.stop-button{display:inline-flex;min-height:36px;align-items:center;gap:7px;border-radius:9px;padding:0 13px;font-size:10px;font-weight:700}.start-button{border:1px solid var(--accent);color:#fff;background:var(--accent)}.stop-button{border:1px solid var(--border-strong);color:var(--text-secondary);background:var(--surface)}.start-button:disabled,.stop-button:disabled{opacity:.55}.privacy{display:flex;max-width:430px;align-items:flex-start;gap:7px;margin-left:auto;color:var(--text-muted);font-size:9px;line-height:1.5}.privacy :global(svg){flex:none;color:var(--success)}
  .diagnostics-grid{display:grid;grid-template-columns:minmax(420px,1.42fr) minmax(310px,.78fr);gap:14px}.camera-card,.result-card,.metrics-card,.vectors-card{border:1px solid var(--border);border-radius:14px;background:var(--surface-raised);box-shadow:var(--shadow-card)}.camera-card{display:grid;align-content:start;gap:10px;padding:14px}.card-heading{display:flex;align-items:center;justify-content:space-between;gap:12px}.card-heading strong{display:block;margin-top:2px;color:var(--text);font-size:11px}.card-heading>b{color:var(--text-muted);font-size:9px;font-weight:650}.camera-stage{position:relative;overflow:hidden;border-radius:11px;background:#111827}.camera-stage img{display:block;width:100%;height:100%;object-fit:contain}.camera-stage.empty{display:grid;min-height:300px;place-content:center;justify-items:center;gap:8px;color:#8190a8;background:linear-gradient(145deg,#121b2a,#1d2c42)}.camera-stage.empty strong{color:#d9e2f0;font-size:12px}.camera-stage.empty span{max-width:330px;color:#8190a8;font-size:9px;text-align:center}.face-box,.eye-box{position:absolute;box-sizing:border-box;border:2px solid #42d392;border-radius:5px;pointer-events:none}.face-box i{position:absolute;top:-20px;left:-2px;border-radius:4px 4px 0 0;padding:3px 6px;color:#09281d;background:#42d392;font-style:normal;font-size:8px;font-weight:800}.eye-box{border-color:#4b9cff;border-width:1.5px}.legend{display:flex;align-items:center;gap:14px;color:var(--text-muted);font-size:9px}.legend span{display:inline-flex;align-items:center;gap:5px}.legend i{width:8px;height:8px;border-radius:2px}.face-dot{background:#42d392}.eye-dot{background:#4b9cff}.legend b{margin-left:auto;font-weight:600;font-variant-numeric:tabular-nums}
  .results{display:grid;align-content:start;gap:9px}.result-card{display:grid;grid-template-columns:auto 1fr auto;align-items:center;gap:10px;padding:11px 12px}.result-card.good{border-color:color-mix(in srgb,var(--success) 38%,var(--border));background:color-mix(in srgb,var(--success-soft) 38%,var(--surface-raised))}.result-icon{display:grid;width:35px;height:35px;place-items:center;border-radius:10px;color:var(--text-muted);background:var(--surface-sunken)}.result-card.good .result-icon{color:var(--success);background:var(--success-soft)}.result-card strong{display:block;margin-top:2px;color:var(--text);font-size:11px}.result-card small{display:block;margin-top:2px;color:var(--text-muted);font-size:8px}.result-card em{color:var(--text);font-style:normal;font-size:15px;font-weight:740;font-variant-numeric:tabular-nums}.metrics-card{display:grid;grid-template-columns:repeat(3,1fr);overflow:hidden}.metrics-card div{display:grid;gap:4px;border-right:1px solid var(--border);border-bottom:1px solid var(--border);padding:10px}.metrics-card div:nth-child(3n){border-right:0}.metrics-card div:nth-child(n+4){border-bottom:0}.metrics-card span{color:var(--text-muted);font-size:8px}.metrics-card strong{color:var(--text);font-size:12px;font-variant-numeric:tabular-nums}.metrics-card small{font-size:8px;font-weight:500}.vectors-card{display:grid;gap:10px;padding:12px}.vectors-card dl{display:grid;grid-template-columns:1fr 1fr;gap:6px;margin:0}.vectors-card dl div{display:flex;justify-content:space-between;border-radius:7px;padding:7px 8px;background:var(--surface-sunken);font-size:9px}.vectors-card dt{color:var(--text-muted)}.vectors-card dd{margin:0;color:var(--text);font-variant-numeric:tabular-nums}.vectors-card p{display:flex;align-items:center;gap:6px;margin:0;color:var(--text-secondary);font-size:9px}.vectors-card p :global(svg){color:var(--success)}.error-banner{display:flex;align-items:flex-start;gap:8px;border:1px solid color-mix(in srgb,var(--danger) 25%,var(--border));border-radius:10px;padding:10px 12px;color:var(--danger);background:var(--danger-soft);font-size:10px;overflow-wrap:anywhere}
  @media(max-width:980px){.diagnostics-grid{grid-template-columns:1fr}.toolbar{align-items:stretch;flex-wrap:wrap}.toolbar label{flex:1}.privacy{width:100%;max-width:none;margin-left:0}.camera-stage.empty{min-height:240px}}
</style>
