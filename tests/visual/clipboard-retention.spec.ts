import { expect, test } from "@playwright/test";

test("idle retention defaults to 30 days, saves forever, and rolls back a failed save", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1180, height: 900 });
  await page.goto("/?visual=1&lang=enUs&page=settings&settingsTab=clipboard");
  await expect(page.locator("html")).toHaveAttribute("data-visual-ready", "true");
  const control = page.getByRole("button", { name: "Idle retention", exact: true });
  await expect(control).toBeEnabled();
  await expect(control).toContainText("30 days without use");
  await expect(page.locator(".settings-tab-content")).toContainText(
    "History has no entry or storage limits.",
  );
  await control.click();
  await page.getByRole("option", { name: "Forever", exact: true }).click();
  await expect(control).toContainText("Forever");
  await expect(control).toBeEnabled();
  expect(
    await page.evaluate(async () => {
      const { bridge } = await import("/src/bridge.mock.ts");
      return bridge.getClipboardRetentionDays();
    }),
  ).toBe(0);
  await page.getByRole("button", { name: "General", exact: true }).click();
  await page.getByRole("button", { name: "Clipboard", exact: true }).click();
  await expect(control).toContainText("Forever");
  await expect(control).toBeEnabled();
  await page.screenshot({ path: "/tmp/arcrelay-clipboard-retention.png", fullPage: true });
  await page.evaluate(async () => {
    const { bridge } = await import("/src/bridge.mock.ts");
    bridge.setClipboardRetentionDays = async () => {
      throw new Error("save failed");
    };
  });
  await control.click();
  await page.getByRole("option", { name: "90 days without use", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("save failed");
  await expect(control).toContainText("Forever");
  await expect(control).toBeEnabled();
});
