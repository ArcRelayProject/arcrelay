import type { ClipboardLabel } from "./types";

export const QUICK_LABEL_LIMIT = 3;

export function filterClipboardLabels(labels: ClipboardLabel[], query: string) {
  const normalized = query.trim().toLocaleLowerCase();
  if (!normalized) return labels;
  return labels.filter((label) => label.name.toLocaleLowerCase().includes(normalized));
}

export function selectQuickLabels(
  labels: ClipboardLabel[],
  recentIds: string[],
  selectedId: string | null,
  limit = QUICK_LABEL_LIMIT,
) {
  if (limit <= 0) return [];
  const byId = new Map(labels.map((label) => [label.id, label]));
  const orderedIds = [selectedId, ...recentIds, ...labels.map((label) => label.id)];
  const seen = new Set<string>();
  const result: ClipboardLabel[] = [];
  for (const id of orderedIds) {
    if (!id || seen.has(id)) continue;
    seen.add(id);
    const label = byId.get(id);
    if (label) result.push(label);
    if (result.length === limit) break;
  }
  return result;
}

export function rememberRecentLabel(recentIds: string[], labelId: string, limit = 12) {
  return [labelId, ...recentIds.filter((id) => id !== labelId)].slice(0, limit);
}

export function parseRecentLabelIds(value: string | null) {
  if (!value) return [];
  try {
    const parsed = JSON.parse(value);
    return Array.isArray(parsed) ? parsed.filter((id): id is string => typeof id === "string") : [];
  } catch {
    return [];
  }
}
