import { expect, test, type Page } from "@playwright/test";

async function installHistory(page: Page, blockInitialization = false) {
  await page.route("**/src/clipboard/bridge.mock.ts", async (route) => {
    const response = await route.fetch();
    await route.fulfill({
      response,
      body: `${await response.text()}
const fixture = window.__clipboardRefresh = { revision: 1, reads: 0, changed: null, hidden: null, shown: null, release: null, removed: [] };
const baseHistory = clipboardBridge.history;
const base = (await baseHistory({ search: '', kind: null, favoriteOnly: false, cursor: null })).entries.find(item => item.kind === 'text');
clipboardBridge.onChanged = async handler => { fixture.changed = handler; return () => { fixture.changed = null; }; };
clipboardBridge.onHidden = async handler => { fixture.hidden = handler; return () => {}; };
clipboardBridge.onShown = async handler => { fixture.shown = handler; return () => {}; };
clipboardBridge.history = async options => {
  fixture.reads++;
  const rows = Array.from({ length: 180 }, (_, i) => ({ ...base, id: 1000 + i, preview: 'record ' + i + (fixture.revision > 1 && i === 3 ? ' updated' : ''), updatedAtMs: Date.now() - i * 1000 })).filter(item => !fixture.removed.includes(item.id));
  if (fixture.revision > 1) rows.unshift({ ...base, id: 999, preview: 'new copy', updatedAtMs: Date.now(), sourceDeviceName: fixture.origin ?? null });
  const offset = options.cursor ? rows.findIndex(item => item.id === options.cursor.id) + 1 : 0;
  const entries = rows.slice(offset, offset + (options.limit ?? 60));
  const last = entries.at(-1);
  return { revision: fixture.revision, entries, totalCount: rows.length, nextCursor: offset + entries.length < rows.length ? { id: last.id, sortAtMs: last.updatedAtMs } : null };
};
clipboardBridge.timeline = async position => {
  const history = await clipboardBridge.history({ cursor: null, limit: 1000 });
  const index = history.entries.findIndex(item => item.id === position.id);
  const item = history.entries[index];
  const result = item ? { revision: history.revision, entries: history.entries.slice(Math.max(0, index - 5), index + 6), anchor: { id: item.id, sortAtMs: item.updatedAtMs }, newerCursor: null, olderCursor: null, totalCount: history.totalCount } : null;
  if (fixture.blockTimeline) {
    fixture.blockTimeline = false;
    await new Promise(resolve => { fixture.releaseTimeline = resolve; });
  }
  return result;
};
${blockInitialization ? "clipboardBridge.pinned = () => new Promise(resolve => { fixture.release = () => resolve(false); });" : ""}
`,
    });
  });
  await page.setViewportSize({ width: 700, height: 580 });
  await page.goto("/clipboard.html?lang=zhCn");
  await expect(page.locator('[data-clipboard-id="1000"]')).toBeVisible();
}

for (const origin of [null, "Remote desktop"]) {
  test(`a ${origin ? "remote" : "local"} copy refreshes before initialization finishes`, async ({
    page,
  }) => {
    await installHistory(page, true);
    await expect
      .poll(() => page.evaluate(() => Boolean((window as any).__clipboardRefresh.release)))
      .toBe(true);
    await page.evaluate((origin) => {
      const fixture = (window as any).__clipboardRefresh;
      fixture.origin = origin;
      fixture.revision++;
      fixture.changed?.();
    }, origin);
    await expect(page.locator('[data-clipboard-id="999"]')).toBeVisible();
    await expect(page.locator('[data-clipboard-id="1000"] .clipboard-row')).toHaveClass(/selected/);
    await page.evaluate(() => (window as any).__clipboardRefresh.release());
  });
}

test("a hidden copy is loaded when the window is shown", async ({ page }) => {
  await installHistory(page);
  await page.evaluate(() => {
    const fixture = (window as any).__clipboardRefresh;
    fixture.hidden();
    fixture.revision++;
    fixture.changed();
  });
  await expect(page.locator("[data-clipboard-id]")).toHaveCount(0);
  await page.evaluate(() => (window as any).__clipboardRefresh.shown());
  await expect(page.locator('[data-clipboard-id="999"]')).toBeVisible();
});

test("an update refreshes paginated rows and keeps the viewport", async ({ page }) => {
  await installHistory(page);
  await page.locator(".clipboard-list").evaluate((node) => {
    node.scrollTop = node.scrollHeight;
  });
  await expect
    .poll(() => page.evaluate(() => (window as any).__clipboardRefresh.reads))
    .toBeGreaterThanOrEqual(2);
  await page.locator(".clipboard-list").evaluate((node) => {
    node.scrollTop += 1200;
  });
  await expect(page.locator('[data-clipboard-id="1070"]')).toBeAttached();
  const id = await page.locator("[data-clipboard-id]").first().getAttribute("data-clipboard-id");
  const before = await page.locator(`[data-clipboard-id="${id}"]`).boundingBox();
  await page.evaluate(() => {
    const fixture = (window as any).__clipboardRefresh;
    fixture.revision++;
    fixture.changed();
  });
  await expect
    .poll(() => page.evaluate(() => (window as any).__clipboardRefresh.reads))
    .toBeGreaterThanOrEqual(4);
  await expect(page.locator(`[data-clipboard-id="${id}"]`)).toBeAttached();
  await expect
    .poll(async () =>
      Math.abs((await page.locator(`[data-clipboard-id="${id}"]`).boundingBox())!.y - before!.y),
    )
    .toBeLessThan(2);
});

test("deleting the selected row does not discard the update", async ({ page }) => {
  await installHistory(page);
  await page.evaluate(() => {
    const fixture = (window as any).__clipboardRefresh;
    fixture.removed = [1000];
    fixture.revision++;
    fixture.changed();
  });
  await expect(page.locator('[data-clipboard-id="1000"]')).toHaveCount(0);
  await expect(page.locator('[data-clipboard-id="999"]')).toBeVisible();
});

for (const duringNavigation of [false, true]) {
  test(`nearby records refresh automatically${duringNavigation ? " after an in-flight navigation" : ""}`, async ({
    page,
  }) => {
    await installHistory(page);
    const reads = await page.evaluate(() => (window as any).__clipboardRefresh.reads);
    await page.getByRole("textbox", { name: "⌘F", exact: true }).fill("record");
    await expect
      .poll(() => page.evaluate(() => (window as any).__clipboardRefresh.reads))
      .toBeGreaterThan(reads);
    await expect(page.locator(".loading-state")).toHaveCount(0);
    if (duringNavigation) {
      await page.evaluate(() => {
        (window as any).__clipboardRefresh.blockTimeline = true;
      });
    }
    await page.locator('[data-clipboard-id="1003"] .clipboard-row').click({ button: "right" });
    await page.getByRole("menuitem", { name: "查看附近记录", exact: true }).click();
    if (duringNavigation) {
      await expect
        .poll(() =>
          page.evaluate(() => Boolean((window as any).__clipboardRefresh.releaseTimeline)),
        )
        .toBe(true);
    } else {
      await expect(page.locator(".nearby-navigation")).toBeVisible();
    }
    await page.evaluate(() => {
      const fixture = (window as any).__clipboardRefresh;
      fixture.revision++;
      fixture.changed();
      fixture.releaseTimeline?.();
    });
    await expect(page.locator('[data-clipboard-id="1003"]')).toContainText("record 3 updated");
  });
}
