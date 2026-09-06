<script lang="ts">
  import { onMount } from "svelte";
  import {
    Bell,
    CaretDown,
    FunnelSimple,
    Play,
    Scales,
    ShieldCheck,
    Sparkle,
    SpeakerHigh,
    SpeakerSlash,
    SquaresFour,
  } from "phosphor-svelte";
  import { bridge } from "../../bridge";
  import { translate as tr, language } from "../../i18n";
  import type {
    AppSettings,
    AppSettingsPatch,
    AutomationDefinition,
    NotificationPreferences,
    SoundEvent,
  } from "../../ipc/generated";
  import {
    detectReminderPreset,
    notificationPresetPatches,
    soundPresetPatches,
    type ReminderPreset,
  } from "./settingsModels";

  export let settings: AppSettings;
  export let settingsSaving = false;
  export let patchAppSettings: (patch: AppSettingsPatch, message?: string) => void | Promise<void>;
  export let testSystemNotification: () => void | Promise<void>;

  let previewing: SoundEvent | null = null;
  let error = "";
  let busy = false;
  let mutedUntil: number | null = null;
  let muteTimer: ReturnType<typeof setTimeout> | undefined;
  let automations: AutomationDefinition[] = [];
  let automationsLoaded = false;
  let volume = settings.sounds.volume;
  let currentPreset: ReminderPreset | "custom" = "custom";
  $: volume = settings.sounds.volume;
  $: currentPreset = detectReminderPreset(settings);

  const soundGroups: { label: string; events: [SoundEvent, string][] }[] = [
    { label: "剪贴板", events: [["clipboardAdded", "本机剪贴板新增"], ["clipboardReceived", "收到同步剪贴板"], ["clipboardUsed", "从剪贴板面板复制或粘贴"]] },
    { label: "文件传输", events: [["transferRequest", "收到文件传输请求"], ["transferSent", "文件发送完成"], ["transferReceived", "文件接收完成"], ["transferFailed", "文件传输失败"]] },
    { label: "快捷动作", events: [["actionStarted", "快捷动作开始"], ["actionSucceeded", "快捷动作成功"], ["actionFailed", "快捷动作失败"]] },
    { label: "自动化", events: [["automationStarted", "自动化开始"], ["automationConfirmation", "自动化等待确认"], ["automationSucceeded", "自动化成功"], ["automationFailed", "自动化失败"], ["automationInterrupted", "自动化中断"]] },
  ];

  type NotificationKey = Exclude<keyof NotificationPreferences, "enabled" | "onlyWhenInactive" | "showPreviews">;
  const notificationGroups: { label: string; events: [NotificationKey, string][] }[] = [
    { label: "需要你处理", events: [["pairingRequests", "设备配对请求"], ["transferRequests", "文件接收确认"], ["workflowActionRequired", "自动化等待确认"], ["inputPermissionRequired", "远程输入权限缺失"]] },
    { label: "传输与文件", events: [["transferCompleted", "附近传输完成"], ["transferFailed", "附近传输失败"], ["remoteFileCompleted", "远程文件操作完成"], ["remoteFileFailed", "远程文件操作失败"]] },
    { label: "任务结果", events: [["printCompleted", "打印任务完成"], ["printFailed", "打印任务异常"], ["workflowCompleted", "自动化执行完成"], ["workflowFailed", "自动化执行失败"]] },
    { label: "其他", events: [["updateAvailable", "发现应用更新"], ["agentNotifications", "Agent 与 MCP 通知"], ["deviceConnections", "设备连接与断开"]] },
  ];

  const presetOptions: { value: ReminderPreset; label: string; description: string; icon: typeof FunnelSimple }[] = [
    { value: "essential", label: "精简", description: "仅需处理", icon: FunnelSimple },
    { value: "balanced", label: "平衡", description: "需处理、失败和重要完成", icon: Scales },
    { value: "all", label: "全部", description: "所有活动", icon: SquaresFour },
  ];

  function updateMute(until: number | null) {
    clearTimeout(muteTimer);
    mutedUntil = until && until > Date.now() ? until : null;
    if (mutedUntil) muteTimer = setTimeout(() => { mutedUntil = null; }, mutedUntil - Date.now());
  }

  onMount(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void bridge.getSoundMuteUntil().then(value => { if (!disposed) updateMute(value); }).catch(e => { if (!disposed) error = String(e); });
    void bridge.onSoundMuteChanged(updateMute).then(stop => { if (disposed) stop(); else unlisten = stop; }).catch(e => { if (!disposed) error = String(e); });
    return () => { disposed = true; unlisten?.(); clearTimeout(muteTimer); };
  });

  async function preview(event: SoundEvent) {
    previewing = event;
    error = "";
    try { await bridge.previewSound(event); }
    catch (e) { error = String(e); }
    finally { previewing = null; }
  }

  async function mute() {
    busy = true;
    error = "";
    try { updateMute(await bridge.setSoundTemporaryMute(!mutedUntil)); }
    catch (e) { error = String(e); }
    finally { busy = false; }
  }

  async function applyPreset(preset: ReminderPreset) {
    busy = true;
    error = "";
    try {
      await patchAppSettings({
        notifications: notificationPresetPatches[preset],
        sounds: soundPresetPatches[preset],
      }, "提醒强度已更新");
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function reset() {
    busy = true;
    error = "";
    try {
      settings = await bridge.resetSoundPreferences();
      await patchAppSettings({
        notifications: {
          ...notificationPresetPatches.balanced,
          enabled: true,
          onlyWhenInactive: true,
          showPreviews: false,
        },
      }, "声音与通知已恢复默认");
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function loadAutomations(open: boolean) {
    if (!open || automationsLoaded) return;
    try { automations = await bridge.listAutomations(); automationsLoaded = true; }
    catch (e) { error = String(e); }
  }

  function toggleAutomation(id: string) {
    const ids = settings.sounds.mutedAutomationIds;
    void patchAppSettings({ sounds: { mutedAutomationIds: ids.includes(id) ? ids.filter(value => value !== id) : [...ids, id] } });
  }

  function toggleSound(event: SoundEvent) {
    void patchAppSettings({ sounds: { [event]: !settings.sounds[event] } });
  }

  function toggleNotification(event: NotificationKey) {
    void patchAppSettings({ notifications: { [event]: !settings.notifications[event] } });
  }

  function presetSummary() {
    if (currentPreset === "essential") return "只提醒需要处理的事项和失败结果。";
    if (currentPreset === "all") return "ArcRelay 的所有活动都会提醒你。";
    if (currentPreset === "custom") return "你已按功能调整提醒内容。";
    return "需要你处理或任务失败时提醒；重要传输完成也会通知。";
  }

  function presetTitle() {
    if (currentPreset === "essential") return "当前为精简模式";
    if (currentPreset === "all") return "当前为全部模式";
    if (currentPreset === "custom") return "当前为自定义设置";
    return "当前为平衡模式";
  }
</script>

<div class="sound-settings">
  <div class="settings-page-heading">
    <div>
      <h2>{tr("声音与通知", $language)}</h2>
      <p>{tr("让重要提醒及时到达，其余保持安静", $language)}</p>
    </div>
    <button class="secondary-button mute-button" class:active={!!mutedUntil} disabled={busy || !settings.sounds.enabled} on:click={mute}>
      <SpeakerSlash size={17} />
      {tr(mutedUntil ? "恢复提示音" : "静音 1 小时", $language)}
    </button>
  </div>

  <section class="settings-block" aria-labelledby="reminder-methods-title">
    <h3 id="reminder-methods-title">{tr("提醒方式", $language)}</h3>
    <div class="method-list">
      <div class="method-row">
        <span class="method-icon accent"><SpeakerHigh size={22} /></span>
        <span class="method-copy">
          <strong>{tr("提示音", $language)}</strong>
          <b>{tr(settings.sounds.enabled ? "已开启" : "已关闭", $language)} · {tr("音量", $language)} {volume}%</b>
          <small>{tr("操作确认与重要结果使用轻提示音", $language)}</small>
        </span>
        <div class="method-volume">
          <SpeakerHigh size={15} />
          <input aria-label={tr("提示音音量", $language)} type="range" min="0" max="100" step="1" bind:value={volume} disabled={!settings.sounds.enabled || settingsSaving || busy} on:change={() => patchAppSettings({ sounds: { volume } })} />
          <output>{volume}%</output>
        </div>
        <button class="secondary-button method-action" disabled={!bridge.isTauri() || !settings.sounds.enabled || volume === 0 || !!mutedUntil || previewing !== null || busy} on:click={() => preview("actionSucceeded")}><Play size={14} />{tr("试听", $language)}</button>
        <button class="switch-button" role="switch" aria-label={tr("启用声音反馈", $language)} aria-checked={settings.sounds.enabled} disabled={settingsSaving || busy} on:click={() => patchAppSettings({ sounds: { enabled: !settings.sounds.enabled } })}><span class:checked={settings.sounds.enabled} class="switch-control"><span></span></span></button>
      </div>
      <div class="method-row">
        <span class="method-icon accent"><Bell size={22} /></span>
        <span class="method-copy">
          <strong>{tr("桌面通知", $language)}</strong>
          <b>{tr(settings.notifications.enabled ? "已开启" : "已关闭", $language)} · {tr(settings.notifications.onlyWhenInactive ? "仅在后台显示" : "始终显示", $language)}</b>
          <small>{tr("需要处理的事项会通过系统通知提醒你", $language)}</small>
        </span>
        <span class="method-spacer"></span>
        <button class="secondary-button method-action" disabled={settingsSaving || !settings.notifications.enabled} on:click={testSystemNotification}><Bell size={14} />{tr("发送测试", $language)}</button>
        <button class="switch-button" role="switch" aria-label={tr("允许桌面通知", $language)} aria-checked={settings.notifications.enabled} disabled={settingsSaving} on:click={() => patchAppSettings({ notifications: { enabled: !settings.notifications.enabled } })}><span class:checked={settings.notifications.enabled} class="switch-control"><span></span></span></button>
      </div>
    </div>
  </section>

  <section class="settings-block" aria-labelledby="reminder-intensity-title">
    <h3 id="reminder-intensity-title">{tr("提醒强度", $language)}</h3>
    <div class="preset-selector">
      {#each presetOptions as option}
        <button class:selected={currentPreset === option.value} aria-pressed={currentPreset === option.value} disabled={busy || settingsSaving} on:click={() => applyPreset(option.value)}>
          <svelte:component this={option.icon} size={22} />
          <span><strong>{tr(option.label, $language)}{#if option.value === "balanced"}<em>{tr("推荐", $language)}</em>{/if}</strong><small>{tr(option.description, $language)}</small></span>
        </button>
      {/each}
    </div>
    <div class="preset-summary"><Sparkle size={22} /><span><strong>{tr(presetTitle(), $language)}</strong><small>{tr(presetSummary(), $language)}</small></span></div>
  </section>

  <section class="settings-block" aria-labelledby="quiet-title">
    <h3 id="quiet-title">{tr("隐私与安静", $language)}</h3>
    <div class="quiet-list">
      <button role="switch" aria-checked={!settings.notifications.showPreviews} disabled={settingsSaving || !settings.notifications.enabled} on:click={() => patchAppSettings({ notifications: { showPreviews: !settings.notifications.showPreviews } })}>
        <span class="quiet-icon"><ShieldCheck size={21} /></span>
        <span><strong>{tr("锁屏时隐藏通知详情", $language)}</strong><small>{tr("不显示设备名、文件名和错误信息", $language)}</small></span>
        <span class:checked={!settings.notifications.showPreviews} class="switch-control"><span></span></span>
      </button>
      <button role="switch" aria-checked={settings.sounds.muteDuringPrivacy} disabled={settingsSaving || !settings.sounds.enabled} on:click={() => patchAppSettings({ sounds: { muteDuringPrivacy: !settings.sounds.muteDuringPrivacy } })}>
        <span class="quiet-icon"><SpeakerSlash size={21} /></span>
        <span><strong>{tr("投屏隐私时自动静音", $language)}</strong><small>{tr("暂停提示音，桌面通知仍按隐私规则显示", $language)}</small></span>
        <span class:checked={settings.sounds.muteDuringPrivacy} class="switch-control"><span></span></span>
      </button>
    </div>
  </section>

  <details class="custom-reminders" on:toggle={(event) => loadAutomations(event.currentTarget.open)}>
    <summary><CaretDown size={18} /><span><strong>{tr("自定义提醒内容", $language)}</strong><small>{tr("按功能调整声音与桌面通知", $language)}</small></span><span class="summary-action">{tr("展开", $language)}</span></summary>
    <div class="custom-reminders-body">
      <div class="custom-display-row">
        <span><strong>{tr("通知出现时机", $language)}</strong><small>{tr("ArcRelay 在前台时可用页面状态代替系统通知。", $language)}</small></span>
        <button class="segmented-toggle" disabled={!settings.notifications.enabled || settingsSaving} on:click={() => patchAppSettings({ notifications: { onlyWhenInactive: !settings.notifications.onlyWhenInactive } })}>{tr(settings.notifications.onlyWhenInactive ? "仅在后台" : "始终", $language)}</button>
      </div>

      <h4>{tr("声音提醒", $language)}</h4>
      <div class="custom-grid">
        {#each soundGroups as group}
          <div class="custom-group">
            <strong>{tr(group.label, $language)}</strong>
            {#each group.events as [event, label]}
              <div class="custom-event">
                <button role="switch" aria-checked={settings.sounds[event]} disabled={!settings.sounds.enabled || settingsSaving || busy} on:click={() => toggleSound(event)}><span>{tr(label, $language)}</span><span class:checked={settings.sounds[event]} class="switch-control"><span></span></span></button>
                <button class="preview-icon" aria-label={`${tr("试听", $language)} ${tr(label, $language)}`} disabled={!bridge.isTauri() || !settings.sounds.enabled || settings.sounds.volume === 0 || !!mutedUntil || previewing !== null || busy} on:click={() => preview(event)}><Play size={13} /></button>
              </div>
            {/each}
          </div>
        {/each}
      </div>

      <h4>{tr("桌面通知", $language)}</h4>
      <div class="custom-grid">
        {#each notificationGroups as group}
          <div class="custom-group">
            <strong>{tr(group.label, $language)}</strong>
            {#each group.events as [event, label]}
              <button class="custom-notification" role="switch" aria-checked={settings.notifications[event]} disabled={!settings.notifications.enabled || settingsSaving} on:click={() => toggleNotification(event)}><span>{tr(label, $language)}</span><span class:checked={settings.notifications[event]} class="switch-control"><span></span></span></button>
            {/each}
          </div>
        {/each}
      </div>

      <details class="automation-muted-list">
        <summary>{tr("为指定自动化静音", $language)}</summary>
        <p>{tr("只影响这台设备上的提示音；任务和桌面通知照常运行。", $language)}</p>
        {#if automationsLoaded && automations.length === 0}<p>{tr("暂无自动化", $language)}</p>{/if}
        {#each automations as automation}
          <label><input type="checkbox" checked={settings.sounds.mutedAutomationIds.includes(automation.id)} disabled={settingsSaving || busy} on:change={() => toggleAutomation(automation.id)} />{automation.name}</label>
        {/each}
      </details>
    </div>
  </details>

  {#if error}<p class="settings-error" role="alert">{error}</p>{/if}
  <button class="reset-button" disabled={settingsSaving || busy} on:click={reset}>{tr("恢复默认设置", $language)}</button>
</div>

<style>
  .sound-settings { display: grid; gap: 0; padding: 26px 0 34px; }
  .settings-page-heading { display: flex; align-items: flex-start; justify-content: space-between; gap: 24px; margin-bottom: 24px; }
  .settings-page-heading h2 { margin: 0; color: var(--text); font-size: 23px; font-weight: 680; letter-spacing: -.02em; }
  .settings-page-heading p { margin: 7px 0 0; color: var(--text-muted); font-size: 13px; }
  .mute-button { height: 38px; }
  .mute-button.active { border-color: color-mix(in srgb, var(--accent) 40%, var(--border)); color: var(--accent-strong); background: var(--accent-soft); }
  .settings-block { display: grid; gap: 10px; margin-bottom: 22px; }
  .settings-block h3 { margin: 0; color: var(--text); font-size: 14px; font-weight: 660; }
  .method-list, .quiet-list, .custom-reminders { overflow: hidden; border: 1px solid var(--border-strong); border-radius: 12px; background: var(--surface-raised); }
  .method-row { display: grid; grid-template-columns: 48px minmax(210px, 1fr) minmax(210px, 330px) auto 48px; align-items: center; gap: 16px; min-height: 94px; padding: 12px 16px; border-bottom: 1px solid var(--border); }
  .method-row:last-child { border-bottom: 0; }
  .method-icon { display: grid; place-items: center; width: 46px; height: 46px; border-radius: 11px; color: var(--text-muted); background: var(--surface-soft); }
  .method-icon.accent { color: var(--accent); background: var(--accent-soft); }
  .method-copy { display: grid; min-width: 0; }
  .method-copy strong { color: var(--text); font-size: 14px; font-weight: 650; }
  .method-copy b { margin-top: 4px; color: #278a50; font-size: 12px; font-weight: 630; }
  .method-copy small { margin-top: 4px; color: var(--text-muted); font-size: 11px; line-height: 1.45; }
  .method-volume { display: flex; align-items: center; gap: 11px; color: var(--text-muted); }
  .method-volume input { width: 100%; accent-color: var(--accent); }
  .method-volume output { min-width: 3.2em; color: var(--text-secondary); font-size: 12px; font-variant-numeric: tabular-nums; text-align: right; }
  .method-action { height: 36px; padding: 0 13px; font-size: 12px; white-space: nowrap; }
  .method-spacer { min-width: 0; }
  .switch-button { display: grid; place-items: center; width: 48px; height: 42px; padding: 0; border: 0; background: transparent; cursor: pointer; }
  .switch-button:disabled { cursor: default; opacity: .55; }
  .preset-selector { display: grid; grid-template-columns: repeat(3, 1fr); overflow: hidden; border: 1px solid var(--border-strong); border-radius: 12px; background: var(--surface-raised); }
  .preset-selector > button { display: flex; align-items: center; justify-content: center; gap: 14px; min-height: 82px; padding: 13px 20px; border: 0; border-right: 1px solid var(--border); color: var(--text-secondary); background: transparent; text-align: left; cursor: pointer; }
  .preset-selector > button:last-child { border-right: 0; }
  .preset-selector > button:hover { background: var(--surface-hover); }
  .preset-selector > button.selected { position: relative; color: var(--accent-strong); background: var(--surface-accent); box-shadow: inset 0 0 0 1.5px var(--accent); }
  .preset-selector span { display: grid; gap: 4px; }
  .preset-selector strong { display: flex; align-items: center; gap: 7px; color: inherit; font-size: 14px; }
  .preset-selector small { color: var(--text-muted); font-size: 11px; }
  .preset-selector em { padding: 2px 6px; border-radius: 999px; color: var(--accent-strong); background: var(--accent-soft); font-size: 9px; font-style: normal; }
  .preset-summary { display: flex; align-items: center; gap: 14px; min-height: 62px; padding: 10px 16px; border: 1px solid color-mix(in srgb, var(--accent) 18%, var(--border)); border-radius: 11px; color: var(--accent); background: var(--surface-accent); }
  .preset-summary span { display: grid; gap: 4px; }
  .preset-summary strong { color: var(--accent-strong); font-size: 13px; }
  .preset-summary small { color: var(--text-secondary); font-size: 11px; }
  .quiet-list > button { display: grid; grid-template-columns: 48px minmax(0, 1fr) auto; align-items: center; gap: 14px; width: 100%; min-height: 72px; padding: 10px 16px; border: 0; border-bottom: 1px solid var(--border); color: inherit; background: transparent; text-align: left; cursor: pointer; }
  .quiet-list > button:last-child { border-bottom: 0; }
  .quiet-list > button:hover { background: var(--surface-hover); }
  .quiet-list > button:disabled { cursor: default; opacity: .55; }
  .quiet-list > button > span:nth-child(2) { display: grid; gap: 4px; }
  .quiet-list strong { font-size: 13px; }
  .quiet-list small { color: var(--text-muted); font-size: 11px; }
  .quiet-icon { display: grid; place-items: center; width: 40px; height: 40px; border-radius: 10px; color: var(--text-secondary); background: var(--surface-soft); }
  .custom-reminders { margin-top: 2px; }
  .custom-reminders > summary { display: grid; grid-template-columns: 22px minmax(0, 1fr) auto; align-items: center; gap: 12px; min-height: 70px; padding: 9px 16px; cursor: pointer; list-style: none; }
  .custom-reminders > summary::-webkit-details-marker { display: none; }
  .custom-reminders[open] > summary { border-bottom: 1px solid var(--border); }
  .custom-reminders[open] > summary :global(svg) { transform: rotate(180deg); }
  .custom-reminders > summary > span:nth-child(2) { display: grid; gap: 4px; }
  .custom-reminders > summary strong { color: var(--text); font-size: 13px; }
  .custom-reminders > summary small { color: var(--text-muted); font-size: 11px; }
  .summary-action { color: var(--accent-strong); font-size: 11px; }
  .custom-reminders-body { display: grid; gap: 20px; padding: 20px; background: var(--surface-soft); }
  .custom-display-row { display: flex; align-items: center; justify-content: space-between; gap: 20px; }
  .custom-display-row > span { display: grid; gap: 4px; }
  .custom-display-row strong { font-size: 12px; }
  .custom-display-row small { color: var(--text-muted); font-size: 11px; }
  .segmented-toggle { min-height: 34px; padding: 0 12px; border: 1px solid var(--border); border-radius: 8px; color: var(--accent-strong); background: var(--surface-raised); font-size: 11px; cursor: pointer; }
  .custom-reminders-body h4 { margin: 0 0 -10px; font-size: 12px; }
  .custom-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  .custom-group { overflow: hidden; border: 1px solid var(--border); border-radius: 10px; background: var(--surface-raised); }
  .custom-group > strong { display: block; padding: 10px 12px; border-bottom: 1px solid var(--border); color: var(--text-secondary); font-size: 11px; }
  .custom-event { display: grid; grid-template-columns: minmax(0, 1fr) 34px; align-items: center; border-bottom: 1px solid var(--border); }
  .custom-event:last-child { border-bottom: 0; }
  .custom-event > button:first-child, .custom-notification { display: flex; align-items: center; justify-content: space-between; gap: 12px; width: 100%; min-height: 46px; padding: 7px 12px; border: 0; color: var(--text-secondary); background: transparent; font-size: 11px; text-align: left; cursor: pointer; }
  .custom-event > button:first-child:disabled, .custom-notification:disabled, .preview-icon:disabled { cursor: default; opacity: .5; }
  .custom-group .switch-control { transform: scale(.82); transform-origin: right center; }
  .preview-icon { display: grid; place-items: center; width: 30px; height: 30px; padding: 0; border: 0; border-radius: 7px; color: var(--text-muted); background: transparent; cursor: pointer; }
  .preview-icon:hover { color: var(--accent); background: var(--accent-soft); }
  .custom-notification { border-bottom: 1px solid var(--border); }
  .custom-notification:last-child { border-bottom: 0; }
  .automation-muted-list { padding-top: 4px; border-top: 1px solid var(--border); }
  .automation-muted-list summary { cursor: pointer; color: var(--text-secondary); font-size: 12px; }
  .automation-muted-list p { color: var(--text-muted); font-size: 11px; }
  .automation-muted-list label { display: flex; align-items: center; gap: 9px; margin-top: 9px; font-size: 11px; }
  .automation-muted-list input { accent-color: var(--accent); }
  .settings-error { margin: 12px 0 0; color: var(--danger); font-size: 12px; }
  .reset-button { justify-self: start; margin-top: 16px; padding: 7px 0; border: 0; color: var(--accent-strong); background: transparent; font: inherit; font-size: 12px; cursor: pointer; }
  .reset-button:hover { text-decoration: underline; }
  @media (max-width: 900px) {
    .method-row { grid-template-columns: 48px minmax(0, 1fr) auto 48px; }
    .method-volume, .method-spacer { grid-column: 2 / 3; }
    .method-action { grid-column: 3 / 4; grid-row: 1; }
    .switch-button { grid-column: 4 / 5; grid-row: 1; }
  }
  @media (max-width: 660px) {
    .settings-page-heading { align-items: stretch; flex-direction: column; }
    .mute-button { align-self: flex-start; }
    .preset-selector { grid-template-columns: 1fr; }
    .preset-selector > button { justify-content: flex-start; border-right: 0; border-bottom: 1px solid var(--border); }
    .preset-selector > button:last-child { border-bottom: 0; }
    .custom-grid { grid-template-columns: 1fr; }
    .method-row { grid-template-columns: 44px minmax(0, 1fr) auto; gap: 10px; }
    .method-volume, .method-spacer { grid-column: 2 / -1; }
    .method-action { grid-column: 2 / 3; grid-row: 3; justify-self: start; }
    .switch-button { grid-column: 3 / 4; grid-row: 1 / 3; }
  }
</style>
