import { getCurrentWindow } from '@tauri-apps/api/window';
import { convertFileSrc } from '@tauri-apps/api/core';
import { invoke, listen } from '../ipc/client';
import type { UnlistenFn } from '@tauri-apps/api/event';
import type { ClipboardSortPreference } from '../types';
import type { ClipboardTimelinePosition } from './types';
import type { ClipboardCursor, ClipboardHistory, ClipboardItem, ClipboardImageOcr, ClipboardKind, ClipboardLabel, NearbyClipboardPeer, ClipboardPasteMode, ContinuousPasteItemInput } from "./types";
async function imageSource(command: "clipboard_thumbnail" | "clipboard_image_preview", id: number) {
    const path = await invoke(command, { id });
    return path ? convertFileSrc(path) : null;
}
export const clipboardBridge = {
    timeline: (position: ClipboardTimelinePosition, sortBy: ClipboardSortPreference, limit = 30) =>
        invoke("clipboard_timeline", { position, sortBy, limit }),
    history: async (options: {
        search: string;
        kind: ClipboardKind | null;
        favoriteOnly: boolean;
        labelIds?: string[];
        cursor: ClipboardCursor | null;
        limit?: number;
    }) => invoke("clipboard_history", { ...options, labelIds: options.labelIds ?? null, limit: options.limit ?? null }),
    thumbnail: (id: number) => imageSource("clipboard_thumbnail", id),
    imagePreview: (id: number) => imageSource("clipboard_image_preview", id),
    imageOcr: (id: number) => invoke("clipboard_image_ocr", { id }),
    htmlPreview: (item: ClipboardItem) => invoke("clipboard_html_preview", { id: item.id }),
    copy: (id: number) => invoke("clipboard_copy_record", { id }),
    copyText: (content: string) => invoke("clipboard_copy_text", { content }),
    pasteText: (content: string) => invoke("clipboard_paste_text", { content }),
    paste: (id: number) => invoke("clipboard_paste_record", { id }),
    pasteAs: (id: number, mode: ClipboardPasteMode) => invoke("clipboard_paste_record_as", { id, mode }),
    remove: (id: number) => invoke("clipboard_delete_record", { id }),
    removeMany: (ids: number[]) => invoke("clipboard_delete_records", { ids }),
    setFavorite: (id: number, favorite: boolean) => invoke("clipboard_set_favorite", { id, favorite }),
    labels: () => invoke("clipboard_labels"),
    createLabel: (name: string, color: string) => invoke("clipboard_create_label", { name, color }),
    updateLabel: (labelId: string, name: string, color: string) => invoke("clipboard_update_label", { labelId, name, color }),
    deleteLabel: (labelId: string) => invoke("clipboard_delete_label", { labelId }),
    setLabels: (id: number, labelIds: string[]) => invoke("clipboard_set_labels", { id, labelIds }),
    setLabelMembership: (id: number, labelId: string, attached: boolean) => invoke("clipboard_set_label_membership", { id, labelId, attached }),
    setContextMenuOpen: (open: boolean) => invoke("set_clipboard_context_menu_open", { open }),
    editText: (id: number, content: string) => invoke("clipboard_edit_text", { id, content }),
    textContent: (item: ClipboardItem) => invoke("clipboard_text_content", { id: item.id }),
    textSegments: async (item: ClipboardItem): Promise<import("./textSegments").TextSliceModel> => {
        return invoke("clipboard_text_segments", { id: item.id });
    },
    joinSegments: async (id: number, version: string, ids: string[]): Promise<string> => {
        return invoke("clipboard_join_segments", { id, version, ids });
    },
    mergeDevices: () => invoke("clipboard_merge_devices"),
    nearbyPeers: async (): Promise<NearbyClipboardPeer[]> => {
        const snapshot = await invoke("get_transfer_state");
        return snapshot.peers;
    },
    sendFiles: (id: number, peerId: string) => invoke("clipboard_send_files", { id, peerId }),
    clear: () => invoke("clipboard_clear_history"),
    pasteRecords: (ids: number[]) => invoke("clipboard_paste_records", { ids }),
    startContinuousPaste: (items: ContinuousPasteItemInput[]) => invoke("clipboard_start_continuous_paste", { items }),
    hide: () => invoke("hide_clipboard_window"),
    visible: () => getCurrentWindow().isVisible(),
    pinned: () => invoke("get_clipboard_window_pinned"),
    setPinnedWindow: (pinned: boolean) => invoke("set_clipboard_window_pinned", { pinned }),
    startDragging: () => invoke("start_clipboard_window_drag"),
    onChanged: async (handler: () => void): Promise<UnlistenFn> => listen("clipboard-changed", handler),
    onOcrChanged: async (handler: (id: number) => void): Promise<UnlistenFn> => listen("clipboard-ocr-changed", (event) => handler(event.payload)),
    onShown: async (handler: () => void): Promise<UnlistenFn> => listen("clipboard-window-shown", handler),
    onHidden: async (handler: () => void): Promise<UnlistenFn> => listen("clipboard-window-hidden", handler),
    onPinChanged: async (handler: (pinned: boolean) => void): Promise<UnlistenFn> => listen("clipboard-window-pin-changed", (event) => handler(event.payload)),
    onContinuousPasteError: async (handler: (error: string) => void): Promise<UnlistenFn> => listen("clipboard-continuous-paste-error", (event) => handler(event.payload))
};
