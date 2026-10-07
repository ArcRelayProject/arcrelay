<script lang="ts">
  import AppSelect from "../../components/AppSelect.svelte";
  import { onMount } from "svelte";
  import {
    ArrowLeft,
    CaretRight,
    DownloadSimple,
    Fingerprint,
    LockKey,
    ShieldCheck,
    UploadSimple,
    X,
  } from "phosphor-svelte";
  import type {
    LoginResponse,
    LoginVaultStatus,
    LoginSettings,
    LoginRestorePreview,
  } from "../../ipc/generated";
  import { loginRequest, onLoginLocked } from "./bridge";
  import LoginUnlock from "./LoginUnlock.svelte";
  import "./login.css";
  export let onBack: () => void;
  let status: LoginVaultStatus | null = null;
  let deviceAvailable = false;
  let deviceEnabled = false;
  let count = 0;
  let settings: LoginSettings = { unlockSeconds: 300, lockOnClose: false, clearSeconds: 30 };
  let page: "security" | "backup" | "password" = "security";
  let busy = false;
  let error = "";
  let message = "";
  let previous = "";
  let password = "";
  let confirmation = "";
  let backupPassword = "";
  let backupConfirmation = "";
  let restorePassword = "";
  let preview: LoginRestorePreview | null = null;
  let token = "";
  let replace = false;
  let confirmReplace = false;
  let disposed = false;
  function apply(r: LoginResponse) {
    if (
      status &&
      r.status &&
      (r.status.sessionVersion < status.sessionVersion ||
        (r.status.sessionVersion === status.sessionVersion &&
          !status.unlocked &&
          r.status.unlocked))
    )
      return;
    if (r.status) {
      status = r.status;
      if (status.error) error = status.error;
      settings = { ...r.status.settings };
    }
    deviceAvailable = r.deviceAvailable;
    deviceEnabled = r.deviceEnabled;
  }
  function clearSecrets() {
    previous = "";
    password = "";
    confirmation = "";
    backupPassword = "";
    backupConfirmation = "";
    restorePassword = "";
    preview = null;
    token = "";
    confirmReplace = false;
  }
  async function refresh() {
    try {
      apply(await loginRequest({ type: "status" }));
      if (status?.unlocked)
        count = (await loginRequest({ type: "list", search: "", tag: null })).entries?.length ?? 0;
      else {
        count = 0;
        clearSecrets();
      }
    } catch (e) {
      error = String(e);
    }
  }
  async function action(work: () => Promise<LoginResponse>, success: string) {
    if (busy) return;
    busy = true;
    error = "";
    message = "";
    try {
      apply(await work());
      message = success;
      return true;
    } catch (e) {
      error = String(e);
      return false;
    } finally {
      busy = false;
    }
  }
  async function saveSettings(next: LoginSettings) {
    const ok = await action(
      () => loginRequest({ type: "settings", settings: next }),
      "保护设置已保存",
    );
    if (!ok) await refresh();
  }
  async function changePassword() {
    if (password.length < 12 || password !== confirmation) {
      error = "新主密码至少 12 个字符，且两次输入必须一致";
      return;
    }
    if (
      await action(
        () => loginRequest({ type: "changePassword", previous, password }),
        "主密码已更换，请重新启用系统快速解锁",
      )
    ) {
      clearSecrets();
      page = "security";
    } else {
      previous = "";
      password = "";
      confirmation = "";
    }
  }
  async function exportBackup() {
    if (backupPassword.length < 12 || backupPassword !== backupConfirmation) {
      error = "请设置至少 12 个字符的备份口令，并确认两次输入一致";
      return;
    }
    busy = true;
    error = "";
    message = "";
    try {
      const r = await loginRequest({ type: "export", password: backupPassword });
      if (r.file) message = `已导出加密备份：${r.file}`;
    } catch (e) {
      error = String(e);
    } finally {
      backupPassword = "";
      backupConfirmation = "";
      busy = false;
    }
  }
  async function previewRestore() {
    if (!restorePassword) {
      error = "请输入该备份的口令";
      return;
    }
    busy = true;
    error = "";
    preview = null;
    token = "";
    try {
      const r = await loginRequest({
        type: "previewRestore",
        password: restorePassword,
        recovery: false,
      });
      preview = r.preview;
      token = r.token ?? "";
      confirmReplace = false;
    } catch (e) {
      error = String(e);
    } finally {
      restorePassword = "";
      busy = false;
    }
  }
  async function restoreBackup() {
    if (replace && !confirmReplace) {
      error = "请先确认替换当前登录项目";
      return;
    }
    if (
      await action(
        () => loginRequest({ type: "restore", token, replace }),
        "登录已恢复。应用优先规则已暂停，请逐项确认并重新启用。",
      )
    ) {
      clearSecrets();
      await refresh();
    } else {
      preview = null;
      token = "";
    }
  }
  onMount(() => {
    void refresh();
    let stop: (() => void) | undefined;
    void onLoginLocked(() => {
      if (status) status = { ...status, unlocked: false };
      count = 0;
      clearSecrets();
    }).then((s) => (disposed ? s() : (stop = s)));
    const timer = setInterval(async () => {
      try {
        const r = await loginRequest({ type: "status" });
        apply(r);
        if (!status?.unlocked) {
          count = 0;
          clearSecrets();
        }
      } catch {
        clearSecrets();
        if (status) status = { ...status, unlocked: false };
      }
    }, 5000);
    return () => {
      disposed = true;
      clearSecrets();
      stop?.();
      clearInterval(timer);
    };
  });
</script>

<div class="login-ui security">
  <header class="flex">
    <button
      class="quiet"
      aria-label="返回"
      disabled={busy}
      on:click={() => {
        clearSecrets();
        error = "";
        message = "";
        if (page === "security") onBack();
        else page = "security";
      }}><ArrowLeft size={20} /></button
    >
    <div class="grow">
      <h2>
        {page === "backup" ? "加密备份与恢复" : page === "password" ? "更换主密码" : "登录与安全"}
      </h2>
      <span class="muted">剪贴板 / 登录保护{page === "backup" ? " / 备份与恢复" : ""}</span>
    </div>
  </header>
  {#if !status}<p class="muted">正在读取保护状态…</p>
  {:else if !status.unlocked}<LoginUnlock
      configured={status.configured}
      {deviceAvailable}
      {deviceEnabled}
      onUnlocked={(r) => {
        apply(r);
        void refresh();
      }}
    />
  {:else if page === "security"}
    <div class="status-card flex">
      <span class="shield"><ShieldCheck size={28} /></span>
      <div class="grow">
        <strong>登录信息始终加密保存</strong>
        <p class="muted">
          当前会话已解锁 · {count} 项登录 · 约 {Math.ceil(status.expiresInSeconds / 60)} 分钟后锁定
        </p>
      </div>
      <button
        disabled={busy}
        on:click={() => action(() => loginRequest({ type: "lock" }), "登录信息已锁定")}
        ><LockKey size={16} />立即锁定</button
      >
    </div>
    <h3>解锁方式</h3>
    <div class="card">
      <button
        class="setting-row"
        disabled={busy}
        on:click={() => {
          page = "password";
          error = "";
          message = "";
        }}
        ><LockKey size={20} /><span class="grow"
          ><strong>主密码</strong><small>已设置 · 更换主密码会更新加密密钥</small></span
        ><span class="muted">更换</span><CaretRight size={16} /></button
      >
      <div class="setting-row">
        <Fingerprint size={21} /><span class="grow"
          ><strong>系统快速解锁</strong><small
            >{deviceAvailable
              ? "通过本机系统用户验证解锁，主密码仍可使用"
              : "此设备暂不支持，请使用主密码"}</small
          ></span
        ><label class="switch"
          ><input
            type="checkbox"
            checked={deviceEnabled}
            disabled={busy || !deviceAvailable}
            aria-label="系统快速解锁"
            on:change={(e) => {
              const enabled = e.currentTarget.checked;
              e.currentTarget.checked = deviceEnabled;
              void action(() => loginRequest({ type: "device", enabled }), "快速解锁设置已更新");
            }}
          /><span></span></label
        >
      </div>
    </div>
    <h3>会话与剪贴板保护</h3>
    <div class="card">
      <label class="setting-row"
        ><span class="grow"
          ><strong>每次解锁的有效时间</strong><small>到期后需要重新验证，使用登录不会延长会话</small
          ></span
        ><AppSelect
          class="small-select"
          aria-label="每次解锁的有效时间"
          value={settings.unlockSeconds}
          disabled={busy}
          onValueChange={(value) => saveSettings({ ...settings, unlockSeconds: value })}
          options={[
            { value: 60, label: "1 分钟" },
            { value: 300, label: "5 分钟" },
            { value: 900, label: "15 分钟" },
          ]}
        /></label
      >
      <div class="setting-row">
        <span class="grow"
          ><strong>屏幕锁定或设备休眠时锁定</strong><small>始终启用，唤醒后重新解锁</small></span
        ><span class="chip">始终启用</span>
      </div>
      <div class="setting-row">
        <span class="grow"
          ><strong>关闭剪贴板时锁定</strong><small>隐藏或关闭剪贴板窗口后结束解锁会话</small></span
        ><label class="switch"
          ><input
            type="checkbox"
            checked={settings.lockOnClose}
            disabled={busy}
            aria-label="关闭剪贴板时锁定"
            on:change={(e) => {
              const lockOnClose = e.currentTarget.checked;
              e.currentTarget.checked = settings.lockOnClose;
              void saveSettings({ ...settings, lockOnClose });
            }}
          /><span></span></label
        >
      </div>
      <label class="setting-row"
        ><span class="grow"
          ><strong>自动清除复制的凭据</strong><small
            >到期或锁定时清除本次复制，保留之后的新复制</small
          ></span
        ><AppSelect
          class="small-select"
          aria-label="自动清除复制的凭据"
          value={settings.clearSeconds}
          disabled={busy}
          onValueChange={(value) => saveSettings({ ...settings, clearSeconds: value })}
          options={[
            { value: 15, label: "15 秒" },
            { value: 30, label: "30 秒" },
            { value: 60, label: "1 分钟" },
            { value: 120, label: "2 分钟" },
          ]}
        /></label
      >
      <div class="setting-row">
        <span class="grow"
          ><strong>动态验证码</strong><small>在本轮验证码到期时自动清除复制内容</small></span
        ><span class="muted">随验证码到期</span>
      </div>
    </div>
    <h3>备份与恢复</h3>
    <button
      class="card setting-row backup-link"
      disabled={busy}
      on:click={() => {
        page = "backup";
        error = "";
        message = "";
      }}
      ><DownloadSimple size={21} /><span class="grow"
        ><strong>加密备份与恢复</strong><small>使用独立备份口令保护登录和 TOTP 配置</small></span
      ><CaretRight size={16} /></button
    >
    <p class="notice">
      登录凭据不进入历史、同步或 MCP。仅本机会话可解锁；主密码与 TOTP 密钥不会回显。
    </p>
  {:else if page === "password"}
    <div class="form-card">
      <p class="notice">更换后重新加密登录信息，并撤销旧的系统快速解锁密钥。请保存好新主密码。</p>
      <label class="field"
        >当前主密码<input
          type="password"
          bind:value={previous}
          autocomplete="current-password"
          maxlength="1024"
        /></label
      ><label class="field"
        >新主密码<input
          type="password"
          bind:value={password}
          autocomplete="new-password"
          maxlength="1024"
          placeholder="至少 12 个字符"
        /></label
      ><label class="field"
        >确认新主密码<input
          type="password"
          bind:value={confirmation}
          autocomplete="new-password"
          maxlength="1024"
        /></label
      >
      <div class="actions">
        <button
          disabled={busy}
          on:click={() => {
            clearSecrets();
            page = "security";
          }}>取消</button
        ><button class="primary" disabled={busy || !previous || !password} on:click={changePassword}
          >{busy ? "更新中…" : "更换主密码"}</button
        >
      </div>
    </div>
  {:else if page === "backup"}
    <div class="status-card flex">
      <ShieldCheck size={25} />
      <div class="grow">
        <strong>独立口令保护的备份</strong>
        <p class="muted">包括 {count} 项登录及其 TOTP 配置 · 全程加密</p>
      </div>
    </div>
    <div class="backup-grid">
      <section class="form-card">
        <h3><DownloadSimple size={20} />导出加密备份</h3>
        <p class="muted">备份口令独立于主密码。保存文件和口令，换机时使用。</p>
        <label class="field"
          >备份口令<input
            type="password"
            bind:value={backupPassword}
            autocomplete="new-password"
            maxlength="1024"
            placeholder="至少 12 个字符"
          /></label
        ><label class="field"
          >确认备份口令<input
            type="password"
            bind:value={backupConfirmation}
            autocomplete="new-password"
            maxlength="1024"
          /></label
        >
        <p class="notice">口令不会保存在备份文件中。忘记备份口令后无法恢复该文件。</p>
        <div class="actions">
          <button class="primary" disabled={busy} on:click={exportBackup}
            >{busy ? "处理中…" : "选择位置并导出"}</button
          >
        </div>
      </section>
      <section class="form-card">
        <h3><UploadSimple size={20} />从备份恢复</h3>
        <p class="muted">选择 .arclogin 文件，验证口令后预览要恢复的项目。</p>
        <label class="field"
          >备份口令<input
            type="password"
            bind:value={restorePassword}
            autocomplete="off"
            maxlength="1024"
          /></label
        ><button disabled={busy || !restorePassword} on:click={previewRestore}
          >选择文件并预览</button
        >
        {#if preview}<div class="restore-preview">
            <strong>已验证 · {preview.count} 项登录</strong>
            <ul>
              {#each preview.titles.slice(0, 5) as title}<li>{title}</li>{/each}
            </ul>
            {#if preview.count > 5}<small>还有 {preview.count - 5} 项</small>{/if}<label
              class="field"
              >恢复方式<AppSelect
                aria-label="恢复方式"
                bind:value={replace}
                options={[
                  { value: false, label: "合并到现有登录" },
                  { value: true, label: "替换所有现有登录" },
                ]}
              /></label
            >{#if replace}<label class="flex"
                ><input
                  type="checkbox"
                  bind:checked={confirmReplace}
                />确认删除当前登录，使用备份中的登录替换</label
              >{/if}
            <p class="notice">恢复后暂停应用优先规则，需逐项确认本机应用。预览 3 分钟后过期。</p>
            <div class="actions">
              <button
                class="primary"
                disabled={busy || (replace && !confirmReplace)}
                on:click={restoreBackup}>确认恢复</button
              >
            </div>
          </div>{/if}
      </section>
    </div>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}{#if message}<p
      class="success"
      role="status"
    >
      {message}
    </p>{/if}
</div>

<style>
  .security {
    width: 100%;
    max-width: 900px;
    padding-bottom: 0;
  }
  header {
    margin-bottom: 8px;
  }
  h2 {
    font-size: 20px;
    font-weight: 600;
    margin: 0 0 5px;
  }
  h3 {
    font-size: 13px;
    margin: 6px 0 3px;
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .status-card {
    padding: 7px 12px;
    background: var(--accent-soft, #f0f0ff);
    border: 1px solid var(--border-color, #dedfe7);
    border-radius: 12px;
  }
  .status-card p {
    margin: 5px 0 0;
  }
  .shield {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    color: var(--accent, #5b5ff0);
  }
  .card {
    border: 1px solid var(--border-color, #dedfe7);
    border-radius: 11px;
    overflow: hidden;
    background: var(--surface, white);
  }
  .setting-row {
    display: flex;
    align-items: center;
    gap: 15px;
    padding: 4px 12px !important;
    min-height: 44px !important;
    line-height: 1.2;
    width: 100%;
    text-align: left;
    border: none !important;
    border-radius: 0 !important;
    border-bottom: 1px solid var(--border, #e8e9f0) !important;
  }
  .setting-row:last-child {
    border-bottom: none !important;
  }
  .setting-row small {
    display: block;
    margin-top: 2px;
    font-size: 11px;
    line-height: 1.35;
  }
  :global(.security .small-select) {
    max-width: 130px;
  }
  .backup-link {
    border: 1px solid var(--border-color, #dedfe7) !important;
    border-radius: 11px !important;
  }
  .switch {
    display: inline-flex;
    flex-shrink: 0;
  }
  .switch input {
    position: absolute;
    opacity: 0;
    width: 36px;
    height: 23px;
  }
  .switch span {
    width: 36px;
    height: 23px;
    border-radius: 14px;
    background: var(--border-color, #dedfe7);
    padding: 3px;
  }
  .switch span:after {
    content: "";
    display: block;
    width: 17px;
    height: 17px;
    border-radius: 50%;
    background: white;
    transition: transform 0.15s;
  }
  .switch input:checked + span {
    background: var(--accent, #5b5ff0);
  }
  .switch input:checked + span:after {
    transform: translateX(13px);
  }
  .switch input:focus-visible + span {
    outline: 2px solid var(--accent, #5b5ff0);
    outline-offset: 3px;
  }
  .notice {
    margin-top: 10px;
    padding: 8px 12px;
  }
  .security > .notice {
    margin: 8px 0 0;
    padding: 6px 12px;
    font-size: 11px;
    line-height: 1.5;
  }
  .form-card {
    border: 1px solid var(--border-color, #dedfe7);
    border-radius: 12px;
    padding: 24px;
    background: var(--surface, white);
  }
  .form-card > h3 {
    margin-top: 0;
    font-size: 16px;
  }
  .backup-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 18px;
    margin-top: 20px;
  }
  .restore-preview {
    border-top: 1px solid var(--border-color, #dedfe7);
    padding-top: 18px;
    margin-top: 18px;
  }
  ul {
    padding-left: 20px;
    font-size: 12px;
    color: var(--text-secondary, #737681);
    line-height: 1.9;
  }
  @media (max-width: 800px) {
    .backup-grid {
      grid-template-columns: 1fr;
    }
    .setting-row {
      gap: 9px;
      padding: 13px !important;
    }
  }
</style>
