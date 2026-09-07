import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const desktopRoot = path.resolve(scriptDirectory, "..");
const workspaceRoot = path.resolve(desktopRoot, "..");
const executableExtension = process.platform === "win32" ? ".exe" : "";
const binariesDirectory = path.join(desktopRoot, "binaries");
const artifactDirectory = process.env.SNIPTRA_ARTIFACT_DIR
  ? path.resolve(process.env.SNIPTRA_ARTIFACT_DIR)
  : null;

const sourceCandidates = [
  process.env.SNIPTRA_SOURCE_DIR,
  path.join(workspaceRoot, "sniptra"),
].filter(Boolean);
const sourceRoot = sourceCandidates.find((candidate) =>
  existsSync(path.join(candidate, "apps", "screenshot-desktop", "Cargo.toml")),
);

const hostTarget = () =>
  execFileSync("rustc", ["--print", "host-tuple"], { encoding: "utf8" }).trim();
const requestedTarget = process.env.SNIPTRA_SIDECAR_TARGET || hostTarget();
const cargoTargetBase = process.env.CARGO_TARGET_DIR || path.join(workspaceRoot, "target");
const cargoTargetDirectory =
  process.env.SNIPTRA_CARGO_TARGET_DIR || path.join(cargoTargetBase, "sniptra-sidecar");

mkdirSync(binariesDirectory, { recursive: true });

function build(target) {
  const buildEnvironment = {
    ...process.env,
    CARGO_TARGET_DIR: cargoTargetDirectory,
  };
  if (target.endsWith("-apple-darwin")) {
    buildEnvironment.MACOSX_DEPLOYMENT_TARGET =
      process.env.SNIPTRA_MACOSX_DEPLOYMENT_TARGET || "12.3";
  }
  const result = spawnSync(
    "cargo",
    [
      "build",
      "--release",
      "--manifest-path",
      path.join(sourceRoot, "apps", "screenshot-desktop", "Cargo.toml"),
      "--target",
      target,
      "--package",
      "screenshot-desktop",
      "--package",
      "screenshot-ocr-worker",
    ],
    {
      cwd: sourceRoot,
      env: buildEnvironment,
      stdio: "inherit",
    },
  );
  if (result.status !== 0) process.exit(result.status ?? 1);
  return {
    sniptra: path.join(
      cargoTargetDirectory,
      target,
      "release",
      `sniptra${executableExtension}`,
    ),
    worker: path.join(
      cargoTargetDirectory,
      target,
      "release",
      `sniptra-ocr-worker${executableExtension}`,
    ),
  };
}

function destination(name, target) {
  return path.join(binariesDirectory, `${name}-${target}${executableExtension}`);
}

function install(source, output) {
  copyFileSync(source, output);
  if (process.platform !== "win32") chmodSync(output, 0o755);
}

const macOSUniversalArchitectures = [
  ["arm64", "aarch64-apple-darwin"],
  ["x86_64", "x86_64-apple-darwin"],
];

function installMacOSUniversalArtifact(name, source) {
  for (const [architecture, target] of macOSUniversalArchitectures) {
    const output = destination(name, target);
    execFileSync("lipo", [source, "-thin", architecture, "-output", output], {
      stdio: "inherit",
    });
    chmodSync(output, 0o755);
  }
  install(source, destination(name, "universal-apple-darwin"));
}

function verifyIntegrationExecutable(executable) {
  const info = JSON.parse(
    execFileSync(executable, ["integration-info"], { encoding: "utf8" }),
  );
  if (
    info.application !== "sniptra" ||
    info.protocol_version !== 1 ||
    info.one_shot_command !== "one-shot" ||
    info.profile_directory_environment !== "SNIPTRA_PROFILE_DIR"
  ) {
    throw new Error(`Incompatible Sniptra integration artifact: ${executable}`);
  }
  return info;
}

function verifyChecksums(directory, expectedNames) {
  const checksumPath = path.join(directory, "SHA256SUMS.txt");
  if (!existsSync(checksumPath)) {
    throw new Error(`Sniptra artifact checksum file is missing: ${checksumPath}`);
  }
  const verified = new Set();
  for (const line of readFileSync(checksumPath, "utf8").split(/\r?\n/)) {
    if (!line.trim()) continue;
    const match = line.match(/^([a-fA-F0-9]{64})\s+\*?(.+)$/);
    if (!match) throw new Error(`Invalid checksum entry: ${line}`);
    if (path.basename(match[2]) !== match[2] || match[2].includes("\\")) throw new Error("Invalid checksum filename");
    const file = path.join(directory, match[2]);
    const actual = createHash("sha256").update(readFileSync(file)).digest("hex");
    if (actual.toLowerCase() !== match[1].toLowerCase()) {
      throw new Error(`Checksum mismatch for ${file}`);
    }
    verified.add(match[2]);
  }
  for (const name of expectedNames) {
    if (!verified.has(name)) throw new Error(`Missing checksum for ${name}`);
  }
}

if (artifactDirectory) {
  const sniptra = path.join(artifactDirectory, `sniptra${executableExtension}`);
  const worker = path.join(artifactDirectory, `sniptra-ocr-worker${executableExtension}`);
  const integrationInfo = path.join(artifactDirectory, "integration-info.json");
  for (const file of [sniptra, worker, integrationInfo]) {
    if (!existsSync(file)) throw new Error(`Sniptra integration artifact is missing: ${file}`);
  }
  verifyChecksums(artifactDirectory, [
    path.basename(sniptra),
    path.basename(worker),
    path.basename(integrationInfo),
  ]);
  if (process.platform !== "win32") chmodSync(sniptra, 0o755);
  const executableInfo = verifyIntegrationExecutable(sniptra);
  if (process.platform !== "win32") chmodSync(worker, 0o755);
  const handshake = execFileSync(worker, [], {
    input: '{"type":"hello","payload":{"protocol_version":1}}\n{"type":"shutdown"}\n',
    encoding: "utf8", timeout: 30000,
  });
  const ready = JSON.parse(handshake.trim().split(/\r?\n/)[0]);
  if (ready.type !== "ready" || ready.payload?.protocol_version !== 1) {
    throw new Error("Sniptra OCR worker protocol startup failed");
  }
  const notices = path.join(binariesDirectory, "sniptra-notices");
  mkdirSync(notices, { recursive: true });
  for (const name of ["BINARY-LICENSE.txt", "README.md"]) {
    // Older Jenkins integration directories predate bundled notices. The official
    // GitHub downloader separately requires these entries in its verified ZIP.
    const notice = path.join(artifactDirectory, name);
    if (existsSync(notice)) copyFileSync(notice, path.join(notices, name));
  }
  const archivedInfo = JSON.parse(readFileSync(integrationInfo, "utf8").replace(/^\uFEFF/, ""));
  if (JSON.stringify(executableInfo) !== JSON.stringify(archivedInfo)) {
    throw new Error("Sniptra integration metadata does not match the executable");
  }
  if (requestedTarget === "universal-apple-darwin") {
    if (process.platform !== "darwin") {
      throw new Error("Universal macOS Sniptra sidecars must be prepared on macOS.");
    }
    installMacOSUniversalArtifact("sniptra", sniptra);
    installMacOSUniversalArtifact("sniptra-ocr-worker", worker);
  } else {
    install(sniptra, destination("sniptra", requestedTarget));
    install(worker, destination("sniptra-ocr-worker", requestedTarget));
  }
  console.log(`Prepared Sniptra sidecars for ${requestedTarget} from CI artifacts at ${artifactDirectory}`);
  process.exit(0);
}

if (!sourceRoot) {
  if (process.env.ARCRELAY_REQUIRE_SNIPTRA === "1") {
    throw new Error(
      `Sniptra source was not found. Set SNIPTRA_ARTIFACT_DIR, SNIPTRA_SOURCE_DIR, or check out the repository at ${path.join(workspaceRoot, "sniptra")}.`,
    );
  }
  console.log("Sniptra is not available; building ArcRelay without the optional screenshot sidecars.");
  process.exit(0);
}

if (requestedTarget === "universal-apple-darwin") {
  if (process.platform !== "darwin") {
    throw new Error("Universal macOS Sniptra sidecars must be built on macOS.");
  }
  const arm64 = build("aarch64-apple-darwin");
  const x64 = build("x86_64-apple-darwin");
  for (const [name, arm64Source, x64Source] of [
    ["sniptra", arm64.sniptra, x64.sniptra],
    ["sniptra-ocr-worker", arm64.worker, x64.worker],
  ]) {
    install(arm64Source, destination(name, "aarch64-apple-darwin"));
    install(x64Source, destination(name, "x86_64-apple-darwin"));

    const universalOutput = destination(name, requestedTarget);
    execFileSync(
      "lipo",
      ["-create", arm64Source, x64Source, "-output", universalOutput],
      { stdio: "inherit" },
    );
    chmodSync(universalOutput, 0o755);
  }
} else {
  const built = build(requestedTarget);
  install(built.sniptra, destination("sniptra", requestedTarget));
  install(built.worker, destination("sniptra-ocr-worker", requestedTarget));
}

console.log(`Prepared Sniptra sidecars for ${requestedTarget} from ${sourceRoot}`);
