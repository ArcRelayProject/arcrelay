<script lang="ts">
  import { t } from "./localization";
  import { localeFor } from "./localization.ts";
  import { isCommandError } from "./ipc/client";
  import { translate as uiTranslate, language as uiLanguage } from "./i18n";
  import { onMount, tick } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import {
    ArrowClockwise, ArrowLeft, ArrowRight, ArrowUp, CaretDown, CaretRight, Check,
    DotsThree, DownloadSimple, File as FileIcon, FileDoc, FilePdf, FileText,
    FolderOpen, FolderSimple, FolderSimplePlus, GridFour, Image, ListBullets,
    MagnifyingGlass, Monitor, MusicNote, PencilSimple, Rows, SortAscending,
    SortDescending, Trash, UploadSimple, VideoCamera,
  } from "phosphor-svelte";
  import { SubscriptionScope } from "./subscriptions";
  import { bridge } from "./bridge";
  import SystemFolders from './SystemFolders.svelte';
  import { translate } from "./i18n";
  import {
    showRemoteFileBackgroundMenu,
    showRemoteFileContextMenu,
    showRemoteFileTreeMenu,
  } from "./remoteFileNativeContextMenu";
  import {
    ancestorPaths,
    buildRemoteFileTreeRows,
    remoteTreeDirectoryKey,
    type RemoteFileTreeRow,
  } from "./remoteFileTree";
  import {
    type RemoteFileSortDirection as SortDirection,
    type RemoteFileSortKey as SortKey,
  } from "./remoteFileSort";
  import type { LanguagePreference, RemoteFileDevice, RemoteFileEntry, RemoteFileShare, RemoteFileTransferSession } from "./types";

  type ViewMode = "details" | "compact" | "grid";

  const scope = new SubscriptionScope();

  export let notify: (message: string, kind?: "success" | "error") => void;
  export let language: LanguagePreference;
  export let openDeviceRequest: { deviceId: string; sequence: number } | null = null;
  let handledDeviceSequence = -1;
  let deviceLoadRevision = 0;
  $: if (openDeviceRequest && openDeviceRequest.sequence !== handledDeviceSequence) {
    handledDeviceSequence = openDeviceRequest.sequence;
    void loadState(openDeviceRequest.deviceId);
  }

  let devices: RemoteFileDevice[] = [];
  let shares: RemoteFileShare[] = [];
  let entries: RemoteFileEntry[] = [];
  let nextCursor: string | null = null;
  let loadingMore = false;
  let treeDirectories = new Map<string, RemoteFileEntry[]>();
  let expandedShareIds = new Set<string>();
  let expandedFolderKeys = new Set<string>();
  let treeLoadingKeys = new Set<string>();
  let selectedPeerId = "";
  let selectedShareId = "";
  let currentPath = "";
  let selectedPaths = new Set<string>();
  let lastSelectedPath = "";
  let search = "";
  let deviceMenuOpen = false;
  let uploadMenuOpen = false;
  let moreMenuOpen = false;
  let sortMenuOpen = false;
  let draggingIn = false;
  let dragTargetPath = "";
  let draggingOutPath = "";
  let openingPaths = new Set<string>();
  let loading = true;
  let operation = "";
  let pageError = "";
  let folderDialogOpen = false;
  let folderDialogParentPath = "";
  let renameDialogOpen = false;
  let deleteDialogOpen = false;
  let dialogValue = "";
  let sortKey: SortKey = "modified";
  let sortDirection: SortDirection = "descending";
  let viewMode: ViewMode = "details";
  let historyPaths = [""];
  let historyIndex = 0;
  let transfers: RemoteFileTransferSession[] = [];
  let transferPanelOpen = true;
  let thumbnailPaths = new Map<string, string>();
  let thumbnailPending = new Set<string>();
  let thumbnailUnavailable = new Set<string>();
  let webviewScaleFactor = 1;
  let directoryRevision = 0;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  $: selectedDevice = devices.find((device) => device.id === selectedPeerId);
  $: selectedShare = shares.find((share) => share.id === selectedShareId);
  $: selectedEntries = entries.filter((entry) => selectedPaths.has(entry.relativePath));
  $: selected = selectedEntries[0];
  $: sortedEntries = entries;
  $: breadcrumbs = currentPath ? currentPath.split("/") : [];
  $: allVisibleSelected = sortedEntries.length > 0 && sortedEntries.every((entry) => selectedPaths.has(entry.relativePath));
  $: selectionSize = selectedEntries.reduce((total, entry) => total + (entry.kind === "file" ? entry.size : 0), 0);
  $: treeRows = buildRemoteFileTreeRows({
    shares,
    selectedShareId,
    currentPath,
    expandedShareIds,
    expandedFolderKeys,
    directories: treeDirectories,
  });
  $: activeTransfers = transfers.filter((transfer) => transfer.status === "transferring");
  $: visibleTransfers = [
    ...activeTransfers,
    ...transfers.filter((transfer) => transfer.status !== "transferring"),
  ].slice(0, 5);

  const tr = (source: string) => translate(source, language);

  onMount(() => {
    if (!openDeviceRequest) void loadState();
    if (bridge.isTauri()) {
      const webviewWindow = getCurrentWebviewWindow();
      void webviewWindow.scaleFactor().then((factor) => (webviewScaleFactor = factor)).catch(() => undefined);

      void scope.add(webviewWindow.onDragDropEvent((event) => {
        if (event.payload.type === "enter" || event.payload.type === "over") {
          draggingIn = true;
          updateDragTarget(event.payload.position.x, event.payload.position.y);
        }
        if (event.payload.type === "leave") {
          draggingIn = false;
          dragTargetPath = "";
        }
        if (event.payload.type === "drop") {
          draggingIn = false;
          updateDragTarget(event.payload.position.x, event.payload.position.y);
          const targetPath = dragTargetPath || currentPath;
          dragTargetPath = "";
          void uploadPaths(event.payload.paths, targetPath);
        }
      })).catch((error) => notify(String(error), "error"));
      void scope.add(webviewWindow.listen<{ message: string; relativePath: string; status: "synced" | "error" | "conflict" }>("remote-file-edit-status", (event) => {
        notify(event.payload.message, event.payload.status === "synced" ? "success" : "error");
        const changedParent = event.payload.relativePath.split("/").slice(0, -1).join("/");
        if (event.payload.status === "synced" && changedParent === currentPath && !operation) void loadDirectory(currentPath);
      })).catch((error) => notify(String(error), "error"));
      void scope.add(webviewWindow.listen<RemoteFileTransferSession>("remote-file-transfer", (event) => {
        const previous = transfers.find((transfer) => transfer.id === event.payload.id);
        upsertTransfer(event.payload);
        if (previous?.status === event.payload.status) return;
        if (event.payload.status === "completed") {
          notify(event.payload.direction === "upload"
            ? (t("已上传 {name}", language, { name: event.payload.name }))
            : (t("已下载 {name}", language, { name: event.payload.name })), "success");
          if (event.payload.direction === "upload"
            && event.payload.peerId === selectedPeerId
            && event.payload.shareId === selectedShareId
            && event.payload.directoryPath === currentPath
            && !operation) void loadDirectory(currentPath);
        } else if (event.payload.status === "failed") {
          notify(event.payload.error ?? tr("远程文件操作失败"), "error");
        }
      })).then(async () => { for (const session of await bridge.listRemoteFileTransfers()) if (!scope.disposed) upsertTransfer(session); }).catch((error) => notify(String(error), "error"));
    }
    return () => {
      scope.dispose();
      directoryRevision++;
      if (searchTimer) clearTimeout(searchTimer);



    };
  });

  function updateDragTarget(physicalX: number, physicalY: number) {
    const element = document.elementFromPoint(physicalX / webviewScaleFactor, physicalY / webviewScaleFactor);
    dragTargetPath = element?.closest<HTMLElement>("[data-remote-drop-path]")?.dataset.remoteDropPath ?? "";
  }

  function upsertTransfer(session: RemoteFileTransferSession) {
    if (transfers.some((current) => current.id === session.id && current.updatedAtMs > session.updatedAtMs)) return;
    const existing = transfers.find((transfer) => transfer.id === session.id);
    if (existing && (existing.updatedAtMs > session.updatedAtMs
      || (existing.updatedAtMs === session.updatedAtMs && existing.status !== "transferring" && session.status === "transferring"))) return;
    transfers = [session, ...transfers.filter((transfer) => transfer.id !== session.id)]
      .sort((left, right) => right.startedAtMs - left.startedAtMs);
  }

  async function loadState(requestedPeerId?: string) {
    const revision = ++deviceLoadRevision;
    directoryRevision++;
    loading = true;
    pageError = "";
    try {
      const state = await bridge.getRemoteFileState();
      if (scope.disposed || revision !== deviceLoadRevision) return;
      devices = state.devices;
      if (requestedPeerId) {
        selectedPeerId = devices.some(device => device.id === requestedPeerId) ? requestedPeerId : "";
        if (!selectedPeerId) {
          shares = []; entries = []; nextCursor = null;
          pageError = tr("设备已离线或不再支持文件浏览");
          return;
        }
      }
      if (!devices.some((device) => device.id === selectedPeerId)) selectedPeerId = devices[0]?.id ?? "";
      if (selectedPeerId) await loadShares();
    } catch (error) {
      if (!scope.disposed && revision === deviceLoadRevision) pageError = String(error);
    } finally {
      if (!scope.disposed && revision === deviceLoadRevision) loading = false;
    }
  }

  async function chooseDevice(id: string) {
    selectedPeerId = id;
    deviceMenuOpen = false;
    await loadShares();
  }

  async function loadShares() {
    directoryRevision++;
    shares = [];
    entries = [];
    nextCursor = null;
    search = "";
    treeDirectories = new Map();
    expandedShareIds = new Set();
    expandedFolderKeys = new Set();
    treeLoadingKeys = new Set();
    selectedShareId = "";
    currentPath = "";
    selectedPaths = new Set();
    historyPaths = [""];
    historyIndex = 0;
    if (!selectedPeerId) return;
    await runOperation(tr("正在读取共享目录…"), async () => {
      const peerId = selectedPeerId;
      const nextShares = await bridge.listRemoteFileShares(peerId);
      if (scope.disposed || peerId !== selectedPeerId) return;
      shares = nextShares;
      selectedShareId = shares[0]?.id ?? "";
      if (!selectedShareId) return;
      expandedShareIds = new Set([selectedShareId]);
      await loadDirectory("");
      const rootEntries = treeDirectories.get(remoteTreeDirectoryKey(selectedShareId, "")) ?? [];
      const previewPath = !bridge.isTauri() && rootEntries.some((folder) => folder.relativePath === "设计资料") ? "设计资料" : "";
      if (previewPath) {
        historyPaths = ["", previewPath];
        historyIndex = 1;
        await loadDirectory(previewPath);
      }
    });
  }

  async function chooseShare(id: string) {
    selectedShareId = id;
    currentPath = "";
    search = "";
    expandedShareIds = new Set(expandedShareIds).add(id);
    historyPaths = [""];
    historyIndex = 0;
    await loadDirectory("");
  }

  async function loadDirectory(path: string) {
    if (!selectedPeerId || !selectedShareId) return false;
    const revision = ++directoryRevision;
    const query = search.trim();
    nextCursor = null;
    loadingMore = false;
    let loaded = false;
    let thumbnailEntries: RemoteFileEntry[] = [];
    operation = query ? tr("正在搜索文件…") : tr("正在读取目录…");
    pageError = "";
    try {
      const page = await bridge.listRemoteDirectory(
        selectedPeerId,
        selectedShareId,
        path,
        null,
        query,
        sortKey,
        sortDirection,
      );
      if (revision !== directoryRevision) return false;
      entries = page.entries;
      nextCursor = page.nextCursor;
      thumbnailEntries = page.entries;
      if (!query) rememberTreeDirectory(selectedShareId, path, page.entries);
      expandTreeAncestors(selectedShareId, path);
      currentPath = path;
      selectedPaths = new Set();
      lastSelectedPath = "";
      loaded = true;
    } catch (error) {
      if (revision === directoryRevision) {
        pageError = String(error);
        notify(String(error), "error");
      }
    } finally {
      if (revision === directoryRevision) operation = "";
    }
    if (loaded) {
      // Let the freshly loaded directory render before thumbnail I/O starts.
      // This also ensures thumbnail keys use the directory's active peer/share.
      await tick();
      void queueThumbnails(thumbnailEntries);
    }
    return loaded;
  }

  async function loadMoreDirectory() {
    if (!nextCursor || loadingMore || operation || !selectedPeerId || !selectedShareId) return;
    const revision = directoryRevision;
    const cursor = nextCursor;
    const path = currentPath;
    const query = search.trim();
    loadingMore = true;
    try {
      const page = await bridge.listRemoteDirectory(
        selectedPeerId,
        selectedShareId,
        path,
        cursor,
        query,
        sortKey,
        sortDirection,
      );
      if (revision !== directoryRevision || path !== currentPath || query !== search.trim()) return;
      entries = [...entries, ...page.entries];
      nextCursor = page.nextCursor;
      if (!query) rememberTreeDirectory(selectedShareId, path, entries);
      await tick();
      void queueThumbnails(page.entries);
    } catch (error) {
      if (revision === directoryRevision) {
        if (isCommandError(error, "directorySnapshotExpired")) {
          nextCursor = null;
          loadingMore = false;
          await loadDirectory(path);
        }
        pageError = String(error);
        notify(String(error), "error");
      }
    } finally {
      if (revision === directoryRevision) loadingMore = false;
    }
  }

  function handleDirectoryScroll(event: Event) {
    const target = event.currentTarget as HTMLElement;
    if (target.scrollHeight - target.scrollTop - target.clientHeight < 280) {
      void loadMoreDirectory();
    }
  }

  function scheduleSearch() {
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => {
      searchTimer = undefined;
      void loadDirectory(currentPath);
    }, 250);
  }

  function rememberTreeDirectory(shareId: string, path: string, nextEntries: RemoteFileEntry[]) {
    const next = new Map(treeDirectories);
    next.set(remoteTreeDirectoryKey(shareId, path), nextEntries);
    treeDirectories = next;
  }

  function expandTreeAncestors(shareId: string, path: string) {
    expandedShareIds = new Set(expandedShareIds).add(shareId);
    const next = new Set(expandedFolderKeys);
    for (const ancestor of ancestorPaths(path)) next.add(remoteTreeDirectoryKey(shareId, ancestor));
    expandedFolderKeys = next;
  }

  async function ensureTreeDirectory(shareId: string, path: string, force = false) {
    const key = remoteTreeDirectoryKey(shareId, path);
    if ((!force && treeDirectories.has(key)) || treeLoadingKeys.has(key) || !selectedPeerId) return;
    treeLoadingKeys = new Set(treeLoadingKeys).add(key);
    try {
      const page = await bridge.listRemoteDirectory(
        selectedPeerId,
        shareId,
        path,
        null,
        "",
        "name",
        "ascending",
      );
      rememberTreeDirectory(shareId, path, page.entries);
    } catch (error) {
      pageError = String(error);
      notify(String(error), "error");
    } finally {
      const next = new Set(treeLoadingKeys);
      next.delete(key);
      treeLoadingKeys = next;
    }
  }

  async function toggleTreeShare(shareId: string, expand?: boolean) {
    const next = new Set(expandedShareIds);
    const shouldExpand = expand ?? !next.has(shareId);
    if (shouldExpand) next.add(shareId); else next.delete(shareId);
    expandedShareIds = next;
    if (shouldExpand) await ensureTreeDirectory(shareId, "");
  }

  async function toggleTreeFolder(row: Extract<RemoteFileTreeRow, { kind: "folder" }>, expand?: boolean) {
    const next = new Set(expandedFolderKeys);
    const shouldExpand = expand ?? !next.has(row.key);
    if (shouldExpand) next.add(row.key); else next.delete(row.key);
    expandedFolderKeys = next;
    if (shouldExpand) await ensureTreeDirectory(row.shareId, row.path);
  }

  async function navigateTreeRow(row: RemoteFileTreeRow) {
    if (selectedShareId !== row.shareId) {
      selectedShareId = row.shareId;
      currentPath = "";
      selectedPaths = new Set();
      historyPaths = [""];
      historyIndex = 0;
    }
    if (row.path === currentPath) return;
    search = "";
    if (!(await loadDirectory(row.path))) return;
    historyPaths = [...historyPaths.slice(0, historyIndex + 1), row.path];
    historyIndex = historyPaths.length - 1;
  }

  async function refreshTreeRow(row: RemoteFileTreeRow) {
    if (selectedShareId === row.shareId && currentPath === row.path) await loadDirectory(row.path);
    else await ensureTreeDirectory(row.shareId, row.path, true);
  }

  async function navigateTo(path: string) {
    if (path === currentPath || operation) return;
    search = "";
    if (!(await loadDirectory(path))) return;
    historyPaths = [...historyPaths.slice(0, historyIndex + 1), path];
    historyIndex = historyPaths.length - 1;
  }

  async function navigateHistory(nextIndex: number) {
    const path = historyPaths[nextIndex];
    if (path === undefined || operation) return;
    search = "";
    if (await loadDirectory(path)) historyIndex = nextIndex;
  }

  function parentPath() { return currentPath.split("/").slice(0, -1).join("/"); }

  async function runOperation(label: string, action: () => Promise<void>) {
    operation = label;
    pageError = "";
    try { await action(); }
    catch (error) { pageError = String(error); notify(String(error), "error"); }
    finally { operation = ""; }
  }

  function entryIcon(entry: RemoteFileEntry) {
    if (entry.kind === "folder") return FolderSimple;
    const extension = entry.name.split(".").pop()?.toLowerCase();
    if (extension === "pdf") return FilePdf;
    if (["doc", "docx"].includes(extension ?? "")) return FileDoc;
    if (["md", "txt", "rtf"].includes(extension ?? "")) return FileText;
    if (["png", "jpg", "jpeg", "gif", "webp", "heic", "psd", "sketch", "fig"].includes(extension ?? "")) return Image;
    if (["mp3", "wav", "aac", "flac"].includes(extension ?? "")) return MusicNote;
    if (["mp4", "mov", "mkv", "avi"].includes(extension ?? "")) return VideoCamera;
    return FileIcon;
  }

  function thumbnailKey(entry: RemoteFileEntry) {
    return `${selectedPeerId}\0${selectedShareId}\0${entry.relativePath}\0${entry.modifiedAtMs}`;
  }

  function isThumbnailCandidate(entry: RemoteFileEntry) {
    return entry.kind === "file" && /\.(?:png|jpe?g|gif|webp)$/i.test(entry.name);
  }

  async function queueThumbnails(nextEntries: RemoteFileEntry[]) {
    const peerId = selectedPeerId;
    const shareId = selectedShareId;
    await Promise.all(nextEntries.filter(isThumbnailCandidate).map(async (entry) => {
      const key = `${peerId}\0${shareId}\0${entry.relativePath}\0${entry.modifiedAtMs}`;
      if (thumbnailPaths.has(key) || thumbnailPending.has(key) || thumbnailUnavailable.has(key)) return;
      thumbnailPending.add(key);
      try {
        const path = await bridge.getRemoteFileThumbnail(peerId, shareId, entry.relativePath, entry.modifiedAtMs);
        if (scope.disposed || peerId !== selectedPeerId || shareId !== selectedShareId) return;
        if (path) {
          const next = new Map(thumbnailPaths); next.set(key, convertFileSrc(path));
          while (next.size > 256) next.delete(next.keys().next().value!);
          thumbnailPaths = next;
        } else {
          thumbnailUnavailable.add(key);
          while (thumbnailUnavailable.size > 256) thumbnailUnavailable.delete(thumbnailUnavailable.values().next().value!);
        }
      } catch { /* Failed requests remain retryable on refresh; Rust owns retries. */ }
      finally { thumbnailPending.delete(key); }
    }));
  }

  function thumbnailUrl(entry: RemoteFileEntry) {
    return thumbnailPaths.get(thumbnailKey(entry));
  }

  async function pickUpload(folder: boolean, targetPath = currentPath) {
    uploadMenuOpen = false;
    if (!selectedPeerId || !selectedShareId) return;
    if (!selectedShare?.writable) {
      notify(uiTranslate("该共享目录为只读。请先在远端电脑开启写入权限后再上传。", $uiLanguage), "error");
      return;
    }
    try {
      const session = await bridge.startRemoteUpload(selectedPeerId, selectedShareId, targetPath, folder);
      if (session) upsertTransfer(session);
    } catch (error) {
      notify(String(error), "error");
    }
  }

  async function uploadPaths(paths: string[], targetPath = currentPath) {
    if (!paths.length || !selectedPeerId || !selectedShareId) return;
    if (!selectedShare?.writable) {
      notify(uiTranslate("无法上传：该共享目录为只读，请先在远端电脑开启写入权限。", $uiLanguage), "error");
      return;
    }
    try {
      upsertTransfer(await bridge.startRemoteUploadPaths(selectedPeerId, selectedShareId, targetPath, paths));
    } catch (error) {
      notify(String(error), "error");
    }
  }

  function openFolderDialog(parentPath = currentPath) {
    folderDialogParentPath = parentPath;
    dialogValue = "";
    folderDialogOpen = true;
  }

  async function createFolder() {
    const name = dialogValue.trim();
    if (!name || !selectedPeerId || !selectedShareId) return;
    const parentPath = folderDialogParentPath;
    folderDialogOpen = false;
    await runOperation(tr("正在创建文件夹…"), async () => {
      const entry = await bridge.createRemoteDirectory(selectedPeerId, selectedShareId, parentPath, name);
      await loadDirectory(currentPath);
      if (parentPath !== currentPath) await ensureTreeDirectory(selectedShareId, parentPath, true);
      else selectedPaths = new Set([entry.relativePath]);
      notify(t("已创建文件夹“{name}”", language, { name: name }));
    });
  }

  function openRename() {
    if (selectedEntries.length !== 1 || !selected) return;
    dialogValue = selected.name;
    renameDialogOpen = true;
    moreMenuOpen = false;
  }

  async function renameSelected() {
    const name = dialogValue.trim();
    if (!name || !selected || !selectedPeerId || !selectedShareId) return;
    const relativePath = selected.relativePath;
    renameDialogOpen = false;
    await runOperation(tr("正在重命名…"), async () => {
      const entry = await bridge.renameRemoteEntry(selectedPeerId, selectedShareId, relativePath, name);
      await loadDirectory(currentPath);
      selectedPaths = new Set([entry.relativePath]);
      notify(tr("名称已更新"));
    });
  }

  async function deleteSelected() {
    if (!selectedEntries.length || !selectedPeerId || !selectedShareId) return;
    const deleting = [...selectedEntries];
    deleteDialogOpen = false;
    await runOperation(deleting.length > 1 ? `正在删除 ${deleting.length} 个项目…` : tr("正在删除…"), async () => {
      const results = await bridge.deleteRemoteEntries(selectedPeerId, selectedShareId, deleting.map((entry) => entry.relativePath));
      await loadDirectory(currentPath);
      const failures = results.filter((result) => result.error);
      selectedPaths = new Set(failures.map((result) => result.path));
      if (failures.length) notify(failures.map((result) => `${result.path}: ${result.error}`).join("\n"), "error");
      else notify(deleting.length > 1 ? `已删除 ${deleting.length} 个项目` : tr("已删除"));
    });
  }

  async function downloadSelected() {
    if (!selectedEntries.length || !selectedPeerId || !selectedShareId) return;
    const downloading = [...selectedEntries];
    try {
      const sessions = await bridge.startRemoteDownloadEntries(selectedPeerId, selectedShareId, downloading.map((entry) => entry.relativePath));
      sessions?.forEach(upsertTransfer);
    } catch (error) {
      notify(String(error), "error");
    }
  }

  async function openEntry(entry: RemoteFileEntry) {
    if (entry.kind === "folder") {
      await navigateTo(entry.relativePath);
      return;
    }
    if (!selectedPeerId || !selectedShareId || openingPaths.has(entry.relativePath)) return;
    openingPaths = new Set(openingPaths).add(entry.relativePath);
    try {
      const result = await bridge.openRemoteEntry(selectedPeerId, selectedShareId, entry.relativePath);
      notify(result.editable
        ? (uiTranslate("已打开；保存后的文本修改将自动同步。", $uiLanguage))
        : (uiTranslate("已使用系统默认应用打开。", $uiLanguage)));
    } catch (error) {
      notify(String(error), "error");
    } finally {
      const next = new Set(openingPaths);
      next.delete(entry.relativePath);
      openingPaths = next;
    }
  }

  function openDeleteConfirmation() {
    if (!selectedEntries.length || !selectedShare?.writable || operation) return;
    deleteDialogOpen = true;
  }

  function handleEntryContextMenu(event: MouseEvent, entry: RemoteFileEntry) {
    selectedPaths = new Set([entry.relativePath]);
    lastSelectedPath = entry.relativePath;
    if (!bridge.isTauri()) return;

    void showRemoteFileContextMenu(event, {
      entry,
      writable: Boolean(selectedShare?.writable),
      busy: Boolean(operation),
      language,
      callbacks: {
        open: () => openEntry(entry),
        stopEditing: async () => {
          await bridge.stopRemoteEdit(selectedPeerId, selectedShareId, entry.relativePath);
          notify(tr("已停止自动同步编辑，本地副本已保留"));
        },
        uploadFiles: () => pickUpload(false, entry.relativePath),
        uploadFolder: () => pickUpload(true, entry.relativePath),
        newFolder: () => openFolderDialog(entry.relativePath),
        download: downloadSelected,
        rename: openRename,
        delete: openDeleteConfirmation,
      },
    }).catch((error) => notify(String(error), "error"));
  }

  function handleBackgroundContextMenu(event: MouseEvent) {
    if (!bridge.isTauri()) return;
    void showRemoteFileBackgroundMenu(event, {
      writable: Boolean(selectedShare?.writable),
      busy: Boolean(operation),
      language,
      callbacks: {
        uploadFiles: () => pickUpload(false),
        uploadFolder: () => pickUpload(true),
        newFolder: () => openFolderDialog(),
        refresh: async () => { await loadDirectory(currentPath); },
      },
    }).catch((error) => notify(String(error), "error"));
  }

  function handleTreeContextMenu(event: MouseEvent, row: RemoteFileTreeRow) {
    if (!bridge.isTauri()) return;
    void showRemoteFileTreeMenu(event, {
      kind: row.kind,
      expanded: row.expanded,
      busy: treeLoadingKeys.has(remoteTreeDirectoryKey(row.shareId, row.path)),
      language,
      callbacks: {
        open: () => navigateTreeRow(row),
        expand: () => row.kind === "share" ? toggleTreeShare(row.shareId, true) : toggleTreeFolder(row, true),
        collapse: () => row.kind === "share" ? toggleTreeShare(row.shareId, false) : toggleTreeFolder(row, false),
        refresh: () => refreshTreeRow(row),
      },
    }).catch((error) => notify(String(error), "error"));
  }

  async function beginRemoteDrag(event: DragEvent, entry: RemoteFileEntry) {
    if (!selectedPaths.has(entry.relativePath)) selectedPaths = new Set([entry.relativePath]);
    if (!bridge.isTauri()) {
      event.dataTransfer?.setData("text/plain", entry.name);
      if (event.dataTransfer) event.dataTransfer.effectAllowed = "copy";
      return;
    }
    event.preventDefault();
    if (draggingOutPath || !selectedPeerId || !selectedShareId) return;
    draggingOutPath = entry.relativePath;
    try {
      await bridge.startRemoteFilePromiseDrag(
        selectedPeerId,
        selectedShareId,
        entry.relativePath,
        entry.kind,
        entry.size,
      );
    } catch (error) {
      notify(String(error), "error");
    } finally {
      draggingOutPath = "";
    }
  }

  async function refreshCurrentDirectory() {
    if (selectedPeerId && selectedShareId) await loadDirectory(currentPath);
    else await loadState();
  }

  function transferProgress(transfer: RemoteFileTransferSession) {
    if (transfer.status === "completed") return 100;
    if (!transfer.totalBytes) return 0;
    return Math.max(0, Math.min(100, transfer.bytesTransferred / transfer.totalBytes * 100));
  }

  function selectEntry(event: MouseEvent, entry: RemoteFileEntry) {
    if (event.shiftKey && lastSelectedPath) {
      const start = sortedEntries.findIndex((item) => item.relativePath === lastSelectedPath);
      const end = sortedEntries.findIndex((item) => item.relativePath === entry.relativePath);
      if (start >= 0 && end >= 0) {
        const [from, to] = start < end ? [start, end] : [end, start];
        selectedPaths = new Set(sortedEntries.slice(from, to + 1).map((item) => item.relativePath));
      }
    } else if (event.metaKey || event.ctrlKey) toggleEntry(entry);
    else selectedPaths = new Set([entry.relativePath]);
    lastSelectedPath = entry.relativePath;
  }

  function toggleEntry(entry: RemoteFileEntry) {
    const next = new Set(selectedPaths);
    if (next.has(entry.relativePath)) next.delete(entry.relativePath); else next.add(entry.relativePath);
    selectedPaths = next;
    lastSelectedPath = entry.relativePath;
  }

  function toggleAllVisible() {
    selectedPaths = allVisibleSelected ? new Set() : new Set(sortedEntries.map((entry) => entry.relativePath));
  }

  function changeSort(nextKey: SortKey) {
    if (sortKey === nextKey) sortDirection = sortDirection === "ascending" ? "descending" : "ascending";
    else {
      sortKey = nextKey;
      sortDirection = nextKey === "name" || nextKey === "type" ? "ascending" : "descending";
    }
    sortMenuOpen = false;
    void loadDirectory(currentPath);
  }

  function sortLabel() { return { name: "名称", modified: "修改日期", type: "类型", size: "大小" }[sortKey]; }

  function typeLabel(entry: RemoteFileEntry) {
    if (entry.kind === "folder") return tr("文件夹");
    const extension = entry.name.includes(".") ? entry.name.split(".").pop()?.toUpperCase() : "";
    return extension ? `${extension} ${tr("文件")}` : tr("文件");
  }

  function formatSize(entry: RemoteFileEntry) { return entry.kind === "folder" ? "—" : formatBytes(entry.size); }

  function formatBytes(size: number) {
    const units = ["B", "KB", "MB", "GB", "TB"];
    let value = size;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit += 1; }
    return `${unit === 0 ? value : value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`;
  }

  function formatModified(value: number) {
    if (!value) return "—";
    return new Intl.DateTimeFormat(localeFor(language), {
      year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hour12: false,
    }).format(new Date(value));
  }

  function breadcrumbPath(index: number) { return breadcrumbs.slice(0, index + 1).join("/"); }

  function handleKeydown(event: KeyboardEvent) {
    if (event.defaultPrevented || (event.target instanceof Element && event.target.closest('[role="dialog"], [role="menu"]'))) return;
    if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) return;
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "a") { event.preventDefault(); toggleAllVisible(); }
    else if ((event.key === "Delete" || event.key === "Backspace") && selectedEntries.length && selectedShare?.writable) { event.preventDefault(); deleteDialogOpen = true; }
    else if (event.key === "Escape") selectedPaths = new Set();
    else if (event.key === "Enter" && selectedEntries.length === 1 && selected) void openEntry(selected);
  }

  function handleEntryKeydown(event: KeyboardEvent, entry: RemoteFileEntry) {
    if (event.key === " " || event.key === "Enter") {
      event.preventDefault();
      if (event.key === "Enter") void openEntry(entry);
      else selectEntry(event as unknown as MouseEvent, entry);
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<main class="remote-files-page">
  <header class="remote-title-row">
    <div class="remote-title-cluster">
      <h1>{uiTranslate("远程文件", $uiLanguage)}</h1>
      <div class="device-selector-wrap">
        <button class:open={deviceMenuOpen} class="device-selector" disabled={devices.length === 0} on:click={() => (deviceMenuOpen = !deviceMenuOpen)}>
          <Monitor size={17} /><span>{uiTranslate(selectedDevice?.name ?? (loading ? "正在查找设备…" : "没有已连接桌面"), $uiLanguage)}</span>
          {#if selectedDevice}<i></i><small>{uiTranslate("在线", $uiLanguage)}</small>{/if}<CaretDown size={13} />
        </button>
        {#if deviceMenuOpen}
          <div class="device-menu">
            {#each devices as device (device.id)}
              <button on:click={() => chooseDevice(device.id)}><Monitor size={17} /><span><strong>{device.name}</strong><small>{uiTranslate("已配对桌面 · 在线", $uiLanguage)}</small></span>{#if selectedPeerId === device.id}<Check size={15} />{/if}</button>
            {/each}
          </div>
        {/if}
      </div>
    </div>
    <label class="remote-search"><MagnifyingGlass size={17} /><input bind:value={search} disabled={!selectedShareId} placeholder={uiTranslate((selectedShare ? `搜索 ${currentPath.split("/").at(-1) || selectedShare.name}` : "搜索当前目录"), $uiLanguage)} on:input={scheduleSearch} /></label>
    <button class="remote-icon-button" aria-label={uiTranslate("刷新当前文件夹", $uiLanguage)} title={uiTranslate("刷新当前文件夹", $uiLanguage)} disabled={Boolean(operation)} on:click={refreshCurrentDirectory}><ArrowClockwise size={18} /></button>
  </header>

  <div class="remote-command-row">
    <div class="navigation-buttons" aria-label={uiTranslate("目录导航", $uiLanguage)}>
      <button aria-label={uiTranslate("后退", $uiLanguage)} title={uiTranslate("后退", $uiLanguage)} disabled={historyIndex <= 0 || Boolean(operation)} on:click={() => navigateHistory(historyIndex - 1)}><ArrowLeft size={18} /></button>
      <button aria-label={uiTranslate("前进", $uiLanguage)} title={uiTranslate("前进", $uiLanguage)} disabled={historyIndex >= historyPaths.length - 1 || Boolean(operation)} on:click={() => navigateHistory(historyIndex + 1)}><ArrowRight size={18} /></button>
      <button aria-label={uiTranslate("上一级", $uiLanguage)} title={uiTranslate("上一级", $uiLanguage)} disabled={!currentPath || Boolean(operation)} on:click={() => navigateTo(parentPath())}><ArrowUp size={18} /></button>
    </div>
    <nav class="remote-breadcrumb" aria-label={uiTranslate("当前位置", $uiLanguage)}>
      {#if selectedShare}
        <button class:current={!currentPath} on:click={() => navigateTo("")}>{selectedShare.name}</button>
        {#each breadcrumbs as crumb, index}
          <CaretRight size={13} /><button class:current={index === breadcrumbs.length - 1} on:click={() => navigateTo(breadcrumbPath(index))}>{crumb}</button>
        {/each}
      {:else}<span>{uiTranslate(operation || (loading ? "正在查找设备…" : "请选择一台已连接桌面"), $uiLanguage)}</span>{/if}
    </nav>

    <div class="command-actions">
      <SystemFolders peerId={selectedPeerId} shareId={selectedShareId} {devices} {shares} {language} {notify} />
      <div class="upload-action-wrap">
        <button class="command-button primary-command" disabled={!selectedShare?.writable || Boolean(operation)} on:click={() => pickUpload(false)}><UploadSimple size={17} /><span>{uiTranslate("上传", $uiLanguage)}</span></button>
        <button class="upload-caret" aria-label={uiTranslate("上传选项", $uiLanguage)} disabled={!selectedShare?.writable || Boolean(operation)} on:click={() => (uploadMenuOpen = !uploadMenuOpen)}><CaretDown size={12} /></button>
        {#if uploadMenuOpen}<div class="compact-menu upload-menu"><button on:click={() => pickUpload(false)}><FileIcon size={16} />{uiTranslate("上传文件", $uiLanguage)}</button><button on:click={() => pickUpload(true)}><FolderOpen size={16} />{uiTranslate("上传文件夹", $uiLanguage)}</button></div>{/if}
      </div>
      <button class="command-button" disabled={!selectedShare?.writable || Boolean(operation)} on:click={() => openFolderDialog()}><FolderSimplePlus size={17} /><span>{uiTranslate("新建文件夹", $uiLanguage)}</span></button>
      {#if selectedEntries.length}
        <span class="command-divider"></span>
        <button class="command-button selection-command" disabled={Boolean(operation)} on:click={downloadSelected}><DownloadSimple size={17} /><span>{uiTranslate("下载", $uiLanguage)}</span></button>
        <button class="command-button selection-command" disabled={selectedEntries.length !== 1 || !selectedShare?.writable || Boolean(operation)} on:click={openRename}><PencilSimple size={17} /><span>{uiTranslate("重命名", $uiLanguage)}</span></button>
      {/if}
      <div class="more-menu-wrap">
        <button class="command-icon-button" aria-label={uiTranslate("更多操作", $uiLanguage)} on:click={() => (moreMenuOpen = !moreMenuOpen)}><DotsThree size={20} weight="bold" /></button>
        {#if moreMenuOpen}
          <div class="compact-menu more-menu">
            <button disabled={!selectedEntries.length} on:click={downloadSelected}><DownloadSimple size={16} />{uiTranslate("下载所选项目", $uiLanguage)}</button>
            <button disabled={selectedEntries.length !== 1 || !selectedShare?.writable} on:click={openRename}><PencilSimple size={16} />{uiTranslate("重命名", $uiLanguage)}</button>
            <button class="danger" disabled={!selectedEntries.length || !selectedShare?.writable} on:click={() => { moreMenuOpen = false; deleteDialogOpen = true; }}><Trash size={16} />{uiTranslate("删除", $uiLanguage)}</button>
            <span></span><button on:click={() => { moreMenuOpen = false; void loadDirectory(currentPath); }}><ArrowClockwise size={16} />{uiTranslate("刷新", $uiLanguage)}</button>
          </div>
        {/if}
      </div>
    </div>

    <div class="display-actions">
      <div class="sort-wrap">
        <button class:open={sortMenuOpen} class="sort-button" on:click={() => (sortMenuOpen = !sortMenuOpen)}>
          {#if sortDirection === "ascending"}<SortAscending size={17} />{:else}<SortDescending size={17} />{/if}<span>{uiTranslate("排序：", $uiLanguage)}{uiTranslate(sortLabel(), $uiLanguage)}</span><CaretDown size={12} />
        </button>
        {#if sortMenuOpen}
          <div class="compact-menu sort-menu">
            {#each [["name", "名称"], ["modified", "修改日期"], ["type", "类型"], ["size", "大小"]] as item}
              <button class:active={sortKey === item[0]} on:click={() => changeSort(item[0] as SortKey)}><span>{item[1]}</span>{#if sortKey === item[0]}<Check size={15} />{/if}</button>
            {/each}
            <span></span><button on:click={() => { sortDirection = "ascending"; sortMenuOpen = false; void loadDirectory(currentPath); }}><SortAscending size={16} />{uiTranslate("升序", $uiLanguage)}</button><button on:click={() => { sortDirection = "descending"; sortMenuOpen = false; void loadDirectory(currentPath); }}><SortDescending size={16} />{uiTranslate("降序", $uiLanguage)}</button>
          </div>
        {/if}
      </div>
      <span class="display-label">{uiTranslate("查看", $uiLanguage)}</span>
      <div class="view-switcher" aria-label={uiTranslate("文件视图", $uiLanguage)}>
        <button class:active={viewMode === "details"} aria-label={uiTranslate("详细信息", $uiLanguage)} title={uiTranslate("详细信息", $uiLanguage)} on:click={() => (viewMode = "details")}><ListBullets size={18} /></button>
        <button class:active={viewMode === "compact"} aria-label={uiTranslate("紧凑列表", $uiLanguage)} title={uiTranslate("紧凑列表", $uiLanguage)} on:click={() => (viewMode = "compact")}><Rows size={18} /></button>
        <button class:active={viewMode === "grid"} aria-label={uiTranslate("大图标", $uiLanguage)} title={uiTranslate("大图标", $uiLanguage)} on:click={() => (viewMode = "grid")}><GridFour size={18} /></button>
      </div>
    </div>
  </div>

  {#if pageError}<div class="remote-error" role="alert">{pageError}</div>{/if}

  <div class="remote-files-body">
    <aside class="remote-source-panel">
      <div class="source-device-heading"><Monitor size={17} /><span>{uiTranslate(selectedDevice?.name ?? "远程设备", $uiLanguage)}</span><i class:online={Boolean(selectedDevice)}></i></div>
      <div class="share-tree" role="tree" aria-label={uiTranslate("远程共享位置", $uiLanguage)}>
        {#each treeRows as row (row.key)}
          <div
            class:active={row.active}
            class:ancestor={row.kind === "folder" && row.ancestor}
            class="share-tree-row"
            role="treeitem"
            aria-expanded={row.expanded}
            aria-selected={row.active}
            tabindex="-1"
            style={`--tree-depth:${row.depth}`}
            on:contextmenu={(event) => handleTreeContextMenu(event, row)}
          >
            <button
              class:expanded={row.expanded}
              class:hidden={row.kind === "folder" && row.childrenKnown && !row.hasFolderChildren}
              class="tree-disclosure"
              aria-label={uiTranslate((row.expanded ? `收缩 ${row.name}` : `展开 ${row.name}`), $uiLanguage)}
              disabled={treeLoadingKeys.has(remoteTreeDirectoryKey(row.shareId, row.path))}
              on:click|stopPropagation={() => row.kind === "share" ? toggleTreeShare(row.shareId) : toggleTreeFolder(row)}
            ><CaretRight size={12} /></button>
            <button class="tree-label" on:click={() => navigateTreeRow(row)}>
              {#if row.kind === "share"}<FolderOpen size={17} weight={row.active ? "fill" : "regular"} />
              {:else}<FolderSimple size={16} weight={row.active ? "fill" : "regular"} />{/if}
              <span>{row.name}</span>
            </button>
          </div>
        {/each}
      </div>
      <div class="source-panel-footer"><span>{uiTranslate("远端共享", $uiLanguage)}</span><strong>{shares.length} {uiTranslate("个位置", $uiLanguage)}</strong></div>
    </aside>

    <section class:dragging={draggingIn} class="remote-file-browser" aria-label={uiTranslate("远程文件浏览器", $uiLanguage)}>
      {#if viewMode === "details"}
        <div class="remote-table" role="table" aria-label={uiTranslate("远程文件列表", $uiLanguage)}>
          <div class="remote-table-head" role="row">
            <button class="check-column" aria-label={uiTranslate("选择全部", $uiLanguage)} on:click={toggleAllVisible}><span class:checked={allVisibleSelected}>{#if allVisibleSelected}<Check size={11} weight="bold" />{/if}</span></button>
            <button class:active={sortKey === "name"} on:click={() => changeSort("name")}><span>{uiTranslate("名称", $uiLanguage)}</span>{#if sortKey === "name"}{#if sortDirection === "ascending"}<SortAscending size={14} />{:else}<SortDescending size={14} />{/if}{/if}</button>
            <button class:active={sortKey === "modified"} on:click={() => changeSort("modified")}><span>{uiTranslate("修改日期", $uiLanguage)}</span>{#if sortKey === "modified"}{#if sortDirection === "ascending"}<SortAscending size={14} />{:else}<SortDescending size={14} />{/if}{/if}</button>
            <button class:active={sortKey === "type"} on:click={() => changeSort("type")}><span>{uiTranslate("类型", $uiLanguage)}</span>{#if sortKey === "type"}{#if sortDirection === "ascending"}<SortAscending size={14} />{:else}<SortDescending size={14} />{/if}{/if}</button>
            <button class:active={sortKey === "size"} on:click={() => changeSort("size")}><span>{uiTranslate("大小", $uiLanguage)}</span>{#if sortKey === "size"}{#if sortDirection === "ascending"}<SortAscending size={14} />{:else}<SortDescending size={14} />{/if}{/if}</button>
          </div>
          <div class="remote-table-body" role="rowgroup" on:scroll={handleDirectoryScroll} on:contextmenu={handleBackgroundContextMenu}>
            {#if operation}<div class="remote-empty"><span class="remote-spinner"></span><strong>{operation}</strong></div>
            {:else if !selectedPeerId}<div class="remote-empty"><Monitor size={29} /><strong>{uiTranslate("没有可访问的桌面设备", $uiLanguage)}</strong><span>{uiTranslate("请先在“设置 → 设备与连接”中连接另一台桌面。", $uiLanguage)}</span><button on:click={() => loadState()}>{uiTranslate("重新查找", $uiLanguage)}</button></div>
            {:else if !selectedShareId}<div class="remote-empty"><FolderOpen size={30} /><strong>{uiTranslate("对方没有可用共享目录", $uiLanguage)}</strong><span>{uiTranslate("请在对方电脑的“设置 → 文件共享”中添加目录。", $uiLanguage)}</span></div>
            {:else if sortedEntries.length === 0}<div class="remote-empty"><MagnifyingGlass size={28} /><strong>{uiTranslate(search ? "没有找到文件" : "这个文件夹是空的", $uiLanguage)}</strong><span>{uiTranslate(search ? "换一个关键词试试。" : "可通过工具栏上传或新建文件夹。", $uiLanguage)}</span></div>
            {:else}
              {#each sortedEntries as entry (entry.relativePath)}
                {@const Icon = entryIcon(entry)}
                <div data-remote-drop-path={entry.kind === "folder" ? entry.relativePath : undefined} class:selected={selectedPaths.has(entry.relativePath)} class:drop-target={draggingIn && dragTargetPath === entry.relativePath} class:preparing-drag={draggingOutPath === entry.relativePath} class="remote-file-row" role="row" tabindex="0" draggable="true" on:keydown={(event) => handleEntryKeydown(event, entry)} on:dragstart={(event) => beginRemoteDrag(event, entry)} on:click={(event) => selectEntry(event, entry)} on:dblclick={() => openEntry(entry)} on:contextmenu={(event) => handleEntryContextMenu(event, entry)}>
                  <button class="check-column" aria-label={uiTranslate((`选择 ${entry.name}`), $uiLanguage)} on:click|stopPropagation={() => toggleEntry(entry)}><span class:checked={selectedPaths.has(entry.relativePath)}>{#if selectedPaths.has(entry.relativePath)}<Check size={11} weight="bold" />{/if}</span></button>
                  <span class="file-name-cell"><span class:has-thumbnail={Boolean(thumbnailUrl(entry))} class="entry-icon">{#if thumbnailUrl(entry)}<img src={thumbnailUrl(entry)} alt="" />{:else}<Icon size={20} weight={entry.kind === "folder" ? "fill" : "duotone"} />{/if}</span><strong>{entry.name}</strong>{#if draggingOutPath === entry.relativePath}<small>{uiTranslate("准备下载…", $uiLanguage)}</small>{/if}</span>
                  <span>{formatModified(entry.modifiedAtMs)}</span><span>{uiTranslate(typeLabel(entry), $uiLanguage)}</span><span>{formatSize(entry)}</span>
                </div>
              {/each}
              {#if loadingMore}<div class="remote-load-more" role="status"><span class="remote-spinner"></span>{uiTranslate("正在加载更多文件…", $uiLanguage)}</div>{/if}
            {/if}
          </div>
        </div>
      {:else}
        <div class:compact={viewMode === "compact"} class="remote-grid-body" role="listbox" aria-label={uiTranslate("远程文件", $uiLanguage)} tabindex="0" on:scroll={handleDirectoryScroll} on:contextmenu={handleBackgroundContextMenu}>
          {#if operation}<div class="remote-empty"><span class="remote-spinner"></span><strong>{operation}</strong></div>
          {:else if sortedEntries.length === 0}<div class="remote-empty"><MagnifyingGlass size={28} /><strong>{uiTranslate(search ? "没有找到文件" : "这个文件夹是空的", $uiLanguage)}</strong></div>
          {:else}
            {#each sortedEntries as entry (entry.relativePath)}
              {@const Icon = entryIcon(entry)}
              <div data-remote-drop-path={entry.kind === "folder" ? entry.relativePath : undefined} class:selected={selectedPaths.has(entry.relativePath)} class:drop-target={draggingIn && dragTargetPath === entry.relativePath} class="remote-grid-item" role="option" aria-selected={selectedPaths.has(entry.relativePath)} tabindex="0" draggable="true" on:keydown={(event) => handleEntryKeydown(event, entry)} on:dragstart={(event) => beginRemoteDrag(event, entry)} on:click={(event) => selectEntry(event, entry)} on:dblclick={() => openEntry(entry)} on:contextmenu={(event) => handleEntryContextMenu(event, entry)}>
                <button class="grid-checkbox" aria-label={uiTranslate((`选择 ${entry.name}`), $uiLanguage)} on:click|stopPropagation={() => toggleEntry(entry)}><span class:checked={selectedPaths.has(entry.relativePath)}>{#if selectedPaths.has(entry.relativePath)}<Check size={11} weight="bold" />{/if}</span></button>
                <span class:has-thumbnail={Boolean(thumbnailUrl(entry))} class="grid-entry-icon">{#if thumbnailUrl(entry)}<img src={thumbnailUrl(entry)} alt="" />{:else}<Icon size={viewMode === "grid" ? 48 : 23} weight={entry.kind === "folder" ? "fill" : "duotone"} />{/if}</span>
                <span class="grid-entry-copy"><strong>{entry.name}</strong><small>{formatModified(entry.modifiedAtMs)}{entry.kind === "file" ? ` · ${formatSize(entry)}` : ""}</small></span>
              </div>
            {/each}
            {#if loadingMore}<div class="remote-load-more grid-load-more" role="status"><span class="remote-spinner"></span>{uiTranslate("正在加载更多文件…", $uiLanguage)}</div>{/if}
          {/if}
        </div>
      {/if}
      {#if draggingIn}<div class:readonly={!selectedShare?.writable} class="drag-feedback"><UploadSimple size={16} />{uiTranslate(selectedShare?.writable ? `上传到“${(dragTargetPath || currentPath).split("/").at(-1) || selectedShare?.name}”` : "该共享目录为只读，无法上传", $uiLanguage)}</div>{/if}
    </section>
  </div>

  {#if transfers.length}
    <aside class:collapsed={!transferPanelOpen} class="remote-transfer-panel" aria-label={uiTranslate("文件传输", $uiLanguage)}>
      <button class="transfer-panel-heading" on:click={() => (transferPanelOpen = !transferPanelOpen)}>
        <span>{uiTranslate(activeTransfers.length ? `正在传输 ${activeTransfers.length} 项` : "最近传输", $uiLanguage)}</span>
        <small>{uiTranslate(transferPanelOpen ? "收起" : "展开", $uiLanguage)}</small><CaretDown size={13} />
      </button>
      {#if transferPanelOpen}
        <div class="remote-transfer-list">
          {#each visibleTransfers as transfer (transfer.id)}
            <div class:failed={transfer.status === "failed"} class:completed={transfer.status === "completed"} class="remote-transfer-item">
              <span class="transfer-direction">{#if transfer.direction === "upload"}<UploadSimple size={16} />{:else}<DownloadSimple size={16} />{/if}</span>
              <span class="transfer-copy"><strong>{transfer.name}</strong><small>{uiTranslate(transfer.status === "failed" ? transfer.error : transfer.status === "completed" ? "已完成" : `${formatBytes(transfer.bytesTransferred)} / ${transfer.totalBytes ? formatBytes(transfer.totalBytes) : "正在准备…"}`, $uiLanguage)}</small><i><b class:indeterminate={!transfer.totalBytes && transfer.status === "transferring"} style={`width:${transferProgress(transfer)}%`}></b></i></span>
              <em>{uiTranslate(transfer.status === "failed" ? "失败" : transfer.status === "completed" ? "完成" : `${Math.round(transferProgress(transfer))}%`, $uiLanguage)}</em>
            </div>
          {/each}
        </div>
      {/if}
    </aside>
  {/if}

  <footer class="remote-status-bar"><span>{uiTranslate(nextCursor ? `已加载 ${sortedEntries.length} 个项目` : `${sortedEntries.length} 个项目`, $uiLanguage)}</span>{#if selectedEntries.length}<i></i><span>{uiTranslate("已选择", $uiLanguage)} {selectedEntries.length} {uiTranslate("个", $uiLanguage)}{selectionSize ? ` · ${formatBytes(selectionSize)}` : ""}</span>{/if}<span class="status-spacer"></span>{#if activeTransfers.length}<span>{activeTransfers.length} {uiTranslate("个传输进行中", $uiLanguage)}</span><i></i>{/if}<span>{uiTranslate(viewMode === "details" ? "详细信息" : viewMode === "compact" ? "紧凑列表" : "大图标", $uiLanguage)}</span></footer>
</main>

{#if folderDialogOpen || (renameDialogOpen && selected)}
  <div class="remote-dialog-backdrop" role="presentation" on:click={() => { folderDialogOpen = false; renameDialogOpen = false; }}>
    <div class="remote-dialog" role="dialog" aria-modal="true" aria-label={uiTranslate((folderDialogOpen ? "新建文件夹" : "重命名"), $uiLanguage)} tabindex="-1" on:click|stopPropagation on:keydown|stopPropagation>
      <h2>{uiTranslate(folderDialogOpen ? "新建文件夹" : "重命名", $uiLanguage)}</h2><p>{uiTranslate(folderDialogOpen ? tr("在当前目录中创建一个新文件夹。") : (t("为“{name}”输入新名称。", language, { name: selected?.name ?? "" })), $uiLanguage)}</p>
      <input bind:value={dialogValue} aria-label={uiTranslate((folderDialogOpen ? "文件夹名称" : "新名称"), $uiLanguage)} on:keydown={(event) => event.key === "Enter" && (folderDialogOpen ? createFolder() : renameSelected())} />
      <footer><button on:click={() => { folderDialogOpen = false; renameDialogOpen = false; }}>{uiTranslate("取消", $uiLanguage)}</button><button class="confirm" on:click={folderDialogOpen ? createFolder : renameSelected}>{uiTranslate(folderDialogOpen ? "创建" : "保存", $uiLanguage)}</button></footer>
    </div>
  </div>
{/if}

{#if deleteDialogOpen}
  <div class="remote-dialog-backdrop" role="presentation" on:click={() => (deleteDialogOpen = false)}>
    <div class="remote-dialog" role="alertdialog" aria-modal="true" aria-label={uiTranslate("删除项目", $uiLanguage)} tabindex="-1" on:click|stopPropagation on:keydown|stopPropagation>
      <h2>{uiTranslate(selectedEntries.length > 1 ? `删除 ${selectedEntries.length} 个项目？` : (t("删除“{name}”？", language, { name: selected?.name })), $uiLanguage)}</h2><p>{uiTranslate("所选项目将从远程电脑永久删除，此操作无法撤销。", $uiLanguage)}</p>
      <footer><button on:click={() => (deleteDialogOpen = false)}>{uiTranslate("取消", $uiLanguage)}</button><button class="delete-confirm" on:click={deleteSelected}>{uiTranslate("删除", $uiLanguage)}</button></footer>
    </div>
  </div>
{/if}

<style>
  .remote-files-page { position: relative; display: grid; grid-template-rows: 76px 54px minmax(0, 1fr) 30px; grid-template-areas: "title" "commands" "files" "status"; width: 100%; height: 100%; min-width: 0; min-height: 0; color: var(--text); background: var(--surface); }
  button, input { font: inherit; } button { color: inherit; } button:disabled { opacity: .42; cursor: not-allowed !important; }
  .remote-title-row { grid-area: title; display: grid; grid-template-columns: minmax(0, 1fr) minmax(230px, 340px) 38px; align-items: center; gap: 12px; padding: 0 22px 0 28px; border-bottom: 1px solid var(--border); }
  .remote-title-cluster { display: flex; min-width: 0; align-items: center; gap: 22px; }
  .remote-title-row h1 { flex: 0 0 auto; margin: 0; font-size: 24px; line-height: 1; letter-spacing: -.035em; }
  .device-selector-wrap, .more-menu-wrap, .upload-action-wrap, .sort-wrap { position: relative; }
  .upload-action-wrap { display: flex; height: 34px; align-items: center; flex: 0 0 auto; }
  .device-selector { display: flex; width: min(100%, 310px); height: 38px; align-items: center; gap: 8px; padding: 0 11px; border: 1px solid var(--border-strong); border-radius: 9px; background: var(--control-bg); cursor: pointer; }
  .device-selector.open { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus-ring); }
  .device-selector span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .device-selector i { width: 7px; height: 7px; margin-left: auto; border-radius: 50%; background: var(--success); }
  .device-selector small { color: var(--success); font-size: 11px; }
  .device-menu, .compact-menu { position: absolute; z-index: 40; top: calc(100% + 7px); min-width: 100%; padding: 6px; border: 1px solid var(--border-strong); border-radius: 10px; background: var(--surface-raised); box-shadow: var(--shadow-floating); }
  .device-menu button { display: flex; width: 100%; align-items: center; gap: 9px; padding: 8px; border: 0; border-radius: 7px; background: transparent; text-align: left; cursor: pointer; }
  .device-menu button:hover { background: var(--surface-hover); }
  .device-menu button span { display: flex; min-width: 190px; flex-direction: column; gap: 3px; }
  .device-menu button small { color: var(--text-muted); font-size: 11px; }
  .remote-search { display: flex; height: 38px; align-items: center; gap: 8px; padding: 0 12px; border: 1px solid var(--border-strong); border-radius: 9px; color: var(--text-muted); background: var(--control-bg); }
  .remote-search:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus-ring); }
  .remote-search input { width: 100%; min-width: 0; border: 0; outline: 0; color: var(--text); background: transparent; }
  .remote-icon-button, .navigation-buttons button, .command-icon-button { display: grid; width: 36px; height: 36px; place-items: center; border: 1px solid var(--border-strong); border-radius: 8px; background: var(--control-bg); cursor: pointer; }
  .remote-icon-button:hover, .navigation-buttons button:hover:not(:disabled), .command-icon-button:hover { background: var(--control-hover); }
  .remote-command-row { grid-area: commands; display: grid; grid-template-columns: auto minmax(170px, 1fr) auto auto; align-items: center; gap: 12px; padding: 0 22px; border-bottom: 1px solid var(--border); }
  .navigation-buttons { display: flex; gap: 5px; } .navigation-buttons button { width: 34px; height: 34px; border-color: transparent; background: transparent; }
  .remote-breadcrumb { display: flex; min-width: 0; align-items: center; gap: 4px; overflow: hidden; color: var(--text-muted); font-size: 13px; }
  .remote-breadcrumb button { flex: 0 1 auto; max-width: 150px; overflow: hidden; padding: 5px 6px; border: 0; border-radius: 6px; color: var(--text-muted); background: transparent; text-overflow: ellipsis; white-space: nowrap; cursor: pointer; }
  .remote-breadcrumb button:hover { color: var(--accent-strong); background: var(--surface-hover); } .remote-breadcrumb button.current { color: var(--text); font-weight: 620; }
  .command-actions, .display-actions { display: flex; min-width: 0; align-items: center; gap: 5px; }
  .command-button, .sort-button { display: inline-flex; height: 34px; align-items: center; gap: 7px; padding: 0 10px; border: 1px solid transparent; border-radius: 8px; background: transparent; font-size: 12px; line-height: 1; white-space: nowrap; cursor: pointer; }
  .command-button :global(svg), .sort-button :global(svg), .upload-caret :global(svg), .command-icon-button :global(svg) { display: block; flex: 0 0 auto; }
  .command-button:hover:not(:disabled), .sort-button:hover, .sort-button.open { background: var(--surface-hover); }
  .primary-command { padding-right: 8px; border-radius: 8px 0 0 8px; color: var(--accent-strong); }
  .upload-caret { display: grid; width: 24px; height: 34px; margin-left: -5px; place-items: center; border: 0; border-left: 1px solid var(--border); border-radius: 0 8px 8px 0; color: var(--accent-strong); background: transparent; cursor: pointer; }
  .upload-caret:hover:not(:disabled) { background: var(--surface-hover); } .upload-menu { left: 0; min-width: 155px; }
  .command-divider { width: 1px; height: 22px; margin: 0 3px; background: var(--border); } .selection-command { color: var(--accent-strong); }
  .command-icon-button { width: 34px; height: 34px; border-color: transparent; background: transparent; } .more-menu { right: 0; min-width: 176px; }
  .compact-menu button { display: flex; width: 100%; align-items: center; gap: 9px; padding: 8px 9px; border: 0; border-radius: 7px; background: transparent; font-size: 12px; text-align: left; cursor: pointer; }
  .compact-menu button:hover:not(:disabled), .compact-menu button.active { background: var(--surface-hover); } .compact-menu button.danger { color: var(--danger); }
  .compact-menu > span { display: block; height: 1px; margin: 5px 3px; background: var(--border); }
  .display-actions { padding-left: 8px; border-left: 1px solid var(--border); } .sort-button { white-space: nowrap; } .sort-menu { right: 0; min-width: 158px; }
  .sort-menu button { justify-content: flex-start; } .sort-menu button > span { margin-right: auto; } .display-label { margin-left: 4px; color: var(--text-muted); font-size: 12px; }
  .view-switcher { display: flex; padding: 2px; border: 1px solid var(--border-strong); border-radius: 8px; background: var(--control-bg); }
  .view-switcher button { display: grid; width: 29px; height: 27px; place-items: center; border: 0; border-radius: 6px; color: var(--text-muted); background: transparent; cursor: pointer; }
  .view-switcher button:hover { color: var(--text); background: var(--surface-hover); } .view-switcher button.active { color: var(--accent-strong); background: var(--accent-soft); }
  .remote-error { position: absolute; z-index: 20; top: 126px; right: 20px; left: 20px; overflow: hidden; padding: 7px 10px; border: 1px solid color-mix(in srgb, var(--danger) 30%, var(--border)); border-radius: 8px; color: var(--danger); background: var(--danger-soft); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
  .remote-files-body { grid-area: files; display: grid; grid-template-columns: 220px minmax(0, 1fr); min-width: 0; min-height: 0; overflow: hidden; }
  .remote-source-panel { display: grid; grid-template-rows: 46px minmax(0, 1fr) 38px; min-height: 0; overflow: hidden; border-right: 1px solid var(--border); background: var(--surface-soft); }
  .source-device-heading { display: flex; min-width: 0; align-items: center; gap: 8px; padding: 0 15px; border-bottom: 1px solid var(--border); font-size: 12px; font-weight: 620; }
  .source-device-heading span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } .source-device-heading i { width: 7px; height: 7px; margin-left: auto; border-radius: 50%; background: var(--text-muted); } .source-device-heading i.online { background: var(--success); }
  .remote-source-panel .share-tree { min-height: 0; padding: 9px 8px; overflow: auto; }
  .share-tree-row { display: grid; width: 100%; height: 34px; grid-template-columns: 18px minmax(0, 1fr); align-items: center; padding-left: calc(3px + var(--tree-depth) * 14px); border-radius: 7px; box-sizing: border-box; color: var(--text-secondary); }
  .share-tree-row:hover { background: var(--surface-hover); }
  .share-tree-row.active { color: var(--accent-strong); background: var(--accent-soft); font-weight: 620; }
  .share-tree-row.ancestor { color: var(--accent-strong); }
  .tree-disclosure { display: grid; width: 18px; height: 28px; place-items: center; padding: 0; border: 0; border-radius: 5px; color: currentColor; background: transparent; cursor: pointer; }
  .tree-disclosure:hover:not(:disabled) { background: color-mix(in srgb, var(--surface-hover) 65%, var(--border)); }
  .tree-disclosure :global(svg) { display: block; transition: transform 120ms ease; }
  .tree-disclosure.expanded :global(svg) { transform: rotate(90deg); }
  .tree-disclosure.hidden { visibility: hidden; pointer-events: none; }
  .tree-label { display: flex; width: 100%; min-width: 0; height: 32px; align-items: center; gap: 8px; padding: 0 7px 0 2px; border: 0; color: inherit; background: transparent; font-size: 12px; text-align: left; cursor: pointer; }
  .tree-label :global(svg) { display: block; flex: 0 0 auto; }
  .tree-label span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .source-panel-footer { display: flex; align-items: center; justify-content: space-between; padding: 0 14px; border-top: 1px solid var(--border); color: var(--text-muted); font-size: 11px; }
  .source-panel-footer strong { color: var(--text-secondary); font-weight: 580; }
  .remote-file-browser { position: relative; min-width: 0; min-height: 0; overflow: hidden; background: var(--surface); } .remote-file-browser.dragging { box-shadow: inset 0 0 0 2px var(--accent); }
  .remote-table { display: grid; height: 100%; grid-template-rows: 42px minmax(0, 1fr); }
  .remote-table-head, .remote-file-row { display: grid; grid-template-columns: 42px minmax(220px, 1.75fr) minmax(145px, .95fr) minmax(105px, .75fr) 86px; align-items: center; }
  .remote-table-head { border-bottom: 1px solid var(--border); color: var(--text-muted); font-size: 11px; }
  .remote-table-head > button:not(.check-column) { display: flex; height: 100%; align-items: center; gap: 5px; padding: 0 8px; border: 0; color: var(--text-muted); background: transparent; text-align: left; cursor: pointer; }
  .remote-table-head > button:hover, .remote-table-head > button.active { color: var(--text); background: var(--surface-hover); }
  .remote-table-body { min-height: 0; overflow: auto; }
  .remote-file-row { width: 100%; min-height: 43px; border-bottom: 1px solid var(--border); color: var(--text-secondary); background: transparent; font-size: 12px; cursor: default; user-select: none; }
  .remote-file-row:hover { background: var(--surface-hover); } .remote-file-row.selected { background: var(--surface-selected); } .remote-file-row.selected:hover { background: color-mix(in srgb, var(--surface-selected) 78%, var(--surface-hover)); } .remote-file-row.preparing-drag { opacity: .68; } .remote-file-row.drop-target, .remote-grid-item.drop-target { outline: 2px solid var(--accent); outline-offset: -2px; background: var(--accent-soft); }
  .remote-file-row > span:not(.file-name-cell) { padding: 0 8px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .check-column { display: grid; width: 100%; height: 100%; place-items: center; padding: 0; border: 0; background: transparent; cursor: pointer; }
  .check-column > span, .grid-checkbox > span { display: grid; width: 15px; height: 15px; place-items: center; border: 1px solid var(--border-strong); border-radius: 4px; color: white; background: var(--control-bg); }
  .check-column > span.checked, .grid-checkbox > span.checked { border-color: var(--accent); background: var(--accent); }
  .file-name-cell { display: flex; min-width: 0; align-items: center; gap: 9px; padding: 0 8px; color: var(--text); }
  .entry-icon { display: grid; width: 22px; height: 24px; flex: 0 0 auto; place-items: center; color: #f3b51b; } .entry-icon.has-thumbnail { overflow: hidden; border-radius: 4px; background: var(--surface-soft); } .entry-icon img { width: 100%; height: 100%; object-fit: cover; } .file-name-cell strong { overflow: hidden; font-size: 12px; font-weight: 560; text-overflow: ellipsis; white-space: nowrap; } .file-name-cell small { margin-left: auto; padding-right: 8px; color: var(--accent-strong); font-size: 10px; }
  .remote-grid-body { display: grid; min-height: 0; height: 100%; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); align-content: start; gap: 8px; padding: 18px; overflow: auto; }
  .remote-grid-body.compact { grid-template-columns: repeat(auto-fill, minmax(270px, 1fr)); gap: 3px 10px; padding: 12px; }
  .remote-grid-item { position: relative; display: grid; min-width: 0; min-height: 116px; grid-template-rows: 56px auto; align-items: center; justify-items: center; gap: 8px; padding: 12px 10px; border: 1px solid transparent; border-radius: 10px; text-align: center; user-select: none; cursor: default; }
  .remote-grid-item:hover { background: var(--surface-hover); } .remote-grid-item.selected { border-color: color-mix(in srgb, var(--accent) 28%, var(--border)); background: var(--surface-selected); }
  .remote-grid-body.compact .remote-grid-item { min-height: 48px; grid-template-columns: 32px minmax(0, 1fr); grid-template-rows: auto; justify-items: start; gap: 9px; padding: 6px 10px; text-align: left; }
  .grid-checkbox { position: absolute; top: 7px; left: 7px; display: none; padding: 0; border: 0; background: transparent; cursor: pointer; }
  .remote-grid-item:hover .grid-checkbox, .remote-grid-item.selected .grid-checkbox { display: block; }
  .grid-entry-icon { display: grid; width: 64px; height: 56px; place-items: center; overflow: hidden; border-radius: 7px; color: #f3b51b; } .grid-entry-icon.has-thumbnail { border: 1px solid var(--border); background: var(--surface-soft); } .grid-entry-icon img { width: 100%; height: 100%; object-fit: cover; } .remote-grid-body.compact .grid-entry-icon { width: 30px; height: 30px; } .grid-entry-copy { display: grid; min-width: 0; gap: 4px; }
  .grid-entry-copy strong, .grid-entry-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } .grid-entry-copy strong { color: var(--text); font-size: 12px; font-weight: 570; } .grid-entry-copy small { color: var(--text-muted); font-size: 10px; }
  .remote-empty { display: flex; height: 100%; min-height: 240px; align-items: center; justify-content: center; flex-direction: column; gap: 8px; color: var(--text-muted); grid-column: 1 / -1; }
  .remote-empty strong { color: var(--text); } .remote-empty span { font-size: 12px; } .remote-empty button { height: 34px; margin-top: 4px; padding: 0 13px; border: 1px solid var(--border-strong); border-radius: 8px; background: var(--control-bg); cursor: pointer; }
  .remote-spinner { width: 23px; height: 23px; border: 3px solid var(--border-strong); border-top-color: var(--accent); border-radius: 50%; animation: remote-spin 800ms linear infinite; }
  .remote-load-more { display: flex; min-height: 48px; align-items: center; justify-content: center; gap: 9px; color: var(--text-muted); font-size: 12px; }
  .remote-load-more .remote-spinner { width: 16px; height: 16px; border-width: 2px; }
  .grid-load-more { grid-column: 1 / -1; }
  .drag-feedback { position: absolute; z-index: 10; top: 10px; right: 12px; display: flex; height: 32px; align-items: center; gap: 7px; padding: 0 11px; border: 1px solid color-mix(in srgb, var(--accent) 45%, var(--border)); border-radius: 8px; color: var(--accent-strong); background: var(--surface-overlay); box-shadow: var(--shadow-soft); font-size: 11px; pointer-events: none; } .drag-feedback.readonly { border-color: color-mix(in srgb, var(--danger) 45%, var(--border)); color: var(--danger); }
  .remote-transfer-panel { position: absolute; z-index: 35; right: 18px; bottom: 38px; width: min(370px, calc(100% - 36px)); overflow: hidden; border: 1px solid var(--border-strong); border-radius: 12px; background: var(--surface-raised); box-shadow: var(--shadow-floating); }
  .remote-transfer-panel.collapsed { width: 220px; }
  .transfer-panel-heading { display: flex; width: 100%; height: 40px; align-items: center; gap: 8px; padding: 0 12px; border: 0; color: var(--text); background: var(--surface-soft); font-size: 11px; font-weight: 640; cursor: pointer; }
  .transfer-panel-heading span { margin-right: auto; } .transfer-panel-heading small { color: var(--text-muted); font-size: 10px; font-weight: 500; } .remote-transfer-panel:not(.collapsed) .transfer-panel-heading :global(svg) { transform: rotate(180deg); }
  .remote-transfer-list { display: grid; max-height: 270px; overflow: auto; }
  .remote-transfer-item { display: grid; grid-template-columns: 30px minmax(0, 1fr) auto; align-items: center; gap: 8px; min-height: 58px; padding: 7px 11px; border-top: 1px solid var(--border); }
  .transfer-direction { display: grid; width: 27px; height: 27px; place-items: center; border-radius: 8px; color: var(--accent-strong); background: var(--accent-soft); } .remote-transfer-item.failed .transfer-direction { color: var(--danger); background: var(--danger-soft); } .remote-transfer-item.completed .transfer-direction { color: var(--success); }
  .transfer-copy { display: grid; min-width: 0; gap: 4px; } .transfer-copy strong, .transfer-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } .transfer-copy strong { font-size: 11px; font-weight: 620; } .transfer-copy small { color: var(--text-muted); font-size: 9px; }
  .transfer-copy i { position: relative; display: block; height: 3px; overflow: hidden; border-radius: 3px; background: var(--border-strong); } .transfer-copy b { position: absolute; inset: 0 auto 0 0; min-width: 0; border-radius: inherit; background: var(--accent); transition: width 120ms linear; } .transfer-copy b.indeterminate { width: 35% !important; animation: remote-transfer-indeterminate 1.1s ease-in-out infinite; }
  .remote-transfer-item em { color: var(--text-muted); font-size: 9px; font-style: normal; } .remote-transfer-item.failed em { color: var(--danger); } .remote-transfer-item.completed em { color: var(--success); }
  .remote-status-bar { grid-area: status; display: flex; align-items: center; gap: 8px; padding: 0 18px; border-top: 1px solid var(--border); color: var(--text-muted); background: var(--surface); font-size: 10px; }
  .remote-status-bar i { width: 3px; height: 3px; border-radius: 50%; background: var(--text-muted); } .status-spacer { flex: 1; }
  .remote-dialog-backdrop { position: fixed; z-index: 100; inset: 0; display: grid; place-items: center; padding: 24px; background: var(--backdrop); }
  .remote-dialog { width: min(390px, 100%); padding: 22px; border: 1px solid var(--border-strong); border-radius: 15px; background: var(--surface-raised); box-shadow: var(--shadow-floating); }
  .remote-dialog h2 { margin: 0; font-size: 18px; } .remote-dialog p { margin: 8px 0 18px; color: var(--text-muted); font-size: 13px; }
  .remote-dialog input { width: 100%; height: 42px; padding: 0 12px; border: 1px solid var(--border-strong); border-radius: 9px; outline: 0; color: var(--text); background: var(--control-bg); box-sizing: border-box; }
  .remote-dialog input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus-ring); } .remote-dialog footer { display: flex; justify-content: flex-end; gap: 8px; margin-top: 18px; }
  .remote-dialog footer button { height: 38px; padding: 0 16px; border: 1px solid var(--border-strong); border-radius: 9px; background: var(--control-bg); cursor: pointer; }
  .remote-dialog footer .confirm { border-color: var(--accent); color: var(--text-inverse); background: var(--accent); } .remote-dialog footer .delete-confirm { border-color: var(--danger); color: white; background: var(--danger); }
  @keyframes remote-spin { to { transform: rotate(360deg); } }
  @keyframes remote-transfer-indeterminate { from { transform: translateX(-110%); } to { transform: translateX(300%); } }
  @media (max-width: 1240px) { .remote-command-row { grid-template-columns: auto minmax(100px, 1fr) auto auto; gap: 7px; padding-inline: 12px; } .command-actions .command-button span, .sort-button span { display: none; } .remote-files-body { grid-template-columns: 195px minmax(0, 1fr); } }
  @media (max-width: 980px) { .remote-title-row { grid-template-columns: minmax(0, 1fr) minmax(190px, 260px) 38px; padding-left: 18px; } .remote-title-cluster { gap: 12px; } .remote-title-row h1 { font-size: 21px; } .device-selector small, .display-label, .sort-button span { display: none; } .remote-files-body { grid-template-columns: 170px minmax(0, 1fr); } .remote-table-head, .remote-file-row { grid-template-columns: 38px minmax(190px, 1.5fr) 130px 85px; } .remote-table-head > :last-child, .remote-file-row > :last-child { display: none; } }
  @media (max-width: 780px) { .remote-title-row { grid-template-columns: minmax(0, 1fr) 38px; } .remote-search { display: none; } .remote-command-row { padding-inline: 10px; } .remote-files-body { grid-template-columns: 1fr; } .remote-source-panel { display: none; } }
</style>
