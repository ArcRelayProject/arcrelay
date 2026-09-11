import { expect, test } from "@playwright/test";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

import { languages, scenes, themes } from "./scenes";

const outputRoot = path.resolve(process.env.ARCRELAY_SCREENSHOT_DIR ?? "visual-artifacts/actual");
const featureFilter = process.env.ARCRELAY_SCREENSHOT_FEATURE;
const languageFilter = process.env.ARCRELAY_SCREENSHOT_LANGUAGE;
const themeFilter = process.env.ARCRELAY_SCREENSHOT_THEME;

function selected(value: string, filter?: string) {
  return !filter || value === filter;
}

function visualUrl(scenePath: string, language: string, theme: string) {
  const url = new URL(scenePath, "http://arcrelay.local");
  url.searchParams.set("visual", "1");
  url.searchParams.set("lang", language);
  url.searchParams.set("theme", theme);
  return `${url.pathname}${url.search}`;
}

for (const scene of scenes.filter((item) => selected(item.feature, featureFilter))) {
  for (const language of languages.filter((item) => selected(item, languageFilter))) {
    for (const theme of themes.filter((item) => selected(item, themeFilter))) {
      test(`${scene.id} · ${language} · ${theme}`, async ({ page }) => {
        await page.setViewportSize({ width: scene.width, height: scene.height });
        await page.emulateMedia({ colorScheme: theme });

        const errors: string[] = [];
        page.on("pageerror", (error) => errors.push(error.message));
        page.on("console", (message) => {
          if (message.type() === "error") errors.push(message.text());
        });

        await page.goto(visualUrl(scene.path, language, theme), { waitUntil: "domcontentloaded" });
        await expect(page.locator("html")).toHaveAttribute("data-visual-ready", "true", { timeout: 10_000 });
        await page.waitForTimeout(250);

        const relative = path.join(scene.feature, scene.id, language, theme, `${scene.width}x${scene.height}.png`);
        const screenshotPath = path.join(outputRoot, relative);
        await mkdir(path.dirname(screenshotPath), { recursive: true });
        await page.screenshot({ path: screenshotPath, animations: "disabled" });
        await writeFile(screenshotPath.replace(/\.png$/, ".json"), `${JSON.stringify({
          scene: scene.id,
          feature: scene.feature,
          surface: scene.surface,
          language,
          theme,
          logicalSize: { width: scene.width, height: scene.height },
          capturedAt: new Date().toISOString(),
        }, null, 2)}\n`);

        const layout = await page.evaluate(() => ({
          width: document.documentElement.scrollWidth,
          viewport: window.innerWidth,
        }));
        expect(layout.width, "page must not overflow horizontally").toBeLessThanOrEqual(layout.viewport);
        expect(errors, "browser errors during visual capture").toEqual([]);
      });
    }
  }
}
