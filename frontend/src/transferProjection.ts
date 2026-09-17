import type { TransferSnapshot, TransferProgress } from "./types.ts";

type PendingProgress = {
  latest: TransferProgress;
  files: Map<number, { revision: number; completedBytes: number }>;
};

/** Retain every dirty file until a full snapshot covers its revision. */
export class TransferProjection {
  value: TransferSnapshot | null = null;
  private baseline = -1;
  private pending = new Map<string, PendingProgress>();

  snapshot(next: TransferSnapshot) {
    if (next.revision <= this.baseline) return false;
    this.baseline = next.revision;
    this.value = next;
    for (const [id, pending] of this.pending) {
      for (const [fileId, file] of pending.files) {
        if (file.revision <= this.baseline) pending.files.delete(fileId);
      }
      if (pending.latest.revision <= this.baseline) this.pending.delete(id);
      else this.apply(pending);
    }
    return true;
  }

  progress(delta: TransferProgress) {
    if (delta.revision <= this.baseline) return true;
    let pending = this.pending.get(delta.transferId);
    if (!pending) {
      pending = { latest: delta, files: new Map() };
      this.pending.set(delta.transferId, pending);
    }
    if (delta.revision > pending.latest.revision) pending.latest = delta;
    for (const file of delta.files) {
      if (delta.revision > (pending.files.get(file.id)?.revision ?? -1)) {
        pending.files.set(file.id, {
          revision: delta.revision,
          completedBytes: file.completedBytes,
        });
      }
    }
    while (this.pending.size > 64) this.pending.delete(this.pending.keys().next().value!);
    return this.apply(pending);
  }

  private apply(pending: PendingProgress) {
    const delta = pending.latest;
    if (
      !this.value ||
      delta.baseRevision > this.baseline ||
      !this.value.transfers.some((t) => t.id === delta.transferId)
    )
      return false;
    this.value = {
      ...this.value,
      revision: Math.max(this.value.revision, delta.revision),
      transfers: this.value.transfers.map((transfer) =>
        transfer.id !== delta.transferId
          ? transfer
          : {
              ...transfer,
              completedBytes: delta.completedBytes,
              speedBytesPerSecond: delta.speedBytesPerSecond,
              remainingSeconds: delta.remainingSeconds,
              updatedAtMs: delta.updatedAtMs,
              files: transfer.files.map((file) => {
                const update = pending.files.get(file.id);
                return !update || update.completedBytes === file.completedBytes
                  ? file
                  : { ...file, completedBytes: update.completedBytes };
              }),
            },
      ),
    };
    return true;
  }
}
