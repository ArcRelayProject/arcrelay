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

test("image preview omits the header and Space closes from a focused image tool", async ({
  page,
}) => {
  await openClipboard(page);
  const row = page.locator('[data-clipboard-id="101"] .clipboard-row');
  await row.focus();
  await row.press("Space");
  const dialog = page.getByRole("dialog", { name: "剪贴板预览", exact: true });
  await expect(dialog).toBeVisible();
  await expect(dialog.locator(".clipboard-preview-header")).toHaveCount(0);
  for (const name of ["编辑", "粘贴为 JPG", "粘贴为 PNG", "插入", "取消"]) {
    await expect(dialog.getByRole("button", { name, exact: true })).toHaveCount(0);
  }
  await expect(dialog.getByRole("button", { name: "拖出原图", exact: true })).toBeVisible();
  const zoom = dialog.getByRole("button", { name: "放大", exact: true });
  await zoom.click();
  await expect(dialog.locator(".image-preview-zoom")).toHaveText("125%");
  await page.setViewportSize({ width: 420, height: 580 });
  await page.screenshot({
    path: "/tmp/arcrelay-clipboard-image-preview.png",
    animations: "disabled",
  });
  await zoom.focus();
  await page.keyboard.down("Space");
  await expect(dialog).toHaveCount(0);
  // Holding the closing key must not reopen the preview on key repeat.
  await row.focus();
  await page.keyboard.down("Space");
  await expect(dialog).toHaveCount(0);
  await page.keyboard.up("Space");
  await row.press("Space");
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Space");
  await expect(dialog).toHaveCount(0);
});

test("Space closes text preview instead of activating its focused toolbar button", async ({
  page,
}) => {
  await openHtml(page);
  await page.getByRole("button", { name: "源文本", exact: true }).focus();
  await page.keyboard.press("Space");
  await expect(page.locator(".clipboard-preview-dialog")).toHaveCount(0);
});

for (const tag of ["input", "textarea", "select", "div"]) {
  test(`Space closes before rendered ${tag} content can consume the event`, async ({ page }) => {
    await openHtml(page);
    await page.locator(".rendered-text").evaluate((root, tag) => {
      const control = document.createElement(tag);
      control.id = "preview-space-consumer";
      if (tag === "div") control.contentEditable = "true";
      (window as any).__previewSpaceConsumed = false;
      control.addEventListener(
        "keydown",
        (event) => {
          if (event.code !== "Space") return;
          (window as any).__previewSpaceConsumed = true;
          event.preventDefault();
          event.stopImmediatePropagation();
        },
        true,
      );
      root.append(control);
      control.focus();
    }, tag);
    await expect(page.locator("#preview-space-consumer")).toBeFocused();
    await page.keyboard.press("Space");
    await expect(page.locator(".clipboard-preview-dialog")).toHaveCount(0);
    expect(await page.evaluate(() => (window as any).__previewSpaceConsumed)).toBe(false);
  });
}

test("HTML renders, double click selects, and source switching clears selection", async ({
  page,
}) => {
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
  await page.mouse.move(strong!.x + strong!.width + 1, strong!.y + strong!.height / 2, {
    steps: 12,
  });
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

test("scrolling reassigns badges and both focus modes paste the displayed record", async ({
  page,
}) => {
  await openClipboard(page);
  // Rows mounted by virtual scrolling must paint immediately instead of fading in.
  await expect
    .poll(() =>
      page
        .locator(".clipboard-item")
        .first()
        .evaluate((row) => getComputedStyle(row).animationName),
    )
    .toBe("none");
  // Spy on the browser adapter to verify actual shortcut routing by record ID.
  await page.evaluate(async () => {
    const { clipboardBridge } = await import("/src/clipboard/bridge.mock.ts");
    (window as any).__pasted = [];
    clipboardBridge.pasteAs = async (id: number) => {
      (window as any).__pasted.push(id);
    };
  });
  const firstBadgeId = () =>
    page
      .locator("[data-clipboard-id]")
      .evaluateAll((rows) =>
        rows
          .find((row) => row.querySelector(".item-number:not(.placeholder)")?.textContent === "1")
          ?.getAttribute("data-clipboard-id"),
      );
  const before = await firstBadgeId();
  await page.locator(".clipboard-list").evaluate((root) => {
    root.scrollTop = root.scrollHeight;
  });
  await expect.poll(firstBadgeId).not.toBe(before);
  const id = await firstBadgeId();
  expect(id).toBeTruthy();
  const row = page.locator(`[data-clipboard-id="${id}"] .clipboard-row`);
  await row.focus();
  await row.press("Control+1");
  await expect.poll(() => page.evaluate(() => (window as any).__pasted)).toEqual([Number(id)]);
  await page.locator(".clipboard-header input").focus();
  await page.keyboard.press("Meta+1");
  await expect
    .poll(() => page.evaluate(() => (window as any).__pasted))
    .toEqual([Number(id), Number(id)]);
  await page.screenshot({ path: "/tmp/arcrelay-clipboard-numbers.png", animations: "disabled" });
});

test("fast jumps through a long history leave no empty viewport", async ({ page }) => {
  await openClipboard(page);
  await page.evaluate(async () => {
    const { clipboardBridge } = await import("/src/clipboard/bridge.mock.ts");
    const initial = await clipboardBridge.history({
      search: "",
      kind: null,
      favoriteOnly: false,
      cursor: null,
    });
    const example = initial.entries.find((item) => item.kind === "text")!;
    const entries = Array.from({ length: 120 }, (_, index) => ({
      ...example,
      id: 10_000 + index,
      syncId: `scroll-${index}`,
      preview: `Scroll record ${index}`,
      updatedAtMs: example.updatedAtMs - index * 1000,
    }));
    clipboardBridge.history = async () => ({
      revision: 2,
      entries,
      nextCursor: null,
      totalCount: entries.length,
    });
  });
  await page.getByRole("button", { name: "文本", exact: true }).click();
  await expect(page.locator(".record-count")).toContainText("120");
  await page.locator(".clipboard-list").evaluate((list) => {
    list.scrollTop = list.scrollHeight * 0.7;
  });
  await expect.poll(() => page.locator(".clipboard-item").count()).toBeLessThan(120);
  await expect
    .poll(() =>
      page.locator(".clipboard-list").evaluate((list) => {
        const viewport = list.getBoundingClientRect();
        const rows = Array.from(list.querySelectorAll<HTMLElement>(".clipboard-row"))
          .map((row) => ({
            rect: row.getBoundingClientRect(),
            opacity: Number(getComputedStyle(row.parentElement!).opacity),
          }))
          .filter(({ rect }) => rect.bottom > viewport.top && rect.top < viewport.bottom)
          .sort((a, b) => a.rect.top - b.rect.top);
        if (rows.length === 0 || rows.some((row) => row.opacity < 1)) return Infinity;
        let maxGap = Math.max(0, rows[0].rect.top - viewport.top);
        for (let index = 1; index < rows.length; index++) {
          maxGap = Math.max(maxGap, rows[index].rect.top - rows[index - 1].rect.bottom);
        }
        return Math.max(maxGap, viewport.bottom - rows.at(-1)!.rect.bottom);
      }),
    )
    .toBeLessThan(20);
});

test("narrow windows keep the compact label filter contained", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 768 });
  await page.emulateMedia({ colorScheme: "dark" });
  await page.goto("/clipboard.html?visual=1&lang=enUs&theme=dark");
  await expect(page.locator(".clipboard-row").first()).toBeVisible();
  await page.screenshot({
    path: "/tmp/arcrelay-clipboard-label-scroll.png",
    animations: "disabled",
  });

  const labelControl = page.locator(".label-filter-control");
  const labelButton = page.locator(".label-filter-button");
  const labelMenu = page.locator(".label-filter-menu");
  await expect(labelButton).toContainText("All labels");
  await labelControl.hover();
  await expect(labelMenu).toBeVisible();
  await page.mouse.move(0, 0);
  await expect(labelMenu).toBeHidden();
  await labelButton.click();
  await expect(labelMenu).toBeVisible();
  await page.mouse.move(0, 0);
  await page.waitForTimeout(180);
  await expect(labelMenu).toBeVisible();
  await labelButton.click();
  await expect(labelMenu).toBeHidden();
  await labelButton.click();
  await page.locator(".label-filter-options > button", { hasText: "代码" }).click();
  await expect(labelButton).toContainText("代码");

  const layout = await page.evaluate(() => {
    const more = document.querySelector<HTMLElement>(".label-filter-control")!;
    const filters = document.querySelector<HTMLElement>(".clipboard-filters")!;
    return {
      documentOverflow: document.documentElement.scrollWidth - window.innerWidth,
      filterOverflow: filters.scrollWidth - filters.clientWidth,
      moreRight: more.getBoundingClientRect().right,
      filtersRight: filters.getBoundingClientRect().right,
    };
  });

  expect(layout.documentOverflow).toBeLessThanOrEqual(0);
  expect(layout.filterOverflow).toBeLessThanOrEqual(0);
  expect(layout.moreRight).toBeLessThanOrEqual(layout.filtersRight);
});
