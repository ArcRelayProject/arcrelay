<script lang="ts">
  import { Eye, EyeSlash, LockKey, ShieldCheck, X } from "phosphor-svelte";
  import { onMount } from "svelte";

  export let shareName = "";
  export let busy = false;
  export let error = "";
  export let cancel: () => void;
  export let unlock: (password: string) => void | Promise<void>;

  let password = "";
  let visible = false;
  let passwordInput: HTMLInputElement;

  onMount(() => passwordInput.focus());
</script>

<svelte:window on:keydown={(event) => event.key === "Escape" && !busy && cancel()} />

<div class="dialog-backdrop" role="button" tabindex="0" aria-label="关闭密码对话框" on:click|self={() => !busy && cancel()} on:keydown={(event) => event.key === "Escape" && !busy && cancel()}>
  <form class="unlock-dialog" aria-labelledby="unlock-title" on:submit|preventDefault={() => unlock(password)}>
    <header class="dialog-title-row">
      <span class="dialog-icon"><LockKey size={24} /></span>
      <button type="button" class="icon-button" aria-label="关闭" disabled={busy} on:click={cancel}><X size={19} /></button>
    </header>
    <div class="dialog-copy"><span class="dialog-eyebrow">密码保护</span><h2 id="unlock-title">解锁“{shareName}”</h2><p>该共享已设置访问密码。验证后仅会解锁当前共享，本会话内有效。</p></div>
    <label class="password-field"><span>访问密码</span><span class="input-with-action"><input bind:this={passwordInput} type={visible ? "text" : "password"} bind:value={password} autocomplete="current-password" disabled={busy} placeholder="输入密码" /><button type="button" aria-label={visible ? "隐藏密码" : "显示密码"} on:click={() => (visible = !visible)}>{#if visible}<EyeSlash size={19} />{:else}<Eye size={19} />{/if}</button></span></label>
    <div class="http-warning"><ShieldCheck size={17} /><span>当前为 HTTP 局域网连接，请仅在可信网络输入密码。</span></div>
    {#if error}<div class="inline-error" role="alert">{error}</div>{/if}
    <footer class="dialog-actions"><button type="button" class="secondary-button" disabled={busy} on:click={cancel}>取消</button><button type="submit" class="primary-action" disabled={busy || !password}>{busy ? "正在验证…" : "解锁文件夹"}</button></footer>
  </form>
</div>
