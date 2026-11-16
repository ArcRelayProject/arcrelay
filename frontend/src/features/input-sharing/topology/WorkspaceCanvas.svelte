<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../../../i18n";
  import { onDestroy } from "svelte";
  import type {
    DisplaySurface as DisplayType,
    WorkspaceLayout,
    RuntimeSnapshot,
  } from "../../../types";
  import { displayStatusLabel } from "../workspaceMembership";
  import { snapDisplayPosition } from "../topologyGeometry";
  import { workspaceBounds, workspaceProjection, type DeskBounds } from "../workspaceViewport";
  import DisplaySurface from "./DisplaySurface.svelte";
  import PortalOverlay from "./PortalOverlay.svelte";

  export let layout: WorkspaceLayout;
  export let displayAvailability: RuntimeSnapshot["displayAvailability"] = {};
  export let controller: string | null = null;
  export let deviceNames: Record<string, string> = {};
  export let mode: "select" | "test" = "select";
  export let selectedDisplayId: string | null = null;
  export let selectedPortalId: string | null = null;
  export let zoom = 1;
  export let onSelectDisplay: (id: string) => void;
  export let onSelectPortal: (id: string) => void;
  export let onMoveDisplay: (id: string, x: number, y: number) => void;
  export let onResizeDisplay: (id: string, width: number) => void;
  export let onClearSelection: () => void = () => {};
  export let onSelectionBounds: (
    bounds: { x: number; y: number; width: number; height: number } | null,
  ) => void = () => {};
  let bounds: DeskBounds | null = null;
  let membership = "";
  let pan = { x: 0, y: 0 };
  let panDrag: { x: number; y: number; originalX: number; originalY: number } | null = null;
  export function fitToView() {
    bounds = workspaceBounds(Object.values(layout.displays));
    pan = { x: 0, y: 0 };
    zoom = 1;
  }

  let width = 0;
  let height = 0;
  let drag: {
    id: string;
    startX: number;
    startY: number;
    originalX: number;
    originalY: number;
    projectScale: number;
  } | null = null;
  let resize: {
    id: string;
    startX: number;
    startY: number;
    originalWidth: number;
    originalHeight: number;
    projectScale: number;
  } | null = null;

  $: displayList = Object.values(layout.displays);
  $: renderList = selectedDisplayId
    ? [...displayList].sort(
        (left, right) =>
          Number(left.displayId === selectedDisplayId) -
          Number(right.displayId === selectedDisplayId),
      )
    : displayList;
  $: if (
    displayList
      .map((display) => display.displayId)
      .sort()
      .join("|") !== membership
  ) {
    membership = displayList
      .map((display) => display.displayId)
      .sort()
      .join("|");
    fitToView();
  }
  $: measured = width > 0 && height > 0;
  $: projection = workspaceProjection(
    bounds ?? workspaceBounds(displayList),
    width,
    height,
    zoom,
    pan,
  );
  $: scale = projection.scale;
  $: selectedDisplay = selectedDisplayId ? layout.displays[selectedDisplayId] : null;
  $: onSelectionBounds(
    selectedDisplay
      ? {
          x: projection.x + selectedDisplay.deskRectUm.x * scale,
          y: projection.y + selectedDisplay.deskRectUm.y * scale,
          width: selectedDisplay.deskRectUm.width * scale,
          height: selectedDisplay.deskRectUm.height * scale,
        }
      : null,
  );

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

  $: project = (x: number, y: number) => ({
    x: projection.x + x * scale,
    y: projection.y + y * scale,
  });
  function beginPan(event: PointerEvent) {
    if (event.button !== 0) return;
    event.preventDefault();
    onClearSelection();
    panDrag = { x: event.clientX, y: event.clientY, originalX: pan.x, originalY: pan.y };
    window.addEventListener("pointermove", movePan);
    window.addEventListener("pointerup", endPan, { once: true });
  }
  function movePan(event: PointerEvent) {
    if (panDrag)
      pan = {
        x: panDrag.originalX + event.clientX - panDrag.x,
        y: panDrag.originalY + event.clientY - panDrag.y,
      };
  }
  function endPan() {
    panDrag = null;
    window.removeEventListener("pointermove", movePan);
    window.removeEventListener("pointerup", endPan);
  }
  function wheel(event: WheelEvent) {
    event.preventDefault();
    if (event.ctrlKey || event.metaKey)
      zoom = Math.min(3, Math.max(0.2, zoom * Math.exp(-event.deltaY * 0.01)));
    else pan = { x: pan.x - event.deltaX, y: pan.y - event.deltaY };
  }

  function beginDrag(event: PointerEvent, display: DisplayType) {
    if (event.button !== 0) return;
    event.stopPropagation();
    if (mode !== "select") {
      onSelectDisplay(display.displayId);
      return;
    }
    event.preventDefault();
    onSelectDisplay(display.displayId);
    drag = {
      id: display.displayId,
      startX: event.clientX,
      startY: event.clientY,
      originalX: display.deskRectUm.x,
      originalY: display.deskRectUm.y,
      projectScale: scale,
    };
    window.addEventListener("pointermove", moveDrag);
    window.addEventListener("pointerup", endDrag, { once: true });
  }

  function moveDrag(event: PointerEvent) {
    if (!drag) return;
    const snap = 5_000;
    const nextX =
      Math.round((drag.originalX + (event.clientX - drag.startX) / drag.projectScale) / snap) *
      snap;
    const nextY =
      Math.round((drag.originalY + (event.clientY - drag.startY) / drag.projectScale) / snap) *
      snap;
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
    endPan();
  });

  function isOnline(display: DisplayType) {
    return displayAvailability[display.displayId] === "Ready";
  }
</script>

<div class="canvas" use:observeCanvas aria-busy={!measured} on:wheel|nonpassive={wheel}>
  {#if measured}
    <svg viewBox={`0 0 ${width} ${height}`} aria-label={uiTranslate("物理桌面工作区", $uiLanguage)}>
      <defs>
        <pattern id="grid" width="20" height="20" patternUnits="userSpaceOnUse">
          <circle cx="1" cy="1" r=".8" fill="currentColor" opacity=".14" />
        </pattern>
      </defs>
      <rect {width} {height} fill="url(#grid)" on:pointerdown={beginPan} role="presentation" />
      {#each renderList as display (display.displayId)}
        {@const point = project(display.deskRectUm.x, display.deskRectUm.y)}
        <DisplaySurface
          {display}
          number={displayList.indexOf(display) + 1}
          label={deviceNames[display.deviceId] ?? display.name}
          x={point.x}
          y={point.y}
          width={display.deskRectUm.width * scale}
          height={display.deskRectUm.height * scale}
          selected={selectedDisplayId === display.displayId}
          controller={controller === display.deviceId}
          online={isOnline(display)}
          statusLabel={displayStatusLabel(displayAvailability[display.displayId])}
          onpointerdown={(event) => beginDrag(event, display)}
          onresizepointerdown={(event) => beginResize(event, display)}
          onselect={() => onSelectDisplay(display.displayId)}
        />
      {/each}
      {#each layout.portals as portal (portal.portalId)}
        <PortalOverlay
          {portal}
          displays={layout.displays}
          {project}
          selected={selectedPortalId === portal.portalId}
          onselect={() => onSelectPortal(portal.portalId)}
        />
      {/each}
    </svg>
  {/if}
</div>

<style>
  .canvas {
    position: relative;
    width: 100%;
    height: 100%;
    min-height: 300px;
    overflow: hidden;
    border-radius: 14px;
    color: var(--text-secondary, #344054);
    background: var(--surface);
    touch-action: none;
    user-select: none;
  }
  svg {
    width: 100%;
    height: 100%;
    display: block;
    cursor: grab;
  }
</style>
