<script lang="ts">
  import { dismissibleDropdown } from "../../dismissibleDropdown";
  import AppSelect from "../../components/AppSelect.svelte";
  import { t } from "../../localization";
  import { translate as uiTranslate, language as uiLanguage } from "../../i18n";
  import { onMount, tick } from "svelte";
  import { Dialog } from "bits-ui";
  import {
    CaretDown,
    Check,
    Copy,
    FileText,
    Info,
    Lightning,
    MagnifyingGlass,
    PencilSimple,
    Play,
    Plus,
    PuzzlePiece,
    SquaresFour,
    Trash,
    WarningCircle,
    X,
  } from "phosphor-svelte";

  import ActionIcon from "../../ActionIcon.svelte";
  import InstalledAppIcon from "../../InstalledAppIcon.svelte";
  import { SubscriptionScope } from "../../subscriptions";
  import { bridge } from "../../bridge";
  import { captureActionShortcut } from "../../actionShortcutCapture";
  import { translate } from "../../i18n";
  import { actionIconCatalog, sanitizeImportedSvg } from "../../quickActionIcons";
  import type { ActionPreset, ActionView, AppSettings, BootstrapState, InstalledApp } from "../../types";
  import {
    actionContentTitle,
    actionDescription,
    actionFromDraft,
    actionSummary,
    actionTypeOptions,
    draftFromAction,
    emptyActionDraft,
    errorMessage,
    mediaOperationLabel,
    shortcutLabel,
    systemOperationLabel,
    type ActionDraft,
  } from "../../app_helpers";

  type ConfirmationRequest = {
    title: string;
    description: string;
    confirmLabel: string;
    kind: "danger" | "primary";
    onConfirm: () => Promise<boolean | void> | boolean | void;
  };

  export let snapshot: BootstrapState | null = null;
  export let language: AppSettings["language"] = "zhCn";
  export let shortcutModifier = "Ctrl";
  export let notify: (message: string, kind?: "success" | "error") => void;
  export let confirm: (request: ConfirmationRequest) => void;
  export let onSnapshot: (state: BootstrapState) => void;

  let search = "";
  let searchInput: HTMLInputElement;
  let actionNameInput: HTMLInputElement;
  let globalShortcutInput: HTMLInputElement;
  let focusGlobalShortcut = false;
  let actionSaving = false;
  let actionSaveError = "";
  let shortcutChecking = false;
  let shortcutValidationRevision = 0;
  let shortcutCaptureError = "";
  let shortcutConflict = "";
  let iconFileInput: HTMLInputElement;
  let selectedActionId = "";
  let actionOutput: string[] = [];
  let busyActionId = "";
  let actionEditorOpen = false;
  let presetPickerOpen = false;
  let transferOpen = false;
  let actionDraft: ActionDraft = emptyActionDraft();
  let actionPresets: ActionPreset[] = [];
  let installedApps: InstalledApp[] = [];
  let appsLoading = false;
  let appPickerOpen = false;
  let appSearch = "";
  let appSearchInput: HTMLInputElement;
  let presetSearch = "";
  let transferMode: "import" | "export" = "import";
  let actionText = "";
  let transferBusy = false;
  let actions: ActionView[] = [];
  let filteredActions: ActionView[] = [];
  let selectedAction: ActionView | undefined;
  let groups: string[] = [];
  let filteredPresets: ActionPreset[] = [];
  let filteredInstalledApps: InstalledApp[] = [];
  let selectedInstalledApp: InstalledApp | undefined;

  $: actions = snapshot?.actions ?? [];
  $: filteredActions = actions.filter((action) => {
    const query = search.trim().toLocaleLowerCase("zh-CN");
    return !query || action.name.toLocaleLowerCase("zh-CN").includes(query)
      || action.group.toLocaleLowerCase("zh-CN").includes(query)
      || action.actionTypeLabel.toLocaleLowerCase("zh-CN").includes(query);
  });
  $: selectedAction = actions.find((action) => action.id === selectedActionId);
  $: shortcutError = shortcutCaptureError || shortcutConflict;
  $: void checkGlobalShortcut(actionEditorOpen, actionDraft.id, actionDraft.globalShortcut);
  $: groups = [...new Set(actions.map((action) => action.group))];
  $: filteredPresets = actionPresets.filter((preset) => {
    const query = presetSearch.trim().toLocaleLowerCase("zh-CN");
    return !query || preset.name.toLocaleLowerCase("zh-CN").includes(query)
      || preset.group.toLocaleLowerCase("zh-CN").includes(query)
      || preset.description.toLocaleLowerCase("zh-CN").includes(query);
  });
  $: selectedInstalledApp = installedApps.find((app) => app.path === actionDraft.appPath);
  $: filteredInstalledApps = installedApps.filter((app) => {
    const query = appSearch.trim().toLocaleLowerCase("zh-CN");
    return !query || app.name.toLocaleLowerCase("zh-CN").includes(query)
      || app.version?.toLocaleLowerCase("zh-CN").includes(query)
      || app.identifier?.toLocaleLowerCase("zh-CN").includes(query)
      || app.path.toLocaleLowerCase("zh-CN").includes(query);
  });
  $: if (!selectedActionId || !actions.some((action) => action.id === selectedActionId)) {
    selectedActionId = actions[0]?.id ?? "";
    void loadOutput(selectedActionId);
  }

  onMount(() => {
    const scope = new SubscriptionScope();
    void scope.add(bridge.onActionOutput(applyOutput)).catch((error) => notify(String(error), "error"));
    return () => {
      scope.dispose();
    };
  });

  function tr(source: string) { return translate(source, language); }

  async function selectAction(actionId: string) {
    selectedActionId = actionId;
    await loadOutput(actionId);
  }

  let outputActionId = "";
  let outputRevision = -1;
  function applyOutput(value: import("../../types").ActionOutputEvent) {
    if (value.actionId !== selectedActionId) return;
    if (outputActionId === value.actionId && value.revision <= outputRevision) return;
    outputActionId = value.actionId; outputRevision = value.revision; actionOutput = value.lines;
  }
  async function loadOutput(actionId: string) {
    if (!actionId) { actionOutput = []; return; }
    applyOutput(await bridge.getActionOutput(actionId));
  }

  function openNewAction() {
    shortcutCaptureError = "";
    actionSaveError = "";
    focusGlobalShortcut = false;
    actionDraft = emptyActionDraft(actions.length);
    actionEditorOpen = true;
    void loadInstalledApps(false);
  }

  function openEditAction() {
    if (!selectedAction) return;
    shortcutCaptureError = "";
    actionSaveError = "";
    focusGlobalShortcut = false;
    actionDraft = draftFromAction(selectedAction);
    actionEditorOpen = true;
    if (actionDraft.type === "LaunchApp") void loadInstalledApps(false);
  }

  function openShortcutSettings() {
    openEditAction();
    focusGlobalShortcut = true;
  }

  async function loadInstalledApps(refresh: boolean) {
    if (appsLoading || (!refresh && installedApps.length > 0)) return;
    appsLoading = true;
    try { installedApps = await bridge.listInstalledApps(refresh); }
    catch (error) { notify(errorMessage(error), "error"); }
    finally { appsLoading = false; }
  }

  async function openInstalledAppPicker() {
    appPickerOpen = true;
    appSearch = "";
    await tick();
    appSearchInput?.focus();
  }

  function selectInstalledApp(app: InstalledApp) {
    actionDraft.appPath = app.path;
    actionDraft.appName = app.name;
    if (!actionDraft.name) actionDraft.name = `打开 ${app.name}`;
    selectActionIcon("folder");
    appPickerOpen = false;
  }

  function selectActionIcon(iconId: string) {
    const icon = actionIconCatalog.find((item) => item.id === iconId);
    if (!icon) return;
    actionDraft.iconId = icon.id;
    actionDraft.iconSvg = icon.svg;
  }

  async function importActionIcon(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    try {
      actionDraft.iconId = "custom";
      actionDraft.iconSvg = sanitizeImportedSvg(await file.text());
      notify(`已导入图标“${file.name}”`);
    } catch (error) { notify(errorMessage(error), "error"); }
  }

  async function openPresetPicker() {
    presetSearch = "";
    actionPresets = await bridge.getActionPresets();
    presetPickerOpen = true;
  }

  async function installPreset(preset: ActionPreset) {
    if (preset.installed) return;
    try {
      onSnapshot(await bridge.installActionPreset(preset.id));
      actionPresets = await bridge.getActionPresets();
      notify(`已添加“${preset.name}”`);
    } catch (error) { notify(errorMessage(error), "error"); }
  }

  function openImport() {
    transferMode = "import";
    actionText = "";
    transferOpen = true;
  }

  async function openExport() {
    transferMode = "export";
    transferBusy = true;
    transferOpen = true;
    try { actionText = await bridge.exportActionsText(); }
    catch (error) { notify(errorMessage(error), "error"); }
    finally { transferBusy = false; }
  }

  async function copyActionText() {
    try { await navigator.clipboard.writeText(actionText); notify(tr("动作文本已复制")); }
    catch (error) { notify(errorMessage(error), "error"); }
  }

  async function importActionText() {
    if (!actionText.trim()) return;
    transferBusy = true;
    try {
      const response = await bridge.importActionsText(actionText);
      onSnapshot(response.state);
      actionPresets = await bridge.getActionPresets();
      transferOpen = false;
      notify(`已导入 ${response.result.importedCount} 个动作`);
    } catch (error) { notify(errorMessage(error), "error"); }
    finally { transferBusy = false; }
  }

  function focusActionName(event: Event) {
    event.preventDefault();
    (focusGlobalShortcut ? globalShortcutInput : actionNameInput)?.focus();
  }

  async function checkGlobalShortcut(open: boolean, actionId: string, shortcut: string) {
    const revision = ++shortcutValidationRevision;
    shortcutConflict = "";
    shortcutChecking = open && !!shortcut;
    if (!shortcutChecking) return;
    try {
      await bridge.validateActionShortcut(actionId, shortcut);
    } catch (error) {
      if (revision === shortcutValidationRevision) shortcutConflict = errorMessage(error);
    } finally {
      if (revision === shortcutValidationRevision) shortcutChecking = false;
    }
  }

  function clearGlobalShortcut() {
    shortcutCaptureError = "";
    actionSaveError = "";
    actionDraft.globalShortcut = "";
    globalShortcutInput?.focus();
  }

  function captureGlobalShortcut(event: KeyboardEvent) {
    const captured = captureActionShortcut(event);
    if (captured.type === "navigate") return;
    event.preventDefault();
    event.stopPropagation();
    if (captured.type === "ignore") return;
    if (captured.type === "clear") {
      clearGlobalShortcut();
      return;
    }
    actionSaveError = "";
    shortcutCaptureError = captured.type === "error" ? captured.message : "";
    if (captured.type === "shortcut") actionDraft.globalShortcut = captured.value;
  }

  async function saveAction() {
    if (actionSaving || shortcutChecking || shortcutError) return;
    actionSaving = true;
    actionSaveError = "";
    try {
      const state = await bridge.saveAction(actionFromDraft(actionDraft));
      onSnapshot(state);
      selectedActionId = actionDraft.id || state.actions.at(-1)?.id || selectedActionId;
      actionEditorOpen = false;
      notify(tr("动作已保存"));
    } catch (error) { actionSaveError = errorMessage(error); }
    finally { actionSaving = false; }
  }

  function deleteSelectedAction() {
    if (!selectedAction) return;
    const action = selectedAction;
    confirm({
      title: tr("删除动作？"),
      description: t("“{name}”将被永久删除，此操作无法撤销。", language, { name: action.name }),
      confirmLabel: tr("确认删除"),
      kind: "danger",
      onConfirm: async () => { onSnapshot(await bridge.deleteAction(action.id)); notify(tr("动作已删除")); },
    });
  }

  function runAction(action: ActionView) {
    if (action.requiresConfirmation) {
      confirm({
        title: tr("运行需确认的动作？"),
        description: t("“{name}”会改变当前系统状态，请确认是否继续执行。", language, { name: action.name }),
        confirmLabel: tr("确认运行"),
        kind: "primary",
        onConfirm: () => { void executeAction(action); },
      });
      return;
    }
    void executeAction(action);
  }

  async function executeAction(action: ActionView) {
    busyActionId = action.id;
    try { notify(await bridge.executeAction(action.id) || tr("动作已执行")); await loadOutput(action.id); }
    catch (error) { notify(errorMessage(error), "error"); }
    finally { busyActionId = ""; }
  }

  function handleShortcut(event: KeyboardEvent) {
    if (event.defaultPrevented || actionEditorOpen || presetPickerOpen || transferOpen) return;
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      searchInput?.focus();
    }
  }

  function actionContentDescription(action: ActionView) {
    if (action.action_type.type !== "System") return actionDescription(action);
    const descriptions = {
      lock_screen: "立即锁定当前电脑屏幕",
      display_sleep: "关闭当前显示器并保持电脑继续运行",
      sleep: "让当前电脑进入睡眠状态",
      screenshot_full: "截取当前屏幕的完整画面",
      screenshot_region: "选择屏幕区域并截取画面",
      shutdown: "关闭当前电脑",
      restart: "重新启动当前电脑",
    } satisfies Record<typeof action.action_type.operation, string>;
    return descriptions[action.action_type.operation];
  }

  function actionContentIsCode(action: ActionView) {
    return action.action_type.type === "ShellCommand"
      || action.action_type.type === "AppleScript"
      || action.action_type.type === "ToggleShellCommand";
  }
</script>

<svelte:window on:keydown={handleShortcut} />
    <div class="actions-layout">
      <main class="content actions-content">
        <header class="page-header actions-header">
          <div class="actions-title-row">
            <div class="actions-title-copy">
              <div>
                <h1>{uiTranslate("快捷动作", $uiLanguage)}</h1>
                <span class="action-count-pill">{filteredActions.length}</span>
              </div>
              <p>{uiTranslate("运行常用操作，或从预设库添加更多跨平台动作。", $uiLanguage)}</p>
            </div>
            <button class="primary-button create-action-button" on:click={openNewAction}>
              <Plus size={18} weight="bold" />
              {uiTranslate("新建动作", $uiLanguage)}
            </button>
          </div>

          <div class="actions-toolbar">
            <label class="search-box">
              <MagnifyingGlass size={19} />
              <input bind:this={searchInput} bind:value={search} placeholder={uiTranslate("搜索动作", $uiLanguage)} />
              <kbd>{shortcutModifier} K</kbd>
            </label>
            <div class="utility-actions">
              <button class="secondary-button" on:click={openPresetPicker}>
                <PuzzlePiece size={17} /> {uiTranslate("预设库", $uiLanguage)}
              </button>
              <button class="secondary-button" on:click={openImport}>{uiTranslate("导入", $uiLanguage)}</button>
              <button class="secondary-button" on:click={openExport}>{uiTranslate("导出", $uiLanguage)}</button>
            </div>
          </div>
        </header>

        <div class="actions-scroll">
          <section class="action-table" aria-label={uiTranslate("快捷动作列表", $uiLanguage)} role="table">
            <div class="table-heading" role="row">
              <span role="columnheader">{uiTranslate("动作名称", $uiLanguage)}</span>
              <span role="columnheader">{uiTranslate("动作类型", $uiLanguage)}</span>
              <span role="columnheader">{uiTranslate("所属分组", $uiLanguage)}</span>
              <span role="columnheader">{tr("全局快捷键")}</span>
              <span role="columnheader">{uiTranslate("操作", $uiLanguage)}</span>
            </div>

            <div class="action-rows-scroll" role="rowgroup">
              {#if filteredActions.length === 0}
                <div class="empty-state">
                  <MagnifyingGlass size={28} />
                  <strong>{uiTranslate("没有找到动作", $uiLanguage)}</strong>
                  <span>{uiTranslate("换一个关键词，或者新建一个动作。", $uiLanguage)}</span>
                </div>
              {:else}
                {#each filteredActions as action (action.id)}
                  <div
                    class="action-row"
                    class:selected={selectedActionId === action.id}
                    role="row"
                  >
                    <div class="action-name-cell" role="cell">
                      <button type="button" class="action-name-cell action-select-button" aria-pressed={selectedActionId === action.id} on:click={() => selectAction(action.id)}>
                        <span class="action-icon" style:--icon-color={action.color}>
                          <ActionIcon iconId={action.icon_id} iconSvg={action.icon_svg} size={25} color={action.color} />
                        </span>
                        <span>
                          <strong>{action.name}</strong>
                          <small>{uiTranslate(action.actionTypeLabel, $uiLanguage)}</small>
                        </span>
                      </button>
                    </div>
                    <span class="muted-cell" role="cell">{uiTranslate(action.actionTypeLabel, $uiLanguage)}</span>
                    <span class="muted-cell" role="cell"><span class="group-chip">{action.group}</span></span>
                    <span class="shortcut-cell" role="cell">
                      {#if action.global_shortcut}<kbd>{uiTranslate(shortcutLabel(action, shortcutModifier === "⌘" ? "⌘" : "Super"), $uiLanguage)}</kbd>{:else}—{/if}
                    </span>
                    <div role="cell">
                      <button
                        class="run-icon-button"
                        class:running={action.isRunning}
                        aria-label={uiTranslate((`运行 ${action.name}`), $uiLanguage)}
                        disabled={busyActionId === action.id}
                        on:click|stopPropagation={() => runAction(action)}
                      >
                        <Play size={16} weight="fill" />
                      </button>
                    </div>
                  </div>
                {/each}
              {/if}
            </div>
          </section>
        </div>
      </main>

      <aside class="inspector">
        {#if selectedAction}
          <div class="inspector-heading">
            <strong>{selectedAction.name}</strong>
            <button aria-label={uiTranslate("取消选择", $uiLanguage)} on:click={() => (selectedActionId = "")}><X size={18} /></button>
          </div>

          <div class="inspector-scroll action-inspector-scroll">
            <div class="inspector-hero">
              <div class="inspector-summary">
                <span class="large-action-icon" style:--icon-color={selectedAction.color}>
                  <ActionIcon iconId={selectedAction.icon_id} iconSvg={selectedAction.icon_svg} size={30} color={selectedAction.color} />
                </span>
                <div>
                  <span class:running={selectedAction.isRunning} class="status-chip">
                    {uiTranslate(selectedAction.isRunning ? "运行中" : "就绪", $uiLanguage)}
                  </span>
                  <p>{uiTranslate(actionDescription(selectedAction), $uiLanguage)}</p>
                </div>
              </div>
              <button class="outline-run-button" on:click={() => runAction(selectedAction)}>
                <Play size={17} weight="fill" />
                {uiTranslate(selectedAction.isRunning ? "停止动作" : "运行动作", $uiLanguage)}
              </button>
            </div>

            <div class="action-inspector-details">
              <section class="action-inspector-section">
                <h2 class="action-inspector-section-title">
                  <Info size={16} weight="bold" />
                  {uiTranslate("基本信息", $uiLanguage)}
                </h2>
                <dl class="detail-list action-detail-list">
                  <div><dt>{uiTranslate("动作类型", $uiLanguage)}</dt><dd>{uiTranslate(selectedAction.actionTypeLabel, $uiLanguage)}</dd></div>
                  <div><dt>{uiTranslate("所属分组", $uiLanguage)}</dt><dd>{selectedAction.group}</dd></div>
                  <div>
                    <dt>{tr("全局快捷键")}</dt>
                    <dd>
                      <button class="action-shortcut-button" on:click={openShortcutSettings} title={uiTranslate((tr("设置全局快捷键")), $uiLanguage)}>
                        {#if selectedAction.global_shortcut}<kbd>{uiTranslate(shortcutLabel(selectedAction, shortcutModifier === "⌘" ? "⌘" : "Super"), $uiLanguage)}</kbd>{:else}<Plus size={14} />{tr("添加全局快捷键")}{/if}
                      </button>
                    </dd>
                  </div>
                  <div>
                    <dt>{uiTranslate("运行状态", $uiLanguage)}</dt>
                    <dd class="action-detail-status">
                      <span class="activity-dot" aria-hidden="true"></span>
                      {uiTranslate(selectedAction.isRunning ? "运行中" : "就绪", $uiLanguage)}
                    </dd>
                  </div>
                </dl>
                {#if selectedAction.globalShortcutError}
                  <p class="action-shortcut-error" role="status"><WarningCircle size={16} />{selectedAction.globalShortcutError}</p>
                {/if}
              </section>

              <section class="action-inspector-section action-content-section">
                <h2 class="action-inspector-section-title">
                  <FileText size={16} weight="bold" />
                  {uiTranslate(actionContentTitle(selectedAction), $uiLanguage)}
                </h2>
                <div class="action-content-well" class:code-content={actionContentIsCode(selectedAction)}>
                  <pre>{actionSummary(selectedAction)}</pre>
                  <p>{uiTranslate(actionContentDescription(selectedAction), $uiLanguage)}</p>
                </div>
              </section>

              {#if actionOutput.length > 0}
                <section class="action-inspector-section action-output-section">
                  <h2 class="action-inspector-section-title">{uiTranslate("最近输出", $uiLanguage)}</h2>
                  <pre>{actionOutput.slice(-5).join("\n")}</pre>
                </section>
              {/if}
            </div>
          </div>

          <div class="inspector-footer">
            <button class="secondary-button" on:click={openEditAction}>
              <PencilSimple size={17} /> {uiTranslate("编辑", $uiLanguage)}
            </button>
            <button class="danger-button" on:click={deleteSelectedAction}>
              <Trash size={17} /> {uiTranslate("删除", $uiLanguage)}
            </button>
          </div>
        {:else}
          <div class="inspector-empty">
            <Lightning size={28} />
            <strong>{uiTranslate("选择一个动作", $uiLanguage)}</strong>
            <span>{uiTranslate("查看详细信息、测试或编辑动作。", $uiLanguage)}</span>
          </div>
        {/if}
      </aside>
    </div>
<Dialog.Root bind:open={actionEditorOpen}>
  <Dialog.Portal>
    <Dialog.Overlay class="modal-backdrop" />
    <Dialog.Content class="modal action-modal" onOpenAutoFocus={focusActionName}>
      <header class="modal-header">
        <div>
          <Dialog.Title class="modal-title" level={2}>{uiTranslate(actionDraft.id ? "编辑动作" : "新建动作", $uiLanguage)}</Dialog.Title>
          <Dialog.Description class="modal-description">{uiTranslate("配置一个可由桌面端或移动端运行的本地动作。", $uiLanguage)}</Dialog.Description>
        </div>
        <Dialog.Close class="modal-close-button" aria-label={uiTranslate("关闭", $uiLanguage)}><X size={19} /></Dialog.Close>
      </header>
      <div class="modal-body action-modal-body">
        <div class="form-grid two-fields">
          <label><span>{uiTranslate("动作名称", $uiLanguage)}</span><input bind:this={actionNameInput} bind:value={actionDraft.name} placeholder={uiTranslate("例如：启动开发环境", $uiLanguage)} /></label>
          <label><span>{uiTranslate("所属分组", $uiLanguage)}</span><input bind:value={actionDraft.group} list="groups" placeholder={uiTranslate("常用", $uiLanguage)} /></label>
          <datalist id="groups">{#each groups as group}<option value={group}></option>{/each}</datalist>
        </div>
        <div class="action-global-shortcut-field">
          <label for="action-global-shortcut">{tr("全局快捷键（可选）")}</label>
          <div class="action-global-shortcut-control">
            <input id="action-global-shortcut" bind:this={globalShortcutInput} value={actionDraft.globalShortcut ? shortcutLabel({ global_shortcut: actionDraft.globalShortcut }, shortcutModifier === "⌘" ? "⌘" : "Super") : ""} placeholder={tr("点击后按下组合键")} readonly on:keydown={captureGlobalShortcut} aria-invalid={!!shortcutError} aria-describedby="action-global-shortcut-hint action-global-shortcut-status" disabled={actionSaving} />
            <button type="button" class="secondary-button compact-button" disabled={(!actionDraft.globalShortcut && !shortcutError) || actionSaving} on:click={clearGlobalShortcut}>{tr("清除")}</button>
          </div>
          <small id="action-global-shortcut-hint">{tr("点击后直接按下组合键，需包含 Ctrl、Alt 或 Command / Super。按 Backspace / Delete 或点击“清除”取消绑定；保存时检查系统或其他应用占用。")}</small>
          <div id="action-global-shortcut-status" aria-live="polite">
            {#if shortcutError}<p class="action-shortcut-error"><WarningCircle size={16} />{tr(shortcutError)}</p>
            {:else if shortcutChecking}<small>{tr("正在检测快捷键冲突…")}</small>{/if}
          </div>
        </div>
        <div class="form-grid action-settings-grid">
          <label>
            <span>{uiTranslate("动作类型", $uiLanguage)}</span>
            <AppSelect bind:value={actionDraft.type}
              aria-label={uiTranslate("动作类型", $uiLanguage)}
              placeholder={uiTranslate("选择动作类型", $uiLanguage)}
              options={actionTypeOptions.map((option) => ({ ...option, label: uiTranslate(option.label, $uiLanguage) }))}
            />
          </label>
          <label class="color-field">
            <span>{uiTranslate("强调色", $uiLanguage)}</span>
            <span class="color-control">
              <input class="color-input" type="color" bind:value={actionDraft.color} aria-label={uiTranslate("选择强调色", $uiLanguage)} />
              <code>{actionDraft.color.toUpperCase()}</code>
            </span>
          </label>
        </div>

        <section class="action-icon-field">
          <div class="action-icon-field-heading">
            <div>
              <strong>{uiTranslate("动作图标", $uiLanguage)}</strong>
              <small>{uiTranslate("桌面端与移动端会渲染同一份 SVG。", $uiLanguage)}</small>
            </div>
            <button class="secondary-button compact-button" type="button" on:click={() => iconFileInput.click()}>
              {uiTranslate("导入 SVG", $uiLanguage)}
            </button>
            <input
              class="visually-hidden"
              bind:this={iconFileInput}
              type="file"
              accept=".svg,image/svg+xml"
              on:change={importActionIcon}
            />
          </div>
          <div class="action-icon-picker" role="list" aria-label={uiTranslate("内置动作图标", $uiLanguage)}>
            {#each actionIconCatalog as icon (icon.id)}
              <button
                type="button"
                class:selected={actionDraft.iconId === icon.id}
                aria-label={icon.label}
                title={icon.label}
                on:click={() => selectActionIcon(icon.id)}
              >
                <ActionIcon iconId={icon.id} iconSvg={icon.svg} size={21} color={actionDraft.color} />
              </button>
            {/each}
          </div>
          {#if actionDraft.iconId === "custom"}
            <div class="custom-icon-status">
              <span class="custom-icon-preview" style:--icon-color={actionDraft.color}>
                <ActionIcon iconId={actionDraft.iconId} iconSvg={actionDraft.iconSvg} size={22} color={actionDraft.color} />
              </span>
              <span><strong>{uiTranslate("自定义 SVG", $uiLanguage)}</strong><small>{uiTranslate("已通过安全检查，将随快捷动作同步。", $uiLanguage)}</small></span>
            </div>
          {/if}
        </section>

        <div class="action-specific-fields">
          {#if actionDraft.type === "LaunchApp"}
            <div class="full-field">
              <span>{uiTranslate("已安装应用", $uiLanguage)}</span>
              <div class="app-picker" use:dismissibleDropdown={{ open: appPickerOpen, close: () => (appPickerOpen = false) }}>
                <button
                  class="app-picker-trigger"
                  class:open={appPickerOpen}
                  type="button"
                  aria-haspopup="listbox"
                  aria-expanded={appPickerOpen}
                  on:click={() => appPickerOpen ? (appPickerOpen = false) : openInstalledAppPicker()}
                >
                  {#if selectedInstalledApp}
                    <span class="app-icon app-icon-large">
                      <InstalledAppIcon path={selectedInstalledApp.path} name={selectedInstalledApp.name} initialIcon={selectedInstalledApp.iconDataUrl} size={28} />
                    </span>
                    <span class="app-picker-trigger-copy">
                      <strong>{selectedInstalledApp.name}</strong>
                      <small>{uiTranslate(selectedInstalledApp.version ? `版本 ${selectedInstalledApp.version}` : selectedInstalledApp.path, $uiLanguage)}</small>
                    </span>
                  {:else}
                    <span class="app-picker-placeholder-icon"><SquaresFour size={19} /></span>
                    <span class="app-picker-trigger-copy">
                      <strong>{uiTranslate(appsLoading ? "正在扫描应用…" : "选择一个应用", $uiLanguage)}</strong>
                      <small>{uiTranslate(appsLoading ? "正在读取应用名称和图标" : "支持按名称、版本或路径搜索", $uiLanguage)}</small>
                    </span>
                  {/if}
                  <CaretDown class="app-picker-caret" size={17} weight="bold" />
                </button>

                {#if appPickerOpen}
                  <div class="app-picker-popover">
                    <div class="app-picker-search">
                      <MagnifyingGlass size={17} />
                      <input
                        bind:this={appSearchInput}
                        bind:value={appSearch}
                        placeholder={uiTranslate("搜索应用", $uiLanguage)}
                        aria-label={uiTranslate("搜索已安装应用", $uiLanguage)}
                      />
                      {#if appSearch}
                        <button
                          type="button"
                          aria-label={uiTranslate("清除搜索", $uiLanguage)}
                          on:mousedown|preventDefault
                          on:click={() => (appSearch = "")}
                        ><X size={15} /></button>
                      {/if}
                    </div>
                    <div class="app-picker-results" role="listbox" aria-label={uiTranslate("已安装应用", $uiLanguage)}>
                      {#if appsLoading}
                        <div class="app-picker-state"><span class="loading-spinner"></span><span>{uiTranslate("正在扫描已安装应用…", $uiLanguage)}</span></div>
                      {:else if filteredInstalledApps.length === 0}
                        <div class="app-picker-state"><MagnifyingGlass size={22} /><span>{uiTranslate("没有找到“", $uiLanguage)}{appSearch}”</span><small>{uiTranslate("可在下方直接填写应用路径", $uiLanguage)}</small></div>
                      {:else}
                        {#each filteredInstalledApps as app (app.path)}
                          <button
                            type="button"
                            class="app-picker-option"
                            class:selected={app.path === actionDraft.appPath}
                            role="option"
                            aria-selected={app.path === actionDraft.appPath}
                            on:click={() => selectInstalledApp(app)}
                          >
                            <span class="app-icon">
                              <InstalledAppIcon path={app.path} name={app.name} initialIcon={app.iconDataUrl} size={24} />
                            </span>
                            <span class="app-picker-option-copy">
                              <strong>{app.name}</strong>
                              <small>{uiTranslate(app.version ? `版本 ${app.version}` : app.identifier ?? app.path, $uiLanguage)}</small>
                            </span>
                            {#if app.path === actionDraft.appPath}<Check size={18} weight="bold" />{/if}
                          </button>
                        {/each}
                      {/if}
                    </div>
                    <div class="app-picker-footer">
                      <span>{uiTranslate("共", $uiLanguage)} {installedApps.length} {uiTranslate("个应用", $uiLanguage)}</span>
                      <button type="button" on:click={() => loadInstalledApps(true)}>{uiTranslate("重新扫描", $uiLanguage)}</button>
                    </div>
                  </div>
                {/if}
              </div>
            </div>
            <div class="inline-field-actions">
              <small>{uiTranslate("应用列表和图标来自系统安装信息。", $uiLanguage)}</small>
            </div>
            <label class="full-field">
              <span>{uiTranslate("自定义应用路径（可选）", $uiLanguage)}</span>
              <input bind:value={actionDraft.appPath} placeholder={uiTranslate("绿色版或未被扫描到的应用路径", $uiLanguage)} />
            </label>
          {:else if actionDraft.type === "OpenPath"}
            <label class="full-field"><span>{uiTranslate("文件或目录路径", $uiLanguage)}</span><input bind:value={actionDraft.path} placeholder={uiTranslate("~/Downloads 或 C:\\Users\\name\\Downloads", $uiLanguage)} /></label>
          {:else if actionDraft.type === "OpenUrl"}
            <label class="full-field"><span>URL</span><input bind:value={actionDraft.url} placeholder="https://example.com" /></label>
          {:else if actionDraft.type === "Hotkey"}
            <div class="form-grid two-fields">
              <label><span>{uiTranslate("修饰键", $uiLanguage)}</span><input bind:value={actionDraft.modifiers} placeholder="command, shift" /></label>
              <label><span>{uiTranslate("按键", $uiLanguage)}</span><input bind:value={actionDraft.key} placeholder="P" /></label>
            </div>
          {:else if actionDraft.type === "AppleScript"}
            <label class="full-field"><span>AppleScript</span><textarea bind:value={actionDraft.script} rows="6" placeholder="tell application…"></textarea></label>
          {:else if actionDraft.type === "System"}
            <label class="full-field">
              <span>{uiTranslate("系统操作", $uiLanguage)}</span>
              <AppSelect bind:value={actionDraft.systemOperation} aria-label={uiTranslate("系统操作", $uiLanguage)}
                options={[
                  { value: "lock_screen", label: uiTranslate("锁定电脑", $uiLanguage) },
                  { value: "display_sleep", label: uiTranslate("关闭显示器", $uiLanguage) },
                  { value: "sleep", label: uiTranslate("电脑睡眠", $uiLanguage) },
                  { value: "screenshot_full", label: uiTranslate("全屏截图", $uiLanguage) },
                  { value: "screenshot_region", label: uiTranslate("区域截图", $uiLanguage) },
                  { value: "shutdown", label: uiTranslate("关闭电脑", $uiLanguage) },
                  { value: "restart", label: uiTranslate("重新启动", $uiLanguage) },
                ]}
              />
            </label>
          {:else if actionDraft.type === "Media"}
            <label class="full-field">
              <span>{uiTranslate("媒体操作", $uiLanguage)}</span>
              <AppSelect bind:value={actionDraft.mediaOperation} aria-label={uiTranslate("媒体操作", $uiLanguage)}
                options={[
                  { value: "toggle_play_pause", label: uiTranslate("播放 / 暂停", $uiLanguage) },
                  { value: "play", label: uiTranslate("播放", $uiLanguage) },
                  { value: "pause", label: uiTranslate("暂停", $uiLanguage) },
                  { value: "previous", label: uiTranslate("上一曲", $uiLanguage) },
                  { value: "next", label: uiTranslate("下一曲", $uiLanguage) },
                  { value: "seek_backward", label: uiTranslate("快退", $uiLanguage) },
                  { value: "seek_forward", label: uiTranslate("快进", $uiLanguage) },
                ]}
              />
            </label>
          {:else if actionDraft.type === "SetSystemVolume"}
            <label class="full-field"><span>{uiTranslate("系统音量：", $uiLanguage)}{actionDraft.volume}%</span><input type="range" min="0" max="100" step="1" bind:value={actionDraft.volume} /></label>
          {:else if actionDraft.type === "SetSystemMuted"}
            <label class="full-field">
              <span>{uiTranslate("声音状态", $uiLanguage)}</span>
              <AppSelect bind:value={actionDraft.muted} aria-label={uiTranslate("声音状态", $uiLanguage)}
                options={[
                  { value: true, label: uiTranslate("静音", $uiLanguage) },
                  { value: false, label: uiTranslate("取消静音", $uiLanguage) },
                ]}
              />
            </label>
          {:else if actionDraft.type === "SetMicrophone"}
            <label class="full-field">
              <span>{uiTranslate("麦克风状态", $uiLanguage)}</span>
              <AppSelect bind:value={actionDraft.microphoneActive} aria-label={uiTranslate("麦克风状态", $uiLanguage)}
                options={[
                  { value: false, label: uiTranslate("静音", $uiLanguage) },
                  { value: true, label: uiTranslate("开启", $uiLanguage) },
                ]}
              />
            </label>
          {:else if actionDraft.type === "ToggleShellCommand"}
            <label class="full-field"><span>{uiTranslate("启动命令", $uiLanguage)}</span><textarea bind:value={actionDraft.startCommand} rows="3"></textarea></label>
            <label class="full-field"><span>{uiTranslate("停止命令（可选）", $uiLanguage)}</span><textarea bind:value={actionDraft.stopCommand} rows="2"></textarea></label>
            <label class="full-field"><span>{uiTranslate("工作目录（可选）", $uiLanguage)}</span><input bind:value={actionDraft.workingDir} /></label>
          {:else}
            <label class="full-field"><span>{uiTranslate("Shell 命令", $uiLanguage)}</span><textarea bind:value={actionDraft.command} rows="3" placeholder="npm run dev"></textarea></label>
            <label class="full-field"><span>{uiTranslate("工作目录（可选）", $uiLanguage)}</span><input bind:value={actionDraft.workingDir} placeholder="~/projects/my-app" /></label>
          {/if}
        </div>
        <label class="confirmation-field">
          <input type="checkbox" bind:checked={actionDraft.confirmBeforeRun} />
          <span>{uiTranslate("执行前要求确认（关机和重启会强制启用）", $uiLanguage)}</span>
        </label>
      </div>
      <footer class="modal-footer">
        {#if actionSaveError}<p class="action-shortcut-error action-save-error" role="alert"><WarningCircle size={16} />{tr(actionSaveError)}</p>{/if}
        <Dialog.Close class="secondary-button">{uiTranslate("取消", $uiLanguage)}</Dialog.Close>
        <button class="primary-button" disabled={actionSaving || shortcutChecking || !!shortcutError} on:click={saveAction}>{uiTranslate(actionSaving ? tr("保存中…") : tr("保存动作"), $uiLanguage)}</button>
      </footer>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={presetPickerOpen}>
  <Dialog.Portal>
    <Dialog.Overlay class="modal-backdrop" />
    <Dialog.Content class="modal preset-modal">
      <header class="modal-header">
        <div>
          <Dialog.Title class="modal-title" level={2}>{uiTranslate("通用动作预设", $uiLanguage)}</Dialog.Title>
          <Dialog.Description class="modal-description">{uiTranslate("这些动作使用系统原生能力，不依赖特定第三方应用。", $uiLanguage)}</Dialog.Description>
        </div>
        <Dialog.Close class="modal-close-button" aria-label={uiTranslate("关闭", $uiLanguage)}><X size={19} /></Dialog.Close>
      </header>
      <div class="modal-body preset-modal-body">
        <label class="preset-search">
          <MagnifyingGlass size={18} />
          <input bind:value={presetSearch} placeholder={uiTranslate("搜索预设", $uiLanguage)} />
        </label>
        <div class="preset-grid">
          {#each filteredPresets as preset (preset.id)}
            <article class="preset-card" class:installed={preset.installed}>
              <span class="preset-card-icon" style:--preset-color={preset.color}>
                <ActionIcon iconId={preset.iconId} iconSvg={preset.iconSvg} size={22} color={preset.color} />
              </span>
              <div>
                <strong>{preset.name}</strong>
                <small>{preset.group} · {uiTranslate(preset.description, $uiLanguage)}</small>
              </div>
              <button
                class={preset.installed ? "secondary-button" : "primary-button"}
                disabled={preset.installed}
                on:click={() => installPreset(preset)}
              >
                {uiTranslate(preset.installed ? "已添加" : "添加", $uiLanguage)}
              </button>
            </article>
          {/each}
        </div>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={transferOpen}>
  <Dialog.Portal>
    <Dialog.Overlay class="modal-backdrop" />
    <Dialog.Content class="modal transfer-modal">
      <header class="modal-header">
        <div>
          <Dialog.Title class="modal-title" level={2}>{uiTranslate(transferMode === "import" ? "导入动作文本" : "导出动作文本", $uiLanguage)}</Dialog.Title>
          <Dialog.Description class="modal-description">
            {uiTranslate(transferMode === "import"
              ? "粘贴来自 GitHub 或其他来源的 ArcRelay JSON。导入前请检查其中的脚本和命令。"
              : "复制这段 JSON 保存为文本文件，或分享到 GitHub。", $uiLanguage)}
          </Dialog.Description>
        </div>
        <Dialog.Close class="modal-close-button" aria-label={uiTranslate("关闭", $uiLanguage)}><X size={19} /></Dialog.Close>
      </header>
      <div class="modal-body">
        <textarea
          class="action-textarea"
          bind:value={actionText}
          readonly={transferMode === "export"}
          rows="18"
          placeholder={uiTranslate("粘贴 ArcRelay JSON 动作文本", $uiLanguage)}
        ></textarea>
        {#if transferMode === "import"}
          <p class="security-note"><WarningCircle size={16} /> {uiTranslate("Shell、AppleScript 和自定义程序路径拥有本机执行权限，只导入可信内容。", $uiLanguage)}</p>
        {/if}
      </div>
      <footer class="modal-footer">
        <Dialog.Close class="secondary-button">{uiTranslate("取消", $uiLanguage)}</Dialog.Close>
        {#if transferMode === "export"}
          <button class="primary-button" disabled={transferBusy || !actionText} on:click={copyActionText}><Copy size={17} /> {uiTranslate("复制文本", $uiLanguage)}</button>
        {:else}
          <button class="primary-button" disabled={transferBusy || !actionText.trim()} on:click={importActionText}>{uiTranslate("导入动作", $uiLanguage)}</button>
        {/if}
      </footer>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
