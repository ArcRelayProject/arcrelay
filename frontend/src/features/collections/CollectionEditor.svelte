<script lang="ts">
  import {
    ArrowLeft,
    ArrowUp,
    ArrowDown,
    Plus,
    Trash,
    Eye,
    EyeSlash,
    DotsSixVertical,
    X,
    ArrowsClockwise,
  } from "phosphor-svelte";
  import type { CollectionDraft, ClipboardCollection, CollectionItem } from "../../ipc/generated";
  import AppSelect from "../../components/AppSelect.svelte";
  import { item, toDraft } from "./model";
  export let draft: CollectionDraft;
  export let current: ClipboardCollection | undefined = undefined;
  export let busy = false;
  export let error = "";
  export let title = "";
  export let fromHistory = false;
  export let onSave: (draft: CollectionDraft) => void;
  export let onClose: () => void;
  export let onSettings: (() => void) | undefined = undefined;
  let initial = JSON.stringify(draft);
  let discard = false;
  let reveal = new Set<string>();
  let dragged = -1;
  let pendingType: number | null = null;
  $: stale =
    !!draft.id &&
    (!current ||
      current.deleted ||
      current.revision !== draft.base?.revision ||
      current.updatedByDeviceId !== draft.base?.deviceId);
  function close() {
    if (busy) return;
    if (JSON.stringify(draft) !== initial) discard = true;
    else onClose();
  }
  function move(from: number, to: number) {
    if (busy || from < 0 || to < 0 || to >= draft.items.length) return;
    const items = [...draft.items];
    const [entry] = items.splice(from, 1);
    items.splice(to, 0, entry);
    draft = { ...draft, items };
  }
  function changeType(index: number) {
    const entry = draft.items[index];
    if (entry.value || entry.totp?.secret) pendingType = index;
    else applyType(index);
  }
  function applyType(index: number) {
    const entry = draft.items[index];
    draft.items[index] = {
      ...entry,
      value: "",
      totp: entry.totp ? null : { secret: "", algorithm: "SHA1", digits: 6, period: 30 },
    };
    draft = { ...draft };
    pendingType = null;
  }
  function toggle(entry: CollectionItem) {
    const next = new Set(reveal);
    if (next.has(entry.id)) next.delete(entry.id);
    else next.add(entry.id);
    reveal = next;
  }
</script>

<div class="collection-editor" class:from-history={fromHistory}>
  <header class="collection-topbar">
    <button class="icon" aria-label="返回" on:click={close}><ArrowLeft size={22} /></button><span
      >{title || (draft.id ? "编辑集合" : "新建集合")}</span
    ><button class="icon" aria-label="关闭" on:click={close}><X size={22} /></button>
  </header>
  <div class="collection-editor-body">
    {#if discard}<div class="collection-notice" role="alert">
        尚未保存修改。<button on:click={() => (discard = false)}>继续编辑</button><button
          on:click={onClose}>放弃修改</button
        >
      </div>{/if}
    {#if error}<div class="collection-error" role="alert">
        {error.includes("COLLECTION_STALE") ? "此集合有新版本，请重新载入或另存为新集合。" : error}
      </div>{/if}
    {#if stale}<div class="collection-notice" role="status">
        此集合有新版本，当前草稿已保留。
        {#if current && !current.deleted}<button
            on:click={() => {
              if (confirm("重新载入会放弃当前草稿，继续吗？")) {
                draft = toDraft(current!);
                initial = JSON.stringify(draft);
              }
            }}>重新载入</button
          >
          <button
            on:click={() => {
              if (confirm("当前草稿将覆盖已收到的新版内容，确认保存吗？"))
                onSave({
                  ...draft,
                  base: { revision: current!.revision, deviceId: current!.updatedByDeviceId },
                });
            }}>保存当前草稿</button
          >{/if}
        <button on:click={() => onSave({ ...draft, id: null, base: null })}>另存为新集合</button>
      </div>{/if}
    <label class="collection-label"
      >集合名称<input
        maxlength="120"
        bind:value={draft.name}
        placeholder="为这组内容起个名字"
        disabled={busy}
      /></label
    >
    <div class="collection-section-title">
      <strong>内容项</strong><small>{draft.items.length}</small><span class="spacer"></span><small
        >拖动调整顺序</small
      >
    </div>
    <div class="collection-fields">
      {#each draft.items as entry, index (entry.id)}
        <section
          class="collection-field"
          on:dragover|preventDefault
          on:drop|preventDefault={() => {
            move(dragged, index);
            dragged = -1;
          }}
          role="group"
          aria-label={entry.label || "内容项"}
        >
          <div class="field-heading">
            <button
              class="icon drag"
              draggable={!busy}
              on:dragstart={() => (dragged = index)}
              on:keydown={(event) => {
                if (event.key === "ArrowUp" || event.key === "ArrowDown") {
                  event.preventDefault();
                  move(index, index + (event.key === "ArrowUp" ? -1 : 1));
                }
              }}
              title="拖动排序，或聚焦后按上下方向键"
              aria-label="拖动内容项"><DotsSixVertical size={20} /></button
            >
            <input
              class="field-name"
              aria-label="字段名称"
              maxlength="120"
              bind:value={entry.label}
              disabled={busy}
            />
            {#key `${entry.id}-${!!entry.totp}-${pendingType}`}
              <AppSelect
                class="field-type"
                aria-label="内容类型"
                value={entry.totp ? "totp" : "text"}
                onValueChange={(value) => {
                  if (value !== (entry.totp ? "totp" : "text")) changeType(index);
                }}
                disabled={busy}
                options={[
                  { value: "text", label: "文本" },
                  { value: "totp", label: "TOTP" },
                ]}
              />
            {/key}
            <button
              class="icon"
              aria-label="删除内容项"
              on:click={() =>
                (draft = { ...draft, items: draft.items.filter((i) => i.id !== entry.id) })}
              disabled={busy}
              >{#if fromHistory}<X size={18} />{:else}<Trash size={18} />{/if}</button
            >
          </div>
          {#if pendingType === index}<div class="collection-notice">
              切换类型会清空当前内容。<button on:click={() => (pendingType = null)}>取消</button
              ><button on:click={() => applyType(index)}>确认切换</button>
            </div>{/if}
          <div class="field-content">
            {#if entry.totp}
              <label
                >密钥或 otpauth 地址<input
                  type={reveal.has(entry.id) ? "text" : "password"}
                  bind:value={entry.totp.secret}
                  autocomplete="off"
                  disabled={busy}
                /></label
              >
              <div class="totp-options">
                <div class="totp-option">
                  算法<AppSelect
                    aria-label="算法"
                    bind:value={entry.totp.algorithm}
                    disabled={busy}
                    options={["SHA1", "SHA256", "SHA512"].map((value) => ({ value, label: value }))}
                  />
                </div>
                <div class="totp-option">
                  位数<AppSelect
                    aria-label="位数"
                    bind:value={entry.totp.digits}
                    disabled={busy}
                    options={[
                      { value: 6, label: "6" },
                      { value: 8, label: "8" },
                    ]}
                  />
                </div>
                <label
                  >周期（秒）<input
                    type="number"
                    min="15"
                    max="120"
                    bind:value={entry.totp.period}
                  /></label
                >
              </div>
            {:else if entry.masked && !reveal.has(entry.id)}
              <input
                aria-label={entry.label + "内容"}
                type="password"
                bind:value={entry.value}
                autocomplete="off"
                disabled={busy}
              />
            {:else}<textarea
                aria-label={entry.label + "内容"}
                rows={entry.value.includes("\n") ? 4 : fromHistory ? 1 : 2}
                bind:value={entry.value}
                disabled={busy}
              ></textarea>{/if}
            {#if !fromHistory || entry.totp || entry.masked}<div class="field-options">
                {#if !entry.totp}<label class="check"
                    ><input type="checkbox" bind:checked={entry.masked} />遮挡显示</label
                  >{/if}
                {#if entry.masked || entry.totp}<button
                    class="icon"
                    aria-label={reveal.has(entry.id) ? "隐藏内容" : "显示内容"}
                    on:click={() => toggle(entry)}
                    >{#if reveal.has(entry.id)}<EyeSlash size={18} />{:else}<Eye
                        size={18}
                      />{/if}</button
                  >{/if}
                <span class="spacer"></span><button
                  class="icon"
                  aria-label="上移"
                  disabled={index === 0 || busy}
                  on:click={() => move(index, index - 1)}><ArrowUp size={16} /></button
                ><button
                  class="icon"
                  aria-label="下移"
                  disabled={index === draft.items.length - 1 || busy}
                  on:click={() => move(index, index + 1)}><ArrowDown size={16} /></button
                >
              </div>{/if}
          </div>
        </section>
      {/each}
    </div>
    <button
      class="collection-add"
      disabled={busy || draft.items.length >= 64}
      on:click={() => (draft = { ...draft, items: [...draft.items, item()] })}
      ><Plus size={20} />添加内容项</button
    >
    {#if fromHistory}<p class="muted">每条记录保存为一个内容项，可修改名称和顺序。</p>{/if}
    <p class="collection-sync-note">
      <ArrowsClockwise size={18} />跟随剪贴板同步设置
      {#if onSettings}<button class="text-link" on:click={onSettings}>查看设置</button>{/if}
    </p>
  </div>
  <footer class="collection-editor-footer">
    <button on:click={close} disabled={busy}>取消</button><button
      class="primary"
      disabled={busy || stale || !draft.name.trim() || draft.items.some((i) => !i.label.trim())}
      on:click={() => onSave(draft)}>{busy ? "正在保存…" : "保存集合"}</button
    >
  </footer>
</div>
