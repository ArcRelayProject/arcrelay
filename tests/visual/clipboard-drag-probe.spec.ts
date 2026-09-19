import { readFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";

const fixture = readFileSync(
  new URL("../../examples/fixtures/clipboard-drag-drop.html", import.meta.url),
  "utf8",
);
const text = "你好，ArcRelay 👋\nUnicode text: café & <tags>";
const html =
  "<p><strong>你好，ArcRelay 👋</strong></p><p>Unicode text: café &amp; &lt;tags&gt;</p>";

async function openProbe(page: Page) {
  const requests: string[] = [];
  await page.route("**/*", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/clipboard-drag-probe.html") {
      await route.fulfill({ contentType: "text/html", body: fixture });
    } else if (path === "/expected.json") {
      await route.fulfill({ json: { text, html, files: [] } });
    } else if (path === "/report") {
      await route.fulfill({ body: "ok" });
    } else {
      requests.push(route.request().url());
      await route.fulfill({ status: 404, body: "unexpected resource" });
    }
  });
  await page.goto("/clipboard-drag-probe.html");
  await page.locator("#delay").fill("0");
  return requests;
}

async function dropText(page: Page, droppedHtml?: string) {
  const report = page.waitForRequest((request) => new URL(request.url()).pathname === "/report");
  await page.locator("#drop").evaluate(
    (target, content) => {
      const dataTransfer = new DataTransfer();
      dataTransfer.setData("text/plain", content.text);
      if (content.html !== undefined) dataTransfer.setData("text/html", content.html);
      target.dispatchEvent(
        new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer }),
      );
    },
    { text, html: droppedHtml },
  );
  return (await report).postDataJSON();
}

test("probe preserves plain and rich text payloads without treating synthetic drops as native", async ({
  page,
}) => {
  const requests = await openProbe(page);
  const plain = await dropText(page);
  expect(plain.types).toEqual(["text/plain"]);
  expect(plain.text).toBe(text);
  expect(plain.checks).toEqual([{ kind: "plainText", pass: true }]);
  const rich = await dropText(page, html);
  expect(rich.types).toEqual(["text/plain", "text/html"]);
  expect(rich.html).toBe(html);
  expect(rich.checks).toEqual([
    { kind: "plainText", pass: true },
    { kind: "html", pass: true },
  ]);
  for (const report of [plain, rich]) {
    expect(report.trusted).toBe(false);
    expect(report.pass).toBe(false);
  }
  expect(requests).toEqual([]);
});

for (const [name, markup] of [
  [
    "script and image handlers",
    '<script>window.__probeExecuted = true; fetch("/script-resource")</script><img src="/image-resource" onerror="window.__probeExecuted = true">',
  ],
  [
    "embedded resources without text",
    '<iframe src="/frame-resource"></iframe><svg onload="window.__probeExecuted = true"><image href="/svg-resource" /></svg>',
  ],
]) {
  test(`probe records ${name} as literal HTML without execution or resource requests`, async ({
    page,
  }) => {
    const requests = await openProbe(page);
    const payload = html + markup;
    const report = await dropText(page, payload);
    expect(report.html).toBe(payload);
    expect(report.checks).toContainEqual({ kind: "html", pass: false });
    expect(JSON.parse((await page.locator("#results").textContent())!).at(-1).html).toBe(payload);
    await expect(page.locator("#results script, #results img, iframe, svg, object")).toHaveCount(0);
    expect(await page.evaluate(() => Reflect.get(window, "__probeExecuted"))).toBeUndefined();
    expect(requests).toEqual([]);
  });
}
