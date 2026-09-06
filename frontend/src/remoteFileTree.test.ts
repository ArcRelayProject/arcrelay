import assert from "node:assert/strict";
import test from "node:test";

import { ancestorPaths, buildRemoteFileTreeRows, remoteTreeDirectoryKey } from "./remoteFileTree.ts";
import type { RemoteFileEntry, RemoteFileShare } from "./types.ts";

const shares: RemoteFileShare[] = [{ id: "downloads", name: "Downloads", writable: true }];
const root: RemoteFileEntry[] = [
  { name: "archive", relativePath: "archive", kind: "folder", size: 0, modifiedAtMs: 1 },
];
const archive: RemoteFileEntry[] = [
  { name: "2026", relativePath: "archive/2026", kind: "folder", size: 0, modifiedAtMs: 2 },
];

test("a selected top-level share can remain collapsed", () => {
  const rows = buildRemoteFileTreeRows({
    shares,
    selectedShareId: "downloads",
    currentPath: "",
    expandedShareIds: new Set(),
    expandedFolderKeys: new Set(),
    directories: new Map([[remoteTreeDirectoryKey("downloads", ""), root]]),
  });
  assert.deepEqual(rows.map((row) => row.name), ["Downloads"]);
  assert.equal(rows[0].active, true);
  assert.equal(rows[0].expanded, false);
});

test("expanded folders render recursively beyond the second level", () => {
  const rows = buildRemoteFileTreeRows({
    shares,
    selectedShareId: "downloads",
    currentPath: "archive/2026",
    expandedShareIds: new Set(["downloads"]),
    expandedFolderKeys: new Set([remoteTreeDirectoryKey("downloads", "archive")]),
    directories: new Map([
      [remoteTreeDirectoryKey("downloads", ""), root],
      [remoteTreeDirectoryKey("downloads", "archive"), archive],
    ]),
  });
  assert.deepEqual(rows.map((row) => [row.name, row.depth]), [
    ["Downloads", 0],
    ["archive", 1],
    ["2026", 2],
  ]);
  assert.equal(rows[1].kind === "folder" && rows[1].ancestor, true);
  assert.equal(rows[2].active, true);
});

test("ancestor paths exclude the current folder itself", () => {
  assert.deepEqual(ancestorPaths("archive/2026/August"), ["archive", "archive/2026"]);
});
