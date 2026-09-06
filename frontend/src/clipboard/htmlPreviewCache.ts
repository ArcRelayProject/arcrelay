import { clipboardBridge } from "./bridge";
import type { ClipboardItem } from "./types";

const MAX_CACHED_HTML_PREVIEWS = 72;

interface CachedHtmlPreview {
  version: string;
  value: string | null;
}

const htmlPreviews = new Map<number, CachedHtmlPreview>();
const pendingHtmlPreviews = new Map<string, Promise<string | null>>();
let cacheGeneration = 0;

function previewVersion(item: ClipboardItem) {
  return `${item.updatedAtMs}:${item.sizeBytes}:${item.preview}`;
}

function requestKey(item: ClipboardItem) {
  return `${item.id}:${previewVersion(item)}`;
}

function remember(item: ClipboardItem, value: string | null) {
  htmlPreviews.delete(item.id);
  htmlPreviews.set(item.id, { version: previewVersion(item), value });

  while (htmlPreviews.size > MAX_CACHED_HTML_PREVIEWS) {
    const oldestId = htmlPreviews.keys().next().value;
    if (oldestId === undefined) break;
    htmlPreviews.delete(oldestId);
  }
}

export function cachedHtmlPreview(item: ClipboardItem): string | null | undefined {
  const cached = htmlPreviews.get(item.id);
  if (!cached || cached.version !== previewVersion(item)) return undefined;

  htmlPreviews.delete(item.id);
  htmlPreviews.set(item.id, cached);
  return cached.value;
}

export function loadCachedHtmlPreview(item: ClipboardItem): Promise<string | null> {
  const cached = cachedHtmlPreview(item);
  if (cached !== undefined) return Promise.resolve(cached);

  const key = requestKey(item);
  const pending = pendingHtmlPreviews.get(key);
  if (pending) return pending;

  const snapshot = { ...item };
  const generation = cacheGeneration;
  const request = clipboardBridge
    .htmlPreview(snapshot)
    .then((value) => {
      if (generation === cacheGeneration) remember(snapshot, value);
      return value;
    })
    .catch(() => {
      return null;
    })
    .finally(() => {
      if (pendingHtmlPreviews.get(key) === request) pendingHtmlPreviews.delete(key);
    });

  pendingHtmlPreviews.set(key, request);
  return request;
}

export function clearHtmlPreviewCache() {
  cacheGeneration += 1;
  htmlPreviews.clear();
  pendingHtmlPreviews.clear();
}
