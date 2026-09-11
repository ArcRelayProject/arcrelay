<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { ArrowRight, CheckCircle } from "phosphor-svelte";
  import { onMount } from "svelte";

  import BrandLogo from "../BrandLogo.svelte";
  import monitorIllustration from "../assets/illustrations/gaze-calibration-monitor.png";
  import type { CalibrationFlowPayload } from "../features/input-sharing/gazeCalibration";
  import { invoke } from "../ipc/client";

  const search = new URLSearchParams(window.location.search);
  const screenIndex = Number(search.get("screen") ?? "0");
  const isTauri = "__TAURI_INTERNALS__" in window;
  let lastFlowAt = Date.now();
  let flow: CalibrationFlowPayload = {
    sessionId: "",
    sourceDeviceId: "",
    targetDeviceId: "",
    stage: "idle",
    screenIndex: -1,
    nextScreenIndex: null,
    screenName: "",
    nextScreenName: null,
    u: 0.5,
    v: 0.5,
    dwellProgress: 0,
    current: 0,
    total: 0,
  };

  $: active = flow.screenIndex === screenIndex;
  $: incoming = flow.stage === "transition" && flow.nextScreenIndex === screenIndex;
  $: dotStyle = `left:${flow.u * 100}%;top:${flow.v * 100}%;--dwell:${flow.dwellProgress * 360}deg`;

  onMount(() => {
    let unlisten: (() => void) | undefined;
    if (isTauri) {
      void listen<CalibrationFlowPayload>("gaze-calibration-flow", (event) => {
        lastFlowAt = Date.now();
        flow = event.payload;
      }).then((dispose) => { unlisten = dispose; });
    } else {
      const stage = (search.get("stage") ?? "calibrating") as CalibrationFlowPayload["stage"];
      flow = {
        sessionId: "preview",
        sourceDeviceId: "preview-source",
        targetDeviceId: "preview-target",
        stage,
        screenIndex: stage === "transition" ? -1 : screenIndex,
        nextScreenIndex: stage === "transition" ? screenIndex : null,
        screenName: search.get("screenName") ?? "Studio Display",
        nextScreenName: stage === "transition" ? search.get("nextScreenName") ?? "Dell U2723QE" : null,
        u: Number(search.get("u") ?? "0.72"),
        v: Number(search.get("v") ?? "0.36"),
        dwellProgress: Number(search.get("dwell") ?? "0.68"),
        current: 7,
        total: 15,
      };
    }

    const staleWatchdog = window.setInterval(() => {
      if (isTauri && flow.sessionId && Date.now() - lastFlowAt > 3_000) {
        void getCurrentWebviewWindow().close();
      }
    }, 1_000);

    const cancel = (event: KeyboardEvent) => {
      if (event.key === "Escape" && flow.sessionId && flow.sourceDeviceId) {
        void invoke("request_gaze_calibration_cancel", {
          sessionId: flow.sessionId,
          sourceDeviceId: flow.sourceDeviceId,
        });
      }
    };
    window.addEventListener("keydown", cancel);
    return () => {
      unlisten?.();
      window.clearInterval(staleWatchdog);
      window.removeEventListener("keydown", cancel);
    };
  });
</script>

<main class:active class:paused={active && flow.stage === "paused"} class:incoming>
  <header>
    <div class="brand"><BrandLogo size={20} /><strong>ArcRelay</strong><span>眼动标定</span></div>
    <div class="escape">按 <kbd>Esc</kbd> 退出</div>
  </header>

  {#if active && (flow.stage === "calibrating" || flow.stage === "paused")}
    <div class="screen-label"><span class="live-dot"></span>{flow.screenName}</div>
    <div class="progress-track" aria-label={`标定进度 ${flow.current}/${flow.total}`}>
      <span style={`width:${flow.total ? flow.current / flow.total * 100 : 0}%`}></span>
    </div>
    <div class="target" style={dotStyle} aria-label="请注视这里">
      <span class="dwell-ring"></span>
      <span class="target-core"></span>
    </div>
    {#if flow.stage === "paused"}
      <section class="pause-card" role="status">
        <strong>暂时看不到你的面部</strong>
        <p>保持自然坐姿，让面部回到摄像头范围。识别恢复后会自动继续，进度不会丢失。</p>
        <span><i></i>正在等待恢复…</span>
      </section>
    {:else}
      <p class="instruction">把头转向圆点，保持自然，不需要点击</p>
      <span class="counter">{flow.current + 1} / {flow.total}</span>
    {/if}
  {:else if incoming}
    <section class="transition-card">
      <div class="monitor-flow" aria-hidden="true">
        <figure class="done-monitor">
          <div><img src={monitorIllustration} alt="" /><CheckCircle size={30} weight="fill" /></div>
          <figcaption>{flow.screenName}</figcaption>
        </figure>
        <ArrowRight class="flow-arrow" size={34} weight="bold" />
        <figure class="next-monitor">
          <div><img src={monitorIllustration} alt="" /></div>
          <figcaption>{flow.nextScreenName}</figcaption>
        </figure>
      </div>
      <strong>接下来，请看这块屏幕</strong>
      <p>{flow.nextScreenName}</p>
      <small>保持坐姿不变，圆点即将出现</small>
    </section>
  {:else}
    <div class="waiting"><span></span><p>请继续看正在标定的屏幕</p></div>
  {/if}

  <footer>画面仅在本机内存中处理，不会保存或上传</footer>
</main>
