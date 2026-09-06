import type { RemoteFileEntry, RemoteFileShare } from "./types";

export type RemoteFileTreeRow =
  | {
      kind: "share";
      key: string;
      shareId: string;
      path: "";
      name: string;
      depth: 0;
      writable: boolean;
      expanded: boolean;
      active: boolean;
    }
  | {
      kind: "folder";
      key: string;
      shareId: string;
      path: string;
      name: string;
      depth: number;
      writable: boolean;
      expanded: boolean;
      active: boolean;
      ancestor: boolean;
      childrenKnown: boolean;
      hasFolderChildren: boolean;
      entry: RemoteFileEntry;
    };

export function remoteTreeDirectoryKey(shareId: string, path: string) {
  return `${shareId}\u0000${path}`;
}

interface BuildRemoteFileTreeRowsOptions {
  shares: RemoteFileShare[];
  selectedShareId: string;
  currentPath: string;
  expandedShareIds: ReadonlySet<string>;
  expandedFolderKeys: ReadonlySet<string>;
  directories: ReadonlyMap<string, RemoteFileEntry[]>;
}

export function buildRemoteFileTreeRows({
  shares,
  selectedShareId,
  currentPath,
  expandedShareIds,
  expandedFolderKeys,
  directories,
}: BuildRemoteFileTreeRowsOptions): RemoteFileTreeRow[] {
  const rows: RemoteFileTreeRow[] = [];

  const appendFolders = (share: RemoteFileShare, parentPath: string, depth: number) => {
    const children = directories.get(remoteTreeDirectoryKey(share.id, parentPath)) ?? [];
    for (const entry of children) {
      if (entry.kind !== "folder") continue;
      const key = remoteTreeDirectoryKey(share.id, entry.relativePath);
      const knownChildren = directories.get(key);
      const expanded = expandedFolderKeys.has(key);
      rows.push({
        kind: "folder",
        key,
        shareId: share.id,
        path: entry.relativePath,
        name: entry.name,
        depth,
        writable: share.writable,
        expanded,
        active: selectedShareId === share.id && currentPath === entry.relativePath,
        ancestor: selectedShareId === share.id && currentPath.startsWith(`${entry.relativePath}/`),
        childrenKnown: knownChildren !== undefined,
        hasFolderChildren: Boolean(knownChildren?.some((child) => child.kind === "folder")),
        entry,
      });
      if (expanded && knownChildren) appendFolders(share, entry.relativePath, depth + 1);
    }
  };

  for (const share of shares) {
    const expanded = expandedShareIds.has(share.id);
    rows.push({
      kind: "share",
      key: `share:${share.id}`,
      shareId: share.id,
      path: "",
      name: share.name,
      depth: 0,
      writable: share.writable,
      expanded,
      active: selectedShareId === share.id && currentPath === "",
    });
    if (expanded) appendFolders(share, "", 1);
  }

  return rows;
}

export function ancestorPaths(path: string) {
  const parts = path.split("/").filter(Boolean);
  return parts.slice(0, -1).map((_, index) => parts.slice(0, index + 1).join("/"));
}
