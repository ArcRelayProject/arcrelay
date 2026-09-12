import assert from 'node:assert/strict';
import test from 'node:test';
import { trayTransferPanelHeight } from './trayTransferPanelSize.ts';

test('expands a tray transfer panel from its clipped viewport to its intrinsic content', () => {
  assert.equal(trayTransferPanelHeight({ renderedHeight: 220, clientHeight: 218, scrollHeight: 512 }), 514);
});

test('keeps borders and caps oversized tray transfer content', () => {
  assert.equal(trayTransferPanelHeight({ renderedHeight: 520, clientHeight: 518, scrollHeight: 518 }), 520);
  assert.equal(trayTransferPanelHeight({ renderedHeight: 220, clientHeight: 218, scrollHeight: 900 }), 640);
});
