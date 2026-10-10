import { expect, test, type Page } from "@playwright/test";

async function openClipboard(page: Page, windows = false) {
  await page.setViewportSize({ width: 420, height: 858 });
  await page.goto(`/clipboard.html?lang=zhCn${windows ? "&preview-platform=windows" : ""}`);
  await expect(page.locator(".clipboard-row").first()).toBeVisible();
}
async function actions(page: Page, id: number) {
  await page.locator(`[data-clipboard-id="${id}"] .clipboard-row`).click({ button: "right" });
  const menu = page.locator(".clipboard-inline-menu");
  await expect(menu).toBeVisible();
  return menu;
}

for (const windows of [false, true]) {
  test(`application pinning keeps a unique prefix only in 最近 (${windows ? "Windows" : "default"})`, async ({
    page,
  }) => {
    await openClipboard(page, windows);
    const menu = await actions(page, 105);
    await menu.getByRole("menuitem", { name: "在 Safari 中置顶", exact: true }).click();
    await expect(page.locator("[data-clipboard-id]").first()).toHaveAttribute(
      "data-clipboard-id",
      "105",
    );
    await expect(page.locator('[data-clipboard-id="105"]')).toHaveCount(1);
    await expect(page.locator('[data-clipboard-id="105"] .application-pin')).toHaveAttribute(
      "title",
      "在 Safari 中置顶",
    );
    await page.getByRole("button", { name: "文本", exact: true }).click();
    await expect(page.locator("[data-clipboard-id]").first()).toHaveAttribute(
      "data-clipboard-id",
      "102",
    );
    await page.getByRole("button", { name: "最近", exact: true }).click();
    await (
      await actions(page, 105)
    )
      .getByRole("menuitem", { name: "取消在 Safari 中置顶", exact: true })
      .click();
    await expect(page.locator(".application-pin")).toHaveCount(0);
    await expect(page.locator("[data-clipboard-id]").first()).toHaveAttribute(
      "data-clipboard-id",
      "101",
    );
  });

  test(`frontend submenu closes DOM and host state before insertion (${windows ? "Windows" : "default"})`, async ({
    page,
  }) => {
    await openClipboard(page, windows);
    await page.evaluate(async () => {
      const { clipboardBridge } = await import("/src/clipboard/bridge.mock.ts");
      (window as any).__menuEvents = [];
      clipboardBridge.setContextMenuOpen = async (open: boolean) => {
        (window as any).__menuEvents.push(["menu", open]);
      };
      clipboardBridge.pasteAs = async (id: number, mode: string) => {
        (window as any).__menuEvents.push([
          "paste",
          id,
          mode,
          Boolean(document.querySelector(".clipboard-inline-menu")),
        ]);
      };
    });
    const menu = await actions(page, 103);
    await menu.getByRole("menuitem", { name: "粘贴为", exact: true }).click();
    const bounds = (await menu.boundingBox())!;
    expect(bounds.x).toBeGreaterThanOrEqual(8);
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(412);
    expect(bounds.y + bounds.height).toBeLessThanOrEqual(850);
    await menu.getByRole("menuitem", { name: "纯文本", exact: true }).click();
    await expect(menu).toHaveCount(0);
    await expect
      .poll(() => page.evaluate(() => (window as any).__menuEvents))
      .toEqual([
        ["menu", true],
        ["menu", false],
        ["paste", 103, "plain_text", false],
      ]);
  });
}

test("switching applications updates the prefix without confusing copied source and pin target", async ({
  page,
}) => {
  await openClipboard(page);
  await (
    await actions(page, 105)
  )
    .getByRole("menuitem", { name: "在 Safari 中置顶", exact: true })
    .click();
  await page.evaluate(async () => {
    const { clipboardBridge } = await import("/src/clipboard/bridge.mock.ts");
    clipboardBridge.targetApplication = async () => ({ id: "mock:code", name: "VS Code" });
  });
  await expect(page.locator(".application-pin")).toHaveCount(0);
  await (
    await actions(page, 103)
  )
    .getByRole("menuitem", { name: "在 VS Code 中置顶", exact: true })
    .click();
  await expect(page.locator("[data-clipboard-id]").first()).toHaveAttribute(
    "data-clipboard-id",
    "103",
  );
  await expect(page.locator('[data-clipboard-id="103"] .clipboard-meta > strong')).toHaveText(
    "Microsoft Edge",
  );
  await page.evaluate(async () => {
    const { clipboardBridge } = await import("/src/clipboard/bridge.mock.ts");
    clipboardBridge.targetApplication = async () => ({ id: "mock:safari", name: "Safari" });
  });
  await expect(page.locator("[data-clipboard-id]").first()).toHaveAttribute(
    "data-clipboard-id",
    "105",
  );
});
