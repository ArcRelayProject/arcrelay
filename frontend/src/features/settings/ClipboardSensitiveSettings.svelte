<script lang="ts">
  import { onMount } from "svelte";
  import { ShieldCheck } from "phosphor-svelte";
  import { bridge } from "../../bridge";
  import { translate, language } from "../../i18n";
  import type { ClipboardSensitiveSettings } from "../../ipc/generated";

  let settings: ClipboardSensitiveSettings | null = null;
  let keywords = "";
  let busy = false;
  let error = "";
  let saved = false;
  const rules = [
    {
      key: "credentialAssignments",
      title: "独立凭据",
      description:
        "仅识别单独复制的 password=…、api_key=… 等完整凭据；不匹配文档中提及的关键词和常见占位符。",
    },
    {
      key: "bearerTokens",
      title: "Bearer 令牌",
      description: "识别单独复制的 Bearer 令牌或 Authorization 请求头。",
    },
    { key: "privateKeys", title: "私钥内容", description: "识别带有完整 BEGIN / END 标记的私钥。" },
  ] as const;
  $: normalizedKeywords = keywords
    .split("\n")
    .map((word) => word.trim())
    .filter(Boolean);
  $: invalid =
    normalizedKeywords.length > 100 ||
    normalizedKeywords.some(
      (word) =>
        new TextEncoder().encode(word).length > 256 || /[\u0000-\u001f\u007f-\u009f]/u.test(word),
    );
  $: keywordsDirty =
    settings !== null &&
    JSON.stringify(normalizedKeywords) !== JSON.stringify(settings.customKeywords);

  async function load() {
    busy = true;
    error = "";
    try {
      settings = await bridge.getClipboardSensitiveSettings();
      keywords = settings.customKeywords.join("\n");
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function save(patch: Partial<ClipboardSensitiveSettings>) {
    if (!settings || busy) return;
    busy = true;
    error = "";
    saved = false;
    try {
      settings = await bridge.setClipboardSensitiveSettings({ ...settings, ...patch });
      if (patch.customKeywords) keywords = settings.customKeywords.join("\n");
      saved = true;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    void load();
  });
</script>

<section aria-label={translate("敏感内容", $language)}>
  <h3 class="settings-group-title">{translate("敏感内容", $language)}</h3>
  <div class="settings-list settings-card-list">
    <button
      class="settings-row settings-toggle-row"
      role="switch"
      aria-label={translate("识别敏感内容", $language)}
      aria-checked={settings?.enabled ?? false}
      disabled={!settings || busy}
      on:click={() => settings && save({ enabled: !settings.enabled })}
    >
      <span class="row-icon"><ShieldCheck size={21} /></span>
      <span class="row-copy"
        ><strong>{translate("识别敏感内容", $language)}</strong><small
          >{translate(
            "仅影响本机之后复制和编辑的文本，不会重新处理已有历史。普通文本中出现 password 不会触发内置规则。",
            $language,
          )}</small
        ></span
      >
      <span class:checked={settings?.enabled} class="switch-control"><span></span></span>
    </button>
    {#each rules as rule}
      <button
        class="settings-row settings-toggle-row rule-row"
        role="switch"
        aria-label={translate(rule.title, $language)}
        aria-checked={settings?.[rule.key] ?? false}
        disabled={!settings?.enabled || busy}
        on:click={() => settings && save({ [rule.key]: !settings[rule.key] })}
      >
        <span class="row-copy"
          ><strong>{translate(rule.title, $language)}</strong><small
            >{translate(rule.description, $language)}</small
          ></span
        >
        <span class:checked={settings?.[rule.key]} class="switch-control"><span></span></span>
      </button>
    {/each}
    <div class="settings-row keyword-row">
      <label class="row-copy" for="clipboard-sensitive-keywords"
        ><strong>{translate("自定义关键词", $language)}</strong><small id="clipboard-keyword-help"
          >{translate(
            "每行一个，匹配整段文本中的字面内容；英文字母不区分大小写，不支持正则表达式。最多 100 条，每条最多 256 字节。",
            $language,
          )}</small
        ></label
      >
      <textarea
        aria-label={translate("自定义关键词", $language)}
        id="clipboard-sensitive-keywords"
        aria-describedby="clipboard-keyword-help"
        bind:value={keywords}
        disabled={!settings?.enabled || busy}
        rows="4"
        spellcheck="false"
        placeholder={translate("例如：内部资料", $language)}
      ></textarea>
      {#if invalid}<small role="alert"
          >{translate(
            "最多 100 条关键词，每条最多 256 字节，且不能包含控制字符。",
            $language,
          )}</small
        >{/if}
      <button
        class="secondary-button save-keywords"
        disabled={!settings?.enabled || busy || invalid || !keywordsDirty}
        on:click={() => save({ customKeywords: normalizedKeywords })}
        >{translate("保存关键词", $language)}</button
      >
    </div>
    <button
      class="settings-row settings-toggle-row rule-row"
      role="switch"
      aria-label={translate("保存命中的敏感内容", $language)}
      aria-checked={settings?.saveSensitive ?? false}
      disabled={!settings?.enabled || busy}
      on:click={() => settings && save({ saveSensitive: !settings.saveSensitive })}
    >
      <span class="row-copy"
        ><strong>{translate("保存命中的敏感内容", $language)}</strong><small
          >{translate(
            "关闭时不保存命中规则的文本。开启后保存并隐藏列表预览；若开启剪贴板同步，内容仍会参与同步。",
            $language,
          )}</small
        ></span
      >
      <span class:checked={settings?.saveSensitive} class="switch-control"><span></span></span>
    </button>
  </div>
  {#if error}<p class="settings-feedback" role="alert">
      {error}
      {#if !settings}<button disabled={busy} on:click={load}>{translate("重试", $language)}</button
        >{/if}
    </p>{/if}
  {#if saved}<p class="settings-feedback" role="status">
      {translate("设置已保存", $language)}
    </p>{/if}
</section>

<style>
  .row-copy small {
    white-space: normal;
    overflow: visible;
    text-overflow: clip;
    line-height: 1.5;
  }
  .rule-row .row-copy {
    grid-column: 2;
  }
  .rule-row .switch-control {
    grid-column: 3;
  }
  section .keyword-row > * {
    grid-column: 2 / -1;
  }
  section .keyword-row textarea {
    justify-self: stretch;
  }
  textarea {
    box-sizing: border-box;
    width: 100%;
    resize: vertical;
    min-height: 96px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
    color: inherit;
    background: var(--surface-raised);
    font: inherit;
  }
  .save-keywords {
    justify-self: end;
  }
  .settings-feedback {
    font-size: 13px;
    padding: 0 16px;
  }
</style>
