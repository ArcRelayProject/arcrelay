<script lang="ts">
  import AppSelect from "../../../components/AppSelect.svelte";
  import { translate as uiTranslate, language as uiLanguage } from "../../../i18n";
  import type { KeyboardProfile } from "../../../types";
  import KeyboardTester from "./KeyboardTester.svelte";
  import MappingRuleEditor from "./MappingRuleEditor.svelte";

  export let profiles: KeyboardProfile[];
  export let onChange: () => void;
  let selectedKind: KeyboardProfile["kind"] = "Productivity";
  $: profile = profiles.find((value) => value.kind === selectedKind) ?? profiles[0];

  function addRule() {
    if (!profile) return;
    profile.semanticOverrides.push({ source: { modifiers: [224], key: 6 }, action: "Copy" });
    onChange();
  }

  function deleteRule(index: number) {
    if (!profile) return;
    profile.semanticOverrides.splice(index, 1);
    onChange();
  }
</script>

<div class="profiles">
  <nav class="profile-list card">
    <span class="section-label">{uiTranslate("内置配置", $uiLanguage)}</span>
    {#each profiles as item}
      <button class:active={item.kind === profile?.kind} on:click={() => selectedKind = item.kind}>
        <strong>{item.name}</strong>
        <small>revision {item.revision}</small>
      </button>
    {/each}
  </nav>

  {#if profile}
    <div class="editor card">
      <div class="editor-header">
        <div><span class="section-label">{uiTranslate("规则优先级已锁定", $uiLanguage)}</span><h2>{profile.name}</h2></div>
        <label class="field">{uiTranslate("文字策略", $uiLanguage)}
          <AppSelect bind:value={profile.textStrategy} onValueChange={onChange} aria-label={uiTranslate("文字策略", $uiLanguage)}
            options={[
              { value: "UseTargetLayout", label: uiTranslate("使用目标布局", $uiLanguage) },
              { value: "FollowSourceText", label: uiTranslate("跟随来源文字", $uiLanguage) },
            ]}
          />
        </label>
      </div>
      <p class="muted explanation">{uiTranslate("临时 Raw 模式 → 目标应用 Profile → 用户规则 → 设备对规则 → 系统预设 → 物理键直通。映射从首个按键按下锁定到整个组合键释放。", $uiLanguage)}</p>
      <div class="rules">
        <MappingRuleEditor />
        <MappingRuleEditor source="Windows Ctrl+V" target="macOS Command+V" description="粘贴" />
        <MappingRuleEditor source="Windows Alt+Tab" target="macOS Command+Tab" description="切换应用" />
        {#if profile.kind === "Terminal"}
          <MappingRuleEditor source="Windows Ctrl+C" target="物理 Ctrl+C" description="终端中断" />
        {/if}
      </div>
      <div class="custom-heading"><h3>{uiTranslate("自定义语义规则", $uiLanguage)}</h3><button class="button secondary" on:click={addRule}>{uiTranslate("添加规则", $uiLanguage)}</button></div>
      <div class="rules">
        {#each profile.semanticOverrides as rule, index}
          <MappingRuleEditor {rule} {onChange} onDelete={() => deleteRule(index)} />
        {/each}
        {#if profile.semanticOverrides.length === 0}<p class="muted">{uiTranslate("尚无自定义覆盖，将使用系统组合预设。", $uiLanguage)}</p>{/if}
      </div>
      <KeyboardTester />
    </div>
  {/if}
</div>

<style>
  .profiles { min-height: 0; flex: 1; display: grid; grid-template-columns: 230px 1fr; gap: 16px; }
  .profile-list { padding: 12px; display: grid; align-content: start; gap: 6px; }
  .section-label { margin: 8px; color: #7b8494; font-size: 10px; font-weight: 800; letter-spacing: .1em; text-transform: uppercase; }
  .profile-list button { display: grid; gap: 4px; padding: 11px 12px; border: 0; border-radius: 10px; text-align: left; color: inherit; background: transparent; }
  .profile-list button.active { color: #4338ca; background: #eeecff; }
  .profile-list small { color: #8a94a6; }
  .editor { padding: 24px; overflow: auto; }
  .editor-header { display: flex; justify-content: space-between; align-items: flex-start; }
  h2 { margin: 4px 8px 0; }
  .explanation { max-width: 760px; font-size: 13px; line-height: 1.55; }
  .rules { display: grid; grid-template-columns: repeat(2, minmax(250px, 1fr)); gap: 10px; margin: 20px 0; }
  .custom-heading { display: flex; justify-content: space-between; align-items: center; margin-top: 26px; }.custom-heading h3 { margin: 0; }
</style>
