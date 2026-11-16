<script lang="ts">
  import { applyKeyboardDraft } from "./workspaceDraft";
  import { Portal } from "bits-ui";
  export let actionsTarget: HTMLElement | undefined = undefined;
  import InputMenu from "./InputMenu.svelte";
  import { diagnosticMessage } from "../../diagnosticMessages";
  import AppSelect from "../../components/AppSelect.svelte";
  import { translate as uiTranslate, language as uiLanguage } from "../../i18n";
  import {
    Briefcase,
    CaretDown,
    ArrowRight,
    ArrowCounterClockwise,
    Keyboard,
    Plus,
    Trash,
  } from "phosphor-svelte";
  import type {
    InputOsFamily,
    KeyboardProfile,
    RuntimeSnapshot,
    WorkspaceConfiguration,
  } from "../../types";
  import { bridge } from "../../bridge";
  import { DEFAULT_KEYBOARD_PROFILES, keyboardProfileName } from "./inputSharingUi";
  import KeyboardTester from "./keyboard/KeyboardTester.svelte";
  import ConsumerKeysSection from "./ConsumerKeysSection.svelte";

  export let snapshot: RuntimeSnapshot;
  export let onSnapshot: (snapshot: RuntimeSnapshot) => void;
  export let notify: (message: string, error?: boolean) => void;

  let configuration: WorkspaceConfiguration = structuredClone(snapshot.configuration);
  if (configuration.keyboardProfiles.length === 0)
    configuration.keyboardProfiles = structuredClone(DEFAULT_KEYBOARD_PROFILES);
  let selectedKind: KeyboardProfile["kind"] = configuration.activeKeyboardProfile ?? "Productivity";
  let dirty = false;
  let showAdvanced = false;
  let saving = false;
  export function hasUnsavedChanges() {
    return dirty;
  }

  function osLabel(system: InputOsFamily | undefined, currentLanguage: typeof $uiLanguage) {
    if (system === "MacOs") return "macOS";
    if (system === "Windows") return "Windows";
    if (system === "Android") return "Android";
    if (system === "Ios") return "iOS / iPadOS";
    if (system === "LinuxX11") return "Linux · X11";
    if (system === "LinuxWayland") return "Linux · Wayland";
    return uiTranslate("等待设备信息", currentLanguage);
  }
  function primaryModifier(system: string) {
    return system === "macOS" ? "⌘" : "Ctrl";
  }
  function applicationSwitchModifier(system: string) {
    return system === "macOS" ? "⌘" : "Alt";
  }
  $: presetRows = [
    {
      action: "复制",
      source: [primaryModifier(sourceSystem), "C"],
      target: [primaryModifier(targetSystem), "C"],
      status: "已适配",
    },
    {
      action: "粘贴",
      source: [primaryModifier(sourceSystem), "V"],
      target: [primaryModifier(targetSystem), "V"],
      status: "已适配",
    },
    {
      action: "撤销",
      source: [primaryModifier(sourceSystem), "Z"],
      target: [primaryModifier(targetSystem), "Z"],
      status: "已适配",
    },
    {
      action: "切换应用",
      source: [applicationSwitchModifier(sourceSystem), "Tab"],
      target: [applicationSwitchModifier(targetSystem), "Tab"],
      status: "已适配",
    },
  ];

  $: profile =
    configuration.keyboardProfiles.find((value) => value.kind === selectedKind) ??
    configuration.keyboardProfiles[0];
  $: remoteDeviceId =
    snapshot.connectedPeers.find((id) =>
      Object.values(configuration.layout?.displays ?? {}).some(
        (display) => display.deviceId === id,
      ),
    ) ??
    Object.values(configuration.layout?.displays ?? {}).find(
      (display) => display.deviceId !== snapshot.serviceInstanceId,
    )?.deviceId;
  $: sourceSystem = osLabel(snapshot.localOperatingSystem, $uiLanguage);
  $: targetSystem = osLabel(
    remoteDeviceId ? snapshot.remoteOperatingSystems[remoteDeviceId] : undefined,
    $uiLanguage,
  );
  $: targetCapabilities = remoteDeviceId
    ? snapshot.nearbyPeers.find((peer) => peer.serviceInstanceId === remoteDeviceId)?.capabilities
    : undefined;
  $: appPointerOnly =
    targetCapabilities?.canInjectAppPointer && !targetCapabilities.canInjectKeyboard;

  async function save() {
    if (saving) return;
    saving = true;
    try {
      const next = await bridge.saveWorkspace(
        applyKeyboardDraft(snapshot.configuration, configuration),
      );
      onSnapshot(next);
      configuration = structuredClone(next.configuration);
      dirty = false;
      notify(uiTranslate("键盘行为已保存", $uiLanguage));
    } catch (error) {
      notify(diagnosticMessage(error, $uiLanguage), true);
    } finally {
      saving = false;
    }
  }

  function restoreDefaults() {
    configuration.keyboardProfiles = structuredClone(DEFAULT_KEYBOARD_PROFILES);
    configuration.activeKeyboardProfile = "Productivity";
    configuration.rightOptionRawMode = true;
    configuration.horizontalScrollBehavior = "NativeScroll";
    configuration.consumerShortcuts = [];
    selectedKind = "Productivity";
    dirty = true;
    notify(uiTranslate("已恢复推荐键盘行为，保存后生效", $uiLanguage));
  }

  function addRule() {
    if (!profile) return;
    profile.semanticOverrides.push({ source: { modifiers: [224], key: 6 }, action: "Copy" });
    configuration = structuredClone(configuration);
    dirty = true;
    showAdvanced = true;
  }

  function deleteRule(index: number) {
    if (!profile) return;
    profile.semanticOverrides.splice(index, 1);
    configuration = structuredClone(configuration);
    dirty = true;
  }

  function update() {
    configuration = structuredClone(configuration);
    dirty = true;
  }

  function selectProfile(kind: KeyboardProfile["kind"]) {
    selectedKind = kind;
    configuration.activeKeyboardProfile = kind;
    update();
  }
</script>

<section class="keyboard-page">
  <Portal to={actionsTarget} disabled={!actionsTarget}
    ><div class="page-actions">
      <button class="input-primary" disabled={!dirty || saving} on:click={save}
        >{uiTranslate(saving ? "保存中…" : "保存更改", $uiLanguage)}</button
      ><InputMenu
        label={uiTranslate("键盘更多选项", $uiLanguage)}
        items={[
          {
            label: uiTranslate("恢复默认", $uiLanguage),
            icon: ArrowCounterClockwise,
            action: restoreDefaults,
          },
        ]}
      />
    </div></Portal
  >
  <section class="settings-section">
    <h2>{uiTranslate("使用场景", $uiLanguage)}</h2>
    <div class="setting-row">
      <span><Briefcase size={19} />{uiTranslate("当前场景", $uiLanguage)}</span><AppSelect
        bind:value={selectedKind}
        onValueChange={() => selectProfile(selectedKind)}
        aria-label={uiTranslate("使用场景", $uiLanguage)}
        options={configuration.keyboardProfiles.map((item) => ({
          value: item.kind,
          label: keyboardProfileName(item, $uiLanguage),
        }))}
      />
    </div>
  </section>
  {#if appPointerOnly}<p>{uiTranslate("仅应用内鼠标；键盘输入返回电脑", $uiLanguage)}</p>
  {:else if profile}
    <section class="settings-section">
      <header>
        <h2>{uiTranslate("常用快捷键", $uiLanguage)}</h2>
        <small>{sourceSystem} → {targetSystem}</small>
      </header>
      <div class="shortcut-table" role="table" aria-label={uiTranslate("常用快捷键", $uiLanguage)}>
        <div class="table-heading" role="row">
          <span role="columnheader">{uiTranslate("操作", $uiLanguage)}</span><span
            role="columnheader">{uiTranslate("这台设备", $uiLanguage)}</span
          ><span></span><span role="columnheader">{uiTranslate("目标设备", $uiLanguage)}</span>
        </div>
        {#each presetRows as row}<div role="row">
            <span role="cell">{uiTranslate(row.action, $uiLanguage)}</span><span
              class="key-group"
              role="cell"
              >{#each row.source as key}<kbd>{key}</kbd>{/each}</span
            ><ArrowRight size={15} /><span class="key-group" role="cell"
              >{#each row.target as key}<kbd>{key}</kbd>{/each}</span
            >
          </div>{/each}
      </div>
    </section>
    <section class="settings-section">
      <h2>{uiTranslate("输入方式", $uiLanguage)}</h2>
      <div class="setting-row">
        <span>{uiTranslate("文字输入", $uiLanguage)}</span><AppSelect
          bind:value={profile.textStrategy}
          onValueChange={update}
          aria-label={uiTranslate("文字输入", $uiLanguage)}
          options={[
            { value: "UseTargetLayout", label: uiTranslate("使用目标键盘布局", $uiLanguage) },
            { value: "FollowSourceText", label: uiTranslate("保持来源文字", $uiLanguage) },
          ]}
        />
      </div>
      <label class="setting-row"
        ><span
          >{uiTranslate("右 Option 直通", $uiLanguage)}<small
            >{uiTranslate("按住时发送原始按键", $uiLanguage)}</small
          ></span
        ><input
          class="toggle"
          aria-label={uiTranslate("右 Option 直通", $uiLanguage)}
          type="checkbox"
          bind:checked={configuration.rightOptionRawMode}
          on:change={update}
        /></label
      >
      <div class="setting-row">
        <span>{uiTranslate("双指横向手势", $uiLanguage)}</span><AppSelect
          bind:value={configuration.horizontalScrollBehavior}
          onValueChange={update}
          aria-label={uiTranslate("双指横向手势", $uiLanguage)}
          options={[
            { value: "NativeScroll", label: uiTranslate("横向滚动", $uiLanguage) },
            { value: "NavigateHistory", label: uiTranslate("返回与前进", $uiLanguage) },
          ]}
        />
      </div>
    </section>
    <div class="disclosures">
      <button
        class="disclosure"
        aria-expanded={showAdvanced}
        on:click={() => (showAdvanced = !showAdvanced)}
        ><span>{uiTranslate("自定义快捷键", $uiLanguage)}</span><small
          >{profile.semanticOverrides.length || uiTranslate("无", $uiLanguage)}</small
        ><CaretDown size={16} class={showAdvanced ? "expanded" : ""} /></button
      >
      {#if showAdvanced}
        <button class="input-secondary" on:click={addRule}
          ><Plus size={16} />{uiTranslate("添加自定义规则", $uiLanguage)}</button
        >
        <div class="custom-rules">
          <p>
            {uiTranslate(
              "用户规则优先于系统预设；一个组合键会在首次按下后锁定到松开。",
              $uiLanguage,
            )}
          </p>
          {#each profile.semanticOverrides as rule, index}
            <div>
              <label
                >{uiTranslate("修饰键", $uiLanguage)}<AppSelect
                  multiple
                  bind:value={rule.source.modifiers}
                  onValueChange={update}
                  aria-label={uiTranslate("修饰键", $uiLanguage)}
                  options={[
                    { value: 224, label: "Ctrl" },
                    { value: 225, label: "Shift" },
                    { value: 226, label: "Option" },
                    { value: 227, label: "Command" },
                  ]}
                /></label
              ><label
                >{uiTranslate("主键", $uiLanguage)}<input
                  type="number"
                  min="4"
                  max="231"
                  bind:value={rule.source.key}
                  on:change={update}
                /></label
              ><label
                >{uiTranslate("语义", $uiLanguage)}<AppSelect
                  bind:value={rule.action}
                  onValueChange={update}
                  aria-label={uiTranslate("语义", $uiLanguage)}
                  options={[
                    { value: "Copy", label: uiTranslate("复制", $uiLanguage) },
                    { value: "Paste", label: uiTranslate("粘贴", $uiLanguage) },
                    { value: "Cut", label: uiTranslate("剪切", $uiLanguage) },
                    { value: "Undo", label: uiTranslate("撤销", $uiLanguage) },
                    { value: "Redo", label: uiTranslate("重做", $uiLanguage) },
                    { value: "SelectAll", label: uiTranslate("全选", $uiLanguage) },
                    { value: "ApplicationSwitch", label: uiTranslate("切换应用", $uiLanguage) },
                  ]}
                /></label
              ><button
                aria-label={uiTranslate("删除规则", $uiLanguage)}
                on:click={() => deleteRule(index)}><Trash size={16} /></button
              >
            </div>
          {/each}
          {#if profile.semanticOverrides.length === 0}<span
              >{uiTranslate("尚无自定义规则。", $uiLanguage)}</span
            >{/if}
        </div>
      {/if}
      <details>
        <summary>{uiTranslate("媒体与系统快捷键", $uiLanguage)}<CaretDown size={16} /></summary
        ><ConsumerKeysSection {snapshot} bind:configuration onChange={update} />
      </details>
      <details>
        <summary
          ><Keyboard size={18} />{uiTranslate("测试按键", $uiLanguage)}<CaretDown
            size={16}
          /></summary
        ><KeyboardTester />
      </details>
    </div>
  {/if}
</section>

<style>
  .keyboard-page {
    position: relative;
    width: min(100%, 960px);
    padding-bottom: 24px;
  }
  .page-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    margin: 0;
    min-height: 36px;
  }
  .settings-section {
    margin: 0 0 24px;
  }
  h2 {
    margin: 0 0 12px;
    font-size: 16px;
    font-weight: 650;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  header small {
    color: var(--text-muted);
    font-size: 12px;
    margin-bottom: 12px;
  }
  .setting-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    min-height: 53px;
    border-block: 1px solid var(--border);
    font-size: 13px;
  }
  .setting-row + .setting-row {
    border-top: 0;
  }
  .setting-row > span {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .setting-row small {
    color: var(--text-muted);
    font-size: 12px;
  }
  .setting-row :global(.app-select-trigger) {
    min-width: 180px;
  }
  .toggle {
    appearance: none;
    position: relative;
    width: 42px;
    height: 24px;
    flex: none;
    margin: 0;
    border: 0;
    border-radius: 99px;
    background: var(--border-strong);
    cursor: pointer;
  }
  .toggle::after {
    content: "";
    position: absolute;
    top: 3px;
    left: 3px;
    width: 18px;
    height: 18px;
    background: white;
    border-radius: 50%;
    transition: transform 120ms;
  }
  .toggle:checked {
    background: var(--accent);
  }
  .toggle:checked::after {
    transform: translateX(18px);
  }
  .shortcut-table > div {
    display: grid;
    grid-template-columns: 1fr 1fr 30px 1fr;
    align-items: center;
    gap: 10px;
    min-height: 42px;
    padding: 4px 12px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
  }
  .shortcut-table .table-heading {
    min-height: 34px;
    font-size: 12px;
    color: var(--text-muted);
    background: var(--surface-soft);
  }
  .key-group {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 25px;
    min-height: 25px;
    padding: 2px 6px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: var(--surface-soft);
    font: inherit;
    font-size: 12px;
  }
  .disclosures {
    display: grid;
    gap: 0;
  }
  .disclosure,
  summary {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 48px;
    width: 100%;
    padding: 10px 0;
    border: 0;
    border-bottom: 1px solid var(--border);
    color: var(--text);
    background: transparent;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
    list-style: none;
  }
  summary::-webkit-details-marker {
    display: none;
  }
  summary :global(svg:last-child) {
    margin-left: auto;
  }
  .disclosure > span {
    flex: 1;
  }
  .disclosure small {
    color: var(--text-muted);
  }
  .disclosure :global(svg) {
    transform: rotate(-90deg);
  }
  .disclosure :global(.expanded) {
    transform: rotate(0);
  }
  .custom-rules {
    display: grid;
    gap: 10px;
    padding: 14px 0;
  }
  .custom-rules p,
  .custom-rules > span {
    color: var(--text-secondary);
    font-size: 12px;
  }
  .custom-rules > div {
    display: grid;
    grid-template-columns: 1.2fr 0.7fr 1fr 32px;
    align-items: end;
    gap: 8px;
  }
  .custom-rules label {
    display: grid;
    gap: 5px;
    font-size: 12px;
    color: var(--text-secondary);
  }
  .custom-rules input {
    min-width: 0;
    min-height: 38px;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    color: var(--text);
    background: var(--surface);
  }
  .custom-rules button {
    width: 32px;
    height: 36px;
    border: 0;
    border-radius: 7px;
    color: var(--danger);
    background: var(--danger-soft);
  }
  .disclosures > .input-secondary {
    justify-self: start;
    margin-top: 12px;
  }
  @media (max-width: 980px) {
    .page-actions {
      margin-top: 0;
    }
    .setting-row > span {
      flex-wrap: wrap;
      gap: 4px 8px;
    }
  }
</style>
