<script lang="ts">
  import AppSelect from "../components/AppSelect.svelte";
  import {
    ArrowsClockwise,
    CaretRight,
    CloudSlash,
    DownloadSimple,
    Eye,
    FileArchive,
    FileAudio,
    FileImage,
    FilePdf,
    FileText,
    FileVideo,
    FolderOpen,
    FolderSimple,
    GlobeHemisphereWest,
    GridFour,
    House,
    LockKey,
    MagnifyingGlass,
    Rows,
    SignOut,
    X,
  } from "phosphor-svelte";

  import BrandLogo from "../BrandLogo.svelte";
  import { api, endpoint } from "./api";
  import PreviewViewer from "./PreviewViewer.svelte";
  import type { DirectoryPage, FileEntry, ShareView } from "./types";

  export let share: ShareView;
  export let siteName = "ArcRelay";
  export let initialPath = "";
  export let navigatePath: (path: string) => void;
  export let goHome: () => void;
  export let lockShare: () => void | Promise<void>;
  export let requestExit: () => void;

  let currentPath = "";
  let page: DirectoryPage | null = null;
  let entries: FileEntry[] = [];
  let loading = true;
  let loadingMore = false;
  let error = "";
  let search = "";
  let preview: FileEntry | null = null;
  let view: "list" | "grid" = "list";
  let sort: "modified" | "name" = "modified";
  let loadedPath: string | null = null;
  let loadRevision = 0;

  $: filtered = search.trim()
    ? entries.filter((entry) => entry.name.toLocaleLowerCase().includes(search.trim().toLocaleLowerCase()))
    : entries;
  $: visibleEntries = [...filtered].sort((left, right) => {
    if (left.kind !== right.kind) return left.kind === "folder" ? -1 : 1;
    return sort === "name"
      ? left.name.localeCompare(right.name, "zh-CN")
      : right.modifiedAtMs - left.modifiedAtMs;
  });
  $: breadcrumbs = currentPath ? currentPath.split("/") : [];
  $: folderName = breadcrumbs.at(-1) ?? share.name;

  $: if (initialPath !== loadedPath) void load(initialPath);

  async function load(path: string) {
    const revision = ++loadRevision;
    loadedPath = path;
    currentPath = path;
    loading = true;
    error = "";
    search = "";
    try {
      const nextPage = await api.entries(share.slug, path);
      if (revision !== loadRevision) return;
      page = nextPage;
      currentPath = nextPage.path;
      loadedPath = nextPage.path;
      entries = nextPage.entries;
    } catch (value) {
      if (revision === loadRevision) error = value instanceof Error ? value.message : String(value);
    } finally {
      if (revision === loadRevision) loading = false;
    }
  }

  async function loadMore() {
    if (!page?.nextCursor || loadingMore) return;
    loadingMore = true;
    try {
      const next = await api.entries(share.slug, currentPath, page.nextCursor);
      entries = [...entries, ...next.entries];
      page = next;
    } catch (value) {
      error = value instanceof Error ? value.message : String(value);
    } finally {
      loadingMore = false;
    }
  }

  async function open(entry: FileEntry) {
    if (entry.kind === "folder") {
      navigatePath(entry.relativePath);
      return;
    }
    try {
      const metadata = await api.metadata(share.slug, entry.relativePath);
      preview = metadata.entry;
    } catch (value) {
      error = value instanceof Error ? value.message : String(value);
    }
  }

  function crumbPath(index: number) { return breadcrumbs.slice(0, index + 1).join("/"); }
  function formatSize(size: number) {
    if (size < 1024) return `${size} B`;
    if (size < 1024 ** 2) return `${(size / 1024).toFixed(1)} KB`;
    if (size < 1024 ** 3) return `${(size / 1024 ** 2).toFixed(1)} MB`;
    return `${(size / 1024 ** 3).toFixed(1)} GB`;
  }
  function formatDate(value: number) {
    return new Intl.DateTimeFormat("zh-CN", { year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hour12: false }).format(value).replaceAll("/", "-");
  }
</script>

<section class:error-state={Boolean(error)} class="file-browser">
  <aside class="browser-sidebar">
    <div class="browser-brand"><BrandLogo size={29} /><strong>ArcRelay</strong></div>
    <div class="browser-nav-label"><FolderOpen size={19} /> 浏览器文件</div>
    <button class="all-shares-button" on:click={goHome}><House size={18} /> 所有共享</button>

    <div class="active-share">
      <span class="active-share-icon"><FolderOpen size={24} weight="duotone" /></span>
      <div><strong>{share.name}</strong><small>只读共享</small></div>
      <span class="capability"><Eye size={17} /> {share.allowPreview ? "可预览" : "不可预览"}</span>
      <span class="capability"><DownloadSimple size={17} /> {share.allowDownload ? "可下载" : "不可下载"}</span>
    </div>

    <div class="sidebar-actions">
      {#if share.mode === "password"}<button on:click={lockShare}><LockKey size={18} /> 锁定文件夹</button>{/if}
      <button on:click={requestExit}><SignOut size={18} /> 退出会话</button>
    </div>
  </aside>

  <div class="browser-main">
    <header class="browser-topbar">
      <nav class="breadcrumbs" aria-label="当前位置">
        <button on:click={() => navigatePath("")}>{share.name}</button>
        {#each breadcrumbs as crumb, index}<CaretRight size={14} /><button class:current={index === breadcrumbs.length - 1} on:click={() => navigatePath(crumbPath(index))}>{crumb}</button>{/each}
      </nav>
      <span class:error={Boolean(error)} class="network-status"><i></i>{error ? "连接已中断" : "局域网连接"}</span>
    </header>

    <div class="browser-workspace">
      <div class="browser-toolbar">
        <label class:focused={Boolean(search)} class="search-field"><MagnifyingGlass size={19} /><input bind:value={search} disabled={loading || Boolean(error)} aria-label="搜索当前文件夹" placeholder="搜索当前文件夹" />{#if search}<button aria-label="清除搜索" on:click={() => (search = "")}><X size={16} /></button>{/if}</label>
        <div class="toolbar-actions">
          <div class="view-switch" aria-label="文件显示方式"><button class:active={view === "list"} disabled={loading || Boolean(error)} aria-label="列表视图" on:click={() => (view = "list")}><Rows size={20} /></button><button class:active={view === "grid"} disabled={loading || Boolean(error)} aria-label="网格视图" on:click={() => (view = "grid")}><GridFour size={19} /></button></div>
          <button class="toolbar-button" disabled={loading || Boolean(error)} aria-label="刷新" on:click={() => load(currentPath)}><ArrowsClockwise size={19} /></button>
          <span class:error={Boolean(error)} class="connection-control"><i></i><GlobeHemisphereWest size={16} />{error ? "连接已中断" : "局域网连接"}</span>
        </div>
      </div>

      <div class="folder-heading">
        <div><h1>{folderName}</h1><span>{#if loading}<span class="mini-spinner"></span> 正在读取{folderName}…{:else}{entries.length} 个项目{/if}</span></div>
        <label class="sort-control">排序<AppSelect bind:value={sort} disabled={loading || Boolean(error)} aria-label={"排序"}
          options={[
            { value: "modified", label: "按修改时间" },
            { value: "name", label: "按名称" },
          ]}
        /></label>
      </div>

      <div class:loading class="browser-content">
        {#if loading}
          <div class="file-table skeleton-table" aria-live="polite" aria-label="正在读取文件夹">
            <div class="file-table-head"><span>名称</span><span>大小</span><span>修改时间</span><span></span></div>
            {#each Array(6) as _}<div class="skeleton-row"><span><i></i><b></b></span><i></i><i></i><i></i></div>{/each}
          </div>
        {:else if error}
          <div class="connection-empty">
            <span class="connection-empty-icon"><CloudSlash size={46} /></span>
            <h2>与这台 Mac 的连接已中断</h2>
            <p>ArcRelay 可能已停止运行、设备已离开当前网络，或会话已过期。</p>
            <div class="connection-details"><span>主机<strong>{siteName}</strong></span><span>上次连接<strong>刚刚</strong></span></div>
            <div class="recovery-actions"><button class="primary-action" on:click={() => load(currentPath)}><ArrowsClockwise size={18} /> 重新连接</button><button class="secondary-button" on:click={goHome}>返回所有共享</button></div>
            <details><summary>仍然无法连接？</summary><p>请确认 ArcRelay 正在运行，并且浏览器和主机仍连接到同一局域网。</p></details>
          </div>
        {:else if visibleEntries.length === 0}
          <div class="search-empty">
            <span class="search-empty-icon"><MagnifyingGlass size={38} /></span>
            <h2>{search ? `没有找到“${search}”` : "这个文件夹是空的"}</h2>
            <p>{search ? `请检查关键词，或清除搜索查看全部 ${entries.length} 个项目。` : "这里还没有可浏览的文件。"}</p>
            {#if search}<button class="text-action" on:click={() => (search = "")}>清除搜索</button>{/if}
          </div>
        {:else if view === "list"}
          <div class="file-table">
            <div class="file-table-head"><span>名称</span><span>大小</span><span>修改时间</span><span></span></div>
            {#each visibleEntries as entry (entry.relativePath)}
              <button class="file-row" on:click={() => open(entry)}>
                <span class="file-name"><i class={`file-icon ${entry.kind} ${entry.previewKind}`}>{#if entry.kind === "folder"}<FolderSimple size={24} weight="duotone" />{:else if entry.previewKind === "image"}<FileImage size={23} />{:else if entry.previewKind === "video"}<FileVideo size={23} />{:else if entry.previewKind === "audio"}<FileAudio size={23} />{:else if entry.mediaType.includes("pdf")}<FilePdf size={23} />{:else if entry.mediaType.includes("zip") || entry.mediaType.includes("archive")}<FileArchive size={23} />{:else}<FileText size={23} />{/if}</i><span><strong>{entry.name}</strong><small>{entry.kind === "folder" ? "文件夹" : entry.mediaType}</small></span></span>
                <span>{entry.kind === "file" ? formatSize(entry.size) : "—"}</span><span>{formatDate(entry.modifiedAtMs)}</span><span class="row-arrow"><CaretRight size={17} /></span>
              </button>
            {/each}
          </div>
        {:else}
          <div class="file-grid">
            {#each visibleEntries as entry (entry.relativePath)}
              <button class="file-tile" on:click={() => open(entry)}>
                <span class={`tile-preview ${entry.kind} ${entry.previewKind}`}>
                  {#if entry.previewKind === "image"}<img src={endpoint(share.slug, "thumbnail", entry.relativePath, { dimension: "480" })} alt="" loading="lazy" />
                  {:else if entry.kind === "folder"}<FolderSimple size={58} weight="duotone" />
                  {:else if entry.previewKind === "video"}<FileVideo size={54} />
                  {:else if entry.previewKind === "audio"}<FileAudio size={54} />
                  {:else if entry.mediaType.includes("pdf")}<FilePdf size={54} />
                  {:else if entry.mediaType.includes("zip") || entry.mediaType.includes("archive")}<FileArchive size={54} />
                  {:else}<FileText size={54} />{/if}
                </span>
                <span class="tile-copy"><strong>{entry.name}</strong><small>{entry.kind === "file" ? `${formatSize(entry.size)} · ${formatDate(entry.modifiedAtMs)}` : `文件夹 · ${formatDate(entry.modifiedAtMs)}`}</small></span>
              </button>
            {/each}
          </div>
        {/if}
        {#if page?.nextCursor && !loading && !error}<button class="load-more" disabled={loadingMore} on:click={loadMore}>{loadingMore ? "正在载入…" : "载入更多"}</button>{/if}
      </div>
    </div>
  </div>
</section>

{#if preview}<PreviewViewer {share} entry={preview} close={() => (preview = null)} />{/if}
