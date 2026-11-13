<script lang="ts">
  import { onMount, tick } from "svelte";
  import { Dialog } from "bits-ui";
  import {
    CaretRight,
    Key,
    LockKey,
    PlusCircle,
    ShieldCheck,
    Stack,
    Trash,
    X,
  } from "phosphor-svelte";
  import AppSelect from "../../components/AppSelect.svelte";
  import { bridge } from "../../bridge";
  import { translate, t, language } from "../../i18n";
  import { keywordIssue } from "../../clipboardSensitiveRules";
  import type { ClipboardSensitiveSettings } from "../../ipc/generated";
  import "./clipboardSensitiveSettings.css";

  type Selection = number | "new" | "close";
  let settings: ClipboardSensitiveSettings | null = null;
  let open = false;
  let busy = false;
  let error = "";
  let saved = false;
  let selectedIndex: number | null = null;
  let adding = false;
  let draft = "";
  let pending: Selection | null = null;
  let keywordInput: HTMLInputElement | undefined;
  const rules = [
    { key: "credentialAssignments", title: "密码与密钥", icon: LockKey },
    { key: "bearerTokens", title: "Bearer 令牌", icon: Stack },
    { key: "privateKeys", title: "私钥", icon: Key },
  ] as const;
  const issues = {
    empty: "请输入关键词。",
    tooLong: "关键词最多 256 字节。",
    control: "关键词不能包含换行或控制字符。",
    duplicate: "已存在相同的关键词。",
    tooMany: "最多可添加 100 个关键词。",
  };
  $: keywords = settings?.customKeywords ?? [];
  $: builtinCount = settings ? rules.filter((rule) => settings?.[rule.key]).length : 0;
  $: original = selectedIndex === null ? "" : (keywords[selectedIndex] ?? "");
  $: dirty = draft.trim() !== original;
  $: issue = keywordIssue(draft, keywords, selectedIndex);
  $: editing = adding || selectedIndex !== null;

  async function load() {
    busy = true;
    error = "";
    try {
      settings = await bridge.getClipboardSensitiveSettings();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function save(patch: Partial<ClipboardSensitiveSettings>): Promise<boolean> {
    if (!settings || busy) return false;
    busy = true;
    error = "";
    saved = false;
    try {
      settings = await bridge.setClipboardSensitiveSettings({ ...settings, ...patch });
      saved = true;
      return true;
    } catch (e) {
      error = String(e);
      return false;
    } finally {
      busy = false;
    }
  }

  function select(target: Selection) {
    pending = null;
    saved = false;
    if (target === "close") {
      open = false;
      return;
    }
    selectedIndex = target === "new" ? null : target;
    adding = target === "new";
    draft = selectedIndex === null ? "" : keywords[selectedIndex];
    error = "";
    void tick().then(() => keywordInput?.focus());
  }

  function request(target: Selection) {
    if (busy || target === selectedIndex || (target === "new" && adding)) return;
    if (dirty) {
      pending = target;
      return;
    }
    select(target);
  }

  function showEditor() {
    if (!settings || busy) return;
    selectedIndex = keywords.length ? 0 : null;
    adding = false;
    draft = keywords[0] ?? "";
    pending = null;
    error = "";
    saved = false;
    open = true;
  }

  async function saveKeyword() {
    if (issue || !dirty || busy) return;
    const value = draft.trim();
    const next = [...keywords];
    const index = selectedIndex ?? next.length;
    if (selectedIndex === null) next.push(value);
    else next[selectedIndex] = value;
    if (await save({ customKeywords: next })) {
      selectedIndex = index;
      adding = false;
      draft = value;
      pending = null;
    }
  }

  async function deleteKeyword() {
    if (selectedIndex === null || busy) return;
    const next = keywords.filter((_, index) => index !== selectedIndex);
    const nextIndex = next.length ? Math.min(selectedIndex, next.length - 1) : null;
    if (await save({ customKeywords: next })) {
      selectedIndex = nextIndex;
      adding = false;
      draft = nextIndex === null ? "" : next[nextIndex];
      pending = null;
    }
  }

  onMount(() => {
    void load();
  });
</script>

<section aria-label={translate("敏感内容", $language)}>
  <h3 class="settings-group-title">{translate("敏感内容", $language)}</h3>
  <div class="settings-list settings-card-list">
    <div class="settings-row sensitive-summary">
      <span class="row-icon"><ShieldCheck size={21} /></span>
      <span class="row-copy">
        <strong>{translate("敏感内容过滤", $language)}</strong>
        <small
          >{!settings
            ? translate("正在加载…", $language)
            : !settings.enabled
              ? translate("过滤已关闭", $language)
              : t("{count} 类内置规则 · {keywords} 个关键词", $language, {
                  count: builtinCount,
                  keywords: keywords.length,
                })}</small
        >
      </span>
      <button
        class="secondary-button sensitive-edit-trigger"
        disabled={!settings || busy}
        on:click={showEditor}>{translate("编辑规则", $language)}<CaretRight size={15} /></button
      >
    </div>
  </div>
  {#if error && !open}<p class="sensitive-feedback" role="alert">
      {error}
      {#if !settings}<button class="secondary-button" disabled={busy} on:click={load}
          >{translate("重试", $language)}</button
        >{/if}
    </p>{/if}
</section>

<Dialog.Root bind:open>
  <Dialog.Portal>
    <Dialog.Overlay class="sensitive-backdrop" />
    <Dialog.Content
      class="sensitive-dialog"
      onEscapeKeydown={(event) => {
        event.preventDefault();
        request("close");
      }}
      onInteractOutside={(event) => {
        event.preventDefault();
        request("close");
      }}
    >
      <header class="sensitive-heading">
        <div class="sensitive-heading-copy">
          <Dialog.Title class="sensitive-title" level={2}
            >{translate("编辑敏感内容规则", $language)}</Dialog.Title
          >
          <Dialog.Description class="sensitive-description"
            >{translate("决定哪些内容不进入剪贴板历史", $language)}</Dialog.Description
          >
        </div>
        <button
          class="sensitive-master"
          role="switch"
          aria-label={translate("启用过滤", $language)}
          aria-checked={settings?.enabled ?? false}
          disabled={busy}
          on:click={() => settings && save({ enabled: !settings.enabled })}
        >
          <span>{translate("启用过滤", $language)}</span><span
            class:checked={settings?.enabled}
            class="switch-control"><span></span></span
          >
        </button>
        <button
          class="sensitive-close"
          aria-label={translate("关闭规则编辑器", $language)}
          disabled={busy}
          on:click={() => request("close")}><X size={21} /></button
        >
      </header>
      <div class="sensitive-workspace">
        <aside class="sensitive-sidebar" aria-label={translate("规则列表", $language)}>
          <h3>{translate("内置规则", $language)}</h3>
          <div class="sensitive-builtins">
            {#each rules as rule}
              <button
                class="sensitive-builtin"
                role="switch"
                aria-label={translate(rule.title, $language)}
                aria-checked={settings?.[rule.key] ?? false}
                disabled={!settings?.enabled || busy}
                on:click={() => settings && save({ [rule.key]: !settings[rule.key] })}
              >
                <svelte:component this={rule.icon} size={19} /><span
                  >{translate(rule.title, $language)}</span
                ><span class:checked={settings?.[rule.key]} class="switch-control"
                  ><span></span></span
                >
              </button>
            {/each}
          </div>
          <div class="sensitive-keyword-heading">
            <h3>{translate("自定义关键词", $language)}</h3>
            <button
              class="sensitive-add"
              aria-label={translate("添加关键词", $language)}
              title={translate("添加关键词", $language)}
              disabled={busy || keywords.length >= 100}
              on:click={() => request("new")}><PlusCircle size={21} /></button
            >
          </div>
          <div class="sensitive-keyword-list" aria-label={translate("自定义关键词", $language)}>
            {#each keywords as keyword, index}
              <button
                class="sensitive-keyword"
                class:selected={selectedIndex === index}
                aria-current={selectedIndex === index ? "true" : undefined}
                disabled={busy}
                on:click={() => request(index)}
                title={keyword}>{keyword}</button
              >
            {:else}
              <p class="sensitive-list-empty">{translate("还没有自定义关键词", $language)}</p>
            {/each}
            {#if adding}<button class="sensitive-keyword selected" aria-current="true" disabled
                >{translate("新关键词", $language)}</button
              >{/if}
          </div>
        </aside>
        <main class="sensitive-editor">
          {#if editing}
            <div class="sensitive-editor-copy">
              <span class="sensitive-eyebrow">{translate("自定义关键词", $language)}</span>
              <h3 title={original}>{adding ? translate("添加关键词", $language) : original}</h3>
              <p>{translate("复制的文本包含此关键词时匹配", $language)}</p>
            </div>
            <form class="sensitive-keyword-form" on:submit|preventDefault={saveKeyword}>
              <div class="sensitive-field">
                <label for="sensitive-keyword-value">{translate("关键词内容", $language)}</label
                ><input
                  id="sensitive-keyword-value"
                  bind:this={keywordInput}
                  bind:value={draft}
                  disabled={busy}
                  aria-describedby="sensitive-keyword-help"
                  aria-invalid={dirty && issue !== null}
                  autocomplete="off"
                  spellcheck="false"
                  placeholder={translate("输入要过滤的关键词", $language)}
                /><small id="sensitive-keyword-help"
                  >{translate("按字面内容匹配，不区分英文字母大小写", $language)}</small
                >{#if dirty && issue}<small class="sensitive-validation" role="alert"
                    >{translate(issues[issue], $language)}</small
                  >{/if}
              </div>
              <div class="sensitive-editor-actions">
                {#if selectedIndex !== null}<button
                    class="sensitive-delete"
                    type="button"
                    disabled={busy}
                    on:click={deleteKeyword}
                    ><Trash size={16} />{translate("删除关键词", $language)}</button
                  >{/if}<button
                  class="sensitive-save"
                  type="submit"
                  disabled={busy || !dirty || issue !== null}
                  >{translate("保存更改", $language)}</button
                >
              </div>
            </form>
          {:else}
            <div class="sensitive-empty">
              <ShieldCheck size={36} weight="light" />
              <h3>{translate("只保留你需要的内容", $language)}</h3>
              <p>
                {translate(
                  "内置规则仅识别独立凭据，普通文章中提及 password 不会被过滤。",
                  $language,
                )}
              </p>
              <button class="secondary-button" disabled={busy} on:click={() => request("new")}
                ><PlusCircle size={17} />{translate("添加关键词", $language)}</button
              >
            </div>
          {/if}
          <div class="sensitive-editor-feedback" aria-live="polite">
            {#if error}<p role="alert">{error}</p>{:else if saved}<p role="status">
                {translate("设置已保存", $language)}
              </p>{/if}
          </div>
          {#if pending !== null}<div class="sensitive-unsaved" role="alert">
              <p>{translate("有未保存的关键词修改。", $language)}</p>
              <div>
                <button
                  type="button"
                  on:click={() => {
                    pending = null;
                    void tick().then(() => keywordInput?.focus());
                  }}>{translate("继续编辑", $language)}</button
                ><button
                  type="button"
                  on:click={() => {
                    if (pending !== null) select(pending);
                  }}>{translate("放弃更改", $language)}</button
                >
              </div>
            </div>{/if}
        </main>
      </div>
      <footer class="sensitive-footer">
        <small>{translate("仅影响本机之后复制和编辑的内容", $language)}</small>
        <div class="sensitive-policy">
          <span id="sensitive-policy-label">{translate("匹配后的处理", $language)}</span><AppSelect
            value={settings?.saveSensitive ? "save" : "skip"}
            options={[
              { value: "skip", label: translate("不保存到历史", $language) },
              { value: "save", label: translate("保存并隐藏预览", $language) },
            ]}
            disabled={!settings?.enabled || busy}
            aria-labelledby="sensitive-policy-label"
            onValueChange={(value) => void save({ saveSensitive: value === "save" })}
          />
        </div>
        <button class="sensitive-done" disabled={busy} on:click={() => request("close")}
          >{translate("完成", $language)}</button
        >
      </footer>
      {#if settings?.saveSensitive}<p class="sensitive-sync-note">
          {translate("开启剪贴板同步时，保存的敏感内容仍会参与同步。", $language)}
        </p>{/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
