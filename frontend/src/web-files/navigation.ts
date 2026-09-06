export type WebFilesRoute =
  | { kind: "home" }
  | { kind: "share"; slug: string; path: string };

export function parseWebFilesRoute(location: Pick<Location, "pathname" | "search">): WebFilesRoute {
  const match = location.pathname.match(/^\/s\/([^/]+)\/?$/);
  if (!match) return { kind: "home" };
  try {
    return {
      kind: "share",
      slug: decodeURIComponent(match[1]),
      path: new URLSearchParams(location.search).get("path") ?? "",
    };
  } catch {
    return { kind: "home" };
  }
}

export function webFilesUrl(route: WebFilesRoute): string {
  if (route.kind === "home") return "/files";
  const query = new URLSearchParams();
  if (route.path) query.set("path", route.path);
  const suffix = query.size ? `?${query}` : "";
  return `/s/${encodeURIComponent(route.slug)}${suffix}`;
}
