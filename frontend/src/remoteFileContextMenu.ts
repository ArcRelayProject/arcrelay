import { translate } from "./localization.ts";
import type { LanguagePreference, RemoteFileKind } from "./types";

export type RemoteFileContextAction =
  | "stopEditing"
  | "open"
  | "expand"
  | "collapse"
  | "download"
  | "uploadFiles"
  | "uploadFolder"
  | "newFolder"
  | "rename"
  | "delete"
  | "refresh";

export type RemoteFileContextMenuItem =
  | { id: "separator" }
  | { id: RemoteFileContextAction; label: string; enabled: boolean };

interface RemoteFileContextMenuOptions {
  kind: RemoteFileKind;
  writable: boolean;
  busy: boolean;
  language: LanguagePreference;
}

export function buildRemoteFileContextMenu({
  kind,
  writable,
  busy,
  language,
}: RemoteFileContextMenuOptions): RemoteFileContextMenuItem[] {
  const items: RemoteFileContextMenuItem[] = [];

  items.push({ id: "open", label: translate("打开", language), enabled: !busy });
  if (kind === "file" && writable) items.push({ id: "stopEditing", label: translate("停止自动同步编辑", language), enabled: !busy });
  items.push({ id: "separator" });

  if (kind === "folder") {
    items.push({ id: "uploadFiles", label: translate("上传文件到此处", language), enabled: writable && !busy });
    items.push({ id: "uploadFolder", label: translate("上传文件夹到此处", language), enabled: writable && !busy });
    items.push({ id: "newFolder", label: translate("新建子文件夹", language), enabled: writable && !busy });
    items.push({ id: "separator" });
  }

  items.push({ id: "download", label: translate("下载", language), enabled: !busy });
  items.push({ id: "rename", label: translate("重命名", language), enabled: writable && !busy });
  items.push({ id: "separator" });
  items.push({ id: "delete", label: translate("删除", language), enabled: writable && !busy });

  return items;
}

export function buildRemoteFileBackgroundMenu({
  writable,
  busy,
  language,
}: Omit<RemoteFileContextMenuOptions, "kind">): RemoteFileContextMenuItem[] {
  return [
    { id: "uploadFiles", label: translate("上传文件", language), enabled: writable && !busy },
    { id: "uploadFolder", label: translate("上传文件夹", language), enabled: writable && !busy },
    { id: "newFolder", label: translate("新建文件夹", language), enabled: writable && !busy },
    { id: "separator" },
    { id: "refresh", label: translate("刷新", language), enabled: !busy },
  ];
}

interface RemoteFileTreeMenuOptions {
  kind: "share" | "folder";
  expanded: boolean;
  busy: boolean;
  language: LanguagePreference;
}

export function buildRemoteFileTreeMenu({
  kind,
  expanded,
  busy,
  language,
}: RemoteFileTreeMenuOptions): RemoteFileContextMenuItem[] {
  return [
    { id: "open", label: translate("打开", language), enabled: !busy },
    {
      id: expanded ? "collapse" : "expand",
      label: expanded ? (translate("收缩", language)) : (translate("展开", language)),
      enabled: !busy,
    },
    { id: "separator" },
    { id: "refresh", label: kind === "share" ? (translate("刷新共享目录", language)) : (translate("刷新此文件夹", language)), enabled: !busy },
  ];
}
