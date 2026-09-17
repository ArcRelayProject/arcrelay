export type FileViewMode = "details" | "compact" | "grid";

/** Row-aligned windows preserve grid placement and keep DOM work viewport-bounded. */
export function fileWindow(
  count: number,
  scrollTop: number,
  height: number,
  width: number,
  mode: FileViewMode,
) {
  const padding = mode === "details" ? 0 : mode === "compact" ? 12 : 18;
  const gap = mode === "grid" ? 8 : mode === "compact" ? 3 : 0;
  const columns =
    mode === "details"
      ? 1
      : Math.max(
          1,
          Math.floor(
            (width - padding * 2 + (mode === "compact" ? 10 : 8)) /
              (mode === "compact" ? 280 : 158),
          ),
        );
  const rowHeight = mode === "details" ? 44 : mode === "compact" ? 51 : 124;
  const rows = Math.ceil(count / columns);
  const firstRow = Math.max(
    0,
    Math.min(
      Math.max(0, rows - Math.ceil(Math.max(height, 1) / rowHeight)),
      Math.floor(Math.max(0, scrollTop - padding) / rowHeight) - 3,
    ),
  );
  const lastRow = Math.min(rows, firstRow + Math.ceil(Math.max(height, 1) / rowHeight) + 7);
  return {
    start: firstRow * columns,
    end: Math.min(count, lastRow * columns),
    top: Math.max(0, firstRow * rowHeight - gap),
    bottom: Math.max(0, (rows - lastRow) * rowHeight - gap),
    columns,
  };
}
let generation = Date.now();
export function nextFileGeneration() {
  return ++generation;
}

export class RequestOwner {
  private generation = 0;
  private disposed = false;
  advance() {
    this.generation++;
  }
  capture(peerId: string) {
    const generation = this.generation;
    return (currentPeerId: string) =>
      !this.disposed && generation === this.generation && peerId === currentPeerId;
  }
  dispose() {
    this.disposed = true;
    this.advance();
  }
}
