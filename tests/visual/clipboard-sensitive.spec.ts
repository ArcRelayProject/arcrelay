import { expect, test } from "@playwright/test";

test("clipboard sensitivity rules are discoverable, editable, and retained across navigation", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1180, height: 1000 });
  await page.goto("/?visual=1&lang=enUs&page=settings&settingsTab=clipboard");
  const section = page.getByRole("region", { name: "Sensitive content", exact: true });
  const enabled = section.getByRole("switch", { name: "Detect sensitive content", exact: true });
  const credentials = section.getByRole("switch", { name: "Standalone credentials", exact: true });
  const saveSensitive = section.getByRole("switch", {
    name: "Save matching sensitive content",
    exact: true,
  });
  const keywords = section.getByLabel("Custom keywords", { exact: true });
  await expect(enabled).toBeEnabled();
  await expect(enabled).toHaveAttribute("aria-checked", "true");
  await expect(section).toContainText(
    "Mentioning password in ordinary text does not trigger built-in rules.",
  );
  await credentials.click();
  await expect(credentials).toHaveAttribute("aria-checked", "false");
  await keywords.fill("Internal-Only\n内部资料");
  await section.getByRole("button", { name: "Save keywords", exact: true }).click();
  await expect(section.getByRole("status")).toHaveText("Settings saved");
  await saveSensitive.click();
  await expect(saveSensitive).toHaveAttribute("aria-checked", "true");
  await expect(saveSensitive).toBeEnabled();
  const stored = await page.evaluate(async () => {
    const { bridge } = await import("/src/bridge.mock.ts");
    return bridge.getClipboardSensitiveSettings();
  });
  expect(stored).toMatchObject({
    credentialAssignments: false,
    saveSensitive: true,
    customKeywords: ["Internal-Only", "内部资料"],
  });
  await page.getByRole("button", { name: "General", exact: true }).click();
  await page.getByRole("button", { name: "Clipboard", exact: true }).click();
  await expect(credentials).toHaveAttribute("aria-checked", "false");
  await expect(keywords).toHaveValue("Internal-Only\n内部资料");
  await enabled.click();
  await expect(enabled).toHaveAttribute("aria-checked", "false");
  await expect(keywords).toBeDisabled();
  await expect(credentials).toBeDisabled();
  await expect(saveSensitive).toBeDisabled();
  await enabled.click();
  await expect(keywords).toBeEnabled();
  await keywords.fill("文".repeat(86));
  await expect(section.getByRole("alert")).toContainText("256 bytes");
  await expect(section.getByRole("button", { name: "Save keywords", exact: true })).toBeDisabled();
  await keywords.fill("");
  await section.getByRole("button", { name: "Save keywords", exact: true }).click();
  await expect(keywords).toBeEnabled();
  expect(
    await keywords.evaluate((element) => element.getBoundingClientRect().width),
  ).toBeGreaterThan(400);
  expect(
    await saveSensitive
      .locator(".row-copy")
      .evaluate((element) => element.getBoundingClientRect().width),
  ).toBeGreaterThan(300);
  await section.screenshot({ path: "/tmp/arcrelay-clipboard-sensitive.png" });
  await page.evaluate(async () => {
    const { bridge } = await import("/src/bridge.mock.ts");
    bridge.setClipboardSensitiveSettings = async () => {
      throw new Error("save failed");
    };
  });
  await credentials.click();
  await expect(section.getByRole("alert")).toContainText("save failed");
  await expect(credentials).toHaveAttribute("aria-checked", "false");
  await expect(credentials).toBeEnabled();
  await keywords.fill("must stay editable");
  await section.getByRole("button", { name: "Save keywords", exact: true }).click();
  await expect(section.getByRole("alert")).toContainText("save failed");
  await expect(keywords).toHaveValue("must stay editable");
  await expect(keywords).toBeEnabled();
});
