<script lang="ts">
  import AppSelect from "../../components/AppSelect.svelte";
  import { translate as uiTranslate, language as uiLanguage } from "../../i18n";
  import {
    ArrowsOutCardinal,
    Crosshair,
    FloppyDisk,
    Minus,
    MinusCircle,
    Monitor,
    Ruler,
    Selection,
    TestTube,
    Plus,
  } from "phosphor-svelte";
  import type { RuntimeSnapshot, WorkspaceConfiguration } from "../../types";
  import { bridge } from "../../bridge";
  import { friendlyDeviceName, portalEdgeTestRequest } from "./inputSharingUi";
  import { moveDisplayGroup, resizeDisplay as resizeDisplaySurface } from "./topologyGeometry";
  import { onDestroy } from "svelte";
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
  let saving = false;
  let addingDisplayId = "";
  let previewGeneration = 0;
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  onDestroy(() => { previewGeneration++; clearTimeout(previewTimer); });

  $: if (!dirty && snapshot.configuration !== configuration) configuration = structuredClone(snapshot.configuration);
  $: layout = configuration.layout;
  $: selectedDisplay = selectedDisplayId && layout ? layout.displays[selectedDisplayId] : null;
  $: availableDisplays = Object.values(configuration.rememberedDisplays).filter((display) => !layout?.displays[display.displayId]);
  $: deviceNames = Object.fromEntries(
    [...new Set(Object.values(layout?.displays ?? {}).map((display) => display.deviceId))]
      .map((deviceId) => [deviceId, friendlyDeviceName(deviceId, snapshot)]),
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

  function selectDisplayForSizing() {
    mode = "select";
    selectedPortalId = null;
    selectedDisplayId = selectedDisplayId ?? Object.keys(layout?.displays ?? {})[0] ?? null;
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
      } catch (error) { if (generation === previewGeneration) notify(String(error), true); }
    }, 80);
  }

  function behaviorChanged() { geometryChanged(); }

  function addDisplay() {
    if (!addingDisplayId) return;
    configuration = addWorkspaceDisplay(configuration, addingDisplayId);
    selectedDisplayId = addingDisplayId;
    selectedPortalId = null;
    addingDisplayId = "";
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
    try { const next = await bridge.arrangeWorkspace(configuration); if (generation === previewGeneration) { configuration = next; dirty = true; } }
    catch (error) { notify(String(error), true); }
  }

  async function save() {
    previewGeneration++;
    clearTimeout(previewTimer);
    saving = true;
    try {
      const next = await bridge.saveWorkspace(configuration);
      onSnapshot(next);
      configuration = structuredClone(next.configuration);
      selectedPortalId = null;
      dirty = false;
      notify("工作区布局已保存");
    } catch (error) {
      notify(String(error), true);
    } finally {
      saving = false;
    }
  }

  async function testEdge(displayId = selectedDisplayId, portalId: string | null = null) {
    if (!layout || !displayId) return notify("请先选择显示器或跨屏边缘", true);
    const candidates = layout.portals.flatMap((portal) => {
      if (portal.status !== "Active" || (portalId && portal.portalId !== portalId)) return [];
      if (portal.sourceDisplay === displayId) return [{ portal, reverse: false }];
      if (portal.direction === "Bidirectional" && portal.targetDisplay === displayId) return [{ portal, reverse: true }];
      return [];
    });
    if (!candidates.length) return notify("这块屏幕没有可测试的已启用边缘", true);
    try {
      for (const candidate of candidates) {
        const request = portalEdgeTestRequest(candidate.portal, layout.displays, candidate.reverse);
        if (!request) continue;
        const result = await bridge.testEdge(request, configuration);
        if (result.portalIds.includes(candidate.portal.portalId)) {
          notify(`边缘测试成功，已进入 ${layout.displays[result.displayId]?.name ?? "目标屏幕"}`);
          return;
        }
      }
      notify("测试移动没有穿越边缘，请检查区段或激活策略", true);
    } catch (error) {
      notify(`边缘测试失败：${String(error)}`, true);
    }
  }

  function testSelectedPortal() {
    const portal = layout?.portals.find((value) => value.portalId === selectedPortalId);
    void testEdge(portal?.sourceDisplay ?? null, portal?.portalId ?? null);
  }
</script>

<section class="page">
  <div class="page-toolbar">
    <div class="tool-switch" aria-label={uiTranslate("布局工具", $uiLanguage)}>
      <button class:active={mode === "select"} on:click={() => mode = "select"}><Selection size={17} weight={mode === "select" ? "fill" : "regular"} />{uiTranslate("选择", $uiLanguage)}</button>
      <button class:active={mode === "test"} on:click={() => mode = "test"}><TestTube size={17} weight={mode === "test" ? "fill" : "regular"} />{uiTranslate("测试", $uiLanguage)}</button>
    </div>
    <div class="actions">
      <label class="add-screen">
        <span class="sr-only">{uiTranslate("添加屏幕", $uiLanguage)}</span>
        <AppSelect bind:value={addingDisplayId} disabled={!availableDisplays.length} aria-label={uiTranslate("添加屏幕", $uiLanguage)}
          options={[
            { value: "", label: uiTranslate(availableDisplays.length ? "选择要添加的屏幕" : "所有已知屏幕均已添加", $uiLanguage) },
            ...availableDisplays.map((display) => ({ value: display.displayId, label: [(friendlyDeviceName(display.deviceId, snapshot)), " · ", (display.name)].join("") })),
          ]}
        />
      </label>
      <button class="secondary-action" disabled={!addingDisplayId} on:click={addDisplay}><Plus size={17} />{uiTranslate("添加屏幕", $uiLanguage)}</button>
      <button class="secondary-action" on:click={autoArrange}><ArrowsOutCardinal size={17} />{uiTranslate("自动排列", $uiLanguage)}</button>
      <button class:active={Boolean(selectedDisplayId)} class="secondary-action" on:click={selectDisplayForSizing}><Ruler size={17} />{uiTranslate("校准尺寸", $uiLanguage)}</button>
      <button class="save-action" disabled={!dirty || saving} on:click={save}><FloppyDisk size={17} />{uiTranslate(saving ? "保存中…" : "保存布局", $uiLanguage)}</button>
    </div>
  </div>

  {#if layout}
    <div class="workspace-row">
      <div class="canvas card">
        <WorkspaceCanvas
          {layout}
          displayAvailability={snapshot.displayAvailability}
          controller={snapshot.controller}
          localServiceInstanceId={snapshot.serviceInstanceId}
          {deviceNames}
          {mode}
          {zoom}
          {selectedDisplayId}
          {selectedPortalId}
          onSelectDisplay={(id: string) => { selectedDisplayId = id; selectedPortalId = null; if (mode === "test") void testEdge(id); }}
          onSelectPortal={(id: string) => {
            selectedPortalId = id;
            selectedDisplayId = null;
            if (mode === "test") {
              const portal = layout?.portals.find((value) => value.portalId === id);
              void testEdge(portal?.sourceDisplay ?? null, id);
            }
          }}
          onMoveDisplay={moveDisplay}
          onResizeDisplay={resizeDisplay}
        />
        {#if selectedDisplay}
          <div class="display-controls" role="group" aria-label={selectedDisplay.name} on:pointerdown={(event) => event.stopPropagation()}>
            <span class="display-control-icon"><Monitor size={18} weight="duotone" /></span>
            <span class="display-control-copy"><strong>{selectedDisplay.name}</strong><small>{uiTranslate("实际尺寸", $uiLanguage)} · {Math.round(selectedDisplay.deskRectUm.width / 1_000)} × {Math.round(selectedDisplay.deskRectUm.height / 1_000)} mm</small></span>
            <button aria-label={`${uiTranslate("缩小", $uiLanguage)} ${selectedDisplay.name}`} title={uiTranslate("缩小", $uiLanguage)} on:click={() => scaleSelectedDisplay(.9)}><Minus size={16} /></button>
            <button aria-label={`${uiTranslate("放大", $uiLanguage)} ${selectedDisplay.name}`} title={uiTranslate("放大", $uiLanguage)} on:click={() => scaleSelectedDisplay(1.1)}><Plus size={16} /></button>
            <button class="remove-display" aria-label={uiTranslate("从布局移除屏幕", $uiLanguage)} title={uiTranslate("从布局移除屏幕", $uiLanguage)} on:click={() => removeDisplay(selectedDisplay.displayId)}><MinusCircle size={17} /></button>
          </div>
        {:else if selectedPortalId}
          <div class="inspector-overlay">
            <PortalInspector
              {layout}
              portalId={selectedPortalId}
              onChange={behaviorChanged}
              onFinish={() => { selectedPortalId = null; mode = "select"; }}
              onTest={testSelectedPortal}
            />
          </div>
        {/if}
        <div class="canvas-footer"><span><Crosshair size={16} />{uiTranslate(mode === "select" ? "同一设备的屏幕按系统排列整体移动；设备边缘靠近后会自动连通" : "点击屏幕或高亮边段验证穿越结果", $uiLanguage)}</span><div><button aria-label={uiTranslate("缩小", $uiLanguage)} disabled={zoom <= .7} on:click={() => zoom = Math.max(.7, +(zoom - .1).toFixed(1))}>−</button><strong>{Math.round(zoom * 100)}%</strong><button aria-label={uiTranslate("放大", $uiLanguage)} disabled={zoom >= 1.4} on:click={() => zoom = Math.min(1.4, +(zoom + .1).toFixed(1))}>＋</button></div></div>
      </div>
    </div>
  {:else}
    <div class="empty card"><span><ArrowsOutCardinal size={38} weight="duotone" /></span><h2>{uiTranslate("尚未检测到显示器", $uiLanguage)}</h2><p>{uiTranslate("连接设备或授予系统权限后，显示器会自动加入工作区。", $uiLanguage)}</p></div>
  {/if}
</section>

<style>
  .add-screen :global(.app-select-trigger) { min-height: 39px; max-width: 220px; border: 1px solid var(--border-strong); border-radius: 9px; padding: 0 10px; color: var(--text); background: var(--surface); }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
  .actions { flex-wrap: wrap; }
  .tool-switch { flex-shrink: 0; }
  .tool-switch button, .secondary-action, .save-action { white-space: nowrap; }
  .page { min-height: 560px; height: 100%; display: flex; flex-direction: column; gap: 14px; padding-bottom: 24px; }
  .page-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 16px; }.tool-switch { display: inline-flex; padding: 4px; border: 1px solid var(--border); border-radius: 11px; background: var(--surface-soft); }.tool-switch button { display: inline-flex; min-height: 36px; align-items: center; gap: 7px; border: 0; border-radius: 8px; padding: 0 13px; color: var(--text-secondary); background: transparent; }.tool-switch button.active { color: var(--accent-strong); background: var(--accent-soft); box-shadow: 0 1px 3px #10182810; }.actions { display: flex; gap: 8px; }.secondary-action, .save-action { display: inline-flex; min-height: 39px; align-items: center; gap: 7px; border-radius: 9px; padding: 0 13px; font-weight: 650; }.secondary-action { border: 1px solid var(--border-strong); color: var(--text); background: var(--surface); }.save-action { border: 1px solid var(--accent); color: #fff; background: var(--accent); }.save-action:disabled { border-color: var(--border-strong); color: var(--text-muted); background: var(--surface-sunken); }
  .workspace-row { min-height: 0; flex: 1; }.canvas { position: relative; width: 100%; height: 100%; min-width: 0; overflow: hidden; padding: 8px; }.display-controls { position: absolute; top: 18px; right: 18px; display: flex; min-width: 330px; align-items: center; gap: 8px; padding: 8px; border: 1px solid var(--border); border-radius: 12px; background: color-mix(in srgb,var(--surface) 94%,transparent); box-shadow: var(--shadow-card); backdrop-filter: blur(12px); }.display-control-icon { display: grid; width: 34px; height: 34px; place-items: center; flex: none; border-radius: 9px; color: var(--accent); background: var(--accent-soft); }.display-control-copy { display: grid; min-width: 0; flex: 1; gap: 2px; }.display-control-copy strong, .display-control-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.display-control-copy strong { font-size: 11px; }.display-control-copy small { color: var(--text-muted); font-size: 9px; }.display-controls button { display: grid; width: 32px; height: 32px; place-items: center; flex: none; border: 1px solid var(--border); border-radius: 8px; color: var(--text-secondary); background: var(--surface); }.display-controls button:hover, .secondary-action.active { border-color: color-mix(in srgb,var(--accent) 45%,var(--border)); color: var(--accent-strong); background: var(--accent-soft); }.display-controls button.remove-display { color: var(--danger); }.inspector-overlay { position: absolute; z-index: 3; top: 18px; right: 18px; bottom: 64px; width: 322px; min-height: 0; }.inspector-overlay :global(.inspector) { box-sizing: border-box; width: 100%; height: 100%; background: color-mix(in srgb,var(--surface) 96%,transparent); box-shadow: 0 12px 36px #10182824; backdrop-filter: blur(14px); }.canvas-footer { position: absolute; right: 18px; bottom: 16px; left: 18px; display: flex; align-items: center; justify-content: space-between; gap: 14px; pointer-events: none; }.canvas-footer > span { display: flex; align-items: center; gap: 7px; padding: 7px 10px; border: 1px solid var(--border); border-radius: 8px; color: var(--text-secondary); background: color-mix(in srgb,var(--surface) 92%,transparent); font-size: 10px; }.canvas-footer > div { display: flex; align-items: center; overflow: hidden; border: 1px solid var(--border); border-radius: 9px; background: var(--surface); pointer-events: auto; }.canvas-footer button, .canvas-footer strong { display: grid; min-width: 34px; height: 34px; place-items: center; border: 0; color: var(--text); background: transparent; font-size: 11px; }.canvas-footer strong { border-inline: 1px solid var(--border); font-weight: 550; }
  .empty { flex: 1; display: grid; place-content: center; justify-items: center; gap: 10px; text-align: center; }.empty > span { display: grid; width: 64px; height: 64px; place-items: center; border-radius: 17px; color: var(--accent); background: var(--accent-soft); }.empty h2 { margin: 0; }.empty p { margin: 0; color: var(--text-secondary); font-size: 12px; }
  @media (max-width: 980px) { .page-toolbar { align-items: flex-start; flex-direction: column; }.actions { width: 100%; flex-wrap: wrap; }.display-controls { right: 12px; left: 12px; min-width: 0; }.inspector-overlay { right: 12px; left: 12px; width: auto; }.canvas-footer > span { max-width: calc(100% - 120px); } }
</style>
