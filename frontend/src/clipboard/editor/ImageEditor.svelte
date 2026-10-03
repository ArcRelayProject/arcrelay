<script lang="ts">
  import AppSelect from "../../components/AppSelect.svelte";
  import { onMount } from "svelte";
  import { modal } from "./modal";
  import Konva from "konva";
  import {
    Cursor,
    Hand,
    Crop,
    GridFour,
    Square,
    Circle,
    ArrowUpRight,
    LineSegment,
    PencilSimple,
    TextT,
    Eye,
    ArrowCounterClockwise,
    ArrowClockwise,
    MagnifyingGlassMinus,
    MagnifyingGlassPlus,
    CornersOut,
    Trash,
    FlipHorizontal,
    FlipVertical,
    ArrowBendUpRight,
  } from "phosphor-svelte";
  import {
    ImageHistory,
    boundedRect,
    type ImageDocument,
    type Mark,
    type Tool,
  } from "./imageDocument";
  export let source: string;
  export let width: number;
  export let height: number;
  export let readonly = false;
  export let onchange: (dirty: boolean) => void;
  let host: HTMLDivElement;
  let stage: Konva.Stage,
    scene: Konva.Group,
    layer: Konva.Layer,
    overlay: Konva.Layer,
    transformer: Konva.Transformer,
    cropNode: Konva.Rect;
  let document: ImageDocument = { base: source, width, height, marks: [] };
  const initial = structuredClone(document),
    history = new ImageHistory(document);
  let tool: Tool = "select",
    color = "#ef5252",
    stroke = 4,
    fontSize = 28,
    fontFamily = "sans-serif",
    text = "标注文字",
    background = "transparent",
    pixelSize = 16;
  let selected = "",
    zoom = 1,
    fitMode = true,
    ratio = "0",
    showingOriginal = false,
    canUndo = false,
    canRedo = false,
    ready = false,
    busy = false,
    error = "",
    showReset = false;
  let crop: { x: number; y: number; width: number; height: number } | null = null;
  let draft: Mark | null = null,
    drawing: Konva.Shape | Konva.Group | null = null,
    start = { x: 0, y: 0 },
    panStart: { x: number; y: number; left: number; top: number } | null = null;
  let generation = 0;
  const images = new Map<string, HTMLImageElement>();
  const tools: { id: Tool; label: string; icon: typeof Cursor }[] = [
    { id: "select", label: "选择 / 移动", icon: Cursor },
    { id: "pan", label: "拖动画布", icon: Hand },
    { id: "crop", label: "裁剪", icon: Crop },
    { id: "mosaic", label: "马赛克", icon: GridFour },
    { id: "cover", label: "实色遮挡", icon: Square },
    { id: "brush", label: "画笔", icon: PencilSimple },
    { id: "rect", label: "矩形", icon: Square },
    { id: "ellipse", label: "椭圆", icon: Circle },
    { id: "arrow", label: "箭头", icon: ArrowUpRight },
    { id: "line", label: "直线", icon: LineSegment },
    { id: "text", label: "文字", icon: TextT },
  ];
  function loadImage(src: string): Promise<HTMLImageElement> {
    const cached = images.get(src);
    if (cached) return Promise.resolve(cached);
    return new Promise((resolve, reject) => {
      const image = new Image();
      image.onload = () => {
        images.set(src, image);
        resolve(image);
      };
      image.onerror = () => reject(new Error("Unable to load the image"));
      image.src = src;
    });
  }
  async function markNode(mark: Mark): Promise<Konva.Shape | Konva.Group> {
    const attrs = {
      ...mark,
      id: mark.id,
      draggable: tool === "select" && !readonly,
      stroke: mark.color,
      strokeWidth: mark.stroke,
      fill: undefined as string | undefined,
    };
    let node: Konva.Shape | Konva.Group;
    if (mark.type === "mosaic")
      node = new Konva.Image({ ...attrs, strokeWidth: 0, image: await loadImage(mark.src!) });
    else if (mark.type === "text") {
      const group = new Konva.Group({ ...attrs });
      const label = new Konva.Text({
        text: mark.text,
        fontSize: mark.fontSize,
        fontFamily: mark.fontFamily,
        fill: mark.color,
        padding: 8,
      });
      group.add(
        new Konva.Rect({
          width: label.width(),
          height: label.height(),
          fill: mark.background,
          strokeWidth: 0,
        }),
        label,
      );
      node = group;
    } else if (mark.type === "ellipse")
      node = new Konva.Ellipse({
        ...attrs,
        x: mark.x + mark.width / 2,
        y: mark.y + mark.height / 2,
        radiusX: mark.width / 2,
        radiusY: mark.height / 2,
      });
    else if (mark.type === "arrow")
      node = new Konva.Arrow({
        ...attrs,
        fill: mark.color,
        pointerLength: mark.stroke * 3,
        pointerWidth: mark.stroke * 3,
        lineCap: "round",
        lineJoin: "round",
      });
    else if (mark.type === "brush" || mark.type === "line")
      node = new Konva.Line({ ...attrs, lineCap: "round", lineJoin: "round" });
    else
      node = new Konva.Rect({
        ...attrs,
        fill: mark.type === "cover" ? mark.color : undefined,
        strokeWidth: mark.type === "cover" ? 0 : mark.stroke,
      });
    (node as Konva.Node).on("click tap", () => {
      if (tool === "select" && !readonly) {
        selected = mark.id;
        if (mark.type === "text") {
          text = mark.text;
          fontSize = mark.fontSize;
          fontFamily = mark.fontFamily;
          background = mark.background;
          color = mark.color;
        }
        transformer.nodes([node]);
        overlay.batchDraw();
      }
    });
    (node as Konva.Node).on("dragend transformend", () => {
      const found = document.marks.find((m) => m.id === mark.id);
      if (!found) return;
      const x = node.x(),
        y = node.y();
      Object.assign(found, {
        x: mark.type === "ellipse" ? x - found.width / 2 : x,
        y: mark.type === "ellipse" ? y - found.height / 2 : y,
        scaleX: node.scaleX(),
        scaleY: node.scaleY(),
        rotation: node.rotation(),
      });
      commit();
    });
    return node;
  }
  async function render() {
    if (!stage) return;
    const id = ++generation;
    ready = false;
    try {
      const doc = showingOriginal ? initial : document;
      const base = await loadImage(doc.base);
      const nodes = await Promise.all(doc.marks.map(markNode));
      if (id !== generation) return;
      transformer.nodes([]);
      scene.destroyChildren();
      scene.add(
        new Konva.Image({ image: base, width: doc.width, height: doc.height, listening: false }),
      );
      nodes.forEach((node) => scene.add(node));
      scene.clip({ x: 0, y: 0, width: doc.width, height: doc.height });
      if (fitMode) fit();
      else redraw();
      ready = true;
    } catch (e) {
      error = String(e);
    }
  }
  function redraw() {
    if (!stage) return;
    scene.scale({ x: zoom, y: zoom });
    cropNode?.scale({ x: zoom, y: zoom });
    if (crop && cropNode)
      cropNode.setAttrs({
        x: scene.x() + crop.x * zoom,
        y: scene.y() + crop.y * zoom,
        width: crop.width,
        height: crop.height,
        visible: true,
      });
    stage.batchDraw();
  }
  function fit() {
    if (!stage) return;
    fitMode = true;
    const doc = showingOriginal ? initial : document;
    zoom = Math.min((stage.width() - 64) / doc.width, (stage.height() - 64) / doc.height, 1);
    scene.position({
      x: (stage.width() - doc.width * zoom) / 2,
      y: (stage.height() - doc.height * zoom) / 2,
    });
    redraw();
  }
  function setZoom(next: number) {
    fitMode = false;
    const old = zoom;
    zoom = Math.max(0.1, Math.min(4, next));
    scene.position({
      x: stage.width() / 2 - ((stage.width() / 2 - scene.x()) * zoom) / old,
      y: stage.height() / 2 - ((stage.height() / 2 - scene.y()) * zoom) / old,
    });
    redraw();
  }
  function commit() {
    history.push(document);
    const retained = new Set([
      source,
      ...history.states.flatMap((s) => [s.base, ...s.marks.map((m) => m.src ?? "")]),
    ]);
    for (const key of images.keys()) if (!retained.has(key)) images.delete(key);
    canUndo = history.canUndo;
    canRedo = history.canRedo;
    onchange(true);
    void render();
  }
  function travel(redo = false) {
    if (readonly || busy) return;
    document = redo ? history.redo() : history.undo();
    canUndo = history.canUndo;
    canRedo = history.canRedo;
    onchange(JSON.stringify(document) !== JSON.stringify(initial));
    crop = null;
    cropNode.hide();
    selected = "";
    void render();
  }
  function point() {
    const p = stage.getPointerPosition()!;
    return {
      x: Math.max(0, Math.min(document.width, (p.x - scene.x()) / zoom)),
      y: Math.max(0, Math.min(document.height, (p.y - scene.y()) / zoom)),
    };
  }
  function exportCanvas() {
    const oldScale = scene.scale(),
      oldPosition = scene.position();
    scene.scale({ x: 1, y: 1 });
    scene.position({ x: 0, y: 0 });
    try {
      return scene.toCanvas({
        x: 0,
        y: 0,
        width: document.width,
        height: document.height,
        pixelRatio: 1,
      });
    } finally {
      scene.scale(oldScale);
      scene.position(oldPosition);
    }
  }
  export async function getDraft() {
    if (!ready || busy || draft || showingOriginal)
      throw new Error("Finish the current image operation before saving");
    if (crop) throw new Error("Apply or cancel the crop before saving");
    return exportCanvas().toDataURL("image/png").split(",")[1];
  }
  async function flatten(kind: "crop" | "rotate" | "flipH" | "flipV") {
    if (readonly || !ready || busy) return;
    busy = true;
    error = "";
    try {
      const old = exportCanvas(),
        canvas = window.document.createElement("canvas");
      const area = kind === "crop" ? crop : null;
      canvas.width = area?.width ?? (kind === "rotate" ? document.height : document.width);
      canvas.height = area?.height ?? (kind === "rotate" ? document.width : document.height);
      const ctx = canvas.getContext("2d")!;
      if (area)
        ctx.drawImage(old, area.x, area.y, area.width, area.height, 0, 0, area.width, area.height);
      else {
        if (kind === "rotate") {
          ctx.translate(canvas.width, 0);
          ctx.rotate(Math.PI / 2);
        }
        if (kind === "flipH") {
          ctx.translate(canvas.width, 0);
          ctx.scale(-1, 1);
        }
        if (kind === "flipV") {
          ctx.translate(0, canvas.height);
          ctx.scale(1, -1);
        }
        ctx.drawImage(old, 0, 0);
      }
      document = {
        base: canvas.toDataURL("image/png"),
        width: canvas.width,
        height: canvas.height,
        marks: [],
      };
      crop = null;
      cropNode.hide();
      selected = "";
      fitMode = true;
      commit();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
  async function down() {
    if (!ready || busy || showingOriginal) return;
    const p = stage.getPointerPosition()!;
    if (tool === "pan") {
      panStart = { x: p.x, y: p.y, left: scene.x(), top: scene.y() };
      return;
    }
    if (readonly || tool === "select") return;
    start = point();
    transformer.nodes([]);
    selected = "";
    if (tool === "crop") {
      crop = null;
      return;
    }
    if (tool === "mosaic") {
      draft = {
        id: crypto.randomUUID(),
        type: tool,
        x: start.x,
        y: start.y,
        width: 1,
        height: 1,
        points: [],
        color,
        stroke,
        text,
        fontSize,
        fontFamily,
        background,
      };
      drawing = new Konva.Rect({
        x: start.x,
        y: start.y,
        width: 1,
        height: 1,
        stroke: "#5b5ff0",
        strokeWidth: 2 / zoom,
        fill: "#5b5ff030",
      });
      scene.add(drawing);
      return;
    }
    draft = {
      id: crypto.randomUUID(),
      type: tool,
      x: start.x,
      y: start.y,
      width: 1,
      height: 1,
      points: [0, 0, 0, 0],
      color: tool === "cover" ? color : color,
      stroke,
      text,
      fontSize,
      fontFamily,
      background,
    };
    drawing = await markNode(draft);
    scene.add(drawing);
    if (tool === "text") {
      document.marks.push(draft);
      draft = null;
      drawing = null;
      commit();
    }
  }
  function move() {
    if (panStart) {
      const p = stage.getPointerPosition()!;
      scene.position({ x: panStart.left + p.x - panStart.x, y: panStart.top + p.y - panStart.y });
      fitMode = false;
      redraw();
      return;
    }
    if (readonly || !stage.getPointerPosition()) return;
    const p = point();
    if (tool === "crop" && stage.getAttr("pointerDown")) {
      crop = boundedRect(start, p, document.width, document.height, Number(ratio));
      redraw();
      return;
    }
    if (!draft || !drawing) return;
    if (tool === "brush") {
      draft.points.push(p.x - start.x, p.y - start.y);
      (drawing as Konva.Node).setAttr("points", draft.points);
    } else if (tool === "arrow" || tool === "line") {
      draft.points = [0, 0, p.x - start.x, p.y - start.y];
      (drawing as Konva.Node).setAttr("points", draft.points);
    } else {
      Object.assign(draft, boundedRect(start, p, document.width, document.height));
      drawing.setAttrs({ x: draft.x, y: draft.y, width: draft.width, height: draft.height });
      if (tool === "ellipse")
        drawing.setAttrs({
          x: draft.x + draft.width / 2,
          y: draft.y + draft.height / 2,
          radiusX: draft.width / 2,
          radiusY: draft.height / 2,
        });
    }
    layer.batchDraw();
  }
  async function up() {
    stage.setAttr("pointerDown", false);
    panStart = null;
    if (!draft) return;
    const mark = draft;
    draft = null;
    drawing?.destroy();
    drawing = null;
    if (mark.type === "mosaic") {
      const sourceCanvas = exportCanvas(),
        tiny = window.document.createElement("canvas"),
        patch = window.document.createElement("canvas");
      tiny.width = Math.max(1, Math.ceil(mark.width / pixelSize));
      tiny.height = Math.max(1, Math.ceil(mark.height / pixelSize));
      tiny
        .getContext("2d")!
        .drawImage(
          sourceCanvas,
          mark.x,
          mark.y,
          mark.width,
          mark.height,
          0,
          0,
          tiny.width,
          tiny.height,
        );
      patch.width = mark.width;
      patch.height = mark.height;
      const ctx = patch.getContext("2d")!;
      ctx.imageSmoothingEnabled = false;
      ctx.drawImage(tiny, 0, 0, patch.width, patch.height);
      mark.src = patch.toDataURL("image/png");
    }
    document.marks.push(mark);
    commit();
  }
  function removeSelected() {
    if (readonly || !selected) return;
    document.marks = document.marks.filter((m) => m.id !== selected);
    selected = "";
    commit();
  }
  function updateText() {
    const mark = document.marks.find((m) => m.id === selected);
    if (!mark || mark.type !== "text" || readonly) return;
    Object.assign(mark, { text, fontSize, fontFamily, background, color });
    commit();
  }
  function keydown(e: KeyboardEvent) {
    if ((e.target as HTMLElement).closest("input,textarea,select")) return;
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "z") {
      e.preventDefault();
      travel(e.shiftKey);
    }
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      removeSelected();
    }
    if (e.key === "Escape") {
      crop = null;
      cropNode?.hide();
      transformer?.nodes([]);
      selected = "";
      redraw();
    }
  }
  onMount(() => {
    stage = new Konva.Stage({
      container: host,
      width: host.clientWidth,
      height: host.clientHeight,
    });
    layer = new Konva.Layer();
    overlay = new Konva.Layer();
    scene = new Konva.Group();
    layer.add(scene);
    stage.add(layer, overlay);
    transformer = new Konva.Transformer({
      rotateEnabled: false,
      flipEnabled: false,
      borderStroke: "#5b5ff0",
      anchorStroke: "#5b5ff0",
      anchorSize: 8,
    });
    cropNode = new Konva.Rect({
      stroke: "#5b5ff0",
      strokeWidth: 2,
      dash: [8, 5],
      fill: "#5b5ff014",
      listening: false,
      visible: false,
    });
    overlay.add(transformer, cropNode);
    stage.on("pointerdown", () => {
      stage.setAttr("pointerDown", true);
      void down();
    });
    stage.on("pointermove", move);
    stage.on("pointerup pointercancel", () => {
      void up();
    });
    const releaseOutside = (event: PointerEvent) => {
      if (!stage.getAttr("pointerDown")) return;
      stage.setPointersPositions(event);
      move();
      void up();
    };
    window.addEventListener("pointerup", releaseOutside);
    window.addEventListener("pointercancel", releaseOutside);
    stage.on("wheel", (e) => {
      e.evt.preventDefault();
      setZoom(zoom * (e.evt.deltaY > 0 ? 0.9 : 1.1));
    });
    const observer = new ResizeObserver(() => {
      stage.size({ width: host.clientWidth, height: host.clientHeight });
      if (fitMode) fit();
      else redraw();
    });
    observer.observe(host);
    void render();
    return () => {
      generation++;
      window.removeEventListener("pointerup", releaseOutside);
      window.removeEventListener("pointercancel", releaseOutside);
      observer.disconnect();
      stage.destroy();
      images.clear();
    };
  });
  $: if (stage) {
    tool;
    readonly;
    showingOriginal;
    selected = "";
    void render();
  }
</script>

<svelte:window onkeydown={keydown} />
<div class="editor-toolbar image-actions">
  {#if !readonly}<button class="quiet-button" disabled={!canUndo || busy} onclick={() => travel()}
      ><ArrowCounterClockwise size={18} />撤销</button
    ><button class="quiet-button" disabled={!canRedo || busy} onclick={() => travel(true)}
      ><ArrowClockwise size={18} />重做</button
    ><span class="tool-divider"></span><button
      class="quiet-button"
      disabled={busy || !ready}
      onclick={() => flatten("rotate")}><ArrowBendUpRight size={18} />旋转</button
    ><button
      class="icon-button"
      title="水平翻转"
      disabled={busy || !ready}
      onclick={() => flatten("flipH")}><FlipHorizontal size={18} /></button
    ><button
      class="icon-button"
      title="垂直翻转"
      disabled={busy || !ready}
      onclick={() => flatten("flipV")}><FlipVertical size={18} /></button
    ><button class="quiet-button" onclick={() => (showReset = true)}>重置</button>{/if}
  <div class="toolbar-end">
    <button
      class="quiet-button"
      onpointerdown={(e) => {
        (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
        showingOriginal = true;
      }}
      onpointerup={() => (showingOriginal = false)}
      onpointercancel={() => (showingOriginal = false)}
      onkeydown={(e) => {
        if (e.key === " " || e.key === "Enter") showingOriginal = true;
      }}
      onkeyup={() => (showingOriginal = false)}
      onblur={() => (showingOriginal = false)}><Eye size={18} />按住查看原图</button
    >
  </div>
</div>
<div class="image-workspace">
  {#if !readonly}<nav class="image-tools" aria-label="图片编辑工具">
      {#each tools as entry}<button
          title={entry.label}
          aria-label={entry.label}
          class:active={tool === entry.id}
          disabled={busy}
          onclick={() => {
            tool = entry.id;
            crop = null;
            cropNode?.hide();
          }}
          ><svelte:component this={entry.icon} size={23} /><small
            >{entry.label.split(" /")[0]}</small
          ></button
        >{/each}
    </nav>{/if}
  <div class="image-canvas" bind:this={host} aria-label="图片编辑画布" role="img"></div>
  {#if !readonly}<aside class="image-properties">
      <h3>{tools.find((t) => t.id === tool)?.label}</h3>
      {#if tool === "crop"}<label for="crop-ratio"
          >裁剪比例<AppSelect
            id="crop-ratio"
            aria-label="裁剪比例"
            bind:value={ratio}
            options={[
              { value: "0", label: "自由比例" },
              { value: "1", label: "1 : 1" },
              { value: String(16 / 9), label: "16 : 9" },
              { value: String(4 / 3), label: "4 : 3" },
              { value: String(3 / 4), label: "3 : 4" },
            ]}
            onValueChange={() => {
              crop = null;
              cropNode.hide();
            }}
          /></label
        >
        <p class="muted">{ratio === "0" ? "拖动选择任意区域" : "按所选比例锁定裁剪区域"}</p>
        {#if crop}<div class="dimension-readout">{crop.width} × {crop.height} px</div>
          <button class="primary-button" onclick={() => flatten("crop")}>应用裁剪</button><button
            class="secondary-button"
            onclick={() => {
              crop = null;
              cropNode.hide();
              redraw();
            }}>取消裁剪</button
          >{:else}<p class="muted">在图片上拖动以框选裁剪区域。</p>{/if}
      {:else if tool === "mosaic"}<label
          >像素块大小 <strong>{pixelSize}px</strong><input
            type="range"
            min="6"
            max="48"
            bind:value={pixelSize}
          /></label
        >
        <p class="muted">框选要模糊的区域。敏感信息建议使用实色遮挡。</p>
      {:else if tool !== "select" && tool !== "pan"}<label
          >{tool === "cover" ? "遮挡颜色" : "颜色"}<input type="color" bind:value={color} /></label
        >{#if tool !== "cover" && tool !== "text"}<label
            >线条粗细 <strong>{stroke}px</strong><input
              type="range"
              min="1"
              max="24"
              bind:value={stroke}
            /></label
          >{/if}{#if tool === "cover"}<p class="notice">
            100% 不透明，保存后与图片合并。
          </p>{/if}{#if tool === "text"}<label
            >文字内容<textarea bind:value={text} rows="3"></textarea></label
          ><label>字号<input type="number" min="10" max="200" bind:value={fontSize} /></label><label
            for="text-font"
            >字体<AppSelect
              id="text-font"
              aria-label="字体"
              bind:value={fontFamily}
              options={[
                { value: "sans-serif", label: "无衬线" },
                { value: "serif", label: "衬线" },
                { value: "monospace", label: "等宽" },
              ]}
            /></label
          ><label for="text-background"
            >背景<AppSelect
              id="text-background"
              aria-label="背景"
              bind:value={background}
              options={[
                { value: "transparent", label: "透明" },
                { value: "#ffffff", label: "白色" },
                { value: "#1d2030", label: "深色" },
              ]}
            /></label
          >
          <p class="muted">点击画布添加文字。</p>{/if}
      {:else}<p class="muted">
          {tool === "select"
            ? "点击标注以移动、调整大小或删除。"
            : "拖动画布查看细节，滚轮可缩放。"}
        </p>{/if}
      {#if selected}<button class="secondary-button" onclick={removeSelected}
          ><Trash size={16} />删除所选标注</button
        >{#if document.marks.find((m) => m.id === selected)?.type === "text"}<label
            >修改文字<textarea bind:value={text}></textarea></label
          ><button class="secondary-button" onclick={updateText}>更新文字</button>{/if}{/if}
      <div class="image-hint">
        <strong>原图会保留</strong>
        <p>保存为新的 PNG 条目。标注会合并到图片，透明背景会保留。</p>
      </div>
    </aside>{/if}
</div>
{#if error}<div class="notice error-message" role="alert">{error}</div>{/if}
<div class="editor-status">
  <span
    >{document.width} × {document.height} px · PNG{showingOriginal ? " · 正在查看原图" : ""}</span
  >
  <div class="zoom-controls">
    <button title="缩小" onclick={() => setZoom(zoom * 0.8)}
      ><MagnifyingGlassMinus size={18} /></button
    ><button onclick={() => setZoom(1)}>{Math.round(zoom * 100)}%</button><button
      title="放大"
      onclick={() => setZoom(zoom * 1.25)}><MagnifyingGlassPlus size={18} /></button
    ><button class="quiet-button" onclick={fit}><CornersOut size={17} />适应窗口</button>
  </div>
</div>
{#if showReset}<dialog
    class="editor-dialog"
    use:modal
    aria-labelledby="reset-title"
    oncancel={() => (showReset = false)}
  >
    <h2 id="reset-title">重置所有图片修改？</h2>
    <p>恢复为打开时的原图。此操作可以撤销。</p>
    <div class="dialog-actions">
      <button class="secondary-button" onclick={() => (showReset = false)}>取消</button><button
        class="primary-button"
        onclick={() => {
          document = structuredClone(initial);
          showReset = false;
          crop = null;
          cropNode.hide();
          commit();
        }}>重置</button
      >
    </div>
  </dialog>{/if}
