import { expect, test, type Page } from "@playwright/test";

async function open(page: Page) {
  await page.setViewportSize({ width: 700, height: 650 });
  await page.goto("/clipboard.html?lang=zhCn&theme=light");
  await expect(page.locator(".clipboard-row").first()).toBeVisible();
  await page.evaluate(async () => {
    const { clipboardBridge } = await import("/src/clipboard/bridge.mock.ts");
    const state = ((window as any).__dragTest = {
      prepared: [],
      started: [],
      cancelled: [],
      pasted: [],
      serial: 0,
    });
    clipboardBridge.prepareDrag = async (request: unknown) => {
      state.prepared.push(request);
      const prepared = { token: `drag-${++state.serial}`, kind: "files" as const, count: 1 };
      if (state.delay)
        return new Promise((resolve) => {
          state.resolve = () => resolve(prepared);
        });
      return prepared;
    };
    clipboardBridge.startDrag = async (token: string) => {
      state.started.push(token);
    };
    clipboardBridge.cancelDrag = async (token: string) => {
      state.cancelled.push(token);
    };
    clipboardBridge.pasteAs = async (id: number) => {
      state.pasted.push(id);
    };
  });
}

async function pressAndMove(page: Page, selector: string) {
  const bounds = await page.locator(selector).boundingBox();
  expect(bounds).not.toBeNull();
  const x = bounds!.x + bounds!.width / 2;
  const y = bounds!.y + bounds!.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + 20, y + 12, { steps: 4 });
}

test("card drag starts once and stays active until the native completion event", async ({
  page,
}) => {
  await open(page);
  await pressAndMove(page, '[data-clipboard-id="101"] .clipboard-row');
  await expect
    .poll(() => page.evaluate(() => (window as any).__dragTest.started))
    .toEqual(["drag-1"]);
  await page.mouse.up();
  await expect(page.locator('[data-drag-phase="dragging"]')).toBeVisible();
  expect(await page.evaluate(() => (window as any).__dragTest.pasted)).toEqual([]);
  await page.evaluate(async () => {
    const { emitMockDragEnded } = await import("/src/clipboard/bridge.mock.ts");
    emitMockDragEnded({ token: "drag-1", outcome: "dropped" });
  });
  await expect(page.locator("[data-drag-phase]")).toHaveCount(0);
});

test("releasing a slow preparation never starts a late native drag", async ({ page }) => {
  await open(page);
  await page.evaluate(() => {
    (window as any).__dragTest.delay = true;
  });
  await pressAndMove(page, '[data-clipboard-id="101"] .clipboard-row');
  await expect(page.locator('[data-drag-phase="preparing"]')).toBeVisible();
  await page.mouse.up();
  await page.evaluate(() => (window as any).__dragTest.resolve());
  await expect
    .poll(() => page.evaluate(() => (window as any).__dragTest.cancelled))
    .toEqual(["drag-1"]);
  expect(await page.evaluate(() => (window as any).__dragTest.started)).toEqual([]);
  await expect(page.locator("[data-drag-phase]")).toHaveCount(0);
});

test("clicking an unavailable file only selects it; availability is checked when actually dragging", async ({
  page,
}) => {
  await open(page);
  await page.evaluate(async () => {
    const { clipboardBridge } = await import("/src/clipboard/bridge.mock.ts");
    const history = clipboardBridge.history;
    clipboardBridge.history = async (options) => {
      const result = await history(options);
      return {
        ...result,
        entries: result.entries.map((item) =>
          item.id === 101
            ? { ...item, kind: "files" as const, available: false, preview: "missing.txt" }
            : item,
        ),
      };
    };
    clipboardBridge.prepareDrag = async (request) => {
      (window as any).__dragTest.prepared.push(request);
      throw new Error("Source file is missing");
    };
  });
  await page.getByRole("button", { name: "刷新", exact: true }).click();
  const row = page.locator('[data-clipboard-id="101"] .clipboard-row');
  await expect(row).toHaveClass(/unavailable/);
  await row.click({ force: true });
  await expect(row).toHaveClass(/selected/);
  expect(await page.evaluate(() => (window as any).__dragTest.prepared)).toEqual([]);
  await expect(page.locator(".clipboard-drag-notice")).toHaveCount(0);
  await pressAndMove(page, '[data-clipboard-id="101"] .clipboard-row');
  await expect(page.getByRole("alert")).toContainText("Source file is missing");
  await page.mouse.up();
});

test("dragging selected records preserves selection order and does not toggle it on release", async ({
  page,
}) => {
  await open(page);
  await page.locator('[data-clipboard-id="103"] .clipboard-row').click({ modifiers: ["Meta"] });
  await page.locator('[data-clipboard-id="101"] .clipboard-row').click({ modifiers: ["Meta"] });
  await pressAndMove(page, '[data-clipboard-id="101"] .clipboard-row');
  await expect
    .poll(() => page.evaluate(() => (window as any).__dragTest.prepared.at(-1)?.ids))
    .toEqual([103, 101]);
  await page.mouse.up();
  await expect(page.locator(".clipboard-row.multi-selected")).toHaveCount(2);
});

test("TXT option is passed for full text while double click still inserts normally", async ({
  page,
}) => {
  await open(page);
  const row = page.locator('[data-clipboard-id="102"] .clipboard-row');
  await row.dblclick();
  await expect.poll(() => page.evaluate(() => (window as any).__dragTest.pasted)).toEqual([102]);
  expect(await page.evaluate(() => (window as any).__dragTest.started)).toEqual([]);
  await page.locator(".clipboard-drag-format").selectOption("text_file");
  await pressAndMove(page, '[data-clipboard-id="102"] .clipboard-row');
  await expect
    .poll(() => page.evaluate(() => (window as any).__dragTest.prepared.at(-1)))
    .toEqual({ ids: [102], mode: "text_file" });
  await page.mouse.up();
});

test("native text selection drag uses the exact selected text and plain mode", async ({ page }) => {
  await open(page);
  const row = page.locator('[data-clipboard-id="103"] .clipboard-row');
  await row.focus();
  await row.press("Space");
  await expect(page.locator(".rendered-text strong")).toBeVisible();
  const selection = await page.locator(".rendered-text strong").evaluate((node) => {
    const range = document.createRange();
    range.selectNodeContents(node);
    const selection = window.getSelection()!;
    selection.removeAllRanges();
    selection.addRange(range);
    document.dispatchEvent(new Event("selectionchange"));
    return selection.toString();
  });
  await page.locator(".rendered-text strong").dispatchEvent("pointerdown", {
    pointerId: 1,
    pointerType: "mouse",
    button: 0,
    buttons: 1,
    clientX: 100,
    clientY: 100,
  });
  await page.locator(".rendered-text strong").dispatchEvent("dragstart", {
    dataTransfer: await page.evaluateHandle(() => new DataTransfer()),
  });
  await expect.poll(() => page.evaluate(() => (window as any).__dragTest.started.length)).toBe(1);
  expect(await page.evaluate(() => (window as any).__dragTest.prepared.at(-1))).toEqual({
    ids: [103],
    mode: "plain_text",
    selection: { id: 103, text: selection },
  });
});

test("selected text TXT handle and image original handle are available in previews", async ({
  page,
}) => {
  await open(page);
  await page.locator('[data-clipboard-id="103"] .clipboard-row').focus();
  await page.keyboard.press("Space");
  await page.locator(".rendered-text strong").dblclick();
  await page.locator(".text-drag-format").selectOption("text_file");
  await pressAndMove(page, ".text-preview-actions .clipboard-drag-handle");
  await expect
    .poll(() => page.evaluate(() => (window as any).__dragTest.prepared.at(-1)?.mode))
    .toBe("text_file");
  expect(
    await page.evaluate(() => (window as any).__dragTest.prepared.at(-1)?.selection?.text),
  ).toBeTruthy();
  await page.mouse.up();
  await open(page);
  await page.locator('[data-clipboard-id="101"] .clipboard-row').focus();
  await page.keyboard.press("Space");
  await pressAndMove(page, ".image-preview-toolbar .clipboard-drag-handle");
  await expect
    .poll(() => page.evaluate(() => (window as any).__dragTest.prepared.at(-1)))
    .toEqual({ ids: [101], mode: "auto" });
  await page.mouse.up();
});

test("dragging an existing browser selection with the mouse starts a plain text export", async ({
  page,
}) => {
  await open(page);
  await page.locator('[data-clipboard-id="103"] .clipboard-row').focus();
  await page.keyboard.press("Space");
  const text = page.locator(".rendered-text strong");
  await text.evaluate((node) => {
    const range = document.createRange();
    range.selectNodeContents(node);
    const selected = window.getSelection()!;
    selected.removeAllRanges();
    selected.addRange(range);
    document.dispatchEvent(new Event("selectionchange"));
  });
  const bounds = await text.boundingBox();
  await page.mouse.move(bounds!.x + bounds!.width / 2, bounds!.y + bounds!.height / 2);
  await page.mouse.down();
  // Chromium's native selected-text drag has a press-and-hold threshold on macOS.
  await page.waitForTimeout(250);
  await page.mouse.move(bounds!.x + bounds!.width + 40, bounds!.y + 20, { steps: 10 });
  await expect.poll(() => page.evaluate(() => (window as any).__dragTest.started.length)).toBe(1);
  expect(await page.evaluate(() => (window as any).__dragTest.prepared.at(-1)?.mode)).toBe(
    "plain_text",
  );
  await page.mouse.up();
});

test("drag controls remain contained at the minimum window width", async ({ page }) => {
  await open(page);
  await page.setViewportSize({ width: 420, height: 650 });
  const size = await page
    .locator(".clipboard-footer")
    .evaluate((node) => ({ width: node.clientWidth, scroll: node.scrollWidth }));
  expect(size.scroll).toBeLessThanOrEqual(size.width);
  await page.screenshot({
    path: "/tmp/arcrelay-clipboard-drag-controls.png",
    animations: "disabled",
  });
  await page.locator('[data-clipboard-id="101"] .clipboard-row').focus();
  await page.keyboard.press("Space");
  const toolbarSize = await page
    .locator(".image-preview-toolbar")
    .evaluate((node) => ({ width: node.clientWidth, scroll: node.scrollWidth }));
  expect(toolbarSize.scroll).toBeLessThanOrEqual(toolbarSize.width);
  await page.screenshot({ path: "/tmp/arcrelay-clipboard-drag-image.png", animations: "disabled" });
});

test("translated drag controls fit the minimum width in all supported languages", async ({
  page,
}) => {
  await page.setViewportSize({ width: 420, height: 650 });
  for (const language of ["zhCn", "enUs", "jaJp", "koKr", "deDe", "frFr", "esEs", "ptBr"]) {
    await page.goto(`/clipboard.html?visual=1&lang=${language}&theme=light`);
    const row = page.locator('[data-clipboard-id="101"] .clipboard-row');
    await expect(row).toBeVisible();
    const footer = await page
      .locator(".clipboard-footer")
      .evaluate((node) => ({ width: node.clientWidth, scroll: node.scrollWidth }));
    expect(footer.scroll, `footer in ${language}`).toBeLessThanOrEqual(footer.width);
    await row.focus();
    await page.keyboard.press("Space");
    const toolbar = page.locator(".image-preview-toolbar");
    await expect(toolbar).toBeVisible();
    const bounds = await toolbar.evaluate((node) => ({
      width: node.clientWidth,
      scroll: node.scrollWidth,
    }));
    expect(bounds.scroll, `image toolbar in ${language}`).toBeLessThanOrEqual(bounds.width);
    if (language !== "zhCn")
      await expect(toolbar.locator(".clipboard-drag-handle")).not.toHaveText(/拖出/);
  }
});
