import { expect, test, type Page } from '@playwright/test';

async function openWindowsClipboard(page: Page) {
  await page.setViewportSize({ width: 540, height: 700 });
  await page.goto('/clipboard.html?lang=zhCn&preview-platform=windows');
  await expect(page.locator('.clipboard-row').first()).toBeVisible();
}

async function nativeKey(page: Page, key: string, code: string, options: Record<string, unknown> = {}) {
  await page.evaluate(({ key, code, options }) => {
    window.dispatchEvent(new CustomEvent('mock-clipboard-navigation', { detail: {
      key, code, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false,
      repeat: false, generation: 0, ...options,
    } }));
  }, { key, code, options });
}

test('Windows overrides saved auto-search focus and routes hook keys by displayed rank', async ({ page }) => {
  await openWindowsClipboard(page);
  await expect(page.locator('.clipboard-header input')).not.toBeFocused();
  await page.evaluate(async () => {
    const { clipboardBridge } = await import('/src/clipboard/bridge.mock.ts');
    (window as any).__pastes = [];
    clipboardBridge.pasteAs = async (id: number, mode: string) => { (window as any).__pastes.push({ id, mode }); };
  });
  const id = await page.locator('[data-clipboard-id]').evaluateAll(rows =>
    rows.find(row => row.querySelector('.item-number:not(.placeholder)')?.textContent === '1')?.getAttribute('data-clipboard-id'));
  await nativeKey(page, '1', 'Digit1', { ctrlKey: true });
  await expect.poll(() => page.evaluate(() => (window as any).__pastes)).toEqual([{ id: Number(id), mode: 'source' }]);
  await nativeKey(page, '1', 'Digit1', { ctrlKey: true, repeat: true });
  await nativeKey(page, '1', 'Digit1', { ctrlKey: true, generation: 999 });
  await expect.poll(() => page.evaluate(() => (window as any).__pastes.length)).toBe(1);
});

test('Windows inline menu retains formats and stays inside a narrow viewport', async ({ page }) => {
  await openWindowsClipboard(page);
  await page.setViewportSize({ width: 420, height: 580 });
  await page.evaluate(async () => {
    const { clipboardBridge } = await import('/src/clipboard/bridge.mock.ts');
    (window as any).__pastes = [];
    clipboardBridge.pasteAs = async (id: number, mode: string) => { (window as any).__pastes.push({ id, mode }); };
  });
  await page.locator('[data-clipboard-id="103"] .clipboard-row').click({ button: 'right' });
  const menu = page.locator('.clipboard-inline-menu');
  await expect(menu).toBeVisible();
  await menu.getByRole('menuitem', { name: '粘贴为', exact: true }).click();
  await expect(menu.getByRole('menuitem', { name: '纯文本', exact: true })).toBeVisible();
  const bounds = (await menu.boundingBox())!;
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.y).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(420);
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(580);
  await menu.getByRole('menuitem', { name: '纯文本', exact: true }).click();
  await expect(menu).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => (window as any).__pastes)).toEqual([{ id: 103, mode: 'plain_text' }]);
});

test('explicit search enters editing and Tab returns to nonactivating results', async ({ page }) => {
  await openWindowsClipboard(page);
  await page.evaluate(async () => {
    const { clipboardBridge } = await import('/src/clipboard/bridge.mock.ts');
    (window as any).__editing = [];
    clipboardBridge.setEditing = async (editing: boolean) => { (window as any).__editing.push(editing); return 0; };
  });
  await nativeKey(page, 'f', 'KeyF', { ctrlKey: true });
  const search = page.locator('.clipboard-header input');
  await expect(search).toBeFocused();
  await expect.poll(() => page.evaluate(() => (window as any).__editing.includes(true))).toBe(true);
  await search.fill('设备');
  await expect(search).toHaveValue('设备');
  await search.press('Tab');
  await expect(search).not.toBeFocused();
  await expect.poll(() => page.evaluate(() => (window as any).__editing.at(-1))).toBe(false);
});

test('pause closes menus and repeated Space does not toggle a preview', async ({ page }) => {
  await openWindowsClipboard(page);
  await nativeKey(page, ' ', 'Space');
  await expect(page.locator('.clipboard-preview-dialog')).toBeVisible();
  await nativeKey(page, ' ', 'Space', { repeat: true });
  await expect(page.locator('.clipboard-preview-dialog')).toBeVisible();
  await nativeKey(page, ' ', 'Space');
  await expect(page.locator('.clipboard-preview-dialog')).toHaveCount(0);
  await page.locator('.clipboard-row').first().click({ button: 'right' });
  await expect(page.locator('.clipboard-inline-menu')).toBeVisible();
  await page.evaluate(() => window.dispatchEvent(new Event('mock-clipboard-navigation-paused')));
  await expect(page.locator('.clipboard-inline-menu')).toHaveCount(0);
  await expect(page.locator('.clipboard-header input')).not.toBeFocused();
});

test('failed activation does not focus search, and input handoffs do not restore the destination', async ({ page }) => {
  await openWindowsClipboard(page);
  await page.evaluate(async () => {
    const { clipboardBridge } = await import('/src/clipboard/bridge.mock.ts');
    clipboardBridge.setEditing = async () => { throw new Error('activation rejected'); };
  });
  await nativeKey(page, 'f', 'KeyF', { ctrlKey: true });
  await expect(page.getByText('activation rejected', { exact: false })).toBeVisible();
  await expect(page.locator('.clipboard-header input')).not.toBeFocused();
  await page.evaluate(async () => {
    const { clipboardBridge } = await import('/src/clipboard/bridge.mock.ts');
    (window as any).__editing = [];
    clipboardBridge.setEditing = async (editing: boolean) => { (window as any).__editing.push(editing); return 0; };
    const other = document.createElement('input');
    other.id = 'focus-handoff-test';
    other.style.cssText = 'position:fixed;bottom:10px;left:10px;z-index:2000';
    document.body.append(other);
  });
  await nativeKey(page, 'f', 'KeyF', { ctrlKey: true });
  await expect(page.locator('.clipboard-header input')).toBeFocused();
  await page.locator('#focus-handoff-test').click();
  await expect(page.locator('#focus-handoff-test')).toBeFocused();
  await expect.poll(() => page.evaluate(() => (window as any).__editing.includes(false))).toBe(false);
});
