<script lang="ts">
  import { translate as uiTranslate, t, language as uiLanguage } from "../../i18n";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import {
    ArrowClockwise,
    Camera,
    Check,
    CheckCircle,
    Crosshair,
    Eye,
    LockKey,
    Monitor,
    Play,
    Stop,
    Trash,
    WarningCircle,
    X,
  } from "phosphor-svelte";

  import { diagnosticMessage } from "../../diagnosticMessages";
  import { bridge } from "../../bridge";
  import AppSelect from "../../components/AppSelect.svelte";
  import calibrationMonitor from "../../assets/illustrations/gaze-calibration-monitor.png";
  import type { GazeCalibrationOverlayEvent } from "../../ipc/generated";
  import { SubscriptionScope, observeSnapshot } from "../../subscriptions";
  import type { DisplaySurface, GazeCamera, GazeStatus, RuntimeSnapshot } from "../../types";
  import {
    clampProgress,
    observationCanFusedCalibrate,
    observationCanHeadCalibrate,
    samplesAreStable,
    toStabilitySample,
    type StabilitySample,
  } from "./gazeCalibration";
  import GazeDiagnosticsPage from "./GazeDiagnosticsPage.svelte";

  export let snapshot: RuntimeSnapshot;
  export let notify: (message: string, error?: boolean) => void;

  type Phase = "ready" | "checking" | "calibrating" | "transition" | "complete";
  type Target = {
    display: DisplaySurface;
    screenIndex: number;
    u: number;
    v: number;
    label: string;
  };

  const DWELL_MS = 320;
  const PREFLIGHT_MS = 600;
  const FRESH_MS = 700;
  const SAMPLES_PER_TARGET = 5;
  const POINTS = [
    [0.18, 0.18, "左上"],
    [0.5, 0.18, "上方"],
    [0.82, 0.18, "右上"],
    [0.18, 0.5, "左侧"],
    [0.5, 0.5, "中央"],
    [0.82, 0.5, "右侧"],
    [0.18, 0.82, "左下"],
    [0.5, 0.82, "下方"],
    [0.82, 0.82, "右下"],
  ] as const;

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
  let pointSamples = 0;
  let lastCapturedInferenceFrame = -1;
  let ticker: number | undefined;
  let transitionTimer: number | undefined;
  let calibrationSessionId = "";
  let completedScreens: string[] = [];
  let refineDisplayId: string | null = null;
  let flowSending = false;
  let lastFlowSentAt = 0;
  let presenceName = "本机用户";
  let diagnosticsOpen = false;
  let camerasLoaded = false;

  $: running = Boolean(status && !["idle", "stopped", "failed"].includes(status.state));
  $: modelsReady = status?.modelPack.state === "ready";
  $: modelsBusy =
    status?.modelPack.state === "downloading" || status?.modelPack.state === "verifying";
  $: modelProgress = status?.modelPack.totalBytes
    ? Math.round((status.modelPack.downloadedBytes / status.modelPack.totalBytes) * 100)
    : 0;
  $: cameraOptions = cameras.map((camera) => ({ value: camera.id, label: camera.name }));
  $: target = targetIndex >= 0 ? targets[targetIndex] : null;
  $: allDisplays = Object.values(snapshot.configuration.layout?.displays ?? {})
    .filter((display) => snapshot.displayAvailability[display.displayId] !== "Offline")
    .sort((a, b) => a.deskRectUm.y - b.deskRectUm.y || a.deskRectUm.x - b.deskRectUm.x);
  $: deviceCount = new Set(allDisplays.map((display) => display.deviceId)).size;
  $: overallProgress = targets.length
    ? Math.round(((Math.max(0, targetIndex) + dwellProgress) / targets.length) * 100)
    : 0;
  $: faceReady = observationCanHeadCalibrate(status?.observation, status?.faceConfidence);
  $: samplingMode = t("头部与眼动融合采样 · 每点 {part0} 帧", $uiLanguage, {
    part0: SAMPLES_PER_TARGET,
  });
  $: liveTargetName = allDisplays.find(
    (display) => display.displayId === status?.target?.displayId,
  )?.name;
  $: calibratedDisplayIds = new Set(status?.calibratedDisplayIds ?? []);
  $: presenceLabel =
    status?.presenceState === "ownerPresent"
      ? "本人在场"
      : status?.presenceState === "absent"
        ? "无人"
        : status?.presenceState === "unknownPresent"
          ? "陌生人在场"
          : status?.presenceState === "multiplePeople"
            ? "多人在场"
            : "正在确认";
  $: presencePoseLabel =
    status?.presenceEnrollmentPose === "left"
      ? "请缓慢向左转头"
      : status?.presenceEnrollmentPose === "right"
        ? "请缓慢向右转头"
        : status?.presenceEnrollmentPose === "up"
          ? "请轻微抬头"
          : status?.presenceEnrollmentPose === "down"
            ? "请轻微低头"
            : "请正对摄像头";

  function screenIndex(display: DisplaySurface): number {
    return Object.values(snapshot.configuration.layout?.displays ?? {})
      .filter((candidate) => candidate.deviceId === display.deviceId)
      .sort(
        (a, b) => a.logicalBounds.y - b.logicalBounds.y || a.logicalBounds.x - b.logicalBounds.x,
      )
      .findIndex((candidate) => candidate.displayId === display.displayId);
  }

  function buildTargets(displayId: string | null = null): Target[] {
    return allDisplays
      .filter((display) => !displayId || display.displayId === displayId)
      .flatMap((display) =>
        POINTS.map(([u, v, label]) => ({
          display,
          screenIndex: Math.max(0, screenIndex(display)),
          u,
          v,
          label,
        })),
      );
  }

  function newSessionId(): string {
    return (
      globalThis.crypto?.randomUUID?.() ??
      `gaze-${Date.now()}-${Math.random().toString(16).slice(2)}`
    );
  }

  function ingestStatus(next: GazeStatus) {
    status = next;
    if (next.modelPack.state === "ready" && !camerasLoaded) {
      camerasLoaded = true;
      void refreshCameras();
    }
    if (
      ["idle", "stopped", "failed"].includes(next.state) &&
      next.cameraId &&
      cameras.some((camera) => camera.id === next.cameraId)
    ) {
      selectedCamera = next.cameraId;
    }
    if (next.inferredFrames === lastInferenceFrame) return;
    lastInferenceFrame = next.inferredFrames;
    const observation = next.observation;
    const canSample =
      phase === "calibrating"
        ? observationCanFusedCalibrate(observation, next.faceConfidence)
        : observationCanHeadCalibrate(observation, next.faceConfidence);
    if (observation && canSample) {
      lastObservationAt = performance.now();
      samples = [...samples, toStabilitySample(observation)].slice(-6);
    } else {
      samples = [];
      stableSince = 0;
      preflightStableSince = 0;
    }
  }

  async function refreshCameras() {
    if (status?.modelPack.state !== "ready") return;
    camerasLoaded = true;
    busy = true;
    try {
      cameras = await bridge.listGazeCameras();
      const savedCamera = status?.cameraId;
      if (savedCamera && cameras.some((camera) => camera.id === savedCamera))
        selectedCamera = savedCamera;
      else if (!selectedCamera || !cameras.some((camera) => camera.id === selectedCamera))
        selectedCamera = cameras[0]?.id ?? "";
      if (!cameras.length)
        notify(uiTranslate("没有检测到摄像头，请检查系统摄像头权限。", $uiLanguage), true);
    } catch (error) {
      notify(
        t("摄像头枚举失败：{part0}", $uiLanguage, { part0: diagnosticMessage(error, $uiLanguage) }),
        true,
      );
    } finally {
      busy = false;
    }
  }

  async function installModels() {
    busy = true;
    try {
      ingestStatus(await bridge.installGazeModels());
      notify(uiTranslate("眼动模型已安装并完成校验。", $uiLanguage));
    } catch (error) {
      notify(
        t("安装眼动模型失败：{part0}", $uiLanguage, {
          part0: diagnosticMessage(error, $uiLanguage),
        }),
        true,
      );
    } finally {
      busy = false;
    }
  }

  async function removeModels() {
    if (
      !window.confirm(
        uiTranslate("移除眼动模型组件？标定资料会保留，下次使用时需要重新下载。", $uiLanguage),
      )
    )
      return;
    busy = true;
    try {
      ingestStatus(await bridge.removeGazeModels());
      cameras = [];
      camerasLoaded = false;
      notify(uiTranslate("眼动模型已移除，标定资料仍然保留。", $uiLanguage));
    } catch (error) {
      notify(
        t("移除眼动模型失败：{part0}", $uiLanguage, {
          part0: diagnosticMessage(error, $uiLanguage),
        }),
        true,
      );
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
      notify(
        t("启动眼动追踪失败：{part0}", $uiLanguage, {
          part0: diagnosticMessage(error, $uiLanguage),
        }),
        true,
      );
      return false;
    } finally {
      busy = false;
    }
  }

  async function startDiagnostics(cameraId: string): Promise<boolean> {
    selectedCamera = cameraId;
    return start();
  }

  async function stop() {
    busy = true;
    try {
      await cancelCalibration(false);
      ingestStatus(await bridge.stopGazeTracking());
    } catch (error) {
      notify(diagnosticMessage(error, $uiLanguage), true);
    } finally {
      busy = false;
    }
  }

  async function resumeTracking() {
    if (await start())
      notify(uiTranslate("头部与眼动融合识别已启动，原有标定已恢复。", $uiLanguage));
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
      if (!targets.length) throw new Error("no online displays in the display layout");
      ingestStatus(await bridge.beginGazeCalibration(selectedCamera, refineDisplayId));
      targetIndex = 0;
      resetPointStability();
      phase = "calibrating";
      await sendFlow("calibrating", targets[0]);
    } catch (error) {
      await bridge.cancelGazeCalibration().catch(() => {});
      phase = "ready";
      notify(
        t("无法开始标定：{part0}", $uiLanguage, { part0: diagnosticMessage(error, $uiLanguage) }),
        true,
      );
    } finally {
      launchBusy = false;
    }
  }

  function flowEvent(
    stage: string,
    active: Target,
    next: Target | null = null,
  ): GazeCalibrationOverlayEvent {
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
      notify(
        t("无法在 {part0} 显示标定圆点：{part1}", $uiLanguage, {
          part0: active.display.name,
          part1: diagnosticMessage(error, $uiLanguage),
        }),
        true,
      );
      await cancelCalibration(false);
    } finally {
      flowSending = false;
    }
  }

  async function closeDevice(deviceId: string) {
    const display = allDisplays.find((item) => item.deviceId === deviceId);
    if (!display || !calibrationSessionId) return;
    await bridge
      .sendGazeCalibrationOverlay({
        ...flowEvent("close", { display, screenIndex: 0, u: 0.5, v: 0.5, label: "" }),
        targetDeviceId: deviceId,
        stage: "close",
        current: 0,
        total: 0,
      })
      .catch(() => {});
  }

  async function closeAllDevices() {
    await Promise.all(
      [...new Set(allDisplays.map((display) => display.deviceId))].map(closeDevice),
    );
  }

  function resetPointStability() {
    dwellProgress = 0;
    stableSince = 0;
    samples = [];
    lastObservationAt = 0;
    pointSamples = 0;
    lastCapturedInferenceFrame = -1;
  }

  async function captureTarget() {
    if (!target || captureBusy || phase !== "calibrating") return;
    if (lastCapturedInferenceFrame === lastInferenceFrame) return;
    lastCapturedInferenceFrame = lastInferenceFrame;
    captureBusy = true;
    try {
      const rect = target.display.deskRectUm;
      await bridge.captureGazeCalibrationSample(
        Math.round(rect.x + rect.width * target.u),
        Math.round(rect.y + rect.height * target.v),
      );
      pointSamples += 1;
      dwellProgress = 0.35 + (pointSamples / SAMPLES_PER_TARGET) * 0.65;
      if (pointSamples < SAMPLES_PER_TARGET) {
        await sendFlow("calibrating", target);
        return;
      }
      if (targetIndex + 1 >= targets.length) {
        ingestStatus(await bridge.finishGazeCalibration());
        completedScreens = [...new Set(targets.map((item) => item.display.name))];
        await closeAllDevices();
        targetIndex = -1;
        phase = "complete";
        notify(
          refineDisplayId
            ? t("{part0} 的头部与眼动标定已优化。", $uiLanguage, { part0: completedScreens[0] })
            : uiTranslate(
                "头部与眼动融合标定已完成。稳定看向另一块屏幕后会自动移动鼠标。",
                $uiLanguage,
              ),
        );
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
        resetPointStability();
        await sendFlow("calibrating", next);
      }
    } catch (error) {
      lastCapturedInferenceFrame = -1;
      stableSince = 0;
      dwellProgress = 0;
      notify(
        t("自动采样失败：{part0}", $uiLanguage, { part0: diagnosticMessage(error, $uiLanguage) }),
        true,
      );
      if (target) await sendFlow("paused", target);
    } finally {
      captureBusy = false;
    }
  }

  async function cancelCalibration(announce = true) {
    if (transitionTimer) clearTimeout(transitionTimer);
    transitionTimer = undefined;
    if (calibrationSessionId) await closeAllDevices();
    if (phase !== "ready" && phase !== "complete")
      await bridge.cancelGazeCalibration().catch(() => {});
    targets = [];
    targetIndex = -1;
    resetPointStability();
    phase = "ready";
    calibrationSessionId = "";
    refineDisplayId = null;
    if (announce) notify(uiTranslate("已退出标定，临时采样没有保存。", $uiLanguage), false);
  }

  async function clearCalibration() {
    if (!window.confirm(uiTranslate("删除当前摄像头和屏幕布局的眼动标定？", $uiLanguage))) return;
    try {
      ingestStatus(await bridge.clearGazeCalibration());
      phase = "ready";
      completedScreens = [];
    } catch (error) {
      notify(diagnosticMessage(error, $uiLanguage), true);
    }
  }

  async function beginPresenceEnrollment() {
    if (!presenceName.trim()) return;
    if (!running && !(await start())) return;
    busy = true;
    try {
      ingestStatus(await bridge.beginPresenceEnrollment(presenceName.trim()));
      notify(uiTranslate("请按提示依次完成正脸、左右转头和上下角度录入。", $uiLanguage));
    } catch (error) {
      notify(
        t("无法开始录入：{part0}", $uiLanguage, { part0: diagnosticMessage(error, $uiLanguage) }),
        true,
      );
    } finally {
      busy = false;
    }
  }

  async function cancelPresenceEnrollment() {
    ingestStatus(await bridge.cancelPresenceEnrollment());
  }

  async function clearPresenceProfile() {
    if (!window.confirm(uiTranslate("删除仅保存在本机的人脸模板？", $uiLanguage))) return;
    try {
      ingestStatus(await bridge.clearPresenceProfile());
    } catch (error) {
      notify(diagnosticMessage(error, $uiLanguage), true);
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
      const settleProgress = clampProgress((now - stableSince) / DWELL_MS);
      dwellProgress = settleProgress * 0.35 + (pointSamples / SAMPLES_PER_TARGET) * 0.65;
      void sendFlow("calibrating", target);
      if (settleProgress >= 1 && lastCapturedInferenceFrame !== lastInferenceFrame)
        void captureTarget();
    }
  }

  onMount(() => {
    const scope = new SubscriptionScope();
    void observeSnapshot(
      scope,
      (accept) => bridge.onGazeState(accept),
      () => bridge.getGazeStatus(),
      ingestStatus,
    ).catch((error) => notify(diagnosticMessage(error, $uiLanguage), true));
    if ("__TAURI_INTERNALS__" in window) {
      void scope.add(
        listen<string>("gaze-calibration-cancel", (event) => {
          if (!calibrationSessionId || event.payload === calibrationSessionId)
            void cancelCalibration();
        }),
      );
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
  {#if diagnosticsOpen}
    <GazeDiagnosticsPage
      {status}
      {cameras}
      {selectedCamera}
      displays={allDisplays}
      {busy}
      onStart={startDiagnostics}
      onStop={stop}
      onClose={() => (diagnosticsOpen = false)}
      {notify}
    />
  {:else}
    {#if !modelsReady}
      <article class="model-pack-card" class:error={status?.modelPack.state === "damaged"}>
        <div class="model-pack-icon"><Eye size={26} weight="duotone" /></div>
        <div>
          <span class="eyebrow">{uiTranslate("可选组件", $uiLanguage)}</span>
          <h2>
            {uiTranslate(
              modelsBusy
                ? status?.modelPack.state === "verifying"
                  ? "正在校验眼动模型"
                  : "正在下载眼动模型"
                : status?.modelPack.state === "damaged"
                  ? "眼动模型需要修复"
                  : "下载眼动模型后启用",
              $uiLanguage,
            )}
          </h2>
          <p>
            {uiTranslate(
              "约 36 MB，只下载一份且不随 CPU 架构重复；模型和摄像头画面均只在本机处理。",
              $uiLanguage,
            )}
          </p>
          {#if modelsBusy}
            <div class="model-download">
              <div class="bar"><span style={`width:${modelProgress}%`}></span></div>
              <b>{modelProgress}%</b>
            </div>
          {:else}
            <button class="start-button" disabled={busy} on:click={installModels}
              >{uiTranslate(
                status?.modelPack.state === "damaged" ? "重新下载" : "下载并启用",
                $uiLanguage,
              )}</button
            >
          {/if}
          {#if status?.modelPack.error}<small
              >{diagnosticMessage(status.modelPack.error, $uiLanguage)}</small
            >{/if}
        </div>
      </article>
    {/if}
    <div class="stepper" aria-label={uiTranslate("标定步骤", $uiLanguage)}>
      <span
        class:active={phase === "ready" || phase === "checking"}
        class:done={phase !== "ready" && phase !== "checking"}
        ><i>{phase === "ready" || phase === "checking" ? "1" : "✓"}</i>{uiTranslate(
          "准备",
          $uiLanguage,
        )}</span
      ><b></b>
      <span
        class:active={phase === "calibrating" || phase === "transition"}
        class:done={phase === "complete"}
        ><i>{phase === "complete" ? "✓" : "2"}</i>{uiTranslate("标定", $uiLanguage)}</span
      ><b></b>
      <span class:active={phase === "complete"}><i>3</i>{uiTranslate("完成", $uiLanguage)}</span>
    </div>

    {#if phase === "ready"}
      <article class="hero-card">
        <div class="hero-copy">
          <span class="eyebrow"
            ><Crosshair size={15} weight="bold" />{uiTranslate(
              "多设备头部与眼动融合标定",
              $uiLanguage,
            )}</span
          >
          <h2>{uiTranslate("依次注视每块屏幕上的九个位置", $uiLanguage)}</h2>
          <p>
            {uiTranslate(
              "ArcRelay 会同时学习头部方向和个人眼动偏差；每个点采集多帧，并在不确定时保持当前屏幕，避免误切。",
              $uiLanguage,
            )}
          </p>
          <label class="camera-field"
            ><span>{uiTranslate("用于标定的摄像头", $uiLanguage)}</span><AppSelect
              bind:value={selectedCamera}
              options={cameraOptions}
              disabled={!modelsReady || running || busy}
              placeholder={uiTranslate(
                modelsReady ? "未发现摄像头" : "请先下载眼动模型",
                $uiLanguage,
              )}
              aria-label={uiTranslate("用于标定的摄像头", $uiLanguage)}
            /></label
          >
          <div class="privacy-line">
            <LockKey size={15} weight="fill" /><span
              ><strong>{uiTranslate("完全本机处理", $uiLanguage)}</strong>　{uiTranslate(
                "只同步圆点位置与进度，摄像头画面和人脸特征不会离开本机。",
                $uiLanguage,
              )}</span
            >
          </div>
          <div class="hero-actions">
            {#if status?.calibrated && !running}
              <button
                class="start-button"
                disabled={!modelsReady || busy || !selectedCamera || !snapshot.configuration.layout}
                on:click={resumeTracking}
                ><Play size={17} weight="fill" />{uiTranslate(
                  busy ? "正在启动…" : "启动识别",
                  $uiLanguage,
                )}</button
              >
              <button class="text-button" disabled={busy} on:click={() => beginCalibration()}
                ><ArrowClockwise size={15} />{uiTranslate("全部重新标定", $uiLanguage)}</button
              >
            {:else if !status?.calibrated}
              <button
                class="start-button"
                disabled={!modelsReady || busy || !selectedCamera || !snapshot.configuration.layout}
                on:click={() => beginCalibration()}
                ><Play size={17} weight="fill" />{uiTranslate(
                  busy ? "正在启动…" : "开始标定",
                  $uiLanguage,
                )}</button
              >
            {:else}
              <button class="text-button" disabled={busy} on:click={() => beginCalibration()}
                ><ArrowClockwise size={15} />{uiTranslate("全部重新标定", $uiLanguage)}</button
              >
            {/if}
            <button
              class="text-button"
              disabled={!modelsReady || busy}
              on:click={() => (diagnosticsOpen = true)}
              ><Camera size={15} />{uiTranslate("视觉诊断", $uiLanguage)}</button
            >
            <button class="text-button" disabled={!modelsReady || busy} on:click={refreshCameras}
              ><ArrowClockwise size={15} />{uiTranslate("重新检测摄像头", $uiLanguage)}</button
            >
            {#if modelsReady}<button
                class="text-button"
                disabled={busy || running}
                on:click={removeModels}
                ><Trash size={15} />{uiTranslate("移除模型", $uiLanguage)}</button
              >{/if}
          </div>
        </div>
        <div class="preview-panel" aria-hidden="true">
          <img src={calibrationMonitor} alt="" />
          <div class="preview-meta">
            <span
              ><Monitor size={15} />{allDisplays.length} {uiTranslate("块屏幕", $uiLanguage)}</span
            ><span>{deviceCount} {uiTranslate("台设备", $uiLanguage)}</span><span
              ><Eye size={15} />{uiTranslate("头部 + 眼动", $uiLanguage)}</span
            >
          </div>
        </div>
      </article>
      <div class="readiness-grid">
        <section>
          <span class:ok={Boolean(cameras.length)}
            >{#if cameras.length}<Check size={16} weight="bold" />{:else}<WarningCircle
                size={16}
              />{/if}</span
          >
          <div>
            <strong>{uiTranslate("摄像头", $uiLanguage)}</strong>
            <p>
              {cameras.length
                ? t("{part0} 个设备可用", $uiLanguage, { part0: cameras.length })
                : uiTranslate("等待摄像头权限", $uiLanguage)}
            </p>
          </div>
        </section>
        <section>
          <span class:ok={Boolean(allDisplays.length)}
            >{#if allDisplays.length}<Check size={16} weight="bold" />{:else}<WarningCircle
                size={16}
              />{/if}</span
          >
          <div>
            <strong>{uiTranslate("标定范围", $uiLanguage)}</strong>
            <p>
              {allDisplays.length}
              {uiTranslate("块屏幕 ·", $uiLanguage)}
              {deviceCount}
              {uiTranslate("台设备", $uiLanguage)}
            </p>
          </div>
        </section>
        <section>
          <span class:ok={running}
            >{#if running}<Check size={16} weight="bold" />{:else}<Camera size={16} />{/if}</span
          >
          <div>
            <strong>{uiTranslate("眼动服务", $uiLanguage)}</strong>
            <p>
              {running
                ? t("{part0} 正在运行", $uiLanguage, { part0: status?.cameraName ?? "摄像头" })
                : uiTranslate("开始标定时自动启动", $uiLanguage)}
            </p>
          </div>
        </section>
      </div>
      {#if status?.calibrated}
        <section class="existing-profile">
          <CheckCircle size={18} weight="fill" />
          <div class="profile-copy">
            <strong>{uiTranslate("标定已保存，重启后可直接恢复", $uiLanguage)}</strong>
            <p>
              {uiTranslate("某块屏幕识别不准时，只优化这一块即可保留其他屏幕的数据。", $uiLanguage)}
            </p>
          </div>
          <div class="profile-screens">
            {#each allDisplays as display}
              <span class:missing={!calibratedDisplayIds.has(display.displayId)}>
                <Monitor size={14} /><b>{display.name}</b>
                <button
                  disabled={busy || !selectedCamera}
                  on:click={() => beginCalibration(display.displayId)}
                  >{uiTranslate(
                    calibratedDisplayIds.has(display.displayId) ? "优化标定" : "补充标定",
                    $uiLanguage,
                  )}</button
                >
              </span>
            {/each}
          </div>
          <button class="delete-profile" on:click={clearCalibration}
            ><Trash size={15} />{uiTranslate("删除全部", $uiLanguage)}</button
          >
        </section>
      {/if}
      <section class:recognized={status?.presenceState === "ownerPresent"} class="presence-card">
        <div class="recognition-icon"><LockKey size={22} weight="duotone" /></div>
        <div class="profile-copy">
          <strong>{uiTranslate("本机用户在场识别", $uiLanguage)}</strong>
          <p>
            {uiTranslate(
              status?.presenceProfileEnrolled
                ? `${status.presenceProfileName ?? uiTranslate("本机用户", $uiLanguage)} · ${uiTranslate(presenceLabel, $uiLanguage)}`
                : "录入后可保护隐私遮罩、通知预览并触发自动化。",
              $uiLanguage,
            )}
          </p>
        </div>
        {#if status?.presenceEnrollmentActive}
          <div class="presence-progress">
            <span
              >{uiTranslate(presencePoseLabel, $uiLanguage)} · {status.presenceEnrollmentSamples}/{status.presenceEnrollmentRequiredSamples}</span
            >
            <div class="bar">
              <span
                style={`width:${(status.presenceEnrollmentSamples / Math.max(1, status.presenceEnrollmentRequiredSamples)) * 100}%`}
              ></span>
            </div>
          </div>
          <button class="text-button" on:click={cancelPresenceEnrollment}
            >{uiTranslate("取消", $uiLanguage)}</button
          >
        {:else if status?.presenceProfileEnrolled}
          <div class="presence-state">
            <b>{uiTranslate(presenceLabel, $uiLanguage)}</b><span
              >{status.presenceFaceCount}
              {uiTranslate("张人脸", $uiLanguage)}{status.presenceOwnerSimilarity == null
                ? ""
                : t(" · 相似度 {part0}", $uiLanguage, {
                    part0: status.presenceOwnerSimilarity.toFixed(2),
                  })}</span
            >
          </div>
          <button class="delete-profile" on:click={clearPresenceProfile}
            ><Trash size={15} />{uiTranslate("删除模板", $uiLanguage)}</button
          >
        {:else}
          <label class="presence-name"
            ><span>{uiTranslate("显示名称", $uiLanguage)}</span><input
              bind:value={presenceName}
              maxlength="80"
            /></label
          >
          <button
            class="start-button"
            disabled={busy || !selectedCamera}
            on:click={beginPresenceEnrollment}>{uiTranslate("开始录入", $uiLanguage)}</button
          >
        {/if}
      </section>
    {:else if phase === "checking"}
      <article class="state-card">
        <div class="check-visual" style={`--check-progress:${preflightProgress * 360}deg`}>
          <Eye size={34} weight="duotone" />
        </div>
        <span class="eyebrow">{uiTranslate("自动环境检查", $uiLanguage)}</span>
        <h2>{uiTranslate(faceReady ? "很好，请保持这个姿势" : "请面向摄像头", $uiLanguage)}</h2>
        <p>
          {uiTranslate(
            faceReady
              ? "正在确认滤波后的头部方向稳定，完成后会自动进入跨设备全屏标定。"
              : "让面部保持在画面中，坐姿自然，允许较大角度偏转。",
            $uiLanguage,
          )}
        </p>
        <div class="check-list">
          <span class:ok={Boolean(status?.faceConfidence && status.faceConfidence >= 0.6)}
            ><i></i>{uiTranslate("面部清晰", $uiLanguage)}</span
          ><span class:ok={Boolean(status?.observation)}
            ><i></i>{uiTranslate("头部方向有效", $uiLanguage)}</span
          ><span class:ok={faceReady}><i></i>{uiTranslate("姿态稳定", $uiLanguage)}</span>
        </div>
        <div class="bar"><span style={`width:${preflightProgress * 100}%`}></span></div>
        <button class="cancel-button" on:click={() => cancelCalibration()}
          ><X size={15} />{uiTranslate("取消", $uiLanguage)}</button
        >
      </article>
    {:else if phase === "calibrating" || phase === "transition"}
      <article class="state-card">
        <div class="running-icon"><Crosshair size={30} weight="duotone" /></div>
        <span class="eyebrow">{uiTranslate("跨设备全屏标定", $uiLanguage)}</span>
        <h2>
          {phase === "transition"
            ? uiTranslate("正在切换到下一块屏幕", $uiLanguage)
            : t("请看向 {part0} 上的圆点", $uiLanguage, {
                part0: target?.display.name ?? uiTranslate("屏幕", $uiLanguage),
              })}
        </h2>
        <p>
          {phase === "transition"
            ? uiTranslate("下一台设备会自动显示引导，不需要移动或点击窗口。", $uiLanguage)
            : t("{part0} · 自然注视圆点并保持坐姿；面部或眼动不稳定时会自动暂停。", $uiLanguage, {
                part0: samplingMode,
              })}
        </p>
        <div class="bar"><span style={`width:${overallProgress}%`}></span></div>
        <strong class="progress-label"
          >{overallProgress}% · {targetIndex + 1}/{targets.length}</strong
        >
        <button class="cancel-button" on:click={() => cancelCalibration()}
          ><Stop size={15} />{uiTranslate("退出标定", $uiLanguage)}</button
        >
      </article>
    {:else}
      <article class="state-card complete-card">
        <div class="complete-icon"><Check size={38} weight="bold" /></div>
        <span class="eyebrow">{uiTranslate("标定完成", $uiLanguage)}</span>
        <h2>{uiTranslate("看向屏幕即可移动鼠标", $uiLanguage)}</h2>
        <p>
          {uiTranslate(
            "ArcRelay 会融合头部方向与眼动选择屏幕；只有结果明确且持续稳定时才会切换，不确定时保持当前屏幕。",
            $uiLanguage,
          )}
        </p>
        <div class="screen-results">
          {#each completedScreens as name}<span
              ><Monitor size={16} /><strong>{name}</strong><i><Check size={13} weight="bold" /></i
              ></span
            >{/each}
        </div>
        <div class="hero-actions">
          <button class="start-button" on:click={() => (phase = "ready")}
            >{uiTranslate("返回眼动设置", $uiLanguage)}</button
          ><button class="text-button" on:click={() => beginCalibration()}
            ><ArrowClockwise size={15} />{uiTranslate("全部重新标定", $uiLanguage)}</button
          >
        </div>
      </article>
    {/if}

    {#if phase === "ready"}
      <section
        class:recognized={Boolean(running && status?.target)}
        class="recognition-card"
        role="status"
      >
        <div class="recognition-icon">
          <Eye size={22} weight={status?.target ? "fill" : "duotone"} />
        </div>
        <div class="recognition-copy">
          <span
            >{uiTranslate(
              running ? "实时识别结果" : status?.calibrated ? "标定已就绪" : "等待标定",
              $uiLanguage,
            )}</span
          >
          <strong
            >{status?.target
              ? t("正在看向 {part0}", $uiLanguage, {
                  part0: liveTargetName ?? uiTranslate("已标定屏幕", $uiLanguage),
                })
              : running
                ? uiTranslate("正在确认头部方向…", $uiLanguage)
                : status?.calibrated
                  ? uiTranslate("启动后显示正在看的屏幕", $uiLanguage)
                  : uiTranslate("完成标定后显示识别结果", $uiLanguage)}</strong
          >
        </div>
        <div class="recognition-screens" aria-label={uiTranslate("屏幕识别状态", $uiLanguage)}>
          {#each allDisplays as display}
            <span class:active={status?.target?.displayId === display.displayId}
              ><Monitor size={15} /><b>{display.name}</b
              >{#if status?.target?.displayId === display.displayId}<i
                  >{uiTranslate("正在看", $uiLanguage)}</i
                >{/if}</span
            >
          {/each}
        </div>
        <dl>
          <div>
            <dt>{uiTranslate("推理延迟", $uiLanguage)}</dt>
            <dd>{status?.inferenceMs?.toFixed(0) ?? "—"} ms</dd>
          </div>
          <div>
            <dt>{uiTranslate("人脸置信度", $uiLanguage)}</dt>
            <dd>{status?.faceConfidence?.toFixed(2) ?? "—"}</dd>
          </div>
        </dl>
        {#if running}<button class="stop-tracking" on:click={stop} disabled={busy}
            ><Stop size={14} />{uiTranslate("停止", $uiLanguage)}</button
          >{/if}
      </section>
    {/if}
    {#if status?.error}<div class="error-banner">
        <WarningCircle size={17} /><span>{diagnosticMessage(status.error, $uiLanguage)}</span>
      </div>{/if}
    <div class="safety-note">
      <strong>{uiTranslate("自动切屏：", $uiLanguage)}</strong>{uiTranslate(
        "稳定看向另一块屏幕后，ArcRelay 会移动鼠标到屏幕中央；按 ⌘⌥⇧ Esc 可随时紧急释放，不会自动点击或输入。",
        $uiLanguage,
      )}
    </div>
  {/if}
</section>

<style>
  .gaze-page {
    display: grid;
    gap: 12px;
    padding: 2px 0 22px;
  }
  .model-pack-card {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: 16px;
    border: 1px solid color-mix(in srgb, var(--accent) 35%, var(--border));
    border-radius: 14px;
    padding: 17px 19px;
    background: color-mix(in srgb, var(--accent-soft) 45%, var(--surface-raised));
  }
  .model-pack-card.error {
    border-color: color-mix(in srgb, var(--danger) 38%, var(--border));
    background: color-mix(in srgb, var(--danger-soft) 35%, var(--surface-raised));
  }
  .model-pack-icon {
    display: grid;
    width: 48px;
    height: 48px;
    place-items: center;
    border-radius: 13px;
    color: var(--accent);
    background: var(--surface-raised);
  }
  .model-pack-card h2 {
    margin: 3px 0;
    color: var(--text);
    font-size: 15px;
  }
  .model-pack-card p {
    margin: 0 0 10px;
    color: var(--text-secondary);
    font-size: 10px;
  }
  .model-pack-card small {
    display: block;
    margin-top: 8px;
    color: var(--danger);
    font-size: 9px;
  }
  .model-download {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .model-download .bar {
    width: min(420px, 70vw);
    margin: 0;
  }
  .model-download b {
    min-width: 34px;
    color: var(--accent-strong);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }
  .stepper {
    display: grid;
    grid-template-columns: auto minmax(28px, 82px) auto minmax(28px, 82px) auto;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 2px 0 5px;
    color: var(--text-muted);
    font-size: 11px;
    font-weight: 650;
  }
  .stepper span {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    white-space: nowrap;
  }
  .stepper i {
    display: grid;
    width: 22px;
    height: 22px;
    place-items: center;
    border: 1px solid var(--border-strong);
    border-radius: 50%;
    background: var(--surface);
    font-style: normal;
    font-size: 10px;
  }
  .stepper b {
    height: 1px;
    background: var(--border);
  }
  .stepper span.active {
    color: var(--accent-strong);
  }
  .stepper span.active i {
    border-color: var(--accent);
    color: #fff;
    background: var(--accent);
  }
  .stepper span.done i {
    border-color: var(--success);
    color: #fff;
    background: var(--success);
  }
  .hero-card {
    display: grid;
    grid-template-columns: minmax(0, 1.12fr) minmax(310px, 0.88fr);
    gap: 32px;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 18px;
    padding: 26px 28px;
    background: var(--surface-raised);
    box-shadow: var(--shadow-card);
  }
  .hero-copy {
    align-self: center;
  }
  .eyebrow {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--accent-strong);
    font-size: 11px;
    font-weight: 750;
    letter-spacing: 0.02em;
  }
  .hero-card h2,
  .state-card h2 {
    margin: 8px 0 7px;
    color: var(--text);
    font-size: 23px;
    font-weight: 760;
    line-height: 1.18;
    letter-spacing: -0.035em;
  }
  .hero-copy > p,
  .state-card > p {
    max-width: 590px;
    margin: 0;
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 1.7;
  }
  .camera-field {
    display: grid;
    gap: 7px;
    max-width: 420px;
    margin-top: 20px;
    color: var(--text-secondary);
    font-size: 11px;
  }
  .privacy-line {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    max-width: 540px;
    margin-top: 13px;
    color: var(--text-muted);
    font-size: 10px;
    line-height: 1.55;
  }
  .privacy-line :global(svg) {
    flex: none;
    margin-top: 1px;
    color: var(--success);
  }
  .privacy-line strong {
    color: var(--text-secondary);
  }
  .hero-actions {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 15px;
    margin-top: 20px;
  }
  .hero-copy .hero-actions {
    justify-content: flex-start;
  }
  .start-button {
    display: inline-flex;
    min-height: 39px;
    align-items: center;
    justify-content: center;
    gap: 8px;
    border: 1px solid var(--accent);
    border-radius: 10px;
    padding: 0 17px;
    color: #fff;
    background: var(--accent);
    font-size: 12px;
    font-weight: 720;
    box-shadow: 0 8px 18px color-mix(in srgb, var(--accent) 20%, transparent);
  }
  .start-button:disabled {
    border-color: var(--border-strong);
    color: var(--text-muted);
    background: var(--surface-sunken);
    box-shadow: none;
  }
  .text-button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 0;
    padding: 8px 0;
    color: var(--text-secondary);
    background: transparent;
    font-size: 11px;
  }
  .preview-panel {
    display: grid;
    align-content: center;
    gap: 8px;
    padding: 0 4px;
  }
  .preview-panel img {
    display: block;
    width: 100%;
    max-height: 255px;
    object-fit: contain;
    filter: drop-shadow(0 16px 22px rgba(31, 33, 47, 0.12));
  }
  .preview-meta {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    color: var(--text-muted);
    font-size: 10px;
  }
  .preview-meta span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .readiness-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 10px;
  }
  .readiness-grid section {
    display: flex;
    align-items: center;
    gap: 11px;
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 12px 14px;
    background: var(--surface-raised);
  }
  .readiness-grid section > span {
    display: grid;
    width: 28px;
    height: 28px;
    flex: none;
    place-items: center;
    border-radius: 9px;
    color: var(--text-muted);
    background: var(--surface-sunken);
  }
  .readiness-grid section > span.ok {
    color: var(--success);
    background: var(--success-soft);
  }
  .readiness-grid strong {
    display: block;
    color: var(--text);
    font-size: 11px;
  }
  .readiness-grid p {
    margin: 2px 0 0;
    color: var(--text-muted);
    font-size: 10px;
  }
  .existing-profile {
    display: grid;
    grid-template-columns: auto minmax(170px, 0.75fr) minmax(300px, 1.25fr) auto;
    align-items: center;
    gap: 12px;
    border: 1px solid color-mix(in srgb, var(--success) 28%, var(--border));
    border-radius: 12px;
    padding: 12px 13px;
    background: color-mix(in srgb, var(--success-soft) 55%, var(--surface));
  }
  .existing-profile > :global(svg) {
    color: var(--success);
  }
  .profile-copy strong {
    display: block;
    color: var(--text);
    font-size: 11px;
  }
  .profile-copy p {
    margin: 2px 0 0;
    color: var(--text-secondary);
    font-size: 10px;
  }
  .profile-screens {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .profile-screens > span {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 1px solid color-mix(in srgb, var(--success) 22%, var(--border));
    border-radius: 8px;
    padding: 5px 6px 5px 8px;
    color: var(--text-secondary);
    background: var(--surface);
    font-size: 9px;
  }
  .profile-screens > span.missing {
    border-color: color-mix(in srgb, var(--warning) 32%, var(--border));
  }
  .profile-screens b {
    max-width: 120px;
    overflow: hidden;
    color: var(--text);
    font-size: 9px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .profile-screens button,
  .delete-profile {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 1px solid var(--border-strong);
    border-radius: 7px;
    padding: 5px 7px;
    color: var(--text-secondary);
    background: var(--surface);
    font-size: 9px;
  }
  .profile-screens button {
    border: 0;
    color: var(--accent-strong);
    background: var(--accent-soft);
  }
  .presence-card {
    display: grid;
    grid-template-columns: auto minmax(180px, 1fr) minmax(220px, 0.8fr) auto;
    align-items: center;
    gap: 12px;
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 12px 13px;
    background: var(--surface-raised);
  }
  .presence-card.recognized {
    border-color: color-mix(in srgb, var(--success) 35%, var(--border));
    background: color-mix(in srgb, var(--success-soft) 45%, var(--surface));
  }
  .presence-name {
    display: grid;
    gap: 4px;
    color: var(--text-muted);
    font-size: 9px;
  }
  .presence-name input {
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 7px 9px;
    color: var(--text);
    background: var(--surface);
  }
  .presence-state,
  .presence-progress {
    display: grid;
    gap: 3px;
    color: var(--text-secondary);
    font-size: 9px;
  }
  .presence-state b {
    color: var(--text);
    font-size: 11px;
  }
  .presence-progress .bar {
    width: 100%;
    margin: 3px 0 0;
  }
  .presence-card .start-button {
    min-height: 34px;
    font-size: 10px;
  }
  .state-card {
    display: grid;
    min-height: 390px;
    place-content: center;
    justify-items: center;
    border: 1px solid var(--border);
    border-radius: 18px;
    padding: 34px;
    background: var(--surface-raised);
    box-shadow: var(--shadow-card);
    text-align: center;
  }
  .check-visual {
    position: relative;
    display: grid;
    width: 94px;
    height: 94px;
    margin-bottom: 20px;
    place-items: center;
    border-radius: 50%;
    color: var(--accent);
    background: conic-gradient(var(--accent) var(--check-progress), var(--border) 0);
  }
  .check-visual:after {
    position: absolute;
    inset: 7px;
    border-radius: 50%;
    background: var(--surface-raised);
    content: "";
  }
  .check-visual :global(svg) {
    position: relative;
    z-index: 1;
  }
  .check-list {
    display: flex;
    gap: 8px;
    margin-top: 25px;
  }
  .check-list span {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 7px 10px;
    color: var(--text-muted);
    background: var(--surface-soft);
    font-size: 10px;
  }
  .check-list i {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--border-strong);
  }
  .check-list span.ok {
    color: var(--success);
    background: var(--success-soft);
  }
  .check-list span.ok i {
    background: var(--success);
  }
  .bar {
    width: min(390px, 70vw);
    height: 5px;
    margin-top: 22px;
    overflow: hidden;
    border-radius: 99px;
    background: var(--surface-sunken);
  }
  .bar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
    transition: width 0.1s linear;
  }
  .cancel-button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-top: 20px;
    border: 0;
    color: var(--text-muted);
    background: transparent;
    font-size: 10px;
  }
  .running-icon {
    display: grid;
    width: 76px;
    height: 76px;
    margin-bottom: 18px;
    place-items: center;
    border-radius: 50%;
    color: var(--accent);
    background: var(--accent-soft);
    box-shadow: 0 0 0 12px color-mix(in srgb, var(--accent-soft) 55%, transparent);
  }
  .progress-label {
    margin-top: 8px;
    color: var(--accent-strong);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .complete-icon {
    display: grid;
    width: 84px;
    height: 84px;
    margin-bottom: 18px;
    place-items: center;
    border-radius: 50%;
    color: #fff;
    background: var(--success);
    box-shadow: 0 0 0 12px var(--success-soft);
  }
  .screen-results {
    display: flex;
    max-width: 680px;
    flex-wrap: wrap;
    justify-content: center;
    gap: 8px;
    margin-top: 24px;
  }
  .screen-results > span {
    display: flex;
    min-width: 180px;
    align-items: center;
    gap: 8px;
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 10px 12px;
    color: var(--text-secondary);
    background: var(--surface-soft);
    font-size: 10px;
  }
  .screen-results strong {
    flex: 1;
    color: var(--text);
    font-size: 11px;
    text-align: left;
  }
  .screen-results i {
    display: grid;
    width: 20px;
    height: 20px;
    place-items: center;
    border-radius: 50%;
    color: #fff;
    background: var(--success);
  }
  .recognition-card {
    display: grid;
    grid-template-columns: auto minmax(155px, 0.6fr) minmax(280px, 1.4fr) auto auto;
    align-items: center;
    gap: 12px;
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 13px 15px;
    background: var(--surface-raised);
    box-shadow: var(--shadow-card);
  }
  .recognition-card.recognized {
    border-color: color-mix(in srgb, var(--accent) 42%, var(--border));
    background: linear-gradient(
      110deg,
      color-mix(in srgb, var(--accent-soft) 62%, var(--surface-raised)),
      var(--surface-raised) 45%
    );
  }
  .recognition-icon {
    display: grid;
    width: 38px;
    height: 38px;
    place-items: center;
    border-radius: 11px;
    color: var(--text-muted);
    background: var(--surface-sunken);
  }
  .recognized .recognition-icon {
    color: var(--accent);
    background: var(--accent-soft);
    box-shadow: 0 0 0 4px color-mix(in srgb, var(--accent-soft) 45%, transparent);
  }
  .recognition-copy span {
    display: block;
    color: var(--text-muted);
    font-size: 9px;
    font-weight: 700;
  }
  .recognition-copy strong {
    display: block;
    margin-top: 2px;
    color: var(--text);
    font-size: 13px;
  }
  .recognition-screens {
    display: flex;
    min-width: 0;
    flex-wrap: wrap;
    gap: 6px;
  }
  .recognition-screens > span {
    display: inline-flex;
    min-width: 0;
    align-items: center;
    gap: 5px;
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 6px 8px;
    color: var(--text-muted);
    background: var(--surface-soft);
    font-size: 9px;
  }
  .recognition-screens > span.active {
    border-color: var(--accent);
    color: var(--accent-strong);
    background: var(--accent-soft);
  }
  .recognition-screens b {
    max-width: 115px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .recognition-screens i {
    border-radius: 99px;
    padding: 2px 5px;
    color: #fff;
    background: var(--accent);
    font-style: normal;
    font-size: 8px;
  }
  .recognition-card dl {
    display: flex;
    gap: 14px;
    margin: 0;
  }
  .recognition-card dl div {
    display: grid;
    gap: 2px;
    font-size: 8px;
  }
  .recognition-card dt {
    color: var(--text-muted);
  }
  .recognition-card dd {
    margin: 0;
    color: var(--text);
    font-size: 9px;
    font-variant-numeric: tabular-nums;
  }
  .stop-tracking {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    padding: 7px 9px;
    color: var(--text-secondary);
    background: var(--surface);
    font-size: 9px;
  }
  .error-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    border: 1px solid color-mix(in srgb, var(--danger) 25%, var(--border));
    border-radius: 10px;
    padding: 9px 12px;
    color: var(--danger);
    background: var(--danger-soft);
    font-size: 10px;
  }
  .safety-note {
    border: 1px solid color-mix(in srgb, var(--accent) 22%, var(--border));
    border-radius: 10px;
    padding: 9px 12px;
    color: var(--text-secondary);
    background: color-mix(in srgb, var(--accent-soft) 48%, var(--surface));
    font-size: 10px;
  }
  .safety-note strong {
    color: var(--text);
  }
  @media (max-width: 900px) {
    .hero-card {
      grid-template-columns: 1fr;
    }
    .preview-panel {
      display: none;
    }
    .readiness-grid {
      grid-template-columns: 1fr;
    }
    .hero-card h2,
    .state-card h2 {
      font-size: 20px;
    }
    .stepper {
      grid-template-columns: auto 30px auto 30px auto;
    }
    .existing-profile,
    .recognition-card {
      grid-template-columns: auto 1fr;
    }
    .profile-screens,
    .recognition-screens {
      grid-column: 1/-1;
    }
    .recognition-card dl {
      display: none;
    }
  }
</style>
