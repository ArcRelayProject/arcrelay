import { expect, test } from "@playwright/test";

async function openClipboard(page: import("@playwright/test").Page) {
  await page.setViewportSize({ width: 700, height: 580 });
  await page.goto("/clipboard.html?lang=zhCn&theme=light");
  await expect(page.locator(".clipboard-row").first()).toBeVisible();
}

async function openHtml(page: import("@playwright/test").Page) {
  await openClipboard(page);
  const row = page.locator('[data-clipboard-id="103"] .clipboard-row');
  await row.focus();
  await row.press("Space");
  await expect(page.locator(".rendered-text h3")).toHaveText("设备列表");
}

test("HTML renders, double click selects, and source switching clears selection", async ({ page }) => {
  await openHtml(page);
  await page.locator(".rendered-text strong").dblclick();
  await expect(page.getByRole("button", { name: "插入选择", exact: true })).toBeVisible();
  await expect(page.locator(".text-selection-preview")).toBeVisible();
  await page.getByRole("button", { name: "源文本", exact: true }).click();
  await expect(page.locator(".text-preview-content pre")).toContainText("<h3>设备列表</h3>");
  await expect(page.getByRole("button", { name: "插入全部", exact: true })).toBeVisible();
  await page.screenshot({ path: "/tmp/arcrelay-clipboard-source.png", animations: "disabled" });
  await page.getByRole("button", { name: "预览", exact: true }).click();
  await page.getByRole("button", { name: "内容格式" }).click();
  await page.getByRole("option", { name: "普通文本", exact: true }).click();
  await expect(page.locator(".text-preview-content pre")).toContainText("<h3>");
  await page.getByRole("button", { name: "内容格式" }).click();
  await page.getByRole("option", { name: "HTML", exact: true }).click();
  await expect(page.locator(".rendered-text h3")).toBeVisible();
});

test("selection survives the insert button and is the exact pasted content", async ({ page }) => {
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await openHtml(page);
  const heading = await page.locator(".rendered-text h3").boundingBox();
  const strong = await page.locator(".rendered-text strong").boundingBox();
  expect(heading).not.toBeNull();
  expect(strong).not.toBeNull();
  await page.mouse.move(heading!.x + 1, heading!.y + heading!.height / 2);
  await page.mouse.down();
  await page.mouse.move(strong!.x + strong!.width + 1, strong!.y + strong!.height / 2, { steps: 12 });
  await page.mouse.up();
  await expect(page.getByRole("button", { name: "插入选择", exact: true })).toBeVisible();
  const selection = await page.evaluate(() => window.getSelection()!.toString());
  expect(selection).toContain("设备列表");
  expect(selection).toContain("MacBook Pro");
  await page.screenshot({ path: "/tmp/arcrelay-clipboard-preview.png", animations: "disabled" });
  await page.getByRole("button", { name: "插入选择", exact: true }).click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(selection);
  await expect(page.locator(".text-selection-preview")).toHaveCount(0);
});

test("scrolling reassigns badges and both focus modes paste the displayed record", async ({ page }) => {
  await openClipboard(page);
  // Spy on the browser adapter to verify actual shortcut routing by record ID.
  await page.evaluate(async () => {
    const { clipboardBridge } = await import("/src/clipboard/bridge.mock.ts");
    (window as any).__pasted = [];
    clipboardBridge.pasteAs = async (id: number) => { (window as any).__pasted.push(id); };
  });
  const firstBadgeId = () => page.locator('[data-clipboard-id]').evaluateAll(rows =>
    rows.find(row => row.querySelector('.item-number:not(.placeholder)')?.textContent === '1')?.getAttribute('data-clipboard-id'));
  const before = await firstBadgeId();
  await page.locator('.clipboard-list').evaluate(root => { root.scrollTop = root.scrollHeight; });
  await expect.poll(firstBadgeId).not.toBe(before);
  const id = await firstBadgeId();
  expect(id).toBeTruthy();
  const row = page.locator(`[data-clipboard-id="${id}"] .clipboard-row`);
  await row.focus();
  await row.press("Control+1");
  await expect.poll(() => page.evaluate(() => (window as any).__pasted)).toEqual([Number(id)]);
  await page.locator('.clipboard-header input').focus();
  await page.keyboard.press("Meta+1");
  await expect.poll(() => page.evaluate(() => (window as any).__pasted)).toEqual([Number(id), Number(id)]);
  await page.screenshot({ path: "/tmp/arcrelay-clipboard-numbers.png", animations: "disabled" });
});
