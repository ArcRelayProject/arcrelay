import assert from "node:assert/strict";
import test from "node:test";

import {
  sortRemoteFileEntries,
  type RemoteFileSortDirection,
  type RemoteFileSortKey,
} from "./remoteFileSort.ts";
import type { RemoteFileEntry } from "./types.ts";

const entries: RemoteFileEntry[] = [
  {
    id: "folder-10",
    name: "folder 10",
    relativePath: "folder 10",
    kind: "folder",
    size: 0,
    modifiedAtMs: 100,
    revision: "1",
  },
  {
    id: "folder-2",
    name: "folder 2",
    relativePath: "folder 2",
    kind: "folder",
    size: 0,
    modifiedAtMs: 300,
    revision: "1",
  },
  {
    id: "zeta",
    name: "zeta.txt",
    relativePath: "zeta.txt",
    kind: "file",
    size: 20,
    modifiedAtMs: 200,
    revision: "1",
  },
  {
    id: "alpha",
    name: "alpha.pdf",
    relativePath: "alpha.pdf",
    kind: "file",
    size: 40,
    modifiedAtMs: 400,
    revision: "1",
  },
  {
    id: "beta",
    name: "beta.txt",
    relativePath: "beta.txt",
    kind: "file",
    size: 10,
    modifiedAtMs: 100,
    revision: "1",
  },
];

function names(sortKey: RemoteFileSortKey, sortDirection: RemoteFileSortDirection) {
  return sortRemoteFileEntries(entries, sortKey, sortDirection, "enUs").map((entry) => entry.name);
}

test("sorts names in both directions while keeping folders first", () => {
  assert.deepEqual(names("name", "ascending"), [
    "folder 2",
    "folder 10",
    "alpha.pdf",
    "beta.txt",
    "zeta.txt",
  ]);
  assert.deepEqual(names("name", "descending"), [
    "folder 10",
    "folder 2",
    "zeta.txt",
    "beta.txt",
    "alpha.pdf",
  ]);
});

test("sorts by modification date", () => {
  assert.deepEqual(names("modified", "ascending"), [
    "folder 10",
    "folder 2",
    "beta.txt",
    "zeta.txt",
    "alpha.pdf",
  ]);
  assert.deepEqual(names("modified", "descending"), [
    "folder 2",
    "folder 10",
    "alpha.pdf",
    "zeta.txt",
    "beta.txt",
  ]);
});

test("sorts files by type and uses name to break equal-type ties", () => {
  assert.deepEqual(names("type", "ascending"), [
    "folder 2",
    "folder 10",
    "alpha.pdf",
    "beta.txt",
    "zeta.txt",
  ]);
  assert.deepEqual(names("type", "descending"), [
    "folder 2",
    "folder 10",
    "beta.txt",
    "zeta.txt",
    "alpha.pdf",
  ]);
});

test("sorts files by size and does not mutate the source array", () => {
  const originalOrder = entries.map((entry) => entry.name);
  assert.deepEqual(names("size", "ascending"), [
    "folder 2",
    "folder 10",
    "beta.txt",
    "zeta.txt",
    "alpha.pdf",
  ]);
  assert.deepEqual(names("size", "descending"), [
    "folder 2",
    "folder 10",
    "alpha.pdf",
    "zeta.txt",
    "beta.txt",
  ]);
  assert.deepEqual(
    entries.map((entry) => entry.name),
    originalOrder,
  );
});
