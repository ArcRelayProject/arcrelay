import assert from "node:assert/strict";
import test from "node:test";

import {
  buildRemoteFileBackgroundMenu,
  buildRemoteFileContextMenu,
  buildRemoteFileTreeMenu,
} from "./remoteFileContextMenu.ts";

test("folder menu exposes open and file operations", () => {
  assert.deepEqual(
    buildRemoteFileContextMenu({ kind: "folder", writable: true, busy: false, language: "zhCn" }),
    [
      { id: "open", label: "打开", enabled: true },
      { id: "separator" },
      { id: "uploadFiles", label: "上传文件到此处", enabled: true },
      { id: "uploadFolder", label: "上传文件夹到此处", enabled: true },
      { id: "newFolder", label: "新建子文件夹", enabled: true },
      { id: "separator" },
      { id: "download", label: "下载", enabled: true },
      { id: "rename", label: "重命名", enabled: true },
      { id: "separator" },
      { id: "delete", label: "删除", enabled: true },
    ],
  );
});

test("file menu opens with the default application and respects read-only permissions", () => {
  assert.deepEqual(
    buildRemoteFileContextMenu({ kind: "file", writable: false, busy: false, language: "enUs" }),
    [
      { id: "open", label: "Open", enabled: true },
      { id: "separator" },
      { id: "download", label: "Download", enabled: true },
      { id: "rename", label: "Rename", enabled: false },
      { id: "separator" },
      { id: "delete", label: "Delete", enabled: false },
    ],
  );
});

test("background and tree menus expose location-appropriate actions", () => {
  assert.deepEqual(
    buildRemoteFileBackgroundMenu({ writable: true, busy: false, language: "zhCn" }).map((item) => item.id),
    ["uploadFiles", "uploadFolder", "newFolder", "separator", "refresh"],
  );
  assert.deepEqual(
    buildRemoteFileTreeMenu({ kind: "folder", expanded: true, busy: false, language: "enUs" }).map((item) => item.id),
    ["open", "collapse", "separator", "refresh"],
  );
});

test("busy state disables every actionable item", () => {
  const items = buildRemoteFileContextMenu({ kind: "folder", writable: true, busy: true, language: "zhCn" });
  assert.ok(items.filter((item) => item.id !== "separator").every((item) => item.enabled === false));
});
