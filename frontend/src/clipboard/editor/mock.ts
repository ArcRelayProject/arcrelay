import { marked } from "marked";
import DOMPurify from "dompurify";
import type {
  ClipboardEditorDraft,
  ClipboardEditorSnapshot,
  ClipboardTextFormat,
  ClipboardTextPreview,
} from "../../ipc/generated";
export const originalText =
  "# 发布说明\nArcRelay 1.1.0 现已发布。\n\n## 本次更新\n- 支持剪贴板文本编辑\n- 新增 Markdown 预览\n\n## 使用方式\n1. 在历史记录中选择内容\n2. 点击「编辑」打开独立窗口\n3. 完成后保存并复制\n";
export async function snapshot(): Promise<ClipboardEditorSnapshot> {
  const params = new URLSearchParams(location.search);
  if (params.get("kind") === "image") {
    const canvas = document.createElement("canvas");
    canvas.width = 1440;
    canvas.height = 900;
    const ctx = canvas.getContext("2d")!;
    ctx.fillStyle = "#f5f6fb";
    ctx.fillRect(0, 0, 1440, 900);
    ctx.fillStyle = "#fff";
    ctx.fillRect(160, 110, 1120, 690);
    ctx.fillStyle = "#5b5ff0";
    ctx.fillRect(160, 110, 1120, 75);
    ctx.font = "28px sans-serif";
    ctx.fillStyle = "#fff";
    ctx.fillText("ArcRelay · 设备协同", 200, 160);
    ctx.fillStyle = "#222433";
    ctx.font = "bold 44px sans-serif";
    ctx.fillText("剪贴板，随处可用。", 230, 290);
    ctx.font = "26px sans-serif";
    ctx.fillStyle = "#73788a";
    ctx.fillText("在你的设备之间安全地复制、编辑和分享。", 230, 345);
    ["MacBook Pro   ·   本机", "iPhone   ·   已连接", "Windows PC   ·   在线"].forEach(
      (text, i) => {
        ctx.fillStyle = "#f3f3fd";
        ctx.fillRect(230, 400 + i * 105, 980, 80);
        ctx.fillStyle = "#444858";
        ctx.fillText(text, 260, 450 + i * 105);
      },
    );
    return {
      sourceId: 2,
      kind: "image",
      text: null,
      image: canvas.toDataURL("image/png"),
      width: 1440,
      height: 900,
      plainCopy: false,
    };
  }
  return {
    sourceId: 1,
    kind: "text",
    text: originalText,
    image: null,
    width: null,
    height: null,
    plainCopy: params.has("plainCopy"),
  };
}
export async function preview(
  source: string,
  format: ClipboardTextFormat,
): Promise<ClipboardTextPreview> {
  const html = DOMPurify.sanitize(await marked.parse(source), {
    FORBID_TAGS: ["img", "iframe", "script", "style"],
    FORBID_ATTR: ["style"],
  });
  return { source, format, safeHtml: format === "text" ? null : html, renderLimited: false };
}
let saved: ClipboardEditorDraft | null = null;
export async function save(draft: ClipboardEditorDraft) {
  await new Promise((resolve) => setTimeout(resolve, 500));
  if (new URLSearchParams(location.search).has("saveError"))
    throw new Error("Insufficient disk space. Free space and retry.");
  saved ??= draft;
  return 279;
}
export async function copy() {
  if (!saved) throw new Error("No saved draft");
  if (new URLSearchParams(location.search).has("copyError"))
    throw new Error("The clipboard is busy in another application.");
}
