import type { DirectoryPage, FileEntry, ShareView, SiteView, TextPreview } from "./types";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    credentials: "same-origin",
    ...init,
    headers: {
      ...(init?.body ? { "Content-Type": "application/json" } : {}),
      ...init?.headers,
    },
  });
  if (!response.ok) {
    const body = await response.json().catch(() => null) as {
      error?: { code?: string; message?: string } | string;
    } | null;
    const message = typeof body?.error === "string" ? body.error : body?.error?.message;
    throw new Error(message || `request failed (${response.status})`);
  }
  return response.json() as Promise<T>;
}

export const api = {
  site: () => request<SiteView>("/api/v1/site"),
  shares: () => request<ShareView[]>("/api/v1/shares"),
  share: (slug: string) => request<{ share: ShareView }>(`/api/v1/shares/${encodeURIComponent(slug)}/metadata`),
  metadata: (slug: string, path: string) => request<{ share: ShareView; entry: FileEntry }>(endpoint(slug, "metadata", path)),
  unlock: (slug: string, password: string) => request<{ unlocked: boolean }>(`/api/v1/shares/${encodeURIComponent(slug)}/unlock`, {
    method: "POST",
    body: JSON.stringify({ password }),
  }),
  lock: (slug: string) => request<{ locked: boolean }>(`/api/v1/shares/${encodeURIComponent(slug)}/lock`, { method: "POST" }),
  logout: () => request<{ loggedOut: boolean }>("/api/v1/session/logout", { method: "POST" }),
  entries: (slug: string, path: string, cursor?: string) => {
    const query = new URLSearchParams({ path, limit: "100" });
    if (cursor) query.set("cursor", cursor);
    return request<DirectoryPage>(`/api/v1/shares/${encodeURIComponent(slug)}/entries?${query}`);
  },
  text: (slug: string, path: string) => request<TextPreview>(endpoint(slug, "text", path)),
};

export function endpoint(slug: string, action: "content" | "thumbnail" | "text" | "metadata", path: string, extra?: Record<string, string>) {
  const query = new URLSearchParams({ path, ...extra });
  return `/api/v1/shares/${encodeURIComponent(slug)}/${action}?${query}`;
}
