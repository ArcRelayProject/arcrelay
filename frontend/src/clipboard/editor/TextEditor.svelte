<script lang="ts">
  import AppSelect from "../../components/AppSelect.svelte";
  import { onMount } from "svelte";
  import { EditorState, Compartment, Transaction } from "@codemirror/state";
  import {
    EditorView,
    keymap,
    lineNumbers,
    highlightActiveLine,
    drawSelection,
  } from "@codemirror/view";
  import { history, historyKeymap, defaultKeymap, undo, redo } from "@codemirror/commands";
  import { markdown } from "@codemirror/lang-markdown";
  import { syntaxHighlighting, defaultHighlightStyle } from "@codemirror/language";
  import { searchKeymap, highlightSelectionMatches, openSearchPanel } from "@codemirror/search";
  import { MergeView, goToNextChunk, goToPreviousChunk } from "@codemirror/merge";
  import {
    ArrowCounterClockwise,
    ArrowClockwise,
    MagnifyingGlass,
    TextB,
    TextItalic,
    LinkSimple,
    ListBullets,
    Code,
    CaretUp,
    CaretDown,
  } from "phosphor-svelte";
  import { editorBridge } from "./bridge";
  import { whitespaceDiff } from "./textDiff";
  export let original = "";
  export let readonly = false;
  export let onchange: (text: string) => void;
  let mode = "edit",
    format: "text" | "markdown" = /^#{1,6}\s|^[-*] |```/m.test(original) ? "markdown" : "text";
  let host: HTMLDivElement, previewHost: HTMLDivElement;
  let merge: MergeView | undefined;
  let text = original,
    html: string | null = null,
    renderLimited = false,
    previewError = "";
  let wrap = true,
    syncScroll = true,
    ignoreWhitespace = false,
    chunks = 0,
    ratio = 50;
  let previewGeneration = 0;
  let appliedWhitespaceMode = false;
  const editable = new Compartment(),
    wrapping = new Compartment(),
    syntax = new Compartment();
  let previewTimer: ReturnType<typeof setTimeout>;
  const theme = EditorView.theme({
    "&": { fontSize: "14px", height: "100%" },
    ".cm-content": {
      fontFamily: "ui-monospace, SFMono-Regular, Consolas, monospace",
      padding: "20px 0",
    },
    ".cm-line": { padding: "0 16px", lineHeight: "1.9" },
    ".cm-gutters": {
      background: "var(--surface-soft)",
      color: "var(--text-muted)",
      borderRight: "none",
    },
    ".cm-scroller": { overflow: "auto" },
    "&.cm-focused": { outline: "none" },
  });
  async function updatePreview() {
    const generation = ++previewGeneration;
    try {
      const result = await editorBridge.preview(text, format);
      if (generation !== previewGeneration) return;
      html = result.safeHtml;
      renderLimited = result.renderLimited;
      previewError = "";
    } catch (error) {
      if (generation === previewGeneration) previewError = String(error);
    }
  }
  function schedulePreview() {
    clearTimeout(previewTimer);
    previewGeneration++;
    previewTimer = setTimeout(() => {
      void updatePreview();
    }, 180);
  }
  onMount(() => {
    merge = new MergeView({
      parent: host,
      a: {
        doc: original,
        extensions: [
          theme,
          lineNumbers(),
          EditorState.lineSeparator.of(original.includes("\r\n") ? "\r\n" : "\n"),
          EditorState.readOnly.of(true),
          EditorView.editable.of(false),
          EditorView.lineWrapping,
        ],
      },
      b: {
        doc: original,
        extensions: [
          theme,
          lineNumbers(),
          highlightActiveLine(),
          drawSelection(),
          history(),
          keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap]),
          highlightSelectionMatches(),
          editable.of([]),
          wrapping.of(EditorView.lineWrapping),
          syntax.of(
            format === "markdown" ? [markdown(), syntaxHighlighting(defaultHighlightStyle)] : [],
          ),
          EditorState.lineSeparator.of(original.includes("\r\n") ? "\r\n" : "\n"),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              text = update.state.sliceDoc();
              onchange(text);
              schedulePreview();
            }
            queueMicrotask(() => {
              if (merge) chunks = merge.chunks.length;
            });
          }),
        ],
      },
      revertControls: "a-to-b",
      renderRevertControl: () => {
        const b = document.createElement("button");
        b.textContent = "↶";
        b.title = "恢复此处原文（可撤销）";
        b.setAttribute("aria-label", b.title);
        return b;
      },
      diffConfig: { scanLimit: 1000, timeout: 50 },
    });
    const scroll = () => {
      if (syncScroll && previewHost && merge && mode === "split") {
        const scroller = merge.b.scrollDOM;
        previewHost.scrollTop =
          (scroller.scrollTop / Math.max(1, scroller.scrollHeight - scroller.clientHeight)) *
          (previewHost.scrollHeight - previewHost.clientHeight);
      }
    };
    merge.b.scrollDOM.addEventListener("scroll", scroll);
    void updatePreview();
    return () => {
      clearTimeout(previewTimer);
      previewGeneration++;
      merge?.destroy();
      merge = undefined;
    };
  });
  $: if (merge) {
    merge.b.dispatch({
      effects: editable.reconfigure([
        EditorState.readOnly.of(readonly),
        EditorView.editable.of(!readonly),
      ]),
    });
  }
  $: if (merge)
    merge.b.dispatch({ effects: wrapping.reconfigure(wrap ? EditorView.lineWrapping : []) });
  $: if (merge) {
    merge.b.dispatch({
      effects: syntax.reconfigure(
        format === "markdown" ? [markdown(), syntaxHighlighting(defaultHighlightStyle)] : [],
      ),
    });
    schedulePreview();
  }
  $: if (merge) {
    merge.reconfigure({
      highlightChanges: mode === "diff",
      gutter: mode === "diff",
      revertControls: mode === "diff" && !readonly ? "a-to-b" : undefined,
      diffConfig: {
        scanLimit: 1000,
        timeout: 50,
        override: ignoreWhitespace ? whitespaceDiff : undefined,
      },
    });
    if (appliedWhitespaceMode !== ignoreWhitespace) {
      appliedWhitespaceMode = ignoreWhitespace;
      // MergeView applies a new diff configuration on its next document transaction.
      // Rescan the identical immutable baseline; never touch the draft or its undo stack.
      const doc = merge.a.state.doc;
      merge.a.dispatch({
        changes: { from: 0, to: doc.length, insert: doc },
        annotations: Transaction.addToHistory.of(false),
      });
    }
    chunks = merge.chunks.length;
    merge.a.requestMeasure();
    merge.b.requestMeasure();
  }
  function insert(before: string, after = "") {
    if (!merge || readonly) return;
    const view = merge.b;
    const selection = view.state.selection.main;
    view.dispatch({
      changes: {
        from: selection.from,
        to: selection.to,
        insert: before + view.state.sliceDoc(selection.from, selection.to) + after,
      },
      selection: { anchor: selection.from + before.length },
      userEvent: "input",
    });
    view.focus();
  }
  function resize(event: PointerEvent) {
    const bar = event.currentTarget as HTMLElement;
    bar.setPointerCapture(event.pointerId);
    const parent = bar.parentElement!;
    const move = (e: PointerEvent) => {
      const rect = parent.getBoundingClientRect();
      ratio = Math.max(25, Math.min(75, ((e.clientX - rect.left) / rect.width) * 100));
      merge?.b.requestMeasure();
    };
    const end = () => {
      bar.removeEventListener("pointermove", move);
      bar.removeEventListener("pointerup", end);
    };
    bar.addEventListener("pointermove", move);
    bar.addEventListener("pointerup", end);
  }
</script>

<div class="editor-toolbar">
  <nav class="view-tabs" aria-label="文本视图">
    {#each [["edit", "编辑"], ["preview", "预览"], ["split", "左右对照"], ["diff", "修改对比"]] as [value, label]}<button
        class:active={mode === value}
        onclick={() => (mode = value)}>{label}</button
      >{/each}
  </nav>
  <div class="toolbar-end">
    {#if readonly}<span class="muted">{format === "markdown" ? "Markdown" : "纯文本"} · 只读</span
      >{:else}<AppSelect
        aria-label="文本格式"
        bind:value={format}
        options={[
          { value: "text", label: "纯文本" },
          { value: "markdown", label: "Markdown" },
        ]}
      />{/if}
  </div>
</div>
<div class="editor-subtoolbar">
  {#if !readonly && mode !== "preview"}<button
      class="icon-button"
      title="撤销 ⌘Z"
      onclick={() => merge && undo(merge.b)}><ArrowCounterClockwise size={18} /></button
    ><button class="icon-button" title="重做 ⇧⌘Z" onclick={() => merge && redo(merge.b)}
      ><ArrowClockwise size={18} /></button
    ><span class="tool-divider"></span>{#if format === "markdown"}<button
        class="icon-button"
        title="粗体"
        onclick={() => insert("**", "**")}><TextB size={18} /></button
      ><button class="icon-button" title="斜体" onclick={() => insert("*", "*")}
        ><TextItalic size={18} /></button
      ><button class="icon-button" title="链接" onclick={() => insert("[", "](https://)")}
        ><LinkSimple size={18} /></button
      ><button class="icon-button" title="列表" onclick={() => insert("- ")}
        ><ListBullets size={18} /></button
      ><button class="icon-button" title="代码" onclick={() => insert("`", "`")}
        ><Code size={18} /></button
      >{/if}{/if}
  {#if mode !== "preview"}<button
      class="quiet-button"
      onclick={() => merge && openSearchPanel(merge.b)}
      ><MagnifyingGlass size={16} />查找{readonly ? "" : "与替换"}</button
    ><label class="check-label"><input type="checkbox" bind:checked={wrap} />自动换行</label>{/if}
  {#if mode === "split"}<label class="check-label"
      ><input type="checkbox" bind:checked={syncScroll} />同步滚动</label
    >{/if}
  {#if mode === "diff"}<label class="check-label"
      ><input type="checkbox" bind:checked={ignoreWhitespace} />忽略空白</label
    ><span class="change-count">{chunks} 处修改</span><button
      class="icon-button"
      title="上一处修改"
      onclick={() => merge && goToPreviousChunk(merge.b)}><CaretUp size={16} /></button
    ><button class="icon-button" title="下一处修改" onclick={() => merge && goToNextChunk(merge.b)}
      ><CaretDown size={16} /></button
    >{/if}
</div>
<div
  class="text-workspace"
  class:diff-mode={mode === "diff"}
  class:preview-mode={mode === "preview"}
  class:split-mode={mode === "split"}
  style={`--split-ratio:${ratio}%`}
>
  <div class="source-pane">
    <div class="pane-heading">
      {#if mode === "diff"}<span>原始内容 <small>只读</small></span><span>当前草稿</span
        >{:else}<span>{readonly ? "源文本 · 只读" : "源文本"}</span><small
          >编辑结果保存为新条目</small
        >{/if}
    </div>
    <div bind:this={host} class="code-host"></div>
  </div>
  {#if mode === "split"}<div
      class="pane-resizer"
      role="slider"
      aria-valuemin="25"
      aria-valuemax="75"
      aria-label="调整两栏宽度"
      aria-orientation="vertical"
      aria-valuenow={ratio}
      tabindex="0"
      onpointerdown={resize}
      onkeydown={(e) => {
        if (e.key === "ArrowLeft") ratio = Math.max(25, ratio - 5);
        if (e.key === "ArrowRight") ratio = Math.min(75, ratio + 5);
      }}
    ></div>{/if}
  {#if mode === "preview" || mode === "split"}<div class="preview-pane">
      <div class="pane-heading">
        {format === "markdown" ? "Markdown 预览" : "纯文本预览"}<small>复制时仍使用源文本</small>
      </div>
      <div class="rendered-preview" bind:this={previewHost}>
        {#if previewError}<p class="error-message">{previewError}</p>{:else if renderLimited}<p
            class="notice"
          >
            内容较长，仅显示源文本。
          </p>
          <pre>{text}</pre>{:else if html}<div class="markdown-body">
            {@html html}
          </div>{:else}<pre>{text}</pre>{/if}
      </div>
    </div>{/if}
</div>
<div class="editor-status">
  <span>{text.length.toLocaleString()} 个字符 · {text.split("\n").length} 行</span><span
    >{mode === "diff"
      ? "绿色为新增，红色为删除 · 恢复操作可撤销"
      : "UTF-8 · 草稿尚未写入系统剪贴板"}</span
  >
</div>
