<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../i18n";
  import { Lightning, FunnelSimple, Play } from "phosphor-svelte";
  import { triggerSummary, conditionSummary, stepSummary } from "../automation";
  import type {
    AutomationDefinition,
    ActionView,
    ConnectedDeviceView,
  } from "../ipc/generated";
  export let definition: AutomationDefinition;
  export let actions: ActionView[] = [];
  export let devices: ConnectedDeviceView[] = [];
</script>

<div class="au-rule-summary">
  <div>
    <Lightning size={18} /><span class="au-muted">{uiTranslate("触发器", $uiLanguage)}</span><strong
      >{triggerSummary(definition.trigger, $uiLanguage)}</strong
    >
  </div>
  <div>
    <FunnelSimple size={18} /><span class="au-muted">{uiTranslate("仅当", $uiLanguage)}</span><strong
      >{definition.conditions.length
        ? definition.conditions
            .map((c) =>
              c.type === "deviceConnected"
                ? `${devices.find(d => d.id === c.deviceId)?.name || uiTranslate("所选设备", $uiLanguage)} · ${uiTranslate(c.connected ? "已连接" : "未连接", $uiLanguage)}`
                : conditionSummary(c, $uiLanguage),
            )
            .join(" · ")
        : uiTranslate("无需额外条件", $uiLanguage)}</strong
    >
  </div>
  <div>
    <Play size={18} /><span class="au-muted">{uiTranslate("执行", $uiLanguage)}</span><strong
      >{definition.steps.map((s) => stepSummary(s, actions, $uiLanguage)).join(" → ") ||
        uiTranslate("尚未添加动作", $uiLanguage)}</strong
    >
  </div>
</div>
