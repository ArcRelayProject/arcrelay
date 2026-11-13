import { expect, test } from "@playwright/test";

test("sensitivity editor manages rules without expanding the settings page", async ({ page }) => {
  await page.setViewportSize({ width: 1180, height: 1000 });
  await page.goto("/?visual=1&lang=enUs&page=settings&settingsTab=clipboard");
  const section = page.getByRole("region", { name: "Sensitive content", exact: true });
  const edit = section.getByRole("button", { name: "Edit rules", exact: true });
  await expect(edit).toBeEnabled();
  expect((await section.boundingBox())!.height).toBeLessThan(160);
  await edit.click();
  const dialog = page.getByRole("dialog", { name: "Edit sensitive content rules", exact: true });
  const enabled = dialog.getByRole("switch", { name: "Enable filtering", exact: true });
  const credentials = dialog.getByRole("switch", { name: "Passwords & keys", exact: true });
  const add = dialog.getByRole("button", { name: "Add keyword", exact: true }).first();
  const input = dialog.getByRole("textbox", { name: "Keyword text", exact: true });
  const save = dialog.getByRole("button", { name: "Save", exact: true });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText(
    "Mentioning password in ordinary documents does not trigger filtering.",
  );
  await expect(credentials).toHaveAttribute("aria-checked", "true");
  await credentials.click();
  await expect(credentials).toHaveAttribute("aria-checked", "false");
  await add.click();
  await input.fill("Internal-Only");
  await save.click();
  await expect(dialog.getByRole("button", { name: "Internal-Only", exact: true })).toHaveAttribute(
    "aria-current",
    "true",
  );
  await add.click();
  await input.fill("Internal-only");
  await expect(dialog.getByRole("alert")).toContainText("This keyword already exists.");
  await expect(save).toBeDisabled();
  await input.fill("内部资料");
  await save.click();
  await expect(save).toBeDisabled();
  await dialog.getByRole("button", { name: "Internal-Only", exact: true }).click();
  await input.fill("unsaved draft");
  await dialog.getByRole("button", { name: "内部资料", exact: true }).click();
  await expect(dialog.getByRole("alert")).toContainText("unsaved keyword changes");
  await dialog.getByRole("button", { name: "Keep editing", exact: true }).click();
  await expect(input).toHaveValue("unsaved draft");
  await input.press("Escape");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("alert")).toContainText("unsaved keyword changes");
  await dialog.getByRole("button", { name: "Discard changes", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(section).toContainText("2 built-in rules · 2 keywords");
  await edit.click();
  await expect(input).toHaveValue("Internal-Only");
  await input.fill("文".repeat(86));
  await expect(dialog.getByRole("alert")).toContainText("256 bytes");
  await expect(save).toBeDisabled();
  await input.fill("Revised keyword");
  await save.click();
  await expect(dialog.getByRole("button", { name: "Revised keyword", exact: true })).toBeVisible();
  await dialog.getByRole("button", { name: "Delete keyword", exact: true }).click();
  await expect(input).toHaveValue("内部资料");
  const policy = dialog.getByRole("button", { name: "When content matches", exact: true });
  await policy.click();
  await page.getByRole("option", { name: "Save with hidden preview", exact: true }).click();
  await expect(dialog).toContainText(
    "Saved sensitive content still participates in clipboard sync",
  );
  await enabled.click();
  await expect(credentials).toBeDisabled();
  await expect(policy).toBeDisabled();
  await enabled.click();
  await expect(policy).toBeEnabled();
  await policy.click();
  await page.getByRole("option", { name: "Do not save to history", exact: true }).click();
  await page.setViewportSize({ width: 700, height: 700 });
  await expect(dialog.getByRole("button", { name: "Done", exact: true })).toBeInViewport();
  await page.setViewportSize({ width: 400, height: 760 });
  const box = (await dialog.boundingBox())!;
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(400);
  await expect(dialog.getByRole("button", { name: "Done", exact: true })).toBeInViewport();
  await page.setViewportSize({ width: 1180, height: 1000 });
  await dialog.getByRole("button", { name: "Done", exact: true }).click();
  await page.getByRole("button", { name: "General", exact: true }).click();
  await page.getByRole("button", { name: "Clipboard", exact: true }).click();
  await expect(section).toContainText("2 built-in rules · 1 keywords");
  await edit.click();
  await expect(input).toHaveValue("内部资料");
  await page.evaluate(async () => {
    const { bridge } = await import("/src/bridge.mock.ts");
    bridge.setClipboardSensitiveSettings = async () => {
      throw new Error("save failed");
    };
  });
  await credentials.click();
  await expect(dialog.getByRole("alert")).toContainText("save failed");
  await expect(credentials).toHaveAttribute("aria-checked", "false");
  await input.fill("draft survives failure");
  await save.click();
  await expect(dialog.getByRole("alert")).toContainText("save failed");
  await expect(input).toHaveValue("draft survives failure");
  await expect(save).toBeEnabled();
});
