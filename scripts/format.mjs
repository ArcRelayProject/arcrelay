import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

// Unchanged legacy formatting is accepted once. Modified and new source must comply.
const baseline = JSON.parse(
  readFileSync(new URL("../.format-baseline.json", import.meta.url), "utf8"),
);
function walk(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return walk(path);
    const normalized = path.replaceAll("\\", "/");
    if (!/\.(?:ts|svelte|css|mjs|js)$/.test(normalized) || normalized.endsWith("ipc/generated.ts"))
      return [];
    const digest = createHash("sha256").update(readFileSync(path)).digest("hex");
    return baseline[normalized] === digest ? [] : [path];
  });
}
const paths = [
  ...walk("frontend/src"),
  ...walk("scripts"),
  ...walk("tests"),
  "eslint.config.js",
  ".prettierrc.json",
];
if (paths.length) {
  const result = spawnSync(
    process.execPath,
    [
      "node_modules/prettier/bin/prettier.cjs",
      process.argv.includes("--write") ? "--write" : "--check",
      ...paths,
    ],
    { stdio: "inherit" },
  );
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
}
