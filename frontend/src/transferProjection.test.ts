import test from 'node:test';
import assert from 'node:assert/strict';
import { TransferProjection } from './transferProjection.ts';
import type { TransferSnapshot, TransferProgress } from './types.ts';
const snapshot = (revision: number): TransferSnapshot => ({ revision, deviceId: 'local', deviceName: 'Local', receiveDirectory: '/tmp', discoverable: true, peers: [], transfers: ['a', 'b'].map(id => ({ id, wireId: 1, peerId: 'peer', peerName: 'Peer', direction: 'sending', status: 'transferring', totalBytes: 1000, completedBytes: 0, speedBytesPerSecond: 0, remainingSeconds: null, errorMessage: null, createdAtMs: 0, updatedAtMs: 0, files: [{ id: 1, name: 'a', relativePath: 'a', size: 1000, mediaType: 'text/plain', kind: 'file', thumbnailDataUrl: null, completedBytes: 0 }] })) } as TransferSnapshot);
const delta = (transferId: string, revision: number, baseRevision = 1): TransferProgress => ({ transferId, revision, baseRevision, completedBytes: revision * 100, speedBytesPerSecond: 100, remainingSeconds: 1, updatedAtMs: revision, files: [{ id: 1, completedBytes: revision * 100 }] });
test('out of order progress for different transfers does not lose either update', () => {
  const p = new TransferProjection();p.snapshot(snapshot(1));p.progress(delta('a',3));p.progress(delta('b',2));
  assert.deepEqual(p.value?.transfers.map(t=>t.completedBytes), [300,200]);
  assert.equal(p.value?.revision,3);
  p.progress(delta('a',2));assert.equal(p.value?.transfers[0].completedBytes,300);
});
test('progress waits for its required full baseline and survives an older query', () => {
  const p = new TransferProjection();
  assert.equal(p.progress(delta('a',5,3)),false);
  p.snapshot(snapshot(1));assert.equal(p.value?.transfers[0].completedBytes,0);
  p.snapshot(snapshot(3));assert.equal(p.value?.transfers[0].completedBytes,500);
  p.snapshot(snapshot(4));assert.equal(p.value?.transfers[0].completedBytes,500);
  p.snapshot(snapshot(6));assert.equal(p.value?.transfers[0].completedBytes,0);
});
