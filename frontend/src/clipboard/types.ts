export type ClipboardKind = "text" | "html" | "image" | "files";
export type ClipboardPasteMode = import('../ipc/generated').ClipboardPasteMode;
export type ClipboardTextSyntax = import('../ipc/generated').ClipboardTextSyntax;

export type ClipboardCursor = import('../ipc/generated').ClipboardCursorView;

export type ClipboardLabel = import('../ipc/generated').ClipboardLabel;

export type ClipboardItem = import('../ipc/generated').ClipboardItemView & { safeHtml?: string | null };

export type ClipboardHistory = import('../ipc/generated').ClipboardHistoryView;

export type ClipboardTimeline = import('../ipc/generated').ClipboardTimelineView;
export type ClipboardTimelinePosition = import('../ipc/generated').ClipboardTimelinePositionInput;

export type ClipboardOcrPoint = import('../ipc/generated').ClipboardOcrPoint;

export type ClipboardOcrCharacter = import('../ipc/generated').ClipboardOcrCharacter;

export type ClipboardOcrBlock = import('../ipc/generated').ClipboardOcrBlock;

export type ClipboardImageOcr = import('../ipc/generated').ClipboardImageOcr;

export type ClipboardFilter = "all" | "text" | "image" | "files" | "favorites";

export interface NearbyClipboardPeer {
  id: string;
  name: string;
  paired: boolean;
}

export type ContinuousPasteProgress = import('../ipc/generated').ContinuousPasteProgress;
