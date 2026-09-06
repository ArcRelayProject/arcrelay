<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../i18n";
  import { onMount } from "svelte";
  import {
    CheckCircle,
    WarningCircle,
    Clock,
    ShieldCheck,
    Stop,
    ArrowSquareOut,
    Info,
  } from "phosphor-svelte";
  import {
    formatDate,
    isActive,
    statusLabels,
    stepSummary,
    triggerSummary,
  } from "../automation";
  import type { AutomationActivity } from "../ipc/generated";
  export let activity: AutomationActivity;
  export let busy = false;
  export let stopping = false;
  export let confirmRun: () => void;
  export let cancel: () => void;
  export let edit: (() => void) | null = null;
  let now = Date.now();
  onMount(() => {
    const timer = setInterval(() => (now = Date.now()), 500);
    return () => clearInterval(timer);
  });
  $: completed = activity.steps.filter((s) => s.status === "succeeded").length;
  $: pending = activity.steps.find((s) => s.status === "awaitingConfirmation");
  function duration(start: string | null, end: string | null) {
    return start
      ? `${Math.max(0, ((end ? Date.parse(end) : now) - Date.parse(start)) / 1000).toFixed(1)} 秒`
      : "尚未开始";
  }
</script>

<section class="au-activity-detail" aria-label={uiTranslate("运行详情", $uiLanguage)}>
  <div
    class="au-status"
    class:success={activity.status === "succeeded"}
    class:warning={activity.status === "awaitingConfirmation" ||
      activity.status === "skipped"}
    class:danger={activity.status === "failed" ||
      activity.status === "interrupted"}
  >
    {#if activity.status === "succeeded"}<CheckCircle
        weight="fill"
        size={20}
      />{:else if activity.status === "awaitingConfirmation"}<ShieldCheck
        size={20}
      />{:else if activity.status === "failed" || activity.status === "interrupted"}<WarningCircle
        size={20}
      />{:else}<Clock size={20} />{/if}{uiTranslate(statusLabels[activity.status], $uiLanguage)}
  </div>
  <h2>{activity.definition.name}</h2>
  <p class="au-muted">
    {formatDate(activity.createdAt)} · {triggerSummary(
      activity.definition.trigger, $uiLanguage)}
  </p>
  <p class="au-note">{uiTranslate("事件：", $uiLanguage)}{activity.event.kind} {uiTranslate("· 本机执行", $uiLanguage)}</p>
  {#if activity.reason}<div
      class:au-warning={activity.status !== "failed"}
      class:au-error={activity.status === "failed"}
      role="status"
    >
      {activity.reason}
    </div>{/if}
  {#if activity.status === "awaitingConfirmation"}
    <div class="au-warning">
      <strong>{uiTranslate(pending?.action?.name || "运行前确认", $uiLanguage)}</strong>
      <p>
        {uiTranslate("请核对下面的本次动作快照。只允许在已解锁的这台电脑上确认，手机不能代为授权。", $uiLanguage)}
      </p>
      {#if pending?.action?.definition}<details open>
          <summary>{uiTranslate("需要确认的动作与参数", $uiLanguage)}</summary>
          <pre>{JSON.stringify(pending.action.definition, null, 2)}</pre>
        </details>{/if}
      <p>
        {uiTranslate("执行后可能改变系统、应用或隐私状态；关机或重启可能丢失未保存的内容。", $uiLanguage)}
      </p>
    </div>
    <div class="au-inline">
      <button
        class="au-button danger-solid"
        disabled={busy || stopping}
        on:click={confirmRun}
        >{uiTranslate("确认", $uiLanguage)}{uiTranslate(pending?.action
          ? `执行「${pending.action.name}」`
          : "运行本次自动化", $uiLanguage)}</button
      ><button class="au-button" disabled={busy || stopping} on:click={cancel}
        >{uiTranslate("跳过这一次", $uiLanguage)}</button
      >
    </div>
  {:else if activity.status === "interrupted"}<div class="au-info">
      <Info size={19} />
      <p>
        {uiTranslate("上次运行被中断，不会自动重放。已经完成的动作可能仍有影响，请先检查设置与结果。", $uiLanguage)}
      </p>
    </div>
  {:else if activity.status === "canceled"}<p class="au-note">
      {uiTranslate("已停止后续步骤。已完成的动作不会撤销。", $uiLanguage)}
    </p>{/if}
  {#if edit && !isActive(activity.status)}<button
      class="au-button"
      on:click={edit}><ArrowSquareOut size={18} />{uiTranslate("检查自动化设置", $uiLanguage)}</button
    >{/if}
  {#if activity.steps.length}<div class="au-label-row">
      <h3>{uiTranslate("本次执行步骤", $uiLanguage)}</h3>
      <span class="au-muted">{completed} / {activity.steps.length} {uiTranslate("已完成", $uiLanguage)}</span>
    </div>
    <ol class="au-run-steps">
      {#each activity.steps as step}
        <li
          class:failed={step.status === "failed"}
          class:current={step.status === "running" ||
            step.status === "awaitingConfirmation"}
        >
          <div class="au-run-step-head">
            <span class="au-step-number">{step.index + 1}</span>
            <div>
              <strong>{step.action?.name || stepSummary(step.step, [], $uiLanguage)}</strong
              ><small
                >{uiTranslate(statusLabels[step.status], $uiLanguage)} · {duration(
                  step.startedAt,
                  step.finishedAt,
                )}</small
              >
            </div>
            {#if step.status === "succeeded"}<CheckCircle
                size={22}
                class="au-success-text"
              />{/if}
          </div>
          {#if step.status === "running" && step.step.type === "delay" && step.startedAt}<progress
              aria-label={uiTranslate("等待进度", $uiLanguage)}
              max={Math.max(1, step.step.durationSeconds)}
              value={Math.max(
                0,
                Math.min(
                  step.step.durationSeconds,
                  (now - Date.parse(step.startedAt)) / 1000,
                ),
              )}
            ></progress>{:else if step.status === "running"}<progress
              aria-label={uiTranslate("动作正在执行", $uiLanguage)}
            ></progress>{/if}
          {#if step.result.error}<p class="au-error">
              {step.result.error}
            </p>{/if}
          {#if step.result.exitCode !== null}<p class="au-note">
              {uiTranslate("退出码：", $uiLanguage)}{step.result.exitCode}
            </p>{/if}
          {#if step.result.stdout || step.result.stderr}<details
              open={step.status === "failed"}
            >
              <summary>{uiTranslate("查看执行输出", $uiLanguage)}</summary>{#if step.result.stdout}<h4>
                  {uiTranslate("标准输出", $uiLanguage)}
                </h4>
                <pre>{step.result.stdout}</pre>{/if}{#if step.result.stderr}<h4>
                  {uiTranslate("错误输出", $uiLanguage)}
                </h4>
                <pre>{step.result.stderr}</pre>{/if}
            </details>{/if}
          {#if step.action}<details>
              <summary>{uiTranslate("查看本次动作快照", $uiLanguage)}</summary>
              <pre>{JSON.stringify(step.action.definition, null, 2)}</pre>
            </details>{:else if step.step.type === "shell"}<details>
              <summary>{uiTranslate("查看本次脚本", $uiLanguage)}</summary>
              <pre>{step.step.script}</pre>
            </details>{/if}
        </li>
      {/each}
    </ol>{:else}<p class="au-note">{uiTranslate("此次没有执行动作。", $uiLanguage)}</p>{/if}
  {#if isActive(activity.status) && activity.status !== "awaitingConfirmation"}<button
      class="au-button danger"
      disabled={busy || stopping}
      on:click={cancel}
      ><Stop size={18} />{uiTranslate(stopping ? "正在停止…" : "停止运行", $uiLanguage)}</button
    >
    <p class="au-note">
      {uiTranslate("会停止后续动作；当前系统操作可能需要执行结束，已经完成的动作不会撤销。", $uiLanguage)}
    </p>{/if}
  <details class="au-event-details">
    <summary>{uiTranslate("触发事件与变量", $uiLanguage)}</summary>
    <p class="au-note">
      {activity.event.id} · {formatDate(activity.event.occurredAt)}
    </p>
    {#each Object.entries(activity.event.variables) as [key, value]}<div>
        <code>{key}</code>
        <pre>{value}</pre>
      </div>{:else}<p class="au-note">{uiTranslate("该事件没有附加变量。", $uiLanguage)}</p>{/each}
  </details>
</section>
