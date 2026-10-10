<script lang="ts">
  import { LockKey, ShieldCheck, Fingerprint } from "phosphor-svelte";
  import { loginRequest } from "./bridge";
  import type { LoginResponse } from "../../ipc/generated";
  import "./login.css";
  export let configured = true;
  export let deviceEnabled = false;
  export let deviceAvailable = false;
  export let onUnlocked: (response: LoginResponse) => void;
  let password = "";
  let confirm = "";
  let quick = false;
  let busy = false;
  let error = "";
  let recovery = false;
  let backupPassword = "";
  let recoveryToken = "";
  let recoveryCount = 0;
  let confirmed = false;
  async function unlock(device = false) {
    if (busy) return;
    if (!configured && (password.length < 12 || password !== confirm)) {
      error = "请设置至少 12 个字符的主密码，并确认两次输入一致";
      return;
    }
    busy = true;
    error = "";
    try {
      let response = await loginRequest(
        device
          ? { type: "unlockDevice" }
          : configured
            ? { type: "unlock", password }
            : { type: "initialize", password },
      );
      if (!configured && quick) {
        try {
          response = await loginRequest({ type: "device", enabled: true });
        } catch (e) {
          error = `主密码已设置；系统快速解锁未启用：${String(e)}`;
        }
      }
      onUnlocked(response);
    } catch (e) {
      error = String(e);
    } finally {
      password = "";
      confirm = "";
      busy = false;
    }
  }
  async function previewRecovery() {
    busy = true;
    error = "";
    recoveryToken = "";
    try {
      const r = await loginRequest({
        type: "previewRestore",
        password: backupPassword,
        recovery: true,
      });
      recoveryToken = r.token ?? "";
      recoveryCount = r.preview?.count ?? 0;
    } catch (e) {
      error = String(e);
    } finally {
      backupPassword = "";
      busy = false;
    }
  }
  async function recover() {
    if (!confirmed || password.length < 12 || password !== confirm) {
      error = "请确认替换数据，并设置至少 12 个字符的新主密码";
      return;
    }
    busy = true;
    error = "";
    try {
      const r = await loginRequest({ type: "recover", token: recoveryToken, password, confirmed });
      onUnlocked(r);
    } catch (e) {
      error = String(e);
      recoveryToken = "";
    } finally {
      password = "";
      confirm = "";
      busy = false;
    }
  }
</script>

<div class="login-ui unlock">
  {#if recovery}
    <h2>使用加密备份恢复</h2>
    <p class="muted">验证备份口令后，设置新的主密码。</p>
    {#if !recoveryToken}<label class="field"
        >备份口令<input
          type="password"
          bind:value={backupPassword}
          autocomplete="off"
          maxlength="1024"
        /></label
      ><button class="primary quick" disabled={busy || !backupPassword} on:click={previewRecovery}
        >选择备份并验证</button
      >
    {:else}<p class="notice">
        已验证 {recoveryCount} 项登录。应用规则恢复后暂停。预览 3 分钟后过期。
      </p>
      <label class="field"
        >新主密码<input
          type="password"
          bind:value={password}
          autocomplete="new-password"
          maxlength="1024"
        /></label
      ><label class="field"
        >确认新主密码<input
          type="password"
          bind:value={confirm}
          autocomplete="new-password"
          maxlength="1024"
        /></label
      ><label class="flex muted"
        ><input
          type="checkbox"
          bind:checked={confirmed}
        />确认以备份替换当前登录。原加密文件保留在本机。</label
      ><button class="primary quick" disabled={busy || !confirmed} on:click={recover}
        >恢复并重新保护</button
      >{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}<button
      class="quiet"
      disabled={busy}
      on:click={() => {
        recovery = false;
        password = "";
        confirm = "";
        backupPassword = "";
        recoveryToken = "";
        error = "";
      }}>返回解锁</button
    >
  {:else}
    <div class="shield"><LockKey size={36} /></div>
    <h2>{configured ? "登录信息已锁定" : "启用登录保护"}</h2>
    <p class="muted">
      {configured ? "解锁后查看用户名、密码与动态验证码。" : "用主密码保护这台设备上的登录信息。"}
    </p>
    {#if configured && deviceEnabled}
      <button class="primary quick" disabled={busy} on:click={() => unlock(true)}
        ><Fingerprint size={20} />使用系统验证解锁</button
      >
      <span class="or">或使用主密码</span>
    {/if}
    <form on:submit|preventDefault={() => unlock()}>
      <label class="field"
        >{configured ? "主密码" : "设置主密码"}<input
          type="password"
          bind:value={password}
          autocomplete={configured ? "current-password" : "new-password"}
          maxlength="1024"
          placeholder={configured ? "输入主密码" : "至少 12 个字符"}
          required
        /></label
      >
      {#if !configured}
        <label class="field"
          >确认主密码<input
            type="password"
            bind:value={confirm}
            autocomplete="new-password"
            maxlength="1024"
            required
          /></label
        >
        {#if deviceAvailable}<label class="flex muted"
            ><input type="checkbox" bind:checked={quick} />启用本机系统快速解锁</label
          >{/if}
        <p class="notice">
          请妥善保管主密码。忘记主密码后，现有加密数据无法直接重置；可在新存储中恢复有口令的加密备份。
        </p>
      {/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <button class="primary quick" type="submit" disabled={busy || !password}
        >{busy ? "正在验证…" : configured ? "解锁登录信息" : "启用并解锁"}</button
      >
    </form>
    {#if configured}<button
        class="quiet muted"
        disabled={busy}
        on:click={() => {
          recovery = true;
          password = "";
          confirm = "";
          error = "";
        }}>忘记主密码？使用加密备份恢复</button
      >{/if}
    <div class="protection muted">
      <ShieldCheck size={15} />信息始终加密保存 · 解锁仅在本次会话有效
    </div>
  {/if}
</div>

<style>
  .unlock {
    max-width: 380px;
    margin: auto;
    padding: 32px 24px;
    text-align: center;
  }
  .shield {
    width: 78px;
    height: 78px;
    margin: 0 auto 22px;
    border-radius: 23px;
    background: var(--accent-soft, #f0f0ff);
    color: var(--accent, #5b5ff0);
    display: grid;
    place-items: center;
  }
  h2 {
    font-size: 20px;
    margin: 0 0 10px;
    font-weight: 600;
  }
  p {
    margin: 0 0 20px;
  }
  form {
    text-align: left;
  }
  .quick {
    width: 100%;
    min-height: 40px;
  }
  .or {
    display: block;
    margin: 18px 0;
    color: var(--text-muted, #8a8d99);
    font-size: 12px;
  }
  .protection {
    display: flex;
    justify-content: center;
    gap: 6px;
    margin-top: 24px;
  }
</style>
