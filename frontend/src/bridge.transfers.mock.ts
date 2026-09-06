import type { TransferDraftFile, TransferSnapshot, TransferView } from './types';
import { mockData } from './bridgeMockData';

// Explicit browser-only fixtures. The Tauri bridge never imports this module.
const scenario = new URLSearchParams(window.location.search).get('transferScenario');
const files: TransferDraftFile[] = [
  { path: '/preview/产品方案.pdf', canonicalPath: '/preview/产品方案.pdf', name: '产品方案.pdf', size: 2516582, modifiedAtMs: 1, kind: 'pdf', thumbnailDataUrl: null, error: null },
  { path: '/preview/封面.png', canonicalPath: '/preview/封面.png', name: '封面.png', size: 7130316, modifiedAtMs: 1, kind: 'image', thumbnailDataUrl: null, error: null },
  { path: '/preview/演示视频.mp4', canonicalPath: '/preview/演示视频.mp4', name: '演示视频.mp4', size: 15518924, modifiedAtMs: 1, kind: 'file', thumbnailDataUrl: null, error: null },
];
const peers: TransferSnapshot['peers'] = [
  { id: 'preview-mac', name: '陈子博的 MacBook Pro', platform: 'macOS', model: 'MacBook-Pro-11607.local', address: '127.0.0.1', addresses: [], port: 8765, publicKey: '', certificateSha256: '', paired: true, automaticReceive: false, lastSeenAtMs: Date.now() },
  { id: 'preview-phone', name: '陈子博的 iPhone', platform: 'iOS', model: 'iPhone 15 Pro', address: '127.0.0.1', addresses: [], port: 8766, publicKey: '', certificateSha256: '', paired: true, automaticReceive: false, lastSeenAtMs: Date.now() },
];
const viewFiles = (paths: string[]) => paths.map((path, index) => {
  const file = files.find(file => file.path === path);
  return { id: index + 1, name: file?.name ?? path.split('/').pop()!, relativePath: file?.name ?? path.split('/').pop()!, size: file?.size ?? 1024, kind: file?.kind ?? 'file' as const, mediaType: 'application/octet-stream', thumbnailDataUrl: null, completedBytes: 0 };
});
const snapshot: TransferSnapshot = { revision: 0, deviceId: 'browser', deviceName: 'ArcRelay', receiveDirectory: '/Users/demo/Downloads', discoverable: true, peers: scenario && scenario !== 'no-devices' ? peers : [], transfers: [] };
if (scenario) {
  const incoming = scenario === 'incoming';
  const task: TransferView = { id: 'preview-history', wireId: 1, peerId: peers[0].id, peerName: peers[0].name, direction: incoming ? 'receiving' : 'sending', status: incoming ? 'awaitingApproval' : scenario === 'sending' ? 'transferring' : scenario === 'failed' ? 'failed' : 'completed', files: viewFiles([files[0].path]), totalBytes: files[0].size!, completedBytes: incoming ? 0 : scenario === 'sending' ? 1000000 : files[0].size!, speedBytesPerSecond: 250000, remainingSeconds: 6, errorMessage: scenario === 'failed' ? 'Connection interrupted' : null, createdAtMs: Date.now() - 60000, updatedAtMs: Date.now() };
  snapshot.transfers.push(task);
}
const listeners = new Set<(snapshot: TransferSnapshot) => void>();
const clone = () => ({ ...structuredClone(snapshot), discoverable: mockData.appSettings.nearbyDiscoverable });
function emit() { snapshot.revision++; listeners.forEach(listener => listener(clone())); }
function update(id: string, status: TransferView['status']) { snapshot.transfers = snapshot.transfers.map(task => task.id === id ? { ...task, status, completedBytes: task.status === 'awaitingApproval' ? 0 : task.completedBytes } : task); emit(); }
let picked = false;

export const transfersMock = {
  async getTransferState(): Promise<TransferSnapshot> { return clone(); },
  async refreshTransferDevices(): Promise<TransferSnapshot> {
    if (scenario === 'offline') snapshot.peers = snapshot.peers.filter(peer => peer.id !== peers[0].id);
    emit(); return clone();
  },
  async onTransferState(listener: (snapshot: TransferSnapshot) => void) { listeners.add(listener); listener(clone()); return () => { listeners.delete(listener); }; },
  async onOpenTransferRequest(_listener: (transferId: string) => void) { return () => {}; },
  async takePendingTransferRequest(): Promise<string | null> { return null; },
  async onOpenSystemShareRequest(_listener: (request: import('./types').SystemShareRequest) => void) { return () => {}; },
  async listSystemShareRequests(): Promise<import('./types').SystemShareRequest[]> { return []; },
  async submitSystemShareRequest(_requestId: string, _transferId: string, _peerId: string) {},
  async discardSystemShareRequest(_requestId: string) {},
  async pickTransferFiles(): Promise<string[]> {
    if (!scenario) throw Error('browser preview cannot access real file paths; add files in the desktop application');
    if (picked) return ['/preview/补充说明.pdf'];
    picked = true; return files.map(file => file.path);
  },
  async inspectTransferFiles(paths: string[]): Promise<TransferDraftFile[]> {
    if (!scenario) throw Error('browser preview cannot access real file paths; add files in the desktop application');
    return paths.map(path => ({ ...(files.find(file => file.path === path) ?? { path, canonicalPath: path, name: path.split('/').pop()!, size: 1024, modifiedAtMs: 1, kind: 'pdf', thumbnailDataUrl: null, error: null }), error: scenario === 'unreadable' && path === files[0].path ? 'File not found' : null }));
  },
  async sendTransfer(peerId: string, paths: string[], _requestIds: string[] = []): Promise<string> {
    if (!scenario) throw Error('Browser preview cannot send files');
    const peer = snapshot.peers.find(peer => peer.id === peerId); if (!peer) throw Error('Device offline');
    const batch = viewFiles(paths); const id = crypto.randomUUID();
    snapshot.transfers.unshift({ id, wireId: 1, peerId, peerName: peer.name, direction: 'sending', status: 'awaitingApproval', files: batch, totalBytes: batch.reduce((sum, file) => sum + file.size, 0), completedBytes: 0, speedBytesPerSecond: 0, remainingSeconds: null, errorMessage: null, createdAtMs: Date.now(), updatedAtMs: Date.now() });
    emit(); return id;
  },
  async respondTransfer(id: string, accepted: boolean, automatic: boolean) {
    const task = snapshot.transfers.find(task => task.id === id);
    if (accepted && automatic && task) snapshot.peers = snapshot.peers.map(peer => peer.id === task.peerId ? { ...peer, automaticReceive: true } : peer);
    update(id, accepted ? 'transferring' : 'rejected');
  },
  async pauseTransfer(id: string) { update(id, 'paused'); },
  async resumeTransfer(id: string) { update(id, 'transferring'); },
  async cancelTransfer(id: string) { update(id, 'cancelled'); },
  async chooseTransferReceiveDirectory() { if (scenario) { snapshot.receiveDirectory = '/Users/demo/Documents'; emit(); } return clone(); },
  async openTransferReceiveDirectory() { /* Browser simulation has no filesystem access. */ },
  async setTransferReceivePolicy(peerId: string, automatic: boolean) { snapshot.peers = snapshot.peers.map(peer => peer.id === peerId ? { ...peer, automaticReceive: automatic } : peer); emit(); return clone(); },
};
