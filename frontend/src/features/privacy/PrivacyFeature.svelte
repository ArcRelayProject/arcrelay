<script lang="ts">
  import { t } from "../../localization";
  import { translate as uiTranslate, language as uiLanguage } from "../../i18n";
  import { onMount } from "svelte";
  import { Dialog } from "bits-ui";
  import {
    ChatCircleDots,
    Check,
    DeviceMobile,
    EyeSlash,
    HandTap,
    MagnifyingGlass,
    Monitor,
    Plus,
    ShieldCheck,
    Trash,
    X,
  } from "phosphor-svelte";

  import InstalledAppIcon from "../../InstalledAppIcon.svelte";
  import { bridge } from "../../bridge";
  import { translate } from "../../i18n";
  import type { AppSettings, BootstrapState, InstalledApp, PrivacySettings, ProtectedApp } from "../../types";
  import { errorMessage } from "../../app_helpers";

  type ConfirmationRequest = {
    title: string;
    description: string;
    confirmLabel: string;
    kind: "danger" | "primary";
    onConfirm: () => Promise<boolean | void> | boolean | void;
  };

  export let snapshot: BootstrapState | null = null;
  export let language: AppSettings["language"] = "zhCn";
  export let notify: (message: string, kind?: "success" | "error") => void;
  export let confirm: (request: ConfirmationRequest) => void;
  export let onSnapshot: (state: BootstrapState) => void;

  let privacyBusy = false;
  let privacyAppPickerOpen = false;
  let privacyAppSearch = "";
  let installedApps: InstalledApp[] = [];
  let appsLoading = false;

  onMount(() => { void loadInstalledApps(); });

  function tr(source: string) { return translate(source, language); }

  async function loadInstalledApps() {
    if (appsLoading || installedApps.length > 0) return;
    appsLoading = true;
    try { installedApps = await bridge.listInstalledApps(); }
    catch (error) { notify(errorMessage(error), "error"); }
    finally { appsLoading = false; }
  }

  function applyPrivacy(privacy: BootstrapState["privacy"]) {
    if (!snapshot) return;
    const state = { ...snapshot, privacy };
    snapshot = state;
    onSnapshot(state);
  }

  async function togglePrivacyMode() {
    if (!snapshot || privacyBusy) return;
    privacyBusy = true;
    try {
      const privacy = await bridge.setPrivacyEnabled(!snapshot.privacy.active);
      applyPrivacy(privacy);
      notify(tr(privacy.active ? "投屏隐私保护已开启" : "投屏隐私保护已关闭"));
    } catch (error) { notify(errorMessage(error), "error"); }
    finally { privacyBusy = false; }
  }

  async function savePrivacySettings(settings: PrivacySettings): Promise<boolean> {
    if (!snapshot || privacyBusy) return false;
    privacyBusy = true;
    try { applyPrivacy(await bridge.updatePrivacySettings(settings)); return true; }
    catch (error) { notify(errorMessage(error), "error"); return false; }
    finally { privacyBusy = false; }
  }

  function patchPrivacySettings(patch: Partial<PrivacySettings>) {
    if (snapshot) void savePrivacySettings({ ...snapshot.privacy.settings, ...patch });
  }

  function setProtectedAppEnabled(index: number, enabled: boolean) {
    if (!snapshot) return;
    patchPrivacySettings({
      protectedApps: snapshot.privacy.settings.protectedApps.map((app, appIndex) =>
        appIndex === index ? { ...app, enabled } : app,
      ),
    });
  }

  function removeProtectedApp(index: number, name: string) {
    if (!snapshot || privacyBusy) return;
    confirm({
      title: tr("移除受保护应用？"),
      description: t("移除“{name}”后，它的窗口将不再受投屏隐私保护。", language, { name: name }),
      confirmLabel: tr("确认移除"),
      kind: "danger",
      onConfirm: async () => {
        if (!snapshot) return false;
        const settings = snapshot.privacy.settings;
        const protectedApps = settings.protectedApps.filter((_, appIndex) => appIndex !== index);
        if (!(await savePrivacySettings({ ...settings, protectedApps }))) return false;
        notify(t("已移除“{name}”", language, { name: name }));
      },
    });
  }

  function addProtectedApp(app: InstalledApp) {
    if (!snapshot) return;
    const next: ProtectedApp = { name: app.name, identifier: app.identifier, path: app.path, enabled: true };
    const exists = snapshot.privacy.settings.protectedApps.some((item) =>
      (next.identifier && item.identifier === next.identifier)
      || (next.path && item.path === next.path)
      || item.name.toLocaleLowerCase("zh-CN") === next.name.toLocaleLowerCase("zh-CN"),
    );
    if (!exists) patchPrivacySettings({ protectedApps: [...snapshot.privacy.settings.protectedApps, next] });
    privacyAppPickerOpen = false;
    privacyAppSearch = "";
  }

  function installedAppFor(protectedApp: ProtectedApp) {
    return installedApps.find((app) =>
      (protectedApp.identifier && app.identifier === protectedApp.identifier)
      || (protectedApp.path && app.path === protectedApp.path)
      || app.name.toLocaleLowerCase("zh-CN") === protectedApp.name.toLocaleLowerCase("zh-CN"),
    );
  }

  function availablePrivacyApps() {
    const query = privacyAppSearch.trim().toLocaleLowerCase("zh-CN");
    const selected = snapshot?.privacy.settings.protectedApps ?? [];
    return installedApps.filter((app) => {
      const alreadySelected = selected.some((item) =>
        (item.identifier && app.identifier === item.identifier) || (item.path && app.path === item.path),
      );
      return !alreadySelected && (!query || app.name.toLocaleLowerCase("zh-CN").includes(query)
        || app.identifier?.toLocaleLowerCase("zh-CN").includes(query)
        || app.path.toLocaleLowerCase("zh-CN").includes(query));
    }).slice(0, 80);
  }
</script>
    <div class="privacy-layout">
      <main class="content privacy-content">
        <header class="privacy-page-header">
          <h1>{uiTranslate("投屏隐私", $uiLanguage)}</h1>
        </header>

        <section class:active={snapshot?.privacy.active} class="privacy-status-banner">
          <span class="privacy-status-icon"><ShieldCheck size={25} weight="duotone" /></span>
          <div class="privacy-status-copy">
            <strong>{uiTranslate(snapshot?.privacy.active ? "隐私模式已开启" : "隐私模式已关闭", $uiLanguage)}</strong>
            <span>
              {#if snapshot?.privacy.active}
                {uiTranslate(snapshot?.privacy.activationSource === "screenMirror"
                  ? "检测到屏幕镜像，已自动保护所选应用"
                  : "受保护应用的可见窗口将被安全遮挡", $uiLanguage)}
              {:else if snapshot?.privacy.mirrorDetected}{uiTranslate("已检测到屏幕镜像，可立即开启保护", $uiLanguage)}{:else}{uiTranslate("开启后将保护所选应用的隐私内容不被投屏查看", $uiLanguage)}{/if}
            </span>
          </div>
          <button class="primary-button privacy-primary-action" disabled={privacyBusy} on:click={togglePrivacyMode}>
            {uiTranslate(snapshot?.privacy.active ? "关闭保护" : "立即开启", $uiLanguage)}
          </button>
        </section>

        <section class="privacy-section">
          <h2>{uiTranslate("开启方式", $uiLanguage)}</h2>
          <div class="privacy-setting-list">
            <div class="privacy-setting-row">
              <span class="privacy-row-icon"><HandTap size={22} /></span>
              <span class="privacy-row-copy">
                <strong>{uiTranslate("手动开启", $uiLanguage)}</strong>
                <small>{uiTranslate("可随时从本页面或系统托盘开启和关闭", $uiLanguage)}</small>
              </span>
              <span class:active={snapshot?.privacy.manualEnabled} class="privacy-state-text">
                {uiTranslate(snapshot?.privacy.manualEnabled ? "已开启" : "待命", $uiLanguage)}
              </span>
            </div>
            <button
              class="privacy-setting-row interactive"
              role="switch"
              aria-checked={snapshot?.privacy.settings.autoEnableOnMirror ?? false}
              disabled={privacyBusy}
              on:click={() =>
                patchPrivacySettings({
                  autoEnableOnMirror: !(snapshot?.privacy.settings.autoEnableOnMirror ?? false),
                })}
            >
              <span class="privacy-row-icon"><Monitor size={22} /></span>
              <span class="privacy-row-copy">
                <strong>{uiTranslate("屏幕镜像时自动开启", $uiLanguage)}</strong>
                <small>{uiTranslate("检测到系统进入镜像模式后自动保护", $uiLanguage)}</small>
              </span>
              <span class:checked={snapshot?.privacy.settings.autoEnableOnMirror} class="switch-control"><span></span></span>
            </button>
            <button
              class="privacy-setting-row interactive"
              role="switch"
              aria-checked={snapshot?.privacy.settings.allowRemoteActions ?? false}
              disabled={privacyBusy}
              on:click={() =>
                patchPrivacySettings({
                  allowRemoteActions: !(snapshot?.privacy.settings.allowRemoteActions ?? false),
                })}
            >
              <span class="privacy-row-icon"><DeviceMobile size={22} /></span>
              <span class="privacy-row-copy">
                <strong>{uiTranslate("允许移动端快捷动作控制", $uiLanguage)}</strong>
                <small>
                  {#if (snapshot?.connectedDevices.length ?? 0) > 0}{uiTranslate("已连接：", $uiLanguage)}{snapshot?.connectedDevices[0]?.name} {uiTranslate("· 在线", $uiLanguage)}{:else}{uiTranslate("连接后可在手机快捷动作中远程切换", $uiLanguage)}{/if}
                </small>
              </span>
              <span class:checked={snapshot?.privacy.settings.allowRemoteActions} class="switch-control"><span></span></span>
            </button>
          </div>
        </section>

        <section class="privacy-section protected-apps-section">
          <div class="privacy-section-heading">
            <div>
              <h2>{uiTranslate("保护这些应用", $uiLanguage)}</h2>
              <span>{uiTranslate("未聚焦但仍显示在桌面上的窗口也会被遮挡", $uiLanguage)}</span>
            </div>
          </div>
          <div class="protected-app-list">
            {#each snapshot?.privacy.settings.protectedApps ?? [] as protectedApp, index (protectedApp.identifier ?? protectedApp.path ?? protectedApp.name)}
              {@const installedApp = installedAppFor(protectedApp)}
              <div class:disabled={!protectedApp.enabled} class="protected-app-row">
                <button
                  class="protected-app-toggle"
                  role="switch"
                  aria-checked={protectedApp.enabled}
                  disabled={privacyBusy}
                  on:click={() => setProtectedAppEnabled(index, !protectedApp.enabled)}
                >
                  <span class="protected-app-icon">
                    {#if installedApp}
                      <InstalledAppIcon path={installedApp.path} name={installedApp.name} initialIcon={installedApp.iconDataUrl} />
                    {:else}<ChatCircleDots size={22} weight="duotone" />{/if}
                  </span>
                  <span class="protected-app-copy">
                    <strong>{protectedApp.name}</strong>
                    <small>{uiTranslate(protectedApp.identifier ?? protectedApp.path ?? "按应用名称识别", $uiLanguage)}</small>
                  </span>
                  <span class:checked={protectedApp.enabled} class="privacy-checkbox">
                    {#if protectedApp.enabled}<Check size={14} weight="bold" />{/if}
                  </span>
                </button>
                <button
                  class="remove-protected-app"
                  aria-label={uiTranslate((`移除 ${protectedApp.name}`), $uiLanguage)}
                  title={uiTranslate("移除应用", $uiLanguage)}
                  disabled={privacyBusy}
                  on:click={() => removeProtectedApp(index, protectedApp.name)}
                >
                  <Trash size={17} />
                </button>
              </div>
            {/each}
          </div>
          <button class="add-protected-app" on:click={() => (privacyAppPickerOpen = true)} disabled={privacyBusy || appsLoading}>
            <Plus size={17} /> {uiTranslate(appsLoading ? "正在读取应用…" : "添加应用", $uiLanguage)}
          </button>
        </section>
      </main>

      <aside class="privacy-inspector">
        <header>
          <strong>{uiTranslate("遮罩效果", $uiLanguage)}</strong>
        </header>
        <div class="privacy-inspector-body">
          <div class:solid={snapshot?.privacy.settings.maskStyle === "solid"} class="mask-preview">
            <EyeSlash size={38} weight="duotone" />
            <strong>{uiTranslate("隐私内容已隐藏", $uiLanguage)}</strong>
            <small>{uiTranslate("效果预览；实际仅遮挡上方已选择应用的可见窗口", $uiLanguage)}</small>
          </div>
          <div class="mask-style-options" role="radiogroup" aria-label={uiTranslate("遮罩样式", $uiLanguage)}>
            <h2>{uiTranslate("遮罩样式", $uiLanguage)}</h2>
            <button
              class:selected={snapshot?.privacy.settings.maskStyle === "frosted"}
              role="radio"
              aria-checked={snapshot?.privacy.settings.maskStyle === "frosted"}
              on:click={() => patchPrivacySettings({ maskStyle: "frosted" })}
            >
              <span class="radio-control"><span></span></span>
              <span><strong>{uiTranslate("磨砂遮罩", $uiLanguage)}</strong><small>{uiTranslate("半透明磨砂，视觉更自然", $uiLanguage)}</small></span>
            </button>
            <button
              class:selected={snapshot?.privacy.settings.maskStyle === "solid"}
              role="radio"
              aria-checked={snapshot?.privacy.settings.maskStyle === "solid"}
              on:click={() => patchPrivacySettings({ maskStyle: "solid" })}
            >
              <span class="radio-control"><span></span></span>
              <span><strong>{uiTranslate("纯色遮罩", $uiLanguage)}</strong><small>{uiTranslate("更高对比度，性能更佳", $uiLanguage)}</small></span>
            </button>
          </div>
        </div>
        <div class="privacy-live-status">
          <span class:online={snapshot?.privacy.mirrorDetected} class="status-dot"></span>
          <span>{uiTranslate(snapshot?.privacy.mirrorDetected ? "已检测到屏幕镜像" : "未检测到屏幕镜像", $uiLanguage)}</span>
        </div>
      </aside>
    </div>
<Dialog.Root bind:open={privacyAppPickerOpen}>
  <Dialog.Portal>
    <Dialog.Overlay class="modal-backdrop" />
    <Dialog.Content class="modal privacy-app-modal">
      <header class="modal-header">
        <div>
          <Dialog.Title class="modal-title" level={2}>{uiTranslate("添加受保护应用", $uiLanguage)}</Dialog.Title>
          <Dialog.Description class="modal-description">{uiTranslate("选择后，该应用当前桌面上的所有可见窗口都会被遮挡。", $uiLanguage)}</Dialog.Description>
        </div>
        <Dialog.Close class="modal-close-button" aria-label={uiTranslate("关闭", $uiLanguage)}><X size={19} /></Dialog.Close>
      </header>
      <div class="privacy-app-modal-body">
        <label class="search-box privacy-app-search">
          <MagnifyingGlass size={17} />
          <input bind:value={privacyAppSearch} placeholder={uiTranslate("搜索已安装应用", $uiLanguage)} />
        </label>
        <div class="privacy-app-picker-list">
          {#each availablePrivacyApps() as app (app.path)}
            <button on:click={() => addProtectedApp(app)}>
              <span class="protected-app-icon">
                <InstalledAppIcon path={app.path} name={app.name} initialIcon={app.iconDataUrl} />
              </span>
              <span><strong>{app.name}</strong><small>{app.identifier ?? app.path}</small></span>
              <Plus size={17} />
            </button>
          {:else}
            <div class="privacy-app-empty">{uiTranslate("没有可添加的应用", $uiLanguage)}</div>
          {/each}
        </div>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
