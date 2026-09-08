import { strict as assert } from 'node:assert';
import { test } from 'node:test';
import { observeWindowVisibility } from './windowVisibility.ts';

test('first open before listeners attach still loads the visible window', async () => {
  const states: boolean[] = [];
  let detached = 0;
  const stop = await observeWindowVisibility({
    onShown: async () => () => { detached++; },
    onHidden: async () => () => { detached++; },
    visible: async () => true,
  }, () => states.push(true), () => states.push(false));
  assert.deepEqual(states, [true]);
  stop();
  assert.equal(detached, 2);
});

test('hide during initial query cannot be overwritten by stale visible state', async () => {
  const states: boolean[] = [];
  let hide = () => {};
  await observeWindowVisibility({
    onShown: async () => () => {},
    onHidden: async handler => { hide = handler; return () => {}; },
    visible: async () => { hide(); return true; },
  }, () => states.push(true), () => states.push(false));
  assert.deepEqual(states, [false]);
});
