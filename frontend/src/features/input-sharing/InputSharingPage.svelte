<script lang="ts">
  import { diagnosticMessage } from "../../diagnosticMessages";
  import { t, translate as uiTranslate, language as uiLanguage } from "../../i18n";
  import { onMount } from "svelte";
  import { Dialog } from "bits-ui";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { ArrowLeft, Bug, Eye, Gear, ArrowCounterClockwise, Keyboard } from "phosphor-svelte";

  import { SubscriptionScope, observeSnapshot } from "../../subscriptions";
  import { bridge } from "../../bridge";
  import type { RuntimeSnapshot } from "../../types";
  import DiagnosticsPage from "./DiagnosticsPage.svelte";
  import InputOverviewPage from "./InputOverviewPage.svelte";
  import InputMenu from "./InputMenu.svelte";
  import InputSetupFlow from "./InputSetupFlow.svelte";
  import InputWorkspacePage from "./InputWorkspacePage.svelte";
  import KeyboardPage from "./KeyboardPage.svelte";
  import GestureDebugPage from "./GestureDebugPage.svelte";
  import GazePage from "./GazePage.svelte";
  import {
    canTakeInputControl,
    friendlyDeviceName,
    recoverableOfflinePeer,
  } from "./inputSharingUi";

  export let notify: (message: string, error?: boolean) => void;

  type Section = "overview" | "workspace" | "keyboard" | "gaze" | "diagnostics" | "gestureDebug";

  // Keep the local gesture lab implementation available for future diagnostics,
  // but do not expose or route into it from the product UI for now.
  const gestureLabVisible = false;
  const requestedSection = new URLSearchParams(window.location.search).get("inputSection");
  let section: Section =
    gestureLabVisible &&
    new URLSearchParams(window.location.search).get("inputDebug") === "gestures"
      ? "gestureDebug"
      : !bridge.isTauri() &&
          ["overview", "workspace", "keyboard", "gaze", "diagnostics"].includes(
            requestedSection ?? "",
          )
        ? (requestedSection as Section)
        : "workspace";
  let snapshot: RuntimeSnapshot | null = null;
  let loading = true;
  let busy = false;
  let workspaceSaving = false;
  let showSetup = false;
  let setupInitialStep: 1 | 2 | 3 | 4 = 1;
  let headerActions: HTMLDivElement | undefined;
  let workspacePage: InputWorkspacePage | undefined;
  let keyboardPage: KeyboardPage | undefined;
  function hasUnsavedChanges() {
    return workspacePage?.hasUnsavedChanges() || keyboardPage?.hasUnsavedChanges();
  }
  let discardOpen = false;
  let pendingNavigation: (() => void) | null = null;
  function requestNavigation(action: () => void) {
    if (hasUnsavedChanges()) {
      pendingNavigation = action;
      discardOpen = true;
    } else action();
  }
  function goTo(next: Section) {
    if (next !== section) requestNavigation(() => (section = next));
  }
  function discardChanges() {
    discardOpen = false;
    pendingNavigation?.();
    pendingNavigation = null;
  }
  function beforeUnload(event: BeforeUnloadEvent) {
    if (hasUnsavedChanges()) {
      event.preventDefault();
      event.returnValue = "";
    }
  }

  $: enabled = Boolean(snapshot?.configuration.inputSharingEnabled);
  $: offlinePeer = snapshot ? recoverableOfflinePeer(snapshot) : null;
  $: childPage = !["workspace", "overview"].includes(section);
  $: menuItems = [
    {
      label: uiTranslate("键盘行为", $uiLanguage),
      icon: Keyboard,
      navigate: true,
      action: () => goTo("keyboard"),
    },
    {
      label: uiTranslate("眼动", $uiLanguage),
      icon: Eye,
      navigate: true,
      action: () => goTo("gaze"),
    },
    {
      label: uiTranslate("诊断与调试", $uiLanguage),
      icon: Bug,
      separator: true,
      navigate: true,
      action: () => goTo("diagnostics"),
    },
    {
      label: uiTranslate("管理设备", $uiLanguage),
      icon: Gear,
      separator: true,
      action: () => openSetup(1),
    },
  ];

  async function refresh(silent = false) {
    try {
      applySnapshot(await bridge.getInputRuntimeSnapshot());
      const previewOnboarding =
        !bridge.isTauri() &&
        new URLSearchParams(window.location.search).get("inputScenario") === "onboarding";
      if (
        snapshot &&
        !snapshot.configuration.layout &&
        (previewOnboarding || window.localStorage.getItem("arc-input-onboarding-complete") !== "1")
      ) {
        setupInitialStep = 1;
        showSetup = true;
      }
    } catch (error) {
      if (!silent) notify(diagnosticMessage(error, $uiLanguage), true);
    } finally {
      loading = false;
    }
  }

  async function toggleInputSharing() {
    if (!snapshot) return;
    busy = true;
    try {
      applySnapshot(await bridge.setInputSharingEnabled(!enabled));
      notify(
        snapshot.configuration.inputSharingEnabled
          ? uiTranslate("跨屏输入已启用，任一设备都可自然接管", $uiLanguage)
          : uiTranslate("跨屏输入已关闭，输入已安全释放", $uiLanguage),
      );
    } catch (error) {
      notify(diagnosticMessage(error, $uiLanguage), true);
    } finally {
      busy = false;
    }
  }

  async function reconnectOfflinePeer() {
    if (!offlinePeer) return;
    busy = true;
    try {
      applySnapshot(await bridge.connectInputPeer(offlinePeer));
      notify(
        t("已重新连接{part0}", $uiLanguage, {
          part0: friendlyDeviceName(offlinePeer.serviceInstanceId, snapshot!, $uiLanguage),
        }),
      );
    } catch (error) {
      notify(
        t("重新连接失败：{part0}", $uiLanguage, { part0: diagnosticMessage(error, $uiLanguage) }),
        true,
      );
    } finally {
      busy = false;
    }
  }

  async function forgetOfflinePeer() {
    if (
      !offlinePeer ||
      !window.confirm(
        t("移除{part0}？\n\n将同时删除该设备的屏幕布局与跨屏边缘。", $uiLanguage, {
          part0: friendlyDeviceName(offlinePeer.serviceInstanceId, snapshot!, $uiLanguage),
        }),
      )
    )
      return;
    busy = true;
    try {
      applySnapshot(await bridge.forgetInputPeer(offlinePeer.serviceInstanceId));
      notify(uiTranslate("设备配对、屏幕布局和相关边缘已移除", $uiLanguage));
    } catch (error) {
      notify(
        t("移除设备失败：{part0}", $uiLanguage, { part0: diagnosticMessage(error, $uiLanguage) }),
        true,
      );
    } finally {
      busy = false;
    }
  }

  function openSetup(step: 1 | 2 | 3 | 4 = 1) {
    requestNavigation(() => {
      setupInitialStep = step;
      showSetup = true;
    });
  }

  function navigate(next: "workspace" | "keyboard" | "diagnostics") {
    goTo(next);
  }

  function applySnapshot(next: RuntimeSnapshot) {
    if (!snapshot || next.revision > snapshot.revision) snapshot = next;
  }
  onMount(() => {
    const scope = new SubscriptionScope();
    void observeSnapshot(
      scope,
      (accept) =>
        bridge.isTauri()
          ? listen<RuntimeSnapshot>("arc-input-state", (event) => accept(event.payload))
          : Promise.resolve(() => {}),
      () => bridge.getInputRuntimeSnapshot(),
      (next) => {
        applySnapshot(next);
        loading = false;
      },
    )
      .then(() => {
        if (scope.disposed || !snapshot) return;
        const previewOnboarding =
          !bridge.isTauri() &&
          new URLSearchParams(window.location.search).get("inputScenario") === "onboarding";
        if (
          snapshot &&
          !snapshot.configuration.layout &&
          (previewOnboarding ||
            window.localStorage.getItem("arc-input-onboarding-complete") !== "1")
        ) {
          setupInitialStep = 1;
          showSetup = true;
        }
      })
      .catch((error) => {
        if (!scope.disposed) {
          loading = false;
          notify(diagnosticMessage(error, $uiLanguage), true);
        }
      });
    return scope.dispose;
  });
</script>

<svelte:window on:beforeunload={beforeUnload} />

<Dialog.Root bind:open={discardOpen}>
  <Dialog.Portal
    ><Dialog.Overlay class="input-discard-overlay" /><Dialog.Content class="input-discard-dialog"
      ><Dialog.Title>{uiTranslate("放弃未保存的更改？", $uiLanguage)}</Dialog.Title
      ><Dialog.Description class="sr-only">{uiTranslate("未保存", $uiLanguage)}</Dialog.Description>
      <div>
        <Dialog.Close class="input-secondary">{uiTranslate("取消", $uiLanguage)}</Dialog.Close
        ><button class="input-primary" on:click={discardChanges}
          >{uiTranslate("放弃更改", $uiLanguage)}</button
        >
      </div></Dialog.Content
    ></Dialog.Portal
  >
</Dialog.Root>
<main
  class="input-sharing-host content full-content"
  class:diagnostics-view={section === "diagnostics"}
>
  {#if section === "gestureDebug"}
    <header class="input-sharing-header">
      <div>
        <h1>{uiTranslate("多指手势实验", $uiLanguage)}</h1>
        <p>{uiTranslate("本机采集、短时拦截与原生手势合成 · 临时诊断页", $uiLanguage)}</p>
      </div>
      <button class="button secondary" on:click={() => (section = "overview")}
        >{uiTranslate("返回跨屏输入", $uiLanguage)}</button
      >
    </header>
    <div class="gesture-debug-content"><GestureDebugPage inputSharingEnabled={enabled} /></div>
  {:else}
    {#if gestureLabVisible}
      <div class="gesture-debug-entry">
        <button class="button secondary" on:click={() => (section = "gestureDebug")}
          ><Bug size={16} /> {uiTranslate("手势实验（本机）", $uiLanguage)}</button
        >
      </div>
    {/if}
    {#if loading}
      <div class="input-loading">
        <span></span>
        <p>{uiTranslate("正在建立跨屏输入工作区…", $uiLanguage)}</p>
      </div>
    {:else if snapshot}
      {#if showSetup}
        <InputSetupFlow
          {snapshot}
          onSnapshot={applySnapshot}
          {notify}
          onClose={() => (showSetup = false)}
          initialStep={setupInitialStep}
        />
      {:else}
        {#if childPage}
          <button class="back-link" on:click={() => goTo("workspace")}
            ><ArrowLeft size={17} />{uiTranslate("跨屏输入", $uiLanguage)}</button
          >
        {/if}
        <header class="input-sharing-header" class:child-page={childPage}>
          <div>
            <h1>
              {uiTranslate(
                section === "keyboard"
                  ? "键盘行为"
                  : section === "gaze"
                    ? "眼动"
                    : section === "diagnostics"
                      ? "诊断与调试"
                      : "跨屏输入",
                $uiLanguage,
              )}{#if section === "diagnostics"}<small class="debug-label">Debug</small>{/if}
            </h1>
            {#if section === "keyboard"}<p>
                {uiTranslate("按熟悉的方式使用键盘", $uiLanguage)}
              </p>{:else if section === "gaze"}<p>
                {uiTranslate("看向另一块屏幕，鼠标随之切换", $uiLanguage)}
              </p>{/if}
          </div>
          {#if !childPage}
            <div class="header-control">
              <button
                class="sharing-switch"
                class:enabled
                role="switch"
                aria-checked={enabled}
                aria-label={uiTranslate("跨屏输入", $uiLanguage)}
                disabled={busy || workspaceSaving || (!enabled && !canTakeInputControl(snapshot))}
                title={uiTranslate(
                  !canTakeInputControl(snapshot)
                    ? (snapshot.capabilities.limitation ?? "需要开启辅助功能权限")
                    : enabled
                      ? "关闭跨屏输入"
                      : "启用跨屏输入",
                  $uiLanguage,
                )}
                on:click={toggleInputSharing}><span></span></button
              >
              <InputMenu label={uiTranslate("跨屏输入更多选项", $uiLanguage)} items={menuItems} />
            </div>
          {:else}
            <div class="header-control" bind:this={headerActions}></div>
          {/if}
        </header>
        {#if !canTakeInputControl(snapshot) && !childPage}
          <div class="recovery-notice" role="status">
            <span>{uiTranslate("需要开启辅助功能权限", $uiLanguage)}</span><button
              on:click={() => openSetup(3)}>{uiTranslate("权限设置", $uiLanguage)}</button
            >
          </div>
        {/if}
        {#if enabled && offlinePeer && !childPage}
          <div class="recovery-notice" role="status">
            <span
              >{friendlyDeviceName(offlinePeer.serviceInstanceId, snapshot, $uiLanguage)} · {uiTranslate(
                "已离线",
                $uiLanguage,
              )}</span
            ><button disabled={busy} on:click={reconnectOfflinePeer}
              ><ArrowCounterClockwise size={15} />{uiTranslate("重新连接", $uiLanguage)}</button
            ><button on:click={forgetOfflinePeer}>{uiTranslate("移除设备", $uiLanguage)}</button>
          </div>
        {/if}
        <div class="input-sharing-content">
          {#if section === "overview"}
            <InputOverviewPage
              {snapshot}
              onNavigate={navigate}
              onSetup={openSetup}
              onToggleInputSharing={toggleInputSharing}
              onRetry={reconnectOfflinePeer}
              onForget={forgetOfflinePeer}
            />
          {:else if section === "workspace"}
            <InputWorkspacePage
              bind:this={workspacePage}
              bind:saving={workspaceSaving}
              inputBusy={busy}
              {snapshot}
              onSnapshot={applySnapshot}
              {notify}
            />
          {:else if section === "keyboard"}
            <KeyboardPage
              actionsTarget={headerActions}
              bind:this={keyboardPage}
              {snapshot}
              onSnapshot={applySnapshot}
              {notify}
            />
          {:else if section === "gaze"}
            <GazePage actionsTarget={headerActions} {snapshot} {notify} />
          {:else}
            <DiagnosticsPage
              actionsTarget={headerActions}
              {snapshot}
              {notify}
              onRefresh={() => refresh()}
            />
          {/if}
        </div>
      {/if}
    {:else}
      <div class="input-loading">
        <p>{uiTranslate("跨屏输入运行时暂不可用。", $uiLanguage)}</p>
        <button class="button" on:click={() => refresh()}>{uiTranslate("重试", $uiLanguage)}</button
        >
      </div>
    {/if}
  {/if}
</main>

<style>
  :global(.input-discard-overlay) {
    position: fixed;
    inset: 0;
    z-index: 1100;
    background: #10182835;
  }
  :global(.input-discard-dialog) {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    z-index: 1101;
    width: min(380px, calc(100vw - 32px));
    padding: 24px;
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 16px 48px #10182825;
  }
  :global(.input-discard-dialog h2) {
    font-size: 17px;
    margin: 0 0 22px;
  }
  :global(.input-discard-dialog > div) {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  :global(.input-discard-dialog .sr-only) {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }

  .input-sharing-host {
    display: flex;
    min-width: 0;
    min-height: 0;
    flex-direction: column;
    gap: 0;
    padding: 24px 32px 0;
    overflow: hidden;
    color: var(--text);
    background: var(--surface);
  }
  .input-sharing-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 58px;
    gap: 24px;
    padding-bottom: 18px;
    border-bottom: 1px solid var(--border);
  }
  .input-sharing-header h1 {
    margin: 0;
    font-size: 27px;
    font-weight: 700;
    letter-spacing: -0.035em;
    line-height: 1.25;
  }
  .input-sharing-header p {
    margin: 7px 0 0;
    font-size: 14px;
    color: var(--text-secondary);
  }
  .child-page {
    border-bottom: 0;
    padding-top: 18px;
    padding-bottom: 12px;
  }
  .debug-label {
    margin-left: 12px;
    color: var(--text-muted);
    font-size: 17px;
    font-weight: 450;
    letter-spacing: normal;
  }
  .back-link {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    align-self: flex-start;
    border: 0;
    padding: 0;
    color: var(--text-secondary);
    background: transparent;
    font-size: 13px;
  }
  .header-control {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .sharing-switch {
    width: 45px;
    height: 26px;
    padding: 3px;
    border: 0;
    border-radius: 99px;
    background: var(--border-strong);
  }
  .sharing-switch > span {
    display: block;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: white;
    box-shadow: 0 1px 3px #10182820;
    transition: transform 120ms ease;
  }
  .sharing-switch.enabled {
    background: var(--accent);
  }
  .sharing-switch.enabled > span {
    transform: translateX(19px);
  }
  .sharing-switch:disabled {
    opacity: 0.5;
  }
  .input-sharing-content {
    min-height: 0;
    flex: 1;
    overflow: auto;
    padding-top: 16px;
  }
  .recovery-notice {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 0;
    font-size: 13px;
    color: var(--text-secondary);
  }
  .recovery-notice > span {
    flex: 1;
  }
  .recovery-notice button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border: 0;
    color: var(--accent);
    background: transparent;
  }
  .input-loading {
    height: 100%;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 12px;
    color: var(--text-secondary);
  }
  .gesture-debug-content {
    min-height: 0;
    flex: 1;
    overflow: auto;
  }
  .gesture-debug-entry {
    display: none;
  }
  :global(.input-sharing-host button) {
    font-family: inherit;
    cursor: pointer;
  }
  :global(.input-sharing-host button:disabled) {
    cursor: default;
  }
  :global(.input-sharing-host button:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 3px;
  }
  :global(.input-sharing-host .card) {
    border-color: var(--border);
    background: var(--surface-raised);
    box-shadow: var(--shadow-card);
  }
  :global(.input-sharing-host .button) {
    border: 1px solid var(--accent);
    border-radius: 9px;
    padding: 9px 13px;
    color: #fff;
    background: var(--accent);
  }
  :global(.input-sharing-host .button.secondary) {
    border-color: var(--border-strong);
    color: var(--text);
    background: var(--surface);
  }
  :global(.input-sharing-host .muted) {
    color: var(--text-secondary);
  }
  :global(.input-sharing-host .mono) {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  :global(.input-sharing-host .badge) {
    display: inline-flex;
    border-radius: 99px;
    padding: 4px 9px;
    color: var(--accent-strong);
    background: var(--accent-soft);
    font-size: 12px;
  }
  :global(.input-sharing-host .field) {
    display: grid;
    gap: 6px;
    color: var(--text-secondary);
    font-size: 12px;
  }
  :global(.input-sharing-host .field input),
  :global(.input-sharing-host .field select) {
    min-height: 38px;
    border: 1px solid var(--border-strong);
    border-radius: 9px;
    padding: 8px 10px;
    color: var(--text);
    background: var(--control-bg);
  }
  @media (max-width: 900px) {
    .input-sharing-host {
      padding: 18px 20px 0;
    }
  }
</style>
