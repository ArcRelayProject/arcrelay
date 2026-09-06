import { LogicalPosition } from "@tauri-apps/api/dpi";
import { Menu, type MenuItemOptions, type PredefinedMenuItemOptions } from "@tauri-apps/api/menu";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  buildRemoteFileBackgroundMenu,
  buildRemoteFileContextMenu,
  buildRemoteFileTreeMenu,
  type RemoteFileContextAction,
  type RemoteFileContextMenuItem,
} from "./remoteFileContextMenu";
import type { LanguagePreference, RemoteFileEntry } from "./types";

type RemoteFileContextMenuCallbacks = Partial<Record<RemoteFileContextAction, () => Promise<void> | void>>;

interface RemoteFileNativeContextMenuOptions {
  entry: RemoteFileEntry;
  writable: boolean;
  busy: boolean;
  language: LanguagePreference;
  callbacks: RemoteFileContextMenuCallbacks;
}

export async function showRemoteFileContextMenu(
  event: MouseEvent,
  { entry, writable, busy, language, callbacks }: RemoteFileNativeContextMenuOptions,
) {
  return showMenu(event, buildRemoteFileContextMenu({ kind: entry.kind, writable, busy, language }), callbacks);
}

export async function showRemoteFileBackgroundMenu(
  event: MouseEvent,
  options: {
    writable: boolean;
    busy: boolean;
    language: LanguagePreference;
    callbacks: RemoteFileContextMenuCallbacks;
  },
) {
  const { writable, busy, language, callbacks } = options;
  return showMenu(event, buildRemoteFileBackgroundMenu({ writable, busy, language }), callbacks);
}

export async function showRemoteFileTreeMenu(
  event: MouseEvent,
  options: {
    kind: "share" | "folder";
    expanded: boolean;
    busy: boolean;
    language: LanguagePreference;
    callbacks: RemoteFileContextMenuCallbacks;
  },
) {
  const { kind, expanded, busy, language, callbacks } = options;
  return showMenu(event, buildRemoteFileTreeMenu({ kind, expanded, busy, language }), callbacks);
}

async function showMenu(
  event: MouseEvent,
  menuItems: RemoteFileContextMenuItem[],
  callbacks: RemoteFileContextMenuCallbacks,
) {
  event.preventDefault();
  event.stopPropagation();

  const action = (id: RemoteFileContextAction) => () => {
    const callback = callbacks[id];
    if (callback) Promise.resolve(callback()).catch(console.error);
  };
  const items: Array<MenuItemOptions | PredefinedMenuItemOptions> = menuItems.map((item) => item.id === "separator"
    ? { item: "Separator" }
    : { id: `remote-file-${item.id}`, text: item.label, enabled: item.enabled, action: action(item.id) });

  const menu = await Menu.new({ items });
  try {
    await menu.popup(new LogicalPosition(event.clientX, event.clientY), getCurrentWindow());
  } finally {
    await menu.close();
  }
}
