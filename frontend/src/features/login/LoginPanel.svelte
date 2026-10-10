<script lang="ts">
  import AppSelect from "../../components/AppSelect.svelte";
  import { onMount } from "svelte";
  import {
    CaretDown,
    CaretRight,
    CopySimple,
    Eye,
    EyeSlash,
    GithubLogo,
    GoogleLogo,
    FigmaLogo,
    Key,
    LockKey,
    PencilSimple,
    Plus,
    PushPin,
    ShieldCheck,
    SlidersHorizontal,
    Star,
    Trash,
  } from "phosphor-svelte";
  import type {
    LoginSummary,
    LoginResponse,
    LoginVaultStatus,
    LoginContext,
    LoginField,
    LoginOtp,
  } from "../../ipc/generated";
  import { loginRequest, onLoginLocked, onLoginContext } from "./bridge";
  import { clipboardBridge } from "../../clipboard/bridge";
  import LoginUnlock from "./LoginUnlock.svelte";
  import LoginEditor from "./LoginEditor.svelte";
  import "./login.css";
  export let search = "";
  export let tag: string | null = null;
  export let compact = false;
  export let editing = false;
  let status: LoginVaultStatus | null = null;
  let context: LoginContext | null = null;
  let entries: LoginSummary[] = [];
  let deviceAvailable = false;
  let deviceEnabled = false;
  let selected: string | null = null;
  let editEntry: LoginSummary | null = null;
  let error = "";
  let message = "";
  let busy = false;
  let loaded = false;
  let otp: LoginOtp | null = null;
  let now = Date.now();
  let revealed = "";
  let revealUntil = 0;
  let removeId: string | null = null;
  let disposed = false;
  let generation = 0;
  let otpBusy = false;
  let contextPending = false;
  let tagFilter = "";
  let tags: string[] = [];
  $: visible = compact ? entries.filter((e) => e.matched) : entries;
  $: active = visible.find((e) => e.id === selected);
  $: seconds = otp ? Math.max(0, Math.ceil((otp.expiresAtMs - now) / 1000)) : 0;
  $: otpEndingSoon = !otp || otp.expiresAtMs - now < 5000;
  $: if (loaded) {
    search;
    tag;
    tagFilter;
    editing;
    void refreshList();
  }
  function serviceIcon(address: string) {
    try {
      const host = new URL(
        address.includes("://") ? address : `https://${address}`,
      ).hostname.toLowerCase();
      const matches = (domain: string) => host === domain || host.endsWith(`.${domain}`);
      return matches("github.com")
        ? GithubLogo
        : matches("figma.com")
          ? FigmaLogo
          : matches("google.com")
            ? GoogleLogo
            : Key;
    } catch {
      return Key;
    }
  }
  function apply(response: LoginResponse) {
    if (
      status &&
      response.status &&
      (response.status.sessionVersion < status.sessionVersion ||
        (response.status.sessionVersion === status.sessionVersion &&
          !status.unlocked &&
          response.status.unlocked))
    )
      return;
    if (response.status) {
      status = response.status;
      if (status.error) error = status.error;
    }
    deviceAvailable = response.deviceAvailable;
    deviceEnabled = response.deviceEnabled;
  }
  function clear() {
    generation++;
    entries = [];
    tags = [];
    selected = null;
    otp = null;
    revealed = "";
    editing = false;
    editEntry = null;
    removeId = null;
    if (status) status = { ...status, unlocked: false };
  }
  async function refreshList() {
    if (!status?.unlocked || editing) return;
    const version = ++generation;
    try {
      const r = await loginRequest({ type: "list", search, tag: tagFilter || tag });
      if (disposed || version !== generation) return;
      apply(r);
      entries = r.entries ?? [];
      tags = r.tags ?? [];
      if (selected && !entries.some((e) => e.id === selected)) {
        selected = null;
        otp = null;
        revealed = "";
      }
    } catch (e) {
      if (version !== generation) return;
      error = String(e);
      clear();
    }
  }
  async function refresh(contextChanged = false) {
    if (editing || disposed) return;
    try {
      apply(await loginRequest({ type: "status" }));
      if (!status?.unlocked) clear();
      if (contextChanged) context = (await loginRequest({ type: "context" })).context;
      await refreshList();
      loaded = true;
    } catch (e) {
      error = String(e);
      clear();
      loaded = true;
    }
  }
  async function fetchOtp(id: string) {
    if (otpBusy || !status?.unlocked) return;
    otpBusy = true;
    try {
      const r = await loginRequest({ type: "otp", id });
      if (selected === id && status?.unlocked) otp = r.otp;
    } catch (e) {
      error = String(e);
    } finally {
      otpBusy = false;
    }
  }
  function expand(row: LoginSummary) {
    revealed = "";
    otp = null;
    removeId = null;
    selected = selected === row.id ? null : row.id;
    if (selected && row.hasTotp) void fetchOtp(row.id);
    if (!selected && contextPending) {
      contextPending = false;
      void refreshList();
    }
  }
  function privateSelection(event: ClipboardEvent) {
    const selection = window.getSelection();
    const root = event.currentTarget as HTMLElement;
    if (
      selection &&
      [...root.querySelectorAll(".field-value")].some((node) => selection.containsNode(node, true))
    ) {
      event.preventDefault();
      message = "请使用字段旁的复制按钮，复制内容会自动清除。";
    }
  }
  async function useField(row: LoginSummary, field: LoginField, paste: boolean) {
    if (busy) return;
    busy = true;
    error = "";
    message = "";
    revealed = "";
    try {
      apply(
        await loginRequest({ type: "use", id: row.id, field, paste, token: context?.token ?? "" }),
      );
      message = paste ? "已插入到当前应用" : "已复制 · 到期自动清除";
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
  async function reveal(row: LoginSummary) {
    if (revealed) {
      revealed = "";
      return;
    }
    try {
      const r = await loginRequest({ type: "reveal", id: row.id });
      if (selected === row.id && status?.unlocked) {
        revealed = r.value ?? "";
        revealUntil = Date.now() + 10000;
      }
    } catch (e) {
      error = String(e);
    }
  }
  async function lock() {
    try {
      await loginRequest({ type: "lock" });
      clear();
    } catch (e) {
      error = String(e);
    }
  }
  async function remove(id: string) {
    busy = true;
    try {
      await loginRequest({ type: "remove", id });
      removeId = null;
      await refreshList();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
  async function openSettings() {
    try {
      await loginRequest({ type: "openSettings" });
    } catch (e) {
      error = String(e);
    }
  }
  onMount(() => {
    const stops: (() => void)[] = [];
    void refresh(true);
    void onLoginContext((next) => {
      if (disposed) return;
      context = next;
      revealed = "";
      if (editing || selected) contextPending = true;
      else void refreshList();
    }).then((s) => (disposed ? s() : stops.push(s)));
    void onLoginLocked(clear).then((s) => (disposed ? s() : stops.push(s)));
    void clipboardBridge
      .onShown(() => {
        revealed = "";
        void refresh(true);
      })
      .then((s) => (disposed ? s() : stops.push(s)));
    void clipboardBridge
      .onHidden(() => {
        revealed = "";
        otp = null;
        entries = [];
        selected = null;
        editing = false;
        editEntry = null;
        loaded = false;
      })
      .then((s) => (disposed ? s() : stops.push(s)));
    const timer = setInterval(() => {
      now = Date.now();
      if (revealed && now >= revealUntil) revealed = "";
      if (active?.hasTotp && (!otp || now >= otp.expiresAtMs)) void fetchOtp(active.id);
    }, 1000);
    const statusTimer = setInterval(async () => {
      try {
        const r = await loginRequest({ type: "status" });
        apply(r);
        if (!r.status?.unlocked) clear();
      } catch {
        clear();
      }
    }, 5000);
    const blur = () => {
      revealed = "";
    };
    window.addEventListener("blur", blur);
    return () => {
      disposed = true;
      clear();
      stops.forEach((s) => s());
      clearInterval(timer);
      clearInterval(statusTimer);
      window.removeEventListener("blur", blur);
    };
  });
</script>

<div
  class="login-ui login-panel"
  class:compact
  class:editor-open={editing}
  on:copy={privateSelection}
  on:cut={privateSelection}
>
  {#if editing}
    <LoginEditor
      entry={editEntry}
      onClose={() => {
        editing = false;
        editEntry = null;
      }}
      onSaved={() => {
        editing = false;
        editEntry = null;
        void refreshList();
      }}
    />
  {:else}
    {#if !compact || visible.length}
      <div class="context flex">
        <span class="grow muted"
          >{context?.name ? `当前应用：${context.name}` : "未识别到外部应用"}</span
        >{#if status?.unlocked}<span class="chip"><ShieldCheck size={12} />已解锁</span
          >{#if !compact}<AppSelect
              class="tag-select"
              aria-label="登录标签"
              bind:value={tagFilter}
              options={[
                { value: "", label: "全部标签" },
                ...tags.map((name) => ({ value: name, label: name })),
              ]}
            />{/if}{/if}
      </div>
    {/if}
    {#if !loaded}<div class="empty muted">正在读取登录保护状态…</div>
    {:else if !status?.unlocked}
      {#if !compact}<LoginUnlock
          configured={status?.configured ?? false}
          {deviceEnabled}
          {deviceAvailable}
          onUnlocked={(r) => {
            apply(r);
            error = "";
            void refreshList();
          }}
        />{/if}
    {:else}
      {#if contextPending && selected}<div class="notice muted">
          应用已切换，收起当前条目后更新优先分组。
        </div>{/if}
      <div class="rows">
        {#each visible as row, index (row.id)}
          {#if row.matched && (index === 0 || !visible[index - 1].matched)}<div class="group">
              <PushPin size={13} />当前应用优先
            </div>{:else if !row.matched && (index === 0 || visible[index - 1].matched)}<div
              class="group"
            >
              所有登录
            </div>{/if}
          <article class:selected={selected === row.id}>
            <button
              class="row-heading"
              aria-expanded={selected === row.id}
              on:click={() => expand(row)}
            >
              <span class="service-icon"
                ><svelte:component this={serviceIcon(row.address)} size={22} /></span
              ><span class="grow"
                ><strong>{row.title}</strong><small>{row.address || row.username}</small></span
              >{#if row.favorite}<Star size={14} weight="fill" />{/if}{#if row.matched}<span
                  class="chip">应用优先</span
                >{/if}{#if selected === row.id}<CaretDown size={16} />{:else}<CaretRight
                  size={16}
                />{/if}
            </button>
            {#if selected === row.id}
              <div class="details">
                {#each [{ field: "username", label: "用户名", value: row.username, present: !!row.username }, { field: "password", label: "密码", value: revealed || "••••••••••••", present: row.hasPassword }, { field: "totp", label: "动态验证码", value: otp?.code ?? "正在生成…", present: row.hasTotp }] as cell}
                  {#if cell.present}<div class="credential flex">
                      <div class="grow">
                        <small>{cell.label}</small><span
                          class:otp={cell.field === "totp"}
                          class="field-value">{cell.value}</span
                        >{#if cell.field === "totp"}<small
                            >{seconds} 秒后更新{otpEndingSoon ? " · 请等待下一轮" : ""}</small
                          >{/if}
                      </div>
                      {#if cell.field === "password"}<button
                          class="quiet"
                          aria-label={revealed ? "隐藏密码" : "显示密码 10 秒"}
                          title={revealed ? "隐藏密码" : "显示密码 10 秒"}
                          on:click={() => reveal(row)}
                          >{#if revealed}<EyeSlash size={17} />{:else}<Eye size={17} />{/if}</button
                        >{/if}
                      <button
                        class="primary"
                        disabled={busy ||
                          !context?.token ||
                          (cell.field === "totp" && otpEndingSoon)}
                        on:click={() => useField(row, cell.field as LoginField, true)}>插入</button
                      >
                      <button
                        class="quiet"
                        disabled={busy || (cell.field === "totp" && otpEndingSoon)}
                        aria-label={`复制${cell.label}`}
                        title={`复制${cell.label}`}
                        on:click={() => useField(row, cell.field as LoginField, false)}
                        ><CopySimple size={17} /></button
                      >
                    </div>{/if}
                {/each}
                <div class="entry-actions flex">
                  <span class="grow"
                    >{#each row.tags as name}<span class="chip">{name}</span>{/each}</span
                  ><button
                    class="quiet muted"
                    on:click={() => {
                      editEntry = row;
                      editing = true;
                      revealed = "";
                      otp = null;
                    }}><PencilSimple size={14} />编辑</button
                  ><button
                    class="quiet muted"
                    aria-label="删除登录"
                    on:click={() => (removeId = row.id)}><Trash size={15} /></button
                  >
                </div>
                {#if removeId === row.id}<div class="notice flex">
                    <span class="grow">删除此登录？此操作无法撤销。</span><button
                      on:click={() => (removeId = null)}>取消</button
                    ><button class="danger" disabled={busy} on:click={() => remove(row.id)}
                      >删除</button
                    >
                  </div>{/if}
              </div>
            {/if}
          </article>
        {/each}
        {#if !visible.length && !compact}<div class="empty">
            <Key size={32} /><strong
              >{search || tag ? "没有找到匹配的登录" : "还没有登录项目"}</strong
            >
            <p class="muted">
              {search || tag
                ? "试试其他名称、用户名或标签。"
                : "保存用户名、密码和验证码，使用时分别插入。"}
            </p>
          </div>{/if}
      </div>
    {/if}
    {#if error && !compact}<div class="error" role="alert">
        {error}
      </div>{/if}{#if message && !compact}<div class="success" role="status">{message}</div>{/if}
    {#if !compact}<footer class="flex">
        <span class="grow muted"
          >{status?.unlocked ? `${entries.length} 项登录 · 本机加密保存` : "本机加密保存"}</span
        >{#if status?.unlocked}<button
            class="quiet"
            title="新建登录"
            on:click={() => {
              editEntry = null;
              editing = true;
            }}><Plus size={19} /></button
          ><button class="quiet" title="立即锁定" on:click={lock}><LockKey size={19} /></button
          >{/if}<button class="quiet" title="登录与安全设置" on:click={openSettings}
          ><SlidersHorizontal size={19} /></button
        >
      </footer>{/if}
  {/if}
</div>

<style>
  .login-panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--surface-soft, #f8f9fc);
  }
  .context {
    min-height: 43px;
    padding: 10px 21px;
    border-bottom: 1px solid var(--border, #e8e9f0);
  }
  :global(.login-panel .tag-select) {
    max-width: 115px;
    min-height: 30px !important;
    padding: 4px 8px !important;
    font-size: 12px;
  }
  .rows {
    padding: 0 16px 16px;
    overflow-y: auto;
    flex: 1;
    min-height: 0;
  }
  .group {
    color: var(--text-muted, #8a8d99);
    font-size: 11px;
    display: flex;
    gap: 5px;
    align-items: center;
    margin: 18px 4px 9px;
  }
  article {
    background: var(--surface, white);
    border: 1px solid var(--border, #e8e9f0);
    border-radius: 10px;
    margin-bottom: 9px;
  }
  article.selected {
    border-color: var(--accent, #5b5ff0);
  }
  .row-heading {
    border: 0 !important;
    width: 100%;
    padding: 14px !important;
    text-align: left;
    justify-content: flex-start !important;
    gap: 11px !important;
  }
  .row-heading strong {
    font-size: 14px;
  }
  .row-heading small {
    display: block;
    margin-top: 5px;
  }
  .row-heading .grow {
    overflow-wrap: anywhere;
  }
  .service-icon {
    width: 38px;
    height: 38px;
    flex-shrink: 0;
    border-radius: 9px;
    background: var(--accent-soft, #f0f0ff);
    color: var(--accent, #5b5ff0);
    display: grid;
    place-items: center;
  }
  .details {
    padding: 0 14px 8px;
  }
  .credential {
    padding: 13px 0;
    border-top: 1px solid var(--border, #e8e9f0);
  }
  .credential small {
    display: block;
  }
  .field-value {
    display: block;
    margin-top: 4px;
    font-size: 14px;
    overflow-wrap: anywhere;
  }
  .field-value.otp {
    font-size: 24px;
    letter-spacing: 4px;
    font-variant-numeric: tabular-nums;
    color: var(--accent, #5b5ff0);
  }
  .entry-actions {
    margin-top: 8px;
    gap: 4px;
  }
  .entry-actions .chip {
    margin-right: 4px;
  }
  footer {
    padding: 10px 18px;
    border-top: 1px solid var(--border, #e8e9f0);
    background: var(--surface, white);
  }
  .error,
  .success {
    margin: 7px 16px;
  }
  .empty {
    margin: auto;
    padding: 45px 24px;
    text-align: center;
  }
  .empty strong {
    display: block;
    margin-top: 14px;
  }
  .compact {
    flex: none;
    max-height: 245px;
  }
  .compact:has(.rows:empty) {
    display: none;
  }
  .compact .group {
    margin-top: 8px;
  }
  .compact .context {
    min-height: 33px;
    padding-block: 6px;
  }
</style>
