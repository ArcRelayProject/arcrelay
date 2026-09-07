<script lang="ts">
  import { dismissibleDropdown } from "./dismissibleDropdown";
  import AppSelect from "./components/AppSelect.svelte";
  import { translate as uiTranslate, language as uiLanguage } from "./i18n";
  import { onMount, onDestroy, tick } from "svelte";
  import {
    Plus,
    Play,
    ArrowLeft,
    Lightning,
    ClockCounterClockwise,
    Trash,
    Copy,
    MagnifyingGlass,
    DotsThree,
    PencilSimple,
    CaretRight,
    CaretDown,
    X,
    CheckCircle,
    WarningCircle,
    Info,
    VideoCamera,
    ArrowSquareOut,
    Briefcase,
    FileArrowDown,
    Moon,
    ShieldCheck,
    Clock,
    FunnelSimple,
  } from "phosphor-svelte";
  import { bridge } from "./bridge";
  import { errorMessage } from "./app_helpers";
  import {
    newDefinition,
    triggerSummary,
    conditionSummary,
    stepSummary,
    generatedName,
    identity,
    statusLabels,
    formatDate,
    isActive,
    ruleSummary,
    matchesAutomation,
    copyDefinition,
    referencedVariables,
    samplesFor,
  } from "./automation";
  import TriggerEditor from "./automation/TriggerEditor.svelte";
  import ConditionsEditor from "./automation/ConditionsEditor.svelte";
  import StepsEditor from "./automation/StepsEditor.svelte";
  import ActivityDetail from "./automation/ActivityDetail.svelte";
  import RuleSummary from "./automation/RuleSummary.svelte";
  import Modal from "./automation/Modal.svelte";
  import "./automation/automation.css";
  import type { ActionView, AppLanguage, InstalledApp } from "./types";
  import type {
    AutomationDefinition,
    AutomationTrigger,
    AutomationActivity,
    AutomationIssue,
    Capability,
    ConnectedDeviceView,
    ActionType,
    QuickAction,
  } from "./ipc/generated";
  export let actions: ActionView[] = [];
  export let language: AppLanguage;
  export let notify: (message: string, kind?: "success" | "error") => void;
  export let confirm: (request: {
    title: string;
    description: string;
    confirmLabel: string;
    kind: "danger" | "primary";
    onConfirm: () => Promise<boolean | void> | boolean | void;
  }) => void;
  export let openDevices: () => void = () => {};
  let definitions: AutomationDefinition[] = [];
  let activities: AutomationActivity[] = [];
  let capabilities: Capability[] = [];
  let badges: Record<string, AutomationIssue[]> = {};
  let apps: InstalledApp[] = [];
  let devices: ConnectedDeviceView[] = [];
  let editor: AutomationDefinition | null = null;
  let screen: "list" | "choose" | "edit" | "check" | "activity" = "list";
  let initial = "";
  let selectedId: string | null = null;
  let selectedActivity: string | null = null;
  let query = "";
  let filter = "all";
  let activityFilter = "";
  let statusFilter = "all";
  let activityQuery = "";
  let menu: string | null = null;
  let loading = true;
  let dataReady = false;
  let busy = false;
  let error = "";
  let issues: AutomationIssue[] = [];
  let checked = false;
  let pendingIds: string[] = [];
  let stoppingIds: string[] = [];
  let pane: "actions" | "variables" | null = null;
  let expanded = 1;
  let testModal = false;
  let testOnly: number | null = null;
  let sampleId = "";
  let workdaySetup = false;
  let workdayApp = "";
  let workdayDirectory = "";
  let workdayAction = "";
  let templateError = "";
  let disposed = false;
  let version = 0;
  let refreshTimer: ReturnType<typeof setTimeout>;
  let pageNode: HTMLElement;
  const tr = (zh: string, en: string) => uiTranslate(zh, language === "system" ? $uiLanguage : language);
  const templates = [
    {
      name: "会议模式",
      icon: VideoCamera,
      color: "purple",
      detail: "会议应用进入前台，暂停音乐并开启投屏隐私",
    },
    {
      name: "离开会议",
      icon: ArrowSquareOut,
      color: "blue",
      detail: "会议应用退出后，询问是否关闭投屏隐私",
    },
    {
      name: "工作日开工",
      icon: Briefcase,
      color: "green",
      detail: "工作日早晨，打开你的常用工作环境",
    },
    {
      name: "收到文件",
      icon: FileArrowDown,
      color: "orange",
      detail: "接收完成后通知你，并打开接收文件夹",
    },
    {
      name: "锁屏安静模式",
      icon: Moon,
      color: "purple",
      detail: "锁定电脑时，暂停音乐并静音麦克风",
    },
  ];
  $: dirty = editor !== null && JSON.stringify(editor) !== initial;
  $: shown = definitions.filter(
    (d) =>
      matchesAutomation(d, query, actions, $uiLanguage) &&
      (filter === "all" ||
        (filter === "enabled"
          ? d.enabled
          : filter === "paused"
            ? !d.enabled
            : badges[d.id]?.length ||
              activities.some(
                (a) =>
                  a.automationId === d.id &&
                  a.status === "awaitingConfirmation",
              ))),
  );
  $: selected = definitions.find((d) => d.id === selectedId);
  $: shownActivities = activities.filter(
    (a) =>
      (!activityFilter || a.automationId === activityFilter) &&
      (statusFilter === "all" || a.status === statusFilter) &&
      (a.definition.name + " " + (a.reason || ""))
        .toLowerCase()
        .includes(activityQuery.toLowerCase()),
  );
  $: currentActivity =
    shownActivities.find((a) => a.id === selectedActivity) ??
    shownActivities[0];
  $: saveLabel =
    editor && definitions.some((d) => d.id === editor?.id && d.enabled)
      ? "保存"
      : "保存并启用";
  $: samples = editor ? samplesFor(editor, activities) : [];
  $: needsSample = editor
    ? referencedVariables(
        testOnly === null
          ? editor
          : { ...editor, steps: [editor.steps[testOnly]] },
      ).length > 0
    : false;
  $: if (screen !== "edit" || expanded !== 3) pane = null;

  async function focusTitle() {
    await tick();
    pageNode?.querySelector<HTMLElement>("h1")?.focus();
    pageNode?.scrollTo({ top: 0 });
  }
  async function task(fn: () => Promise<void>) {
    if (busy) return;
    busy = true;
    error = "";
    try {
      await fn();
    } catch (e) {
      error = errorMessage(e);
      notify(error, "error");
      await tick();
      pageNode?.querySelector<HTMLElement>('[role="alert"]')?.focus();
    } finally {
      busy = false;
    }
  }
  async function refresh() {
    const current = ++version;
    const [ds, logs, cs] = await Promise.all([
      bridge.listAutomations(),
      bridge.listAutomationActivities(),
      bridge.automationCapabilities(),
    ]);
    if (disposed || current !== version) return;
    definitions = ds;
    activities = logs;
    capabilities = cs;
    dataReady = true;
    stoppingIds = stoppingIds.filter((id) =>
      logs.some((a) => a.id === id && isActive(a.status)),
    );
    const checks = await Promise.all(
      ds.map(async (d) => [d.id, await bridge.checkAutomation(d)] as const),
    );
    if (!disposed && current === version) badges = Object.fromEntries(checks);
  }
  function scheduleRefresh() {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(
      () => void refresh().catch((e) => (error = errorMessage(e))),
      150,
    );
  }
  onMount(() => {
    let unlisten: (() => void) | undefined;
    let unlistenConfiguration: (() => void) | undefined;
    void task(async () => {
      const [installed, state] = await Promise.all([
        bridge.listInstalledApps().catch((e) => {
          error = "failed to load application list: " + errorMessage(e);
          return [];
        }),
        bridge.getBootstrapState(),
      ]);
      if (disposed) return;
      apps = installed;
      devices = state.pairedDevices;
      unlisten = await bridge.onAutomationActivity(scheduleRefresh);
      unlistenConfiguration = await bridge.onAutomationConfiguration(scheduleRefresh);
      if (disposed) {
        unlisten();
        unlistenConfiguration();
        return;
      }
      await refresh();
      selectedId = definitions[0]?.id ?? null;
    }).finally(() => (loading = false));
    return () => { unlisten?.(); unlistenConfiguration?.(); };
  });
  onDestroy(() => {
    disposed = true;
    version++;
    clearTimeout(refreshTimer);
  });
  export function requestLeave(next: () => void) {
    if (busy) {
      notify("正在处理，请稍候。");
      return;
    }
    if (dirty)
      confirm({
        title: "放弃未保存的修改？",
        description: "已经保存的自动化不受影响；测试不会保存你的编辑。",
        confirmLabel: "放弃修改",
        kind: "danger",
        onConfirm: () => {
          editor = null;
          next();
        },
      });
    else next();
  }
  function navigate(next: typeof screen) {
    requestLeave(() => {
      editor = null;
      screen = next;
      error = "";
      menu = null;
      void focusTitle();
    });
  }
  function edit(d: AutomationDefinition, section = 3) {
    editor = structuredClone(d);
    initial = JSON.stringify(editor);
    issues = [];
    checked = false;
    pane = null;
    expanded = section;
    error = "";
    screen = "edit";
    void focusTitle();
  }
  function create(
    trigger: AutomationTrigger = {
      type: "application",
      event: "foreground",
      apps: [],
    },
  ) {
    edit(newDefinition(trigger), 1);
  }
  async function installAction(name: string, action_type: ActionType) {
    const existing = actions.find(
      (a) =>
        a.name === name &&
        JSON.stringify(a.action_type) === JSON.stringify(action_type),
    );
    if (existing) return existing.id;
    const action: QuickAction = {
      revision: 0,
      id: crypto.randomUUID(),
      name,
      icon_id: "zap",
      icon_svg: "",
      color: "#5b5ff0",
      group: "自动化",
      sort_order: actions.length,
      source_preset_id: null,
      confirm_before_run: false,
      action_type,
    };
    const state = await bridge.saveAction(action);
    actions = state.actions;
    return action.id;
  }
  async function useTemplate(name: string) {
    if (name === "工作日开工") {
      workdaySetup = true;
      templateError = "";
      workdayApp =
        apps.find((a) => /code|studio|idea|zed/i.test(a.name))?.path || "";
      workdayDirectory = "";
      workdayAction = "";
      return;
    }
    await task(async () => {
      const meeting = apps.filter((a) =>
        /zoom|teams|腾讯会议|tencent.*meeting|wemeet|飞书|feishu|lark/i.test(
          a.name,
        ),
      );
      const trigger: AutomationTrigger =
        name === "会议模式" || name === "离开会议"
          ? {
              type: "application",
              event: name === "会议模式" ? "foreground" : "exited",
              apps: meeting.map(identity),
            }
          : name === "锁屏安静模式"
            ? { type: "system", event: "locked" }
            : {
                type: "transfer",
                received: true,
                deviceIds: [],
                fileKinds: [],
              };
      const d = newDefinition(trigger);
      d.name = name;
      if (name === "会议模式")
        d.steps = [
          {
            type: "quickAction",
            actionId: await installAction("暂停音乐", {
              type: "Media",
              operation: "pause",
            }),
          },
          { type: "quickAction", actionId: "builtin-automation-privacy-on" },
        ];
      if (name === "离开会议")
        d.steps = [
          { type: "quickAction", actionId: "builtin-automation-privacy-off" },
        ];
      if (name === "锁屏安静模式")
        d.steps = [
          {
            type: "quickAction",
            actionId: await installAction("暂停音乐", {
              type: "Media",
              operation: "pause",
            }),
          },
          {
            type: "quickAction",
            actionId: await installAction("麦克风静音", {
              type: "SetMicrophone",
              active: false,
            }),
          },
        ];
      if (name === "收到文件")
        d.steps = [
          {
            type: "notification",
            title: "收到文件",
            body: "来自 {{ event.device.name }} 的 {{ event.file.count }} 个文件已保存到 {{ event.file.directory }}",
            sendToConnectedDevices: false,
          },
          {
            type: "shell",
            shell: "system",
            script: /Win/i.test(navigator.platform)
              ? 'Invoke-Item -LiteralPath "{{ event.file.directory }}"'
              : /Mac/i.test(navigator.platform)
                ? 'open -- "{{ event.file.directory }}"'
                : 'xdg-open "{{ event.file.directory }}"',
            workingDirectory: null,
            timeoutSeconds: 30,
          },
        ];
      edit(d, trigger.type === "application" && !meeting.length ? 1 : 3);
      initial = "";
    });
  }
  async function applyWorkday() {
    if (busy) return;
    busy = true;
    templateError = "";
    try {
      const app = apps.find((a) => a.path === workdayApp);
      if (!app || !workdayDirectory || !workdayAction)
        throw Error("select an editor, project folder, and startup quick action");
      const d = newDefinition({
        type: "schedule",
        time: "09:00",
        weekdays: [1, 2, 3, 4, 5],
        timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
        catchUp: false,
      });
      d.name = "工作日开工";
      d.steps = [
        {
          type: "quickAction",
          actionId: await installAction("打开 " + app.name, {
            type: "LaunchApp",
            app_name: app.name,
            app_path: app.path,
          }),
        },
        {
          type: "quickAction",
          actionId: await installAction("打开项目文件夹", {
            type: "OpenPath",
            path: workdayDirectory,
          }),
        },
        { type: "quickAction", actionId: workdayAction },
      ];
      workdaySetup = false;
      edit(d);
      initial = "";
    } catch (e) {
      templateError = errorMessage(e);
    } finally {
      busy = false;
    }
  }
  async function check() {
    await task(async () => {
      if (!editor) return;
      const d = {
        ...editor,
        name: editor.name.trim() || generatedName(editor, actions, $uiLanguage),
        enabled: true,
      };
      checked = false;
      issues = await bridge.checkAutomation(d);
      checked = true;
      screen = "check";
      void focusTitle();
    });
  }
  async function save(enabled: boolean) {
    await task(async () => {
      if (!editor) return;
      const d = {
        ...structuredClone(editor),
        name: editor.name.trim() || generatedName(editor, actions, $uiLanguage),
        enabled,
      };
      if (enabled) {
        issues = await bridge.checkAutomation(d);
        checked = true;
        if (issues.length) {
          screen = "check";
          return;
        }
      }
      const saved = await bridge.saveAutomation(d);
      initial = JSON.stringify(saved);
      editor = null;
      selectedId = saved.id;
      screen = "list";
      await refresh();
      notify(enabled ? "自动化已保存并启用" : "自动化已保存为停用", "success");
      void focusTitle();
    });
  }
  async function toggle(d: AutomationDefinition) {
    if (pendingIds.includes(d.id)) return;
    pendingIds = [...pendingIds, d.id];
    error = "";
    try {
      if (!d.enabled) {
        const found = await bridge.checkAutomation({ ...d, enabled: true });
        if (found.length) {
          edit(d);
          issues = found;
          checked = true;
          screen = "check";
          return;
        }
      }
      await bridge.setAutomationEnabled(d.id, !d.enabled);
      await refresh();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      pendingIds = pendingIds.filter((id) => id !== d.id);
    }
  }
  function remove(d: AutomationDefinition) {
    menu = null;
    confirm({
      title: `删除“${d.name}”？`,
      description: "已开始的运行与活动记录会保留，删除不会停止正在运行的动作。",
      confirmLabel: "删除自动化",
      kind: "danger",
      onConfirm: async () => {
        await bridge.deleteAutomation(d.id);
        if (selectedId === d.id) selectedId = null;
        await refresh();
      },
    });
  }
  function openActivities(id = "") {
    activityFilter = id;
    statusFilter = "all";
    activityQuery = "";
    selectedActivity = null;
    screen = "activity";
    menu = null;
    void focusTitle();
  }
  function beginTest(index: number | null = null, d?: AutomationDefinition) {
    if (d) edit(d);
    testOnly = index;
    sampleId = "";
    testModal = true;
    error = "";
  }
  async function runTest() {
    await task(async () => {
      if (!editor) return;
      const d = structuredClone(editor);
      d.name = d.name.trim() || generatedName(d, actions, $uiLanguage);
      if (testOnly !== null) d.steps = [d.steps[testOnly]];
      const sample = samples.find((a) => a.id === sampleId)?.event;
      if (needsSample && !sample)
        throw Error("a real event sample is required; trigger the event before testing");
      selectedActivity = await bridge.testAutomation(
        d,
        sample ? { ...sample, id: crypto.randomUUID() } : null,
      );
      testModal = false;
      activityFilter = d.id;
      statusFilter = "all";
      activityQuery = "";
      await refresh();
      screen = "activity";
      void focusTitle();
    });
  }
  async function run(d: AutomationDefinition) {
    await task(async () => {
      const id = await bridge.runAutomation(d.id);
      activityFilter = d.id;
      statusFilter = "all";
      activityQuery = "";
      selectedActivity = id;
      await refresh();
      screen = "activity";
      void focusTitle();
    });
  }
  async function cancelActivity(a: AutomationActivity) {
    stoppingIds = [...stoppingIds, a.id];
    await task(async () => {
      try {
        await bridge.cancelAutomationActivity(a.id);
        statusFilter = "all";
        selectedActivity = a.id;
        await refresh();
      } catch (e) {
        stoppingIds = stoppingIds.filter((id) => id !== a.id);
        throw e;
      }
    });
  }
  async function repair(issue: AutomationIssue) {
    if (issue.remedy === "inputPermission")
      await task(() => bridge.openInputPermissionSettings());
    else if (issue.remedy === "screenPermission")
      await task(() => bridge.openAutomationScreenPermission());
    else if (issue.remedy === "devices") requestLeave(openDevices);
    else {
      screen = "edit";
      expanded = issue.stepIndex === null ? 1 : 3;
      pane = issue.remedy === "actions" ? "actions" : null;
      void focusTitle();
    }
  }
  function clearActivities() {
    confirm({
      title: "清理已完成的活动？",
      description: "等待确认、排队和运行中的活动不会被删除。此操作无法撤销。",
      confirmLabel: "清理活动",
      kind: "danger",
      onConfirm: async () => {
        await bridge.clearAutomationActivities();
        await refresh();
      },
    });
  }
</script>

<svelte:window
  on:focus={() => !loading && scheduleRefresh()}
  on:beforeunload={(e) => {
    if (dirty) {
      e.preventDefault();
      e.returnValue = "";
    }
  }}
/>
<main
  class="au-page"
  class:has-pane={pane !== null && screen === "edit"}
  bind:this={pageNode}
>
  {#if !bridge.isTauri()}<div class="au-preview-notice">
      {uiTranslate("浏览器预览 · 仅模拟数据与交互，不执行这台电脑上的动作", $uiLanguage)}
    </div>{/if}
  {#if error}<div class="au-error au-page-error" role="alert" tabindex="-1">
      <WarningCircle size={19} /><span>{error}</span><button
        class="au-icon"
        aria-label={uiTranslate("关闭错误提示", $uiLanguage)}
        on:click={() => (error = "")}><X size={18} /></button
      >
    </div>{/if}
  {#if screen === "list"}
    <header class="au-heading">
      <div>
        <h1 tabindex="-1">{uiTranslate(tr("自动化", "Automations"), $uiLanguage)}</h1>
        <p>{uiTranslate("让重复的小事，按你的节奏完成。", $uiLanguage)}</p>
      </div>
      <div class="au-inline">
        <button class="au-button" on:click={() => openActivities()}
          ><ClockCounterClockwise
            size={19}
          />{uiTranslate("活动记录", $uiLanguage)}{#if activities.some((a) => a.status === "awaitingConfirmation")}<span
              class="au-count"
              >{activities.filter((a) => a.status === "awaitingConfirmation")
                .length}</span
            >{/if}</button
        ><button
          class="au-button primary"
          disabled={loading}
          on:click={() => navigate("choose")}
          ><Plus size={19} />{uiTranslate("新建自动化", $uiLanguage)}</button
        >
      </div>
    </header>
    {#if loading}<div class="au-empty" role="status">
        <span class="loading-spinner"></span>
        <p>{uiTranslate("正在加载自动化…", $uiLanguage)}</p>
      </div>
    {:else if !dataReady}<div class="au-empty">
        <WarningCircle size={30} />
        <h3>{uiTranslate("暂时无法加载自动化", $uiLanguage)}</h3>
        <p>{uiTranslate("现有数据未被修改，请重试。", $uiLanguage)}</p>
        <button class="au-button" disabled={busy} on:click={() => task(refresh)}
          >{uiTranslate("重新加载", $uiLanguage)}</button
        >
      </div>{:else if !definitions.length}<section class="au-welcome">
        <span class="au-hero-icon"
          ><Lightning size={34} weight="duotone" /></span
        >
        <h2>{uiTranslate("让日常小事自动完成", $uiLanguage)}</h2>
        <p>{uiTranslate("选一个熟悉的场景，或从自己的想法开始。", $uiLanguage)}</p>
        <div class="au-example">
          <span>{uiTranslate("当会议应用进入前台", $uiLanguage)}</span><CaretRight size={18} /><span
            >{uiTranslate("暂停音乐", $uiLanguage)}</span
          ><CaretRight size={18} /><span>{uiTranslate("开启投屏隐私", $uiLanguage)}</span>
        </div>
        <div class="au-templates">
          {#each templates as item}<button
              disabled={busy}
              on:click={() => useTemplate(item.name)}
              ><span class="au-template-icon {item.color}"
                ><svelte:component this={item.icon} size={25} /></span
              ><strong>{uiTranslate(item.name, $uiLanguage)}</strong>
              <p>{uiTranslate(item.detail, $uiLanguage)}</p>
              <span class="au-text">{uiTranslate("使用模板", $uiLanguage)} <CaretRight size={16} /></span
              ></button
            >{/each}
        </div>
        <button class="au-button primary" on:click={() => create()}
          ><Plus size={19} />{uiTranslate("从空白创建", $uiLanguage)}</button
        >
        <p class="au-note">{uiTranslate("在这台电脑上运行，创建后可先测试，再启用。", $uiLanguage)}</p>
      </section>
    {:else}<div class="au-management">
        <section class="au-list-region" aria-label={uiTranslate("自动化列表", $uiLanguage)}>
          <div class="au-list-toolbar">
            <label class="au-search"
              ><MagnifyingGlass size={18} /><input
                bind:value={query}
                placeholder={uiTranslate("搜索名称或动作", $uiLanguage)}
                aria-label={uiTranslate("搜索自动化", $uiLanguage)}
              /></label
            >
            <div class="au-tabs">
              <button
                class:chosen={filter === "all"}
                on:click={() => (filter = "all")}
                >{uiTranslate("全部", $uiLanguage)} {definitions.length}</button
              ><button
                class:chosen={filter === "enabled"}
                on:click={() => (filter = "enabled")}
                >{uiTranslate("已启用", $uiLanguage)} {definitions.filter((d) => d.enabled).length}</button
              ><button
                class:chosen={filter === "paused"}
                on:click={() => (filter = "paused")}
                >{uiTranslate("已停用", $uiLanguage)} {definitions.filter((d) => !d.enabled).length}</button
              ><button
                class:chosen={filter === "attention"}
                on:click={() => (filter = "attention")}
                aria-label={uiTranslate("筛选需要处理的自动化", $uiLanguage)}
                ><WarningCircle size={18} /></button
              >
            </div>
          </div>
          <div class="au-rule-list">
            {#each shown as d}
              {@const latest = activities.find((a) => a.automationId === d.id)}
              <article class="au-rule-row" class:selected={selectedId === d.id}>
                <button
                  class="au-rule-select"
                  on:click={() => (selectedId = d.id)}
                  aria-label={uiTranslate(`查看 ${d.name}`, $uiLanguage)}
                  aria-pressed={selectedId === d.id}
                  ><span
                    class="au-template-icon {d.trigger.type === 'schedule'
                      ? 'green'
                      : d.trigger.type === 'transfer'
                        ? 'orange'
                        : 'purple'}"
                    >{#if d.trigger.type === "application"}<VideoCamera
                        size={25}
                      />{:else if d.trigger.type === "schedule"}<Briefcase
                        size={25}
                      />{:else if d.trigger.type === "transfer"}<FileArrowDown
                        size={25}
                      />{:else}<Lightning size={25} />{/if}</span
                  ><span
                    ><strong>{d.name}</strong><small
                      >{ruleSummary(d, actions, $uiLanguage)}</small
                    ></span
                  ></button
                >
                <span class="au-row-time"
                  >{uiTranslate(latest
                    ? formatDate(latest.createdAt)
                    : "尚未运行", $uiLanguage)}{#if latest}<small
                      class:au-danger-text={latest.status === "failed"}
                      >{uiTranslate(statusLabels[latest.status], $uiLanguage)}</small
                    >{/if}</span
                >
                <button
                  class="au-switch"
                  role="switch"
                  aria-checked={d.enabled}
                  aria-label={uiTranslate(`${d.enabled ? "停用" : "启用"} ${d.name}`, $uiLanguage)}
                  disabled={pendingIds.includes(d.id)}
                  on:click={() => toggle(d)}
                  ><span class:enabled={d.enabled}></span><small
                    >{uiTranslate(pendingIds.includes(d.id)
                      ? "处理中"
                      : d.enabled
                        ? "启用"
                        : "停用", $uiLanguage)}</small
                  ></button
                >
                <div class="au-row-menu" use:dismissibleDropdown={{ open: menu === d.id, close: () => (menu = null) }}>
                  <button
                    class="au-icon"
                    aria-label={uiTranslate(`${d.name} 更多操作`, $uiLanguage)}
                    aria-expanded={menu === d.id}
                    on:click={() => (menu = menu === d.id ? null : d.id)}
                    ><DotsThree size={24} /></button
                  >{#if menu === d.id}<div class="au-menu">
                      <button
                        on:click={() => {
                          menu = null;
                          edit(copyDefinition(d, $uiLanguage));
                          initial = "";
                        }}><Copy size={17} />{uiTranslate("复制", $uiLanguage)}</button
                      ><button on:click={() => openActivities(d.id)}
                        ><ClockCounterClockwise size={17} />{uiTranslate("查看活动", $uiLanguage)}</button
                      ><button class="danger" on:click={() => remove(d)}
                        ><Trash size={17} />{uiTranslate("删除…", $uiLanguage)}</button
                      >
                    </div>{/if}
                </div>
                {#if badges[d.id]?.length}<button
                    class="au-row-issue"
                    on:click={() => {
                      edit(d);
                      issues = badges[d.id];
                      checked = true;
                      screen = "check";
                    }}
                    ><WarningCircle size={16} />{uiTranslate("需要处理 ·", $uiLanguage)} {badges[d.id][0]
                      .message}<CaretRight size={16} /></button
                  >{/if}
              </article>
            {:else}<div class="au-empty">
                <MagnifyingGlass size={30} />
                <h3>{uiTranslate("没有找到匹配的自动化", $uiLanguage)}</h3>
                <p>{uiTranslate("试试其他关键词或筛选条件。", $uiLanguage)}</p>
                <button
                  class="au-button"
                  on:click={() => {
                    query = "";
                    filter = "all";
                  }}>{uiTranslate("清除搜索与筛选", $uiLanguage)}</button
                >
              </div>{/each}
          </div>
        </section>
        {#if selected}{@const latest = activities.find(
            (a) => a.automationId === selected.id,
          )}
          <aside class="au-inspector">
            <div class="au-label-row">
              <h2>{selected.name}</h2>
              <button
                class="au-icon au-mobile-only"
                aria-label={uiTranslate("关闭自动化详情", $uiLanguage)}
                on:click={() => (selectedId = null)}><X size={19} /></button
              >
            </div>
            <p class="au-status" class:success={selected.enabled}>
              <span class="au-dot"></span>{uiTranslate(selected.enabled
                ? "已启用 · 等待触发"
                : "已停用 · 不会自动触发", $uiLanguage)}
            </p>
            <RuleSummary
              definition={selected}
              {actions}
              {devices}
            />{#if selected.trigger.type === "schedule"}<p class="au-note">
                {selected.trigger.timezone} · {uiTranslate(selected.trigger.catchUp
                  ? "错过后补跑一次"
                  : "错过后跳过", $uiLanguage)}
              </p>{/if}
            <div class="au-inline">
              <button class="au-button" on:click={() => edit(selected)}
                ><PencilSimple size={18} />{uiTranslate("编辑", $uiLanguage)}</button
              ><button
                class="au-button"
                disabled={busy ||
                  activities.some(
                    (a) => a.automationId === selected.id && isActive(a.status),
                  )}
                on:click={() => beginTest(null, selected)}
                ><Play size={18} />{uiTranslate("测试动作", $uiLanguage)}</button
              >
            </div>
            {#if selected.trigger.type === "manual" || selected.trigger.type === "hotkey"}<button
                class="au-button primary"
                disabled={busy ||
                  activities.some(
                    (a) => a.automationId === selected.id && isActive(a.status),
                  )}
                on:click={() => run(selected)}
                ><Play size={18} />{uiTranslate("运行一次", $uiLanguage)}</button
              >{/if}
            <h3>{uiTranslate("最近运行", $uiLanguage)}</h3>
            {#if latest}<button
                class="au-recent"
                on:click={() => {
                  openActivities(selected.id);
                  selectedActivity = latest.id;
                }}
                ><span
                  >{formatDate(latest.createdAt)} · {uiTranslate(statusLabels[
                    latest.status
                  ], $uiLanguage)}</span
                ><CaretRight size={17} /></button
              >{#if latest.reason}<p class="au-note">
                  {latest.reason}
                </p>{/if}{:else}<p class="au-muted">
                {uiTranslate("还没有运行记录。可以先测试动作。", $uiLanguage)}
              </p>{/if}
            <p class="au-note">
              {uiTranslate("启用不等于正在运行。同一条自动化同时只运行一个实例。", $uiLanguage)}
            </p>
          </aside>{/if}
      </div>{/if}
  {:else if screen === "choose"}
    <button class="au-breadcrumb" on:click={() => navigate("list")}
      ><ArrowLeft size={17} />{uiTranslate("自动化", $uiLanguage)}</button
    >
    <header class="au-heading">
      <div>
        <h1 tabindex="-1">{uiTranslate("从一个小场景开始", $uiLanguage)}</h1>
        <p>{uiTranslate("模板只是起点，每一个步骤都可以修改。", $uiLanguage)}</p>
      </div>
    </header>
    <div class="au-templates choose">
      {#each templates as item}<button
          disabled={busy}
          on:click={() => useTemplate(item.name)}
          ><span class="au-template-icon {item.color}"
            ><svelte:component this={item.icon} size={25} /></span
          ><strong>{uiTranslate(item.name, $uiLanguage)}</strong>
          <p>{uiTranslate(item.detail, $uiLanguage)}</p>
          <span class="au-text">{uiTranslate("使用模板", $uiLanguage)} <CaretRight size={16} /></span></button
        >{/each}
    </div>
    <button class="au-button primary" on:click={() => create()}
      ><Plus size={18} />{uiTranslate("从空白创建", $uiLanguage)}</button
    >
  {:else if screen === "edit" && editor}
    <div class="au-editor-main">
      <button class="au-breadcrumb" on:click={() => navigate("list")}
        ><ArrowLeft size={17} />{uiTranslate("自动化", $uiLanguage)} <span>{uiTranslate("/ 编辑", $uiLanguage)}</span></button
      >
      <header class="au-editor-heading">
        <h1 tabindex="-1">
          <span class="au-sr-only">{uiTranslate(editor.name || "编辑自动化", $uiLanguage)}</span><input
            aria-label={uiTranslate("自动化名称", $uiLanguage)}
            bind:value={editor.name}
            maxlength={128}
            placeholder={uiTranslate("给自动化起个名字", $uiLanguage)}
          />
        </h1>
        <span class="au-tag"
          >{uiTranslate(dirty || !definitions.some((d) => d.id === editor?.id)
            ? "未保存"
            : "已保存", $uiLanguage)}</span
        >
      </header>
      <section class="au-section">
        <button
          class="au-section-toggle"
          aria-expanded={expanded === 1}
          on:click={() => (expanded = expanded === 1 ? 0 : 1)}
          ><span class="au-section-number">1</span><span
            ><strong>{uiTranslate("当……时", $uiLanguage)}</strong><small
              >{triggerSummary(editor.trigger, $uiLanguage)}</small
            ></span
          ><span class="au-section-edit">{uiTranslate("更改", $uiLanguage)}</span><CaretDown
            size={18}
          /></button
        >{#if expanded === 1}<div class="au-section-body">
            <TriggerEditor
              bind:trigger={editor.trigger}
              {apps}
              {devices}
              {capabilities}
            /><button
              class="au-button primary au-section-next"
              on:click={() => (expanded = 2)}
              >{uiTranslate("继续：附加条件", $uiLanguage)} <CaretRight size={17} /></button
            >
          </div>{/if}
      </section>
      <section class="au-section">
        <button
          class="au-section-toggle"
          aria-expanded={expanded === 2}
          on:click={() => (expanded = expanded === 2 ? 0 : 2)}
          ><span class="au-section-number">2</span><span
            ><strong>{uiTranslate("仅当……", $uiLanguage)} <em>{uiTranslate("可选", $uiLanguage)}</em></strong><small
              >{uiTranslate(editor.conditions.length
                ? editor.conditions.map(c => conditionSummary(c, $uiLanguage)).join(" · ")
                : "没有附加条件，触发后即可运行", $uiLanguage)}</small
            ></span
          ><span class="au-section-edit">{uiTranslate("编辑", $uiLanguage)}</span><CaretDown
            size={18}
          /></button
        >{#if expanded === 2}<div class="au-section-body">
            <ConditionsEditor
              bind:conditions={editor.conditions}
              {apps}
              {devices}
            /><button
              class="au-button primary au-section-next"
              on:click={() => (expanded = 3)}
              >{uiTranslate("继续：添加动作", $uiLanguage)} <CaretRight size={17} /></button
            >
          </div>{/if}
      </section>
      <section class="au-section">
        <button
          class="au-section-toggle"
          aria-expanded={expanded === 3}
          on:click={() => (expanded = expanded === 3 ? 0 : 3)}
          ><span class="au-section-number">3</span><span
            ><strong>{uiTranslate("执行以下操作", $uiLanguage)}</strong><small
              >{uiTranslate(editor.steps.length
                ? `${editor.steps.length} 个动作，按顺序执行`
                : "添加你常用的快捷动作", $uiLanguage)}</small
            ></span
          ><CaretDown size={18} /></button
        >{#if expanded === 3}<div class="au-section-body">
            <StepsEditor
              bind:steps={editor.steps}
              trigger={editor.trigger}
              {actions}
              {apps}
              {capabilities}
              bind:pane
              install={installAction}
              test={(index) => beginTest(index)}
            />
          </div>{/if}
      </section>
      <p class="au-note">
        <ShieldCheck
          size={17}
        />{uiTranslate("本机运行。保存不会执行动作；危险动作始终需要确认。", $uiLanguage)}
      </p>
    </div>
    <footer class="au-sticky-footer">
      <div class="au-inline">
        <button
          class="au-button"
          disabled={busy || !editor.steps.length}
          on:click={() => beginTest()}><Play size={18} />{uiTranslate("测试动作", $uiLanguage)}</button
        ><span class="au-muted au-footer-hint">{uiTranslate("将实际执行动作", $uiLanguage)}</span>
      </div>
      <div class="au-inline">
        <button class="au-button" disabled={busy} on:click={() => save(false)}
          >{uiTranslate("保存为停用", $uiLanguage)}</button
        ><button class="au-button primary" disabled={busy} on:click={check}
          >{uiTranslate(busy ? "检查中…" : saveLabel, $uiLanguage)}</button
        >
      </div>
    </footer>
  {:else if screen === "check" && editor}
    <button
      class="au-breadcrumb"
      on:click={() => {
        screen = "edit";
        void focusTitle();
      }}
      ><ArrowLeft size={17} />{uiTranslate("自动化 /", $uiLanguage)} {uiTranslate(editor.name || "新自动化", $uiLanguage)} {uiTranslate("/ 启用检查", $uiLanguage)}</button
    >
    <header class="au-heading">
      <div>
        <h1 tabindex="-1">
          {uiTranslate(issues.length
            ? "启用前，还有一些事项需要处理"
            : "准备好了，让它自动完成", $uiLanguage)}
        </h1>
        <p>
          {uiTranslate(issues.length
            ? "你的配置仍保留在这里，修复后再检查一次。"
            : "核对规则和运行方式，确认后保存并启用。", $uiLanguage)}
        </p>
      </div>
    </header>
    <div class="au-check-grid">
      <section>
        <div class="au-check-card">
          <div class="au-label-row">
            <h2>{editor.name || generatedName(editor, actions, $uiLanguage)}</h2>
            <button class="au-text" on:click={() => (screen = "edit")}
              >{uiTranslate("编辑", $uiLanguage)}</button
            >
          </div>
          <RuleSummary definition={editor} {actions} {devices} />
        </div>
        <h3>{uiTranslate("运行方式", $uiLanguage)}</h3>
        <fieldset class="au-radio-list">
          <legend class="au-sr-only">{uiTranslate("运行方式", $uiLanguage)}</legend><label
            ><input
              type="radio"
              bind:group={editor.runMode}
              value="automatic"
            /><span>{uiTranslate("自动运行", $uiLanguage)}<small>{uiTranslate("满足条件时，自动执行动作。", $uiLanguage)}</small></span
            ></label
          ><label
            ><input
              type="radio"
              bind:group={editor.runMode}
              value="askBeforeRun"
            /><span>{uiTranslate("运行前询问", $uiLanguage)}<small>{uiTranslate("满足条件时，先询问是否执行。", $uiLanguage)}</small></span
            ></label
          >
        </fieldset>
        <p class="au-note">
          <Info size={17} />{uiTranslate("危险动作始终需要确认，自动运行也不能跳过。", $uiLanguage)}
        </p>
      </section>
      <section class="au-check-card">
        <h2>{uiTranslate("启用检查", $uiLanguage)}</h2>
        {#if checked && !issues.length}<div class="au-check-pass">
            <CheckCircle size={22} weight="fill" />{uiTranslate("当前配置与所需能力检查通过", $uiLanguage)}
          </div>
          <p class="au-note">
            {uiTranslate("全局快捷键将在保存时进行最终占用检查。环境或权限变化会在运行前再次验证。", $uiLanguage)}
          </p>{/if}{#each issues as issue}<div class="au-issue">
            <div>
              <WarningCircle size={21} /><strong>{issue.message}</strong>
            </div>
            {#if issue.stepIndex !== null}<p>
                {uiTranslate("动作", $uiLanguage)} {issue.stepIndex + 1} · {stepSummary(
                  editor.steps[issue.stepIndex],
                  actions, $uiLanguage)}
              </p>{/if}<button
              class="au-button"
              disabled={busy}
              on:click={() => repair(issue)}
              >{uiTranslate(issue.remedy === "inputPermission" ||
              issue.remedy === "screenPermission"
                ? "打开系统设置"
                : issue.remedy === "devices"
                  ? "设备与连接"
                  : "修改配置", $uiLanguage)}</button
            >
          </div>{/each}<button class="au-text" disabled={busy} on:click={check}
          >{uiTranslate(busy ? "检查中…" : "重新检查", $uiLanguage)}</button
        >
      </section>
    </div>
    <footer class="au-sticky-footer">
      <button
        class="au-button"
        disabled={busy}
        on:click={() => (screen = "edit")}
        ><ArrowLeft size={17} />{uiTranslate("返回编辑", $uiLanguage)}</button
      >
      <div class="au-inline">
        <button class="au-button" disabled={busy} on:click={() => save(false)}
          >{uiTranslate("保存为停用", $uiLanguage)}</button
        ><button
          class="au-button primary"
          disabled={busy || !checked || issues.length > 0}
          on:click={() => save(true)}>{saveLabel}</button
        >
      </div>
    </footer>
  {:else if screen === "activity"}
    <button
      class="au-breadcrumb"
      on:click={() => (editor ? (screen = "edit") : navigate("list"))}
      ><ArrowLeft size={17} />{uiTranslate(editor
        ? "返回编辑（内容已保留）"
        : "自动化", $uiLanguage)}</button
    >
    <header class="au-heading">
      <div>
        <h1 tabindex="-1">{uiTranslate(editor ? "测试与运行结果" : "活动记录", $uiLanguage)}</h1>
        <p>{uiTranslate("最近 30 天，最多 100 条已完成记录。待处理的活动会保留。", $uiLanguage)}</p>
      </div>
      <div class="au-inline">
        <button class="au-button" disabled={busy} on:click={() => task(refresh)}
          ><ClockCounterClockwise size={18} />{uiTranslate("刷新", $uiLanguage)}</button
        ><button class="au-button" disabled={busy} on:click={clearActivities}
          ><Trash size={18} />{uiTranslate("清理", $uiLanguage)}</button
        >
      </div>
    </header>
    <div class="au-activity-toolbar">
      <label class="au-search"
        ><MagnifyingGlass size={18} /><input
          placeholder={uiTranslate("搜索自动化或失败原因", $uiLanguage)}
          aria-label={uiTranslate("搜索活动", $uiLanguage)}
          bind:value={activityQuery}
        /></label
      ><AppSelect aria-label={uiTranslate("按自动化筛选活动", $uiLanguage)} bind:value={activityFilter}
        options={[
          { value: "", label: uiTranslate("全部自动化", $uiLanguage) },
          ...([...new Map(activities.map( (a) => [a.automationId, a.definition.name] )).entries()]).map(([id, name]) => ({ value: id, label: name })),
        ]}
      /><AppSelect aria-label={uiTranslate("按运行状态筛选", $uiLanguage)} bind:value={statusFilter}
        options={[
          { value: "all", label: uiTranslate("全部状态", $uiLanguage) },
          ...(Object.entries(statusLabels)).map(([value, label]) => ({ value: value, label: uiTranslate(label, $uiLanguage) })),
        ]}
      />
    </div>
    <div class="au-activity-layout">
      <section class="au-activity-list" aria-label={uiTranslate("活动列表", $uiLanguage)}>
        {#each shownActivities as activity}<button
            class:selected={currentActivity?.id === activity.id}
            on:click={() => (selectedActivity = activity.id)}
            ><span
              class="au-activity-mark"
              class:success={activity.status === "succeeded"}
              class:warning={activity.status === "awaitingConfirmation"}
              class:danger={activity.status === "failed"}
              >{#if activity.status === "succeeded"}<CheckCircle
                  size={23}
                />{:else if activity.status === "failed"}<WarningCircle
                  size={23}
                />{:else}<Clock size={23} />{/if}</span
            ><span
              ><strong>{activity.definition.name}</strong><small
                >{formatDate(activity.createdAt)} · {uiTranslate(statusLabels[
                  activity.status
                ], $uiLanguage)}</small
              >{#if activity.reason}<small>{activity.reason}</small>{/if}</span
            ><CaretRight size={16} /></button
          >{:else}<div class="au-empty">
            <ClockCounterClockwise size={28} />
            <h3>{uiTranslate("暂无匹配的活动", $uiLanguage)}</h3>
            <p>{uiTranslate("触发或测试自动化后，可在这里查看结果。", $uiLanguage)}</p>
            <button
              class="au-text"
              on:click={() => {
                activityFilter = "";
                statusFilter = "all";
                activityQuery = "";
              }}>{uiTranslate("清除筛选", $uiLanguage)}</button
            >
          </div>{/each}
      </section>
      {#if currentActivity}<ActivityDetail
          activity={currentActivity}
          {busy}
          stopping={stoppingIds.includes(currentActivity.id)}
          confirmRun={() =>
            task(async () => {
              const id = currentActivity.id;
              await bridge.confirmAutomationActivity(id);
              statusFilter = "all";
              selectedActivity = id;
              await refresh();
            })}
          cancel={() => cancelActivity(currentActivity)}
          edit={editor
            ? () => (screen = "edit")
            : definitions.some((d) => d.id === currentActivity.automationId)
              ? () => {
                  const d = definitions.find(
                    (d) => d.id === currentActivity.automationId,
                  );
                  if (d) edit(d);
                }
              : null}
        />{/if}
    </div>
  {/if}
</main>

{#if testModal && editor}<Modal
    title={uiTranslate("测试这些动作？", $uiLanguage)}
    {busy}
    close={() => (testModal = false)}
    wide
  >
    <p class="au-warning">
      <WarningCircle size={20} />{uiTranslate("测试会实际执行当前未保存的", $uiLanguage)}{uiTranslate(testOnly === null
        ? "全部动作"
        : `第 ${testOnly + 1} 个动作`, $uiLanguage)}{uiTranslate("。不等待触发、不检查附加条件，也不会保存或启用自动化。", $uiLanguage)}
    </p>
    <p class="au-note">
      {uiTranslate("危险动作仍会要求确认。已完成的动作无法通过“停止”撤销。", $uiLanguage)}
    </p>
    {#if needsSample}<label class="au-field"
        >{uiTranslate("真实事件样本", $uiLanguage)}<AppSelect bind:value={sampleId} aria-label={uiTranslate("真实事件样本", $uiLanguage)}
          options={[
            { value: "", label: uiTranslate("选择一条历史触发事件", $uiLanguage) },
            ...samples.map((a) => ({ value: a.id, label: [(formatDate(a.event.occurredAt)), " · ", (a.definition.name)].join("") })),
          ]}
        /></label
      >{#if !samples.length}<p class="au-warning">
          {uiTranslate("需要真实事件样本，请先触发一次再测试；不会使用虚构变量。", $uiLanguage)}
        </p>{:else if sampleId}{@const sample = samples.find(
          (a) => a.id === sampleId,
        )}
        <details open>
          <summary>{uiTranslate("本次使用的事件变量", $uiLanguage)}</summary>
          <pre>{JSON.stringify(sample?.event.variables, null, 2)}</pre>
        </details>{/if}{:else}<p class="au-info">
        <Info size={18} />{uiTranslate("本次动作不需要事件变量。", $uiLanguage)}
      </p>{/if}
    {#if error}<p class="au-error" role="alert">{error}</p>{/if}
    <footer>
      <button
        class="au-button"
        disabled={busy}
        on:click={() => (testModal = false)}>{uiTranslate("返回编辑", $uiLanguage)}</button
      ><button
        class="au-button primary"
        disabled={busy ||
          (needsSample && !samples.some((a) => a.id === sampleId))}
        on:click={runTest}
        ><Play size={18} />{uiTranslate(busy ? "正在提交…" : "开始测试", $uiLanguage)}</button
      >
    </footer>
  </Modal>{/if}
{#if workdaySetup}<Modal
    title={uiTranslate("设置你的开工场景", $uiLanguage)}
    close={() => (workdaySetup = false)}
    {busy}
  >
    <p class="au-muted">
      {uiTranslate("工作日 09:00，按顺序打开工作环境。生成后仍可自由修改。", $uiLanguage)}
    </p>
    <label class="au-field"
      >{uiTranslate("1 · 常用编辑器", $uiLanguage)}<AppSelect bind:value={workdayApp} aria-label={uiTranslate("1 · 常用编辑器", $uiLanguage)}
        options={[
          { value: "", label: uiTranslate("选择已安装应用", $uiLanguage) },
          ...apps.map((app) => ({ value: app.path, label: app.name })),
        ]}
      /></label
    >
    <div class="au-path-field">
      <label class="au-field"
        >{uiTranslate("2 · 项目文件夹", $uiLanguage)}<input
          bind:value={workdayDirectory}
          placeholder={uiTranslate("选择这台电脑上的文件夹", $uiLanguage)}
        /></label
      ><button
        class="au-button"
        on:click={async () => {
          try {
            const path = await bridge.pickAutomationPath(true);
            if (path) workdayDirectory = path;
          } catch (e) {
            templateError = errorMessage(e);
          }
        }}>{uiTranslate("选择…", $uiLanguage)}</button
      >
    </div>
    <label class="au-field"
      >{uiTranslate("3 · 开工快捷动作", $uiLanguage)}<AppSelect bind:value={workdayAction} aria-label={uiTranslate("3 · 开工快捷动作", $uiLanguage)}
        options={[
          { value: "", label: uiTranslate("选择已有快捷动作", $uiLanguage) },
          ...actions.map((action) => ({ value: action.id, label: action.name })),
        ]}
      /></label
    >
    {#if !actions.length}<p class="au-note">
        {uiTranslate("还没有快捷动作。可先从空白创建，在“添加动作”中就地新建。", $uiLanguage)}
      </p>{/if}
    {#if templateError}<p class="au-error" role="alert">{templateError}</p>{/if}
    <p class="au-note">
      <Info size={17} />{uiTranslate("生成只创建配置，不会立即执行或启用。", $uiLanguage)}
    </p>
    <footer>
      <button
        class="au-button"
        disabled={busy}
        on:click={() => (workdaySetup = false)}>{uiTranslate("取消", $uiLanguage)}</button
      ><button class="au-button primary" disabled={busy} on:click={applyWorkday}
        >{uiTranslate(busy ? "正在生成…" : "生成自动化", $uiLanguage)}</button
      >
    </footer>
  </Modal>{/if}
