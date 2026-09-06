<script lang="ts">
  import { t } from "../localization";
  import { localeFor } from "../localization.ts";
  import { translate as uiTranslate, language as uiLanguage } from "../i18n";
  import { onMount, tick } from "svelte";
  import {
    ArrowClockwise,
    ArrowCounterClockwise,
    ArrowsClockwise,
    ImageSquare,
    MagnifyingGlassMinus,
    MagnifyingGlassPlus,
  } from "phosphor-svelte";

  import type { LanguagePreference } from "../types";
  import { clipboardBridge } from "./bridge";
  import { imagePreviewHeight, imagePreviewLayout } from "./imagePreviewGeometry";
  import {
    hitTestImageCharacter,
    imageTextBoundaryAtPoint,
    imageTextLineRange,
    imageTextSelectionBands,
    imageTextSelectionRange,
    imageTextWordRange,
    polygonPoints,
    selectableImageCharacters,
    selectedImageText,
  } from "./imageTextSelection";
  import { tr } from "./i18n";
  import { cachedThumbnail, loadCachedThumbnail } from "./thumbnailCache";
  import type { ClipboardImageOcr, ClipboardItem, ClipboardOcrPoint } from "./types";

  export let item: ClipboardItem;
  export let language: LanguagePreference;
  export let busy = false;
  export let onCopy: (content: string) => Promise<void> | void = () => undefined;
  export let onPaste: (content: string) => Promise<void> | void = () => undefined;

  const MIN_SCALE = 0.25;
  const MAX_SCALE = 4;
  const SCALE_STEP = 0.25;

  let thumbnailSource: string | null = cachedThumbnail(item.id) ?? null;
  let highResolutionSource: string | null = null;
  let highResolutionLoading = false;
  let highResolutionFailed = false;
  let renderedWidth = 0;
  let renderedHeight = 0;
  let loadedItemId = 0;
  let request = 0;
  let scale = 1;
  let rotation = 0;
  let canvasWidth = 0;
  let canvasHeight = 0;
  let canvasElement: HTMLElement;
  let gestureActive = false;
  let gestureStartScale = 1;
  let centerFrame = 0;
  let ocr: ClipboardImageOcr | null = null;
  let ocrLoading = true;
  let ocrFailed = false;
  let selectionAnchor: number | null = null;
  let selectionFocus: number | null = null;
  let selectionDragging = false;
  let selectionSvg: SVGSVGElement;
  let previewRoot: HTMLElement;
  let selectionPopover: HTMLElement;
  let selectionPopoverVisible = false;
  let selectionPopoverLeft = 0;
  let selectionPopoverTop = 0;
  let selectionPopoverPlacement: "above" | "below" | "left" | "right" = "above";
  let selectionPopoverFrame = 0;
  let selectionPointerId: number | null = null;
  let pendingSelectionBoundary: number | null = null;
  let pointerStartX = 0;
  let pointerStartY = 0;
  let lastPointerX = 0;
  let lastPointerY = 0;
  let autoScrollFrame = 0;

  $: if (item.id !== loadedItemId) resetForItem(item.id);
  $: source = highResolutionSource ?? thumbnailSource;
  $: zoomPercent = Math.round(scale * 100);
  $: layout = imagePreviewLayout({
    imageWidth: renderedWidth,
    imageHeight: renderedHeight,
    canvasWidth,
    canvasHeight,
    scale,
    rotation,
  });
  $: preferredPreviewHeight = imagePreviewHeight({
    imageWidth: renderedWidth,
    imageHeight: renderedHeight,
    containerWidth: canvasWidth,
    rotation,
  });
  $: sourceWidth = Math.max(1, item.width ?? renderedWidth);
  $: sourceHeight = Math.max(1, item.height ?? renderedHeight);
  $: selectableCharacters = ocr ? selectableImageCharacters(ocr) : [];
  $: selectedText = selectedImageText(selectableCharacters, selectionAnchor, selectionFocus);
  $: selectedRange = imageTextSelectionRange(selectionAnchor, selectionFocus);
  $: selectionBands = imageTextSelectionBands(selectableCharacters, selectionAnchor, selectionFocus);
  $: selectedCharacterCount = selectedRange ? selectedRange[1] - selectedRange[0] : 0;

  onMount(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void clipboardBridge.onOcrChanged((id) => {
      if (id === loadedItemId) void loadOcr(id, request, true);
    }).then((value) => {
      if (disposed) value();
      else unlisten = value;
    });
    window.addEventListener("keydown", handleSelectionKeyDown, true);
    window.addEventListener("resize", scheduleSelectionPopover);
    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener("keydown", handleSelectionKeyDown, true);
      window.removeEventListener("resize", scheduleSelectionPopover);
      window.cancelAnimationFrame(selectionPopoverFrame);
      window.cancelAnimationFrame(autoScrollFrame);
    };
  });

  function resetForItem(id: number) {
    const currentRequest = ++request;
    loadedItemId = id;
    scale = 1;
    rotation = 0;
    thumbnailSource = cachedThumbnail(id) ?? null;
    highResolutionSource = null;
    highResolutionLoading = true;
    highResolutionFailed = false;
    renderedWidth = 0;
    renderedHeight = 0;
    ocr = null;
    ocrLoading = true;
    ocrFailed = false;
    clearSelection();
    void loadThumbnail(id, currentRequest);
    void loadHighResolution(id, currentRequest);
    void loadOcr(id, currentRequest, false);
  }

  async function loadOcr(id: number, currentRequest: number, completedEvent: boolean) {
    try {
      const value = await clipboardBridge.imageOcr(id);
      if (currentRequest !== request || loadedItemId !== id) return;
      ocr = value;
      ocrLoading = value === null && !completedEvent;
      ocrFailed = value === null && completedEvent;
    } catch {
      if (currentRequest !== request || loadedItemId !== id) return;
      ocrLoading = false;
      ocrFailed = true;
    }
  }

  async function loadThumbnail(id: number, currentRequest: number) {
    const cached = cachedThumbnail(id);
    if (cached !== undefined) {
      thumbnailSource = cached;
      return;
    }

    const value = await loadCachedThumbnail(id);
    if (currentRequest === request && loadedItemId === id) thumbnailSource = value;
  }

  function decodeImage(source: string) {
    const image = new Image();
    if (typeof image.decode === "function") {
      image.src = source;
      return image.decode();
    }
    return new Promise<void>((resolve, reject) => {
      image.onload = () => resolve();
      image.onerror = () => reject(new Error("image decode failed"));
      image.src = source;
    });
  }

  async function loadHighResolution(id: number, currentRequest: number) {
    try {
      const value = await clipboardBridge.imagePreview(id);
      if (value) await decodeImage(value);
      if (currentRequest !== request || loadedItemId !== id) return;
      highResolutionSource = value;
      highResolutionFailed = value === null;
    } catch {
      if (currentRequest !== request || loadedItemId !== id) return;
      highResolutionFailed = true;
    } finally {
      if (currentRequest === request && loadedItemId === id) highResolutionLoading = false;
    }
  }

  function handleImageLoad(event: Event) {
    const image = event.currentTarget as HTMLImageElement;
    renderedWidth = image.naturalWidth;
    renderedHeight = image.naturalHeight;
    centerImage();
  }

  function handleImageError() {
    if (!highResolutionSource) return;
    highResolutionSource = null;
    highResolutionFailed = true;
  }

  function centerImage() {
    window.cancelAnimationFrame(centerFrame);
    centerFrame = window.requestAnimationFrame(() => {
      if (!canvasElement) return;
      canvasElement.scrollLeft = Math.max(0, (canvasElement.scrollWidth - canvasElement.clientWidth) / 2);
      canvasElement.scrollTop = Math.max(0, (canvasElement.scrollHeight - canvasElement.clientHeight) / 2);
    });
  }

  function setScale(value: number) {
    const next = Math.min(MAX_SCALE, Math.max(MIN_SCALE, Math.round(value * 100) / 100));
    if (next === scale) return;
    scale = next;
    centerImage();
    scheduleSelectionPopover();
  }

  function rotate(delta: -90 | 90) {
    rotation = (rotation + delta + 360) % 360;
    centerImage();
    scheduleSelectionPopover();
  }

  function resetTransform() {
    scale = 1;
    rotation = 0;
    centerImage();
    scheduleSelectionPopover();
  }

  function clearSelection() {
    if (selectionPointerId !== null && selectionSvg?.hasPointerCapture(selectionPointerId)) {
      selectionSvg.releasePointerCapture(selectionPointerId);
    }
    selectionAnchor = null;
    selectionFocus = null;
    selectionDragging = false;
    selectionPopoverVisible = false;
    pendingSelectionBoundary = null;
    selectionPointerId = null;
    stopAutoScroll();
  }

  function sourcePointAt(clientX: number, clientY: number): ClipboardOcrPoint | null {
    const matrix = selectionSvg?.getScreenCTM();
    if (!matrix) return null;
    const point = selectionSvg.createSVGPoint();
    point.x = clientX;
    point.y = clientY;
    const transformed = point.matrixTransform(matrix.inverse());
    return { x: transformed.x, y: transformed.y };
  }

  function selectionTolerance() {
    if (!layout) return 0;
    return 6 * Math.max(sourceWidth / layout.imageWidth, sourceHeight / layout.imageHeight);
  }

  function characterIndexAt(clientX: number, clientY: number, dragging: boolean) {
    const point = sourcePointAt(clientX, clientY);
    if (!point) return null;
    return hitTestImageCharacter(
      selectableCharacters,
      point,
      dragging ? Number.POSITIVE_INFINITY : selectionTolerance(),
    );
  }

  function selectionBoundaryAt(clientX: number, clientY: number, dragging: boolean) {
    const point = sourcePointAt(clientX, clientY);
    if (!point) return null;
    return imageTextBoundaryAtPoint(
      selectableCharacters,
      point,
      dragging ? Number.POSITIVE_INFINITY : selectionTolerance(),
    );
  }

  function selectRange(range: [number, number]) {
    selectionAnchor = range[0];
    selectionFocus = range[1];
    selectionDragging = false;
    selectionPopoverVisible = false;
    void revealSelectionPopover();
  }

  function handleSelectionStart(event: PointerEvent) {
    if (event.button !== 0 || busy) return;
    const index = characterIndexAt(event.clientX, event.clientY, false);
    if (index === null) {
      clearSelection();
      return;
    }
    event.preventDefault();
    event.stopPropagation();

    if (event.detail >= 3) {
      selectRange(imageTextLineRange(selectableCharacters, index));
      return;
    }
    if (event.detail === 2) {
      selectRange(imageTextWordRange(
        selectableCharacters,
        index,
        localeFor(language),
      ));
      return;
    }

    const boundary = selectionBoundaryAt(event.clientX, event.clientY, false);
    if (boundary === null) return;
    selectionSvg.setPointerCapture(event.pointerId);
    selectionPointerId = event.pointerId;
    pendingSelectionBoundary = boundary;
    pointerStartX = event.clientX;
    pointerStartY = event.clientY;
    lastPointerX = event.clientX;
    lastPointerY = event.clientY;
    selectionPopoverVisible = false;
    if (event.shiftKey && selectionAnchor !== null) {
      selectionFocus = boundary;
      selectionDragging = true;
    } else {
      selectionDragging = false;
    }
  }

  function handleSelectionMove(event: PointerEvent) {
    if (selectionPointerId !== event.pointerId) return;
    lastPointerX = event.clientX;
    lastPointerY = event.clientY;
    if (!selectionDragging) {
      const distance = Math.hypot(event.clientX - pointerStartX, event.clientY - pointerStartY);
      if (distance < 3 || pendingSelectionBoundary === null) return;
      selectionAnchor = pendingSelectionBoundary;
      selectionDragging = true;
    }
    const boundary = selectionBoundaryAt(event.clientX, event.clientY, true);
    if (boundary !== null) selectionFocus = boundary;
    startAutoScroll();
    event.preventDefault();
  }

  function handleSelectionEnd(event: PointerEvent) {
    if (selectionPointerId !== event.pointerId) return;
    const selectedByDragging = selectionDragging;
    selectionDragging = false;
    if (selectionSvg.hasPointerCapture(event.pointerId)) {
      selectionSvg.releasePointerCapture(event.pointerId);
    }
    selectionPointerId = null;
    pendingSelectionBoundary = null;
    stopAutoScroll();
    if (selectedByDragging) void revealSelectionPopover();
    else clearSelection();
  }

  function handleSelectionContextMenu(event: MouseEvent) {
    const index = characterIndexAt(event.clientX, event.clientY, false);
    if (index === null) return;
    event.preventDefault();
    event.stopPropagation();
    const selected = selectedRange && index >= selectedRange[0] && index < selectedRange[1];
    if (!selected) {
      const range = imageTextWordRange(
        selectableCharacters,
        index,
        localeFor(language),
      );
      selectionAnchor = range[0];
      selectionFocus = range[1];
    }
    selectionPopoverVisible = false;
    void revealSelectionPopover();
  }

  function edgeScrollVelocity(position: number, start: number, end: number) {
    const edge = 34;
    if (position < start + edge) return -Math.min(20, Math.max(0, start + edge - position) * 0.55);
    if (position > end - edge) return Math.min(20, Math.max(0, position - end + edge) * 0.55);
    return 0;
  }

  function startAutoScroll() {
    if (autoScrollFrame) return;
    const step = () => {
      autoScrollFrame = 0;
      if (!selectionDragging || selectionPointerId === null || !canvasElement) return;
      const rect = canvasElement.getBoundingClientRect();
      const deltaX = edgeScrollVelocity(lastPointerX, rect.left, rect.right);
      const deltaY = edgeScrollVelocity(lastPointerY, rect.top, rect.bottom);
      if (deltaX || deltaY) {
        canvasElement.scrollLeft += deltaX;
        canvasElement.scrollTop += deltaY;
        const boundary = selectionBoundaryAt(lastPointerX, lastPointerY, true);
        if (boundary !== null) selectionFocus = boundary;
      }
      autoScrollFrame = window.requestAnimationFrame(step);
    };
    autoScrollFrame = window.requestAnimationFrame(step);
  }

  function stopAutoScroll() {
    window.cancelAnimationFrame(autoScrollFrame);
    autoScrollFrame = 0;
  }

  function activeSelectionCharacterIndex() {
    if (!selectedRange || selectionAnchor === null || selectionFocus === null) return null;
    return selectionFocus > selectionAnchor
      ? Math.max(selectedRange[0], selectionFocus - 1)
      : Math.min(selectedRange[1] - 1, selectionFocus);
  }

  async function revealSelectionPopover() {
    await tick();
    if (!selectedText) {
      selectionPopoverVisible = false;
      return;
    }
    selectionPopoverVisible = true;
    await tick();
    positionSelectionPopover();
  }

  function scheduleSelectionPopover() {
    if (!selectionPopoverVisible) return;
    window.cancelAnimationFrame(selectionPopoverFrame);
    selectionPopoverFrame = window.requestAnimationFrame(positionSelectionPopover);
  }

  function positionSelectionPopover() {
    selectionPopoverFrame = 0;
    const activeIndex = activeSelectionCharacterIndex();
    const target = activeIndex === null
      ? null
      : selectionSvg?.querySelector<SVGGraphicsElement>(`[data-character-index="${activeIndex}"]`);
    if (!target || !previewRoot || !selectionPopover || !canvasElement) return;

    const targetRect = target.getBoundingClientRect();
    const rootRect = previewRoot.getBoundingClientRect();
    const canvasRect = canvasElement.getBoundingClientRect();
    const popoverRect = selectionPopover.getBoundingClientRect();
    const visibleLeft = Math.max(canvasRect.left, Math.min(canvasRect.right, targetRect.left));
    const visibleRight = Math.max(canvasRect.left, Math.min(canvasRect.right, targetRect.right));
    const visibleTop = Math.max(canvasRect.top, Math.min(canvasRect.bottom, targetRect.top));
    const visibleBottom = Math.max(canvasRect.top, Math.min(canvasRect.bottom, targetRect.bottom));
    const centerX = (visibleLeft + visibleRight) / 2;
    const centerY = (visibleTop + visibleBottom) / 2;
    const horizontalCenter = Math.max(
      canvasRect.left + popoverRect.width / 2 + 6,
      Math.min(canvasRect.right - popoverRect.width / 2 - 6, centerX),
    );
    const verticalCenter = Math.max(
      canvasRect.top + popoverRect.height / 2 + 6,
      Math.min(canvasRect.bottom - popoverRect.height / 2 - 6, centerY),
    );
    const textRects = [...selectionSvg.querySelectorAll<SVGGraphicsElement>(
      ".image-text-hit-target",
    )].map((element) => element.getBoundingClientRect());
    const candidates = [
      {
        placement: "above" as const,
        left: horizontalCenter - popoverRect.width / 2,
        top: visibleTop - 8 - popoverRect.height,
        cssLeft: horizontalCenter - rootRect.left,
        cssTop: visibleTop - rootRect.top - 8,
      },
      {
        placement: "below" as const,
        left: horizontalCenter - popoverRect.width / 2,
        top: visibleBottom + 8,
        cssLeft: horizontalCenter - rootRect.left,
        cssTop: visibleBottom - rootRect.top + 8,
      },
      {
        placement: "left" as const,
        left: visibleLeft - 8 - popoverRect.width,
        top: verticalCenter - popoverRect.height / 2,
        cssLeft: visibleLeft - rootRect.left - 8,
        cssTop: verticalCenter - rootRect.top,
      },
      {
        placement: "right" as const,
        left: visibleRight + 8,
        top: verticalCenter - popoverRect.height / 2,
        cssLeft: visibleRight - rootRect.left + 8,
        cssTop: verticalCenter - rootRect.top,
      },
    ];
    const placementScore = ({ left, top }: (typeof candidates)[number]) => {
      const right = left + popoverRect.width;
      const bottom = top + popoverRect.height;
      const outside = Math.max(0, canvasRect.left + 6 - left)
        + Math.max(0, right - canvasRect.right + 6)
        + Math.max(0, canvasRect.top + 6 - top)
        + Math.max(0, bottom - canvasRect.bottom + 6);
      const overlap = textRects.reduce((total, rect) => {
        const width = Math.max(0, Math.min(right, rect.right) - Math.max(left, rect.left));
        const height = Math.max(0, Math.min(bottom, rect.bottom) - Math.max(top, rect.top));
        return total + width * height;
      }, 0);
      return outside * popoverRect.width * 4 + overlap;
    };
    const best = candidates.reduce((current, candidate) => (
      placementScore(candidate) < placementScore(current) ? candidate : current
    ));
    selectionPopoverPlacement = best.placement;
    selectionPopoverLeft = best.cssLeft;
    selectionPopoverTop = best.cssTop;
  }

  function handleSelectionKeyDown(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (event.target instanceof HTMLButtonElement && event.key === "Enter") return;
    const primaryModifier = event.metaKey || event.ctrlKey;
    if (primaryModifier && !event.altKey && event.code === "KeyA" && selectableCharacters.length) {
      event.preventDefault();
      event.stopImmediatePropagation();
      selectionAnchor = 0;
      selectionFocus = selectableCharacters.length;
      selectionPopoverVisible = false;
      void revealSelectionPopover();
    } else if (primaryModifier && !event.altKey && event.code === "KeyC" && selectedText) {
      event.preventDefault();
      event.stopImmediatePropagation();
      if (!busy) void onCopy(selectedText);
    } else if (!primaryModifier && !event.altKey && !event.shiftKey && event.key === "Enter" && selectedText) {
      event.preventDefault();
      event.stopImmediatePropagation();
      if (!busy) void onPaste(selectedText);
    } else if (
      !primaryModifier
      && !event.altKey
      && event.shiftKey
      && selectionAnchor !== null
      && selectionFocus !== null
      && (event.key === "ArrowLeft" || event.key === "ArrowRight")
    ) {
      event.preventDefault();
      event.stopImmediatePropagation();
      const delta = event.key === "ArrowLeft" ? -1 : 1;
      selectionFocus = Math.max(0, Math.min(selectableCharacters.length, selectionFocus + delta));
      selectionPopoverVisible = false;
      void revealSelectionPopover();
    } else if (event.key === "Escape" && selectedText) {
      event.preventDefault();
      event.stopImmediatePropagation();
      clearSelection();
    }
  }

  function handleWheel(event: WheelEvent) {
    if (gestureActive || (!event.metaKey && !event.ctrlKey)) return;
    event.preventDefault();
    const boundedDelta = Math.max(-50, Math.min(50, event.deltaY));
    setScale(scale * Math.exp(-boundedDelta * 0.004));
  }

  function observeCanvas(node: HTMLElement) {
    const update = () => {
      canvasWidth = node.clientWidth;
      canvasHeight = node.clientHeight;
    };
    const observer = new ResizeObserver(update);
    observer.observe(node);
    update();
    return { destroy: () => observer.disconnect() };
  }

  type WebKitGestureEvent = Event & { scale: number };

  function gestureZoom(node: HTMLElement) {
    const start = (event: Event) => {
      event.preventDefault();
      gestureActive = true;
      gestureStartScale = scale;
    };
    const change = (event: Event) => {
      event.preventDefault();
      const gestureScale = (event as WebKitGestureEvent).scale;
      if (Number.isFinite(gestureScale) && gestureScale > 0) {
        setScale(gestureStartScale * gestureScale);
      }
    };
    const end = (event: Event) => {
      event.preventDefault();
      gestureActive = false;
      centerImage();
    };
    node.addEventListener("gesturestart", start, { passive: false });
    node.addEventListener("gesturechange", change, { passive: false });
    node.addEventListener("gestureend", end, { passive: false });
    return {
      destroy: () => {
        node.removeEventListener("gesturestart", start);
        node.removeEventListener("gesturechange", change);
        node.removeEventListener("gestureend", end);
      },
    };
  }
</script>

<div
  class:gesture-active={gestureActive}
  class:selecting-text={selectionDragging}
  class="image-preview-interactive"
  bind:this={previewRoot}
  style:--image-preview-height={`${preferredPreviewHeight}px`}
>
  <div class="image-preview-toolbar" aria-label={uiTranslate("图片操作", $uiLanguage)}>
    <button type="button" disabled={scale <= MIN_SCALE} aria-label={uiTranslate("缩小", $uiLanguage)} title={uiTranslate("缩小", $uiLanguage)} on:click={() => setScale(scale - SCALE_STEP)}>
      <MagnifyingGlassMinus size={17} />
    </button>
    <span class="image-preview-zoom" aria-live="polite">{zoomPercent}%</span>
    <button type="button" disabled={scale >= MAX_SCALE} aria-label={uiTranslate("放大", $uiLanguage)} title={uiTranslate("放大", $uiLanguage)} on:click={() => setScale(scale + SCALE_STEP)}>
      <MagnifyingGlassPlus size={17} />
    </button>
    <span class="image-preview-toolbar-separator"></span>
    <button type="button" aria-label={uiTranslate("向左旋转", $uiLanguage)} title={uiTranslate("向左旋转", $uiLanguage)} on:click={() => rotate(-90)}>
      <ArrowCounterClockwise size={17} />
    </button>
    <button type="button" aria-label={uiTranslate("向右旋转", $uiLanguage)} title={uiTranslate("向右旋转", $uiLanguage)} on:click={() => rotate(90)}>
      <ArrowClockwise size={17} />
    </button>
    <button type="button" disabled={scale === 1 && rotation === 0} aria-label={uiTranslate("复位图片", $uiLanguage)} title={uiTranslate("复位图片", $uiLanguage)} on:click={resetTransform}>
      <ArrowsClockwise size={17} />
    </button>
    <span class="image-preview-load-announcement" aria-live="polite">
      {#if highResolutionLoading}
        {uiTranslate("正在加载高清图…", $uiLanguage)}
      {:else if highResolutionFailed}
        {uiTranslate("已回退缩略图", $uiLanguage)}
      {:else if ocrLoading}
        {uiTranslate("正在识别文字…", $uiLanguage)}
      {:else if ocrFailed}
        {uiTranslate("文字识别不可用", $uiLanguage)}
      {/if}
    </span>
  </div>

  <div
    class="image-preview-canvas"
    bind:this={canvasElement}
    use:observeCanvas
    use:gestureZoom
    on:scroll={scheduleSelectionPopover}
    on:wheel|nonpassive={handleWheel}
  >
    {#if source}
      <div
        class="image-preview-stage"
        style:width={layout ? `${layout.stageWidth}px` : "100%"}
        style:height={layout ? `${layout.stageHeight}px` : "100%"}
      >
        <div
          class="image-preview-media"
          style:width={layout ? `${layout.imageWidth}px` : null}
          style:height={layout ? `${layout.imageHeight}px` : null}
          style:transform={`translate(-50%, -50%) rotate(${rotation}deg)`}
          on:transitionend={scheduleSelectionPopover}
        >
          <img
            class="image-preview-content"
            src={source}
            alt={tr("剪贴板图片预览", language)}
            draggable="false"
            on:load={handleImageLoad}
            on:error={handleImageError}
          />
          {#if selectableCharacters.length}
            <svg
              class="image-text-selection-layer"
              class:dragging={selectionDragging}
              bind:this={selectionSvg}
              viewBox={`0 0 ${sourceWidth} ${sourceHeight}`}
              preserveAspectRatio="none"
              role="group"
              aria-label={uiTranslate("图片中的可选择文字", $uiLanguage)}
              on:pointerdown={handleSelectionStart}
              on:pointermove={handleSelectionMove}
              on:pointerup={handleSelectionEnd}
              on:pointercancel={handleSelectionEnd}
              on:contextmenu={handleSelectionContextMenu}
            >
              <title>{uiTranslate("在识别到的文字上拖动即可选择", $uiLanguage)}</title>
              {#each selectionBands as band}
                <polygon
                  class="image-text-selected-range"
                  points={polygonPoints(band.points)}
                  aria-hidden="true"
                />
              {/each}
              {#each selectableCharacters as character, index}
                <polygon
                  class="image-text-hit-target"
                  data-character-index={index}
                  points={polygonPoints(character.points)}
                  aria-hidden="true"
                />
              {/each}
            </svg>
          {/if}
        </div>
      </div>
    {:else}
      <div class="thumbnail-fallback image-preview-fallback">
        <ImageSquare size={42} />
        <span>{tr("图片", language)}</span>
      </div>
    {/if}
  </div>

  {#if selectionPopoverVisible && selectedText}
    <div
      class:below={selectionPopoverPlacement === "below"}
      class:left={selectionPopoverPlacement === "left"}
      class:right={selectionPopoverPlacement === "right"}
      class="image-selection-popover"
      bind:this={selectionPopover}
      role="toolbar"
      aria-label={uiTranslate("所选图片文字操作", $uiLanguage)}
      style:left={`${selectionPopoverLeft}px`}
      style:top={`${selectionPopoverTop}px`}
    >
      <button type="button" disabled={busy} on:click={() => onCopy(selectedText)}>
        {uiTranslate("复制", $uiLanguage)}
      </button>
      <span aria-hidden="true"></span>
      <button class="primary" type="button" disabled={busy} on:click={() => onPaste(selectedText)}>
        {busy ? (uiTranslate("正在插入…", $uiLanguage)) : (uiTranslate("插入", $uiLanguage))}
      </button>
    </div>
  {/if}
  <span class="image-selection-announcement" aria-live="polite">
    {#if selectedCharacterCount}
      {t("已选择 {count} 个字符", language, { count: selectedCharacterCount })}
    {/if}
  </span>
</div>
