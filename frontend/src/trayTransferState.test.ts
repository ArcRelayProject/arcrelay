import test from 'node:test';
import assert from 'node:assert/strict';
import { trayTransferProgress, trayTransferStatus } from './trayTransferState.ts';

test('all bytes sent is not a delivery confirmation', () => {
  const transferring = { status: 'transferring' as const, totalBytes: 84, completedBytes: 84 };
  assert.equal(trayTransferProgress(transferring), 100);
  assert.equal(trayTransferStatus(transferring.status), '正在传输');
  assert.equal(trayTransferStatus('completed'), '文件已送达');
  assert.equal(trayTransferProgress({ status: 'completed', totalBytes: 0, completedBytes: 0 }), 100);
});

test('empty and out-of-order counters cannot yield NaN or an invalid progress bar', () => {
  assert.equal(trayTransferProgress({ status: 'preparing', totalBytes: 0, completedBytes: 0 }), 0);
  assert.equal(trayTransferProgress({ status: 'transferring', totalBytes: 100, completedBytes: 120 }), 100);
  assert.equal(trayTransferProgress({ status: 'transferring', totalBytes: 100, completedBytes: -2 }), 0);
});
