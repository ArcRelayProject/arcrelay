<script lang="ts">
  import AppSelect from "../components/AppSelect.svelte";
  import { weekdayName } from "../automation";
  import { translate as uiTranslate, language as uiLanguage } from "../i18n";
  import { Plus, Trash, Info } from "phosphor-svelte";
  import { weekdays, dayNames, identity } from "../automation";
  import type {
    AutomationCondition,
    ConnectedDeviceView,
  } from "../ipc/generated";
  import type { InstalledApp } from "../types";
  export let conditions: AutomationCondition[];
  export let apps: InstalledApp[];
  export let devices: ConnectedDeviceView[];
  let kind = "time";
  function add() {
    conditions = [
      ...conditions,
      kind === "time"
        ? {
            type: "timeRange",
            weekdays: [1, 2, 3, 4, 5],
            start: "09:00",
            end: "18:00",
            timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
          }
        : kind === "app"
          ? {
              type: "applicationRunning",
              app: { id: "", name: "", path: "" },
              running: true,
            }
          : { type: "deviceConnected", deviceId: "", connected: true },
    ];
  }
</script>

<p class="au-note">
  <Info size={17} />{uiTranslate("可选。必须全部满足才运行；不满足会记录“未运行”。", $uiLanguage)}
</p>
{#each conditions as condition, index}
  {#if index}<div class="au-and">{uiTranslate("并且", $uiLanguage)}</div>{/if}
  <div class="au-condition">
    <div class="au-label-row">
      <h3>
        {uiTranslate(condition.type === "timeRange"
          ? "星期和时间范围"
          : condition.type === "applicationRunning"
            ? "应用运行状态"
            : "设备连接状态", $uiLanguage)}
      </h3>
      <button
        class="au-icon danger"
        aria-label={uiTranslate(`移除条件 ${index + 1}`, $uiLanguage)}
        on:click={() => (conditions = conditions.filter((_, i) => i !== index))}
        ><Trash size={18} /></button
      >
    </div>
    {#if condition.type === "timeRange"}
      <div class="au-weekdays">
        {#each weekdays as day}<button
            aria-label={`${index + 1}: ${weekdayName(day, $uiLanguage, "long")}`}
            aria-pressed={condition.weekdays.includes(day)}
            class:chosen={condition.weekdays.includes(day)}
            on:click={() => {
              condition.weekdays = condition.weekdays.includes(day)
                ? condition.weekdays.filter((d) => d !== day)
                : [...condition.weekdays, day].sort();
              conditions = [...conditions];
            }}>{weekdayName(day, $uiLanguage)}</button
          >{/each}
      </div>
      <div class="au-two-fields">
        <label class="au-field"
          >{uiTranslate("开始时间", $uiLanguage)}<input type="time" bind:value={condition.start} /></label
        ><label class="au-field"
          >{uiTranslate("结束时间", $uiLanguage)} {uiTranslate(condition.end <= condition.start ? "（次日）" : "", $uiLanguage)}<input
            type="time"
            bind:value={condition.end}
          /></label
        >
      </div>
      <label class="au-field"
        >{uiTranslate("固定时区", $uiLanguage)}<input
          bind:value={condition.timezone}
          placeholder="Asia/Shanghai"
        /></label
      >
    {:else if condition.type === "applicationRunning"}
      <div class="au-two-fields">
        <label class="au-field"
          >{uiTranslate("应用", $uiLanguage)}<AppSelect value={condition.app.id} onValueChange={(e) => {
              const app = apps.find(
                (a) => identity(a).id === e,
              );
              if (app) {
                condition.app = identity(app);
                conditions = [...conditions];
              }
            }} aria-label={uiTranslate("应用", $uiLanguage)}
            options={[
              { value: "", label: uiTranslate("选择已安装应用", $uiLanguage) },
              ...(condition.app.id && !apps.some((a) => identity(a).id === condition.app.id) ? [{ value: condition.app.id, label: [(condition.app.name), uiTranslate("（需要重新选择）", $uiLanguage)].join("") }] : []),
              ...apps.map((app) => ({ value: (identity(app).id), label: app.name })),
            ]}
          /></label
        ><label class="au-field"
          >{uiTranslate("要求状态", $uiLanguage)}<AppSelect bind:value={condition.running} aria-label={uiTranslate("要求状态", $uiLanguage)}
            options={[
              { value: true, label: uiTranslate("正在运行", $uiLanguage) },
              { value: false, label: uiTranslate("未运行", $uiLanguage) },
            ]}
          /></label
        >
      </div>
    {:else}
      <div class="au-two-fields">
        <label class="au-field"
          >{uiTranslate("设备", $uiLanguage)}<AppSelect bind:value={condition.deviceId} aria-label={uiTranslate("设备", $uiLanguage)}
            options={[
              { value: "", label: uiTranslate("选择已配对设备", $uiLanguage) },
              ...(condition.deviceId && !devices.some((d) => d.id === condition.deviceId) ? [{ value: condition.deviceId, label: uiTranslate("设备已解除配对", $uiLanguage) }] : []),
              ...devices.map((device) => ({ value: device.id, label: device.name })),
            ]}
          /></label
        ><label class="au-field"
          >{uiTranslate("要求状态", $uiLanguage)}<AppSelect bind:value={condition.connected} aria-label={uiTranslate("要求状态", $uiLanguage)}
            options={[
              { value: true, label: uiTranslate("已连接", $uiLanguage) },
              { value: false, label: uiTranslate("未连接", $uiLanguage) },
            ]}
          /></label
        >
      </div>
      <p class="au-note">{uiTranslate("这是运行所需的条件，不是设备当前的实时状态。", $uiLanguage)}</p>
    {/if}
  </div>
{/each}
<div class="au-inline au-add-condition">
  <AppSelect aria-label={uiTranslate("新增条件类型", $uiLanguage)} bind:value={kind}
    options={[
      { value: "time", label: uiTranslate("星期和时间范围", $uiLanguage) },
      { value: "app", label: uiTranslate("应用运行状态", $uiLanguage) },
      { value: "device", label: uiTranslate("设备连接状态", $uiLanguage) },
    ]}
  /><button class="au-button" disabled={conditions.length >= 16} on:click={add}
    ><Plus size={18} />{uiTranslate("添加条件", $uiLanguage)}</button
  >
</div>
