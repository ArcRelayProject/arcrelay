import { readFileSync, writeFileSync } from "node:fs";

const semver = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

function read(path) {
  return readFileSync(path, "utf8");
}

function write(path, value) {
  writeFileSync(path, value);
}

function matchOne(path, pattern, label) {
  const match = read(path).match(pattern);
  if (!match) throw new Error(`Cannot find ${label} in ${path}`);
  return match[1];
}

function currentVersions() {
  const cargo = matchOne(
    "Cargo.toml",
    /\[package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/,
    "package version",
  );
  const packageJson = JSON.parse(read("package.json"));
  const packageLock = JSON.parse(read("package-lock.json"));
  const tauri = JSON.parse(read("tauri.conf.json"));
  const projectYaml = [...read("gen/apple-macos/project.yml").matchAll(
    /^\s+(?:MARKETING_VERSION|CURRENT_PROJECT_VERSION):\s+([^\s]+)$/gm,
  )].map((match) => match[1]);
  const xcodeProject = [...read("gen/apple-macos/ArcRelay.xcodeproj/project.pbxproj").matchAll(
    /^\s+(?:MARKETING_VERSION|CURRENT_PROJECT_VERSION) = ([^;]+);$/gm,
  )].map((match) => match[1]);
  const frontend = [
    matchOne("frontend/src/App.svelte", /currentVersion:\s*"([^"]+)"/, "frontend version"),
    matchOne("frontend/src/bridge.mock.ts", /currentVersion:\s*"([^"]+)"/, "mock version"),
  ];

  return new Map([
    ["Cargo.toml", cargo],
    ["package.json", packageJson.version],
    ["package-lock.json", packageLock.version],
    ["package-lock.json workspace", packageLock.packages?.[""]?.version],
    ["tauri.conf.json", tauri.version],
    ...projectYaml.map((version, index) => [`project.yml entry ${index + 1}`, version]),
    ...xcodeProject.map((version, index) => [`project.pbxproj entry ${index + 1}`, version]),
    ...frontend.map((version, index) => [`frontend fallback ${index + 1}`, version]),
  ]);
}

function check(expected) {
  const versions = currentVersions();
  const reference = expected ?? versions.values().next().value;
  if (!semver.test(reference)) throw new Error(`Invalid SemVer: ${reference}`);
  const mismatches = [...versions].filter(([, version]) => version !== reference);
  if (mismatches.length) {
    throw new Error(
      `Expected ${reference}:\n${mismatches.map(([file, version]) => `  ${file}: ${version}`).join("\n")}`,
    );
  }
  console.log(`ArcRelay desktop version ${reference} is consistent.`);
  return reference;
}

function replace(path, pattern, replacement, expectedCount) {
  const source = read(path);
  let count = 0;
  const updated = source.replace(pattern, (...args) => {
    count += 1;
    return typeof replacement === "function" ? replacement(...args) : replacement;
  });
  if (count !== expectedCount) {
    throw new Error(`Expected ${expectedCount} version fields in ${path}, found ${count}`);
  }
  write(path, updated);
}

function setVersion(next) {
  if (!semver.test(next)) throw new Error(`Invalid SemVer: ${next}`);
  const previous = check();

  replace(
    "Cargo.toml",
    /(\[package\][\s\S]*?\nversion\s*=\s*")[^"]+(" )?/,
    (whole, prefix, suffix = "") => `${prefix}${next}${suffix}`,
    1,
  );

  const cargoLockPattern = new RegExp(
    `(name = "arcrelay-desktop"\\r?\\nversion = ")${previous.replaceAll(".", "\\.")}(" )?`,
  );
  replace(
    "Cargo.lock",
    cargoLockPattern,
    (whole, prefix, suffix = "") => `${prefix}${next}${suffix}`,
    1,
  );

  for (const path of ["package.json", "package-lock.json", "tauri.conf.json"]) {
    const data = JSON.parse(read(path));
    data.version = next;
    if (path === "package-lock.json") data.packages[""].version = next;
    write(path, `${JSON.stringify(data, null, 2)}\n`);
  }

  replace(
    "gen/apple-macos/project.yml",
    /(^\s+(?:MARKETING_VERSION|CURRENT_PROJECT_VERSION):\s+)[^\s]+$/gm,
    (whole, prefix) => `${prefix}${next}`,
    6,
  );
  replace(
    "gen/apple-macos/ArcRelay.xcodeproj/project.pbxproj",
    /(^\s+(?:MARKETING_VERSION|CURRENT_PROJECT_VERSION) = )[^;]+;/gm,
    (whole, prefix) => `${prefix}${next};`,
    12,
  );
  for (const path of ["frontend/src/App.svelte", "frontend/src/bridge.mock.ts"]) {
    replace(
      path,
      /currentVersion:\s*"[^"]+"/,
      `currentVersion: "${next}"`,
      1,
    );
  }
  check(next);
}

const [command, value] = process.argv.slice(2);
if (command === "--check") {
  check(value?.replace(/^v/, ""));
} else if (command) {
  setVersion(command.replace(/^v/, ""));
} else {
  throw new Error("Usage: node scripts/version.mjs --check [VERSION] | VERSION");
}
