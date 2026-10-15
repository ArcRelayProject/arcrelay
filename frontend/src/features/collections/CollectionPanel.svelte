<script lang="ts">
  import { onMount, tick } from "svelte";
  import {
    Plus,
    CaretRight,
    CaretDown,
    StackSimple,
    Star,
    Copy,
    Eye,
    EyeSlash,
    PencilSimple,
    Trash,
    ArrowLeft,
    X,
    ArrowsClockwise,
    User,
    MapPin,
    CreditCard,
    Receipt,
    Clock,
    FileText,
  } from "phosphor-svelte";
  import type {
    ClipboardCollection,
    CollectionDraft,
    CollectionItem,
    TotpCode,
    CollectionPeerState,
  } from "../../ipc/generated";
  import { collectionRequest as request } from "./bridge";
  import { templates, fromTemplate, toDraft, matches } from "./model";
  import CollectionEditor from "./CollectionEditor.svelte";
  import "./collections.css";
  export let targetName = "";
  export let search = "";
  export let editing = false;
  export let historyIds: number[] = [];
  let peers: CollectionPeerState[] = [];
  let used: Record<string, number> = {};
  try {
    used = JSON.parse(localStorage.getItem("collection-last-used") ?? "{}");
  } catch {
    /* optional local ordering */
  }
  let frozenOrder: string[] = [];
  let records: ClipboardCollection[] = [];
  let expanded = "";
  let page: "list" | "new" | "edit" | "sync" = "list";
  let draft: CollectionDraft = fromTemplate(0);
  let chosen = 0;
  let busy = false;
  let loading = true;
  let error = "";
  let syncEnabled = true;
  let syncEdits = true;
  let copied = "";
  let selected = "";
  let reveal = new Set<string>();
  let otp: Record<string, TotpCode> = {};
  let now = Date.now();
  let remove: ClipboardCollection | null = null;
  let disposed = false;
  let refreshing = false;
  $: editing = page !== "list";
  $: current = records.find((r) => r.id === draft.id);
  $: visible = records
    .filter((r) => !r.deleted && matches(r, search))
    .sort(
      (a, b) =>
        Number(b.favorite) - Number(a.favorite) ||
        (used[b.id] ?? 0) - (used[a.id] ?? 0) ||
        a.name.localeCompare(b.name) ||
        a.id.localeCompare(b.id),
    )
    .sort((a, b) =>
      expanded && frozenOrder.length
        ? (frozenOrder.indexOf(a.id) < 0 ? Infinity : frozenOrder.indexOf(a.id)) -
          (frozenOrder.indexOf(b.id) < 0 ? Infinity : frozenOrder.indexOf(b.id))
        : 0,
    );
  async function refresh() {
    if (refreshing) return;
    refreshing = true;
    try {
      let after = "",
        all: ClipboardCollection[] = [];
      do {
        const result = await request({ type: "list", after });
        all.push(...result.records);
        syncEnabled = result.syncEnabled;
        syncEdits = result.syncEdits;
        if (!result.records.length) break;
        after = result.records[result.records.length - 1].id;
      } while (all.length < 10000 && !disposed);
      if (!disposed) records = all;
      if (page === "sync") peers = (await request({ type: "sync", id: expanded })).peers;
    } catch (e) {
      if (!disposed) error = String(e);
    } finally {
      loading = false;
      refreshing = false;
    }
  }
  async function act(action: () => Promise<unknown>) {
    if (busy) return;
    busy = true;
    error = "";
    try {
      await action();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
  async function save(value: CollectionDraft) {
    await act(async () => {
      const result = await request({ type: "save", draft: value });
      expanded = result.saved?.id ?? "";
      frozenOrder = [];
      page = "list";
      reveal = new Set();
      await refresh();
      await tick();
      document.getElementById("collection-" + expanded)?.scrollIntoView({ block: "nearest" });
    });
  }
  async function use(row: ClipboardCollection, entry: CollectionItem, paste: boolean) {
    await act(async () => {
      await request({ type: "use", id: row.id, item: entry.id, paste });
      selected = entry.id;
      copied = entry.id;
      used = { ...used, [row.id]: Date.now() };
      try {
        localStorage.setItem("collection-last-used", JSON.stringify(used));
      } catch {
        /* copying still succeeded */
      }
      setTimeout(() => (copied = ""), 1500);
    });
  }
  async function updateOtp() {
    const row = records.find((r) => r.id === expanded);
    if (!row || page !== "list") return;
    for (const entry of row.items.filter((i) => i.totp)) {
      try {
        const result = await request({ type: "value", id: row.id, item: entry.id });
        if (!disposed && result.value) otp = { ...otp, [entry.id]: result.value };
      } catch {
        /* next tick retries; no stale code actions */
      }
    }
  }
  function toggle(id: string) {
    frozenOrder = visible.map((r) => r.id);
    expanded = expanded === id ? "" : id;
    reveal = new Set();
    otp = {};
    void updateOtp();
  }
  function show(entry: CollectionItem) {
    const next = new Set(reveal);
    if (next.has(entry.id)) next.delete(entry.id);
    else next.add(entry.id);
    reveal = next;
  }
  function newCollection() {
    chosen = 0;
    error = "";
    page = "new";
  }
  function keydown(event: KeyboardEvent) {
    if (
      page !== "list" ||
      (event.target instanceof Element && event.target.closest("input,textarea,select"))
    )
      return;
    const row = records.find((r) => r.id === expanded);
    if (!row) return;
    const index = row.items.findIndex((i) => i.id === selected);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      selected =
        row.items[
          Math.max(0, Math.min(row.items.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)))
        ]?.id ?? "";
    }
    if (event.key === "Enter") {
      const entry = row.items.find((i) => i.id === selected);
      if (entry) {
        event.preventDefault();
        void use(row, entry, true);
      }
    }
    if (event.key === "Escape") {
      expanded = "";
      reveal = new Set();
    }
  }
  onMount(() => {
    void refresh();
    const refreshTimer = setInterval(() => void refresh(), 3000);
    const tick = setInterval(() => {
      now = Date.now();
      void updateOtp();
    }, 1000);
    return () => {
      disposed = true;
      clearInterval(refreshTimer);
      clearInterval(tick);
    };
  });
  $: if (historyIds.length) {
    const ids = historyIds;
    historyIds = [];
    void act(async () => {
      const result = await request({ type: "fromHistory", ids });
      draft = { ...fromTemplate(0), items: result.items };
      page = "edit";
    });
  }
</script>

<svelte:window on:keydown={keydown} />
<div class="collection-panel">
  {#if page === "edit"}
    <CollectionEditor
      bind:draft
      {current}
      {busy}
      {error}
      onSave={save}
      onClose={() => {
        page = "list";
        error = "";
      }}
      onSettings={() => void act(() => request({ type: "openSettings" }))}
    />
  {:else if page === "new"}
    <header class="collection-topbar">
      <button class="icon" aria-label="返回" on:click={() => (page = "list")}
        ><ArrowLeft size={22} /></button
      ><span>新建集合</span><button class="icon" aria-label="关闭" on:click={() => (page = "list")}
        ><X size={22} /></button
      >
    </header>
    <div class="collection-editor-body">
      <h2>从哪里开始？</h2>
      <p class="muted">选择一个模板，内容都可以自由调整。</p>
      <div class="collection-templates">
        {#each templates as template, index}<button
            class:selected={chosen === index}
            on:click={() => (chosen = index)}
            aria-pressed={chosen === index}
            >{#if index === 0}<StackSimple size={24} />{:else if index === 1}<User
                size={24}
              />{:else if index === 2}<MapPin size={24} />{:else if index === 3}<CreditCard
                size={24}
              />{:else}<Receipt size={24} />{/if}<span
              ><strong>{template.name}</strong><small
                >{index ? template.labels.join("、") : "从一组空白内容开始"}</small
              ></span
            ></button
          >{/each}
      </div>
    </div>
    <footer class="collection-editor-footer">
      <button on:click={() => (page = "list")}>取消</button><button
        class="primary"
        on:click={() => {
          draft = fromTemplate(chosen);
          page = "edit";
        }}>继续</button
      >
    </footer>
  {:else if page === "sync"}
    <header class="collection-topbar">
      <button class="icon" aria-label="返回集合" on:click={() => (page = "list")}
        ><ArrowLeft size={22} /></button
      ><span>同步详情</span>
    </header>
    <div class="collection-editor-body">
      <h2>{records.find((r) => r.id === expanded)?.name ?? "剪贴板集合"}</h2>
      <p class="muted">跟随剪贴板同步设置</p>
      <div class="collection-notice">
        <ArrowsClockwise size={22} />{!syncEnabled
          ? "同步已暂停"
          : !syncEdits
            ? "修改仅保存在本机"
            : "本机已保存，后台持续校对设备副本"}
      </div>
      <div class="collection-sync-peers">
        {#each peers as peer}<div class="collection-notice">
            <strong>{peer.deviceName}</strong><span>{peer.status}</span>
          </div>{:else}<p class="muted">暂无已授权的同步设备。连接设备后会自动同步。</p>{/each}
      </div>
      <p class="muted">集合更新不会改变系统剪贴板；主动复制按剪贴板设置同步。</p>
      <button class="text-link" on:click={() => void act(() => request({ type: "openSettings" }))}
        >查看剪贴板设置 <CaretRight size={16} /></button
      >{#if error}<p role="alert" class="collection-error">{error}</p>{/if}
    </div>
    <footer class="collection-editor-footer">
      <button on:click={() => (page = "list")}>返回集合</button><button
        class="primary"
        disabled={busy || !syncEnabled}
        on:click={() =>
          void act(async () => {
            await request({ type: "retry" });
            await refresh();
          })}>{busy ? "正在同步…" : "重试同步"}</button
      >
    </footer>
  {:else}
    <div class="collection-list-body">
      <div class="collection-heading">
        <h2>我的集合</h2>
        <button class="primary" on:click={newCollection}><Plus size={18} />新建</button>
      </div>
      {#if error}<p role="alert" class="collection-error">{error}</p>{/if}
      {#if remove}<div class="collection-notice" role="alert">
          删除“{remove.name}”？{syncEnabled && syncEdits
            ? "已连接设备将同步删除。"
            : "其他设备副本不会因此删除。"}<button on:click={() => (remove = null)}>取消</button
          ><button
            class="danger"
            on:click={() =>
              void act(async () => {
                await request({
                  type: "remove",
                  id: remove!.id,
                  base: { revision: remove!.revision, deviceId: remove!.updatedByDeviceId },
                });
                remove = null;
                await refresh();
              })}>删除集合</button
          >
        </div>{/if}
      {#if loading}<p class="collection-empty">正在读取集合…</p>{:else if !visible.length}<div
          class="collection-empty"
        >
          <StackSimple size={36} />
          <p>{search ? "没有找到匹配的集合" : "把常用内容放在一起，需要时逐项粘贴。"}</p>
          {#if !search}<button on:click={newCollection}>新建集合</button>{/if}
        </div>{/if}
      {#each visible as row (row.id)}
        <article
          id={"collection-" + row.id}
          class="collection-card"
          class:expanded={expanded === row.id}
        >
          <div class="collection-card-header">
            <button
              class="collection-card-title"
              aria-expanded={expanded === row.id}
              on:click={() => toggle(row.id)}
              ><span class="collection-symbol"><StackSimple size={24} /></span><span
                ><strong>{row.name}</strong><small
                  >{row.items.map((i) => i.label).join("、") || "暂无内容"}</small
                ><small>{row.items.length} 项 · 已保存</small></span
              >{#if expanded === row.id}<CaretDown size={18} />{:else}<CaretRight
                  size={18}
                />{/if}</button
            ><button
              class="icon"
              aria-label={row.favorite ? "取消收藏" : "收藏"}
              on:click={() =>
                void act(async () => {
                  await request({ type: "favorite", id: row.id, favorite: !row.favorite });
                  await refresh();
                })}><Star size={18} weight={row.favorite ? "fill" : "regular"} /></button
            >
          </div>
          {#if expanded === row.id}
            <div class="collection-card-tools">
              <button
                on:click={() => {
                  draft = toDraft(row);
                  error = "";
                  page = "edit";
                }}><PencilSimple size={16} />编辑</button
              ><button
                on:click={() => {
                  error = "";
                  page = "sync";
                  void refresh();
                }}><ArrowsClockwise size={16} />同步</button
              ><button class="icon" aria-label="删除集合" on:click={() => (remove = row)}
                ><Trash size={16} /></button
              >
            </div>
            <p class="collection-target muted">
              {targetName ? "粘贴到 " + targetName : "选择目标应用后粘贴"}
            </p>
            <div class="collection-values">
              {#each row.items as entry (entry.id)}
                {@const remaining = entry.totp
                  ? Math.max(0, Math.ceil(((otp[entry.id]?.expiresAtMs ?? 0) - now) / 1000))
                  : 0}
                <div class="collection-value" class:selected={selected === entry.id}>
                  <button class="value-select" on:click={() => (selected = entry.id)}
                    >{#if entry.totp}<Clock size={18} />{:else}<FileText size={18} />{/if}<span
                      ><small>{entry.label}</small><span class:otp={!!entry.totp}
                        >{entry.totp
                          ? (otp[entry.id]?.code ?? "正在生成…")
                          : entry.masked && !reveal.has(entry.id)
                            ? "••••••••"
                            : entry.value || "空内容"}</span
                      >{#if entry.totp}<small
                          >{remaining < 5 ? "即将更新" : remaining + " 秒后更新"}</small
                        >{/if}</span
                    ></button
                  >
                  <div class="value-actions">
                    {#if entry.masked && !entry.totp}<button
                        class="icon"
                        aria-label={reveal.has(entry.id) ? "隐藏内容" : "显示内容"}
                        on:click={() => show(entry)}
                        >{#if reveal.has(entry.id)}<EyeSlash size={18} />{:else}<Eye
                            size={18}
                          />{/if}</button
                      >{/if}<button
                      class="icon"
                      aria-label={copied === entry.id ? "已复制" : "复制" + entry.label}
                      disabled={busy || (!!entry.totp && remaining < 5)}
                      on:click={() => void use(row, entry, false)}><Copy size={18} /></button
                    ><button
                      class:primary={selected === entry.id}
                      disabled={busy || (!!entry.totp && remaining < 5)}
                      on:click={() => void use(row, entry, true)}
                      >{copied === entry.id ? "已复制" : "粘贴"}</button
                    >
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </article>
      {/each}
    </div>
    <footer class="collection-list-footer">
      <ArrowsClockwise size={16} /><button
        class="text-link"
        on:click={() => {
          page = "sync";
          error = "";
          void refresh();
        }}>跟随剪贴板同步</button
      ><span class="spacer"></span><small>↑↓ 选择　Enter 粘贴</small>
    </footer>
  {/if}
</div>
