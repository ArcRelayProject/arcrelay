<script lang="ts">
  import AppSelect from "../../components/AppSelect.svelte";
  import { translate as uiTranslate, language as uiLanguage } from "../../i18n";
  import {
    Briefcase,
    CaretDown,
    CheckCircle,
    Code,
    DesktopTower,
    FloppyDisk,
    GameController,
    Keyboard,
    Laptop,
    Plus,
    Presentation,
    TerminalWindow,
    Trash,
  } from "phosphor-svelte";
  import type { InputOsFamily, KeyboardProfile, RuntimeSnapshot, WorkspaceConfiguration } from "../../types";
  import { bridge } from "../../bridge";
  import { DEFAULT_KEYBOARD_PROFILES, friendlyDeviceName } from "./inputSharingUi";
  import KeyboardTester from "./keyboard/KeyboardTester.svelte";
  import ConsumerKeysSection from "./ConsumerKeysSection.svelte";

  export let snapshot: RuntimeSnapshot;
  export let onSnapshot: (snapshot: RuntimeSnapshot) => void;
  export let notify: (message: string, error?: boolean) => void;

  let configuration: WorkspaceConfiguration = structuredClone(snapshot.configuration);
  if (configuration.keyboardProfiles.length === 0) configuration.keyboardProfiles = structuredClone(DEFAULT_KEYBOARD_PROFILES);
  let selectedKind: KeyboardProfile["kind"] = configuration.activeKeyboardProfile ?? "Productivity";
  let dirty = false;
  let showAdvanced = false;

  const profileIcons = { Productivity: Briefcase, Terminal: TerminalWindow, Ide: Code, RemoteDesktop: DesktopTower, GameRaw: GameController, Presentation };
  function osLabel(system: InputOsFamily | undefined) {
    if (system === "MacOs") return "macOS";
    if (system === "Windows") return "Windows";
    if (system === "Android") return "Android";
    if (system === "Ios") return "iOS / iPadOS";
    if (system === "LinuxX11") return "Linux · X11";
    if (system === "LinuxWayland") return "Linux · Wayland";
    return "等待设备信息";
  }
  function primaryModifier(system: string) { return system === "macOS" ? "⌘" : "Ctrl"; }
  function applicationSwitchModifier(system: string) { return system === "macOS" ? "⌘" : "Alt"; }
  $: presetRows = [
    { action: "复制", source: [primaryModifier(sourceSystem), "C"], target: [primaryModifier(targetSystem), "C"], status: "已适配" },
    { action: "粘贴", source: [primaryModifier(sourceSystem), "V"], target: [primaryModifier(targetSystem), "V"], status: "已适配" },
    { action: "撤销", source: [primaryModifier(sourceSystem), "Z"], target: [primaryModifier(targetSystem), "Z"], status: "已适配" },
    { action: "切换应用", source: [applicationSwitchModifier(sourceSystem), "Tab"], target: [applicationSwitchModifier(targetSystem), "Tab"], status: "已适配" },
  ];

  $: profile = configuration.keyboardProfiles.find((value) => value.kind === selectedKind) ?? configuration.keyboardProfiles[0];
  $: remoteDeviceId = Object.values(configuration.layout?.displays ?? {}).find((display) => display.deviceId !== snapshot.serviceInstanceId)?.deviceId;
  $: sourceSystem = osLabel(snapshot.localOperatingSystem);
  $: targetSystem = osLabel(remoteDeviceId ? snapshot.remoteOperatingSystems[remoteDeviceId] : undefined);
  $: targetCapabilities = remoteDeviceId ? snapshot.nearbyPeers.find((peer) => peer.serviceInstanceId === remoteDeviceId)?.capabilities : undefined;
  $: appPointerOnly = targetCapabilities?.canInjectAppPointer && !targetCapabilities.canInjectKeyboard;

  async function save() {
    try {
      const next = await bridge.saveWorkspace(configuration);
      onSnapshot(next);
      configuration = structuredClone(next.configuration);
      dirty = false;
      notify("键盘行为已保存");
    } catch (error) {
      notify(String(error), true);
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
    notify("已恢复推荐键盘行为，保存后生效");
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

<section class="page">
  <div class="page-actions"><button class="secondary-action" on:click={restoreDefaults}>{uiTranslate("恢复默认", $uiLanguage)}</button><button class="save-action" disabled={!dirty} on:click={save}><FloppyDisk size={17} />{uiTranslate("保存更改", $uiLanguage)}</button></div>
  <div class="keyboard-layout">
    <aside class="profile-column">
      <div class="profile-list card">
        <h3>{uiTranslate("使用场景", $uiLanguage)}</h3>
        {#each configuration.keyboardProfiles as item}
          {@const Icon = profileIcons[item.kind]}
          <button class:active={profile?.kind === item.kind} on:click={() => selectProfile(item.kind)}>
            <span><Icon size={18} weight={profile?.kind === item.kind ? "fill" : "regular"} /></span>
            <strong>{item.name}</strong>
            {#if item.kind === "Productivity"}<em>{uiTranslate("推荐", $uiLanguage)}</em>{/if}
            <i></i>
          </button>
        {/each}
      </div>
      <div class="device-pair card">
        <h3>{uiTranslate("当前设备对", $uiLanguage)}</h3>
        <div><span><Laptop size={20} /></span><p><strong>{uiTranslate("这台 Mac", $uiLanguage)}</strong><small>{sourceSystem} {uiTranslate("· 来源", $uiLanguage)}</small></p></div>
        <i>↓</i>
        <div><span><DesktopTower size={20} /></span><p><strong>{uiTranslate(remoteDeviceId ? friendlyDeviceName(remoteDeviceId, snapshot) : "目标设备", $uiLanguage)}</strong><small>{targetSystem} {uiTranslate("· 目标", $uiLanguage)}</small></p></div>
        <label class="device-profile"><Briefcase size={16} /><AppSelect bind:value={selectedKind} onValueChange={() => selectProfile(selectedKind)} aria-label={uiTranslate("当前设备对键盘配置", $uiLanguage)}
          options={[
            ...configuration.keyboardProfiles.map((item) => ({ value: item.kind, label: item.name })),
          ]}
        /></label>
      </div>
    </aside>

    {#if appPointerOnly}
      <div class="editor card"><div class="editor-heading"><div><h2>{targetSystem}</h2><p>{uiTranslate("仅应用内鼠标；键盘输入返回电脑", $uiLanguage)}</p></div></div></div>
    {:else if profile}
      <div class="editor card">
        <div class="editor-heading"><div><h2>{profile.name}</h2><p>{uiTranslate("复制、粘贴、撤销和切换应用会自动适配目标系统。", $uiLanguage)}</p></div><span><CheckCircle size={17} weight="fill" />{uiTranslate("正在使用", $uiLanguage)}</span></div>

        <div class="system-pair">
          <div><span>{uiTranslate("来源键盘", $uiLanguage)}</span><strong class="os-value">{sourceSystem}</strong></div>
          <strong>→</strong>
          <div><span>{uiTranslate("目标系统", $uiLanguage)}</span><strong class="os-value">{targetSystem}</strong></div>
        </div>

        <section class="shortcut-section">
          <h3>{uiTranslate("常用快捷键", $uiLanguage)}</h3>
          <div class="shortcut-table">
            <header><span>{uiTranslate("操作", $uiLanguage)}</span><span>{uiTranslate("来源", $uiLanguage)}</span><span>{uiTranslate("目标", $uiLanguage)}</span><span>{uiTranslate("状态", $uiLanguage)}</span></header>
            {#each presetRows as row}
              <div><strong>{row.action}</strong><span class="key-group">{#each row.source as key}<kbd>{key}</kbd>{/each}</span><span class="key-group">{#each row.target as key}<kbd class:text={key.length > 3}>{key}</kbd>{/each}</span><em class:local={row.status === "本机"}><i></i>{row.status}</em></div>
            {/each}
          </div>
          <button class="add-rule" on:click={addRule}><Plus size={16} />{uiTranslate("添加自定义规则", $uiLanguage)}</button>
        </section>

        <ConsumerKeysSection {snapshot} bind:configuration onChange={update} />

        <section class="text-section">
          <h3>{uiTranslate("文字输入", $uiLanguage)}</h3>
          <div class="strategy-grid">
            <button class:active={profile.textStrategy === "UseTargetLayout"} on:click={() => { profile.textStrategy = "UseTargetLayout"; update(); }}><i></i><span><strong>{uiTranslate("使用目标键盘布局", $uiLanguage)}</strong><small>{uiTranslate("按目标系统的键盘布局输入文字", $uiLanguage)}</small></span></button>
            <button class:active={profile.textStrategy === "FollowSourceText"} on:click={() => { profile.textStrategy = "FollowSourceText"; update(); }}><i></i><span><strong>{uiTranslate("保持来源文字", $uiLanguage)}</strong><small>{uiTranslate("尽量保持来源键盘输入的原始字符", $uiLanguage)}</small></span></button>
          </div>
          <label class="toggle-row"><span><strong>{uiTranslate("按住右 Option 临时直通原始按键", $uiLanguage)}</strong><small>{uiTranslate("适合终端、游戏或远程桌面；保存后由输入引擎执行", $uiLanguage)}</small></span><input type="checkbox" bind:checked={configuration.rightOptionRawMode} on:change={update} /><i></i></label>
        </section>

        <section class="gesture-section">
          <h3>{uiTranslate("触控板横向手势", $uiLanguage)}</h3>
          <div class="strategy-grid">
            <button class:active={configuration.horizontalScrollBehavior === "NativeScroll"} on:click={() => { configuration.horizontalScrollBehavior = "NativeScroll"; update(); }}><i></i><span><strong>{uiTranslate("原生横向滚动", $uiLanguage)}</strong><small>{uiTranslate("在表格、时间线等内容中保持横向滚动", $uiLanguage)}</small></span></button>
            <button class:active={configuration.horizontalScrollBehavior === "NavigateHistory"} on:click={() => { configuration.horizontalScrollBehavior = "NavigateHistory"; update(); }}><i></i><span><strong>{uiTranslate("返回与前进", $uiLanguage)}</strong><small>{uiTranslate("连接 Windows 时，双指右滑返回、左滑前进", $uiLanguage)}</small></span></button>
          </div>
        </section>

        <button class="advanced-row" on:click={() => showAdvanced = !showAdvanced}>{uiTranslate("高级规则与优先级", $uiLanguage)}<CaretDown class={showAdvanced ? "expanded" : ""} size={17} /></button>
        {#if showAdvanced}
          <div class="custom-rules">
            <p>{uiTranslate("用户规则优先于系统预设；一个组合键会在首次按下后锁定到松开。", $uiLanguage)}</p>
            {#each profile.semanticOverrides as rule, index}
              <div><label>{uiTranslate("修饰键", $uiLanguage)}<AppSelect multiple bind:value={rule.source.modifiers} onValueChange={update} aria-label={uiTranslate("修饰键", $uiLanguage)}
                options={[
                  { value: 224, label: "Ctrl" },
                  { value: 225, label: "Shift" },
                  { value: 226, label: "Option" },
                  { value: 227, label: "Command" },
                ]}
              /></label><label>{uiTranslate("主键", $uiLanguage)}<input type="number" min="4" max="231" bind:value={rule.source.key} on:change={update} /></label><label>{uiTranslate("语义", $uiLanguage)}<AppSelect bind:value={rule.action} onValueChange={update} aria-label={uiTranslate("语义", $uiLanguage)}
                options={[
                  { value: "Copy", label: uiTranslate("复制", $uiLanguage) },
                  { value: "Paste", label: uiTranslate("粘贴", $uiLanguage) },
                  { value: "Cut", label: uiTranslate("剪切", $uiLanguage) },
                  { value: "Undo", label: uiTranslate("撤销", $uiLanguage) },
                  { value: "Redo", label: uiTranslate("重做", $uiLanguage) },
                  { value: "SelectAll", label: uiTranslate("全选", $uiLanguage) },
                  { value: "ApplicationSwitch", label: uiTranslate("切换应用", $uiLanguage) },
                ]}
              /></label><button aria-label={uiTranslate("删除规则", $uiLanguage)} on:click={() => deleteRule(index)}><Trash size={16} /></button></div>
            {/each}
            {#if profile.semanticOverrides.length === 0}<span>{uiTranslate("尚无自定义规则。", $uiLanguage)}</span>{/if}
          </div>
        {/if}

        <KeyboardTester />
      </div>
    {/if}
  </div>
</section>

<style>
  .page { display: flex; min-height: 0; height: 100%; flex-direction: column; gap: 14px; padding-bottom: 24px; }.page-actions { display: flex; justify-content: flex-end; gap: 8px; }.secondary-action, .save-action { display: inline-flex; min-height: 39px; align-items: center; gap: 7px; border-radius: 9px; padding: 0 14px; font-weight: 650; }.secondary-action { border: 1px solid var(--border-strong); color: var(--text); background: var(--surface); }.save-action { border: 1px solid var(--accent); color: #fff; background: var(--accent); }.save-action:disabled { border-color: var(--border-strong); color: var(--text-muted); background: var(--surface-sunken); }
  .keyboard-layout { min-height: 0; display: grid; grid-template-columns: 245px minmax(0,1fr); gap: 14px; }.profile-column { display: grid; align-content: start; gap: 14px; }.profile-list, .device-pair { padding: 14px; }.profile-list h3, .device-pair h3 { margin: 4px 6px 12px; font-size: 14px; }.profile-list > button { display: grid; grid-template-columns: 34px minmax(0,1fr) auto 15px; align-items: center; gap: 8px; width: 100%; min-height: 50px; border: 0; border-radius: 10px; padding: 0 9px; color: var(--text); background: transparent; text-align: left; }.profile-list > button.active { color: var(--accent-strong); background: var(--accent-soft); }.profile-list > button > span { display: grid; width: 30px; height: 30px; place-items: center; border-radius: 8px; color: var(--accent); background: color-mix(in srgb,var(--accent) 10%,var(--surface)); }.profile-list strong { font-size: 12px; }.profile-list em { padding: 3px 6px; border-radius: 6px; color: var(--accent-strong); background: color-mix(in srgb,var(--accent) 13%,var(--surface)); font-size: 9px; font-style: normal; }.profile-list > button > i { width: 14px; height: 14px; border: 1px solid var(--border-strong); border-radius: 50%; }.profile-list > button.active > i { border: 4px solid var(--accent); }
  .device-pair > div { display: grid; grid-template-columns: 36px minmax(0,1fr); align-items: center; gap: 8px; padding: 8px; }.device-pair > div > span { display: grid; width: 34px; height: 34px; place-items: center; border-radius: 8px; color: var(--accent); background: var(--accent-soft); }.device-pair p { margin: 0; }.device-pair strong, .device-pair small { display: block; }.device-pair strong { font-size: 11px; }.device-pair small { margin-top: 3px; color: var(--text-muted); font-size: 9px; }.device-pair > i { display: block; margin-left: 25px; color: var(--text-muted); font-style: normal; }.device-profile { display: grid; grid-template-columns: 20px minmax(0, 1fr); align-items: center; gap: 7px; width: 100%; min-height: 40px; margin-top: 8px; border: 1px solid var(--border); border-radius: 9px; padding: 0 10px; color: var(--text); background: var(--surface-soft); font-size: 11px; }.device-profile :global(.app-select-trigger) { min-width: 0; width: 100%; border: 0; color: var(--text); background: transparent; }
  .editor { min-height: 0; padding: 22px; overflow: auto; }.editor-heading { display: flex; align-items: flex-start; justify-content: space-between; gap: 14px; }.editor-heading h2 { margin: 0 0 5px; font-size: 20px; }.editor-heading p { margin: 0; color: var(--text-secondary); font-size: 12px; }.editor-heading > span { display: flex; align-items: center; gap: 5px; color: var(--success); font-size: 10px; }
  .system-pair { display: grid; grid-template-columns: 1fr 30px 1fr; align-items: end; gap: 12px; margin: 20px 0; }.system-pair > div { display: grid; grid-template-columns: auto 1fr; align-items: center; gap: 12px; min-height: 50px; padding: 0 12px; border: 1px solid var(--border); border-radius: 10px; color: var(--text-muted); font-size: 11px; }.system-pair .os-value { justify-self: end; color: var(--text); font-size: 12px; }.system-pair > strong { padding-bottom: 16px; color: var(--text-muted); text-align: center; }
  .shortcut-section, .text-section, .gesture-section { display: grid; gap: 10px; }.shortcut-section h3, .text-section h3, .gesture-section h3 { margin: 0; font-size: 14px; }.shortcut-table { overflow: hidden; border: 1px solid var(--border); border-radius: 11px; }.shortcut-table header, .shortcut-table > div { display: grid; grid-template-columns: .8fr 1fr 1fr .6fr; align-items: center; min-height: 42px; padding: 0 13px; gap: 10px; }.shortcut-table header { color: var(--text-muted); background: var(--surface-soft); font-size: 9px; font-weight: 700; }.shortcut-table > div { border-top: 1px solid var(--border); font-size: 11px; }.key-group { display: flex; align-items: center; gap: 4px; }.key-group kbd { display: grid; min-width: 27px; height: 26px; place-items: center; padding: 0 6px; border: 1px solid var(--border-strong); border-radius: 6px; background: var(--surface); box-shadow: 0 1px 0 var(--border-strong); font-family: inherit; font-size: 10px; }.key-group kbd.text { min-width: 80px; box-shadow: none; }.shortcut-table em { display: flex; align-items: center; gap: 6px; color: var(--success); font-size: 10px; font-style: normal; }.shortcut-table em i { width: 7px; height: 7px; border-radius: 50%; background: currentColor; }.shortcut-table em.local { color: var(--text-muted); }.add-rule { display: flex; width: fit-content; min-height: 34px; align-items: center; gap: 6px; border: 1px solid var(--accent); border-radius: 8px; padding: 0 10px; color: var(--accent-strong); background: var(--surface); font-size: 10px; font-weight: 650; }
  .text-section, .gesture-section { margin-top: 20px; }.strategy-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }.strategy-grid button { display: grid; grid-template-columns: 16px minmax(0,1fr); gap: 9px; min-height: 64px; align-items: center; border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px; color: var(--text); background: var(--surface); text-align: left; }.strategy-grid button.active { border-color: var(--accent); background: var(--accent-soft); }.strategy-grid button > i { width: 14px; height: 14px; border: 1px solid var(--border-strong); border-radius: 50%; }.strategy-grid button.active > i { border: 4px solid var(--accent); }.strategy-grid span { display: grid; gap: 3px; }.strategy-grid strong { font-size: 11px; }.strategy-grid small { color: var(--text-muted); font-size: 9px; }.toggle-row { display: grid; grid-template-columns: 1fr 42px; align-items: center; gap: 12px; }.toggle-row > span { display: grid; gap: 3px; }.toggle-row strong { font-size: 11px; }.toggle-row small { color: var(--text-muted); font-size: 9px; }.toggle-row input { position: absolute; opacity: 0; }.toggle-row > i { position: relative; width: 40px; height: 23px; border-radius: 999px; background: var(--border-strong); }.toggle-row > i::after { position: absolute; top: 3px; left: 3px; width: 17px; height: 17px; border-radius: 50%; background: #fff; content: ""; transition: 120ms ease; }.toggle-row input:checked + i { background: var(--accent); }.toggle-row input:checked + i::after { transform: translateX(17px); }
  .advanced-row { display: flex; width: 100%; min-height: 42px; align-items: center; justify-content: space-between; margin-top: 15px; border: 1px solid var(--border); border-radius: 9px; padding: 0 11px; color: var(--text-secondary); background: var(--surface-soft); }.advanced-row :global(svg) { transition: transform 120ms ease; }.advanced-row :global(svg.expanded) { transform: rotate(180deg); }.custom-rules { display: grid; gap: 9px; padding: 12px; border-radius: 10px; background: var(--surface-soft); }.custom-rules > p, .custom-rules > span { margin: 0; color: var(--text-muted); font-size: 10px; }.custom-rules > div { display: grid; grid-template-columns: 1.2fr .7fr 1fr 32px; align-items: end; gap: 7px; }.custom-rules label { display: grid; gap: 4px; color: var(--text-muted); font-size: 9px; }.custom-rules input, .custom-rules :global(.app-select-trigger) { min-width: 0; min-height: 34px; border: 1px solid var(--border); border-radius: 7px; color: var(--text); background: var(--surface); }.custom-rules button { display: grid; width: 32px; height: 32px; place-items: center; border: 0; border-radius: 7px; color: var(--danger); background: var(--danger-soft); }
  :global(.editor .tester) { min-height: 110px; margin-top: 16px; }
  @media (max-width: 980px) { .keyboard-layout { grid-template-columns: 1fr; }.profile-column { grid-template-columns: 1fr 1fr; }.profile-list { max-height: 280px; overflow: auto; } }
</style>
