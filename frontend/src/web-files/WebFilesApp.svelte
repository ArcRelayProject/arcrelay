<script lang="ts">
  import { onMount } from "svelte";
  import { GlobeHemisphereWest, SignOut } from "phosphor-svelte";

  import BrandLogo from "../BrandLogo.svelte";
  import { appIcon32Url } from "../brandAssets";
  import { api } from "./api";
  import ExitSessionDialog from "./ExitSessionDialog.svelte";
  import FileBrowser from "./FileBrowser.svelte";
  import { parseWebFilesRoute, webFilesUrl } from "./navigation";
  import PasswordDialog from "./PasswordDialog.svelte";
  import ShareHome from "./ShareHome.svelte";
  import type { ShareView, SiteView } from "./types";

  let site: SiteView | null = null;
  let shares: ShareView[] = [];
  let selected: ShareView | null = null;
  let selectedPath = "";
  let pendingUnlock: ShareView | null = null;
  let pendingUnlockPath = "";
  let pendingUnlockNavigate = true;
  let unlockBusy = false;
  let unlockError = "";
  let loading = true;
  let fatalError = "";
  let exitOpen = false;
  let exitBusy = false;
  let exitError = "";
  let navigationRevision = 0;

  initialize();

  onMount(() => {
    const handlePopState = () => void syncFromLocation().catch((value) => {
      fatalError = value instanceof Error ? value.message : String(value);
    });
    window.addEventListener("popstate", handlePopState);
    return () => window.removeEventListener("popstate", handlePopState);
  });

  async function initialize() {
    loading = true;
    fatalError = "";
    try {
      site = await api.site();
      await syncFromLocation();
    } catch (value) {
      fatalError = value instanceof Error ? value.message : String(value);
    } finally {
      loading = false;
    }
  }

  async function syncFromLocation() {
    const revision = ++navigationRevision;
    const route = parseWebFilesRoute(location);
    if (route.kind === "home") {
      selected = null;
      selectedPath = "";
      pendingUnlock = null;
      const nextShares = await api.shares();
      if (revision === navigationRevision) shares = nextShares;
      return;
    }
    if (selected?.slug === route.slug && selected.unlocked) {
      selectedPath = route.path;
      return;
    }
    const { share } = await api.share(route.slug);
    if (revision === navigationRevision) openShare(share, { path: route.path, navigate: false });
  }

  function openShare(share: ShareView, options: { path?: string; navigate?: boolean } = {}) {
    const path = options.path ?? "";
    const navigate = options.navigate ?? true;
    if (!share.unlocked) {
      pendingUnlock = share;
      pendingUnlockPath = path;
      pendingUnlockNavigate = navigate;
      unlockError = "";
      return;
    }
    selected = share;
    selectedPath = path;
    if (navigate) {
      navigationRevision += 1;
      history.pushState(null, "", webFilesUrl({ kind: "share", slug: share.slug, path }));
    }
  }

  async function unlock(password: string) {
    if (!pendingUnlock) return;
    unlockBusy = true;
    unlockError = "";
    try {
      await api.unlock(pendingUnlock.slug, password);
      const unlocked = { ...pendingUnlock, unlocked: true };
      const path = pendingUnlockPath;
      const navigate = pendingUnlockNavigate;
      shares = shares.map((share) => share.slug === unlocked.slug ? unlocked : share);
      pendingUnlock = null;
      pendingUnlockPath = "";
      openShare(unlocked, { path, navigate });
    } catch (value) {
      unlockError = value instanceof Error ? value.message : String(value);
    } finally {
      unlockBusy = false;
    }
  }

  async function goHome() {
    navigationRevision += 1;
    selected = null;
    selectedPath = "";
    history.pushState(null, "", webFilesUrl({ kind: "home" }));
    shares = await api.shares();
  }

  function navigatePath(path: string) {
    if (!selected) return;
    navigationRevision += 1;
    selectedPath = path;
    const url = webFilesUrl({ kind: "share", slug: selected.slug, path });
    if (`${location.pathname}${location.search}` !== url) history.pushState(null, "", url);
  }

  async function cancelUnlock() {
    navigationRevision += 1;
    pendingUnlock = null;
    pendingUnlockPath = "";
    if (parseWebFilesRoute(location).kind === "share") history.replaceState(null, "", webFilesUrl({ kind: "home" }));
    shares = await api.shares();
  }

  async function lockSelected() {
    if (!selected) return;
    await api.lock(selected.slug);
    navigationRevision += 1;
    const locked = { ...selected, unlocked: false };
    selected = null;
    selectedPath = "";
    shares = shares.map((share) => share.slug === locked.slug ? locked : share);
    history.replaceState(null, "", webFilesUrl({ kind: "home" }));
  }

  function requestExit() {
    exitError = "";
    exitOpen = true;
  }

  async function logout() {
    exitBusy = true;
    exitError = "";
    try {
      await api.logout();
      navigationRevision += 1;
      selected = null;
      selectedPath = "";
      shares = await api.shares();
      history.replaceState(null, "", webFilesUrl({ kind: "home" }));
      exitOpen = false;
    } catch (value) {
      exitError = value instanceof Error ? value.message : String(value);
    } finally {
      exitBusy = false;
    }
  }

  $: unlockedCount = shares.filter((share) => share.mode === "password" && share.unlocked).length
    + (selected?.mode === "password" && selected.unlocked && !shares.some((share) => share.slug === selected?.slug) ? 1 : 0);
</script>

<svelte:head>
  <title>{site?.siteName ?? "ArcRelay 浏览器文件"}</title>
  <link rel="icon" type="image/png" href={appIcon32Url} />
</svelte:head>

{#if loading}
  <main class="loading-screen" aria-live="polite">
    <BrandLogo size={54} />
    <span class="spinner"></span>
    <div><strong>正在连接 ArcRelay</strong><p>正在确认这台 Mac 的共享状态…</p></div>
  </main>
{:else if fatalError}
  <main class="loading-screen error-screen">
    <span class="status-illustration error">!</span>
    <div><h1>无法连接这台 Mac</h1><p>{fatalError}</p></div>
    <button class="primary-action" on:click={initialize}>重新连接</button>
  </main>
{:else if selected}
  <FileBrowser share={selected} siteName={site?.siteName ?? "ArcRelay"} initialPath={selectedPath} {navigatePath} {goHome} lockShare={lockSelected} requestExit={requestExit} />
{:else}
  <div class="site-shell">
    <header class="site-header">
      <div class="site-brand">
        <BrandLogo size={29} />
        <strong>ArcRelay</strong>
        <span class="header-separator"></span>
        <button class="host-switcher" aria-label="当前主机">{site?.siteName}</button>
      </div>
      <div class="site-actions">
        <span class="connection-pill"><i></i><GlobeHemisphereWest size={16} /> 局域网连接</span>
        <span class="header-separator"></span>
        <button class="secondary-button" on:click={requestExit}><SignOut size={17} /> 退出会话</button>
      </div>
    </header>

    <main class="home-main">
      <ShareHome {shares} openShare={openShare} />
    </main>

    <footer class="site-footer"><span>ArcRelay {site?.version}</span><span>本机只读共享 · HTTP 局域网连接</span></footer>
  </div>
{/if}

{#if pendingUnlock}
  <PasswordDialog shareName={pendingUnlock.name} busy={unlockBusy} error={unlockError} cancel={cancelUnlock} {unlock} />
{/if}

{#if exitOpen}
  <ExitSessionDialog
    busy={exitBusy}
    error={exitError}
    shareCount={shares.length || (selected ? 1 : 0)}
    {unlockedCount}
    cancel={() => !exitBusy && (exitOpen = false)}
    confirm={logout}
  />
{/if}
