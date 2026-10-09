/** Stable pinned prefix without changing chronological pagination or duplicating rows. */
export function mergeAppPinnedEntries<T extends { id: number }>(
  entries: readonly T[],
  pins: readonly T[],
): T[] {
  const seen = new Set<number>();
  return [...pins, ...entries].filter((entry) => {
    if (seen.has(entry.id)) return false;
    seen.add(entry.id);
    return true;
  });
}
