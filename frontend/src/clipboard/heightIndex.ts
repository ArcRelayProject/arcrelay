/** Fenwick height index: O(n) construction, O(log n) resize and viewport lookup. */
export class HeightIndex {
  private readonly ids: number[];
  private readonly positions: Map<number, number>;
  private readonly heights: Float64Array;
  private readonly sums: Float64Array;

  constructor(rows: readonly { id: number; height: number }[]) {
    this.ids = rows.map(row => row.id);
    this.positions = new Map(rows.map((row, index) => [row.id, index]));
    this.heights = Float64Array.from(rows, row => HeightIndex.validHeight(row.height));
    this.sums = new Float64Array(rows.length + 1);
    for (let i = 1; i < this.sums.length; i++) {
      this.sums[i] += this.heights[i - 1];
      const parent = i + (i & -i);
      if (parent < this.sums.length) this.sums[parent] += this.sums[i];
    }
  }

  private static validHeight(height: number) {
    if (!Number.isFinite(height) || height <= 0) throw new RangeError("Row height must be positive and finite");
    return height;
  }

  get length() { return this.ids.length; }
  get total() { return this.offset(this.length); }

  set(id: number, height: number) {
    const index = this.positions.get(id);
    if (index === undefined) return;
    const delta = HeightIndex.validHeight(height) - this.heights[index];
    this.heights[index] = height;
    for (let i = index + 1; i < this.sums.length; i += i & -i) this.sums[i] += delta;
  }

  offset(index: number) {
    let sum = 0;
    for (let i = Math.max(0, Math.min(Math.trunc(index), this.length)); i > 0; i -= i & -i) sum += this.sums[i];
    return sum;
  }

  /** Number of rows ending at or before this pixel offset. */
  indexAt(top: number) {
    let index = 0;
    let sum = 0;
    let step = 2 ** Math.floor(Math.log2(Math.max(1, this.length)));
    for (; step >= 1; step /= 2) {
      const next = index + step;
      if (next <= this.length && sum + this.sums[next] <= top) {
        index = next;
        sum += this.sums[next];
      }
    }
    return index;
  }

  anchor(top: number) {
    if (this.length === 0) return null;
    const index = Math.min(this.indexAt(top), this.length - 1);
    return { id: this.ids[index], offset: top - this.offset(index) };
  }

  anchorTop(anchor: { id: number; offset: number }) {
    const index = this.positions.get(anchor.id);
    return index === undefined ? null : Math.max(0, this.offset(index) + anchor.offset);
  }
}
