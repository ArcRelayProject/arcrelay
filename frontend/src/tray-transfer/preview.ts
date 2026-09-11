// Loaded only by the browser entry point. Never sends files or accesses the filesystem.
import { bridge } from '../bridge';
import type { TransferDraftFile, TransferSnapshot, TransferView } from '../types';
import type { TrayTransferPort } from './port';
import { visualPreviewLanguage, visualPreviewTheme } from '../visualPreview';

const query = new URLSearchParams(location.search);
const state = query.get('state') ?? 'hover';
const paths = ['/preview/设计稿.zip'];
const file: TransferDraftFile = { path: paths[0], canonicalPath: paths[0], name: '设计稿.zip', size: 84 * 1024 ** 2, modifiedAtMs: 1, kind: 'archive', thumbnailDataUrl: null, error: null };
const peers: TransferSnapshot['peers'] = [
  { id: 'mac', name: '办公室 MacBook Pro', platform: 'macOS', model: 'MacBook Pro', address: '127.0.0.1', addresses: [], port: 8765, publicKey: '', certificateSha256: '', paired: true, automaticReceive: true, lastSeenAtMs: Date.now() },
  { id: 'phone', name: '我的 iPhone', platform: 'iOS', model: 'iPhone', address: '127.0.0.1', addresses: [], port: 8766, publicKey: '', certificateSha256: '', paired: true, automaticReceive: false, lastSeenAtMs: Date.now() },
];
const snapshot: TransferSnapshot = { revision: 0, deviceId: 'preview', deviceName: 'ArcRelay', receiveDirectory: '', discoverable: true, peers: state === 'empty' ? [] : peers, transfers: [] };
const listeners = new Set<(state: TransferSnapshot) => void>();
const emit = () => { snapshot.revision++; listeners.forEach(listener => listener(structuredClone(snapshot))); };
const metadata = (paths: string[]) => paths.map(path => ({ ...file, path, canonicalPath: path, name: path.split('/').pop()!, error: state === 'unreadable' ? 'File not found' : null }));
function task(id: string, peerId: string, paths: string[], status: TransferView['status']): TransferView {
  const files = metadata(paths).map((file, index) => ({ id: index + 1, name: file.name, relativePath: file.name, size: file.size!, kind: file.kind, mediaType: 'application/zip', thumbnailDataUrl: null, completedBytes: 0 }));
  const totalBytes = files.reduce((sum, file) => sum + file.size, 0);
  return { id, wireId: 1, peerId, peerName: peers.find(peer => peer.id === peerId)!.name, direction: 'sending', status, files, totalBytes, completedBytes: status === 'completed' ? totalBytes : status === 'transferring' ? Math.ceil(totalBytes * .64) : 0, speedBytesPerSecond: 15.1 * 1024 ** 2, remainingSeconds: 2, errorMessage: status === 'failed' ? '连接已中断，请重新选择设备发送' : null, createdAtMs: Date.now(), updatedAtMs: Date.now() };
}
const initialStatus = { sending: 'transferring', complete: 'completed', waiting: 'awaitingApproval', failed: 'failed' }[state] as TransferView['status'] | undefined;
if (initialStatus) snapshot.transfers.push(task('preview-transfer', 'mac', paths, initialStatus));
let consumed = false;
let picked = false;
export const port: TrayTransferPort = {
  preview: true,
  initialTransferId: initialStatus ? 'preview-transfer' : undefined,
  async takeDrop() {
    const next = !consumed && state !== 'hover' && !initialStatus ? paths : [];
    consumed = true; return { revision: 1, hovering: state === 'hover', paths: next };
  },
  async onDrop() { return () => {}; },
  async updatePanel() {},
  async hide() { document.documentElement.dataset.previewHidden = 'true'; },
  async history() { location.href = '/?page=transfer&transferScenario=history'; },
  async inspect(paths) { return metadata(paths); },
  async pick() { const next = picked ? ['/preview/补充说明.pdf'] : paths; picked = true; return next; },
  async send(peerId, paths) {
    const id = crypto.randomUUID();
    snapshot.transfers.push(task(id, peerId, paths, 'awaitingApproval')); emit();
    setTimeout(() => {
      const current = snapshot.transfers.find(task => task.id === id)!;
      if (current.status !== 'awaitingApproval') return;
      Object.assign(current, task(id, peerId, paths, 'transferring')); emit();
      setTimeout(() => {
        if (current.status !== 'transferring') return;
        Object.assign(current, task(id, peerId, paths, 'completed')); emit();
      }, 5000);
    }, 2000);
    return id;
  },
  async cancel(id) { const current = snapshot.transfers.find(task => task.id === id); if (current) current.status = 'cancelled'; emit(); },
  async resume(id) { const current = snapshot.transfers.find(task => task.id === id); if (current) current.status = 'transferring'; emit(); },
  async refresh() { return structuredClone(snapshot); },
  async onTransfer(listener) { listeners.add(listener); listener(structuredClone(snapshot)); return () => { listeners.delete(listener); }; },
  async getSettings() { return { ...await bridge.getAppSettings(), language: visualPreviewLanguage() ?? 'zhCn', theme: visualPreviewTheme() ?? 'light' }; },
  async onSettings() { return () => {}; },
};
