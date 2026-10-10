<script lang="ts">
  import AppSelect from "../../components/AppSelect.svelte";
  import {
    ArrowLeft,
    CaretRight,
    Check,
    Plus,
    PushPin,
    ShieldCheck,
    Trash,
    X,
  } from "phosphor-svelte";
  import type {
    LoginDraft,
    LoginSummary,
    LoginAppRule,
    InstalledAppView,
  } from "../../ipc/generated";
  import { loginRequest, loginApps, applicationId } from "./bridge";
  import { onMount } from "svelte";
  import InstalledAppIcon from "../../InstalledAppIcon.svelte";
  import "./login.css";
  export let entry: LoginSummary | null = null;
  export let onClose: () => void;
  export let onSaved: () => void;
  let draft: LoginDraft = {
    id: entry?.id ?? null,
    title: entry?.title ?? "",
    address: entry?.address ?? "",
    username: entry?.username ?? "",
    password: entry ? null : "",
    keepTotp: !!entry?.hasTotp,
    totp: null,
    tags: entry?.tags.slice() ?? [],
    favorite: entry?.favorite ?? false,
    apps: entry?.apps.map((a) => ({ ...a })) ?? [],
  };
  let password = "";
  let removePassword = false;
  let tags = entry?.tags.join(", ") ?? "";
  let page: "entry" | "apps" | "totp" = "entry";
  let secret = "";
  let algorithm = entry?.totpAlgorithm ?? "SHA1";
  let digits = entry?.totpDigits ?? 6;
  let period = entry?.totpPeriod ?? 30;
  let apps: InstalledAppView[] = [];
  let appSearch = "";
  let rules: LoginAppRule[] = [];
  let priority = 80;
  let enabled = true;
  let manual = false;
  let manualName = "";
  let manualId = "";
  let busy = false;
  let error = "";
  onMount(() => () => {
    password = "";
    secret = "";
    draft.password = null;
    draft.totp = null;
  });
  async function openApps() {
    page = "apps";
    rules = draft.apps.map((a) => ({ ...a }));
    priority = rules[0]?.priority ?? 80;
    enabled = rules.some((a) => a.enabled) || !rules.length;
    error = "";
    try {
      apps = await loginApps();
    } catch (e) {
      error = String(e);
    }
  }
  function toggleApp(app: InstalledAppView, checked: boolean) {
    const id = applicationId(app);
    rules = rules.filter((a) => a.id !== id);
    if (checked) rules = [...rules, { id, name: app.name, enabled, priority }];
  }
  async function pickApp() {
    try {
      const r = await loginRequest({ type: "pickApp" });
      if (r.application && !rules.some((a) => a.id === r.application!.id))
        rules = [...rules, { ...r.application, enabled, priority }];
    } catch (e) {
      error = String(e);
    }
  }
  function addManual() {
    if (!manualName.trim() || !manualId.trim()) {
      error = "请输入应用名称和准确的应用标识";
      return;
    }
    const id = /Win/.test(navigator.platform) ? manualId.trim().toLowerCase() : manualId.trim();
    if (!rules.some((a) => a.id === id))
      rules = [...rules, { id, name: manualName.trim(), enabled, priority }];
    manual = false;
    manualName = "";
    manualId = "";
    error = "";
  }
  async function copyPrivate(event: ClipboardEvent) {
    event.preventDefault();
    const input = event.currentTarget as HTMLInputElement | HTMLTextAreaElement;
    const text = input.value.slice(input.selectionStart ?? 0, input.selectionEnd ?? 0);
    if (!text) return;
    try {
      await loginRequest({ type: "copyDraft", value: text });
    } catch (e) {
      error = String(e);
    }
  }
  function cutPrivate(event: ClipboardEvent) {
    event.preventDefault();
    error = "请使用复制，或删除字段文字；凭据不会进入普通剪贴板。";
  }
  async function save() {
    if (!draft.title.trim()) {
      error = "请填写登录名称";
      return;
    }
    busy = true;
    error = "";
    try {
      const payload = {
        ...draft,
        password: entry ? (removePassword ? "" : password || null) : password,
        tags: tags
          .split(/[,，]/)
          .map((t) => t.trim())
          .filter(Boolean),
      };
      await loginRequest({ type: "save", draft: payload });
      password = "";
      secret = "";
      onSaved();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
  function keepTotp() {
    if (!secret.trim()) {
      error = "请输入密钥或 otpauth:\u002f\u002ftotp 地址";
      return;
    }
    draft = { ...draft, keepTotp: false, totp: { secret, algorithm, digits, period } };
    secret = "";
    page = "entry";
    error = "";
  }
</script>

<div class="login-ui editor">
  <header class="flex">
    <button
      class="quiet"
      aria-label="返回"
      disabled={busy}
      on:click={() => (page === "entry" ? onClose() : ((page = "entry"), (error = "")))}
      ><ArrowLeft size={20} /></button
    >
    <strong class="grow"
      >{page === "apps"
        ? "在特定应用中优先显示"
        : page === "totp"
          ? "配置动态验证码"
          : entry
            ? "编辑登录"
            : "新建登录"}</strong
    >
    <button class="quiet" aria-label="取消编辑" disabled={busy} on:click={onClose}
      ><X size={19} /></button
    >
  </header>
  <div class="body">
    {#if page === "entry"}
      <div class="notice flex"><ShieldCheck size={18} />保存后自动加密 · 仅在这台设备上使用</div>
      <label class="field"
        >登录名称<input
          bind:value={draft.title}
          maxlength="256"
          placeholder="例如 GitHub · 工作账号"
          required
        /></label
      >
      <label class="field"
        >网站或地址<input
          bind:value={draft.address}
          maxlength="2048"
          placeholder="github.com（选填）"
        /></label
      >
      <label class="field"
        >用户名<input
          bind:value={draft.username}
          on:copy={copyPrivate}
          on:cut={cutPrivate}
          maxlength="4096"
          autocomplete="off"
          placeholder="邮箱或用户名"
        /></label
      >
      <label class="field"
        >密码<input
          type="password"
          bind:value={password}
          on:copy={copyPrivate}
          on:cut={cutPrivate}
          on:input={() => (removePassword = false)}
          maxlength="16384"
          autocomplete="new-password"
          placeholder={entry?.hasPassword ? "已保存密码；留空保留，输入后替换" : "输入密码（选填）"}
        /></label
      >
      {#if entry?.hasPassword}<button
          class="quiet muted"
          on:click={() => {
            removePassword = true;
            password = "";
          }}>移除已保存密码</button
        >{#if removePassword}<small>保存后移除密码</small>{/if}{/if}
      <div class="setting-line flex">
        <span class="grow"
          ><strong>动态验证码</strong><small
            >{draft.keepTotp || draft.totp ? "已配置 · 密钥不会回显" : "尚未配置 TOTP"}</small
          ></span
        ><button
          on:click={() => {
            page = "totp";
            error = "";
          }}>配置<CaretRight size={14} /></button
        >
      </div>
      <label class="field"
        >标签<input bind:value={tags} placeholder="工作, 个人（以逗号分隔）" /></label
      >
      <label class="flex"><input type="checkbox" bind:checked={draft.favorite} />收藏此登录</label>
      <button class="rule-link flex" on:click={openApps}
        ><PushPin size={20} /><span class="grow"
          ><strong>应用优先显示</strong><small
            >{draft.apps.filter((a) => a.enabled).length
              ? draft.apps
                  .filter((a) => a.enabled)
                  .map((a) => a.name)
                  .join("、")
              : "配置特定应用在前台时置顶"}</small
          ></span
        ><CaretRight size={16} /></button
      >
    {:else if page === "apps"}
      <p class="muted">打开剪贴板时，关联的登录会排在该应用的优先分组中。</p>
      <label class="flex"
        ><input type="checkbox" bind:checked={enabled} />在所选应用中优先显示</label
      >
      <label class="field"
        >优先级<AppSelect
          aria-label="优先级"
          bind:value={priority}
          options={[
            { value: 100, label: "最高 · 100" },
            { value: 80, label: "高 · 80" },
            { value: 50, label: "普通 · 50" },
            { value: 20, label: "低 · 20" },
          ]}
        /><small>同一应用中，优先级越高越靠前。</small></label
      >
      <label class="field"
        >选择应用 · 可多选<input bind:value={appSearch} placeholder="搜索已安装的应用" /></label
      >
      <div class="app-list">
        {#each apps.filter((a) => a.name
            .toLowerCase()
            .includes(appSearch.toLowerCase())) as app (app.path)}
          <label class="app-row flex"
            ><input
              type="checkbox"
              checked={rules.some((a) => a.id === applicationId(app))}
              on:change={(e) => toggleApp(app, e.currentTarget.checked)}
            /><InstalledAppIcon
              path={app.path}
              name={app.name}
              initialIcon={app.iconDataUrl}
              size={24}
            /><span class="grow"
              ><strong>{app.name}</strong><small>{applicationId(app)}</small></span
            ></label
          >
        {/each}
        {#if apps.length === 0}<p class="muted">
            未找到已安装应用，可手动添加准确的应用标识。
          </p>{/if}
      </div>
      {#each rules.filter((r) => !apps.some((a) => applicationId(a) === r.id)) as rule (rule.id)}<div
          class="app-row flex"
        >
          <span class="grow">{rule.name}<small>{rule.id}</small></span><button
            class="quiet"
            aria-label="移除应用"
            on:click={() => (rules = rules.filter((a) => a.id !== rule.id))}><X size={16} /></button
          >
        </div>{/each}
      <button class="quiet" on:click={pickApp}><Plus size={16} />从文件选择应用</button>
      <button class="quiet" on:click={() => (manual = !manual)}
        ><Plus size={16} />手动添加应用</button
      >
      {#if manual}<label class="field"
          >应用名称<input bind:value={manualName} maxlength="256" /></label
        ><label class="field"
          >应用标识<input
            bind:value={manualId}
            maxlength="4096"
            placeholder="macOS Bundle ID 或 Windows 可执行文件完整路径"
          /></label
        ><button on:click={addManual}>添加到已选应用</button>{/if}
      <div class="notice">
        预览：{enabled && rules.length
          ? `在 ${rules.map((a) => a.name).join("、")} 中，此登录优先显示（${priority}）。`
          : "此登录按普通顺序显示。"}<br
        />规则仅在本机生效。浏览器关联适用于整个浏览器；不会自动识别网站、解锁或插入。
      </div>
    {:else}
      <p class="muted">
        输入身份验证器密钥，或粘贴完整的 otpauth:&#47;&#47;totp 地址。保存后密钥不再回显。
      </p>
      <label class="field"
        >密钥或 TOTP 地址<textarea
          bind:value={secret}
          on:copy={copyPrivate}
          on:cut={cutPrivate}
          rows="3"
          maxlength="8192"
          autocomplete="off"
          spellcheck="false"
        ></textarea></label
      >
      <label class="field"
        >算法<AppSelect
          aria-label="验证码算法"
          bind:value={algorithm}
          options={["SHA1", "SHA256", "SHA512"].map((value) => ({ value, label: value }))}
        /></label
      >
      <label class="field"
        >位数<AppSelect
          aria-label="验证码位数"
          bind:value={digits}
          options={[
            { value: 6, label: "6 位" },
            { value: 8, label: "8 位" },
          ]}
        /></label
      >
      <label class="field"
        >周期（秒）<input type="number" min="15" max="120" bind:value={period} /></label
      >
      <p class="notice">
        TOTP 地址中的算法、位数和周期会优先使用。验证码即将到期时会等待下一轮；请保持设备时间准确。
      </p>
      {#if draft.keepTotp || draft.totp}<button
          class="danger"
          on:click={() => {
            draft = { ...draft, keepTotp: false, totp: null };
            secret = "";
            page = "entry";
          }}><Trash size={16} />移除验证码配置</button
        >{/if}
    {/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </div>
  <footer class="flex">
    <span class="grow muted"
      >{page === "entry" ? "保存前的修改仅保留在当前编辑中" : "完成后返回登录编辑"}</span
    ><button disabled={busy} on:click={() => (page === "entry" ? onClose() : (page = "entry"))}
      >取消</button
    ><button
      class="primary"
      disabled={busy}
      on:click={() =>
        page === "entry"
          ? save()
          : page === "apps"
            ? ((draft = { ...draft, apps: rules.map((a) => ({ ...a, enabled, priority })) }),
              (page = "entry"))
            : keepTotp()}
      ><Check size={16} />{busy ? "保存中…" : page === "entry" ? "保存登录" : "完成"}</button
    >
  </footer>
</div>

<style>
  .editor {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--surface, white);
  }
  header {
    padding: 15px 17px;
    border-bottom: 1px solid var(--border-color, #dedfe7);
  }
  header strong {
    font-size: 16px;
  }
  .body {
    flex: 1;
    overflow-y: auto;
    padding: 12px 24px;
  }
  :global(.editor label.field) {
    margin: 12px 0;
  }
  small {
    display: block;
    margin-top: 4px;
    overflow-wrap: anywhere;
  }
  .setting-line {
    border-block: 1px solid var(--border, #e8e9f0);
    padding: 10px 0;
    margin-block: 10px;
  }
  .rule-link {
    width: 100%;
    text-align: left;
    margin-top: 12px;
    padding: 10px !important;
    justify-content: flex-start !important;
  }
  .rule-link small {
    font-size: 11px;
  }
  .app-list {
    max-height: 245px;
    overflow-y: auto;
    border: 1px solid var(--border-color, #dedfe7);
    border-radius: 9px;
  }
  .app-row {
    padding: 12px;
    border-bottom: 1px solid var(--border, #e8e9f0);
  }
  .app-row:last-child {
    border-bottom: 0;
  }
  .app-row small {
    font-size: 10px;
  }
  .notice {
    margin-top: 4px;
  }
  footer {
    padding: 14px 18px;
    border-top: 1px solid var(--border-color, #dedfe7);
  }
  footer span {
    font-size: 11px;
  }
</style>
