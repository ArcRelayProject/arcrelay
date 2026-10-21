import { expect, test, type Page } from "@playwright/test";

async function open(page: Page) {
  await page.setViewportSize({ width: 468, height: 906 });
  await page.goto("/clipboard.html?lang=zhCn");
  await expect(page.locator(".clipboard-row").first()).toBeVisible();
}
async function captureText(page: Page) {
  await page.getByRole("button", { name: "文本", exact: true }).click();
  await page.locator('[data-clipboard-id="102"] .clipboard-row').click({ button: "right" });
  await page.getByRole("menuitem", { name: "添加到集合…", exact: true }).click();
  await expect(page.getByRole("textbox", { name: "内容项名称", exact: true })).toBeVisible();
}

test("text appends to an existing collection and preserves the source query and history", async ({
  page,
}) => {
  await open(page);
  await page.getByRole("textbox", { name: "⌘F", exact: true }).fill("arcrelay");
  await expect(page.locator(".clipboard-row")).toHaveCount(1);
  await captureText(page);
  await page.getByRole("textbox", { name: "内容项名称", exact: true }).fill("备用网址");
  await page.getByRole("button", { name: /^工作账号 .*3 项$/ }).click();
  await page.getByRole("button", { name: "添加到集合", exact: true }).click();
  await expect(page.getByRole("textbox", { name: "⌘F", exact: true })).toHaveValue("arcrelay");
  await expect(page.locator('[data-clipboard-id="102"]')).toBeVisible();
  await expect(page.getByRole("status")).toContainText("已添加到 工作账号");
  await page.getByRole("textbox", { name: "⌘F", exact: true }).fill("");
  await page.getByRole("button", { name: "集合", exact: true }).click();
  await page.getByRole("button", { name: /^工作账号 用户名、密码、TOTP、备用网址$/ }).click();
  const card = page.locator(".collection-card.expanded");
  await expect(card.locator(".collection-value")).toHaveCount(4);
  await expect(card.locator(".collection-value").last()).toContainText(
    "https://www.arcrelay.app/clipboard",
  );
});

test("multi-select drafts preserve selection order and cancel restores selections", async ({
  page,
}) => {
  await open(page);
  await page.getByRole("button", { name: "文本", exact: true }).click();
  for (const id of [105, 102, 103]) {
    await page
      .locator(`[data-clipboard-id="${id}"] .clipboard-row`)
      .click({ modifiers: ["ControlOrMeta"] });
  }
  await page.getByRole("button", { name: "保存为集合", exact: true }).click();
  const values = page.locator(".collection-field textarea");
  await expect(values).toHaveCount(3);
  await expect(values.nth(0)).toHaveValue(/git commit/);
  await expect(values.nth(1)).toHaveValue("https://www.arcrelay.app/clipboard");
  await page.getByRole("button", { name: "拖动内容项", exact: true }).first().press("ArrowDown");
  await expect(values.nth(0)).toHaveValue("https://www.arcrelay.app/clipboard");
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await page.getByRole("button", { name: "放弃修改", exact: true }).click();
  await expect(page.locator(".multi-select-status")).toContainText("已选 3 项");
  await expect(page.locator('[data-clipboard-id="105"] .selection-order')).toHaveText("1");
  await page.getByRole("button", { name: "保存为集合", exact: true }).click();
  await page.getByRole("textbox", { name: "集合名称", exact: true }).fill("测试环境");
  await page.getByRole("button", { name: "保存集合", exact: true }).click();
  await expect(page.locator(".collection-card.expanded")).toContainText("测试环境");
  await expect(page.locator(".collection-card.expanded .collection-value")).toHaveCount(3);
  await expect(page.getByRole("toolbar", { name: "多选操作" })).toHaveCount(0);
});

test("a single text can create a new collection without the template step", async ({ page }) => {
  await open(page);
  await captureText(page);
  await page.getByRole("button", { name: "新建集合…", exact: true }).click();
  await expect(page.getByText("从哪里开始？", { exact: true })).toHaveCount(0);
  await expect(page.locator(".collection-field textarea")).toHaveValue(
    "https://www.arcrelay.app/clipboard",
  );
  await page.getByRole("textbox", { name: "集合名称", exact: true }).fill("单条测试");
  await page.getByRole("button", { name: "保存集合", exact: true }).click();
  await expect(page.locator(".collection-card.expanded")).toContainText("单条测试");
});

test("collection app pins persist and change with the recipient application", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "集合", exact: true }).click();
  await page.getByRole("button", { name: /^家庭地址 / }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "在 Safari 中置顶", exact: true }).click();
  await expect(page.locator(".collection-card").first()).toContainText("家庭地址");
  await expect(page.locator(".collection-app-pin")).toHaveText("Safari");
  await page.reload();
  await page.getByRole("button", { name: "集合", exact: true }).click();
  await expect(page.locator(".collection-app-pin")).toHaveText("Safari");
  await page.evaluate(async () => {
    const { clipboardBridge } = await import("/src/clipboard/bridge.mock.ts");
    clipboardBridge.targetApplication = async () => ({ id: "mock:editor", name: "VS Code" });
  });
  await expect(page.locator(".collection-app-pin")).toHaveCount(0);
  await page.getByRole("button", { name: /^家庭地址 / }).click({ button: "right" });
  await expect(
    page.getByRole("menuitem", { name: "在 VS Code 中置顶", exact: true }),
  ).toBeVisible();
});

test("mixed image/text selections cannot be silently saved as a partial collection", async ({
  page,
}) => {
  await open(page);
  for (const id of [101, 102])
    await page
      .locator(`[data-clipboard-id="${id}"] .clipboard-row`)
      .click({ modifiers: ["ControlOrMeta"] });
  await expect(page.getByRole("button", { name: "保存为集合", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: "保存为集合", exact: true })).toHaveAttribute(
    "title",
    /仅支持可用的文本/,
  );
});

test("append conflicts preserve the captured text and field name", async ({ page }) => {
  await open(page);
  await captureText(page);
  await page.getByRole("textbox", { name: "内容项名称", exact: true }).fill("保留这个名称");
  await page.getByRole("button", { name: /^工作账号 / }).click();
  await page.evaluate(async () => {
    const { collectionRequest } = await import("/src/features/collections/bridge.ts");
    const row = (await collectionRequest({ type: "list", after: "" })).records.find(
      (r) => r.name === "工作账号",
    )!;
    await collectionRequest({
      type: "save",
      draft: {
        id: row.id,
        base: { revision: row.revision, deviceId: row.updatedByDeviceId },
        name: row.name,
        items: row.items,
      },
    });
  });
  const add = page.getByRole("button", { name: "添加到集合", exact: true });
  if (await add.isEnabled()) await add.click();
  await expect(page.getByRole("alert")).toContainText("已更新或删除");
  await expect(page.getByRole("textbox", { name: "内容项名称", exact: true })).toHaveValue(
    "保留这个名称",
  );
  await expect(page.locator(".collection-capture-preview")).toHaveText(
    "https://www.arcrelay.app/clipboard",
  );
});

test("a full collection cannot be chosen as an append destination", async ({ page }) => {
  await open(page);
  await page.evaluate(async () => {
    const { collectionRequest } = await import("/src/features/collections/bridge.ts");
    await collectionRequest({
      type: "save",
      draft: {
        id: null,
        base: null,
        name: "容量测试",
        items: Array.from({ length: 64 }, (_, i) => ({
          id: crypto.randomUUID(),
          label: "内容 " + i,
          value: "test",
          masked: false,
          totp: null,
        })),
      },
    });
  });
  await captureText(page);
  await expect(
    page.getByRole("button", { name: "容量测试 已达 64 项上限", exact: true }),
  ).toBeDisabled();
});
