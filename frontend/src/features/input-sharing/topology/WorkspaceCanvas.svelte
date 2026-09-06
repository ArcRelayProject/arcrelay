<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../../../i18n";
  import { onDestroy } from "svelte";
  import type { DisplaySurface as DisplayType, WorkspaceLayout, RuntimeSnapshot } from "../../../types";
  import { displayStatusLabel } from "../workspaceMembership";
  import { snapDisplayPosition } from "../topologyGeometry";
  import DeviceGroup from "./DeviceGroup.svelte";
  import DisplaySurface from "./DisplaySurface.svelte";
  import PortalOverlay from "./PortalOverlay.svelte";

  export let layout: WorkspaceLayout;
  export let displayAvailability: RuntimeSnapshot["displayAvailability"] = {};
  export let controller: string | null = null;
  export let localServiceInstanceId = "";
  export let deviceNames: Record<string, string> = {};
  export let mode: "select" | "test" = "select";
  export let selectedDisplayId: string | null = null;
  export let selectedPortalId: string | null = null;
  export let zoom = 1;
  export let onSelectDisplay: (id: string) => void;
  export let onSelectPortal: (id: string) => void;
  export let onMoveDisplay: (id: string, x: number, y: number) => void;
  export let onResizeDisplay: (id: string, width: number) => void;

  const padding = 72;
  let width = 0;
  let height = 0;
  let drag: { id: string; startX: number; startY: number; originalX: number; originalY: number } | null = null;
  let resize: { id: string; startX: number; startY: number; originalWidth: number; originalHeight: number; projectScale: number } | null = null;

  $: displayList = Object.values(layout.displays);
  $: renderList = selectedDisplayId
    ? [...displayList].sort((left, right) => Number(left.displayId === selectedDisplayId) - Number(right.displayId === selectedDisplayId))
    : displayList;
  $: minX = Math.min(...displayList.map((display) => display.deskRectUm.x), 0);
  $: minY = Math.min(...displayList.map((display) => display.deskRectUm.y), 0);
  $: maxX = Math.max(...displayList.map((display) => display.deskRectUm.x + display.deskRectUm.width), 1);
  $: maxY = Math.max(...displayList.map((display) => display.deskRectUm.y + display.deskRectUm.height), 1);
  $: measured = width > padding * 2 && height > padding * 2;
  $: scale = measured
    ? Math.min((width - padding * 2) / (maxX - minX), (height - padding * 2) / (maxY - minY)) * zoom
    : 0;
  $: deviceGroups = groupDisplays(displayList, deviceNames, displayAvailability);

  function observeCanvas(node: HTMLElement) {
    let frame = 0;
    const update = () => {
      frame = 0;
      width = node.clientWidth;
      height = node.clientHeight;
    };
    const scheduleUpdate = () => {
      if (frame) cancelAnimationFrame(frame);
      frame = requestAnimationFrame(update);
    };
    const observer = new ResizeObserver(scheduleUpdate);
    observer.observe(node);
    update();
    scheduleUpdate();
    return {
      destroy() {
        observer.disconnect();
        if (frame) cancelAnimationFrame(frame);
      },
    };
  }

  function groupDisplays(displays: DisplayType[], names: Record<string, string>, availability: RuntimeSnapshot["displayAvailability"]) {
    const groups = new Map<string, DisplayType[]>();
    for (const display of displays) {
      const values = groups.get(display.deviceId) ?? [];
      values.push(display);
      groups.set(display.deviceId, values);
    }
    return [...groups].map(([deviceId, values], index) => ({
      deviceId,
      label: names[deviceId] ?? (deviceId === localServiceInstanceId ? "这台 Mac" : index === 0 && !localServiceInstanceId ? "这台 Mac" : "远程电脑"),
      x: Math.min(...values.map((display) => display.deskRectUm.x)),
      y: Math.min(...values.map((display) => display.deskRectUm.y)),
      online: values.some((display) => availability[display.displayId] === "Ready"),
    }));
  }

  function project(x: number, y: number) {
    return { x: padding + (x - minX) * scale, y: padding + (y - minY) * scale };
  }

  function beginDrag(event: PointerEvent, display: DisplayType) {
    if (mode !== "select") {
      onSelectDisplay(display.displayId);
      return;
    }
    event.preventDefault();
    onSelectDisplay(display.displayId);
    drag = { id: display.displayId, startX: event.clientX, startY: event.clientY, originalX: display.deskRectUm.x, originalY: display.deskRectUm.y };
    window.addEventListener("pointermove", moveDrag);
    window.addEventListener("pointerup", endDrag, { once: true });
  }

  function moveDrag(event: PointerEvent) {
    if (!drag) return;
    const snap = 5_000;
    const nextX = Math.round((drag.originalX + (event.clientX - drag.startX) / scale) / snap) * snap;
    const nextY = Math.round((drag.originalY + (event.clientY - drag.startY) / scale) / snap) * snap;
    const visualSnapUm = Math.min(25_000, Math.max(5_000, 14 / Math.max(scale, Number.EPSILON)));
    const snapped = snapDisplayPosition(drag.id, nextX, nextY, layout.displays, visualSnapUm);
    onMoveDisplay(drag.id, snapped.x, snapped.y);
  }

  function endDrag() {
    window.removeEventListener("pointermove", moveDrag);
    window.removeEventListener("pointerup", endDrag);
    drag = null;
  }

  function beginResize(event: PointerEvent, display: DisplayType) {
    if (mode !== "select" || scale <= 0) return;
    event.preventDefault();
    onSelectDisplay(display.displayId);
    resize = {
      id: display.displayId,
      startX: event.clientX,
      startY: event.clientY,
      originalWidth: display.deskRectUm.width,
      originalHeight: display.deskRectUm.height,
      projectScale: scale,
    };
    window.addEventListener("pointermove", moveResize);
    window.addEventListener("pointerup", endResize, { once: true });
  }

  function moveResize(event: PointerEvent) {
    if (!resize) return;
    const deltaX = (event.clientX - resize.startX) / resize.projectScale;
    const deltaY = (event.clientY - resize.startY) / resize.projectScale;
    const aspect = resize.originalWidth / resize.originalHeight;
    const widthFromX = resize.originalWidth + deltaX;
    const widthFromY = (resize.originalHeight + deltaY) * aspect;
    const requestedWidth = Math.abs(deltaX) >= Math.abs(deltaY * aspect) ? widthFromX : widthFromY;
    onResizeDisplay(resize.id, requestedWidth);
  }

  function endResize() {
    window.removeEventListener("pointermove", moveResize);
    window.removeEventListener("pointerup", endResize);
    resize = null;
  }

  onDestroy(() => {
    window.removeEventListener("pointermove", moveDrag);
    window.removeEventListener("pointerup", endDrag);
    window.removeEventListener("pointermove", moveResize);
    window.removeEventListener("pointerup", endResize);
  });

  function isOnline(display: DisplayType) {
    return displayAvailability[display.displayId] === "Ready";
  }
</script>

<div class="canvas" use:observeCanvas aria-busy={!measured}>
  {#if measured}
    <svg viewBox={`0 0 ${width} ${height}`} aria-label={uiTranslate("物理桌面工作区", $uiLanguage)}>
      <defs>
        <pattern id="grid" width="20" height="20" patternUnits="userSpaceOnUse">
          <path d="M 20 0 L 0 0 0 20" fill="none" stroke="currentColor" stroke-opacity=".08" />
        </pattern>
      </defs>
      <rect width={width} height={height} fill="url(#grid)" />
      {#each deviceGroups as group}
        {@const groupPoint = project(group.x, group.y)}
        <DeviceGroup name={`${group.label}${group.online ? " · 在线" : " · 离线"}`} x={groupPoint.x} y={groupPoint.y - 18} width={180} />
      {/each}
      {#each layout.portals as portal (portal.portalId)}
        <PortalOverlay {portal} displays={layout.displays} {project} selected={selectedPortalId === portal.portalId} onselect={() => onSelectPortal(portal.portalId)} />
      {/each}
      {#each renderList as display (display.displayId)}
        {@const point = project(display.deskRectUm.x, display.deskRectUm.y)}
        <DisplaySurface
          {display}
          x={point.x}
          y={point.y}
          width={Math.max(display.deskRectUm.width * scale, 90)}
          height={Math.max(display.deskRectUm.height * scale, 60)}
          selected={selectedDisplayId === display.displayId}
          controller={controller === display.deviceId}
          online={isOnline(display)}
          statusLabel={displayStatusLabel(displayAvailability[display.displayId])}
          onpointerdown={(event) => beginDrag(event, display)}
          onresizepointerdown={(event) => beginResize(event, display)}
          onselect={() => onSelectDisplay(display.displayId)}
        />
      {/each}
    </svg>
  {/if}
</div>

<style>
  .canvas { position: relative; width: 100%; height: 100%; min-height: 470px; overflow: hidden; border-radius: 14px; color: var(--text-secondary, #344054); background: var(--surface-soft, #f8f9fc); }
  svg { width: 100%; height: 100%; display: block; }
</style>
