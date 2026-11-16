import { expect, test, type Page } from "@playwright/test";

const preview = "/?page=input&inputScenario=complex&visual=1&lang=zhCn&theme=light";
const screen = (page: Page, number: number) =>
  page.locator(`g[aria-pressed][aria-label^="${number} ·"]`);
async function resizeSelectedScreen(page: Page) {
  await screen(page, 2).click();
  await page.getByRole("button", { name: "尺寸", exact: true }).click();
  await page.getByRole("button", { name: "放大屏幕", exact: true }).click();
  await page.keyboard.press("Escape");
}

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 928 });
  await page.goto(preview);
  await expect(screen(page, 5)).toBeVisible();
});

test("complex layouts and edge markers stay aligned when the viewport changes", async ({
  page,
}) => {
  for (const width of [1440, 1000, 1440]) {
    await page.setViewportSize({ width, height: 928 });
    await expect
      .poll(async () =>
        page.evaluate(() => {
          const canvas = document.querySelector('svg[aria-label="物理桌面工作区"]')!;
          const rect = canvas.querySelector('g[aria-label^="1 ·"] > rect')!;
          const edge = canvas.querySelector("line.segment")!;
          return Math.abs(Number(rect.getAttribute("y")) - Number(edge.getAttribute("y1")));
        }),
      )
      .toBeLessThan(0.1);
    const canvas = await page.locator('svg[aria-label="物理桌面工作区"]').boundingBox();
    for (const number of [1, 2, 3, 4, 5]) {
      const bounds = await screen(page, number).locator(":scope > rect").boundingBox();
      expect(bounds!.x).toBeGreaterThanOrEqual(canvas!.x);
      expect(bounds!.y).toBeGreaterThanOrEqual(canvas!.y);
      expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(canvas!.x + canvas!.width);
    }
  }
});

test("screen operations are contextual and unsaved layout changes can be kept or cancelled", async ({
  page,
}) => {
  await expect(page.getByRole("button", { name: "尺寸", exact: true })).toHaveCount(0);
  await resizeSelectedScreen(page);
  await expect(page.getByRole("button", { name: "保存布局", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "跨屏输入更多选项", exact: true }).click();
  await page.getByRole("menuitem", { name: "键盘行为", exact: true }).click();
  const dialog = page.locator(".input-discard-dialog");
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "取消", exact: true }).click();
  await expect(page.getByRole("heading", { name: "屏幕排列", exact: true })).toBeVisible();
  await page.locator(".save-controls").getByRole("button", { name: "取消", exact: true }).click();
  await expect(page.getByRole("button", { name: "保存布局", exact: true })).toHaveCount(0);
  await expect(page.locator("g[aria-pressed]")).toHaveCount(5);
});

test("saving a dirty layout does not re-enable input sharing", async ({ page }) => {
  await resizeSelectedScreen(page);
  await page.getByRole("switch", { name: "跨屏输入", exact: true }).click();
  await expect(page.getByRole("switch", { name: "跨屏输入", exact: true })).toHaveAttribute(
    "aria-checked",
    "false",
  );
  await page.getByRole("button", { name: "保存布局", exact: true }).click();
  await expect(page.getByRole("button", { name: "保存布局", exact: true })).toHaveCount(0);
  await expect(page.getByRole("switch", { name: "跨屏输入", exact: true })).toHaveAttribute(
    "aria-checked",
    "false",
  );
});

test("multiple screens belonging to one computer move as a group", async ({ page }) => {
  const fourth = await screen(page, 4).boundingBox();
  const fifth = await screen(page, 5).boundingBox();
  await page.mouse.move(fourth!.x + 40, fourth!.y + 35);
  await page.mouse.down();
  await page.mouse.move(fourth!.x + 70, fourth!.y + 70, { steps: 8 });
  await page.mouse.up();
  const movedFourth = await screen(page, 4).boundingBox();
  const movedFifth = await screen(page, 5).boundingBox();
  expect(movedFourth!.x).not.toBe(fourth!.x);
  expect(movedFifth!.x - movedFourth!.x).toBeCloseTo(fifth!.x - fourth!.x, 1);
  expect(movedFifth!.y - movedFourth!.y).toBeCloseTo(fifth!.y - fourth!.y, 1);
});

test("keyboard, gaze, and diagnostics remain accessible as secondary pages", async ({ page }) => {
  for (const label of ["键盘行为", "眼动", "诊断与调试"]) {
    await page.getByRole("button", { name: "跨屏输入更多选项", exact: true }).click();
    await page.getByRole("menuitem", { name: label, exact: true }).click();
    await expect(page.getByRole("heading", { level: 1, name: new RegExp(label) })).toBeVisible();
    await page.locator(".back-link").click();
    await expect(page.getByRole("heading", { name: "屏幕排列", exact: true })).toBeVisible();
  }
});
