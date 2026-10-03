import { expect, test } from "@playwright/test";

test("views share the complete draft and undo history; copy failure freezes the saved snapshot", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1100, height: 760 });
  await page.goto("/clipboard-editor.html?copyError=1");
  const draft =
    "# Release\nVersion 2\n\n- Text and images\n- Preview\n- Save a new entry\n\n> Keep the original.  \n\n";
  const editor = page.locator(".cm-merge-b .cm-content");
  await editor.fill(draft);
  for (const mode of ["预览", "左右对照", "修改对比", "编辑"]) {
    await page.getByRole("button", { name: mode, exact: true }).click();
    if (mode !== "预览") await expect(editor).toContainText("Keep the original.");
  }
  await page.getByRole("button", { name: "修改对比", exact: true }).click();
  await page.getByRole("button", { name: "恢复此处原文（可撤销）" }).first().click();
  await page.getByRole("button", { name: "撤销 ⌘Z", exact: true }).click();
  await expect(editor).toContainText("Keep the original.");
  await page.getByRole("button", { name: "保存并复制", exact: true }).click();
  await expect(page.getByText("已保存为新条目，但未能复制", { exact: true })).toBeVisible();
  await expect(editor).toHaveAttribute("contenteditable", "false");
  await expect(page.getByRole("combobox", { name: "文本格式" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "粗体", exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "重试复制", exact: true }).click();
  await expect(page.getByText("新条目 #279 已保存，原记录保留", { exact: true })).toBeVisible();
});

test("save failures retain the draft and the requested save-and-copy intent", async ({ page }) => {
  await page.goto("/clipboard-editor.html?saveError=1&plainCopy=1");
  await expect(page.getByText(/正在编辑纯文本副本/)).toBeVisible();
  await page.locator(".cm-merge-b .cm-content").fill("Uncommitted draft  \n");
  await page.getByRole("button", { name: "保存并复制", exact: true }).click();
  await expect(page.getByRole("button", { name: "重试保存并复制", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "重试保存", exact: true })).toHaveCount(0);
  await expect(page.locator(".cm-merge-b .cm-content")).toHaveAttribute("contenteditable", "true");
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.getByRole("button", { name: "继续编辑", exact: true }).click();
  await expect(page.locator(".cm-merge-b .cm-content")).toContainText("Uncommitted draft");
});

test("image export keeps native dimensions and flattens opaque covers at zoomed coordinates", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1100, height: 760 });
  await page.goto("/clipboard-editor.html?kind=image&copyError=1");
  await expect(page.getByText("1440 × 900 px · PNG", { exact: true })).toBeVisible();
  await page.evaluate(async () => {
    // @ts-expect-error Browser-side Vite module URL.
    const { editorBridge } = await import("/src/clipboard/editor/bridge.ts");
    editorBridge.save = async (draft: { kind: string; png: string }) => {
      const image = new Image();
      image.src = "data:image/png;base64," + draft.png;
      await image.decode();
      const canvas = document.createElement("canvas");
      canvas.width = image.width;
      canvas.height = image.height;
      const ctx = canvas.getContext("2d")!;
      ctx.drawImage(image, 0, 0);
      const pixel = [...ctx.getImageData(400, 500, 1, 1).data];
      document.body.dataset.exportedImage = JSON.stringify({
        width: image.width,
        height: image.height,
        pixel,
      });
      return 279;
    };
  });
  await page.getByRole("button", { name: "实色遮挡", exact: true }).click();
  const bounds = (await page.locator(".image-canvas").boundingBox())!;
  const scale = Math.min((bounds.width - 64) / 1440, (bounds.height - 64) / 900, 1);
  const x = bounds.x + (bounds.width - 1440 * scale) / 2,
    y = bounds.y + (bounds.height - 900 * scale) / 2;
  await page.mouse.move(x + 300 * scale, y + 450 * scale);
  await page.mouse.down();
  await page.mouse.move(x + 600 * scale, y + 550 * scale, { steps: 6 });
  await page.mouse.up();
  await expect(page.getByRole("button", { name: "撤销", exact: true })).toBeEnabled();
  await page.getByRole("button", { name: "保存并复制", exact: true }).click();
  await expect(page.locator("body")).toHaveAttribute(
    "data-exported-image",
    JSON.stringify({ width: 1440, height: 900, pixel: [239, 82, 82, 255] }),
  );
});

test("ignore-whitespace recomputes immediately and does not change saved source bytes", async ({
  page,
}) => {
  await page.goto("/clipboard-editor.html?copyError=1");
  const editor = page.locator(".cm-merge-b .cm-content");
  await editor.click();
  await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.insertText("  ");
  await page.getByRole("button", { name: "修改对比", exact: true }).click();
  await expect(page.locator(".change-count")).toHaveText("1 处修改");
  const before = await editor.innerText();
  await page.getByRole("checkbox", { name: "忽略空白", exact: true }).check();
  await expect(page.locator(".change-count")).toHaveText("0 处修改");
  expect(await editor.innerText()).toBe(before);
  await page.getByRole("checkbox", { name: "忽略空白", exact: true }).uncheck();
  await expect(page.locator(".change-count")).toHaveText("1 处修改");
});
