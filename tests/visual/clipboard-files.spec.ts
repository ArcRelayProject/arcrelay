import { expect, test } from "@playwright/test";

async function openFiles(page: import("@playwright/test").Page, fixture = "normal") {
  await page.setViewportSize({ width: 420, height: 580 });
  await page.goto(`/clipboard.html?visual=1&lang=zhCn&theme=light&preview-files=${fixture}`);
  return page.locator('[data-clipboard-id="107"] .clipboard-row');
}

test("file summary expands into complete ordered filenames and inserts the batch", async ({
  page,
}) => {
  const row = await openFiles(page);
  await expect(row.locator(".file-name")).toHaveText([
    "烘焙演示版-安卓-RK3568-v1.0.16.apk",
    "烘焙演示版-安卓-RK3576-v1.0.16.apk",
  ]);
  await row.getByRole("button", { name: "展开其余 4 个文件", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.locator(".file-name")).toHaveText([
    "烘焙演示版-安卓-RK3568-v1.0.16.apk",
    "烘焙演示版-安卓-RK3576-v1.0.16.apk",
    "烘焙演示版-Windows64位-v1.0.15.zip",
    "小碗菜演示版-安卓-RK3568-v1.0.16.apk",
    "小碗菜演示版-安卓-RK3576-v1.0.16.apk",
    "小碗菜演示版-Windows64位-v1.0.15.zip",
  ]);
  await dialog.getByRole("button", { name: "插入全部 6 个文件", exact: true }).click();
  await expect(dialog).toHaveCount(0);
});

test("keyboard activation of the summary opens preview instead of pasting", async ({ page }) => {
  const row = await openFiles(page);
  const more = row.getByRole("button", { name: "展开其余 4 个文件", exact: true });
  await more.focus();
  await more.press("Enter");
  await expect(page.getByRole("dialog")).toBeVisible();
});

test("many files scroll inside the preview while insertion stays visible", async ({ page }) => {
  const row = await openFiles(page, "many");
  await row.getByRole("button", { name: "展开其余 46 个文件", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.locator(".file-name")).toHaveCount(48);
  const insert = dialog.getByRole("button", { name: "插入全部 48 个文件", exact: true });
  await expect(insert).toBeInViewport();
  await dialog.locator(".file-name").last().scrollIntoViewIfNeeded();
  await expect(insert).toBeInViewport();
  await expect(dialog.getByRole("button", { name: "返回", exact: true })).toBeInViewport();
});

test("long names keep punctuation and suffix without horizontal overflow", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 580 });
  await page.goto("/clipboard.html?visual=1&lang=zhCn&theme=light&preview-files=long");
  await page.getByRole("button", { name: "展开其余 4 个文件", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.locator(".file-name").first()).toHaveText(
    "报告,终稿-".repeat(24) + "v1.0.16.apk",
  );
  expect(
    await dialog.locator(".files-preview").evaluate((el) => el.scrollWidth <= el.clientWidth),
  ).toBe(true);
  await expect(
    dialog.getByRole("button", { name: "插入全部 6 个文件", exact: true }),
  ).toBeInViewport();
});

test("unavailable files retain their names but cannot be inserted", async ({ page }) => {
  const row = await openFiles(page, "unavailable");
  await row.getByRole("button", { name: "展开其余 4 个文件", exact: true }).click();
  await expect(page.getByRole("dialog").locator(".file-name")).toHaveCount(6);
  await expect(page.getByRole("button", { name: "插入全部 6 个文件", exact: true })).toBeDisabled();
});

test("failed file reads expose retry and sensitive files hide all filenames", async ({ page }) => {
  const row = await openFiles(page, "error");
  await expect(row.getByText("无法读取文件列表", { exact: true })).toBeVisible();
  await expect(row.getByRole("button", { name: "重试", exact: true })).toBeVisible();
  await openFiles(page, "sensitive");
  await expect(page.locator('[data-clipboard-id="107"]')).toContainText("敏感内容已隐藏");
  await expect(page.locator('[data-clipboard-id="107"] .file-name')).toHaveCount(0);
});
