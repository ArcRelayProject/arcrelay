import { clipboardBridge } from "./bridge";

const MAX_CACHED_THUMBNAILS = 24;
const MAX_CONCURRENT_THUMBNAIL_LOADS = 3;

const thumbnails = new Map<number, string | null>();
const pendingThumbnails = new Map<number, Promise<string | null>>();
const thumbnailWaiters: Array<() => void> = [];
let activeThumbnailLoads = 0;
let cacheGeneration = 0;

async function withThumbnailSlot<T>(operation: () => Promise<T>): Promise<T> {
  if (activeThumbnailLoads >= MAX_CONCURRENT_THUMBNAIL_LOADS) {
    await new Promise<void>((resolve) => thumbnailWaiters.push(resolve));
  }
  activeThumbnailLoads += 1;
  try {
    return await operation();
  } finally {
    activeThumbnailLoads -= 1;
    thumbnailWaiters.shift()?.();
  }
}

function remember(id: number, source: string | null) {
  thumbnails.delete(id);
  thumbnails.set(id, source);

  while (thumbnails.size > MAX_CACHED_THUMBNAILS) {
    const oldestId = thumbnails.keys().next().value;
    if (oldestId === undefined) break;
    thumbnails.delete(oldestId);
  }
}

function decodeImage(source: string) {
  if (typeof Image === "undefined") return Promise.resolve();

  const image = new Image();
  if (typeof image.decode === "function") {
    image.src = source;
    return image.decode().catch(() => undefined);
  }

  return new Promise<void>((resolve) => {
    image.onload = () => resolve();
    image.onerror = () => resolve();
    image.src = source;
  });
}

export function cachedThumbnail(id: number): string | null | undefined {
  if (!thumbnails.has(id)) return undefined;
  const source = thumbnails.get(id) ?? null;
  remember(id, source);
  return source;
}

export function loadCachedThumbnail(id: number): Promise<string | null> {
  const cached = cachedThumbnail(id);
  if (cached !== undefined) return Promise.resolve(cached);

  const pending = pendingThumbnails.get(id);
  if (pending) return pending;

  const generation = cacheGeneration;
  const request = withThumbnailSlot(async () => {
    if (generation !== cacheGeneration) return null;
    try {
      const source = await clipboardBridge.thumbnail(id);
      if (source) await decodeImage(source);
      if (generation !== cacheGeneration) return null;
      remember(id, source);
      return source;
    } catch {
      // A transient failure is retryable; do not cache it as unavailable.
      return null;
    }
  }).finally(() => { if (pendingThumbnails.get(id) === request) pendingThumbnails.delete(id); });

  pendingThumbnails.set(id, request);
  return request;
}

export function clearThumbnailCache() {
  cacheGeneration += 1;
  thumbnails.clear();
  pendingThumbnails.clear();
}
