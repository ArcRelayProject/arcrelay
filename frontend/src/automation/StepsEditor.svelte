<script lang="ts">
  import AppSelect from "../components/AppSelect.svelte";
  import { translate as uiTranslate, language as uiLanguage } from "../i18n";
  import { tick } from "svelte";
  import {
    Plus,
    ArrowUp,
    ArrowDown,
    Trash,
    DotsSixVertical,
    Clock,
    Bell,
    TerminalWindow,
    X,
    MagnifyingGlass,
    MusicNote,
    ShieldCheck,
    Info,
    Code,
    Play,
    CheckCircle,
  } from "phosphor-svelte";
  import { bridge } from "../bridge";
  import ActionIcon from "../ActionIcon.svelte";
  import QuickActionDialog from "./QuickActionDialog.svelte";
  import { stepSummary, variables, builtinNames } from "../automation";
  import { actionSummary, errorMessage } from "../app_helpers";
  import type {
    AutomationStep,
    AutomationTrigger,
    ActionView,
    ActionType,
    Capability,
  } from "../ipc/generated";
  import type { InstalledApp } from "../types";
  export let steps: AutomationStep[];
  export let trigger: AutomationTrigger;
  export let actions: ActionView[];
  export let apps: InstalledApp[];
  export let capabilities: Capability[];
  export let pane: "actions" | "variables" | null = null;
  export let install: (name: string, type: ActionType) => Promise<string>;
  export let test: (index: number) => void;
  let search = "";
  let group = "";
  let tab = "mine";
  let create = false;
  let drag = -1;
  let busy = false;
  let error = "";
  let announcement = "";
  let target: {
    step: AutomationStep;
    field: "script" | "title" | "body";
    node: HTMLInputElement | HTMLTextAreaElement;
    start: number;
    end: number;
  } | null = null;
  $: matches = actions.filter(
    (a) =>
      (!group || a.group === group) &&
      `${a.name} ${actionSummary(a)}`
        .toLowerCase()
        .includes(search.toLowerCase()),
  );
  $: available = variables(trigger);
  function focusPanel(node: HTMLElement) {
    const previous = document.activeElement as HTMLElement | null;
    (
      node.querySelector<HTMLElement>("input") ??
      node.querySelector<HTMLElement>("button")
    )?.focus();
    const keyboard = (event: KeyboardEvent) => {
      if (event.key !== "Tab" || !matchMedia("(max-width: 1080px)").matches)
        return;
      const nodes = [
        ...node.querySelectorAll<HTMLElement>(
          "button:not(:disabled),input,select,summary",
        ),
      ].filter((element) => element.getClientRects().length);
      const first = nodes[0],
        last = nodes[nodes.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    };
    node.addEventListener("keydown", keyboard);
    return {
      destroy() {
        node.removeEventListener("keydown", keyboard);
        previous?.focus();
      },
    };
  }
  function add(step: AutomationStep) {
    steps = [...steps, step];
    announcement = `已添加动作 ${steps.length}`;
  }
  function move(from: number, to: number) {
    if (to < 0 || to >= steps.length) return;
    const copy = [...steps];
    const [item] = copy.splice(from, 1);
    copy.splice(to, 0, item);
    steps = copy;
    announcement = `动作已移到第 ${to + 1} 步`;
  }
  function remember(
    step: AutomationStep,
    field: "script" | "title" | "body",
    node: HTMLInputElement | HTMLTextAreaElement,
  ) {
    target = {
      step,
      field,
      node,
      start: node.selectionStart ?? 0,
      end: node.selectionEnd ?? 0,
    };
  }
  async function insert(key: string) {
    if (!target || !steps.includes(target.step)) return;
    const { step, field, start, end, node } = target;
    const token = `{{ ${key} }}`;
    if (step.type === "shell" && field === "script")
      step.script =
        step.script.slice(0, start) + token + step.script.slice(end);
    if (step.type === "notification" && field !== "script")
      step[field] =
        step[field].slice(0, start) + token + step[field].slice(end);
    steps = [...steps];
    await tick();
    node.focus();
    node.setSelectionRange(start + token.length, start + token.length);
    remember(step, field, node);
  }
  async function common(name: string, type: ActionType) {
    if (busy) return;
    busy = true;
    error = "";
    try {
      add({ type: "quickAction", actionId: await install(name, type) });
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
  async function pickDirectory(step: AutomationStep) {
    try {
      const value = await bridge.pickAutomationPath(true);
      if (value && step.type === "shell") {
        step.workingDirectory = value;
        steps = [...steps];
      }
    } catch (e) {
      error = errorMessage(e);
    }
  }
</script>

<svelte:window
  on:keydown={(e) => {
    if (e.key === "Escape" && pane && !create) pane = null;
  }}
/>
<p class="au-note">{uiTranslate("从上到下依次执行，某一步失败就停止。最多 64 个动作。", $uiLanguage)}</p>
<span class="au-sr-only" aria-live="polite">{announcement}</span>
<ol class="au-steps">
  {#each steps as step, index (step)}
    <li class="au-step" class:advanced={step.type === "shell"}>
      <div class="au-step-head">
        <button
          class="au-drag au-icon"
          aria-label={uiTranslate(`拖动动作 ${index + 1}，也可使用上移和下移按钮`, $uiLanguage)}
          draggable="true"
          on:dragstart={(e) => {
            drag = index;
            if (e.dataTransfer) {
              e.dataTransfer.effectAllowed = "move";
              e.dataTransfer.setData("text/plain", String(index));
            }
          }}
          on:dragend={() => (drag = -1)}><DotsSixVertical size={20} /></button
        >
        <span class="au-step-number">{index + 1}</span>
        <span class="au-step-icon"
          >{#if step.type === "quickAction"}{#if builtinNames[step.actionId]}<ShieldCheck
                size={24}
              />{:else}{@const action = actions.find(
                (a) => a.id === step.actionId,
              )}<ActionIcon
                iconId={action?.icon_id || "zap"}
                iconSvg={action?.icon_svg || ""}
                size={24}
              />{/if}{:else if step.type === "delay"}<Clock
              size={24}
            />{:else if step.type === "notification"}<Bell
              size={24}
            />{:else}<TerminalWindow size={24} />{/if}</span
        >
        <div class="au-step-title">
          {#if step.type === "delay"}
            <label class="au-delay-field"
              >{uiTranslate("等待", $uiLanguage)}
              <input
                aria-label={uiTranslate("等待时长", $uiLanguage)}
                type="number"
                min="0"
                max="86400"
                step="1"
                bind:value={step.durationSeconds}
              />
              {uiTranslate("秒", $uiLanguage)}
            </label>
            <small>{uiTranslate("最长 24 小时", $uiLanguage)}</small>
          {:else}<strong
              >{step.type === "notification"
                ? "显示通知"
                : stepSummary(step, actions, $uiLanguage)}</strong
            >{#if step.type === "quickAction"}<small
                >{uiTranslate(builtinNames[step.actionId]
                  ? "ArcRelay 内置操作"
                  : "来自快捷动作", $uiLanguage)}</small
              >{/if}
          {/if}
        </div>
        <div class="au-step-tools">
          <button
            class="au-icon"
            aria-label={uiTranslate(`上移动作 ${index + 1}`, $uiLanguage)}
            disabled={index === 0}
            on:click={() => move(index, index - 1)}
            ><ArrowUp size={17} /></button
          ><button
            class="au-icon"
            aria-label={uiTranslate(`下移动作 ${index + 1}`, $uiLanguage)}
            disabled={index === steps.length - 1}
            on:click={() => move(index, index + 1)}
            ><ArrowDown size={17} /></button
          ><button
            class="au-icon"
            aria-label={uiTranslate(`测试动作 ${index + 1}`, $uiLanguage)}
            on:click={() => test(index)}><Play size={17} /></button
          ><button
            class="au-icon danger"
            aria-label={uiTranslate(`删除动作 ${index + 1}`, $uiLanguage)}
            on:click={() => {
              steps = steps.filter((_, i) => i !== index);
              target = null;
            }}><Trash size={17} /></button
          >
        </div>
      </div>
      {#if step.type === "notification"}
        <div class="au-step-body">
          <label class="au-field"
            >{uiTranslate("通知标题", $uiLanguage)}<input
              bind:value={step.title}
              placeholder={uiTranslate("例如：收到文件", $uiLanguage)}
              maxlength={256}
              on:focus={(e) => remember(step, "title", e.currentTarget)}
              on:blur={(e) => remember(step, "title", e.currentTarget)}
            /></label
          ><label class="au-field"
            >{uiTranslate("通知内容", $uiLanguage)}<textarea
              rows="3"
              bind:value={step.body}
              placeholder={uiTranslate("填写内容，也可以插入事件变量", $uiLanguage)}
              on:focus={(e) => remember(step, "body", e.currentTarget)}
              on:blur={(e) => remember(step, "body", e.currentTarget)}
            ></textarea></label
          >
          <div class="au-label-row">
            <button class="au-text" on:click={() => (pane = "variables")}
              ><Code size={17} />{uiTranslate("插入事件变量", $uiLanguage)}</button
            ><label class="au-checkbox"
              ><input
                type="checkbox"
                bind:checked={step.sendToConnectedDevices}
              />{uiTranslate("同时通知已连接设备", $uiLanguage)}</label
            >
          </div>
        </div>
      {:else if step.type === "shell"}
        <div class="au-step-body">
          <div class="au-two-fields">
            <label class="au-field"
              >Shell<AppSelect bind:value={step.shell} aria-label={"Shell"}
                options={[
                  ...(["system", "sh", "bash", "zsh", "powershell"]).map((shell) => ({ value: shell, disabled: (!capabilities.some(
                      (c) => c.id === `shell.${shell}` && c.available,
                    )), label: [uiTranslate(shell === "system"
                      ? "系统默认"
                      : shell, $uiLanguage), (uiTranslate(!capabilities.some(
                      (c) => c.id === `shell.${shell}` && c.available,
                    )
                      ? "（不可用）"
                      : "", $uiLanguage))].join("") })),
                ]}
              /></label
            ><label class="au-field"
              >{uiTranslate("超时（秒）", $uiLanguage)}<input
                type="number"
                min="1"
                max="3600"
                bind:value={step.timeoutSeconds}
              /></label
            >
          </div>
          <label class="au-field"
            >{uiTranslate("脚本", $uiLanguage)}<textarea
              class="au-code-input"
              spellcheck="false"
              rows="7"
              bind:value={step.script}
              placeholder={uiTranslate("输入脚本", $uiLanguage)}
              on:focus={(e) => remember(step, "script", e.currentTarget)}
              on:blur={(e) => remember(step, "script", e.currentTarget)}
            ></textarea></label
          ><button class="au-text" on:click={() => (pane = "variables")}
            ><Code size={17} />{uiTranslate("插入事件变量", $uiLanguage)}</button
          >
          <div class="au-path-field">
            <label class="au-field"
              >{uiTranslate("工作目录（可选）", $uiLanguage)}<input
                value={step.workingDirectory || ""}
                placeholder={uiTranslate("使用默认工作目录", $uiLanguage)}
                on:input={(e) => {
                  if (step.type === "shell") {
                    step.workingDirectory = e.currentTarget.value || null;
                    steps = [...steps];
                  }
                }}
              /></label
            ><button class="au-button" on:click={() => pickDirectory(step)}
              >{uiTranslate("选择…", $uiLanguage)}</button
            >
          </div>
          <p class="au-note">
            <Info
              size={16}
            />{uiTranslate("事件变量通过环境变量安全传入；不要把它们再次作为代码执行。", $uiLanguage)}
          </p>
        </div>
      {/if}
      <div
        class="au-drop-target"
        role="presentation"
        on:dragover|preventDefault={(e) => {
          if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
        }}
        on:drop|preventDefault={() => {
          if (drag >= 0) move(drag, index);
          drag = -1;
        }}
      >
        {uiTranslate("拖到这里排序", $uiLanguage)}
      </div>
    </li>
  {/each}
</ol>
{#if !steps.length}<div class="au-step-empty">
    <Play size={28} />
    <h3>{uiTranslate("接下来，让它做什么？", $uiLanguage)}</h3>
    <p>{uiTranslate("使用已有快捷动作，或添加等待、通知和脚本。", $uiLanguage)}</p>
  </div>{/if}
<button
  class="au-add-step"
  disabled={steps.length >= 64}
  on:click={() => (pane = "actions")}><Plus size={19} />{uiTranslate("添加动作", $uiLanguage)}</button
>
{#if error}<p class="au-error" role="alert">{error}</p>{/if}
{#if pane}
  <aside
    class="au-tool-panel"
    aria-label={uiTranslate(pane === "actions" ? "添加动作" : "事件变量", $uiLanguage)}
    use:focusPanel
  >
    <div class="au-label-row">
      <h2>{uiTranslate(pane === "actions" ? "添加动作" : "事件变量", $uiLanguage)}</h2>
      <button
        class="au-icon"
        aria-label={uiTranslate("关闭动作侧栏", $uiLanguage)}
        on:click={() => (pane = null)}><X size={20} /></button
      >
    </div>
    {#if pane === "actions"}
      <label class="au-search"
        ><MagnifyingGlass size={18} /><input
          aria-label={uiTranslate("搜索动作", $uiLanguage)}
          placeholder={uiTranslate("搜索动作", $uiLanguage)}
          bind:value={search}
        /></label
      >
      <div class="au-tabs">
        <button class:chosen={tab === "mine"} on:click={() => (tab = "mine")}
          >{uiTranslate("我的快捷动作", $uiLanguage)}</button
        ><button
          class:chosen={tab === "common"}
          on:click={() => (tab = "common")}>{uiTranslate("常用操作", $uiLanguage)}</button
        >
      </div>
      {#if tab === "mine"}<AppSelect class="au-group-select" aria-label={uiTranslate("动作分组", $uiLanguage)} bind:value={group}
        options={[
          { value: "", label: uiTranslate("全部分组", $uiLanguage) },
          ...([...new Set(actions.map((a) => a.group))]).map((value) => ({ value: value, label: uiTranslate(value || "未分组", $uiLanguage) })),
        ]}
      />
        <div class="au-picker-actions">
          {#each matches as action}<button
              disabled={steps.length >= 64}
              on:click={() => add({ type: "quickAction", actionId: action.id })}
              ><span class="au-step-icon"
                ><ActionIcon
                  iconId={action.icon_id}
                  iconSvg={action.icon_svg}
                  size={22}
                /></span
              ><span
                ><strong>{action.name}</strong><small
                  >{action.group || actionSummary(action)}</small
                ></span
              ><Plus size={18} /></button
            >{:else}<p class="au-muted">
              {uiTranslate(search
                ? "没有匹配的动作，试试其他关键词。"
                : "还没有快捷动作，可以在下方新建。", $uiLanguage)}
            </p>{/each}
        </div>
      {:else}<div class="au-picker-actions">
          {#if "暂停音乐".includes(search)}<button
              disabled={busy || steps.length >= 64}
              on:click={() =>
                common("暂停音乐", { type: "Media", operation: "pause" })}
              ><MusicNote size={23} /><span>{uiTranslate("暂停音乐", $uiLanguage)}</span><Plus
                size={18}
              /></button
            >{/if}{#each Object.entries(builtinNames).filter( ([, name]) => name.includes(search) ) as [id, name]}<button
              disabled={steps.length >= 64}
              on:click={() => add({ type: "quickAction", actionId: id })}
              ><ShieldCheck size={23} /><span>{name}</span><Plus
                size={18}
              /></button
            >{/each}
        </div>{/if}
      <div class="au-picker-divider">{uiTranslate("流程辅助", $uiLanguage)}</div>
      <div class="au-picker-actions">
        <button
          disabled={steps.length >= 64}
          on:click={() => add({ type: "delay", durationSeconds: 2 })}
          ><Clock size={23} /><span>{uiTranslate("等待", $uiLanguage)}</span><Plus size={18} /></button
        ><button
          disabled={steps.length >= 64}
          on:click={() =>
            add({
              type: "notification",
              title: "",
              body: "",
              sendToConnectedDevices: false,
            })}
          ><Bell size={23} /><span>{uiTranslate("显示通知", $uiLanguage)}</span><Plus size={18} /></button
        >
      </div>
      <details class="au-picker-advanced">
        <summary>{uiTranslate("高级", $uiLanguage)}</summary>
        <div class="au-picker-actions">
          <button
            disabled={steps.length >= 64}
            on:click={() =>
              add({
                type: "shell",
                shell: "system",
                script: "",
                workingDirectory: null,
                timeoutSeconds: 300,
              })}
            ><TerminalWindow size={23} /><span>{uiTranslate("运行 Shell 脚本", $uiLanguage)}</span><Plus
              size={18}
            /></button
          >
        </div>
      </details>
      <button
        class="au-text au-new-action"
        disabled={steps.length >= 64}
        on:click={() => (create = true)}><Plus size={19} />{uiTranslate("新建快捷动作", $uiLanguage)}</button
      >
    {:else}
      <p class="au-muted">{uiTranslate("先将光标放在通知或脚本中，再点击变量插入。", $uiLanguage)}</p>
      {#if target && steps.includes(target.step)}<p class="au-note">
          <CheckCircle size={16} />{uiTranslate("插入到动作", $uiLanguage)} {steps.indexOf(target.step) + 1} · {uiTranslate(target.field ===
          "script"
            ? "脚本"
            : target.field === "title"
              ? "标题"
              : "正文", $uiLanguage)}
        </p>{/if}
      <div class="au-picker-actions">
        {#each available as [label, key]}<button
            disabled={!target || !steps.includes(target.step)}
            on:click={() => insert(key)}
            ><span><strong>{uiTranslate(label, $uiLanguage)}</strong><small>{key}</small></span><Plus
              size={18}
            /></button
          >{:else}<p class="au-note">{uiTranslate("当前触发类型没有可用事件变量。", $uiLanguage)}</p>{/each}
      </div>
      <div class="au-info">
        <Info size={19} />
        <p>{uiTranslate("变量来自真实触发事件。切换触发类型后，保存时会校验已有引用。", $uiLanguage)}</p>
      </div>
    {/if}
  </aside>
{/if}
{#if create}<QuickActionDialog
    {apps}
    close={() => (create = false)}
    save={async (name, type) =>
      add({ type: "quickAction", actionId: await install(name, type) })}
  />{/if}
