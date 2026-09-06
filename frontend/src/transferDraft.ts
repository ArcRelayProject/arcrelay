import type { TransferDraftFile, TransferPeer, TransferSnapshot, TransferStatus } from './types.ts';

export const MAX_DRAFT_FILES = 256;
export const isActiveTransfer = (status: TransferStatus) => ['preparing', 'awaitingApproval', 'connecting', 'transferring', 'paused'].includes(status);
export interface DraftEntry extends TransferDraftFile { id: number; preparing: boolean; manual: boolean; systemShareRequestIds?: string[] }
export interface TransferBatch { peerId: string; peerName: string; files: DraftEntry[] }
export interface DraftState {
  files: DraftEntry[];
  undo: DraftEntry[];
  submitting: TransferBatch | null;
  batches: Record<string, TransferBatch>;
}
type Dependencies = {
  inspect: (paths: string[]) => Promise<TransferDraftFile[]>;
  send: (peerId: string, paths: string[], requestIds: string[]) => Promise<string>;
};
export class DraftError extends Error {
  readonly code: 'limit' | 'empty' | 'preparing' | 'invalid' | 'offline' | 'busy' | 'changed' | 'unavailable';
  constructor(code: DraftError['code']) { super(code); this.code = code; }
}
const fileKey = (file: TransferDraftFile) => file.canonicalPath ?? file.path;
const copyFiles = (files: DraftEntry[]) => files.map(file => ({ ...file, systemShareRequestIds: [...(file.systemShareRequestIds ?? [])] }));
const message = (error: unknown) => error instanceof Error ? error.message : String(error);

/** A session-scoped Svelte-compatible store, independent of the page lifecycle.
 * Nothing here chooses a default peer. Only sendTo/retry invoke the send port.
 */
export class TransferDraft {
  private value: DraftState = { files: [], undo: [], submitting: null, batches: {} };
  private listeners = new Set<(state: DraftState) => void>();
  private sequence = 0;
  private peers: TransferPeer[] = [];
  private transfers: TransferSnapshot['transfers'] = [];
  private reservations = new Map<string, string>();
  private dependencies: Dependencies;
  constructor(dependencies: Dependencies) { this.dependencies = dependencies; }
  get state() { return this.value; }
  subscribe = (listener: (state: DraftState) => void) => {
    this.listeners.add(listener);
    listener(this.value);
    return () => { this.listeners.delete(listener); };
  };
  private update(patch: Partial<DraftState>) {
    this.value = { ...this.value, ...patch };
    this.listeners.forEach(listener => listener(this.value));
  }
  updateNetwork(snapshot: TransferSnapshot) {
    this.peers = snapshot.peers;
    this.transfers = snapshot.transfers;
    for (const [id] of this.reservations) {
      if (snapshot.transfers.some(transfer => transfer.id === id)) this.reservations.delete(id);
    }
    // Notify even when only device readiness changes.
    this.update({});
  }
  targetBusy(peerId: string) {
    return this.value.submitting?.peerId === peerId
      || [...this.reservations.values()].includes(peerId)
      || this.transfers.some(transfer => transfer.peerId === peerId && transfer.direction === 'sending' && isActiveTransfer(transfer.status));
  }
  private validateTarget(peerId: string) {
    const peer = this.peers.find(peer => peer.id === peerId);
    if (!peer) throw new DraftError('offline');
    if (this.targetBusy(peerId)) throw new DraftError('busy');
    return peer;
  }
  async add(paths: string[], systemShareRequestId?: string) {
    const existing = new Set(this.value.files.flatMap(file => [file.path, fileKey(file)]));
    const unique = [...new Set(paths)].filter(path => path && !existing.has(path));
    if (this.value.files.length + unique.length > MAX_DRAFT_FILES) throw new DraftError('limit');
    const linked = systemShareRequestId ? this.value.files.map(file =>
      paths.some(path => path === file.path || path === fileKey(file))
        ? { ...file, systemShareRequestIds: [...new Set([...(file.systemShareRequestIds ?? []), systemShareRequestId])] }
        : file) : this.value.files;
    if (!unique.length) {
      if (systemShareRequestId) this.update({ files: linked });
      return 0;
    }
    const entries: DraftEntry[] = unique.map(path => ({
      id: ++this.sequence, path, canonicalPath: null, name: path.split(/[\\/]/).pop() || path,
      size: null, modifiedAtMs: null, kind: 'file', thumbnailDataUrl: null, error: null, preparing: true,
      manual: !systemShareRequestId,
      systemShareRequestIds: systemShareRequestId ? [systemShareRequestId] : [],
    }));
    this.update({ files: [...linked, ...entries] });
    let metadata: TransferDraftFile[];
    try { metadata = await this.dependencies.inspect(unique); }
    catch (error) { metadata = entries.map(file => ({ ...file, error: message(error) })); }
    const ids = new Set(entries.map(file => file.id));
    const results = new Map(metadata.map(file => [file.path, file]));
    // Resolve only surviving entry IDs: clearing/removing during inspection must
    // never resurrect files or overwrite a later addition of the same path.
    const prepared = this.value.files.map(file => {
      if (!ids.has(file.id)) return file;
      return { ...file, ...(results.get(file.path) ?? { error: 'File metadata unavailable' }), preparing: false };
    });
    const deduplicated = new Map<string, DraftEntry>();
    for (const file of prepared) {
      const key = fileKey(file);
      const existing = deduplicated.get(key);
      if (existing) {
        existing.manual ||= file.manual;
        existing.systemShareRequestIds = [...new Set([...(existing.systemShareRequestIds ?? []), ...(file.systemShareRequestIds ?? [])])];
      } else deduplicated.set(key, file);
    }
    const files = [...deduplicated.values()];
    // Keep metadata current in an undo record, without putting it back in draft.
    const undo = this.value.undo.map(file => ids.has(file.id)
      ? { ...file, ...(results.get(file.path) ?? { error: 'File metadata unavailable' }), preparing: false }
      : file);
    this.update({ files, undo });
    return files.filter(file => ids.has(file.id)).length;
  }
  locked(id: number) { return this.value.submitting?.files.some(file => file.id === id) ?? false; }
  remove(id: number) {
    if (!this.locked(id)) this.update({ files: this.value.files.filter(file => file.id !== id) });
  }
  clear() {
    if (!this.value.submitting) this.update({ undo: copyFiles(this.value.files), files: [] });
  }
  discardUndo() { this.update({ undo: [] }); }
  undoClear() {
    const files = [...this.value.files];
    const keys = new Set(files.map(fileKey));
    for (const file of this.value.undo) {
      if (!keys.has(fileKey(file))) { keys.add(fileKey(file)); files.push(file); }
    }
    if (files.length > MAX_DRAFT_FILES) throw new DraftError('limit');
    this.update({ files, undo: [] });
  }
  async restore(transferId: string) {
    const batch = this.value.batches[transferId];
    if (!batch) throw new DraftError('unavailable');
    return this.add(batch.files.map(file => file.path));
  }
  async sendTo(peerId: string) {
    if (this.value.submitting) throw new DraftError('busy');
    const peer = this.validateTarget(peerId);
    const files = copyFiles(this.value.files);
    if (!files.length) throw new DraftError('empty');
    if (files.some(file => file.preparing)) throw new DraftError('preparing');
    if (files.some(file => file.error)) throw new DraftError('invalid');
    return this.submit({ peerId: peer.id, peerName: peer.name, files }, true);
  }
  async sendSystemRequestTo(systemShareRequestId: string, peerId: string) {
    if (this.value.submitting) throw new DraftError('busy');
    const peer = this.validateTarget(peerId);
    const files = copyFiles(this.value.files.filter(file => file.systemShareRequestIds?.includes(systemShareRequestId)))
      .map(file => ({ ...file, systemShareRequestIds: [systemShareRequestId] }));
    if (!files.length) throw new DraftError('empty');
    if (files.some(file => file.preparing)) throw new DraftError('preparing');
    if (files.some(file => file.error)) throw new DraftError('invalid');
    return this.submit({ peerId: peer.id, peerName: peer.name, files }, true, systemShareRequestId);
  }
  systemRequestIds(transferId?: string) {
    const files = transferId ? this.value.batches[transferId]?.files ?? [] : this.value.files;
    return [...new Set(files.flatMap(file => file.systemShareRequestIds ?? []))];
  }
  hasSystemRequest(systemShareRequestId: string) {
    return this.value.files.some(file => file.systemShareRequestIds?.includes(systemShareRequestId));
  }
  async retry(transferId: string) {
    if (this.value.submitting) throw new DraftError('busy');
    const batch = this.value.batches[transferId];
    const task = this.transfers.find(transfer => transfer.id === transferId);
    if (!batch || !task || !['failed', 'rejected', 'cancelled'].includes(task.status)) throw new DraftError('unavailable');
    this.validateTarget(batch.peerId);
    return this.submit({ ...batch, files: copyFiles(batch.files) }, false);
  }
  private async submit(batch: TransferBatch, consumeDraft: boolean, consumeRequestId?: string) {
    this.update({ submitting: batch });
    try {
      // Revalidate exact original paths, including existence and symlink policy.
      const inspected = await this.dependencies.inspect(batch.files.map(file => file.path));
      const current = new Map(inspected.map(file => [file.path, file]));
      let invalid = false;
      let changed = false;
      for (const file of batch.files) {
        const latest = current.get(file.path);
        invalid ||= !latest || Boolean(latest.error);
        changed ||= Boolean(latest && !latest.error && (fileKey(latest) !== fileKey(file) || latest.size !== file.size || latest.modifiedAtMs !== file.modifiedAtMs));
      }
      if (invalid || changed) {
        if (consumeDraft) {
          const ids = new Set(batch.files.map(file => file.id));
          this.update({ files: this.value.files.map(file => ids.has(file.id)
            ? { ...file, ...(current.get(file.path) ?? { error: 'File metadata unavailable' }) } : file) });
        }
        throw new DraftError(invalid ? 'invalid' : 'changed');
      }
      // Discovery/approval events can arrive while metadata is being read.
      // Recheck the same target, never fall back to another peer.
      if (!this.peers.some(peer => peer.id === batch.peerId)) throw new DraftError('offline');
      if (this.transfers.some(task => task.peerId === batch.peerId && task.direction === 'sending' && isActiveTransfer(task.status))) throw new DraftError('busy');
      const requestIds = [...new Set(batch.files.flatMap(file => file.systemShareRequestIds ?? []))];
      const transferId = await this.dependencies.send(batch.peerId, batch.files.map(file => file.path), requestIds);
      if (!this.transfers.some(task => task.id === transferId)) this.reservations.set(transferId, batch.peerId);
      const ids = new Set(batch.files.map(file => file.id));
      const remainingFiles = consumeRequestId ? this.value.files.flatMap(file => {
        if (!ids.has(file.id)) return [file];
        const systemShareRequestIds = (file.systemShareRequestIds ?? []).filter(id => id !== consumeRequestId);
        return file.manual || systemShareRequestIds.length ? [{ ...file, systemShareRequestIds }] : [];
      }) : consumeDraft ? this.value.files.filter(file => !ids.has(file.id)) : this.value.files;
      this.update({
        files: remainingFiles,
        undo: consumeDraft ? [] : this.value.undo,
        batches: { ...this.value.batches, [transferId]: batch },
      });
      return transferId;
    } finally { this.update({ submitting: null }); }
  }
}
