import { test } from 'node:test';
import assert from 'node:assert/strict';
import { autoFocusSearch, isWindowsClipboard } from './focusPolicy.ts';

test('Windows native clipboard overrides old auto-focus preferences, not other platforms', () => {
  assert.equal(autoFocusSearch(isWindowsClipboard('Win32', true), true), false);
  assert.equal(autoFocusSearch(isWindowsClipboard('Win32', true), undefined), false);
  assert.equal(autoFocusSearch(isWindowsClipboard('MacIntel', true), true), true);
  assert.equal(autoFocusSearch(isWindowsClipboard('Linux', true), false), false);
  assert.equal(autoFocusSearch(isWindowsClipboard('Win32', false), true), true);
});
