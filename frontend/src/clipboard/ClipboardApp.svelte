<script lang="ts">
  import { t } from "../localization";
  import { translate as uiTranslate, language as uiLanguage, setLanguage } from "../i18n";
  import { SubscriptionScope } from "../subscriptions";
  import { onMount, tick } from "svelte";
  import { Dialog } from "bits-ui";
  import {
    Check,
    ArrowLeft,
    ClockCounterClockwise,
    CopySimple,
    FileText,
    ImageSquare,
    MagnifyingGlass,
    PushPin,
    StackSimple,
    Star,
    Tag,
    TextT,
    X,
  } from "phosphor-svelte";

  import BrandLogo from "../BrandLogo.svelte";
  import type { AppSettings, ClipboardSortPreference, LanguagePreference } from "../types";
  import ClipboardLabelDialog from "./ClipboardLabelDialog.svelte";
  import ClipboardRow from "./ClipboardRow.svelte";
  import ContentPreview from "./ContentPreview.svelte";
  import SegmentPicker from "./SegmentPicker.svelte";
  import { clipboardBridge } from "./bridge";
  import { HeightIndex } from "./heightIndex";
  import { clearThumbnailCache } from "./thumbnailCache";
  import { clearHtmlPreviewCache } from "./htmlPreviewCache";
  import { loadAppSettings, localeFor, onAppSettingsChanged, tr } from "./i18n";
  import { resolveClipboardKeyboardAction, resolvePreviewKeyboardAction, type ClipboardKeyboardMode } from "./keyboardShortcuts";
  import { appendSelectedIds, selectionOrder, toggleSelectedId } from "./multiSelect";
  import { showClipboardContextMenu } from "./nativeContextMenu";
  import { historyQueryKey, mergeTimelineEntries, type NearbyHistory, type ScrollAnchor, type SearchSnapshot } from "./historyNavigation";
  import type { ClipboardCursor, ClipboardFilter, ClipboardHistory, ClipboardItem, ClipboardKind, ClipboardLabel, ClipboardPasteMode, ContinuousPasteProgress } from "./types";
  import type { NearbyClipboardPeer } from "./types";

  const FETCH_SIZE = 60;
  const QUICK_PASTE_LIMIT = 5;
  const VIRTUAL_OVERSCAN_ROWS = 4;
  const ROW_GAP = 8;
  const isMacPlatform = /Mac|iPhone|iPad/.test(navigator.platform);
  const primaryShortcutLabel = isMacPlatform ? "⌘" : "Ctrl+";
  const filters: Array<{ id: ClipboardFilter; label: string; icon: typeof ClockCounterClockwise; size: number; weight?: "fill" }> = [
    { id: "all", label: "最近", icon: ClockCounterClockwise, size: 19 },
    { id: "text", label: "文本", icon: TextT, size: 20 },
    { id: "image", label: "图片", icon: ImageSquare, size: 20 },
    { id: "files", label: "文件", icon: FileText, size: 19 },
    { id: "favorites", label: "收藏", icon: Star, size: 20, weight: "fill" },
  ];

  $: language = $uiLanguage;
  let sortBy: ClipboardSortPreference = "updatedAt";
  let filter: ClipboardFilter = "all";
  let search = "";
  let debouncedSearch = "";
  let history: ClipboardHistory = { revision: 0, entries: [], nextCursor: null, totalCount: 0 };
  let selectedId: number | null = null;
  let selectedIds: number[] = [];
  let multiPasting = false;
  let continuousPaste: ContinuousPasteProgress = { current: 0, total: 0, active: false };
  let windowVisible = !("__TAURI_INTERNALS__" in window);
  let loading = true;
  let loadingMore = false;
  let ocrPasting = false;
  let error = "";
  let labelFilterOpen = false;
  let keyboardMode: ClipboardKeyboardMode = "search";
  let previewDialogOpen = false;
  let previewingItem: ClipboardItem | null = null;
  let previewGeneration = 0;
  let previewModel: import("./textSegments").TextSliceModel | null = null;
  let previewLoading = false;
  let previewError = "";
  let previewActionBusy = false;
  let previewSourceWarning = "";
  let imagePreviewReady = false;
  let editDialogOpen = false;
  let editingItem: ClipboardItem | null = null;
  let editContent = "";
  let labelDialogOpen = false;
  let labelingItem: ClipboardItem | null = null;
  let labels: ClipboardLabel[] = [];
  let selectedLabelIds: string[] = [];
  let newLabelName = "";
  let newLabelColor = "#5B5FF0";
  let newLabelInput: HTMLInputElement;
  let selectedLabelFilter: string | null = null;
  let nearbyPeers: NearbyClipboardPeer[] = [];
  let windowPinned = false;
  let scrollElement: HTMLElement;
  let scrollTop = 0;
  let viewportHeight = 0;
  let visibleStartIndex = 0;
  let visibleEndIndex = 0;
  let virtualStartIndex = 0;
  let virtualEndIndex = 0;
  let virtualTopPadding = 0;
  let virtualBottomPadding = 0;
  let renderedEntries: Array<{ item: ClipboardItem; index: number }> = [];
  let rowHeights = new Map<number, number>();
  let heightIndex = new HeightIndex([]);
  let indexedEntries: ClipboardItem[] | undefined;
  let indexedMeasurements: Map<number, number> | undefined;
  let scrollResizeObserver: ResizeObserver | undefined;
  let searchInput: HTMLInputElement;
  let debounceTimer: number | undefined;
  let scrollAnimationFrame = 0;
  let loadGeneration = 0;
  let loadedQueryKey: string | null = null;
  let nearby: NearbyHistory | null = null;
  let locating = false;
  let navigationError = "";
  let positionAnchor: ScrollAnchor | null = null;
  let historyChangeCount = 0;
  let appearanceMedia: MediaQueryList | undefined;
  let handleSystemThemeChange: (() => void) | undefined;
  let currentSettings: AppSettings | undefined;

  $: imagePreviewReady = previewingItem?.kind === "image"
    && !previewingItem.sensitive
    && !previewLoading
    && !previewError;

  $: selectedIndex = history.entries.findIndex((item) => item.id === selectedId);
  $: selectedItem = selectedIndex >= 0 ? history.entries[selectedIndex] : null;
  $: shortcutNumbers = new Map((nearby ? [] : history.entries.slice(0, QUICK_PASTE_LIMIT)).map((_, index) => [index, index + 1]));
  $: rowShortcutModifier = keyboardMode === "search" ? primaryShortcutLabel : "";
  $: selectedLabel = labels.find((label) => label.id === selectedLabelFilter) ?? null;
  $: recalculateVisibleRange(history.entries, scrollTop, viewportHeight);
  $: {
    window.clearTimeout(debounceTimer);
    const value = search;
    debounceTimer = window.setTimeout(() => {
      debouncedSearch = value;
      reloadForQuery(filter, value, selectedLabelFilter);
    }, 180);
  }
  $: reloadForQuery(filter, debouncedSearch, selectedLabelFilter);

  function reloadForQuery(nextFilter: ClipboardFilter, query: string, labelId: string | null) {
    const key = historyQueryKey(query, nextFilter, labelId);
    if (key === loadedQueryKey) return;
    loadedQueryKey = key;
    nearby = null;
    positionAnchor = null;
    navigationError = "";
    void load();
  }

  function setSearchState(query: string, nextFilter: ClipboardFilter, labelId: string | null) {
    window.clearTimeout(debounceTimer);
    loadedQueryKey = historyQueryKey(query, nextFilter, labelId);
    search = query;
    debouncedSearch = query;
    filter = nextFilter;
    selectedLabelFilter = labelId;
    labelFilterOpen = false;
  }

  function abandonNearby() {
    loadGeneration += 1;
    nearby = null;
    locating = false;
    loading = false;
    loadingMore = false;
    positionAnchor = null;
    navigationError = "";
  }

  function handleSearchInput(event: Event) {
    search = (event.currentTarget as HTMLInputElement).value;
    abandonNearby();
    loadedQueryKey = null;
  }

  function changeFilter(value: ClipboardFilter) {
    abandonNearby();
    filter = value;
    loadedQueryKey = null;
    reloadForQuery(filter, debouncedSearch, selectedLabelFilter);
  }

  function estimatedRowHeight(item: ClipboardItem) {
    if (item.kind === "image") return 140;
    if (item.kind === "html") return 126;
    if (item.kind === "text" && (item.preview.includes("\n") || /^(git|npm|pnpm|yarn|cargo|cd)\s/m.test(item.preview.trim()))) return 108;
    return 94;
  }

  function rowBlockHeight(item: ClipboardItem) {
    return (rowHeights.get(item.id) ?? estimatedRowHeight(item)) + ROW_GAP;
  }

  function ensureHeightIndex(entries = history.entries) {
    if (indexedEntries !== entries || indexedMeasurements !== rowHeights) {
      heightIndex = new HeightIndex(entries.map(item => ({ id: item.id, height: rowBlockHeight(item) })));
      indexedEntries = entries;
      indexedMeasurements = rowHeights;
    }
    return heightIndex;
  }

  function offsetForIndex(index: number) { return ensureHeightIndex().offset(index); }
  function anchorTop(anchor: ScrollAnchor) { return ensureHeightIndex().anchorTop(anchor); }

  function recalculateVisibleRange(entries: ClipboardItem[], top: number, height: number) {
    if (entries.length === 0) {
      visibleStartIndex = 0;
      visibleEndIndex = 0;
      virtualStartIndex = 0;
      virtualEndIndex = 0;
      virtualTopPadding = 0;
      virtualBottomPadding = 0;
      renderedEntries = [];
      return;
    }

    const index = ensureHeightIndex(entries);
    const viewportBottom = top + Math.max(height, 1);
    const first = Math.min(index.indexAt(Math.max(0, top)), entries.length - 1);
    const end = index.indexAt(viewportBottom);
    const last = Math.min(entries.length, end + (index.offset(end) < viewportBottom ? 1 : 0));

    visibleStartIndex = first;
    visibleEndIndex = last;
    virtualStartIndex = Math.max(0, first - VIRTUAL_OVERSCAN_ROWS);
    virtualEndIndex = Math.min(entries.length, last + VIRTUAL_OVERSCAN_ROWS);
    virtualTopPadding = offsetForIndex(virtualStartIndex);
    virtualBottomPadding = Math.max(0, index.total - index.offset(virtualEndIndex));
    renderedEntries = entries
      .slice(virtualStartIndex, virtualEndIndex)
      .map((item, itemOffset) => ({ item, index: virtualStartIndex + itemOffset }));
  }

  function updateVisibleRange() {
    recalculateVisibleRange(history.entries, scrollTop, viewportHeight);
  }

  function measureClipboardItem(node: HTMLElement) {
    const update = () => {
      const id = Number(node.dataset.clipboardId);
      const height = node.getBoundingClientRect().height;
      if (!Number.isFinite(id) || height <= 0 || Math.abs((rowHeights.get(id) ?? 0) - height) < 0.5) return;
      if (nearby && !positionAnchor) positionAnchor = viewportAnchor();
      ensureHeightIndex().set(id, height + ROW_GAP);
      rowHeights.set(id, height);
      scheduleListMetricsUpdate();
    };
    const observer = new ResizeObserver(update);
    observer.observe(node);
    requestAnimationFrame(update);
    return { destroy: () => observer.disconnect() };
  }

  function scheduleListMetricsUpdate() {
    if (scrollAnimationFrame) return;
    scrollAnimationFrame = requestAnimationFrame(() => {
      scrollAnimationFrame = 0;
      if (positionAnchor && scrollElement) {
        const top = anchorTop(positionAnchor);
        if (top !== null) scrollElement.scrollTop = top;
      }
      scrollTop = scrollElement?.scrollTop ?? 0;
      viewportHeight = scrollElement?.clientHeight ?? viewportHeight;
      updateVisibleRange();
      void maybeLoadMore();
    });
  }

  function observeListContent(node: HTMLElement) {
    const update = () => scheduleListMetricsUpdate();
    const observer = new ResizeObserver(update);
    observer.observe(node);
    requestAnimationFrame(update);
    return { destroy: () => observer.disconnect() };
  }

  onMount(() => {
    let disposed = false;
    const scope = new SubscriptionScope();

    void (async () => {
      // This webview is created hidden and then reused. Register its visibility
      // lifecycle before settings and metadata I/O so an early shortcut cannot
      // restore stale focus or list metrics.
      await scope.add(clipboardBridge.onShown(() => {
        windowVisible = true;
        const shouldFocusSearch = currentSettings?.clipboardAutoFocusSearch ?? true;
        resetRestoredFocus();
        keyboardMode = shouldFocusSearch ? "search" : "results";
        void load().then(() => {
          requestAnimationFrame(() => {
            scheduleListMetricsUpdate();
            if (shouldFocusSearch) focusSearch();
            else focusSelectedRow();
          });
        });
      }));
      await scope.add(clipboardBridge.onHidden(() => {
        windowVisible = false;
        loadGeneration += 1;
        loading = false;
        loadingMore = false;
        locating = false;
        nearby = null;
        navigationError = "";
        positionAnchor = null;
        loadedQueryKey = null;
        window.clearTimeout(debounceTimer);
        clearThumbnailCache();
        resetRestoredFocus();
        clearHtmlPreviewCache();
        keyboardMode = "search";
        labelFilterOpen = false;
        previewGeneration++;
        previewDialogOpen = false;
        previewingItem = null;
        previewModel = null;
        previewError = "";
        previewActionBusy = false;
        previewSourceWarning = "";
        search = "";
        selectedId = null;
        selectedIds = [];
        scrollElement?.scrollTo({ top: 0 });
        scrollTop = 0;
        history = { revision: 0, entries: [], nextCursor: null, totalCount: 0 };
        rowHeights = new Map();
      }));

      const settings = await loadAppSettings();
      if (!disposed) {
        const sortChanged = settings.clipboardSortBy !== sortBy;
        applySettings(settings);
        requestAnimationFrame(() => {
          if (settings.clipboardAutoFocusSearch) focusSearch();
          else focusSelectedRow();
        });
        appearanceMedia = window.matchMedia("(prefers-color-scheme: dark)");
        handleSystemThemeChange = () => currentSettings?.theme === "system" && applyTheme(currentSettings);
        appearanceMedia.addEventListener("change", handleSystemThemeChange);
        if (sortChanged && !nearby) void load();
      }
      await scope.add(onAppSettingsChanged((next) => {
        const sortChanged = next.clipboardSortBy !== sortBy;
        applySettings(next);
        if (sortChanged && !nearby) void load();
      }));
      windowPinned = await clipboardBridge.pinned();
      nearbyPeers = await clipboardBridge.nearbyPeers().catch(() => []);
      labels = await clipboardBridge.labels().catch(() => []);
      const previewParams = new URLSearchParams(window.location.search);
      if (import.meta.env.DEV && previewParams.get("preview-theme") === "dark") {
        applyTheme({ theme: "dark" });
      }
      if (import.meta.env.DEV && previewParams.get("preview-dialog") === "labels") {
        await load();
        const previewItem = history.entries.find((item) => item.labels.length > 0) ?? history.entries[0];
        if (previewItem) await beginLabels(previewItem);
      }
      if (import.meta.env.DEV && previewParams.has("preview-nearby")) {
        setSearchState("ChatGPT", "all", null);
        await load();
        const previewItem = history.entries.find((item) => item.sourceApp === "ChatGPT");
        if (previewItem) await viewNearby(previewItem);
      }
      await scope.add(clipboardBridge.onPinChanged((value) => (windowPinned = value)));
      await scope.add(clipboardBridge.onContinuousPasteProgress((progress) => {
        continuousPaste = progress;
      }));
      await scope.add(clipboardBridge.onContinuousPasteError((reason) => {
        continuousPaste = { current: 0, total: 0, active: false };
        error = reason;
      }));
      await scope.add(clipboardBridge.onChanged(() => {
        historyChangeCount += 1;
        if (nearby) nearby = { ...nearby, updated: true };
        else if (!locating) void load();
      }));
      await scope.add(clipboardBridge.onOcrChanged(() => {
        if (!nearby && !locating && debouncedSearch.trim()) void load();
      }));
    })().catch((reason) => { if (!disposed) error = String(reason); });

    window.addEventListener("keydown", handleKeyDown);
    scrollResizeObserver = new ResizeObserver(() => scheduleListMetricsUpdate());
    if (scrollElement) scrollResizeObserver.observe(scrollElement);
    viewportHeight = scrollElement?.clientHeight ?? 0;
    return () => {
      disposed = true;
      window.clearTimeout(debounceTimer);
      cancelAnimationFrame(scrollAnimationFrame);
      window.removeEventListener("keydown", handleKeyDown);
      scrollResizeObserver?.disconnect();
      if (appearanceMedia && handleSystemThemeChange) appearanceMedia.removeEventListener("change", handleSystemThemeChange);
      scope.dispose();
    };
  });

  $: document.title = tr("ArcRelay 剪贴板", language);

  function applyLanguage(value: LanguagePreference) {
    setLanguage(value);
  }

  function applyTheme(settings: Pick<AppSettings, "theme">) {
    const resolvedTheme = settings.theme === "system"
      ? (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light")
      : settings.theme;
    document.documentElement.dataset.theme = resolvedTheme;
    document.documentElement.style.colorScheme = resolvedTheme;
    const themeColor = resolvedTheme === "dark" ? "#1c1e24" : "#ffffff";
    document.querySelector<HTMLMetaElement>('meta[name="theme-color"]')?.setAttribute("content", themeColor);
  }

  function applySettings(settings: AppSettings) {
    if (currentSettings && settings.revision < currentSettings.revision) return;
    currentSettings = settings;
    sortBy = settings.clipboardSortBy;
    applyLanguage(settings.language);
    applyTheme(settings);
  }

  function itemTimestamp(item: ClipboardItem) {
    return (nearby?.sortBy ?? sortBy) === "createdAt" ? item.firstCapturedAtMs : item.updatedAtMs;
  }

  function previewTimestamp(item: ClipboardItem) {
    return new Intl.DateTimeFormat(localeFor(language), {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(itemTimestamp(item));
  }

  function timelineTimestamp(timestamp: number) {
    return new Intl.DateTimeFormat(localeFor(language), { dateStyle: "short", timeStyle: "short" }).format(timestamp);
  }

  function kindForFilter(value: ClipboardFilter): ClipboardKind | null {
    if (value === "text") return "text";
    if (value === "image") return "image";
    if (value === "files") return "files";
    return null;
  }

  async function load({ append = false, cursor = null }: { append?: boolean; cursor?: ClipboardCursor | null } = {}) {
    if (!windowVisible) return;
    if (nearby) return refreshNearby();
    const generation = append ? loadGeneration : ++loadGeneration;
    if (!append) {
      loadedQueryKey = historyQueryKey(debouncedSearch, filter, selectedLabelFilter);
      locating = false;
      loadingMore = false;
    }
    append ? (loadingMore = true) : (loading = true);
    try {
      const page = await clipboardBridge.history({
        search: debouncedSearch,
        kind: kindForFilter(filter),
        favoriteOnly: filter === "favorites",
        labelIds: selectedLabelFilter ? [selectedLabelFilter] : [],
        cursor,
        limit: FETCH_SIZE,
      });
      if (generation !== loadGeneration) return;
      history = {
        revision: page.revision,
        entries: append ? [...history.entries, ...page.entries] : page.entries,
        nextCursor: page.nextCursor,
        totalCount: page.totalCount ?? history.totalCount,
      };
      if (!append) {
        rowHeights = new Map(page.entries.flatMap((item) => {
          const height = rowHeights.get(item.id);
          return height === undefined ? [] : [[item.id, height] as const];
        }));
        selectedId = page.entries[0]?.id ?? null;
        scrollElement?.scrollTo({ top: 0 });
        scrollTop = 0;
      }
      error = "";
    } catch (reason) {
      if (generation === loadGeneration) error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (generation === loadGeneration) {
        if (append) loadingMore = false;
        loading = false;
      }
      requestAnimationFrame(() => {
        updateVisibleRange();
        void maybeLoadMore();
      });
    }
  }

  function viewportAnchor() {
    return ensureHeightIndex().anchor(scrollElement?.scrollTop ?? scrollTop);
  }

  async function restorePosition(anchor: ScrollAnchor | null, fallback: number, generation: number) {
    positionAnchor = anchor;
    scrollTop = (anchor && anchorTop(anchor)) ?? fallback;
    updateVisibleRange();
    await tick();
    if (generation !== loadGeneration) return;
    if (scrollElement) scrollElement.scrollTop = scrollTop;
    scheduleListMetricsUpdate();
  }

  function searchSnapshot(): SearchSnapshot {
    return {
      search: debouncedSearch, filter, labelId: selectedLabelFilter, sortBy,
      history, selectedId, selectedIds: [...selectedIds],
      scrollTop, scrollAnchor: viewportAnchor(), rowHeights: new Map(rowHeights), keyboardMode,
    };
  }

  async function viewNearby(item: ClipboardItem) {
    const snapshot = nearby?.snapshot ?? searchSnapshot();
    const timelineSort = nearby?.sortBy ?? sortBy;
    const generation = ++loadGeneration;
    const changeCount = historyChangeCount;
    locating = true;
    loading = false;
    loadingMore = false;
    navigationError = "";
    try {
      const page = await clipboardBridge.timeline({ type: "around", id: item.id }, timelineSort);
      if (generation !== loadGeneration || !windowVisible) return;
      if (!page?.anchor) {
        navigationError = tr("这条记录已被删除或清理，无法查看附近记录。", language);
        return;
      }
      setSearchState("", "all", null);
      nearby = {
        snapshot, anchor: page.anchor, sortBy: timelineSort,
        newerCursor: page.newerCursor, olderCursor: page.olderCursor,
        updated: historyChangeCount !== changeCount,
      };
      history = { revision: page.revision, entries: page.entries, nextCursor: page.olderCursor, totalCount: page.totalCount };
      selectedId = page.anchor.id;
      selectedIds = [];
      error = "";
      keyboardMode = "results";
      rowHeights = new Map();
      await tick();
      if (generation !== loadGeneration) return;
      viewportHeight = scrollElement?.clientHeight ?? viewportHeight;
      const target = page.entries.find(entry => entry.id === page.anchor?.id)!;
      await restorePosition({ id: target.id, offset: (rowBlockHeight(target) - viewportHeight) / 2 }, 0, generation);
      if (generation === loadGeneration) {
        await tick();
        rowElement(target)?.focus({ preventScroll: true });
      }
    } catch (reason) {
      if (generation === loadGeneration) navigationError = String(reason);
    } finally {
      if (generation === loadGeneration) locating = false;
    }
  }

  async function returnToSearch() {
    if (!nearby) return;
    const snapshot = nearby.snapshot;
    abandonNearby();
    const generation = loadGeneration;
    setSearchState(snapshot.search, snapshot.filter, snapshot.labelId);
    history = snapshot.history;
    selectedId = snapshot.selectedId;
    selectedIds = [...snapshot.selectedIds];
    rowHeights = new Map(snapshot.rowHeights);
    keyboardMode = snapshot.keyboardMode;
    error = "";
    await restorePosition(snapshot.scrollAnchor, snapshot.scrollTop, generation);
    if (generation !== loadGeneration) return;
    if (snapshot.sortBy !== sortBy) {
      // The user may have changed global settings in another window. Never
      // render a saved old ordering with timestamps from a different ordering.
      positionAnchor = null;
      await load();
    }
    if (generation !== loadGeneration) return;
    if (keyboardMode === "search") focusSearch();
    else { await tick(); rowElement()?.focus({ preventScroll: true }); }
  }

  async function refreshNearby() {
    const context = nearby;
    if (!context || locating) return;
    const anchor = viewportAnchor();
    const generation = ++loadGeneration;
    const changeCount = historyChangeCount;
    loading = true;
    loadingMore = false;
    navigationError = "";
    try {
      const page = await clipboardBridge.timeline({ type: "around", id: anchor?.id ?? context.anchor.id }, context.sortBy);
      if (generation !== loadGeneration || !nearby) return;
      if (!page) {
        navigationError = tr("这条记录已被删除或清理，请返回搜索重新选择。", language);
        return;
      }
      nearby = { ...context, newerCursor: page.newerCursor, olderCursor: page.olderCursor, updated: historyChangeCount !== changeCount };
      history = { revision: page.revision, entries: page.entries, nextCursor: page.olderCursor, totalCount: page.totalCount };
      selectedId = page.entries.some(item => item.id === selectedId) ? selectedId : page.anchor?.id ?? null;
      selectedIds = selectedIds.filter(id => page.entries.some(item => item.id === id));
      error = "";
      await restorePosition(anchor, scrollTop, generation);
    } catch (reason) {
      if (generation === loadGeneration) navigationError = String(reason);
    } finally {
      if (generation === loadGeneration) loading = false;
    }
  }

  async function loadTimelinePage(newer: boolean) {
    const context = nearby;
    const cursor = newer ? context?.newerCursor : context?.olderCursor;
    if (!context || !cursor || loading || loadingMore || locating) return;
    const generation = loadGeneration;
    loadingMore = true;
    navigationError = "";
    try {
      const page = await clipboardBridge.timeline({ type: newer ? "newer" : "older", cursor }, context.sortBy, FETCH_SIZE);
      if (generation !== loadGeneration || !nearby) return;
      if (!page) throw new Error(tr("没有找到剪贴板记录", language));
      const anchor = viewportAnchor();
      nearby = {
        ...nearby,
        newerCursor: newer ? page.newerCursor : nearby.newerCursor,
        olderCursor: newer ? nearby.olderCursor : page.olderCursor,
        updated: nearby.updated || page.revision !== history.revision,
      };
      history = {
        revision: page.revision, entries: mergeTimelineEntries(history.entries, page, newer),
        nextCursor: nearby.olderCursor, totalCount: page.totalCount ?? history.totalCount,
      };
      await restorePosition(anchor, scrollTop, generation);
    } catch (reason) {
      if (generation === loadGeneration) navigationError = String(reason);
    } finally {
      if (generation === loadGeneration) loadingMore = false;
    }
  }

  function releasePositionAnchor() {
    positionAnchor = null;
  }

  function selectIndex(index: number) {
    positionAnchor = null;
    const item = history.entries[index];
    if (!item) return;
    selectedId = item.id;
    const top = offsetForIndex(index);
    const bottom = top + rowBlockHeight(item);
    if (top < scrollTop) scrollElement?.scrollTo({ top });
    else if (bottom > scrollTop + viewportHeight) {
      scrollElement?.scrollTo({ top: Math.max(0, bottom - viewportHeight) });
    }
    requestAnimationFrame(() => {
      scrollElement
        ?.querySelector<HTMLElement>(`[data-clipboard-id="${item.id}"]`)
        ?.scrollIntoView({ block: "nearest" });
    });
  }

  function rowElement(item: ClipboardItem | null = selectedItem) {
    if (!item || !scrollElement) return null;
    return scrollElement.querySelector<HTMLElement>(`[data-clipboard-id="${item.id}"] .clipboard-row`);
  }

  function focusSearch({ select = false }: { select?: boolean } = {}) {
    keyboardMode = "search";
    searchInput?.focus();
    if (select) searchInput?.select();
  }

  function resetRestoredFocus() {
    const activeElement = document.activeElement;
    if (activeElement instanceof HTMLElement) activeElement.blur();
  }

  function cancelSearch() {
    if (nearby) {
      abandonNearby();
      void load();
    }
    search = "";
    keyboardMode = "results";
    focusSelectedRow();
  }

  function focusSelectedRow() {
    const item = selectedItem ?? history.entries[0] ?? null;
    if (!item) return;
    selectedId = item.id;
    keyboardMode = "results";
    requestAnimationFrame(() => rowElement(item)?.focus({ preventScroll: true }));
  }

  function focusRow(item: ClipboardItem) {
    selectedId = item.id;
    keyboardMode = "results";
  }

  function selectRow(event: MouseEvent | undefined, item: ClipboardItem) {
    const toggleMulti = Boolean(event?.metaKey || event?.ctrlKey) || selectedIds.length > 0;
    if (!toggleMulti) {
      focusRow(item);
      return;
    }
    selectedIds = toggleSelectedId(selectedIds, item.id);
    selectedId = selectedIds.length > 0 ? null : item.id;
    keyboardMode = "results";
  }

  function clearMultiSelection() {
    const fallback = history.entries.find((item) => selectedIds.includes(item.id)) ?? history.entries[0] ?? null;
    selectedIds = [];
    selectedId = fallback?.id ?? null;
  }

  async function pasteSelectedItems() {
    if (selectedIds.length === 0 || multiPasting) return;
    multiPasting = true;
    error = "";
    try {
      await clipboardBridge.pasteRecords([...selectedIds]);
      clearMultiSelection();
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      multiPasting = false;
    }
  }

  async function startContinuousPaste() {
    if (selectedIds.length === 0 || multiPasting) return;
    multiPasting = true;
    error = "";
    try {
      continuousPaste = await clipboardBridge.startContinuousPaste([...selectedIds]);
      clearMultiSelection();
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      multiPasting = false;
    }
  }

  async function stopContinuousPaste() {
    continuousPaste = await clipboardBridge.stopContinuousPaste();
  }

  async function pasteItem(item: ClipboardItem, mode: ClipboardPasteMode = "source") {
    if (ocrPasting) return;
    if (mode === "plain_text" && item.kind === "files") {
      error = uiTranslate("文件条目不能粘贴为文本", $uiLanguage);
      return;
    }
    const recognizingImage = mode === "plain_text" && item.kind === "image";
    if (recognizingImage) {
      ocrPasting = true;
      error = "";
    }
    try {
      await clipboardBridge.pasteAs(item.id, mode);
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (recognizingImage) ocrPasting = false;
    }
  }

  async function copyItem(item: ClipboardItem) {
    try {
      await clipboardBridge.copy(item.id);
      await clipboardBridge.hide();
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    }
  }

  async function toggleFavorite(item: ClipboardItem) {
    try {
      await clipboardBridge.setFavorite(item.id, !item.favorite);
      history = {
        ...history,
        entries: history.entries.map((entry) => entry.id === item.id ? { ...entry, favorite: !entry.favorite } : entry),
      };
      if (filter === "favorites" && item.favorite) {
        await load();
        focusSelectedRow();
      }
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    }
  }

  async function beginPreview(item: ClipboardItem) {
    const generation = ++previewGeneration;
    previewingItem = item;
    previewModel = null;
    previewError = "";
    previewActionBusy = false;
    previewSourceWarning = "";
    previewLoading = (item.kind === "text" || item.kind === "html") && !item.sensitive;
    previewDialogOpen = true;
    if (!previewLoading) return;
    try {
      const model = await clipboardBridge.textSegments(item);
      if (generation === previewGeneration && previewDialogOpen) previewModel = model;
    } catch (reason) {
      if (generation === previewGeneration) previewError = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (generation === previewGeneration) previewLoading = false;
    }
  }

  async function maybeLoadMore() {
    if (nearby) {
      if (!windowVisible || loading || loadingMore || locating || navigationError || !scrollElement) return;
      if (nearby.newerCursor && scrollTop < viewportHeight * 0.75) await loadTimelinePage(true);
      else if (nearby.olderCursor && scrollElement.scrollHeight - (scrollTop + viewportHeight) < viewportHeight * 1.5) await loadTimelinePage(false);
      return;
    }
    if (locating) return;
    if (!windowVisible || loading || loadingMore || !history.nextCursor || !scrollElement) return;
    const remaining = scrollElement.scrollHeight - (scrollTop + viewportHeight);
    if (remaining > viewportHeight * 1.5) return;
    await load({ append: true, cursor: history.nextCursor });
  }

  async function moveSelection(delta: -1 | 1, focusResult = keyboardMode === "results") {
    if (history.entries.length === 0) return;
    let currentIndex = selectedIndex >= 0 ? selectedIndex : visibleStartIndex;
    const currentId = history.entries[currentIndex]?.id;
    if (nearby && ((delta < 0 && currentIndex === 0) || (delta > 0 && currentIndex === history.entries.length - 1))) {
      await loadTimelinePage(delta < 0);
      currentIndex = Math.max(0, history.entries.findIndex(item => item.id === currentId));
    } else if (!nearby && delta > 0 && currentIndex >= history.entries.length - 1 && history.nextCursor) {
      await load({ append: true, cursor: history.nextCursor });
    }
    const nextIndex = Math.min(history.entries.length - 1, Math.max(0, currentIndex + delta));
    const item = history.entries[nextIndex];
    selectIndex(nextIndex);
    if (focusResult && item) requestAnimationFrame(() => rowElement(item)?.focus({ preventScroll: true }));
  }

  function scrollByViewport(direction: -1 | 1) {
    positionAnchor = null;
    if (!scrollElement) return;
    const behavior = window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
    scrollElement.scrollBy({ top: direction * Math.max(1, viewportHeight), behavior });
  }

  function handleListScroll() {
    scheduleListMetricsUpdate();
  }

  function handleKeyDown(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (previewDialogOpen) {
      const previewAction = resolvePreviewKeyboardAction(event);
      if (previewAction) {
        event.preventDefault();
        event.stopPropagation();
        if (previewAction === "close") previewDialogOpen = false;
        return;
      }
    }
    if (event.key === "Escape") {
      if (selectedIds.length > 0) {
        event.preventDefault();
        clearMultiSelection();
        return;
      }
      if (previewDialogOpen) {
        previewDialogOpen = false;
        return;
      }
      if (editDialogOpen) {
        editDialogOpen = false;
        return;
      }
      if (labelDialogOpen) {
        labelDialogOpen = false;
        return;
      }
      if (labelFilterOpen) {
        labelFilterOpen = false;
        return;
      }
      if (nearby) {
        event.preventDefault();
        void returnToSearch();
        return;
      }
      event.preventDefault();
      void clipboardBridge.hide();
      return;
    }
    if (previewDialogOpen || editDialogOpen || labelDialogOpen) return;

    const target = event.target;
    const editingTarget = target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement || (target instanceof HTMLElement && target.isContentEditable);
    if (editingTarget && target !== searchInput) return;
    const element = target instanceof Element ? target : null;
    const rowTarget = element?.closest(".clipboard-row");
    if (!editingTarget && !rowTarget && element?.closest("button, select, [role='option']")) return;

    if ((event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey && event.code === "KeyA" && target !== searchInput) {
      event.preventDefault();
      selectedIds = appendSelectedIds(selectedIds, history.entries.map((item) => item.id));
      selectedId = null;
      keyboardMode = "results";
      return;
    }

    if (selectedIds.length > 0 && event.key === "Enter" && !event.altKey) {
      event.preventDefault();
      void pasteSelectedItems();
      return;
    }

    const mode: ClipboardKeyboardMode = target === searchInput ? "search" : rowTarget ? "results" : keyboardMode;
    const action = resolveClipboardKeyboardAction(event, mode);
    if (!action) return;
    event.preventDefault();

    if (action.type === "focusSearch") {
      focusSearch({ select: event.code === "KeyF" });
    } else if (action.type === "cancelSearch") {
      cancelSearch();
    } else if (action.type === "focusResults") {
      focusSelectedRow();
    } else if (action.type === "move") {
      void moveSelection(action.delta, mode === "results");
    } else if (action.type === "page") {
      scrollByViewport(action.direction);
    } else if (action.type === "paste" && selectedItem) {
      void pasteItem(selectedItem, action.plainText ? "plain_text" : "source");
    } else if (action.type === "pasteRank" && !nearby) {
      const item = history.entries[action.index];
      if (item) void pasteItem(item);
    } else if (action.type === "copy" && selectedItem) {
      void copyItem(selectedItem);
    } else if (action.type === "favorite" && selectedItem) {
      void toggleFavorite(selectedItem);
    } else if (action.type === "edit" && selectedItem) {
      if (selectedItem.kind === "text") void beginEdit(selectedItem);
      else error = uiTranslate("仅文本条目支持编辑", $uiLanguage);
    } else if (action.type === "labels" && selectedItem) {
      void beginLabels(selectedItem);
    } else if (action.type === "preview" && selectedItem) {
      void beginPreview(selectedItem);
    } else if (action.type === "actions") {
      openSelectedActions();
    }
  }

  async function beginEdit(item: ClipboardItem) {
    editingItem = item;
    try {
      editContent = await clipboardBridge.textContent(item);
      editDialogOpen = true;
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    }
  }

  async function beginSegments(item: ClipboardItem) {
    if (item.kind !== "text" && item.kind !== "html") return;
    await beginPreview(item);
  }

  async function copySegment(content: string) {
    if (previewActionBusy) return;
    previewActionBusy = true;
    try {
      await clipboardBridge.copyText(content);
      previewDialogOpen = false;
    } catch (reason) {
      previewError = reason instanceof Error ? reason.message : String(reason);
      error = previewError;
    } finally {
      previewActionBusy = false;
    }
  }

  async function pasteSegment(content: string) {
    if (previewActionBusy) return;
    previewActionBusy = true;
    try {
      await clipboardBridge.pasteText(content);
      previewDialogOpen = false;
    } catch (reason) {
      previewError = reason instanceof Error ? reason.message : String(reason);
      error = previewError;
    } finally {
      previewActionBusy = false;
    }
  }

  async function saveEdit() {
    if (!editingItem) return;
    await clipboardBridge.editText(editingItem.id, editContent);
    editDialogOpen = false;
    await load();
  }

  async function beginLabels(item: ClipboardItem) {
    labelingItem = item;
    labels = await clipboardBridge.labels();
    selectedLabelIds = item.labels.map((label) => label.id);
    newLabelName = "";
    newLabelColor = "#5B5FF0";
    labelDialogOpen = true;
  }

  async function openContextMenu(event: MouseEvent, item: ClipboardItem) {
    event.preventDefault();
    event.stopPropagation();
    selectedId = item.id;
    keyboardMode = "results";
    if (labels.length === 0) labels = await clipboardBridge.labels().catch(() => []);
    await showClipboardContextMenu(event, item, labels, nearbyPeers, language, {
      nearby: !loading && !locating && search === debouncedSearch && debouncedSearch.trim() ? () => viewNearby(item) : undefined,
      reload: load,
      edit: () => beginEdit(item),
      segment: () => beginSegments(item),
      manageLabels: () => beginLabels(item),
      paste: (mode) => pasteItem(item, mode),
      sendFiles: async (peerId) => { await clipboardBridge.sendFiles(item.id, peerId); },
    }).catch((reason) => {
      error = reason instanceof Error ? reason.message : String(reason);
    });
  }

  function openSelectedActions() {
    if (!selectedItem) return;
    const bounds = rowElement(selectedItem)?.getBoundingClientRect();
    const event = new MouseEvent("contextmenu", {
      clientX: bounds ? Math.min(bounds.right - 24, window.innerWidth - 24) : window.innerWidth / 2,
      clientY: bounds ? Math.min(bounds.top + 42, window.innerHeight - 80) : window.innerHeight / 2,
    });
    void openContextMenu(event, selectedItem);
  }

  async function saveLabels() {
    if (!labelingItem) return;
    await clipboardBridge.setLabels(labelingItem.id, selectedLabelIds);
    labelDialogOpen = false;
    await load();
  }

  async function createLabel() {
    if (!newLabelName.trim()) return;
    const created = await clipboardBridge.createLabel(newLabelName.trim(), newLabelColor);
    labels = [...labels, created];
    selectedLabelIds = [...selectedLabelIds, created.id];
    newLabelName = "";
    requestAnimationFrame(() => newLabelInput?.focus());
  }

  function handleNewLabelKeyDown(event: KeyboardEvent) {
    if (event.key !== "Enter" || !newLabelName.trim()) return;
    event.preventDefault();
    void createLabel();
  }

  async function deleteLabel(labelId: string) {
    await clipboardBridge.deleteLabel(labelId);
    labels = labels.filter((label) => label.id !== labelId);
    selectedLabelIds = selectedLabelIds.filter((id) => id !== labelId);
    if (selectedLabelFilter === labelId) selectedLabelFilter = null;
    await load();
  }

  function toggleLabel(labelId: string) {
    selectedLabelIds = selectedLabelIds.includes(labelId)
      ? selectedLabelIds.filter((id) => id !== labelId)
      : [...selectedLabelIds, labelId];
  }

  async function toggleWindowPinned() {
    const pinned = !windowPinned;
    windowPinned = pinned;
    try {
      await clipboardBridge.setPinnedWindow(pinned);
    } catch (reason) {
      windowPinned = !pinned;
      error = reason instanceof Error ? reason.message : String(reason);
    }
  }

  function startWindowDrag(event: MouseEvent) {
    if (event.button !== 0 || (event.target as HTMLElement).closest("button, input, label, a")) return;
    const resizeEdge = 10;
    if (event.clientX <= resizeEdge || event.clientY <= resizeEdge || window.innerWidth - event.clientX <= resizeEdge || window.innerHeight - event.clientY <= resizeEdge) return;
    void clipboardBridge.startDragging();
  }

  function suppressWebviewContextMenu(event: MouseEvent) {
    event.preventDefault();
  }

  function handleWindowPointerDown(event: PointerEvent) {
    const target = event.target as HTMLElement;
    if (labelFilterOpen && !target.closest(".label-filter-control")) labelFilterOpen = false;
  }

  function selectLabelFilter(labelId: string | null) {
    abandonNearby();
    selectedLabelFilter = labelId;
    labelFilterOpen = false;
    loadedQueryKey = null;
    reloadForQuery(filter, debouncedSearch, selectedLabelFilter);
  }

  function windowDrag(node: HTMLElement) {
    node.addEventListener("mousedown", startWindowDrag);
    return { destroy: () => node.removeEventListener("mousedown", startWindowDrag) };
  }
</script>

<svelte:window oncontextmenu={suppressWebviewContextMenu} on:pointerdown={handleWindowPointerDown} />

<main class="clipboard-window">
  <header class="clipboard-header drag-region" use:windowDrag>
    <button class:active={windowPinned} class="icon-button pin-button" type="button" aria-label={tr(windowPinned ? "取消锁定窗口" : "锁定窗口", language)} aria-pressed={windowPinned} title={tr(windowPinned ? "已锁定，失焦后保持显示" : "锁定后失焦不隐藏", language)} on:click={toggleWindowPinned}>
      <PushPin size={18} weight={windowPinned ? "fill" : "regular"} />
    </button>
    <label class:keyboard-active={keyboardMode === "search"} class="clipboard-search">
      <input bind:this={searchInput} bind:value={search} placeholder={tr("输入开始搜索…", language)} on:input={handleSearchInput} on:focus={() => (keyboardMode = "search")} />
      <kbd>{primaryShortcutLabel}F</kbd>
    </label>
    <button class="icon-button search-button" type="button" aria-label={tr("搜索", language)} on:click={() => searchInput?.focus()}><MagnifyingGlass size={20} /></button>
    <span class="brand-mark" aria-label="ArcRelay"><BrandLogo size={23} /></span>
  </header>

  <nav class="clipboard-filters" aria-label={tr("剪贴板类型", language)}>
    <div class="filter-group">
      {#each filters as item (item.id)}
        <button type="button" class:active={filter === item.id} class="filter-button" aria-label={tr(item.label, language)} title={tr(item.label, language)} on:click={() => changeFilter(item.id)}>
          <svelte:component this={item.icon} size={item.size} weight={item.weight} />
        </button>
      {/each}
    </div>
    <div class:open={labelFilterOpen} class="label-filter-control">
      <button
        class="label-filter-button"
        type="button"
        aria-label={selectedLabel ? `${tr("管理标签", language)}：${selectedLabel.name}` : tr("管理标签", language)}
        title={selectedLabel?.name ?? (uiTranslate("全部标签", $uiLanguage))}
        aria-haspopup="listbox"
        aria-expanded={labelFilterOpen}
        on:click={() => (labelFilterOpen = !labelFilterOpen)}
      >
        <Tag size={19} weight={selectedLabel ? "fill" : "regular"} />
        {#if selectedLabel}<span class="label-filter-selection-dot" style:--label-color={selectedLabel.color}></span>{/if}
      </button>
      {#if labelFilterOpen}
        <div class="label-filter-menu" role="listbox" aria-label={tr("管理标签", language)}>
          <button class:active={selectedLabelFilter === null} type="button" role="option" aria-selected={selectedLabelFilter === null} on:click={() => selectLabelFilter(null)}>
            <Tag size={14} /><span>{uiTranslate("全部标签", $uiLanguage)}</span>
            {#if selectedLabelFilter === null}<Check size={14} weight="bold" />{/if}
          </button>
          {#each labels as label (label.id)}
            <button class:active={selectedLabelFilter === label.id} type="button" role="option" aria-selected={selectedLabelFilter === label.id} on:click={() => selectLabelFilter(label.id)}>
              <span class="label-filter-dot" style:--label-color={label.color}></span><span>{label.name}</span>
              {#if selectedLabelFilter === label.id}<Check size={14} weight="bold" />{/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </nav>

  <div class="clipboard-navigation">
    {#if nearby}
      <div class="nearby-navigation">
        <button type="button" class="nearby-back" on:click={returnToSearch} title={t("返回搜索：{query}", language, { query: nearby.snapshot.search })}>
          <ArrowLeft size={16} /><span>{tr("返回搜索", language)}</span>
        </button>
        <div class="nearby-context" role="status" aria-live="polite">
          <strong>{t("{time} 附近", language, { time: timelineTimestamp(nearby.anchor.sortAtMs) })}</strong>
          <span>{tr(nearby.sortBy === "createdAt" ? "按首次记录时间" : "按最近使用时间", language)} · {tr("全部类型与标签", language)}</span>
        </div>
        {#if nearby.updated}
          <button type="button" class="nearby-update" disabled={loading || locating} on:click={refreshNearby}>{tr("历史有更新", language)}</button>
        {/if}
      </div>
    {/if}
    {#if locating}
      <div class="navigation-message" role="status">{tr("正在定位附近记录…", language)}</div>
    {/if}
    {#if navigationError}
      <div class="navigation-message navigation-error" role="alert">
        <span>{navigationError}</span>
        <button type="button" class="icon-button" aria-label={tr("关闭提示", language)} on:click={() => (navigationError = "")}><X size={16} /></button>
      </div>
    {/if}
  </div>

  <section class:keyboard-active={keyboardMode === "results"} class="clipboard-list" aria-label={tr("剪贴板记录", language)} bind:this={scrollElement} aria-busy={loading || loadingMore || locating} on:scroll={handleListScroll} on:wheel|passive={releasePositionAnchor} on:touchstart|passive={releasePositionAnchor} on:pointerdown={releasePositionAnchor}>
    {#if loading && history.entries.length === 0}
      <div class="list-state loading-state">
        <span class="loading-spinner" aria-hidden="true"></span>
        <span>{tr("正在读取剪贴板…", language)}</span>
      </div>
    {:else if error}
      <div class="list-state error-state">{error}</div>
    {:else if history.entries.length === 0}
      <div class="list-state">{tr("没有找到剪贴板记录", language)}</div>
    {:else}
      <div
        class="clipboard-items"
        style:padding-top={`${virtualTopPadding}px`}
        style:padding-bottom={`${virtualBottomPadding}px`}
        use:observeListContent
      >
        {#each renderedEntries as entry (entry.item.id)}
          <div
            data-clipboard-id={entry.item.id}
            data-clipboard-index={entry.index}
            class="clipboard-item"
            class:timeline-target={nearby?.anchor.id === entry.item.id}
            style:--enter-order={Math.min(entry.index, 7)}
            aria-posinset={entry.index + 1}
            aria-setsize={history.entries.length}
            use:measureClipboardItem
          >
            <ClipboardRow
              item={entry.item}
              shortcutIndex={shortcutNumbers.get(entry.index) ?? null}
              shortcutModifier={rowShortcutModifier}
              selected={selectedId === entry.item.id}
              multiSelected={selectedIds.includes(entry.item.id)}
              selectionOrder={selectionOrder(selectedIds, entry.item.id)}
              {language}
              timestamp={itemTimestamp(entry.item)}
              timelineTarget={nearby?.anchor.id === entry.item.id}
              exactTime={Boolean(nearby)}
              previewActive={entry.index >= Math.max(0, visibleStartIndex - 2) && entry.index < visibleEndIndex + 2}
              onSelect={(event) => selectRow(event, entry.item)}
              onFocus={() => selectedIds.length === 0 && focusRow(entry.item)}
              onPaste={(plainText) => selectedIds.length > 0 ? void pasteSelectedItems() : void pasteItem(entry.item, plainText ? "plain_text" : "source")}
              onContextMenu={(event) => void openContextMenu(event, entry.item)}
            />
          </div>
        {/each}
      </div>
    {/if}
  </section>

  {#if selectedIds.length > 0}
    <div class="multi-select-actions" role="toolbar" aria-label={uiTranslate("多选操作", $uiLanguage)}>
      <strong>{t("已选择 {count} 项", language, { count: selectedIds.length })}</strong>
      <button type="button" disabled={multiPasting} on:click={pasteSelectedItems} title={uiTranslate("按选择顺序粘贴", $uiLanguage)}>
        <CopySimple size={17} weight="bold" />
        <span>{uiTranslate("多选粘贴", $uiLanguage)}</span>
      </button>
      <button class="continuous-action" type="button" disabled={multiPasting} on:click={startContinuousPaste} title={uiTranslate("每次按剪贴板快捷键粘贴下一项", $uiLanguage)}>
        <StackSimple size={17} weight="bold" />
        <span>{uiTranslate("连续粘贴", $uiLanguage)}</span>
      </button>
      <button class="clear-selection" type="button" on:click={clearMultiSelection} aria-label={tr("取消", language)} title={tr("取消", language)}><X size={16} /></button>
    </div>
  {:else if continuousPaste.active}
    <div class="multi-select-actions continuous-progress" role="status">
      <StackSimple size={18} weight="bold" />
      <strong>{t("连续粘贴 {current}/{total}", language, { current: continuousPaste.current, total: continuousPaste.total })}</strong>
      <span>{uiTranslate("再次按剪贴板快捷键粘贴下一项", $uiLanguage)}</span>
      <button class="clear-selection" type="button" on:click={stopContinuousPaste} aria-label={uiTranslate("停止连续粘贴", $uiLanguage)} title={uiTranslate("停止连续粘贴", $uiLanguage)}><X size={16} /></button>
    </div>
  {/if}

  <footer class="clipboard-footer drag-region" use:windowDrag>
    <div class:busy={ocrPasting} class="record-count">
      <strong>
        {#if ocrPasting}
          {uiTranslate("正在识别图片文字…", $uiLanguage)}
        {:else if loadingMore && nearby}
          {tr("正在读取附近记录…", language)}
        {:else}
          {t("共 {count} 条", language, { count: history.totalCount ?? history.entries.length })}
        {/if}
      </strong>
    </div>
    <div class="footer-actions">
      <button class:loading class="icon-button" type="button" aria-label={tr("刷新", language)} aria-busy={loading} title={tr("刷新", language)} on:click={() => load()}><ClockCounterClockwise size={22} /></button>
      <button class="icon-button" type="button" aria-label={tr("隐藏", language)} title={tr("隐藏", language)} on:click={() => clipboardBridge.hide()}><X size={22} /></button>
    </div>
  </footer>

  <Dialog.Root bind:open={previewDialogOpen}>
    <Dialog.Portal>
      <Dialog.Overlay class="dialog-overlay" />
      <Dialog.Content class={`dialog-content clipboard-preview-dialog${imagePreviewReady ? " image-preview-dialog" : ""}`}>
        <div class:image-preview-header={imagePreviewReady} class="clipboard-preview-header">
          <div class="clipboard-preview-heading">
            <Dialog.Title class="dialog-title">
              {imagePreviewReady && previewingItem
                ? (previewingItem.sourceApp ?? tr("图片", language))
                : (uiTranslate("剪贴板预览", $uiLanguage))}
            </Dialog.Title>
            {#if previewingItem}
              <Dialog.Description class="clipboard-preview-description">
                {#if imagePreviewReady && previewingItem.width && previewingItem.height}
                  {previewingItem.width}×{previewingItem.height} · {previewTimestamp(previewingItem)}
                {:else}
                  {previewingItem.sourceApp ?? tr("此电脑", language)} · {previewTimestamp(previewingItem)}
                {/if}
              </Dialog.Description>
            {/if}
          </div>
          <div class="clipboard-preview-header-actions">
            {#if imagePreviewReady && previewingItem}
              <button
                class="image-preview-header-paste"
                type="button"
                disabled={previewActionBusy}
                on:click={() => pasteItem(previewingItem!)}
              >{uiTranslate("插入", $uiLanguage)}</button>
            {/if}
            <Dialog.Close class="segment-dialog-close" aria-label={tr("取消", language)} title={tr("取消", language)}><X size={17} /></Dialog.Close>
          </div>
        </div>
        <div
          class:structured-preview={previewingItem && (previewingItem.kind === "text" || previewingItem.kind === "html") && !previewingItem.sensitive}
          class:image-preview={previewingItem?.kind === "image" && !previewingItem.sensitive}
          class="clipboard-preview-body"
        >
          {#if previewLoading}
            <div class="clipboard-preview-state">{uiTranslate("正在读取完整内容…", $uiLanguage)}</div>
          {:else if previewError}
            <div class="clipboard-preview-state error-state">{previewError}</div>
          {:else if previewingItem && (previewingItem.kind === "text" || previewingItem.kind === "html") && !previewingItem.sensitive}
            {#key previewingItem.id}
              <SegmentPicker
                model={previewModel!}
                itemId={previewingItem!.id}
                {language}
                busy={previewActionBusy}
                errorMessage={previewError}
                sourceWarning={previewSourceWarning}
                onCopy={copySegment}
                onPaste={pasteSegment}
                onPasteAll={() => pasteItem(previewingItem!)}
              />
            {/key}
          {:else if previewingItem}
            <ContentPreview
              item={previewingItem}
              {language}
              interactiveImage={true}
              imageSelectionBusy={previewActionBusy}
              onCopyImageText={copySegment}
              onPasteImageText={pasteSegment}
            />
          {/if}
        </div>
        {#if !imagePreviewReady && (!previewingItem || previewLoading || previewError || previewingItem.sensitive || (previewingItem.kind !== "text" && previewingItem.kind !== "html"))}
          <div class="dialog-actions">
            <Dialog.Close class="dialog-button">{uiTranslate("返回", $uiLanguage)}</Dialog.Close>
            {#if previewingItem}
              <button class="dialog-button primary" type="button" disabled={previewActionBusy} on:click={() => pasteItem(previewingItem!)}>
                {previewingItem.kind === "text" || previewingItem.kind === "html"
                  ? (uiTranslate("粘贴全文", $uiLanguage))
                  : (uiTranslate("插入", $uiLanguage))}
              </button>
            {/if}
          </div>
        {/if}
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>

  <Dialog.Root bind:open={editDialogOpen}>
    <Dialog.Portal>
      <Dialog.Overlay class="dialog-overlay" />
      <Dialog.Content class="dialog-content">
        <Dialog.Title class="dialog-title">{tr("编辑文本", language)}</Dialog.Title>
        <textarea class="clipboard-edit-textarea" bind:value={editContent} rows="8"></textarea>
        <div class="dialog-actions">
          <Dialog.Close class="dialog-button">{tr("取消", language)}</Dialog.Close>
          <button class="dialog-button" type="button" disabled={!editContent.trim()} on:click={saveEdit}>{uiTranslate("保存", $uiLanguage)}</button>
        </div>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>

  <ClipboardLabelDialog
    bind:open={labelDialogOpen}
    bind:labels
    bind:selectedLabelIds
    bind:newLabelName
    bind:newLabelColor
    bind:newLabelInput
    {language}
    onSave={saveLabels}
    onCreate={createLabel}
    onDelete={deleteLabel}
    onToggle={toggleLabel}
    onNewLabelKeyDown={handleNewLabelKeyDown}
  />
</main>
