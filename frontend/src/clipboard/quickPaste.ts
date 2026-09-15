/** Only number substantially visible cards, including cards taller than the viewport. */
export function visibleQuickPasteIds(
  entries: readonly { id: number }[], start: number, end: number,
  top: number, height: number, offset: (index: number) => number, gap = 8,
): number[] {
  if (height <= 0) return [];
  const ids: number[] = [];
  for (let index = start; index < Math.min(end, entries.length) && ids.length < 9; index++) {
    const rowTop = offset(index);
    const rowBottom = offset(index + 1) - gap;
    const visible = Math.max(0, Math.min(rowBottom, top + height) - Math.max(rowTop, top));
    if (visible >= Math.min(rowBottom - rowTop, height) * 0.5) ids.push(entries[index].id);
  }
  return ids;
}
