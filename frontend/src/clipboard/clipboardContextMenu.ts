import { t, translate } from "../localization";

import type { LanguagePreference } from "../types";
import { tr } from "./i18n";
import { clipboardBridge } from "./bridge";
import type {
  ClipboardItem,
  ClipboardLabel,
  ClipboardPasteMode,
  NearbyClipboardPeer,
} from "./types";
import {
  closeInlineContextMenu,
  showInlineContextMenu,
  type ContextMenuEntry,
} from "./inlineContextMenu";

let menuGeneration = 0;

interface ContextMenuCallbacks {
  application: import("../ipc/generated").ClipboardTargetApplication | null;
  appPinned: boolean;
  pinApplication: (pinned: boolean) => Promise<void> | void;
  error: (reason: unknown) => void;
  nearby?: () => Promise<void> | void;
  original?: () => Promise<void> | void;
  reload: () => Promise<void> | void;
  edit: () => Promise<void> | void;
  segment: () => Promise<void> | void;
  manageLabels: () => Promise<void> | void;
  paste: (mode: ClipboardPasteMode) => Promise<void> | void;
  typeAsKeys: () => Promise<void> | void;
  sendFiles: (peerId: string) => Promise<void> | void;
}

export async function showClipboardContextMenu(
  event: MouseEvent,
  item: ClipboardItem,
  labels: ClipboardLabel[],
  nearbyPeers: NearbyClipboardPeer[],
  language: LanguagePreference,
  callbacks: ContextMenuCallbacks,
) {
  event.preventDefault();
  event.stopPropagation();
  const action = (callback: () => Promise<unknown> | unknown) => () => {
    void Promise.resolve().then(callback).catch(callbacks.error);
  };
  const pasteItem = (text: string, mode: ClipboardPasteMode, enabled = true) => ({
    text,
    enabled,
    action: action(() => callbacks.paste(mode)),
  });
  const pasteItems: ContextMenuEntry[] = [
    pasteItem(translate("原格式", language), "source", item.available),
    pasteItem(
      item.kind === "image"
        ? translate("识别文字（OCR）", language)
        : translate("纯文本", language),
      "plain_text",
      item.available && item.kind !== "files",
    ),
  ];
  if (item.kind === "text" || item.kind === "html") {
    pasteItems.push({ item: "Separator" });
    pasteItems.push({
      text: translate("模拟键盘输入", language),
      enabled: item.available,
      action: action(callbacks.typeAsKeys),
    });
  }
  if (item.kind === "image") {
    pasteItems.push({ item: "Separator" });
    pasteItems.push(pasteItem("JPG", "image_jpg", item.available));
    pasteItems.push(pasteItem("PNG", "image_png", item.available));
  }
  if (item.kind === "html")
    pasteItems.push(pasteItem(translate("带格式文本", language), "rich_text", item.available));
  const syntax = typeof item.textSyntax === "string" ? item.textSyntax : "code";
  if (syntax === "json" || syntax === "yaml") {
    pasteItems.push({ item: "Separator" });
    pasteItems.push(pasteItem(translate("压缩 JSON", language), "json_compact"));
    pasteItems.push(pasteItem(translate("格式化 JSON", language), "json_formatted"));
    if (syntax === "json") pasteItems.push(pasteItem("YAML", "yaml"));
  }

  const labelItems: ContextMenuEntry[] = labels.map((label) => ({
    id: `clipboard-label-${label.id}`,
    text: label.name,
    checked: item.labels.some((current) => current.id === label.id),
    action: action(async () => {
      const attached = !item.labels.some((current) => current.id === label.id);
      await clipboardBridge.setLabelMembership(item.id, label.id, attached);
      await callbacks.reload();
    }),
  }));
  if (labelItems.length) labelItems.push({ item: "Separator" });
  labelItems.push({
    text: translate("新建或管理标签…", language),
    action: action(callbacks.manageLabels),
  });

  const pairedPeers = nearbyPeers.filter((peer) => peer.paired);
  const items: ContextMenuEntry[] = [
    {
      text: tr("插入到当前应用", language),
      icon: "insert",
      enabled: item.available,
      action: action(() => callbacks.paste("source")),
    },
    { text: translate("粘贴为", language), items: pasteItems },
    {
      text: tr("复制到剪贴板", language),
      icon: "copy",
      enabled: item.available,
      action: action(() => clipboardBridge.copy(item.id)),
    },
    ...(item.kind === "text" || item.kind === "html"
      ? [{ text: translate("预览与选择…", language), action: action(callbacks.segment) }]
      : []),
    {
      icon: "edit",
      text:
        item.kind === "html"
          ? "编辑纯文本副本…"
          : item.kind === "image"
            ? "编辑图片…"
            : item.kind === "files"
              ? "文件暂不支持编辑"
              : "编辑文本…",
      enabled: item.available && item.kind !== "files",
      action: action(callbacks.edit),
    },
    ...(callbacks.original ? [{ text: "查看原记录", action: action(callbacks.original) }] : []),
    ...(callbacks.nearby
      ? [{ text: tr("查看附近记录", language), action: action(callbacks.nearby) }]
      : []),
    { item: "Separator" },
    {
      text: callbacks.application
        ? t(callbacks.appPinned ? "取消在 {name} 中置顶" : "在 {name} 中置顶", language, {
            name: callbacks.application.name,
          })
        : translate("在当前应用中置顶", language),
      icon: "pin",
      enabled: callbacks.application !== null,
      action: action(() => callbacks.pinApplication(!callbacks.appPinned)),
    },
    {
      icon: "favorite",
      text: tr(item.favorite ? "取消收藏" : "收藏", language),
      action: action(async () => {
        await clipboardBridge.setFavorite(item.id, !item.favorite);
        await callbacks.reload();
      }),
    },
    { text: tr("添加到标签", language), icon: "label", items: labelItems },
    ...(item.kind === "files"
      ? [
          {
            text: tr("发送到", language),
            items: pairedPeers.length
              ? pairedPeers.map((peer) => ({
                  text: peer.name,
                  action: action(() => callbacks.sendFiles(peer.id)),
                }))
              : [{ text: tr("没有可用的附近设备", language), enabled: false }],
          },
        ]
      : []),
    { item: "Separator" },
    {
      text: tr("删除记录", language),
      icon: "delete",
      destructive: true,
      action: action(async () => {
        await clipboardBridge.remove(item.id);
        await callbacks.reload();
      }),
    },
  ];
  const generation = ++menuGeneration;
  closeInlineContextMenu();
  let selected: (() => void) | null = null;
  try {
    await clipboardBridge.setContextMenuOpen(true);
    if (generation !== menuGeneration) return;
    selected = await showInlineContextMenu(items, event.clientX, event.clientY);
  } finally {
    if (generation === menuGeneration) await clipboardBridge.setContextMenuOpen(false);
  }
  // The WebView menu and host suppression state are both closed before paste,
  // editing, or another action can restore/focus the saved recipient.
  if (generation === menuGeneration) selected?.();
}
