import { localeFor } from "./localization.ts";
import type { LanguagePreference, RemoteFileEntry } from "./types";

export type RemoteFileSortKey = "name" | "modified" | "type" | "size";
export type RemoteFileSortDirection = "ascending" | "descending";

export function sortRemoteFileEntries(
  entries: RemoteFileEntry[],
  sortKey: RemoteFileSortKey,
  sortDirection: RemoteFileSortDirection,
  language: LanguagePreference,
) {
  const locale = localeFor(language);
  const collator = new Intl.Collator(locale, { numeric: true });
  const direction = sortDirection === "ascending" ? 1 : -1;

  return [...entries].sort((left, right) => {
    // Keep directories together at the top, matching the file browser convention.
    if (left.kind !== right.kind) return left.kind === "folder" ? -1 : 1;

    let comparison = 0;
    if (sortKey === "name") comparison = collator.compare(left.name, right.name);
    else if (sortKey === "modified") comparison = left.modifiedAtMs - right.modifiedAtMs;
    else if (sortKey === "type") comparison = left.kind === "file"
      ? collator.compare(fileExtension(left.name), fileExtension(right.name))
      : 0;
    else comparison = left.size - right.size;

    // Equal dates, sizes, and types still need a deterministic, visible order.
    return comparison === 0 ? collator.compare(left.name, right.name) : comparison * direction;
  });
}

function fileExtension(name: string) {
  const separator = name.lastIndexOf(".");
  return separator > 0 && separator < name.length - 1 ? name.slice(separator + 1) : "";
}
