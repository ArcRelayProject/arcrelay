<script lang="ts">
  import appIcon from "../../../../icons/128x128.png";
  import { loadAppSettings, onAppSettingsChanged } from "../i18n";
  import { onMount } from "svelte";
  import {
    PencilSimple,
    Copy,
    CheckCircle,
    WarningCircle,
    FloppyDisk,
    SpinnerGap,
  } from "phosphor-svelte";
  import { modal } from "./modal";
  import TextEditor from "./TextEditor.svelte";
  import ImageEditor from "./ImageEditor.svelte";
  import { editorBridge, native } from "./bridge";
  import type { ClipboardEditorSnapshot, ClipboardEditorDraft } from "../../ipc/generated";
  let snapshot: ClipboardEditorSnapshot | null = null;
  let imageEditor: ImageEditor;
  let text = "",
    dirty = false,
    phase: "loading" | "editing" | "saving" | "copying" | "saveFailed" | "copyFailed" | "saved" =
      "loading";
  let error = "",
    closeConfirm = false,
    accessLocked = false,
    intentCopy = false,
    savedId: number | null = null;
  $: busy = phase === "saving" || phase === "copying";
  $: committed = savedId !== null;
  $: frozen = busy || committed || accessLocked;
  async function load() {
    error = "";
    phase = "loading";
    try {
      snapshot = await editorBridge.snapshot();
      text = snapshot.text ?? "";
      phase = "editing";
    } catch (e) {
      error = String(e);
    }
  }
  function requestClose() {
    if (busy) return;
    if (dirty && !committed) closeConfirm = true;
    else void finish();
  }
  async function finish() {
    try {
      await editorBridge.close();
    } catch (e) {
      error = String(e);
    }
  }
  async function save(copy: boolean) {
    if (busy || committed || accessLocked || !snapshot) return;
    intentCopy = copy;
    closeConfirm = false;
    error = "";
    phase = "saving";
    try {
      // Materialize exactly once before freezing. Save/copy retries use the committed session.
      const draft: ClipboardEditorDraft =
        snapshot.kind === "image"
          ? { kind: "image", png: await imageEditor.getDraft() }
          : { kind: "text", content: text };
      phase = "saving";
      savedId = await editorBridge.save(draft);
      dirty = false;
    } catch (e) {
      phase = "saveFailed";
      error = String(e);
      return;
    }
    if (copy) await retryCopy();
    else {
      phase = "saved";
      await finish();
    }
  }
  async function retryCopy() {
    if (savedId === null || (busy && phase !== "saving")) return;
    phase = "copying";
    error = "";
    try {
      await editorBridge.copy();
      phase = "saved";
      await finish();
    } catch (e) {
      phase = "copyFailed";
      error = String(e);
    }
  }
  function keyboard(e: KeyboardEvent) {
    if (e.isComposing) return;
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "w") {
      e.preventDefault();
      requestClose();
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      void save(true);
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      if (!busy && !committed) void save(e.shiftKey);
    }
    if (e.key === "Escape" && closeConfirm) {
      e.preventDefault();
      closeConfirm = false;
    }
  }
  onMount(() => {
    void load();
    let settingsCleanup: (() => void) | undefined;
    const scheme = matchMedia("(prefers-color-scheme: dark)");
    let preference = "light";
    const applyTheme = () => {
      document.documentElement.dataset.theme =
        preference === "system" ? (scheme.matches ? "dark" : "light") : preference;
    };
    const updateSettings = (settings: { theme: string }) => {
      preference = settings.theme;
      applyTheme();
    };
    if (native()) {
      void loadAppSettings()
        .then(updateSettings)
        .catch(() => {});
      void onAppSettingsChanged(updateSettings).then((unlisten) => {
        if (disposed) unlisten();
        else settingsCleanup = unlisten;
      });
    }
    scheme.addEventListener("change", applyTheme);
    let cleanup: (() => void) | undefined;
    let disposed = false;
    if (native())
      void import("@tauri-apps/api/window").then(async ({ getCurrentWindow }) => {
        const unlisten = await getCurrentWindow().onCloseRequested((event) => {
          event.preventDefault();
          requestClose();
        });
        if (disposed) unlisten();
        else cleanup = unlisten;
      });
    const check = async () => {
      try {
        await editorBridge.access();
        accessLocked = false;
      } catch {
        accessLocked = true;
      }
    };
    const timer = setInterval(() => {
      void check();
    }, 1000);
    return () => {
      disposed = true;
      cleanup?.();
      settingsCleanup?.();
      scheme.removeEventListener("change", applyTheme);
      clearInterval(timer);
    };
  });
</script>

<svelte:window onkeydown={keyboard} />
<main class="editor-shell" aria-busy={busy}>
  <header class="editor-header">
    <img class="editor-title-icon" src={appIcon} alt="ArcRelay" />
    <div>
      <h1>{snapshot?.kind === "image" ? "编辑图片" : "编辑文本"}</h1>
      <p>剪贴板工作台 <span>·</span> 原记录 #{snapshot?.sourceId ?? "—"}</p>
    </div>
    <div class="header-status" class:saved={committed}>
      {#if committed}<CheckCircle size={16} />已保存{:else}<span class="status-dot"></span>{dirty
          ? "有未保存的修改"
          : "独立草稿"}{/if}
    </div>
  </header>
  {#if phase === "loading"}<div class="loading-state">
      {#if error}<WarningCircle size={32} />
        <p role="alert">{error}</p>
        <button class="primary-button" onclick={load}>重新加载</button>{:else}<SpinnerGap
          size={30}
          class="spinning"
        />
        <p>正在加载原始内容…</p>{/if}
    </div>
  {:else if snapshot}
    {#if snapshot.plainCopy}<div class="notice">
        正在编辑纯文本副本。富文本样式、链接格式和嵌入图片不会保留，原记录保持不变。
      </div>{/if}
    {#if phase === "saveFailed"}<div class="error-banner" role="alert">
        <WarningCircle size={22} />
        <div>
          <strong>保存失败，草稿仍然保留</strong>
          <p>{error}</p>
          <small>重试将继续{intentCopy ? "保存并复制" : "仅保存"}，不会覆盖原记录。</small>
        </div>
      </div>{/if}
    {#if phase === "copyFailed"}<div class="warning-banner" role="alert">
        <WarningCircle size={22} />
        <div>
          <strong>已保存为新条目，但未能复制</strong>
          <p>{error}</p>
          <small>当前是只读快照。重试复制不会再次保存，也不会生成重复条目。</small>
        </div>
      </div>{/if}
    <div class="editor-content" class:access-locked={accessLocked} inert={accessLocked}>
      {#if snapshot.kind === "image"}<ImageEditor
          bind:this={imageEditor}
          source={snapshot.image!}
          width={snapshot.width!}
          height={snapshot.height!}
          readonly={frozen}
          onchange={(value) => {
            dirty = value;
            if (phase === "saveFailed") phase = "editing";
          }}
        />{:else}<TextEditor
          original={snapshot.text ?? ""}
          readonly={frozen}
          onchange={(value) => {
            text = value;
            dirty = text !== snapshot?.text;
            if (phase === "saveFailed") phase = "editing";
          }}
        />{/if}
    </div>
    {#if accessLocked}<div class="access-overlay" role="alert">
        <WarningCircle size={32} />
        <h2>剪贴板已锁定</h2>
        <p>确认本机用户身份后，可继续编辑当前草稿。</p>
      </div>{/if}
    <footer class="editor-footer">
      <div class="save-explanation">
        {#if busy}<SpinnerGap size={18} class="spinning" /><span
            >{phase === "saving" ? "正在保存为新条目…" : "已保存，正在复制…"}</span
          >{:else if committed}<CheckCircle size={18} /><span
            >新条目 #{savedId} 已保存，原记录保留</span
          >{:else}<FloppyDisk size={18} /><span>保存为新条目，保留原记录</span>{/if}
      </div>
      <div class="footer-actions">
        {#if committed}<button class="secondary-button" disabled={busy} onclick={finish}
            >关闭</button
          >{#if phase === "copyFailed"}<button
              class="primary-button"
              disabled={accessLocked}
              onclick={retryCopy}><Copy size={17} />重试复制</button
            >{/if}{:else}<button class="quiet-button" disabled={busy} onclick={requestClose}
            >取消</button
          >{#if phase === "saveFailed"}<button
              class="primary-button"
              disabled={accessLocked}
              onclick={() => save(intentCopy)}>{intentCopy ? "重试保存并复制" : "重试保存"}</button
            >{:else}<button
              class="secondary-button"
              disabled={busy || accessLocked || (snapshot.kind === "text" && !text.length)}
              onclick={() => save(false)}>仅保存</button
            ><button
              class="primary-button"
              disabled={busy || accessLocked || (snapshot.kind === "text" && !text.length)}
              onclick={() => save(true)}><Copy size={17} />{busy ? "处理中…" : "保存并复制"}</button
            >{/if}{/if}
      </div>
    </footer>
  {/if}
</main>
{#if closeConfirm}<dialog
    class="editor-dialog"
    use:modal
    aria-labelledby="close-title"
    oncancel={() => (closeConfirm = false)}
  >
    <div class="dialog-icon"><PencilSimple size={26} /></div>
    <h2 id="close-title">保存这次修改？</h2>
    <p>当前草稿尚未保存。保存后会新增一条记录，原记录仍会保留。</p>
    <div class="dialog-actions">
      <button class="secondary-button" onclick={() => (closeConfirm = false)}>继续编辑</button
      ><button class="danger-button" onclick={finish}>放弃修改</button><button
        class="primary-button"
        onclick={() => save(false)}>保存并关闭</button
      >
    </div>
  </dialog>{/if}
