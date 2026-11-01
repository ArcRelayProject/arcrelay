import { expect, test, type Page } from "@playwright/test";

async function updateFixture(page: Page, source: "github" | "microsoftStore", statusFails = false) {
  await page.route("**/src/bridge.mock.ts", async (route) => {
    const response = await route.fetch();
    await route.fulfill({
      response,
      body: `${await response.text()}
        window.updateCalls = { check: 0, install: 0, store: 0 };
        const updateStatus = {
          currentVersion: "0.3.0", channel: "test", source: ${JSON.stringify(source)},
          update: { version: "0.3.10001", notes: null, publishedAt: null },
        };
        bridge.getAppUpdateStatus = async () => {
          if (${statusFails}) throw new Error("Update source is temporarily unavailable");
          return updateStatus;
        };
        bridge.checkForAppUpdate = async () => { window.updateCalls.check++; return updateStatus; };
        bridge.installAppUpdate = async () => { window.updateCalls.install++; };
        bridge.openMicrosoftStore = async () => { window.updateCalls.store++; };
      `,
    });
  });
  await page.setViewportSize({ width: 1180, height: 900 });
  await page.goto("/?visual=1&lang=enUs&theme=dark&page=settings&settingsTab=general");
  await expect(page.locator("html")).toHaveAttribute("data-visual-ready", "true");
}

test("Store installation opens Microsoft Store and hides standalone update controls", async ({
  page,
}) => {
  // Even stale GitHub metadata and a saved test channel must not enable installation.
  await updateFixture(page, "microsoftStore");
  await expect(page.getByRole("switch", { name: /Automatically Check for Updates/ })).toHaveCount(
    0,
  );
  await expect(page.getByRole("button", { name: "Update Channel", exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Install and Restart", exact: true })).toHaveCount(
    0,
  );
  await expect(page.locator(".app-update-row")).toContainText(
    "This version updates through Microsoft Store.",
  );
  await page
    .getByRole("button", { name: "Check for updates in Microsoft Store", exact: true })
    .click();
  await expect(page.getByText("ArcRelay is up to date", { exact: true })).toHaveCount(0);
  await expect
    .poll(() => page.evaluate("window.updateCalls"))
    .toEqual({
      check: 0,
      install: 0,
      store: 1,
    });
});

test("unavailable update status keeps the app usable and retries through the host", async ({
  page,
}) => {
  await updateFixture(page, "microsoftStore", true);
  await expect(page.getByRole("switch", { name: /Automatically Check for Updates/ })).toHaveCount(
    0,
  );
  await expect(page.getByRole("button", { name: "Update Channel", exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "Check for Updates", exact: true }).click();
  await expect(page.locator(".app-update-row")).toContainText(
    "This version updates through Microsoft Store.",
  );
  await expect
    .poll(() => page.evaluate("window.updateCalls"))
    .toEqual({
      check: 1,
      install: 0,
      store: 1,
    });
});

test("standalone installation retains GitHub checks and installation confirmation", async ({
  page,
}) => {
  await updateFixture(page, "github");
  await expect(page.getByRole("switch", { name: /Automatically Check for Updates/ })).toBeVisible();
  await expect(page.getByRole("button", { name: "Update Channel", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Check for Updates", exact: true }).click();
  await expect
    .poll(() => page.evaluate("window.updateCalls"))
    .toEqual({
      check: 1,
      install: 0,
      store: 0,
    });
  await page.getByRole("button", { name: "Install and Restart", exact: true }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Install and Restart", exact: true })
    .click();
  await expect
    .poll(() => page.evaluate("window.updateCalls"))
    .toEqual({
      check: 1,
      install: 1,
      store: 0,
    });
});
