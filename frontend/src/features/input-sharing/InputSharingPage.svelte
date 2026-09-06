<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../../i18n";
  import { onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { ArrowsOutCardinal, ArrowLeft, Bug, GridFour, Keyboard, Power } from "phosphor-svelte";

  import { SubscriptionScope, observeSnapshot } from "../../subscriptions";
  import { bridge } from "../../bridge";
  import type { RuntimeSnapshot } from "../../types";
  import DiagnosticsPage from "./DiagnosticsPage.svelte";
  import InputOverviewPage from "./InputOverviewPage.svelte";
  import InputSetupFlow from "./InputSetupFlow.svelte";
  import InputWorkspacePage from "./InputWorkspacePage.svelte";
  import KeyboardPage from "./KeyboardPage.svelte";
  import GestureDebugPage from "./GestureDebugPage.svelte";
  import {
    canTakeInputControl,
    friendlyDeviceName,
    recoverableOfflinePeer,
    workspaceIsReady,
  } from "./inputSharingUi";

  export let notify: (message: string, error?: boolean) => void;

  type Section = "overview" | "workspace" | "keyboard" | "diagnostics" | "gestureDebug";

  // Keep the local gesture lab implementation available for future diagnostics,
  // but do not expose or route into it from the product UI for now.
  const gestureLabVisible = false;
  let section: Section = gestureLabVisible && new URLSearchParams(window.location.search).get("inputDebug") === "gestures" ? "gestureDebug"
    : !bridge.isTauri() && new URLSearchParams(window.location.search).get("inputSection") === "diagnostics" ? "diagnostics" : "overview";
  let snapshot: RuntimeSnapshot | null = null;
  let loading = true;
  let busy = false;
  let showSetup = false;
  let setupInitialStep: 1 | 2 | 3 | 4 = 1;

  const sections = [
    { id: "overview" as const, label: "总览", icon: GridFour },
    { id: "workspace" as const, label: "布局与通道", icon: ArrowsOutCardinal },
    { id: "keyboard" as const, label: "键盘行为", icon: Keyboard },
    { id: "diagnostics" as const, label: "诊断", icon: Bug },
  ];

  $: enabled = Boolean(snapshot?.configuration.inputSharingEnabled);
  $: controlling = Boolean(snapshot && snapshot.controller === snapshot.serviceInstanceId);
  $: remoteController = snapshot?.controller && snapshot.controller !== snapshot.serviceInstanceId
    ? friendlyDeviceName(snapshot.controller, snapshot)
    : null;
  $: ready = snapshot ? workspaceIsReady(snapshot) : false;
  $: offlinePeer = snapshot ? recoverableOfflinePeer(snapshot) : null;
  $: status = !enabled ? "已关闭" : controlling || remoteController ? "输入活动" : offlinePeer ? "需要处理" : ready ? "自动互通" : "待设置";

  async function refresh(silent = false) {
    try {
      applySnapshot(await bridge.getInputRuntimeSnapshot());
      const previewOnboarding = !bridge.isTauri() && new URLSearchParams(window.location.search).get("inputScenario") === "onboarding";
      if (snapshot && !snapshot.configuration.layout && (previewOnboarding || window.localStorage.getItem("arc-input-onboarding-complete") !== "1")) {
        setupInitialStep = 1;
        showSetup = true;
      }
    } catch (error) {
      if (!silent) notify(String(error), true);
    } finally {
      loading = false;
    }
  }

  async function toggleInputSharing() {
    if (!snapshot) return;
    busy = true;
    try {
      applySnapshot(await bridge.setInputSharingEnabled(!enabled));
      notify(snapshot.configuration.inputSharingEnabled ? "跨屏输入已启用，任一设备都可自然接管" : "跨屏输入已关闭，输入已安全释放");
    } catch (error) {
      notify(String(error), true);
    } finally {
      busy = false;
    }
  }

  async function reconnectOfflinePeer() {
    if (!offlinePeer) return;
    busy = true;
    try {
      applySnapshot(await bridge.connectInputPeer(offlinePeer));
      notify(`已重新连接${friendlyDeviceName(offlinePeer.serviceInstanceId, snapshot!)}`);
    } catch (error) {
      notify(`重新连接失败：${String(error)}`, true);
    } finally {
      busy = false;
    }
  }

  async function forgetOfflinePeer() {
    if (!offlinePeer || !window.confirm(`移除${friendlyDeviceName(offlinePeer.serviceInstanceId, snapshot!)}？\n\n将同时删除该设备的屏幕布局与跨屏边缘。`)) return;
    busy = true;
    try {
      applySnapshot(await bridge.forgetInputPeer(offlinePeer.serviceInstanceId));
      notify("设备配对、屏幕布局和相关边缘已移除");
    } catch (error) {
      notify(`移除设备失败：${String(error)}`, true);
    } finally {
      busy = false;
    }
  }

  function openSetup(step: 1 | 2 | 3 | 4 = 1) {
    setupInitialStep = step;
    showSetup = true;
  }

  function navigate(next: "workspace" | "keyboard" | "diagnostics") {
    section = next;
  }

  function applySnapshot(next: RuntimeSnapshot) {
    if (!snapshot || next.revision > snapshot.revision) snapshot = next;
  }
  onMount(() => {
    const scope = new SubscriptionScope();
    void observeSnapshot(scope,
      (accept) => bridge.isTauri() ? listen<RuntimeSnapshot>("arc-input-state", (event) => accept(event.payload)) : Promise.resolve(() => {}),
      () => bridge.getInputRuntimeSnapshot(),
      (next) => { applySnapshot(next); loading = false; },
    ).then(() => {
      if (scope.disposed || !snapshot) return;
      const previewOnboarding = !bridge.isTauri() && new URLSearchParams(window.location.search).get("inputScenario") === "onboarding";
      if (snapshot && !snapshot.configuration.layout && (previewOnboarding || window.localStorage.getItem("arc-input-onboarding-complete") !== "1")) { setupInitialStep = 1; showSetup = true; }
    }).catch((error) => { if (!scope.disposed) { loading = false; notify(String(error), true); } });
    return scope.dispose;
  });

</script>

<main class="input-sharing-host content full-content" class:diagnostics-view={section === "diagnostics"}>
  {#if section === "gestureDebug"}
    <header class="input-sharing-header">
      <div><h1>{uiTranslate("多指手势实验", $uiLanguage)}</h1><p>{uiTranslate("本机采集、短时拦截与原生手势合成 · 临时诊断页", $uiLanguage)}</p></div>
      <button class="button secondary" on:click={() => section = "overview"}>{uiTranslate("返回跨屏输入", $uiLanguage)}</button>
    </header>
    <div class="gesture-debug-content"><GestureDebugPage inputSharingEnabled={enabled} /></div>
  {:else}
  {#if gestureLabVisible}
    <div class="gesture-debug-entry"><button class="button secondary" on:click={() => section = "gestureDebug"}><Bug size={16} /> {uiTranslate("手势实验（本机）", $uiLanguage)}</button></div>
  {/if}
  {#if loading}
    <div class="input-loading"><span></span><p>{uiTranslate("正在建立跨屏输入工作区…", $uiLanguage)}</p></div>
  {:else if snapshot}
    {#if showSetup}
      <InputSetupFlow {snapshot} onSnapshot={applySnapshot} {notify} onClose={() => showSetup = false} initialStep={setupInitialStep} />
    {:else}
      <header class="input-sharing-header">
        <div>
          <div class="title-line"><h1>{uiTranslate(section === "keyboard" ? "键盘行为" : section === "diagnostics" ? "诊断" : "跨屏输入", $uiLanguage)}</h1><span class:controlling class:warning={Boolean(offlinePeer)}>{uiTranslate(status, $uiLanguage)}</span></div>
          <p>{uiTranslate(section === "workspace" ? "按桌面上的真实位置排列屏幕，相邻重叠边段会自动连通。" : section === "keyboard" ? "让快捷键在不同系统上保持相同意图。" : section === "diagnostics" ? "查看连接质量、跨屏通道和最近事件。" : offlinePeer ? "布局已保留，但一台设备暂时不可用。" : "在设备之间自然移动鼠标和键盘。", $uiLanguage)}</p>
        </div>
        <div class="header-control">
          <small>{uiTranslate(offlinePeer ? `等待${friendlyDeviceName(offlinePeer.serviceInstanceId, snapshot!)}重新连接` : enabled ? "任一设备活动即可自动接管 · ⌘⌥⇧ Esc 紧急释放" : "启用一次后持续待命", $uiLanguage)}</small>
          <button
            class:controlling={enabled}
            class="control-button"
            disabled={busy || (!enabled && !canTakeInputControl(snapshot))}
            title={uiTranslate((!canTakeInputControl(snapshot) ? snapshot.capabilities.limitation ?? "需要开启辅助功能权限" : ""), $uiLanguage)}
            on:click={toggleInputSharing}
          >
            <Power size={18} weight="bold" />
            {uiTranslate(busy ? "处理中…" : enabled ? "关闭跨屏输入" : "启用跨屏输入", $uiLanguage)}
          </button>
        </div>
      </header>

      <nav class="section-tabs" aria-label={uiTranslate("跨屏输入设置", $uiLanguage)}>
        {#each sections as item}
          <button class:active={section === item.id} on:click={() => section = item.id}>
            <svelte:component this={item.icon} size={18} weight={section === item.id ? "fill" : "regular"} />
            {uiTranslate(item.label, $uiLanguage)}
          </button>
        {/each}
      </nav>

      {#if snapshot.controller}
        <div class="control-session" role="status" aria-live="polite">
          <span class="live-dot"></span>
          <div><strong>{uiTranslate("当前输入来自：", $uiLanguage)}{uiTranslate(controlling ? "这台设备" : remoteController, $uiLanguage)}</strong><small>{uiTranslate("检测到另一台设备的本地输入时会自动切换 · 会话已加密", $uiLanguage)}</small></div>
          {#if section !== "overview"}<button class="session-secondary" on:click={() => section = "overview"}><ArrowLeft size={15} />{uiTranslate("切回总览", $uiLanguage)}</button>{/if}
        </div>
      {/if}

      <div class="input-sharing-content">
        {#if section === "overview"}
          <InputOverviewPage {snapshot} onNavigate={navigate} onSetup={openSetup} onToggleInputSharing={toggleInputSharing} onRetry={reconnectOfflinePeer} onForget={forgetOfflinePeer} />
        {:else if section === "workspace"}
          <InputWorkspacePage {snapshot} onSnapshot={applySnapshot} {notify} />
        {:else if section === "keyboard"}
          <KeyboardPage {snapshot} onSnapshot={applySnapshot} {notify} />
        {:else}
          <DiagnosticsPage {snapshot} {notify} onRefresh={() => refresh()} />
        {/if}
      </div>
    {/if}
  {:else}
    <div class="input-loading"><p>{uiTranslate("跨屏输入运行时暂不可用。", $uiLanguage)}</p><button class="button" on:click={() => refresh()}>{uiTranslate("重试", $uiLanguage)}</button></div>
  {/if}
  {/if}
</main>

<style>
  .input-sharing-host { display: flex; min-width: 0; min-height: 0; flex-direction: column; gap: 0; padding: 20px 32px 0; overflow: hidden; color: var(--text); background: var(--surface); }
  .gesture-debug-entry { display: flex; justify-content: flex-end; padding-bottom: 12px; }.gesture-debug-entry button { display: flex; align-items: center; gap: 6px; }.gesture-debug-content { min-height: 0; flex: 1; overflow: auto; }
  .input-sharing-header { display: flex; min-height: 40px; align-items: center; justify-content: space-between; gap: 24px; padding-bottom: 10px; }
  .input-sharing-header h1 { margin: 0; color: var(--text); font-size: 24px; font-weight: 720; line-height: 1.1; letter-spacing: -.035em; }
  .input-sharing-header p { margin: 3px 0 0; color: var(--text-secondary); font-size: 11px; line-height: 1.35; }
  .title-line { display: flex; align-items: center; gap: 11px; }
  .title-line span { padding: 5px 10px; border-radius: 999px; color: var(--success); background: var(--success-soft); font-size: 11px; font-weight: 750; }
  .title-line span.controlling { color: var(--accent-strong); background: var(--accent-soft); }.title-line span.warning { color: var(--warning); background: var(--warning-soft); }
  .header-control { display: flex; align-items: center; justify-content: flex-end; gap: 12px; }.header-control small { max-width: 310px; color: var(--text-muted); font-size: 10px; text-align: right; }
  .control-button { display: inline-flex; min-width: 132px; min-height: 36px; align-items: center; justify-content: center; gap: 7px; border: 1px solid var(--accent); border-radius: 9px; padding: 0 14px; color: #fff; background: var(--accent); font-size: 12px; font-weight: 680; transition: 120ms ease; }
  .control-button:hover { background: var(--accent-strong); }.control-button.controlling { border-color: var(--danger); color: var(--danger); background: var(--surface); }.control-button:disabled { border-color: var(--border-strong); color: var(--text-muted); background: var(--surface-sunken); }
  .section-tabs { display: flex; gap: 32px; border-bottom: 1px solid var(--border); }
  .section-tabs button { position: relative; display: inline-flex; min-height: 40px; align-items: center; gap: 8px; border: 0; color: var(--text-secondary); background: transparent; font-size: 12px; }
  .section-tabs button::after { position: absolute; right: 0; bottom: -1px; left: 0; height: 3px; border-radius: 3px 3px 0 0; background: transparent; content: ""; }.section-tabs button.active { color: var(--accent-strong); font-weight: 700; }.section-tabs button.active::after { background: var(--accent); }
  .control-session { display: grid; grid-template-columns: 9px minmax(0,1fr) auto; align-items: center; gap: 10px; min-height: 40px; margin-top: 10px; padding: 6px 12px; border: 1px solid color-mix(in srgb,var(--accent) 30%,var(--border)); border-radius: 10px; background: color-mix(in srgb,var(--accent-soft) 72%,var(--surface)); }.live-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--success); box-shadow: 0 0 0 3px color-mix(in srgb,var(--success) 15%,transparent); }.control-session > div { display: flex; min-width: 0; align-items: baseline; gap: 9px; }.control-session strong { flex: none; font-size: 12px; }.control-session small { min-width: 0; overflow: hidden; color: var(--text-secondary); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }.session-secondary { display: inline-flex; min-height: 30px; align-items: center; gap: 5px; border: 1px solid color-mix(in srgb,var(--accent) 30%,var(--border)); border-radius: 7px; padding: 0 9px; color: var(--accent-strong); background: var(--surface); font-size: 10px; font-weight: 650; }
  .input-sharing-content { min-height: 0; flex: 1; overflow: auto; padding-top: 12px; }
  .diagnostics-view .input-sharing-content { padding-right: 4px; scrollbar-gutter: stable; }
  .diagnostics-view .control-session { min-height: 34px; margin-top: 8px; border-color: var(--border); background: var(--surface-soft); }
  .diagnostics-view .control-session strong { flex-shrink: 1; overflow-wrap: anywhere; font-size: 11px; }
  .diagnostics-view .session-secondary { border-color: transparent; background: transparent; }
  .input-loading { height: 100%; display: grid; place-content: center; justify-items: center; gap: 12px; color: var(--text-secondary); }
  .input-loading > span { width: 30px; height: 30px; border: 3px solid var(--border); border-top-color: var(--accent); border-radius: 50%; animation: spin .8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  :global(.input-sharing-host button) { font-family: inherit; cursor: pointer; }:global(.input-sharing-host button:disabled) { cursor: default; }
  :global(.input-sharing-host .card) { border-color: var(--border); background: var(--surface-raised); box-shadow: var(--shadow-card); }
  :global(.input-sharing-host .button) { border: 1px solid var(--accent); border-radius: 9px; padding: 9px 13px; color: #fff; background: var(--accent); transition: 120ms ease; }
  :global(.input-sharing-host .button:hover) { background: var(--accent-strong); }
  :global(.input-sharing-host .button.secondary) { border-color: var(--border-strong); color: var(--text); background: var(--surface); }
  :global(.input-sharing-host .button.danger) { border-color: var(--danger); background: var(--danger); }
  :global(.input-sharing-host .muted) { color: var(--text-secondary); }
  :global(.input-sharing-host .mono) { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
  :global(.input-sharing-host .badge) { display: inline-flex; border-radius: 999px; padding: 4px 9px; color: var(--accent-strong); background: var(--accent-soft); font-size: 12px; }
  :global(.input-sharing-host .field) { display: grid; gap: 6px; color: var(--text-secondary); font-size: 12px; }
  :global(.input-sharing-host .field input), :global(.input-sharing-host .field select) { min-height: 38px; border: 1px solid var(--border-strong); border-radius: 9px; padding: 8px 10px; color: var(--text); background: var(--control-bg); }
  @media (max-width: 900px) { .input-sharing-host { padding-inline: 20px; }.input-sharing-header { align-items: flex-start; }.header-control small { display: none; }.section-tabs { gap: 14px; }.section-tabs button { font-size: 11px; }.control-session { grid-template-columns: 9px 1fr auto; }.control-session small, .session-secondary { display: none; } }
</style>
