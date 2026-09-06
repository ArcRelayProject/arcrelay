import test from 'node:test';
import assert from 'node:assert/strict';
import { DraftError, TransferDraft } from './transferDraft.ts';
import type { TransferDraftFile, TransferPeer, TransferSnapshot, TransferStatus } from './types.ts';

const peer = (id: string): TransferPeer => ({ id, name: id, paired: true, automaticReceive: false, platform: 'macOS', model: 'Mac', address: '127.0.0.1', addresses: [], port: 1, publicKey: '', certificateSha256: '', lastSeenAtMs: 0 });
const metadata = (path: string): TransferDraftFile => ({ path, canonicalPath: path, name: path.split('/').pop()!, size: 100, modifiedAtMs: 1, kind: 'file', thumbnailDataUrl: null, error: null });
const snapshot = (peers = ['a', 'b'], id?: string, status: TransferStatus = 'transferring'): TransferSnapshot => ({
  revision: 1, deviceId: 'local', deviceName: 'local', discoverable: true, receiveDirectory: '/tmp', peers: peers.map(peer),
  transfers: id ? [{ id, wireId: 1, peerId: 'b', peerName: 'b', direction: 'sending', status, files: [], totalBytes: 100, completedBytes: 0, speedBytesPerSecond: 0, remainingSeconds: null, errorMessage: null, createdAtMs: 1, updatedAtMs: 1 }] : [],
});
function fixture(inspect = async (paths: string[]) => paths.map(metadata)) {
  const sent: Array<{ peerId: string; paths: string[]; requestIds: string[] }> = [];
  const draft = new TransferDraft({ inspect, send: async (peerId, paths, requestIds) => { sent.push({ peerId, paths, requestIds }); return `task-${sent.length}`; } });
  draft.updateNetwork(snapshot());
  return { draft, sent };
}
const deferred = <T>() => { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { promise, resolve }; };
const code = (expected: DraftError['code']) => (error: unknown) => error instanceof DraftError && error.code === expected;

test('adding files stages only, including without peers, and deduplicates canonical paths', async () => {
  const { draft, sent } = fixture(async paths => paths.map(path => ({ ...metadata(path), canonicalPath: path.replace('/alias/', '/real/') })));
  draft.updateNetwork(snapshot([]));
  await draft.add(['/real/one', '/real/one', '/alias/one', '/real/two']);
  assert.deepEqual(draft.state.files.map(file => file.path), ['/real/one', '/real/two']);
  assert.equal(sent.length, 0);
  assert.equal(draft.state.submitting, null);
});
test('same names in different directories remain separate', async () => {
  const { draft } = fixture(); await draft.add(['/a/file', '/b/file']); assert.equal(draft.state.files.length, 2);
});
test('removing or clearing while inspection is pending never resurrects files', async () => {
  const wait = deferred<TransferDraftFile[]>();
  const { draft } = fixture(() => wait.promise);
  const add = draft.add(['/one', '/two']);
  assert.ok(draft.state.files.every(file => file.preparing));
  draft.remove(draft.state.files[0].id); draft.clear();
  wait.resolve([metadata('/one'), metadata('/two')]); await add;
  assert.equal(draft.state.files.length, 0);
  draft.undoClear();
  assert.equal(draft.state.files[0].path, '/two');
  assert.equal(draft.state.files[0].preparing, false);
});
test('failed inspection marks each remaining entry and blocks sending', async () => {
  const { draft, sent } = fixture(async () => { throw Error('permission denied'); });
  await draft.add(['/one']);
  assert.equal(draft.state.files[0].error, 'permission denied');
  await assert.rejects(draft.sendTo('b'), code('invalid')); assert.equal(sent.length, 0);
});
test('file limit rejects a batch without silently truncating it', async () => {
  const { draft } = fixture(); await assert.rejects(draft.add(Array.from({ length: 257 }, (_, i) => `/${i}`)), code('limit'));
  assert.equal(draft.state.files.length, 0);
});
test('clear undo merges new files without deleting or duplicating them', async () => {
  const { draft } = fixture(); await draft.add(['/one']); draft.clear(); await draft.add(['/two']); draft.undoClear();
  assert.deepEqual(new Set(draft.state.files.map(file => file.path)), new Set(['/one', '/two']));
});
test('a disappearing target during revalidation never falls back to the first peer', async () => {
  const wait = deferred<TransferDraftFile[]>(); let count = 0;
  const { draft, sent } = fixture(paths => ++count === 1 ? Promise.resolve(paths.map(metadata)) : wait.promise);
  await draft.add(['/one']); const sending = draft.sendTo('b'); draft.updateNetwork(snapshot(['a']));
  wait.resolve([metadata('/one')]); await assert.rejects(sending, code('offline'));
  assert.equal(sent.length, 0); assert.equal(draft.state.files.length, 1);
});
test('double submission is blocked and a new draft survives completion', async () => {
  const wait = deferred<string>(); const sent: string[][] = [];
  const draft = new TransferDraft({ inspect: async paths => paths.map(metadata), send: async (_peer, paths) => { sent.push(paths); return wait.promise; } });
  draft.updateNetwork(snapshot()); await draft.add(['/one']);
  const sending = draft.sendTo('b');
  await assert.rejects(draft.sendTo('a'), code('busy'));
  await draft.add(['/two']); wait.resolve('job'); await sending;
  assert.deepEqual(sent, [['/one']]); assert.deepEqual(draft.state.files.map(file => file.path), ['/two']);
  assert.equal(draft.state.batches.job.peerId, 'b');
  draft.updateNetwork(snapshot(['a', 'b'], 'job', 'completed'));
  assert.deepEqual(draft.state.files.map(file => file.path), ['/two']);
});
test('failed task creation preserves the entire draft', async () => {
  const draft = new TransferDraft({ inspect: async paths => paths.map(metadata), send: async () => { throw Error('network failed'); } });
  draft.updateNetwork(snapshot()); await draft.add(['/one']);
  await assert.rejects(draft.sendTo('b'), /network failed/);
  assert.equal(draft.state.files.length, 1); assert.equal(draft.state.submitting, null);
});
test('incoming snapshots do not submit or consume draft files', async () => {
  const { draft, sent } = fixture(); await draft.add(['/one']); const next = snapshot(['a', 'b'], 'incoming', 'awaitingApproval'); next.transfers[0].direction = 'receiving';
  draft.updateNetwork(next); assert.equal(draft.state.files.length, 1); assert.equal(sent.length, 0);
  await draft.sendTo('b'); assert.equal(sent[0].peerId, 'b');
});
test('retry uses the original batch and peer, not current draft or first device', async () => {
  const { draft, sent } = fixture(); await draft.add(['/one']); const id = await draft.sendTo('b');
  draft.updateNetwork(snapshot(['a', 'b'], id, 'failed')); await draft.add(['/two']); await draft.retry(id);
  assert.deepEqual(sent, [{ peerId: 'b', paths: ['/one'], requestIds: [] }, { peerId: 'b', paths: ['/one'], requestIds: [] }]);
  assert.deepEqual(draft.state.files.map(file => file.path), ['/two']);
});
test('retry refuses an offline original peer and unknown source references', async () => {
  const { draft, sent } = fixture(); await draft.add(['/one']); const id = await draft.sendTo('b');
  draft.updateNetwork(snapshot(['a'], id, 'cancelled'));
  await assert.rejects(draft.retry(id), code('offline')); await assert.rejects(draft.retry('missing'), code('unavailable'));
  assert.equal(sent.length, 1);
});
test('changed source metadata requires a fresh explicit confirmation', async () => {
  let size = 100; const { draft, sent } = fixture(async paths => paths.map(path => ({ ...metadata(path), size })));
  await draft.add(['/one']); size = 200;
  await assert.rejects(draft.sendTo('b'), code('changed'));
  assert.equal(sent.length, 0); assert.equal(draft.state.files[0].size, 200);
  await draft.sendTo('b'); assert.equal(sent.length, 1);
});
test('subscriptions can remount without losing the session draft', async () => {
  const { draft } = fixture(); const stop = draft.subscribe(() => {}); await draft.add(['/one']); stop();
  let count = 0; const nextStop = draft.subscribe(value => { count = value.files.length; }); assert.equal(count, 1); nextStop();
});
test('system share batches stay isolated for automatic targeted sends', async () => {
  const { draft, sent } = fixture();
  await draft.add(['/manual']);
  await draft.add(['/system-a', '/system-b'], 'share-1');
  const id = await draft.sendSystemRequestTo('share-1', 'b');
  assert.deepEqual(sent, [{ peerId: 'b', paths: ['/system-a', '/system-b'], requestIds: ['share-1'] }]);
  assert.deepEqual(draft.state.files.map(file => file.path), ['/manual']);
  assert.deepEqual(draft.systemRequestIds(id), ['share-1']);
});
test('overlapping system shares retain the other request and manual intent', async () => {
  const { draft, sent } = fixture();
  await draft.add(['/same'], 'share-2');
  await draft.add(['/same'], 'share-1');
  const id = await draft.sendSystemRequestTo('share-1', 'b');
  assert.deepEqual(sent, [{ peerId: 'b', paths: ['/same'], requestIds: ['share-1'] }]);
  assert.equal(draft.hasSystemRequest('share-1'), false);
  assert.equal(draft.hasSystemRequest('share-2'), true);
  assert.deepEqual(draft.systemRequestIds(id), ['share-1']);

  const manual = fixture();
  await manual.draft.add(['/manual']);
  await manual.draft.add(['/manual'], 'share-3');
  await manual.draft.sendSystemRequestTo('share-3', 'b');
  assert.deepEqual(manual.draft.state.files.map(file => file.path), ['/manual']);
  assert.equal(manual.draft.systemRequestIds().length, 0);
});
