import assert from "node:assert/strict";
import test from "node:test";

import { parseWebFilesRoute, webFilesUrl } from "./web-files/navigation.ts";

test("parses the home and share routes", () => {
  assert.deepEqual(parseWebFilesRoute({ pathname: "/files", search: "" }), { kind: "home" });
  assert.deepEqual(parseWebFilesRoute({ pathname: "/s/team%20docs", search: "?path=design%2Freviews" }), {
    kind: "share",
    slug: "team docs",
    path: "design/reviews",
  });
});

test("builds stable URLs for folder history", () => {
  assert.equal(webFilesUrl({ kind: "home" }), "/files");
  assert.equal(webFilesUrl({ kind: "share", slug: "team docs", path: "" }), "/s/team%20docs");
  assert.equal(
    webFilesUrl({ kind: "share", slug: "team docs", path: "design/reviews" }),
    "/s/team%20docs?path=design%2Freviews",
  );
});

test("falls back to home for an invalid encoded slug", () => {
  assert.deepEqual(parseWebFilesRoute({ pathname: "/s/%E0%A4%A", search: "" }), { kind: "home" });
});
