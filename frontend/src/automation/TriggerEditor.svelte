<script lang="ts">
  import AppSelect from "../components/AppSelect.svelte";
  import { weekdayName } from "../automation";
  import { translate as uiTranslate, language as uiLanguage } from "../i18n";
  import { onDestroy } from "svelte";
  import {
    AppWindow,
    Desktop,
    Clock,
    Devices,
    Keyboard,
    MagnifyingGlass,
    CheckCircle,
    Info,
    Eye,
  } from "phosphor-svelte";
  import { bridge } from "../bridge";
  import InstalledAppIcon from "../InstalledAppIcon.svelte";
  import {
    identity,
    triggerChoices,
    applicationLabels,
    weekdays,
    dayNames,
    formatDate,
    presenceLabels,
  } from "../automation";
  import { captureActionShortcut } from "../actionShortcutCapture";
  import { errorMessage } from "../app_helpers";
  import type {
    AutomationTrigger,
    Capability,
    ConnectedDeviceView,
  } from "../ipc/generated";
  import type { InstalledApp } from "../types";
  export let trigger: AutomationTrigger;
  export let apps: InstalledApp[];
  export let devices: ConnectedDeviceView[];
  export let capabilities: Capability[];
  let search = "";
  let next: string | null = null;
  let previewError = "";
  let shortcutError = "";
  let request = 0;
  let timer: ReturnType<typeof setTimeout>;
  const categories = [
    { name: "应用", icon: AppWindow },
    { name: "系统", icon: Desktop },
    { name: "时间", icon: Clock },
    { name: "在场", icon: Eye },
    { name: "ArcRelay", icon: Devices },
    { name: "手动", icon: Keyboard },
  ];
  $: category =
    trigger.type === "application"
      ? "应用"
      : trigger.type === "system"
        ? "系统"
        : trigger.type === "schedule"
          ? "时间"
          : trigger.type === "presence"
            ? "在场"
          : ["device", "transfer"].includes(trigger.type)
            ? "ArcRelay"
            : "手动";
  $: matching = apps.filter((a) =>
    `${a.name} ${a.path}`
      .toLocaleLowerCase()
      .includes(search.toLocaleLowerCase()),
  );
  $: queuePreview(JSON.stringify(trigger));
  onDestroy(() => {
    clearTimeout(timer);
    request++;
  });
  function queuePreview(_key: string) {
    clearTimeout(timer);
    const version = ++request;
    next = null;
    previewError = "";
    if (trigger.type !== "schedule") return;
    const snapshot = structuredClone(trigger);
    timer = setTimeout(async () => {
      try {
        const value = await bridge.previewAutomationSchedule(snapshot);
        if (version === request) next = value;
      } catch (error) {
        if (version === request) previewError = errorMessage(error);
      }
    }, 250);
  }
  function capability(id: string) {
    return capabilities.find((c) => c.id === id);
  }
  function changeCategory(name: string) {
    const option = triggerChoices.find(
      (c) => c.category === name && capability(c.capability)?.available,
    );
    if (option) trigger = option.trigger();
  }
  function pickApp(app: InstalledApp) {
    if (trigger.type !== "application") return;
    const selected = identity(app);
    trigger = {
      ...trigger,
      apps: trigger.apps.some((a) => a.id === selected.id)
        ? trigger.apps.filter((a) => a.id !== selected.id)
        : [...trigger.apps, selected],
    };
  }
  function pickDevice(id: string) {
    if (trigger.type !== "device" && trigger.type !== "transfer") return;
    trigger = {
      ...trigger,
      deviceIds: trigger.deviceIds.includes(id)
        ? trigger.deviceIds.filter((v) => v !== id)
        : [...trigger.deviceIds, id],
    };
  }
  function capture(event: KeyboardEvent) {
    if (trigger.type !== "hotkey") return;
    const result = captureActionShortcut(event);
    if (result.type !== "navigate") event.preventDefault();
    if (result.type === "shortcut") {
      trigger = { ...trigger, shortcut: result.value };
      shortcutError = "";
    }
    if (result.type === "clear") trigger = { ...trigger, shortcut: "" };
    if (result.type === "error") shortcutError = result.message;
  }
</script>

<div class="au-categories" aria-label={uiTranslate("触发类型", $uiLanguage)}>
  {#each categories as item}
    <button
      class:chosen={category === item.name}
      aria-pressed={category === item.name}
      on:click={() => changeCategory(item.name)}
      ><svelte:component this={item.icon} size={22} />{uiTranslate(item.name, $uiLanguage)}</button
    >
  {/each}
</div>
{#if trigger.type === "application"}
  <label class="au-field"
    >{uiTranslate("何时触发", $uiLanguage)}<AppSelect bind:value={trigger.event} aria-label={uiTranslate("何时触发", $uiLanguage)}
      options={[
        ...(Object.entries(applicationLabels)).map(([value, label]) => ({ value: value, disabled: (!capability(`application.${value}`)?.available), label: [uiTranslate(label, $uiLanguage), (uiTranslate(!capability(`application.${value}`)?.available
            ? " · 当前环境不支持"
            : "", $uiLanguage))].join("") })),
      ]}
    /></label
  >
  <div class="au-label-row">
    <h3>{uiTranslate("选择应用", $uiLanguage)}</h3>
    <span class="au-muted">{uiTranslate("任意一个匹配即可", $uiLanguage)}</span>
  </div>
  <label class="au-search"
    ><MagnifyingGlass size={18} /><input
      aria-label={uiTranslate("搜索已安装应用", $uiLanguage)}
      placeholder={uiTranslate("搜索已安装应用", $uiLanguage)}
      bind:value={search}
    /></label
  >
  <div class="au-app-list">
    {#each matching as app (app.path)}
      <button
        class="au-app-option"
        class:chosen={trigger.apps.some((a) => a.id === identity(app).id)}
        aria-pressed={trigger.apps.some((a) => a.id === identity(app).id)}
        on:click={() => pickApp(app)}
        ><InstalledAppIcon path={app.path} name={app.name} size={28} /><span
          ><strong>{app.name}</strong><small>{app.path}</small></span
        >{#if trigger.apps.some((a) => a.id === identity(app).id)}<CheckCircle
            size={21}
            weight="fill"
          />{/if}</button
      >
    {:else}<p class="au-muted">
        {uiTranslate(search
          ? "没有找到匹配的应用"
          : "没有可选应用。请确认应用已安装后重新加载页面。", $uiLanguage)}
      </p>{/each}
  </div>
  {#if trigger.apps.length}<div class="au-chips">
      {#each trigger.apps as app}<button
          on:click={() => {
            if (trigger.type === "application")
              trigger = {
                ...trigger,
                apps: trigger.apps.filter((a) => a.id !== app.id),
              };
          }}
          aria-label={uiTranslate(`移除应用 ${app.name}`, $uiLanguage)}>{app.name} ×</button
        >{/each}
    </div>{/if}
  <p class="au-note">
    <Info
      size={16}
    />{uiTranslate("按应用身份识别，不读取窗口标题。切换同一应用的窗口不会重复触发。", $uiLanguage)}
  </p>
{:else if trigger.type === "schedule"}
  <div class="au-two-fields">
    <label class="au-field"
      >{uiTranslate("运行时间", $uiLanguage)}<input type="time" bind:value={trigger.time} /></label
    ><label class="au-field"
      >{uiTranslate("固定时区", $uiLanguage)}<input
        placeholder="Asia/Shanghai"
        bind:value={trigger.timezone}
        list="automation-timezones"
      /></label
    >
  </div>
  <datalist id="automation-timezones"
    ><option value="Asia/Shanghai"></option><option value="Asia/Tokyo"
    ></option><option value="Europe/London"></option><option
      value="America/New_York"
    ></option><option value="America/Los_Angeles"></option><option value="UTC"
    ></option></datalist
  >
  <div class="au-label-row">
    <h3>{uiTranslate("重复日期", $uiLanguage)}</h3>
    <div class="au-inline">
      <button
        class="au-text"
        on:click={() => {
          if (trigger.type === "schedule") trigger.weekdays = [1, 2, 3, 4, 5];
        }}>{uiTranslate("工作日", $uiLanguage)}</button
      ><button
        class="au-text"
        on:click={() => {
          if (trigger.type === "schedule") trigger.weekdays = [...weekdays];
        }}>{uiTranslate("每天", $uiLanguage)}</button
      >
    </div>
  </div>
  <div class="au-weekdays">
    {#each weekdays as day}<button
        aria-label={weekdayName(day, $uiLanguage, "long")}
        aria-pressed={trigger.weekdays.includes(day)}
        class:chosen={trigger.weekdays.includes(day)}
        on:click={() => {
          if (trigger.type === "schedule")
            trigger.weekdays = trigger.weekdays.includes(day)
              ? trigger.weekdays.filter((d) => d !== day)
              : [...trigger.weekdays, day].sort();
        }}>{weekdayName(day, $uiLanguage)}</button
      >{/each}
  </div>
  <div class="au-info">
    <Clock size={20} />
    <div>
      <strong>{uiTranslate("下一次运行", $uiLanguage)}</strong>
      <p>
        {uiTranslate(next
          ? `${formatDate(next, trigger.timezone)} · ${trigger.timezone}`
          : previewError ||
            (bridge.isTauri()
              ? "请填写有效时间、日期和时区"
              : "下次运行时间由桌面调度服务计算"), $uiLanguage)}
      </p>
    </div>
  </div>
  <fieldset class="au-radio-list">
    <legend>{uiTranslate("电脑休眠或 ArcRelay 未运行，错过时间时", $uiLanguage)}</legend><label
      ><input type="radio" bind:group={trigger.catchUp} value={false} /><span
        >{uiTranslate("跳过这一次", $uiLanguage)}<small>{uiTranslate("等到下一个计划时间再运行。", $uiLanguage)}</small></span
      ></label
    ><label
      ><input type="radio" bind:group={trigger.catchUp} value={true} /><span
        >{uiTranslate("恢复后补跑一次", $uiLanguage)}<small>{uiTranslate("错过多次也只补一次，仍检查附加条件。", $uiLanguage)}</small></span
      ></label
    >
  </fieldset>
  <p class="au-note">
    {uiTranslate("时间跟随固定时区，不随电脑切换时区改变。夏令时由调度服务处理。", $uiLanguage)}
  </p>
{:else if trigger.type === "system"}
  <div class="au-choice-list">
    {#each triggerChoices.filter((c) => c.category === "系统") as choice}<button
        class:chosen={choice.capability === `system.${trigger.event}`}
        disabled={!capability(choice.capability)?.available}
        on:click={() => (trigger = choice.trigger())}
        ><Desktop size={21} /><span
          ><strong>{uiTranslate(choice.label, $uiLanguage)}</strong
          >{#if !capability(choice.capability)?.available}<small
              >{uiTranslate(capability(choice.capability)?.reason ||
                "当前系统不支持", $uiLanguage)}</small
            >{/if}</span
        ></button
      >{/each}
  </div>
{:else if trigger.type === "presence"}
  <div class="au-choice-list">
    {#each triggerChoices.filter((choice) => choice.category === "在场") as choice}<button
        class:chosen={choice.capability === `presence.${trigger.state}`}
        disabled={!capability(choice.capability)?.available}
        on:click={() => (trigger = choice.trigger())}
        ><Eye size={21} /><span><strong>{uiTranslate(choice.label, $uiLanguage)}</strong><small>{uiTranslate("状态稳定后触发；原始人脸特征不会写入自动化。", $uiLanguage)}</small></span></button
      >{/each}
  </div>
  <p class="au-note"><Info size={16} />{uiTranslate(`当前选择：${presenceLabels[trigger.state]}。需要先在眼动设置中录入本机用户。`, $uiLanguage)}</p>
{:else if trigger.type === "device" || trigger.type === "transfer"}
  <label class="au-field"
    >{uiTranslate("何时触发", $uiLanguage)}<AppSelect value={trigger.type === "device"
        ? `device.${trigger.connected ? "connected" : "disconnected"}`
        : `transfer.${trigger.received ? "received" : "sent"}`} onValueChange={(e) => {
        const choice = triggerChoices.find(
          (c) => c.capability === e,
        );
        if (choice) trigger = choice.trigger();
      }} aria-label={uiTranslate("何时触发", $uiLanguage)}
      options={[
        ...(triggerChoices.filter((c) => c.category === "ArcRelay")).map((c) => ({ value: c.capability, label: uiTranslate(c.label, $uiLanguage) })),
      ]}
    /></label
  >
  <div class="au-label-row">
    <h3>{uiTranslate("设备范围", $uiLanguage)}</h3>
    <button
      class="au-text"
      on:click={() => {
        if (trigger.type === "device" || trigger.type === "transfer")
          trigger.deviceIds = [];
      }}>{uiTranslate("任意已配对设备", $uiLanguage)}</button
    >
  </div>
  <p class="au-muted">
    {uiTranslate(trigger.deviceIds.length
      ? "仅以下设备，任意一个匹配即可。"
      : "未限定设备：匹配任意已配对设备。", $uiLanguage)}
  </p>
  <div class="au-choice-list">
    {#each devices as device}<button
        aria-pressed={trigger.deviceIds.includes(device.id)}
        class:chosen={trigger.deviceIds.includes(device.id)}
        on:click={() => pickDevice(device.id)}
        ><Devices size={22} /><span>{device.name}</span
        >{#if trigger.deviceIds.includes(device.id)}<CheckCircle
            size={20}
          />{/if}</button
      >{:else}<p class="au-note">
        {uiTranslate("尚无已配对设备。可先保存，配对设备后再指定范围。", $uiLanguage)}
      </p>{/each}
  </div>
  {#each trigger.deviceIds.filter((id) => !devices.some((d) => d.id === id)) as id}<div
      class="au-warning"
    >
      {uiTranslate("设备已解除配对：", $uiLanguage)}{id}<button
        class="au-text"
        on:click={() => pickDevice(id)}>{uiTranslate("移除", $uiLanguage)}</button
      >
    </div>{/each}
  {#if trigger.type === "transfer"}<label class="au-field"
      >{uiTranslate("文件类型", $uiLanguage)} <span class="au-muted">{uiTranslate("可选，以逗号分隔扩展名", $uiLanguage)}</span><input
        placeholder={uiTranslate("全部类型；例如 pdf, png, docx", $uiLanguage)}
        value={trigger.fileKinds.join(", ")}
        on:change={(e) => {
          if (trigger.type === "transfer")
            trigger.fileKinds = e.currentTarget.value
              .split(/[,，]/)
              .map((s) => s.trim().replace(/^\./, "").toLowerCase())
              .filter(Boolean);
        }}
      /></label
    >{/if}
{:else}
  <div class="au-choice-list">
    {#each triggerChoices.filter((c) => c.category === "手动") as c}<button
        class:chosen={trigger.type === c.capability}
        on:click={() => (trigger = c.trigger())}
        ><Keyboard size={21} /><span>{uiTranslate(c.label, $uiLanguage)}</span></button
      >{/each}
  </div>
  {#if trigger.type === "hotkey"}<label class="au-field"
      >{uiTranslate("录入快捷键", $uiLanguage)}<input
        readonly
        placeholder={uiTranslate("点击后按下组合键", $uiLanguage)}
        value={trigger.shortcut}
        on:keydown={capture}
      /></label
    >
    <p class="au-note">
      {uiTranslate("使用 Command / Control / Option 等修饰键。Backspace 清除，Tab\n      移动焦点。保存时检查快捷键占用。", $uiLanguage)}
    </p>
    {#if shortcutError}<p role="alert" class="au-error">
        {shortcutError}
      </p>{/if}{:else}<div class="au-info">
      <Info size={20} />
      <p>{uiTranslate("由你在 ArcRelay 中手动发起，不会在后台自动触发。", $uiLanguage)}</p>
    </div>{/if}
{/if}
