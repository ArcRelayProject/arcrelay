<script lang="ts">
  import { onMount } from "svelte";
  import { Dialog } from "bits-ui";
  import AppSelect from "../../components/AppSelect.svelte";
  import {
    ArrowClockwise,
    Bell,
    CaretRight,
    CheckCircle,
    Copy,
    Eye,
    Gear,
    LockKeyOpen,
    Plus,
    Robot,
    ShieldCheck,
    WarningCircle,
    X,
  } from "phosphor-svelte";
  import { clipboardBridge } from "../../clipboard/bridge";
  import { mcpConfigText, type McpConfigFormat } from "../../mcpClientConfig";
  import { bridge } from "../../bridge";
  import { errorMessage } from "../../app_helpers";
  import { translate as tr, language, locale } from "../../i18n";
  import type { McpClientView, McpPermissions, ConfigurationChange } from "../../ipc/generated";
  import {
    detectPermissionPreset,
    permissionPresets,
    type PermissionPreset,
  } from "./settingsModels";

  export let mcpRunning = false;

  type ConfigFormat = McpConfigFormat;

  let clients: McpClientView[] = [];
  let activeClients: McpClientView[] = [];
  let revokedClients: McpClientView[] = [];
  let changes: ConfigurationChange[] = [];
  let name = "";
  let busy = false;
  let message = "";
  let error = "";
  let createOpen = false;
  let manageOpen = false;
  let managedClient: McpClientView | null = null;
  let managedPermissions: McpPermissions = { ...permissionPresets.daily };
  let createPreset: PermissionPreset = "daily";
  let defaultPreset: PermissionPreset = "daily";
  let format: ConfigFormat = "installPrompt";

  $: activeClients = clients.filter(client => !client.revoked);
  $: revokedClients = clients.filter(client => client.revoked);

  const scopes: { key: keyof McpPermissions; label: string; description: string }[] = [
    { key: "notifications", label: "发送和读取通知", description: "发送通知并查询已读状态" },
    { key: "read", label: "读取配置和活动", description: "查询快捷动作、自动化、本机应用和设备" },
    { key: "manage", label: "创建和修改配置", description: "管理快捷动作和自动化，包含删除与恢复" },
    { key: "enable", label: "启停和绑定快捷键", description: "启停自动化，修改动作和全局快捷键" },
    { key: "execute", label: "运行和取消动作", description: "允许 Agent 发起真实执行和测试" },
    { key: "scripts", label: "使用脚本", description: "允许配置和执行 Shell、AppleScript 等脚本" },
  ];

  const permissionOptions: { value: PermissionPreset; label: string; description: string; icon: typeof Eye }[] = [
    { value: "readOnly", label: "只读", description: "可查看配置与通知", icon: Eye },
    { value: "daily", label: "日常管理", description: "可查看并管理配置", icon: Gear },
    { value: "full", label: "完全控制", description: "可管理并运行脚本", icon: ShieldCheck },
  ];

  let formatOptions: { value: ConfigFormat; label: string }[] = [];
  $: formatOptions = [
    { value: "installPrompt", label: tr("给 Agent 的安装说明（推荐）", $language) },
    { value: "codex", label: "Codex · TOML" },
    { value: "claudeDesktop", label: "Claude Desktop · JSON" },
    { value: "httpJson", label: "Claude Code / HTTP · JSON" },
    { value: "codexHttp", label: "Codex HTTP · TOML" },
  ];

  onMount(() => {
    const savedPreset = window.localStorage.getItem("arcrelay-agent-default-permission");
    if (savedPreset === "readOnly" || savedPreset === "daily" || savedPreset === "full") defaultPreset = savedPreset;
    createPreset = defaultPreset;
    void refresh().catch(e => { error = errorMessage(e); });
  });

  async function refresh() {
    [clients, changes] = await Promise.all([bridge.listMcpClients(), bridge.listMcpConfigurationChanges(12)]);
  }

  async function task(work: () => Promise<void>) {
    if (busy) return;
    busy = true;
    error = "";
    message = "";
    try { await work(); }
    catch (e) { error = errorMessage(e); }
    finally { busy = false; }
  }

  async function copy(id: string) {
    try {
      const config = await bridge.getMcpClientConfig(id);
      await clipboardBridge.copyText(mcpConfigText(config, format, $language));
    } catch {
      // Do not surface credential-bearing backend errors or undo completed authorization.
      throw new Error(tr("复制失败，请在“管理”中重试复制配置。已完成的授权仍然有效。", $language));
    }
    message = tr("配置已复制，粘贴给桌面 Agent 即可添加。", $language);
  }

  function openCreate() {
    name = "";
    createPreset = defaultPreset;
    message = "";
    error = "";
    createOpen = true;
  }

  async function create() {
    await task(async () => {
      const client = await bridge.createMcpClient(name, permissionPresets[createPreset]);
      createOpen = false;
      await refresh();
      await copy(client.id);
    });
  }

  function setDefaultPreset(preset: PermissionPreset) {
    defaultPreset = preset;
    try { window.localStorage.setItem("arcrelay-agent-default-permission", preset); } catch { /* Private browsing may disable storage. */ }
  }

  function openManage(client: McpClientView) {
    managedClient = structuredClone(client);
    managedPermissions = { ...client.permissions };
    manageOpen = true;
    error = "";
    message = "";
  }

  function setScope(current: McpPermissions, key: keyof McpPermissions, checked: boolean): McpPermissions {
    const next = { ...current, [key]: checked };
    if (checked && key !== "notifications") next.read = true;
    if (key === "read" && !checked) next.manage = next.enable = next.execute = next.scripts = false;
    return next;
  }

  async function saveManagedPermissions() {
    if (!managedClient) return;
    await task(async () => {
      await bridge.setMcpClientPermissions(managedClient!.id, managedPermissions);
      await refresh();
      manageOpen = false;
      message = tr("权限已更新。", $language);
    });
  }

  async function rotateManagedToken() {
    if (!managedClient || !window.confirm(tr("更换后，旧配置会立即失效。继续吗？", $language))) return;
    await task(async () => {
      await bridge.rotateMcpClientToken(managedClient!.id);
      manageOpen = false;
      await refresh();
      await copy(managedClient!.id);
    });
  }

  async function revokeManagedClient() {
    if (!managedClient || !window.confirm(tr("停用后，这个 Agent 将无法继续访问 ArcRelay。继续吗？", $language))) return;
    await task(async () => {
      await bridge.revokeMcpClient(managedClient!.id);
      await refresh();
      manageOpen = false;
      message = tr("Agent 已停用，新的调用会立即被拒绝。", $language);
    });
  }

  async function restore(client: McpClientView) {
    await task(async () => {
      await bridge.rotateMcpClientToken(client.id);
      await refresh();
      await copy(client.id);
      message = tr("Agent 已重新授权，新配置已复制。", $language);
    });
  }

  function permissionLabel(permissions: McpPermissions, legacy = false) {
    if (legacy) return "仅通知";
    const preset = detectPermissionPreset(permissions);
    if (preset === "readOnly") return "只读";
    if (preset === "daily") return "日常管理";
    if (preset === "full") return "完全控制";
    return "自定义";
  }

  function permissionSummary(permissions: McpPermissions, legacy = false) {
    if (legacy) return "只可发送和读取通知";
    const preset = detectPermissionPreset(permissions);
    if (preset === "readOnly") return "可查看配置、活动与通知";
    if (preset === "daily") return "可读取、管理并启停快捷动作";
    if (preset === "full") return "可管理、运行并使用脚本";
    return "已单独调整访问范围";
  }

  function createdLabel(value: string) {
    return new Intl.DateTimeFormat($locale, { month: "short", day: "numeric" }).format(new Date(value));
  }
</script>

<div class="agent-settings">
  <div class="settings-page-heading">
    <div>
      <h2>{tr("Agent 接入", $language)}</h2>
      <p>{tr("让你信任的 Agent 安全使用 ArcRelay", $language)}</p>
    </div>
    <button class="primary-button add-agent-button" on:click={openCreate}><Plus size={18} />{tr("接入新 Agent", $language)}</button>
  </div>

  <div class:unavailable={!mcpRunning} class="agent-service-status">
    {#if mcpRunning}<CheckCircle size={22} weight="fill" />{:else}<WarningCircle size={22} weight="fill" />{/if}
    <strong>{tr(mcpRunning ? "接入服务运行中" : "接入服务暂不可用", $language)}</strong>
    <span>{tr(mcpRunning ? "所有访问均在本机授权，可随时停用" : "请保持 ArcRelay 服务运行后再接入 Agent", $language)}</span>
  </div>

  <section class="agent-section" aria-labelledby="connected-agents-title">
    <div class="section-heading"><h3 id="connected-agents-title">{tr("已接入的 Agent", $language)}</h3><span>{activeClients.length}</span></div>
    <div class="agent-list">
      <div class="agent-list-header"><span>{tr("Agent", $language)}</span><span>{tr("访问权限", $language)}</span><span>{tr("可执行操作", $language)}</span><span>{tr("接入时间", $language)}</span><span></span></div>
      {#if activeClients.length === 0}
        <div class="agent-empty"><Robot size={26} /><span><strong>{tr("还没有接入 Agent", $language)}</strong><small>{tr("接入后可在这里单独管理权限和停用访问。", $language)}</small></span><button class="secondary-button" on:click={openCreate}>{tr("接入 Agent", $language)}</button></div>
      {/if}
      {#each activeClients as client (client.id)}
        <div class="agent-row">
          <span class="agent-identity">
            <span class:legacy={client.legacy} class="agent-icon">{#if client.legacy}<Bell size={21} />{:else}<Robot size={21} />{/if}</span>
            <span><strong>{client.legacy ? tr("通知助手", $language) : client.name}</strong><small><i></i>{tr(client.legacy ? "兼容接入" : mcpRunning ? "使用中" : "等待服务", $language)}</small></span>
          </span>
          <span><em class:notification-only={client.legacy} class="permission-pill">{tr(permissionLabel(client.permissions, client.legacy), $language)}</em></span>
          <span class="permission-summary">{tr(permissionSummary(client.permissions, client.legacy), $language)}</span>
          <span class="created-at">{createdLabel(client.createdAt)}</span>
          <button class="secondary-button manage-button" on:click={() => openManage(client)}>{tr("管理", $language)}</button>
        </div>
      {/each}
    </div>

    {#if revokedClients.length > 0}
      <details class="revoked-agents">
        <summary>{tr("查看已停用的 Agent", $language)} <span>{revokedClients.length}</span><CaretRight size={14} /></summary>
        {#each revokedClients as client (client.id)}
          <div><span><strong>{client.name}</strong><small>{tr("已停用", $language)}</small></span><button class="text-button" disabled={busy} on:click={() => restore(client)}>{tr("重新授权并复制", $language)}</button></div>
        {/each}
      </details>
    {/if}
  </section>

  <section class="agent-section" aria-labelledby="default-access-title">
    <div class="section-heading"><h3 id="default-access-title">{tr("默认授权方式", $language)}</h3></div>
    <div class="permission-preset-surface">
      <div class="permission-presets">
        {#each permissionOptions as option}
          <button class:selected={defaultPreset === option.value} aria-pressed={defaultPreset === option.value} on:click={() => setDefaultPreset(option.value)}>
            <svelte:component this={option.icon} size={22} />
            <span><strong>{tr(option.label, $language)}{#if option.value === "daily"}<em>{tr("推荐", $language)}</em>{/if}</strong><small>{tr(option.description, $language)}</small></span>
          </button>
        {/each}
      </div>
      <div class="permission-note"><span>{tr("新接入默认可查看并管理配置；运行脚本仍需单独确认。", $language)}</span><button class="text-button" on:click={openCreate}>{tr("了解权限", $language)}<CaretRight size={14} /></button></div>
    </div>
  </section>

  <details class="advanced-agent-settings">
    <summary><Gear size={20} /><span><strong>{tr("高级接入设置", $language)}</strong><small>{tr("自定义格式、令牌与访问记录", $language)}</small></span><CaretRight size={18} /></summary>
    <div class="advanced-agent-body">
      <label><span id="mcp-format-label">{tr("复制格式", $language)}</span><AppSelect bind:value={format} aria-labelledby="mcp-format-label" options={formatOptions} /></label>
      <div class="history-heading"><strong>{tr("最近配置变更", $language)}</strong><button class="text-button" disabled={busy} on:click={() => task(refresh)}>{tr("刷新", $language)}</button></div>
      {#if changes.length === 0}<p>{tr("暂无配置变更记录。", $language)}</p>{/if}
      {#each changes as change}
        <div class="history-row"><strong>{change.entity === "quickAction" ? tr("快捷动作", $language) : tr("自动化", $language)}</strong><span>{change.actor === "desktop" ? tr("本机编辑", $language) : change.actor.split(":").slice(2).join(":")}</span><small>{new Date(change.createdAt).toLocaleString()} · {change.status}</small></div>
      {/each}
    </div>
  </details>

  {#if message}<p class="agent-message" role="status">{message}</p>{/if}
  {#if error}<p class="agent-error" role="alert">{error}</p>{/if}
</div>

<Dialog.Root bind:open={createOpen}>
  <Dialog.Portal>
    <Dialog.Overlay class="modal-backdrop" />
    <Dialog.Content class="modal agent-dialog">
      <header class="modal-header">
        <div><Dialog.Title class="modal-title" level={2}>{tr("接入新 Agent", $language)}</Dialog.Title><Dialog.Description class="modal-description">{tr("为这个 Agent 单独授权，之后可以随时调整或停用。", $language)}</Dialog.Description></div>
        <Dialog.Close class="modal-close-button" aria-label={tr("关闭", $language)}><X size={19} /></Dialog.Close>
      </header>
      <div class="modal-body agent-dialog-body">
        <label class="agent-name-field"><span>{tr("Agent 名称", $language)}</span><input bind:value={name} placeholder={tr("例如：Codex", $language)} maxlength="100" required disabled={busy} /></label>
        <fieldset disabled={busy}>
          <legend>{tr("授权方式", $language)}</legend>
          <div class="dialog-permission-presets">
            {#each permissionOptions as option}
              <label class:selected={createPreset === option.value}><input type="radio" name="create-permission" value={option.value} bind:group={createPreset} /><span><strong>{tr(option.label, $language)}</strong><small>{tr(option.description, $language)}</small></span>{#if option.value === "daily"}<em>{tr("推荐", $language)}</em>{/if}</label>
            {/each}
          </div>
        </fieldset>
        <label class="agent-format-field"><span>{tr("配置类型", $language)}</span><AppSelect bind:value={format} options={formatOptions} /></label>
        {#if error}<p class="agent-error" role="alert">{error}</p>{/if}
        <p class="security-note"><LockKeyOpen size={17} />{tr("配置包含一次性可复制的访问令牌，请只交给你信任的 Agent。", $language)}</p>
      </div>
      <footer class="modal-footer"><Dialog.Close class="secondary-button">{tr("取消", $language)}</Dialog.Close><button class="primary-button" disabled={busy || !name.trim() || !mcpRunning} on:click={create}><Copy size={16} />{tr("创建并复制配置", $language)}</button></footer>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={manageOpen}>
  <Dialog.Portal>
    <Dialog.Overlay class="modal-backdrop" />
    <Dialog.Content class="modal agent-dialog">
      <header class="modal-header">
        <div><Dialog.Title class="modal-title" level={2}>{managedClient?.legacy ? tr("通知助手", $language) : managedClient?.name ?? tr("管理 Agent", $language)}</Dialog.Title><Dialog.Description class="modal-description">{tr("查看权限、复制新配置或停用这个 Agent。", $language)}</Dialog.Description></div>
        <Dialog.Close class="modal-close-button" aria-label={tr("关闭", $language)}><X size={19} /></Dialog.Close>
      </header>
      <div class="modal-body agent-dialog-body">
        {#if managedClient?.legacy}
          <div class="legacy-explanation"><Bell size={22} /><span><strong>{tr("兼容通知接入", $language)}</strong><small>{tr("这个接入只能发送和读取通知，不能管理或运行操作。", $language)}</small></span></div>
        {:else}
          <fieldset disabled={busy}>
            <legend>{tr("授权方式", $language)}</legend>
            <div class="dialog-permission-presets">
              {#each permissionOptions as option}
                <label class:selected={detectPermissionPreset(managedPermissions) === option.value}><input type="radio" name="manage-permission" checked={detectPermissionPreset(managedPermissions) === option.value} on:change={() => managedPermissions = { ...permissionPresets[option.value] }} /><span><strong>{tr(option.label, $language)}</strong><small>{tr(option.description, $language)}</small></span>{#if option.value === "daily"}<em>{tr("推荐", $language)}</em>{/if}</label>
              {/each}
            </div>
          </fieldset>
          <details class="custom-permissions">
            <summary>{tr("自定义权限", $language)}<CaretRight size={14} /></summary>
            <div class="scope-grid">
              {#each scopes as scope}
                <label><input type="checkbox" checked={managedPermissions[scope.key]} on:change={event => managedPermissions = setScope(managedPermissions, scope.key, event.currentTarget.checked)} /><span><strong>{tr(scope.label, $language)}</strong><small>{tr(scope.description, $language)}</small></span></label>
              {/each}
            </div>
          </details>
          <button class="secondary-button save-permissions" disabled={busy} on:click={saveManagedPermissions}>{tr("保存权限", $language)}</button>
        {/if}
        {#if message}<p class="agent-message" role="status">{message}</p>{/if}
        {#if error}<p class="agent-error" role="alert">{error}</p>{/if}
        <div class="credential-actions">
          {#if !managedClient?.legacy}<button class="secondary-button" disabled={busy} on:click={() => managedClient && task(() => copy(managedClient!.id))}><Copy size={15} />{tr("复制配置", $language)}</button><button class="text-button" disabled={busy} on:click={rotateManagedToken}><ArrowClockwise size={15} />{tr("更换令牌并复制", $language)}</button>{/if}
          <button class="text-button danger-text" disabled={busy} on:click={revokeManagedClient}>{tr("停用 Agent", $language)}</button>
        </div>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  .agent-settings { display: grid; gap: 0; padding: 26px 0 34px; min-width: 0; }
  .settings-page-heading { display: flex; align-items: flex-start; justify-content: space-between; gap: 24px; margin-bottom: 24px; }
  .settings-page-heading h2 { margin: 0; color: var(--text); font-size: 23px; font-weight: 680; letter-spacing: -.02em; }
  .settings-page-heading p { margin: 7px 0 0; color: var(--text-muted); font-size: 13px; }
  .add-agent-button { height: 44px; padding-inline: 18px; }
  .agent-service-status { display: grid; grid-template-columns: 26px auto 1fr; align-items: center; gap: 12px; min-height: 64px; padding: 10px 16px; border: 1px solid var(--border-strong); border-radius: 11px; color: #2fa75b; background: var(--surface-raised); }
  .agent-service-status strong { color: var(--text); font-size: 13px; }
  .agent-service-status span { color: var(--text-muted); font-size: 12px; }
  .agent-service-status.unavailable { color: var(--danger); background: var(--danger-soft); }
  .agent-section { display: grid; gap: 10px; margin-top: 28px; }
  .section-heading { display: flex; align-items: center; gap: 9px; min-height: 25px; }
  .section-heading h3 { margin: 0; color: var(--text); font-size: 15px; font-weight: 660; }
  .section-heading > span { display: grid; place-items: center; min-width: 23px; height: 23px; padding-inline: 6px; border-radius: 999px; color: var(--text-secondary); background: var(--surface-soft); font-size: 10px; }
  .agent-list { overflow: hidden; border: 1px solid var(--border-strong); border-radius: 12px; background: var(--surface-raised); }
  .agent-list-header, .agent-row { display: grid; grid-template-columns: minmax(180px, 1.2fr) minmax(100px, .75fr) minmax(210px, 1.35fr) 90px 72px; align-items: center; column-gap: 18px; padding: 0 16px; }
  .agent-list-header { min-height: 42px; color: var(--text-muted); font-size: 10px; }
  .agent-row { min-height: 82px; border-top: 1px solid var(--border); }
  .agent-identity { display: flex; align-items: center; gap: 12px; min-width: 0; }
  .agent-icon { display: grid; place-items: center; width: 40px; height: 40px; flex: 0 0 auto; border-radius: 10px; color: var(--accent); background: var(--accent-soft); }
  .agent-icon.legacy { color: var(--text-secondary); background: var(--surface-soft); }
  .agent-identity > span:last-child { display: grid; gap: 4px; min-width: 0; }
  .agent-identity strong { overflow: hidden; color: var(--text); font-size: 13px; text-overflow: ellipsis; white-space: nowrap; }
  .agent-identity small { display: flex; align-items: center; gap: 6px; color: var(--text-muted); font-size: 10px; }
  .agent-identity i { width: 6px; height: 6px; border-radius: 50%; background: #35b765; }
  .permission-pill { display: inline-flex; align-items: center; width: fit-content; min-height: 25px; padding: 0 9px; border-radius: 7px; color: var(--accent-strong); background: var(--accent-soft); font-size: 10px; font-style: normal; font-weight: 620; }
  .permission-pill.notification-only { color: #278a50; background: color-mix(in srgb, #35b765 13%, var(--surface-raised)); }
  .permission-summary, .created-at { color: var(--text-muted); font-size: 11px; line-height: 1.5; }
  .manage-button { height: 34px; padding-inline: 12px; font-size: 11px; }
  .agent-empty { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 14px; min-height: 90px; padding: 14px 18px; color: var(--text-muted); }
  .agent-empty > span { display: grid; gap: 4px; }
  .agent-empty strong { color: var(--text); font-size: 12px; }
  .agent-empty small { font-size: 11px; }
  .revoked-agents { margin-top: 2px; }
  .revoked-agents summary { display: flex; align-items: center; gap: 7px; width: fit-content; color: var(--accent-strong); font-size: 11px; cursor: pointer; list-style: none; }
  .revoked-agents summary::-webkit-details-marker { display: none; }
  .revoked-agents[open] summary :global(svg) { transform: rotate(90deg); }
  .revoked-agents summary span { display: grid; place-items: center; min-width: 18px; height: 18px; border-radius: 999px; background: var(--accent-soft); font-size: 9px; }
  .revoked-agents > div { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-top: 9px; padding: 11px 14px; border: 1px solid var(--border); border-radius: 9px; background: var(--surface-soft); }
  .revoked-agents > div > span { display: grid; gap: 3px; }
  .revoked-agents strong { font-size: 11px; }
  .revoked-agents small { color: var(--text-muted); font-size: 10px; }
  .permission-preset-surface { overflow: hidden; border: 1px solid var(--border-strong); border-radius: 12px; background: var(--surface-raised); }
  .permission-presets { display: grid; grid-template-columns: repeat(3, 1fr); padding: 12px; }
  .permission-presets > button { display: flex; align-items: center; gap: 14px; min-height: 76px; padding: 12px 18px; border: 1px solid var(--border); border-right: 0; color: var(--text-secondary); background: transparent; text-align: left; cursor: pointer; }
  .permission-presets > button:first-child { border-radius: 10px 0 0 10px; }
  .permission-presets > button:last-child { border-right: 1px solid var(--border); border-radius: 0 10px 10px 0; }
  .permission-presets > button:hover { background: var(--surface-hover); }
  .permission-presets > button.selected { position: relative; z-index: 1; border-color: var(--accent); color: var(--accent-strong); background: var(--surface-accent); box-shadow: 0 0 0 1px var(--accent); }
  .permission-presets span { display: grid; gap: 4px; }
  .permission-presets strong { display: flex; align-items: center; gap: 7px; color: inherit; font-size: 13px; }
  .permission-presets small { color: var(--text-muted); font-size: 10px; }
  .permission-presets em, .dialog-permission-presets em { padding: 2px 6px; border-radius: 999px; color: var(--accent-strong); background: var(--accent-soft); font-size: 9px; font-style: normal; }
  .permission-note { display: flex; align-items: center; justify-content: space-between; gap: 20px; padding: 10px 16px 14px; color: var(--text-muted); font-size: 11px; }
  .permission-note .text-button { display: inline-flex; align-items: center; gap: 3px; white-space: nowrap; }
  .advanced-agent-settings { margin-top: 26px; overflow: hidden; border: 1px solid var(--border-strong); border-radius: 12px; background: var(--surface-raised); }
  .advanced-agent-settings > summary { display: grid; grid-template-columns: 34px minmax(0, 1fr) auto; align-items: center; gap: 12px; min-height: 70px; padding: 9px 16px; cursor: pointer; list-style: none; }
  .advanced-agent-settings > summary::-webkit-details-marker { display: none; }
  .advanced-agent-settings > summary > span { display: grid; gap: 4px; }
  .advanced-agent-settings > summary strong { color: var(--text); font-size: 13px; }
  .advanced-agent-settings > summary small { color: var(--text-muted); font-size: 11px; }
  .advanced-agent-settings[open] > summary { border-bottom: 1px solid var(--border); }
  .advanced-agent-settings[open] > summary > :global(svg:last-child) { transform: rotate(90deg); }
  .advanced-agent-body { display: grid; gap: 14px; padding: 18px; background: var(--surface-soft); }
  .advanced-agent-body > label { display: grid; gap: 7px; font-size: 11px; }
  .history-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding-top: 4px; }
  .history-heading strong { font-size: 12px; }
  .advanced-agent-body p { margin: 0; color: var(--text-muted); font-size: 11px; }
  .history-row { display: grid; grid-template-columns: auto 1fr; gap: 3px 12px; padding: 9px 0; border-top: 1px solid var(--border); font-size: 11px; }
  .history-row small { grid-column: 1 / -1; color: var(--text-muted); }
  .agent-message, .agent-error { margin: 12px 0 0; font-size: 12px; }
  .agent-message { color: #278a50; }
  .agent-error { color: var(--danger); }
  :global(.agent-dialog) { width: min(560px, calc(100vw - 48px)); }
  .agent-dialog-body { gap: 20px; }
  .agent-name-field, .agent-format-field { display: grid; gap: 7px; color: var(--text-secondary); font-size: 11px; }
  .agent-name-field input { min-height: 42px; padding: 0 12px; border: 1px solid var(--border-strong); border-radius: 9px; color: var(--text); background: var(--surface-raised); font: inherit; font-size: 13px; }
  fieldset { min-width: 0; margin: 0; padding: 0; border: 0; }
  legend { margin-bottom: 10px; color: var(--text-secondary); font-size: 11px; }
  .dialog-permission-presets { display: grid; gap: 8px; }
  .dialog-permission-presets > label { display: grid; grid-template-columns: 20px minmax(0, 1fr) auto; align-items: center; gap: 10px; min-height: 62px; padding: 9px 12px; border: 1px solid var(--border); border-radius: 9px; cursor: pointer; }
  .dialog-permission-presets > label.selected { border-color: var(--accent); background: var(--surface-accent); }
  .dialog-permission-presets input { accent-color: var(--accent); }
  .dialog-permission-presets label > span { display: grid; gap: 3px; }
  .dialog-permission-presets strong { color: var(--text); font-size: 12px; }
  .dialog-permission-presets small { color: var(--text-muted); font-size: 10px; }
  .security-note { display: flex; align-items: flex-start; gap: 9px; margin: 0; padding: 11px 12px; border-radius: 9px; color: var(--text-secondary); background: var(--surface-soft); font-size: 11px; line-height: 1.5; }
  .security-note :global(svg) { flex: 0 0 auto; color: var(--accent); }
  .legacy-explanation { display: flex; align-items: center; gap: 12px; padding: 14px; border-radius: 10px; color: var(--text-secondary); background: var(--surface-soft); }
  .legacy-explanation > span { display: grid; gap: 4px; }
  .legacy-explanation strong { color: var(--text); font-size: 12px; }
  .legacy-explanation small { font-size: 10px; line-height: 1.5; }
  .custom-permissions { border-top: 1px solid var(--border); }
  .custom-permissions summary { display: flex; align-items: center; justify-content: space-between; padding: 13px 0; color: var(--text-secondary); font-size: 11px; cursor: pointer; list-style: none; }
  .custom-permissions summary::-webkit-details-marker { display: none; }
  .custom-permissions[open] summary :global(svg) { transform: rotate(90deg); }
  .scope-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; padding-bottom: 12px; }
  .scope-grid label { display: flex; align-items: flex-start; gap: 8px; }
  .scope-grid input { margin-top: 2px; accent-color: var(--accent); }
  .scope-grid label > span { display: grid; gap: 3px; }
  .scope-grid strong { color: var(--text); font-size: 10px; }
  .scope-grid small { color: var(--text-muted); font-size: 9px; line-height: 1.45; }
  .save-permissions { justify-self: start; height: 36px; font-size: 11px; }
  .credential-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; padding-top: 16px; border-top: 1px solid var(--border); }
  .credential-actions .secondary-button { height: 36px; font-size: 11px; }
  .credential-actions .text-button { display: inline-flex; align-items: center; gap: 6px; }
  .danger-text { margin-left: auto; color: var(--danger); }
  @media (max-width: 900px) {
    .agent-list-header { display: none; }
    .agent-row { grid-template-columns: minmax(180px, 1fr) auto auto; gap: 12px; padding-block: 13px; }
    .agent-row > :nth-child(3) { grid-column: 1 / 3; padding-left: 52px; }
    .agent-row > :nth-child(4) { display: none; }
    .manage-button { grid-column: 3; grid-row: 1 / 3; }
  }
  @media (max-width: 660px) {
    .settings-page-heading { align-items: stretch; flex-direction: column; }
    .add-agent-button { align-self: flex-start; }
    .agent-service-status { grid-template-columns: 24px 1fr; }
    .agent-service-status span { grid-column: 2; }
    .permission-presets { grid-template-columns: 1fr; }
    .permission-presets > button { border-right: 1px solid var(--border); border-bottom: 0; }
    .permission-presets > button:first-child { border-radius: 10px 10px 0 0; }
    .permission-presets > button:last-child { border-bottom: 1px solid var(--border); border-radius: 0 0 10px 10px; }
    .agent-row { grid-template-columns: minmax(0, 1fr) auto; }
    .agent-row > :nth-child(2) { grid-column: 1; padding-left: 52px; }
    .agent-row > :nth-child(3) { grid-column: 1; padding-left: 52px; }
    .manage-button { grid-column: 2; grid-row: 1 / 4; }
    .scope-grid { grid-template-columns: 1fr; }
  }
</style>
