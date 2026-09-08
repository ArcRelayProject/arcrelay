<script lang="ts">
  import { dismissibleDetailsDropdown } from "../../dismissibleDropdown";
  import AppSelect from "../../components/AppSelect.svelte";
  import { translate as uiTranslate, language as uiLanguage } from "../../i18n";
  import { Dialog } from "bits-ui";
  import QRCode from "qrcode";
  import {
    ArrowSquareOut,
    CheckCircle,
    Copy,
    DotsThree,
    DownloadSimple,
    Eye,
    EyeSlash,
    FolderOpen,
    FolderSimplePlus,
    Gear,
    GlobeHemisphereWest,
    House,
    Key,
    LockKey,
    PencilSimple,
    ShieldCheck,
    SignOut,
    Trash,
    WarningCircle,
    X,
  } from "phosphor-svelte";

  import { bridge } from "../../bridge";
  import type {
    AppSettings,
    LocalSharedDirectory,
    WebAccessMode,
    WebGatewayStatus,
  } from "../../types";

  type WebPolicy = {
    mode: WebAccessMode;
    listed: boolean;
    allowPreview: boolean;
    allowDownload: boolean;
  };

  type DialogKind =
    | "enable"
    | "gateway"
    | "access"
    | "password"
    | "link"
    | "revoke"
    | "remove"
    | null;

  export let appSettings: AppSettings;
  export let settingsSaving = false;
  export let localShares: LocalSharedDirectory[] = [];
  export let localSharesLoaded = false;
  export let localShareBusy = "";
  export let webGatewayStatus: WebGatewayStatus | null = null;

  export let addLocalShare: () => void | Promise<void>;
  export let toggleLocalShareWriteAccess: (share: LocalSharedDirectory) => void | Promise<void>;
  export let updateLocalShareWebAccess: (
    share: LocalSharedDirectory,
    policy: WebPolicy,
    newPassword?: string,
  ) => void | Promise<void>;
  export let refreshWebGatewayStatus: () => void | Promise<void>;
  export let revokeWebAccess: (shareId?: string) => void | Promise<void>;
  export let removeLocalShare: (share: LocalSharedDirectory) => void | Promise<void>;
  export let patchAppSettings: (patch: import("../../ipc/generated").AppSettingsPatch, message?: string) => void | Promise<void>;

  let activeDialog: DialogKind = null;
  let selectedShare: LocalSharedDirectory | null = null;
  let enableAcknowledged = false;
  let gatewayDraft = { siteName: "", port: 8767, sessionIdleMinutes: 720, allowVpnPrivate: false };
  let accessDraft: WebPolicy & { writable: boolean } = {
    writable: false,
    mode: "disabled",
    listed: true,
    allowPreview: true,
    allowDownload: true,
  };
  let passwordValue = "";
  let passwordConfirmation = "";
  let passwordVisible = false;
  let passwordBusy = false;
  let qrUrl = "";
  let qrCode = "";
  let copiedUrl = "";
  let revokeScope: "share" | "all" = "share";

  $: gatewayUrl = webGatewayStatus?.addresses[0] ?? "";
  $: dialogBusy = settingsSaving || passwordBusy || Boolean(localShareBusy);
  $: passwordValid = passwordValue.length >= 8 && passwordValue === passwordConfirmation;

  function policyFromShare(share: LocalSharedDirectory): WebPolicy {
    return {
      mode: share.web.mode,
      listed: share.web.listed,
      allowPreview: share.web.allowPreview,
      allowDownload: share.web.allowDownload,
    };
  }

  function webModeLabel(mode: WebAccessMode) {
    return mode === "password" ? "密码保护" : mode === "public" ? "局域网公开" : "未开放";
  }

  function webModeDescription(mode: WebAccessMode) {
    return mode === "password" ? "访客输入密码后访问" : mode === "public" ? "同一局域网内直接访问" : "未通过浏览器共享";
  }

  function directShareUrl(share: LocalSharedDirectory) {
    return gatewayUrl ? `${gatewayUrl}/s/${share.web.slug}` : "";
  }

  function closeDialog() {
    if (dialogBusy) return;
    activeDialog = null;
    selectedShare = null;
    passwordValue = "";
    passwordConfirmation = "";
    qrUrl = "";
    qrCode = "";
  }

  function openEnableDialog() {
    enableAcknowledged = false;
    activeDialog = "enable";
  }

  function openGatewayDialog() {
    gatewayDraft = {
      siteName: appSettings.webFiles.siteName,
      port: appSettings.webFiles.port,
      sessionIdleMinutes: appSettings.webFiles.sessionIdleMinutes,
      allowVpnPrivate: appSettings.webFiles.allowVpnPrivate,
    };
    activeDialog = "gateway";
  }

  function openAccessDialog(share: LocalSharedDirectory) {
    selectedShare = share;
    accessDraft = { writable: share.writable, ...policyFromShare(share) };
    activeDialog = "access";
  }

  function openPasswordDialog(share: LocalSharedDirectory, policy = policyFromShare(share)) {
    selectedShare = share;
    accessDraft = { writable: share.writable, ...policy, mode: "password" };
    passwordValue = "";
    passwordConfirmation = "";
    passwordVisible = false;
    activeDialog = "password";
  }

  async function openLinkDialog(share: LocalSharedDirectory) {
    const url = directShareUrl(share);
    if (!url) return;
    selectedShare = share;
    qrUrl = url;
    qrCode = "";
    copiedUrl = "";
    activeDialog = "link";
    try {
      qrCode = await bridge.webGatewayQrCode(url);
    } catch {
      qrCode = "";
    }
    if (!qrCode) qrCode = await QRCode.toDataURL(url, { width: 420, margin: 1 });
  }

  function openRevokeDialog(share?: LocalSharedDirectory) {
    selectedShare = share ?? null;
    revokeScope = share ? "share" : "all";
    activeDialog = "revoke";
  }

  function openRemoveDialog(share: LocalSharedDirectory) {
    selectedShare = share;
    activeDialog = "remove";
  }

  async function copyUrl(url: string) {
    if (!url) return;
    await navigator.clipboard.writeText(url);
    copiedUrl = url;
    setTimeout(() => copiedUrl === url && (copiedUrl = ""), 1800);
  }

  async function enableGateway() {
    if (!enableAcknowledged || settingsSaving) return;
    await patchAppSettings(
      { webFiles: { enabled: true } },
      "浏览器访问已开启",
    );
    await refreshWebGatewayStatus();
    closeDialog();
  }

  async function saveGatewaySettings() {
    if (settingsSaving) return;
    await patchAppSettings(
      { webFiles: { ...appSettings.webFiles, ...gatewayDraft } },
      "浏览器访问设置已保存",
    );
    await refreshWebGatewayStatus();
    closeDialog();
  }

  async function disableGateway() {
    if (settingsSaving) return;
    await patchAppSettings(
      { webFiles: { enabled: false } },
      "浏览器访问已关闭",
    );
    await refreshWebGatewayStatus();
    closeDialog();
  }

  async function saveAccessSettings() {
    if (!selectedShare || dialogBusy) return;
    const share = selectedShare;
    if (accessDraft.mode === "password" && !share.web.hasPassword) {
      openPasswordDialog(share, accessDraft);
      return;
    }
    try {
      if (accessDraft.writable !== share.writable) await toggleLocalShareWriteAccess(share);
      await updateLocalShareWebAccess(share, accessDraft);
      closeDialog();
    } catch {
      // The owning view reports the error; keep the dialog open for correction or retry.
    }
  }

  async function savePassword() {
    if (!selectedShare || !passwordValid || passwordBusy) return;
    passwordBusy = true;
    try {
      if (accessDraft.writable !== selectedShare.writable) {
        await toggleLocalShareWriteAccess(selectedShare);
      }
      await updateLocalShareWebAccess(selectedShare, { ...accessDraft, mode: "password" }, passwordValue);
      activeDialog = null;
      selectedShare = null;
      passwordValue = "";
      passwordConfirmation = "";
    } catch {
      // The owning view reports the error; keep the dialog open for correction or retry.
    } finally {
      passwordBusy = false;
    }
  }

  async function confirmRevoke() {
    if (dialogBusy) return;
    await revokeWebAccess(revokeScope === "share" ? selectedShare?.id : undefined);
    closeDialog();
  }

  async function confirmRemove() {
    if (!selectedShare || dialogBusy) return;
    try {
      await removeLocalShare(selectedShare);
      closeDialog();
    } catch {
      // The owning view reports the error; keep the dialog open for retry.
    }
  }
</script>

<section class="settings-tab-content file-sharing-settings" aria-labelledby="file-sharing-title">
  <header class="file-sharing-page-heading">
    <div>
      <h2 id="file-sharing-title">{uiTranslate("文件共享", $uiLanguage)}</h2>
      <p>{uiTranslate("在本机与已配对设备之间安全地共享文件夹，文件始终保留在你的设备上。", $uiLanguage)}</p>
    </div>
    <button class="primary-button file-sharing-add" disabled={Boolean(localShareBusy)} on:click={addLocalShare}>
      <FolderSimplePlus size={18} /> {uiTranslate("添加文件夹", $uiLanguage)}
    </button>
  </header>

  <div class:error={Boolean(webGatewayStatus?.lastError)} class:running={webGatewayStatus?.running} class="gateway-overview">
    <span class="gateway-overview-icon"><GlobeHemisphereWest size={25} weight="duotone" /></span>
    <div class="gateway-overview-copy">
      <span class="gateway-title-line">
        <strong>{uiTranslate(webGatewayStatus?.running ? "浏览器访问已开启" : "浏览器访问已关闭", $uiLanguage)}</strong>
        <span class="gateway-state"><i></i>{uiTranslate(webGatewayStatus?.lastError ? "启动失败" : webGatewayStatus?.running ? "运行中" : "已关闭", $uiLanguage)}</span>
      </span>
      <small>
        {#if webGatewayStatus?.lastError}
          {webGatewayStatus.lastError}
        {:else if webGatewayStatus?.running}
          {webGatewayStatus.sessionCount} {uiTranslate("个活跃会话", $uiLanguage)}{gatewayUrl ? ` · ${gatewayUrl}` : ""}
        {:else}{uiTranslate("开启后，同一局域网中的浏览器可以访问已授权文件夹。", $uiLanguage)}{/if}
      </small>
    </div>
    <div class="gateway-overview-actions">
      {#if webGatewayStatus?.running && gatewayUrl}
        <button class="primary-button" on:click={() => bridge.openWebGatewayUrl(gatewayUrl)}><ArrowSquareOut size={17} /> {uiTranslate("打开共享主页", $uiLanguage)}</button>
      {:else}
        <button class="primary-button" disabled={settingsSaving} on:click={openEnableDialog}>{uiTranslate("开启浏览器访问", $uiLanguage)}</button>
      {/if}
      <button class="secondary-button" on:click={openGatewayDialog}><Gear size={17} /> {uiTranslate("管理", $uiLanguage)}</button>
    </div>
  </div>

  <div class="gateway-security-note">
    <ShieldCheck size={17} />
    <span><strong>{uiTranslate("HTTP 局域网共享", $uiLanguage)}</strong>{uiTranslate("不会加密密码和文件内容，请勿在公共 Wi-Fi 或端口转发环境中开启。", $uiLanguage)}</span>
  </div>

  <div class="file-sharing-section-heading">
    <div><h3>{uiTranslate("共享文件夹", $uiLanguage)}</h3><span>{localShares.length} {uiTranslate("个位置", $uiLanguage)}</span></div>
    <button class="text-button" disabled={!webGatewayStatus?.running} on:click={() => openRevokeDialog()}>{uiTranslate("撤销全部浏览器会话", $uiLanguage)}</button>
  </div>

  <div class="local-share-table">
    {#if localShareBusy === "load" && !localSharesLoaded}
      <div class="local-share-settings-empty"><span class="loading-spinner"></span><span>{uiTranslate("正在读取共享位置…", $uiLanguage)}</span></div>
    {:else if localShares.length === 0}
      <div class="local-share-settings-empty">
        <FolderOpen size={28} />
        <strong>{uiTranslate("还没有共享文件夹", $uiLanguage)}</strong>
        <span>{uiTranslate("添加后，已配对设备可以在“远程文件”中访问。", $uiLanguage)}</span>
        <button class="text-button" disabled={Boolean(localShareBusy)} on:click={addLocalShare}>{uiTranslate("添加第一个文件夹", $uiLanguage)}</button>
      </div>
    {:else}
      <div class="local-share-list-heading" aria-hidden="true">
        <span>{uiTranslate("文件夹", $uiLanguage)}</span><span>{uiTranslate("已配对设备", $uiLanguage)}</span><span>{uiTranslate("浏览器访问", $uiLanguage)}</span><span>{uiTranslate("操作", $uiLanguage)}</span>
      </div>
      {#each localShares as share (share.id)}
        <div class="local-share-row">
          <div class="local-share-identity">
            <span class="local-share-folder-icon"><FolderOpen size={22} weight="duotone" /></span>
            <span class="local-share-copy">
              <span class="local-share-name"><strong>{share.name}</strong><small>{uiTranslate("已共享", $uiLanguage)}</small></span>
              <small title={share.path}>{share.path}</small>
            </span>
          </div>
          <div class:write-enabled={share.writable} class="share-access-status">
            {#if share.writable}<PencilSimple size={16} />{:else}<Eye size={16} />{/if}
            <span><strong>{uiTranslate(share.writable ? "可读写" : "只读", $uiLanguage)}</strong><small>{uiTranslate(share.writable ? "可查看与编辑" : "可查看与下载", $uiLanguage)}</small></span>
          </div>
          <div class:enabled={share.web.mode !== "disabled"} class:protected={share.web.mode === "password"} class="share-access-status browser-access-status">
            {#if share.web.mode === "password"}<LockKey size={16} />{:else}<GlobeHemisphereWest size={16} />{/if}
            <span><strong>{uiTranslate(webModeLabel(share.web.mode), $uiLanguage)}</strong><small>{webModeDescription(share.web.mode)}</small></span>
          </div>
          <div class="local-share-row-actions">
            <button class="secondary-button manage-share-button" disabled={Boolean(localShareBusy)} on:click={() => openAccessDialog(share)}>{uiTranslate("管理访问", $uiLanguage)}</button>
            <details class="share-more-menu" use:dismissibleDetailsDropdown>
              <summary aria-label={uiTranslate((`更多 ${share.name} 操作`), $uiLanguage)}><DotsThree size={20} /></summary>
              <div class="share-more-popover">
                {#if share.web.mode !== "disabled" && directShareUrl(share)}
                  <button on:click={() => openLinkDialog(share)}><ArrowSquareOut size={16} /> {uiTranslate("分享链接", $uiLanguage)}</button>
                  {#if share.web.mode === "password"}<button on:click={() => openPasswordDialog(share)}><Key size={16} /> {uiTranslate("修改密码", $uiLanguage)}</button>{/if}
                  <button on:click={() => openRevokeDialog(share)}><SignOut size={16} /> {uiTranslate("撤销会话", $uiLanguage)}</button>
                  <span class="share-menu-separator"></span>
                {/if}
                <button class="danger-menu-item" on:click={() => openRemoveDialog(share)}><Trash size={16} /> {uiTranslate("停止共享", $uiLanguage)}</button>
              </div>
            </details>
          </div>
        </div>
      {/each}
    {/if}
  </div>

  <Dialog.Root
    open={activeDialog !== null}
    onOpenChange={(open) => {
      if (!open) closeDialog();
    }}
  >
    <Dialog.Portal>
      <Dialog.Overlay class="modal-backdrop file-sharing-dialog-backdrop" />
      <Dialog.Content class={`modal file-sharing-dialog file-sharing-dialog-${activeDialog ?? "closed"}`}>
        {#if activeDialog === "enable"}
          <header class="modal-header file-dialog-heading">
            <span class="file-dialog-icon"><GlobeHemisphereWest size={27} /><ShieldCheck size={18} /></span>
            <div><Dialog.Title class="modal-title" level={2}>{uiTranslate("开启浏览器访问？", $uiLanguage)}</Dialog.Title><Dialog.Description class="modal-description">{uiTranslate("开启后，浏览器可通过局域网访问本机共享内容。请注意：", $uiLanguage)}</Dialog.Description></div>
            <Dialog.Close class="modal-close-button" aria-label={uiTranslate("关闭", $uiLanguage)}><X size={19} /></Dialog.Close>
          </header>
          <div class="modal-body enable-gateway-body">
            <div class="gateway-consequence-list">
              <span><GlobeHemisphereWest size={19} /><span><strong>{uiTranslate("同一局域网设备可以连接", $uiLanguage)}</strong><small>{uiTranslate("任何连接到当前局域网的设备都可以尝试访问。", $uiLanguage)}</small></span></span>
              <span><LockKey size={19} /><span><strong>{uiTranslate("HTTP 不会加密密码和文件内容", $uiLanguage)}</strong><small>{uiTranslate("数据在传输中为明文。", $uiLanguage)}</small></span></span>
              <span><WarningCircle size={19} /><span><strong>{uiTranslate("请勿在公共 Wi-Fi 或端口转发环境开启", $uiLanguage)}</strong><small>{uiTranslate("仅适用于你信任的家庭、办公或专用网络。", $uiLanguage)}</small></span></span>
            </div>
            <label class="gateway-acknowledgement"><input type="checkbox" bind:checked={enableAcknowledged} /> <span>{uiTranslate("我确认当前网络可信", $uiLanguage)}</span></label>
          </div>
          <footer class="modal-footer"><Dialog.Close class="secondary-button">{uiTranslate("取消", $uiLanguage)}</Dialog.Close><button class="primary-button" disabled={!enableAcknowledged || settingsSaving} on:click={enableGateway}>{uiTranslate("开启访问", $uiLanguage)}</button></footer>

        {:else if activeDialog === "gateway"}
          <header class="modal-header">
            <div><Dialog.Title class="modal-title" level={2}>{uiTranslate("浏览器访问设置", $uiLanguage)}</Dialog.Title><Dialog.Description class="modal-description">{uiTranslate("管理共享主页的网络与会话配置", $uiLanguage)}</Dialog.Description></div>
            <div class="dialog-heading-actions"><span class:running={webGatewayStatus?.running} class="dialog-running-state"><i></i>{uiTranslate(webGatewayStatus?.running ? "运行中" : "已关闭", $uiLanguage)}</span><Dialog.Close class="modal-close-button" aria-label={uiTranslate("关闭", $uiLanguage)}><X size={19} /></Dialog.Close></div>
          </header>
          <div class="modal-body gateway-settings-body">
            <div class="gateway-field-grid">
              <label><span>{uiTranslate("站点名称", $uiLanguage)}</span><input maxlength="128" placeholder={uiTranslate("使用设备名称", $uiLanguage)} bind:value={gatewayDraft.siteName} /></label>
              <label class="gateway-port-field"><span>{uiTranslate("端口", $uiLanguage)}</span><input type="number" min="1" max="65535" bind:value={gatewayDraft.port} /></label>
            </div>
            <label class="gateway-select-field"><span>{uiTranslate("会话有效期", $uiLanguage)}</span><AppSelect bind:value={gatewayDraft.sessionIdleMinutes} aria-label={uiTranslate("会话有效期", $uiLanguage)}
              options={[
                { value: 30, label: uiTranslate("30 分钟", $uiLanguage) },
                { value: 120, label: uiTranslate("2 小时", $uiLanguage) },
                { value: 720, label: uiTranslate("12 小时", $uiLanguage) },
                { value: 1440, label: uiTranslate("24 小时", $uiLanguage) },
              ]}
            /></label>
            <button class:checked={gatewayDraft.allowVpnPrivate} class="gateway-dialog-toggle" role="switch" aria-checked={gatewayDraft.allowVpnPrivate} on:click={() => (gatewayDraft.allowVpnPrivate = !gatewayDraft.allowVpnPrivate)}>
              <span><strong>{uiTranslate("允许 VPN 私网来源", $uiLanguage)}</strong><small>{uiTranslate("允许来自 VPN 或覆盖网络的受信任设备访问共享主页。", $uiLanguage)}</small></span><span class:checked={gatewayDraft.allowVpnPrivate} class="switch-control"><span></span></span>
            </button>
            <label class="gateway-address-field"><span>{uiTranslate("访问地址（只读）", $uiLanguage)}</span><span class="gateway-readonly-address"><code>{uiTranslate(gatewayUrl || "开启后生成访问地址", $uiLanguage)}</code><button aria-label={uiTranslate("复制访问地址", $uiLanguage)} disabled={!gatewayUrl} on:click={() => copyUrl(gatewayUrl)}><Copy size={17} /></button></span></label>
            {#if appSettings.webFiles.enabled}<button class="gateway-stop-button" on:click={disableGateway}>{uiTranslate("关闭浏览器访问", $uiLanguage)}</button>{/if}
          </div>
          <footer class="modal-footer"><Dialog.Close class="secondary-button">{uiTranslate("取消", $uiLanguage)}</Dialog.Close><button class="primary-button" disabled={settingsSaving || gatewayDraft.port < 1 || gatewayDraft.port > 65535} on:click={saveGatewaySettings}>{uiTranslate("保存设置", $uiLanguage)}</button></footer>

        {:else if activeDialog === "access" && selectedShare}
          <header class="modal-header">
            <div><Dialog.Title class="modal-title" level={2}>{uiTranslate("管理“", $uiLanguage)}{selectedShare.name}{uiTranslate("”访问", $uiLanguage)}</Dialog.Title><Dialog.Description class="modal-description">{uiTranslate("分别控制已配对设备和浏览器访客", $uiLanguage)}</Dialog.Description></div>
            <Dialog.Close class="modal-close-button" aria-label={uiTranslate("关闭", $uiLanguage)}><X size={19} /></Dialog.Close>
          </header>
          <div class="modal-body share-access-body">
            <fieldset><legend>{uiTranslate("已配对设备", $uiLanguage)}</legend><div class="share-segmented-control"><button class:active={!accessDraft.writable} on:click={() => (accessDraft.writable = false)}><Eye size={16} /> {uiTranslate("只读", $uiLanguage)}</button><button class:active={accessDraft.writable} on:click={() => (accessDraft.writable = true)}><PencilSimple size={16} /> {uiTranslate("可读写", $uiLanguage)}</button></div></fieldset>
            <fieldset><legend>{uiTranslate("浏览器访问", $uiLanguage)}</legend><div class="web-mode-options">
              <label class:selected={accessDraft.mode === "disabled"}><input type="radio" name="web-mode" value="disabled" bind:group={accessDraft.mode} /><GlobeHemisphereWest size={18} /><span><strong>{uiTranslate("未开放", $uiLanguage)}</strong><small>{uiTranslate("不通过浏览器共享", $uiLanguage)}</small></span></label>
              <label class:selected={accessDraft.mode === "public"}><input type="radio" name="web-mode" value="public" bind:group={accessDraft.mode} /><GlobeHemisphereWest size={18} /><span><strong>{uiTranslate("局域网公开", $uiLanguage)}</strong><small>{uiTranslate("同一局域网内的访客无需密码", $uiLanguage)}</small></span></label>
              <label class:selected={accessDraft.mode === "password"}><input type="radio" name="web-mode" value="password" bind:group={accessDraft.mode} /><LockKey size={18} /><span><strong>{uiTranslate("密码保护", $uiLanguage)}</strong><small>{uiTranslate("访客输入密码后访问", $uiLanguage)}</small></span></label>
            </div></fieldset>
            <fieldset class:disabled={accessDraft.mode === "disabled"}><legend>{uiTranslate("访客能力", $uiLanguage)}</legend><div class="visitor-capability-list">
              <button role="switch" aria-checked={accessDraft.listed} disabled={accessDraft.mode === "disabled"} on:click={() => (accessDraft.listed = !accessDraft.listed)}><House size={16} /><span>{uiTranslate("在共享主页显示", $uiLanguage)}</span><span class:checked={accessDraft.listed} class="switch-control"><span></span></span></button>
              <button role="switch" aria-checked={accessDraft.allowPreview} disabled={accessDraft.mode === "disabled"} on:click={() => (accessDraft.allowPreview = !accessDraft.allowPreview)}><Eye size={16} /><span>{uiTranslate("允许预览", $uiLanguage)}</span><span class:checked={accessDraft.allowPreview} class="switch-control"><span></span></span></button>
              <button role="switch" aria-checked={accessDraft.allowDownload} disabled={accessDraft.mode === "disabled"} on:click={() => (accessDraft.allowDownload = !accessDraft.allowDownload)}><DownloadSimple size={16} /><span>{uiTranslate("允许下载", $uiLanguage)}</span><span class:checked={accessDraft.allowDownload} class="switch-control"><span></span></span></button>
            </div></fieldset>
            <p class="share-session-note">{uiTranslate("网关当前共", $uiLanguage)} {webGatewayStatus?.sessionCount ?? 0} {uiTranslate("个浏览器会话", $uiLanguage)}</p>
          </div>
          <footer class="modal-footer"><Dialog.Close class="secondary-button">{uiTranslate("取消", $uiLanguage)}</Dialog.Close><button class="primary-button" disabled={dialogBusy} on:click={saveAccessSettings}>{uiTranslate("保存更改", $uiLanguage)}</button></footer>

        {:else if activeDialog === "password" && selectedShare}
          <header class="modal-header">
            <div><Dialog.Title class="modal-title" level={2}>{uiTranslate(selectedShare.web.hasPassword ? "修改" : "设置", $uiLanguage)}{uiTranslate("访问密码", $uiLanguage)}</Dialog.Title><Dialog.Description class="modal-description">{uiTranslate("为“", $uiLanguage)}{selectedShare.name}{uiTranslate("”的浏览器访问添加保护", $uiLanguage)}</Dialog.Description></div>
            <Dialog.Close class="modal-close-button" aria-label={uiTranslate("关闭", $uiLanguage)} disabled={passwordBusy}><X size={19} /></Dialog.Close>
          </header>
          <form on:submit|preventDefault={savePassword}>
            <div class="modal-body password-dialog-body">
              <label><span>{uiTranslate("新密码", $uiLanguage)}</span><span class="password-input-wrap"><input type={passwordVisible ? "text" : "password"} bind:value={passwordValue} minlength="8" maxlength="256" autocomplete="new-password" /><button type="button" aria-label={uiTranslate((passwordVisible ? "隐藏密码" : "显示密码"), $uiLanguage)} on:click={() => (passwordVisible = !passwordVisible)}>{#if passwordVisible}<EyeSlash size={17} />{:else}<Eye size={17} />{/if}</button></span></label>
              <label><span>{uiTranslate("确认密码", $uiLanguage)}</span><span class="password-input-wrap"><input type={passwordVisible ? "text" : "password"} bind:value={passwordConfirmation} minlength="8" maxlength="256" autocomplete="new-password" /><button type="button" aria-label={uiTranslate((passwordVisible ? "隐藏密码" : "显示密码"), $uiLanguage)} on:click={() => (passwordVisible = !passwordVisible)}>{#if passwordVisible}<EyeSlash size={17} />{:else}<Eye size={17} />{/if}</button></span></label>
              <div class="password-strength"><span>{uiTranslate("安全强度：", $uiLanguage)}{uiTranslate(passwordValue.length >= 12 ? "良好" : passwordValue.length >= 8 ? "可用" : "需加强", $uiLanguage)}</span><span class:filled={passwordValue.length >= 8}></span><span class:filled={passwordValue.length >= 10}></span><span class:filled={passwordValue.length >= 12}></span></div>
              <small>{uiTranslate("至少 8 位，建议包含数字与符号", $uiLanguage)}</small>
              {#if passwordConfirmation && passwordValue !== passwordConfirmation}<p class="password-mismatch">{uiTranslate("两次输入的密码不一致", $uiLanguage)}</p>{/if}
              <p class="password-session-warning"><WarningCircle size={17} />{uiTranslate("保存后会撤销此文件夹的旧会话，访客需要重新登录。", $uiLanguage)}</p>
            </div>
            <footer class="modal-footer"><Dialog.Close class="secondary-button" disabled={passwordBusy}>{uiTranslate("取消", $uiLanguage)}</Dialog.Close><button type="submit" class="primary-button" disabled={!passwordValid || passwordBusy}>{uiTranslate(passwordBusy ? "正在保存…" : "保存并撤销旧会话", $uiLanguage)}</button></footer>
          </form>

        {:else if activeDialog === "link" && selectedShare}
          <header class="modal-header share-link-heading"><div><Dialog.Title class="modal-title" level={2}>{uiTranslate("分享“", $uiLanguage)}{selectedShare.name}”</Dialog.Title><Dialog.Description class="modal-description">{uiTranslate("使用同一局域网中的浏览器打开", $uiLanguage)}</Dialog.Description></div><Dialog.Close class="modal-close-button" aria-label={uiTranslate("关闭", $uiLanguage)}><X size={19} /></Dialog.Close></header>
          <div class="modal-body share-link-body">
            <div class="share-qr-frame">{#if qrCode}<img src={qrCode} alt={uiTranslate((`${selectedShare.name} 访问二维码`), $uiLanguage)} />{:else}<span class="loading-spinner"></span>{/if}</div>
            <span class:public={selectedShare.web.mode === "public"} class="share-protection-badge">{#if selectedShare.web.mode === "password"}<LockKey size={15} /> {uiTranslate("密码保护", $uiLanguage)}{:else}<GlobeHemisphereWest size={15} /> {uiTranslate("局域网公开", $uiLanguage)}{/if}</span>
            <label class="share-link-field"><span>{uiTranslate("访问链接", $uiLanguage)}</span><span><code>{qrUrl}</code><button aria-label={uiTranslate("复制链接", $uiLanguage)} on:click={() => copyUrl(qrUrl)}><Copy size={17} /></button><button aria-label={uiTranslate("打开链接", $uiLanguage)} on:click={() => bridge.openWebGatewayUrl(qrUrl)}><ArrowSquareOut size={17} /></button></span></label>
            {#if copiedUrl === qrUrl}<span class="link-copied-state"><CheckCircle size={16} weight="fill" /> {uiTranslate("链接已复制", $uiLanguage)}</span>{/if}
            <p class="share-link-note"><ShieldCheck size={17} />{uiTranslate("链接仅在当前局域网可用；", $uiLanguage)}{uiTranslate(selectedShare.web.mode === "password" ? "访客仍需输入访问密码。" : "任何获得链接的局域网访客都能访问。", $uiLanguage)}</p>
          </div>
          <footer class="modal-footer centered-footer"><Dialog.Close class="secondary-button">{uiTranslate("完成", $uiLanguage)}</Dialog.Close></footer>

        {:else if activeDialog === "revoke"}
          <header class="modal-header file-dialog-heading">
            <span class="file-dialog-icon danger"><SignOut size={26} /></span>
            <div><Dialog.Title class="modal-title" level={2}>{uiTranslate("撤销浏览器访问？", $uiLanguage)}</Dialog.Title><Dialog.Description class="modal-description">{uiTranslate("已登录的浏览器会立即退出，但不会删除文件或关闭共享。", $uiLanguage)}</Dialog.Description></div>
            <Dialog.Close class="modal-close-button" aria-label={uiTranslate("关闭", $uiLanguage)}><X size={19} /></Dialog.Close>
          </header>
          <div class="modal-body revoke-dialog-body">
            <fieldset><legend>{uiTranslate("撤销范围", $uiLanguage)}</legend>
              {#if selectedShare}<label class:selected={revokeScope === "share"}><input type="radio" name="revoke-scope" value="share" bind:group={revokeScope} /><span>{uiTranslate("仅", $uiLanguage)} {selectedShare.name}</span></label>{/if}
              <label class:selected={revokeScope === "all"}><input type="radio" name="revoke-scope" value="all" bind:group={revokeScope} /><span>{uiTranslate("所有共享（", $uiLanguage)}{webGatewayStatus?.sessionCount ?? 0} {uiTranslate("个会话）", $uiLanguage)}</span></label>
            </fieldset>
            <p class="neutral-dialog-note">{uiTranslate("新访客仍可继续访问；密码保护的访客需要重新登录。", $uiLanguage)}</p>
          </div>
          <footer class="modal-footer"><Dialog.Close class="secondary-button">{uiTranslate("取消", $uiLanguage)}</Dialog.Close><button class="danger-button filled-danger-button" disabled={dialogBusy} on:click={confirmRevoke}>{uiTranslate("立即撤销", $uiLanguage)}</button></footer>

        {:else if activeDialog === "remove" && selectedShare}
          <header class="modal-header file-dialog-heading">
            <span class="file-dialog-icon danger"><Trash size={26} /></span>
            <div><Dialog.Title class="modal-title" level={2}>{uiTranslate("停止共享“", $uiLanguage)}{selectedShare.name}”？</Dialog.Title><Dialog.Description class="modal-description">{uiTranslate("此操作会影响以下访问方式：", $uiLanguage)}</Dialog.Description></div>
            <Dialog.Close class="modal-close-button" aria-label={uiTranslate("关闭", $uiLanguage)}><X size={19} /></Dialog.Close>
          </header>
          <div class="modal-body remove-dialog-body">
            <span><WarningCircle size={18} /><span>{uiTranslate("已配对设备将无法再访问", $uiLanguage)}</span></span>
            <span><WarningCircle size={18} /><span>{uiTranslate("浏览器访客将立即断开", $uiLanguage)}</span></span>
            <p class="files-safe-note"><CheckCircle size={20} />{uiTranslate("本机文件不会被删除，“", $uiLanguage)}{selectedShare.name}{uiTranslate("”中的内容保持不变。", $uiLanguage)}</p>
          </div>
          <footer class="modal-footer"><Dialog.Close class="secondary-button">{uiTranslate("取消", $uiLanguage)}</Dialog.Close><button class="danger-button filled-danger-button" disabled={dialogBusy} on:click={confirmRemove}>{uiTranslate("停止共享", $uiLanguage)}</button></footer>
        {/if}
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
</section>
