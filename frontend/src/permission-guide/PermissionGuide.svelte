<script lang="ts">
  import { translate, language, setLanguage } from "../i18n";
  import { loadAppSettings, onAppSettingsChanged } from "../clipboard/i18n";
  import { invoke } from "../ipc/client";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { onMount } from "svelte";
  import { startDrag } from "@crabnebula/tauri-plugin-drag";
  import { Check, ClipboardText, CursorClick, HandGrabbing, LockKeyOpen } from "phosphor-svelte";

  import appIconUrl from "../../../icons/icon-macos-1024.png?url";
  import type { AppSettings, BootstrapState } from "../types";

  const previewState = "__TAURI_INTERNALS__" in window ? null : new URLSearchParams(window.location.search).get("preview");
  let granted = false;
  let checking = previewState === null;
  let refreshing = false;
  let openingSettings = false;
  let settingsOpened = previewState === "settings";
  let dragStarted = false;
  let completed = false;

  let appBundlePath = "";
  let dragIconPath = "";
  let timer: ReturnType<typeof setInterval> | undefined;

  const strings = {
      title: "开启辅助功能",
      intro: "允许 ArcRelay 将剪贴板内容插入当前应用，并让已授权设备控制键盘和指针。",
      clipboard: "剪贴板监控已可用，无需单独授权",
      permission: "辅助功能权限",
      granted: "已开启",
      missing: "尚未开启",
      open: "打开辅助功能设置",
      reopen: "重新打开系统设置",
      opening: "正在打开…",
      later: "稍后处理",
      firstStep: "打开“系统设置 → 隐私与安全性 → 辅助功能”",
      dragTitle: "将 ArcRelay 拖入辅助功能列表",
      dragDescription: "按住左侧应用图标，拖到旁边的系统设置窗口中，然后开启 ArcRelay 的开关。",
      dragHint: "按住并拖动",
      dragged: "已拖入，接着开启开关",
      returnStep: "完成后无需点击按钮，ArcRelay 会自动检测并关闭此窗口",
      waiting: "正在等待授权状态更新…",
      privacy: "权限仅在本机用于模拟粘贴和远程输入，不会用于记录键盘输入。",
      success: "辅助功能已开启",
  } as const;
  $: text = Object.fromEntries(Object.entries(strings).map(([key, value]) => [key, translate(value, $language)])) as Record<keyof typeof strings, string>;

  async function refreshPermission() {
    if (refreshing) return;
    refreshing = true;
    try {
      const permission = await invoke("refresh_input_permission");
      const nextGranted = permission === "Granted";
      if (!granted && nextGranted) {
        granted = true;
        completed = true;
        if (timer) clearInterval(timer);
        setTimeout(() => void closeGuide(), 1500);
      } else {
        granted = nextGranted;
      }
    } catch (error) {
      console.error("Failed to refresh Accessibility permission", error);
    } finally {
      refreshing = false;
      checking = false;
    }
  }

  async function closeGuide() {
    try {
      await invoke("close_input_permission_guide");
    } catch (error) {
      console.error("Failed to close Accessibility permission guide", error);
      await getCurrentWebviewWindow().close();
    }
  }

  async function startWindowDrag(event: MouseEvent) {
    if (event.button !== 0) return;
    try {
      await invoke("start_permission_guide_window_drag");
    } catch (error) {
      console.error("Failed to drag Accessibility permission guide", error);
    }
  }

  async function openSettings() {
    settingsOpened = true;
    openingSettings = true;
    try {
      await invoke("open_accessibility_system_settings");
    } catch (error) {
      console.error("Failed to open Accessibility settings", error);
    } finally {
      openingSettings = false;
    }
  }

  async function beginAppDrag(event: DragEvent) {
    event.preventDefault();
    if (!appBundlePath || !dragIconPath || granted) return;
    dragStarted = true;
    try {
      await startDrag(
        { item: [appBundlePath], icon: dragIconPath, mode: "copy" },
        ({ result }) => {
          if (result === "Cancelled") dragStarted = false;
        },
      );
    } catch (error) {
      dragStarted = false;
      console.error("Failed to start app drag", error);
    }
  }

  onMount(() => {
    void loadAppSettings().then(settings => setLanguage(settings.language));
    const unsubscribe = onAppSettingsChanged(settings => setLanguage(settings.language));
    return () => { void unsubscribe.then(stop => stop()); };
  });

  onMount(() => {
    if (previewState !== null) return;
    void invoke("permission_guide_ready")
      .catch((error) => console.error("Failed to show ready permission guide", error));
    void Promise.all([
      invoke("get_app_settings"),
      invoke("get_application_bundle_path"),
      invoke("get_application_drag_icon_path"),
    ])
      .then(([settings, bundlePath, iconPath]) => {
        setLanguage(settings.language);
        appBundlePath = bundlePath;
        dragIconPath = iconPath;
      })
      .catch((error) => console.error("Failed to prepare permission guide", error));
    void refreshPermission();
    timer = setInterval(() => void refreshPermission(), 800);
    return () => {
      if (timer) clearInterval(timer);
    };
  });
</script>

<main class="permission-window">
  <!-- svelte-ignore a11y_no_static_element_interactions (native window drag handle) -->
  <div class="window-drag-region" data-tauri-drag-region on:mousedown={startWindowDrag}></div>
  {#if completed}
    <section class="success-state" aria-live="polite">
      <span><Check size={38} weight="bold" /></span>
      <h1>{text.success}</h1>
    </section>
  {:else}
    <!-- svelte-ignore a11y_no_static_element_interactions (native window drag handle) -->
    <header class="header" data-tauri-drag-region on:mousedown={startWindowDrag}>
      <img src={appIconUrl} alt="ArcRelay" />
      <div>
        <p>ARCRELAY</p>
        <h1>{text.title}</h1>
        <span>{text.intro}</span>
      </div>
    </header>

    <div class="clipboard-ready">
      <ClipboardText size={18} weight="duotone" />
      <span>{text.clipboard}</span>
      <Check size={15} weight="bold" />
    </div>

    <section class:active={!granted} class="permission-card">
      <span class="permission-icon"><CursorClick size={25} weight="duotone" /></span>
      <div>
        <div class="permission-title">
          <h2>{text.permission}</h2>
          <span class:granted>{granted ? text.granted : text.missing}</span>
        </div>
        <p>{text.intro}</p>
      </div>
    </section>

    {#if settingsOpened}
      <section class="drag-guide">
        <div class="drag-copy">
          <h2>{text.dragTitle}</h2>
          <p>{text.dragDescription}</p>
        </div>
        <div class="drag-target-row">
          <div class:dragging={dragStarted} class="draggable-app" draggable="true" role="button" tabindex="0" aria-label={text.dragHint} on:dragstart={beginAppDrag}>
            <img src={appIconUrl} alt="ArcRelay" />
            <span><HandGrabbing size={16} weight="fill" />{dragStarted ? text.dragged : text.dragHint}</span>
          </div>
          <div class="drag-arrow" aria-hidden="true">→</div>
          <div class="settings-destination">
            <LockKeyOpen size={29} weight="duotone" />
            <span>{translate("辅助功能", $language)}</span>
          </div>
        </div>
      </section>
    {/if}

    <section class="steps">
      <div class:done={settingsOpened} class="step">
        <b>{settingsOpened ? "✓" : "1"}</b>
        <span>{text.firstStep}</span>
      </div>
      <div class:current={settingsOpened} class="step">
        <b>2</b>
        <span>{settingsOpened ? text.dragTitle : (translate("在辅助功能列表中开启 ArcRelay", $language))}</span>
      </div>
      <div class="step"><b>3</b><span>{text.returnStep}</span></div>
    </section>

    <footer>
      {#if settingsOpened}<p class="waiting"><i></i>{text.waiting}</p>{/if}
      <button class="primary" disabled={openingSettings || checking} on:click={openSettings}>{openingSettings ? text.opening : settingsOpened ? text.reopen : text.open}</button>
      <button class="later" on:click={closeGuide}>{text.later}</button>
      <p class="privacy">{text.privacy}</p>
    </footer>
  {/if}
</main>
