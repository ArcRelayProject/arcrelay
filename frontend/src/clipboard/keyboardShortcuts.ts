export type ClipboardKeyboardMode = "search" | "results";

export type ClipboardKeyboardAction =
  | { type: "focusSearch" }
  | { type: "cancelSearch" }
  | { type: "focusResults" }
  | { type: "move"; delta: -1 | 1 }
  | { type: "page"; direction: -1 | 1 }
  | { type: "paste"; plainText: boolean }
  | { type: "pasteRank"; index: number }
  | { type: "copy" }
  | { type: "favorite" }
  | { type: "edit" }
  | { type: "labels" }
  | { type: "preview" }
  | { type: "actions" };

export interface ClipboardShortcutEvent {
  key: string;
  code: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  isComposing: boolean;
  repeat?: boolean;
}

export function resolvePreviewKeyboardAction(
  event: ClipboardShortcutEvent,
): "close" | "suppressRepeat" | null {
  const unmodified = !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey;
  if (event.isComposing || !unmodified || event.code !== "Space") return null;
  return event.repeat ? "suppressRepeat" : "close";
}

function numberedShortcutIndex(event: ClipboardShortcutEvent) {
  const match = event.code.match(/^(?:Digit|Numpad)([1-5])$/);
  return match ? Number(match[1]) - 1 : -1;
}

export function resolveClipboardKeyboardAction(
  event: ClipboardShortcutEvent,
  mode: ClipboardKeyboardMode,
): ClipboardKeyboardAction | null {
  if (event.isComposing) return null;

  const primaryModifier = event.metaKey || event.ctrlKey;
  const unmodified = !primaryModifier && !event.altKey && !event.shiftKey;

  if (primaryModifier && !event.altKey && !event.shiftKey && event.code === "KeyF") {
    return { type: mode === "search" ? "cancelSearch" : "focusSearch" };
  }
  if (primaryModifier && !event.altKey && !event.shiftKey && event.code === "KeyK") {
    return { type: "actions" };
  }
  if (mode === "search" && event.key === "Tab" && !primaryModifier && !event.altKey && !event.shiftKey) {
    return { type: "focusResults" };
  }
  if (mode === "results" && event.key === "Tab" && !primaryModifier && !event.altKey) {
    return { type: "move", delta: event.shiftKey ? -1 : 1 };
  }
  if (unmodified && event.key === "ArrowDown") return { type: "move", delta: 1 };
  if (unmodified && event.key === "ArrowUp") return { type: "move", delta: -1 };
  if (!primaryModifier && !event.altKey && event.key === "Enter") {
    return { type: "paste", plainText: event.shiftKey };
  }
  if (unmodified && event.code === "PageUp") return { type: "page", direction: -1 };
  if (unmodified && event.code === "PageDown") return { type: "page", direction: 1 };

  const shortcutIndex = numberedShortcutIndex(event);
  if (shortcutIndex >= 0) {
    if (mode === "search" && primaryModifier && !event.altKey && !event.shiftKey) {
      return { type: "pasteRank", index: shortcutIndex };
    }
    if (mode === "results" && unmodified) {
      return { type: "pasteRank", index: shortcutIndex };
    }
  }

  if (mode !== "results" || !unmodified) return null;
  if (event.code === "Slash") return { type: "focusSearch" };
  if (event.code === "KeyJ") return { type: "move", delta: 1 };
  if (event.code === "KeyK") return { type: "move", delta: -1 };
  if (event.code === "KeyC") return { type: "copy" };
  if (event.code === "KeyF") return { type: "favorite" };
  if (event.code === "KeyE") return { type: "edit" };
  if (event.code === "KeyL") return { type: "labels" };
  if (event.code === "Space") return { type: "preview" };
  if (event.key === "ArrowRight" || event.code === "ContextMenu") return { type: "actions" };
  return null;
}
