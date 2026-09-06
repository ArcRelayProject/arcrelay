export function createCachedLoader<T>(loader: () => Promise<T>): () => Promise<T> {
  let cached: Promise<T> | undefined;

  return () => {
    cached ??= loader().catch((error: unknown) => {
      cached = undefined;
      throw error;
    });
    return cached;
  };
}
