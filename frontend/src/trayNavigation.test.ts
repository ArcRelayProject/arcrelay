import assert from 'node:assert/strict';
import test from 'node:test';
import { parseTrayDestination, shouldAutoSendSystemShare } from './trayNavigation.ts';

test('tray navigation preserves exact peer identity and rejects incomplete destinations', () => {
  for (const page of ['settings', 'devices', 'transfers'] as const) assert.deepEqual(parseTrayDestination(page), { page });
  assert.deepEqual(parseTrayDestination('files:peer:one'), { page: 'files', peerId: 'peer:one' });
  for (const value of [null, '', 'files:', 'unknown']) assert.equal(parseTrayDestination(value), null);
});

test('a tray recipient never bypasses confirmation or falls back to another device', () => {
  assert.equal(shouldAutoSendSystemShare({ source: 'tray', targetPeerId: 'phone' }), false);
  assert.equal(shouldAutoSendSystemShare({ source: 'tray', targetPeerId: null }), false);
  for (const source of ['windowsContextMenu', 'windowsShareTarget', 'macosShareExtension', 'linuxFileManager', 'commandLine'] as const) {
    assert.equal(shouldAutoSendSystemShare({ source, targetPeerId: 'phone' }), true);
    assert.equal(shouldAutoSendSystemShare({ source, targetPeerId: null }), false);
  }
});
