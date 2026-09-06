import test from 'node:test';
import assert from 'node:assert/strict';
import { DesktopProjection, type DesktopStateUpdate } from './desktopProjection.ts';
import type { BootstrapState } from './types.ts';
const runtime = (revision: number) => ({ revision, connectedDevices: [{ id: String(revision) }] } as DesktopStateUpdate);
const full = (revision: number) => ({ ...runtime(revision), actions: [{ id: String(revision) }] } as BootstrapState);

test('runtime arriving before bootstrap retains the newer device state', () => {
  const p = new DesktopProjection();
  assert.equal(p.apply(runtime(3)), null);
  const value = p.apply(full(2));
  assert.equal(value?.connectedDevices[0].id, '3');
  assert.equal(value?.actions[0].id, '2');
});
test('late catalog events still update actions after a newer runtime event', () => {
  const p = new DesktopProjection();
  p.apply(full(1)); p.apply(runtime(4));
  const value = p.apply(full(3));
  assert.equal(value?.connectedDevices[0].id, '4');
  assert.equal(value?.actions[0].id, '3');
  assert.equal(p.apply(full(2)), null);
});
