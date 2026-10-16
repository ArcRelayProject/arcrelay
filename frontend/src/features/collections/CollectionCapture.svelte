<script lang="ts">
  import { onMount } from "svelte";
  import { ArrowLeft, X, StackSimple, MagnifyingGlass, Plus, Check } from "phosphor-svelte";
  import type { ClipboardCollection, CollectionDraft, CollectionItem } from "../../ipc/generated";
  import { collectionRequest as request, readCollections } from "./bridge";
  import { fromTemplate, toDraft } from "./model";
  import CollectionEditor from "./CollectionEditor.svelte";
  import "./collections.css";

  export let ids: number[];
  export let intent: "add" | "new";
  export let onClose: () => void;
  export let onSaved: (record: ClipboardCollection, appended: boolean) => void;
  let items: CollectionItem[] = [];
  let records: ClipboardCollection[] = [];
  let destination: ClipboardCollection | null = null;
  let query = "";
  let draft: CollectionDraft = fromTemplate(0);
  let editor = false;
  let loading = true;
  let busy = false;
  let error = "";
  let disposed = false;
  $: choices = records
    .filter(
      (record) =>
        !record.deleted &&
        record.name.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()),
    )
    .sort((a, b) => Number(b.favorite) - Number(a.favorite) || a.name.localeCompare(b.name));
  $: latest = records.find((record) => record.id === destination?.id);
  $: stale =
    !!destination &&
    (!latest ||
      latest.deleted ||
      latest.revision !== destination.revision ||
      latest.updatedByDeviceId !== destination.updatedByDeviceId);
  $: full = !!destination && destination.items.length + items.length > 64;

  function newDraft() {
    draft = { ...fromTemplate(0), items: structuredClone(items) };
    editor = true;
    error = "";
  }
  async function load() {
    loading = true;
    error = "";
    try {
      const [result, all] = await Promise.all([
        request({ type: "fromHistory", ids }),
        readCollections(),
      ]);
      if (disposed) return;
      items = result.items;
      records = all;
      if (intent === "new") newDraft();
    } catch (reason) {
      error = String(reason);
    } finally {
      loading = false;
    }
  }
  async function refresh() {
    try {
      const all = await readCollections();
      if (!disposed) records = all;
    } catch {
      /* The version check on save remains authoritative. Keep the user's input. */
    }
  }
  async function save(value: CollectionDraft, appended = false) {
    if (busy) return;
    busy = true;
    error = "";
    try {
      const result = await request({ type: "save", draft: value });
      if (result.saved) onSaved(result.saved, appended);
    } catch (reason) {
      error = String(reason).includes("COLLECTION_STALE")
        ? "目标集合已更新或删除，内容已保留，请重新选择集合并确认。"
        : String(reason);
      await refresh();
      if (appended) destination = null;
    } finally {
      busy = false;
    }
  }
  onMount(() => {
    void load();
    const timer = setInterval(() => {
      if (!editor && !busy) void refresh();
    }, 3000);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  });
</script>

<div class="collection-panel collection-capture">
  {#if editor}
    <CollectionEditor
      bind:draft
      {busy}
      {error}
      fromHistory
      title="保存为集合"
      onSave={(value) => void save(value)}
      onClose={() => {
        if (intent === "new") onClose();
        else {
          editor = false;
          error = "";
        }
      }}
    />
  {:else}
    <header class="collection-topbar">
      <button class="icon" aria-label="返回剪贴板" disabled={busy} on:click={onClose}
        ><ArrowLeft size={22} /></button
      >
      <span>添加到集合</span>
      <button class="icon" aria-label="关闭" disabled={busy} on:click={onClose}
        ><X size={22} /></button
      >
    </header>
    <div class="collection-editor-body collection-capture-body">
      {#if loading}<p class="muted" role="status">正在读取文本…</p>
      {:else if !items.length}
        <p class="collection-error" role="alert">{error || "未读取到可用文本"}</p>
        <button on:click={() => void load()}>重试</button>
      {:else}
        <small>待添加内容</small>
        <div class="collection-capture-preview">{items[0].value}</div>
        <label class="collection-label"
          >内容项名称<input maxlength="120" bind:value={items[0].label} disabled={busy} /></label
        >
        <div class="collection-destination-heading">保存到</div>
        <label class="collection-destination-search"
          ><MagnifyingGlass size={18} /><input
            aria-label="搜索集合"
            placeholder="搜索集合…"
            bind:value={query}
            disabled={busy}
          /></label
        >
        <div class="collection-destinations" role="group" aria-label="选择集合">
          {#each choices as record (record.id)}
            <button
              class:selected={destination?.id === record.id}
              aria-pressed={destination?.id === record.id}
              disabled={busy || record.items.length + items.length > 64}
              on:click={() => {
                destination = structuredClone(record);
                error = "";
              }}
            >
              <StackSimple size={22} /><span
                ><strong>{record.name}</strong><small
                  >{record.items.length >= 64
                    ? "已达 64 项上限"
                    : (record.items.map((entry) => entry.label).join("、") || "暂无内容") +
                      " · " +
                      record.items.length +
                      " 项"}</small
                ></span
              >
              {#if destination?.id === record.id}<Check size={18} />{/if}
            </button>
          {:else}<p class="muted collection-destination-empty">
              {query ? "没有找到匹配的集合" : "还没有集合，可以直接新建。"}
            </p>{/each}
          <button class="collection-new-destination" on:click={newDraft} disabled={busy}
            ><Plus size={22} /><span>新建集合…</span></button
          >
        </div>
        <p class="muted">添加为新的内容项，原剪贴板记录保留。</p>
        {#if stale}<p class="collection-error" role="alert">
            目标集合已更新或删除，请重新选择并确认。
          </p>{/if}
        {#if full}<p class="collection-error" role="alert">
            添加后超过 64 项，请选择其他集合。
          </p>{/if}
        {#if error}<p class="collection-error" role="alert">{error}</p>{/if}
      {/if}
    </div>
    <footer class="collection-editor-footer">
      <button on:click={onClose} disabled={busy}>取消</button>
      <button
        class="primary"
        disabled={loading || busy || !destination || stale || full || !items[0]?.label.trim()}
        on:click={() =>
          destination &&
          void save(
            {
              ...toDraft(destination),
              items: [...structuredClone(destination.items), ...structuredClone(items)],
            },
            true,
          )}>{busy ? "正在添加…" : "添加到集合"}</button
      >
    </footer>
  {/if}
</div>
