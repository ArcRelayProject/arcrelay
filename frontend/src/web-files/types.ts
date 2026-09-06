export type WebAccessMode = "disabled" | "public" | "password";
export type PreviewKind = "text" | "image" | "audio" | "video" | "none";

export interface SiteView {
  siteName: string;
  version: string;
  filesEnabled: boolean;
}

export interface ShareView {
  name: string;
  slug: string;
  mode: WebAccessMode;
  unlocked: boolean;
  allowPreview: boolean;
  allowDownload: boolean;
}

export interface FileEntry {
  name: string;
  relativePath: string;
  kind: "file" | "folder";
  size: number;
  modifiedAtMs: number;
  mediaType: string;
  previewKind: PreviewKind;
}

export interface DirectoryPage {
  path: string;
  entries: FileEntry[];
  nextCursor: string | null;
}

export interface TextPreview {
  text: string;
  truncated: boolean;
  mediaType: string;
}
