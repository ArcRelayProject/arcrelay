import { clipboardBridge } from "./bridge";
import type { ClipboardItem } from "./types";

const previews = new Map<string, string[]>();
const pending = new Map<string, Promise<string[]>>();
const MAX_CACHED_PREVIEWS = 72;
const MAX_CACHED_NAME_CHARACTERS = 32_768;
let generation = 0;

export function filePreviewKey(item: ClipboardItem) {
  return `${item.id}:${item.updatedAtMs}:${item.itemCount}:${item.sizeBytes}:${item.preview}`;
}

export function cachedFileNames(item: ClipboardItem): string[] | undefined {
  if (item.sensitive || item.kind !== "files") return undefined;
  const key = filePreviewKey(item);
  const value = previews.get(key);
  if (value) {
    previews.delete(key);
    previews.set(key, value);
  }
  return value;
}

export function loadFileNames(item: ClipboardItem): Promise<string[]> {
  if (item.sensitive || item.kind !== "files") return Promise.resolve([]);
  const value = cachedFileNames(item);
  if (value) return Promise.resolve(value);
  const key = filePreviewKey(item);
  const existing = pending.get(key);
  if (existing) return existing;
  const currentGeneration = generation;
  const request = clipboardBridge
    .fileNames(item)
    .then((names) => {
      // Large batches are still shown in full, but must not fill a persistent
      // cache with unbounded filename data across many history records.
      if (
        generation === currentGeneration &&
        names.reduce((total, name) => total + name.length, 0) <= MAX_CACHED_NAME_CHARACTERS
      ) {
        previews.set(key, names);
        while (previews.size > MAX_CACHED_PREVIEWS) previews.delete(previews.keys().next().value!);
      }
      return names;
    })
    .finally(() => {
      if (pending.get(key) === request) pending.delete(key);
    });
  pending.set(key, request);
  return request;
}

export function clearFilePreviewCache() {
  generation += 1;
  previews.clear();
  pending.clear();
}
