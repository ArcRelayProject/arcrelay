import type { TransferSnapshot, TransferProgress } from "./types.ts";

/** A delta is usable only after its full baseline. A gap requests a fresh snapshot. */
export class TransferProjection {
  value: TransferSnapshot | null = null;
  private baseline = -1;
  private revisions = new Map<string, number>();
  private pending = new Map<string, TransferProgress>();
  snapshot(next: TransferSnapshot) {
    if (next.revision <= this.baseline) return false;
    this.baseline = next.revision;
    this.value = next;
    this.revisions.clear();
    for (const delta of [...this.pending.values()].sort((a,b) => a.revision-b.revision)) {
      if (delta.revision <= this.baseline) this.pending.delete(delta.transferId);
      else this.progress(delta);
    }
    return true;
  }
  progress(delta: TransferProgress) {
    if (delta.revision <= this.baseline || delta.revision <= (this.revisions.get(delta.transferId) ?? -1)) return true;
    if (!this.value || delta.baseRevision > this.baseline || !this.value.transfers.some((t) => t.id === delta.transferId)) {
      const previous = this.pending.get(delta.transferId);
      if (!previous || delta.revision > previous.revision) this.pending.set(delta.transferId, delta);
      while (this.pending.size > 64) this.pending.delete(this.pending.keys().next().value!);
      return false;
    }
    this.pending.set(delta.transferId, delta);
    this.revisions.set(delta.transferId, delta.revision);
    this.value = { ...this.value, revision: Math.max(this.value.revision, delta.revision), transfers: this.value.transfers.map((transfer) => transfer.id !== delta.transferId ? transfer : {
      ...transfer, completedBytes: delta.completedBytes, speedBytesPerSecond: delta.speedBytesPerSecond,
      remainingSeconds: delta.remainingSeconds, updatedAtMs: delta.updatedAtMs,
      files: transfer.files.map((file) => ({...file, completedBytes: delta.files.find((f) => f.id === file.id)?.completedBytes ?? file.completedBytes})),
    }) };
    return true;
  }
}
