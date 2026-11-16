<script lang="ts">
  import { applyLayoutDraft } from "./workspaceDraft";
  import { diagnosticMessage } from "../../diagnosticMessages";
  import { t, translate as uiTranslate, language as uiLanguage } from "../../i18n";
  import AppSelect from "../../components/AppSelect.svelte";
  import { Popover } from "bits-ui";
  import InputMenu from "./InputMenu.svelte";
  import {
    ArrowsOutCardinal,
    CornersOut,
    Hand,
    X,
    Minus,
    MinusCircle,
    Monitor,
    Ruler,
    TestTube,
    Plus,
  } from "phosphor-svelte";
  import type { RuntimeSnapshot, WorkspaceConfiguration } from "../../types";
  import { bridge } from "../../bridge";
  import { friendlyDeviceName, portalEdgeTestRequest } from "./inputSharingUi";
  import { moveDisplayGroup, resizeDisplay as resizeDisplaySurface } from "./topologyGeometry";
  import { onDestroy, tick } from "svelte";
  import PortalInspector from "./topology/PortalInspector.svelte";
  import WorkspaceCanvas from "./topology/WorkspaceCanvas.svelte";
  import { addWorkspaceDisplay, removeWorkspaceDisplay } from "./workspaceMembership";

  export let snapshot: RuntimeSnapshot;
  export let onSnapshot: (snapshot: RuntimeSnapshot) => void;
  export let notify: (message: string, error?: boolean) => void;

  let configuration: WorkspaceConfiguration = structuredClone(snapshot.configuration);
  let selectedDisplayId: string | null = null;
  let selectedPortalId: string | null = null;
  let mode: "select" | "test" = "select";
  let zoom = 1;
  let dirty = false;
  export let saving = false;
  export let inputBusy = false;
  let addingDisplayId = "";
  let addOpen = false;
  let sizeOpen = false;
  let canvas: WorkspaceCanvas;
  let selectedBounds: { x: number; y: number; width: number; height: number } | null = null;
  function updateSelectionBounds(bounds: typeof selectedBounds) {
    selectedBounds = bounds;
  }
  export function hasUnsavedChanges() {
    return dirty;
  }
  async function cancel() {
    previewGeneration++;
    clearTimeout(previewTimer);
    configuration = structuredClone(snapshot.configuration);
    dirty = false;
    selectedDisplayId = null;
    selectedPortalId = null;
    sizeOpen = false;
    await tick();
    canvas?.fitToView();
  }
  function clearSelection() {
    selectedDisplayId = null;
    selectedPortalId = null;
    sizeOpen = false;
  }
  function selectDisplay(id: string) {
    if (selectedDisplayId !== id) sizeOpen = false;
    selectedDisplayId = id;
    selectedPortalId = null;
  }
  function restoreSize() {
    if (!selectedDisplay) return;
    const original =
      snapshot.configuration.layout?.displays[selectedDisplay.displayId] ??
      snapshot.configuration.rememberedDisplays[selectedDisplay.displayId];
    if (original) resizeDisplay(selectedDisplay.displayId, original.physicalSizeUm.width);
  }

  let previewGeneration = 0;
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  onDestroy(() => {
    previewGeneration++;
    clearTimeout(previewTimer);
  });

  $: if (!dirty && snapshot.configuration !== configuration)
    configuration = structuredClone(snapshot.configuration);
  $: layout = configuration.layout;
  $: selectedDisplay = selectedDisplayId && layout ? layout.displays[selectedDisplayId] : null;
  $: availableDisplays = Object.values(configuration.rememberedDisplays).filter(
    (display) => !layout?.displays[display.displayId],
  );
  $: deviceNames = Object.fromEntries(
    [...new Set(Object.values(layout?.displays ?? {}).map((display) => display.deviceId))].map(
      (deviceId) => [deviceId, friendlyDeviceName(deviceId, snapshot, $uiLanguage)],
    ),
  );

  function moveDisplay(id: string, x: number, y: number) {
    if (!layout) return;
    moveDisplayGroup(id, x, y, layout.displays);
    geometryChanged();
  }

  function resizeDisplay(id: string, width: number) {
    if (!layout || !resizeDisplaySurface(id, width, layout.displays)) return;
    geometryChanged();
  }

  function scaleSelectedDisplay(factor: number) {
    if (!selectedDisplay) return;
    resizeDisplay(selectedDisplay.displayId, selectedDisplay.deskRectUm.width * factor);
  }

  function geometryChanged() {
    if (!layout) return;
    configuration = structuredClone(configuration);
    dirty = true;
    const generation = ++previewGeneration;
    clearTimeout(previewTimer);
    previewTimer = setTimeout(async () => {
      try {
        const next = await bridge.previewWorkspace(configuration);
        if (generation === previewGeneration) configuration = next;
      } catch (error) {
        if (generation === previewGeneration) notify(diagnosticMessage(error, $uiLanguage), true);
      }
    }, 80);
  }

  function behaviorChanged() {
    geometryChanged();
  }

  function addDisplay() {
    if (!addingDisplayId) return;
    configuration = addWorkspaceDisplay(configuration, addingDisplayId);
    selectedDisplayId = addingDisplayId;
    selectedPortalId = null;
    addingDisplayId = "";
    addOpen = false;
    geometryChanged();
  }

  function removeDisplay(id: string) {
    configuration = removeWorkspaceDisplay(configuration, id);
    selectedDisplayId = null;
    selectedPortalId = null;
    geometryChanged();
  }

  async function autoArrange() {
    const generation = ++previewGeneration;
    clearTimeout(previewTimer);
    try {
      const next = await bridge.arrangeWorkspace(configuration);
      if (generation === previewGeneration) {
        configuration = next;
        dirty = true;
      }
    } catch (error) {
      notify(diagnosticMessage(error, $uiLanguage), true);
    }
  }

  async function save() {
    if (saving || inputBusy) return;
    previewGeneration++;
    clearTimeout(previewTimer);
    saving = true;
    try {
      const next = await bridge.saveWorkspace(
        applyLayoutDraft(snapshot.configuration, configuration),
      );
      onSnapshot(next);
      configuration = structuredClone(next.configuration);
      selectedPortalId = null;
      dirty = false;
      notify(uiTranslate("工作区布局已保存", $uiLanguage));
    } catch (error) {
      notify(diagnosticMessage(error, $uiLanguage), true);
    } finally {
      saving = false;
    }
  }

  async function testEdge(displayId = selectedDisplayId, portalId: string | null = null) {
    if (!layout || !displayId)
      return notify(uiTranslate("请先选择显示器或跨屏边缘", $uiLanguage), true);
    const candidates = layout.portals.flatMap((portal) => {
      if (portal.status !== "Active" || (portalId && portal.portalId !== portalId)) return [];
      if (portal.sourceDisplay === displayId) return [{ portal, reverse: false }];
      if (portal.direction === "Bidirectional" && portal.targetDisplay === displayId)
        return [{ portal, reverse: true }];
      return [];
    });
    if (!candidates.length)
      return notify(uiTranslate("这块屏幕没有可测试的已启用边缘", $uiLanguage), true);
    try {
      for (const candidate of candidates) {
        const request = portalEdgeTestRequest(candidate.portal, layout.displays, candidate.reverse);
        if (!request) continue;
        const result = await bridge.testEdge(request, configuration);
        if (result.portalIds.includes(candidate.portal.portalId)) {
          notify(
            t("边缘测试成功，已进入 {part0}", $uiLanguage, {
              part0: layout.displays[result.displayId]?.name ?? "目标屏幕",
            }),
          );
          return;
        }
      }
      notify(uiTranslate("测试移动没有穿越边缘，请检查区段或激活策略", $uiLanguage), true);
    } catch (error) {
      notify(
        t("边缘测试失败：{part0}", $uiLanguage, { part0: diagnosticMessage(error, $uiLanguage) }),
        true,
      );
    }
  }

  function testSelectedPortal() {
    const portal = layout?.portals.find((value) => value.portalId === selectedPortalId);
    void testEdge(portal?.sourceDisplay ?? null, portal?.portalId ?? null);
  }
</script>

<section class="page" aria-label={uiTranslate("屏幕排列", $uiLanguage)}>
  <div class="page-toolbar">
    <h2>{uiTranslate("屏幕排列", $uiLanguage)}</h2>
    <div class="actions">
      <Popover.Root bind:open={addOpen}>
        <Popover.Trigger class="input-secondary"
          ><Plus size={17} />{uiTranslate("添加屏幕", $uiLanguage)}</Popover.Trigger
        >
        <Popover.Portal
          ><Popover.Content
            class="input-popover add-display-popover"
            sideOffset={8}
            align="end"
            collisionPadding={12}
          >
            <h3>{uiTranslate("添加屏幕", $uiLanguage)}</h3>
            <AppSelect
              bind:value={addingDisplayId}
              disabled={!availableDisplays.length}
              aria-label={uiTranslate("选择要添加的屏幕", $uiLanguage)}
              options={[
                {
                  value: "",
                  label: uiTranslate(
                    availableDisplays.length ? "选择要添加的屏幕" : "所有已知屏幕均已添加",
                    $uiLanguage,
                  ),
                },
                ...availableDisplays.map((display) => ({
                  value: display.displayId,
                  label: `${friendlyDeviceName(display.deviceId, snapshot, $uiLanguage)} · ${display.name}`,
                })),
              ]}
            />
            <button class="input-primary" disabled={!addingDisplayId} on:click={addDisplay}
              >{uiTranslate("添加屏幕", $uiLanguage)}</button
            >
          </Popover.Content></Popover.Portal
        >
      </Popover.Root>
      <InputMenu
        label={uiTranslate("布局更多选项", $uiLanguage)}
        items={[
          {
            label: uiTranslate("自动排列", $uiLanguage),
            icon: ArrowsOutCardinal,
            action: () => {
              void autoArrange();
            },
          },
          {
            label: uiTranslate(mode === "test" ? "结束测试" : "测试边缘", $uiLanguage),
            icon: TestTube,
            action: () => {
              mode = mode === "test" ? "select" : "test";
              clearSelection();
            },
          },
        ]}
      />
    </div>
  </div>
  {#if layout && Object.keys(layout.displays).length}
    <div class="canvas-area">
      <WorkspaceCanvas
        bind:this={canvas}
        {layout}
        displayAvailability={snapshot.displayAvailability}
        controller={snapshot.controller}
        {deviceNames}
        {mode}
        bind:zoom
        {selectedDisplayId}
        {selectedPortalId}
        onSelectDisplay={(id) => {
          selectDisplay(id);
          if (mode === "test") void testEdge(id);
        }}
        onSelectPortal={(id) => {
          selectedPortalId = id;
          selectedDisplayId = null;
        }}
        onClearSelection={clearSelection}
        onSelectionBounds={updateSelectionBounds}
        onMoveDisplay={moveDisplay}
        onResizeDisplay={resizeDisplay}
      />
      {#if selectedDisplay && selectedBounds && mode === "select"}
        <div
          class="selection-toolbar"
          style:left={`clamp(150px, ${selectedBounds.x + selectedBounds.width / 2}px, calc(100% - 150px))`}
          style:top={`${Math.max(10, selectedBounds.y - 50)}px`}
        >
          <span title={selectedDisplay.name}
            >{uiTranslate("屏幕", $uiLanguage)}
            {Object.keys(layout.displays).indexOf(selectedDisplay.displayId) + 1}</span
          >
          <Popover.Root bind:open={sizeOpen}>
            <Popover.Trigger class="size-trigger"
              ><Ruler size={17} />{uiTranslate("尺寸", $uiLanguage)}</Popover.Trigger
            >
            <Popover.Portal
              ><Popover.Content
                class="input-popover"
                side="top"
                sideOffset={10}
                collisionPadding={12}
              >
                <h3>{uiTranslate("屏幕尺寸", $uiLanguage)}</h3>
                <p>{uiTranslate("调整图中的相对大小", $uiLanguage)}</p>
                <div class="size-controls">
                  <button
                    class="input-icon-button"
                    aria-label={uiTranslate("缩小屏幕", $uiLanguage)}
                    on:click={() => scaleSelectedDisplay(0.9)}><Minus size={16} /></button
                  ><output
                    >{Math.round(selectedDisplay.deskRectUm.width / 1000)} × {Math.round(
                      selectedDisplay.deskRectUm.height / 1000,
                    )} mm</output
                  ><button
                    class="input-icon-button"
                    aria-label={uiTranslate("放大屏幕", $uiLanguage)}
                    on:click={() => scaleSelectedDisplay(1.1)}><Plus size={16} /></button
                  >
                </div>
                <button class="input-text-button" on:click={restoreSize}
                  >{uiTranslate("恢复原始尺寸", $uiLanguage)}</button
                >
              </Popover.Content></Popover.Portal
            >
          </Popover.Root>
          <InputMenu
            label={uiTranslate("屏幕更多选项", $uiLanguage)}
            items={[
              {
                label: uiTranslate("测试边缘", $uiLanguage),
                icon: TestTube,
                action: () => {
                  void testEdge();
                },
              },
              {
                label: uiTranslate("从布局移除屏幕", $uiLanguage),
                icon: MinusCircle,
                danger: true,
                separator: true,
                action: () => removeDisplay(selectedDisplay!.displayId),
              },
            ]}
          />
          <button
            class="selection-close"
            aria-label={uiTranslate("取消选择", $uiLanguage)}
            on:click={clearSelection}><X size={14} /></button
          >
        </div>
      {:else if selectedPortalId}
        <div class="inspector-overlay">
          <PortalInspector
            {layout}
            portalId={selectedPortalId}
            onChange={behaviorChanged}
            onFinish={clearSelection}
            onTest={testSelectedPortal}
          />
        </div>
      {/if}
      <div class="canvas-footer">
        <span
          ><Hand size={16} />{uiTranslate(
            mode === "test" ? "点击屏幕或边缘进行测试" : "拖动设备，调整屏幕位置",
            $uiLanguage,
          )}</span
        >
        <div class="zoom-controls">
          <button
            aria-label={uiTranslate("缩小", $uiLanguage)}
            disabled={zoom <= 0.2}
            on:click={() => (zoom = Math.max(0.2, +(zoom - 0.1).toFixed(1)))}
            ><Minus size={15} /></button
          ><output>{Math.round(zoom * 100)}%</output><button
            aria-label={uiTranslate("放大", $uiLanguage)}
            disabled={zoom >= 3}
            on:click={() => (zoom = Math.min(3, +(zoom + 0.1).toFixed(1)))}
            ><Plus size={15} /></button
          ><button class="fit-button" on:click={() => canvas.fitToView()}
            ><CornersOut size={17} />{uiTranslate("显示全部", $uiLanguage)}</button
          >
        </div>
      </div>
    </div>
  {:else}
    <div class="empty">
      <Monitor size={36} />
      <h2>{uiTranslate("尚未检测到显示器", $uiLanguage)}</h2>
      <p>{uiTranslate("连接设备或授予系统权限后，显示器会自动加入工作区。", $uiLanguage)}</p>
    </div>
  {/if}
  <footer class="workspace-footer">
    <small><kbd>⌘ ⌥ ⇧ Esc</kbd> {uiTranslate("返回本机", $uiLanguage)}</small>{#if dirty}<div
        class="save-controls"
      >
        <span>{uiTranslate("未保存", $uiLanguage)}</span><button
          class="input-secondary"
          disabled={saving}
          on:click={cancel}>{uiTranslate("取消", $uiLanguage)}</button
        ><button class="input-primary" disabled={saving || inputBusy} on:click={save}
          >{uiTranslate(saving ? "保存中…" : "保存布局", $uiLanguage)}</button
        >
      </div>{/if}
  </footer>
</section>

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: 14px;
    height: 100%;
    min-height: 410px;
  }
  .page-toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }
  .page-toolbar h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 650;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  :global(.input-secondary),
  :global(.input-primary) {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    min-height: 36px;
    padding: 7px 13px;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    color: var(--text);
    background: var(--surface);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }
  :global(.input-primary) {
    color: white;
    background: var(--accent);
    border-color: var(--accent);
  }
  :global(.input-primary:disabled),
  :global(.input-secondary:disabled) {
    opacity: 0.45;
    cursor: default;
  }
  :global(.input-icon-button) {
    display: inline-grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--surface);
    color: var(--text);
    cursor: pointer;
  }
  :global(.input-text-button) {
    border: 0;
    padding: 0;
    color: var(--accent);
    background: transparent;
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }
  :global(.input-popover) {
    z-index: 1000;
    width: 280px;
    max-width: calc(100vw - 24px);
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: 12px;
    color: var(--text);
    background: var(--surface);
    box-shadow: 0 8px 28px #10182820;
    font: 14px var(--font-family-ui);
  }
  :global(.input-popover h3) {
    margin: 0;
    font-size: 15px;
  }
  :global(.input-popover p) {
    margin: 6px 0 14px;
    font-size: 13px;
    color: var(--text-secondary);
  }
  :global(.add-display-popover) {
    display: grid;
    gap: 12px;
    width: 330px;
  }
  :global(.size-controls) {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 15px 0;
  }
  :global(.size-controls output) {
    flex: 1;
    text-align: center;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  .canvas-area {
    flex: 1;
    min-height: 300px;
    position: relative;
    border: 1px solid var(--border);
    border-radius: 12px;
    overflow: hidden;
  }
  .selection-toolbar {
    position: absolute;
    z-index: 3;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px;
    max-width: calc(100% - 20px);
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    box-shadow: 0 3px 14px #10182816;
  }
  .selection-toolbar > span {
    padding: 0 10px;
    font-size: 12px;
    white-space: nowrap;
    color: var(--text-secondary);
  }
  :global(.size-trigger) {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 7px 10px;
    border: 0;
    border-left: 1px solid var(--border);
    color: var(--text);
    background: transparent;
    font-size: 13px;
  }
  .selection-close {
    display: grid;
    place-items: center;
    width: 25px;
    height: 30px;
    border: 0;
    border-left: 1px solid var(--border);
    color: var(--text-muted);
    background: transparent;
  }
  .inspector-overlay {
    position: absolute;
    z-index: 4;
    top: 12px;
    right: 12px;
    bottom: 64px;
    width: min(322px, calc(100% - 24px));
    overflow: auto;
    background: var(--surface);
    border-radius: 12px;
    box-shadow: 0 8px 28px #10182820;
  }
  .canvas-footer {
    position: absolute;
    bottom: 14px;
    left: 16px;
    right: 16px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    pointer-events: none;
  }
  .canvas-footer > span {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    color: var(--text-secondary);
    font-size: 12px;
  }
  .zoom-controls {
    display: flex;
    align-items: center;
    flex: none;
    padding: 3px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    pointer-events: auto;
  }
  .zoom-controls button,
  .zoom-controls output {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-width: 30px;
    height: 30px;
    padding: 0 7px;
    border: 0;
    color: var(--text);
    background: transparent;
    font-size: 12px;
  }
  .zoom-controls .fit-button {
    border-left: 1px solid var(--border);
    margin-left: 4px;
    padding-inline: 10px;
  }
  .workspace-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 46px;
    gap: 14px;
    padding-bottom: 12px;
  }
  .workspace-footer small {
    font-size: 11px;
    color: var(--text-secondary);
  }
  kbd {
    font: inherit;
    margin-right: 5px;
  }
  .save-controls {
    display: flex;
    align-items: center;
    gap: 9px;
  }
  .save-controls > span {
    margin-right: 15px;
    color: var(--text-muted);
    font-size: 12px;
  }
  .empty {
    flex: 1;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 10px;
    color: var(--text-secondary);
  }
  .empty h2 {
    font-size: 17px;
  }
  .empty p {
    font-size: 13px;
  }
  @media (max-width: 800px) {
    .canvas-footer > span {
      display: none;
    }
    .canvas-footer {
      justify-content: flex-end;
    }
    .save-controls > span {
      display: none;
    }
    .workspace-footer small {
      font-size: 10px;
    }
  }
</style>
