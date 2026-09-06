import { translate } from "../localization";
import { Menu, Submenu, type MenuItemOptions, type PredefinedMenuItemOptions } from "@tauri-apps/api/menu";
import { LogicalPosition } from "@tauri-apps/api/dpi";
import { getCurrentWindow } from "@tauri-apps/api/window";

import type { LanguagePreference } from "../types";
import { tr } from "./i18n";
import { clipboardBridge } from "./bridge";
import type { ClipboardItem, ClipboardLabel, ClipboardPasteMode, NearbyClipboardPeer } from "./types";

interface ContextMenuCallbacks {
  nearby?: () => Promise<void> | void;
  reload: () => Promise<void> | void;
  edit: () => Promise<void> | void;
  segment: () => Promise<void> | void;
  manageLabels: () => Promise<void> | void;
  paste: (mode: ClipboardPasteMode) => Promise<void> | void;
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
    Promise.resolve(callback()).catch(console.error);
  };
  const pasteItem = (text: string, mode: ClipboardPasteMode, enabled = true) => ({
    text,
    enabled,
    action: action(() => callbacks.paste(mode)),
  });
  const pasteItems: Array<MenuItemOptions | PredefinedMenuItemOptions> = [
    pasteItem(translate("原格式", language), "source", item.available),
    pasteItem(
      item.kind === "image" ? (translate("识别文字（OCR）", language)) : (translate("纯文本", language)),
      "plain_text",
      item.available && item.kind !== "files",
    ),
  ];
  if (item.kind === "html") pasteItems.push(pasteItem(translate("带格式文本", language), "rich_text", item.available));
  const syntax = typeof item.textSyntax === "string" ? item.textSyntax : "code";
  if (syntax === "json" || syntax === "yaml") {
    pasteItems.push({ item: "Separator" });
    pasteItems.push(pasteItem(translate("压缩 JSON", language), "json_compact"));
    pasteItems.push(pasteItem(translate("格式化 JSON", language), "json_formatted"));
    if (syntax === "json") pasteItems.push(pasteItem("YAML", "yaml"));
  }

  const labelItems: Array<MenuItemOptions | PredefinedMenuItemOptions> = labels.map((label) => ({
    id: `clipboard-label-${label.id}`,
    text: `${item.labels.some((current) => current.id === label.id) ? "●" : "○"} ${label.name}`,
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
  const menu = await Menu.new({
    items: [
      { text: tr("插入到当前应用", language), enabled: item.available, action: action(() => callbacks.paste("source")) },
      await Submenu.new({ text: translate("粘贴为", language), items: pasteItems }),
      { text: tr("复制到剪贴板", language), enabled: item.available, action: action(() => clipboardBridge.copy(item.id)) },
      ...((item.kind === "text" || item.kind === "html") ? [{ text: translate("分段选择…", language), action: action(callbacks.segment) }] : []),
      ...(item.kind === "text" ? [{ text: tr("编辑文本", language), action: action(callbacks.edit) }] : []),
      ...(callbacks.nearby ? [{ text: tr("查看附近记录", language), action: action(callbacks.nearby) }] : []),
      { item: "Separator" },
      { text: tr(item.favorite ? "取消收藏" : "收藏", language), action: action(async () => { await clipboardBridge.setFavorite(item.id, !item.favorite); await callbacks.reload(); }) },
      await Submenu.new({ text: tr("添加到标签", language), items: labelItems }),
      ...(item.kind === "files" ? [
        await Submenu.new({ text: tr("发送到", language), items: pairedPeers.length ? pairedPeers.map((peer) => ({ text: peer.name, action: action(() => callbacks.sendFiles(peer.id)) })) : [{ text: tr("没有可用的附近设备", language), enabled: false }] }),
      ] : []),
      { item: "Separator" },
      { text: tr("删除记录", language), action: action(async () => { await clipboardBridge.remove(item.id); await callbacks.reload(); }) },
    ],
  });
  await clipboardBridge.setContextMenuOpen(true);
  try {
    await menu.popup(new LogicalPosition(event.clientX, event.clientY), getCurrentWindow());
  } finally {
    await clipboardBridge.setContextMenuOpen(false);
    await menu.close();
  }
}
