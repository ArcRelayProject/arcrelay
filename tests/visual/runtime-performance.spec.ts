import { expect, test } from "@playwright/test";

test("large remote directories keep each view bounded while scrolling to the final entry", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/?visual=1&page=files&lang=zhCn&theme=light");
  await expect(page.locator(".remote-file-row").first()).toBeVisible();
  await page.evaluate(async () => {
    // Exercise the actual component with an already loaded large directory.
    // @ts-expect-error Browser-side Vite module URL.
    const { bridge } = await import("/src/bridge.mock.ts");
    bridge.listRemoteDirectory = async () => ({
      entries: Array.from({ length: 10_000 }, (_, index) => ({
        name: `entry-${String(index).padStart(5, "0")}.txt`,
        relativePath: `entry-${String(index).padStart(5, "0")}.txt`,
        kind: "file",
        size: index,
        modifiedAtMs: 10_000 - index,
      })),
      nextCursor: null,
    });
  });
  await page.getByRole("button", { name: "刷新当前文件夹", exact: true }).click();
  await expect(page.locator(".remote-file-row").first()).toContainText("entry-00000.txt");
  for (const mode of ["详细信息", "紧凑列表", "大图标"]) {
    await page.getByRole("button", { name: mode, exact: true }).click();
    const rows = page.locator(mode === "详细信息" ? ".remote-file-row" : ".remote-grid-item");
    const scroller = page.locator(mode === "详细信息" ? ".remote-table-body" : ".remote-grid-body");
    await expect.poll(() => rows.count()).toBeGreaterThan(0);
    expect(await rows.count()).toBeLessThan(150);
    await scroller.evaluate((node) => {
      node.scrollTop = node.scrollHeight;
    });
    await expect(rows.last()).toContainText("entry-09999.txt");
    expect(await rows.count()).toBeLessThan(150);
    const geometry = await scroller.evaluate((node) => ({
      height: node.clientHeight,
      top: node.scrollTop,
      total: node.scrollHeight,
    }));
    expect(geometry.top + geometry.height).toBeGreaterThanOrEqual(geometry.total - 2);
  }
  expect(errors).toEqual([]);
});

test("thumbnail requests are limited to four and follow the current viewport", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto("/?visual=1&page=files&lang=zhCn&theme=light");
  await expect(page.locator(".remote-file-row").first()).toBeVisible();
  await page.evaluate(async () => {
    // @ts-expect-error Browser-side Vite module URL.
    const { bridge } = await import("/src/bridge.mock.ts");
    const state = { requested: [] as string[], complete: [] as Array<() => void> };
    Object.assign(window, { thumbnailTest: state });
    bridge.listRemoteDirectory = async () => ({
      entries: Array.from({ length: 1_000 }, (_, index) => ({
        name: `image-${String(index).padStart(4, "0")}.png`,
        relativePath: `image-${String(index).padStart(4, "0")}.png`,
        kind: "file",
        size: 100,
        modifiedAtMs: 1_000 - index,
      })),
      nextCursor: null,
    });
    bridge.getRemoteFileThumbnail = async (_peer: string, _share: string, path: string) => {
      state.requested.push(path);
      await new Promise<void>((resolve) => state.complete.push(resolve));
      return null;
    };
  });
  await page.getByRole("button", { name: "刷新当前文件夹", exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).thumbnailTest.requested.length))
    .toBe(4);
  await page.locator(".remote-table-body").evaluate((node) => {
    node.scrollTop = node.scrollHeight;
  });
  await expect(page.locator(".remote-file-row").last()).toContainText("image-0999.png");
  await page.evaluate(() => {
    for (const finish of (window as any).thumbnailTest.complete.splice(0)) finish();
  });
  await expect
    .poll(() => page.evaluate(() => (window as any).thumbnailTest.requested.length))
    .toBe(8);
  const following = await page.evaluate(
    () => (window as any).thumbnailTest.requested.slice(4) as string[],
  );
  expect(following.every((path) => Number(path.slice(6, 10)) > 900)).toBe(true);
});

test("activity events update their row without rereading or rechecking automations", async ({
  page,
}) => {
  await page.goto("/?visual=1&page=actions&automationScenario=dashboard&lang=zhCn&theme=light");
  await expect(page.locator("html")).toHaveAttribute("data-visual-ready", "true");
  await page.evaluate(async () => {
    // @ts-expect-error Browser-side Vite module URL.
    const { bridge } = await import("/src/bridge.mock.ts");
    const activity = (await bridge.listAutomationActivities())[0];
    const state = {
      activity,
      counts: {} as Record<string, number>,
      emit: (_activity: unknown) => {},
      configure: () => {},
    };
    Object.assign(window, { automationTest: state });
    for (const name of [
      "listAutomations",
      "listAutomationActivities",
      "automationCapabilities",
      "checkAutomations",
    ]) {
      const original = bridge[name].bind(bridge);
      state.counts[name] = 0;
      bridge[name] = (...args: unknown[]) => {
        state.counts[name]++;
        return original(...args);
      };
    }
    bridge.onAutomationActivity = async (callback: typeof state.emit) => {
      state.emit = callback;
      return () => {};
    };
    bridge.onAutomationConfiguration = async (callback: typeof state.configure) => {
      state.configure = callback;
      return () => {};
    };
  });
  await page.getByRole("button", { name: "自动化", exact: true }).click();
  await expect(page.getByRole("heading", { name: "自动化", exact: true })).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => (window as any).automationTest.counts.checkAutomations))
    .toBe(1);
  const before = await page.evaluate(() => ({ ...(window as any).automationTest.counts }));
  await page.getByRole("button", { name: /^活动记录/ }).click();
  await page.evaluate(() => {
    const state = (window as any).automationTest;
    for (let index = 0; index < 20; index++)
      state.emit({ ...state.activity, status: "failed", reason: `Local progress ${index}` });
  });
  await expect(page.locator(".au-activity-list")).toContainText("Local progress 19");
  // Wait beyond the old debounce so a regressed full refresh cannot pass this assertion.
  await page.waitForTimeout(350);
  expect(await page.evaluate(() => (window as any).automationTest.counts)).toEqual(before);
  await page.evaluate(() => {
    for (let i = 0; i < 20; i++) (window as any).automationTest.configure();
  });
  await expect
    .poll(() => page.evaluate(() => (window as any).automationTest.counts.listAutomations))
    .toBe(before.listAutomations + 1);
  await page.waitForTimeout(200);
  const after = await page.evaluate(() => (window as any).automationTest.counts);
  expect(after.listAutomationActivities).toBe(before.listAutomationActivities + 1);
  expect(after.automationCapabilities).toBe(before.automationCapabilities + 1);
  expect(after.checkAutomations).toBe(before.checkAutomations);
});
