import type { ClipboardSortPreference } from "../types";
import type { ClipboardCursor, ClipboardFilter, ClipboardHistory, ClipboardItem, ClipboardTimeline } from "./types";
import type { ClipboardKeyboardMode } from "./keyboardShortcuts";

export interface ScrollAnchor {
  id: number;
  /** Pixels from the row's top to the viewport's top; negative means below it. */
  offset: number;
}

export interface SearchSnapshot {
  search: string;
  filter: ClipboardFilter;
  labelId: string | null;
  sortBy: ClipboardSortPreference;
  history: ClipboardHistory;
  selectedId: number | null;
  selectedIds: number[];
  scrollTop: number;
  scrollAnchor: ScrollAnchor | null;
  rowHeights: Map<number, number>;
  keyboardMode: ClipboardKeyboardMode;
}

export interface NearbyHistory {
  snapshot: SearchSnapshot;
  anchor: ClipboardCursor;
  sortBy: ClipboardSortPreference;
  newerCursor: ClipboardCursor | null;
  olderCursor: ClipboardCursor | null;
  updated: boolean;
}

export function historyQueryKey(search: string, filter: ClipboardFilter, labelId: string | null) {
  return JSON.stringify([search, filter, labelId]);
}

/** Keep already-visible rows stable when a concurrent recopy moves a record
 * across a cursor. A deliberate refresh picks up reordered metadata. */
export function mergeTimelineEntries(current: ClipboardItem[], page: ClipboardTimeline, newer: boolean) {
  const seen = new Set(current.map((item) => item.id));
  const added = page.entries.filter((item) => {
    if (seen.has(item.id)) return false;
    seen.add(item.id);
    return true;
  });
  return newer ? [...added, ...current] : [...current, ...added];
}

export function captureScrollAnchor<T extends { id: number }>(
  entries: readonly T[], top: number, height: (item: T) => number,
): ScrollAnchor | null {
  let offset = 0;
  for (const item of entries) {
    const bottom = offset + height(item);
    if (bottom > top) return { id: item.id, offset: top - offset };
    offset = bottom;
  }
  const last = entries.at(-1);
  return last ? { id: last.id, offset: top - offset + height(last) } : null;
}

export function scrollTopForAnchor<T extends { id: number }>(
  entries: readonly T[], anchor: ScrollAnchor, height: (item: T) => number,
): number | null {
  let offset = 0;
  for (const item of entries) {
    if (item.id === anchor.id) return Math.max(0, offset + anchor.offset);
    offset += height(item);
  }
  return null;
}
