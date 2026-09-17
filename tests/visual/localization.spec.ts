import { expect, test } from "@playwright/test";

const ready = async (page: import("@playwright/test").Page, query: string) => {
  await page.setViewportSize({ width: 1180, height: 760 });
  await page.goto(`/?visual=1&lang=enUs&theme=dark&${query}`);
  await expect(page.locator("html")).toHaveAttribute("data-visual-ready", "true");
};

test("preset metadata, localized search, and language changes render together", async ({
  page,
}) => {
  await ready(page, "page=actions");
  await page.evaluate(async () => {
    const { bridge } = await import("/src/bridge.mock.ts");
    bridge.getActionPresets = async () => [
      {
        id: "system.lock",
        name: "锁定电脑",
        description: "立即锁定当前用户会话。",
        group: "系统",
        iconId: "settings",
        iconSvg: "",
        color: "#2563EB",
        defaultEnabled: true,
        requiresConfirmation: false,
        installed: false,
        actionType: { type: "System", operation: "lock_screen" },
      },
      {
        id: "system.sleep",
        name: "电脑睡眠",
        description: "让电脑进入睡眠状态。",
        group: "系统",
        iconId: "settings",
        iconSvg: "",
        color: "#2563EB",
        defaultEnabled: false,
        requiresConfirmation: true,
        installed: false,
        actionType: { type: "System", operation: "sleep" },
      },
    ];
  });
  await page.getByRole("button", { name: "Presets", exact: true }).click();
  await expect(page.locator(".preset-card")).toHaveCount(2);
  await expect(page.locator(".preset-card").first()).toContainText("Lock Computer");
  await expect(page.locator(".preset-card").first()).toContainText(
    "System · Lock the current user session immediately.",
  );
  const search = page.getByPlaceholder("Search presets");
  await search.fill("lock");
  await expect(page.locator(".preset-card")).toHaveCount(1);
  await search.fill("sleep");
  await expect(page.locator(".preset-card")).toHaveCount(1);
  await expect(page.locator(".preset-card")).toContainText("Put the computer to sleep.");
  await search.fill("");
  await page.evaluate(async () => {
    const { setLanguage } = await import("/src/i18n.ts");
    setLanguage("deDe");
  });
  await expect(page.locator(".preset-card").first()).toContainText("Computer sperren");
  await page.locator(".preset-search input").fill("sperren");
  await expect(page.locator(".preset-card")).toHaveCount(1);
});

test("input overview and keyboard labels localize and preserve device names", async ({ page }) => {
  await ready(page, "page=input&inputSection=overview");
  await expect(page.locator(".readiness-heading")).toContainText("Automatic sharing is ready");
  await expect(page.locator(".checks")).toContainText("Devices online");
  await expect(page.locator(".checks")).toContainText("Automatic links available");
  await expect(page.locator(".device-cluster.remote header")).toContainText("书房 Mac mini");
  await expect(page.locator(".activity-card")).toContainText("Control session started");
  await page.getByRole("button", { name: "Keyboard Behavior", exact: true }).click();
  await expect(page.locator(".profile-column")).toContainText("Productivity");
  await expect(page.locator(".shortcut-table")).toContainText("Copy");
  await expect(page.locator(".shortcut-table")).toContainText("Adapted");
});

test("gaze settings and camera diagnostics have no hardcoded Chinese UI text", async ({ page }) => {
  await ready(page, "page=input&inputSection=gaze");
  await expect(page.locator(".gaze-page")).toContainText(
    "Head and gaze calibration across devices",
  );
  await expect(page.locator(".gaze-page")).not.toContainText(/[\u4e00-\u9fff]/);
  await page.getByRole("button", { name: "Vision diagnostics", exact: true }).click();
  await expect(page.locator(".diagnostics-page h2")).toHaveText("Camera and face recognition");
  await expect(page.locator(".diagnostics-page")).toContainText("Local user not enrolled");
  await expect(page.locator(".diagnostics-page")).toContainText("No valid observation");
  await expect(page.locator(".diagnostics-page")).not.toContainText(/[\u4e00-\u9fff]/);
  await page.getByRole("button", { name: "Start camera", exact: true }).click();
  await expect(page.locator(".diagnostics-page")).toContainText("Left eye open");
  await expect(page.locator(".diagnostics-page")).toContainText("Right eye open");
  await expect(page.locator(".diagnostics-page")).not.toContainText(/[\u4e00-\u9fff]/);
});
