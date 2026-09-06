import { readFileSync } from "node:fs";

const semverPattern = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;
const testPatchFloor = 10_000;
const platformPatchLimit = 65_535;

export function testVersion(baseVersion, runNumber) {
  const match = baseVersion.match(/^(\d+)\.(\d+)\.(\d+)$/);
  if (!match) {
    throw new Error(`Test releases require a stable base version, received ${baseVersion}`);
  }
  if (!/^\d+$/.test(String(runNumber)) || Number(runNumber) < 1) {
    throw new Error(`Invalid GitHub run number: ${runNumber}`);
  }
  const stablePatch = Number(match[3]);
  const testPatch = testPatchFloor + Number(runNumber);
  if (stablePatch >= testPatchFloor) {
    throw new Error(`Stable patch versions must be below ${testPatchFloor}`);
  }
  if (testPatch > platformPatchLimit) {
    throw new Error(`Test patch version exceeds the platform limit ${platformPatchLimit}`);
  }
  return `${match[1]}.${match[2]}.${testPatch}`;
}

export function validateStableVersion(version) {
  const match = version.match(/^(\d+)\.(\d+)\.(\d+)$/);
  if (!match || Number(match[3]) >= testPatchFloor) {
    throw new Error(`Stable versions must use numeric SemVer with a patch below ${testPatchFloor}`);
  }
  return version;
}

export function validateUpdaterManifest(manifest, expectedVersion, requiredTargets) {
  if (!semverPattern.test(expectedVersion)) {
    throw new Error(`Invalid expected version: ${expectedVersion}`);
  }
  if (manifest.version !== expectedVersion) {
    throw new Error(`Expected updater version ${expectedVersion}, found ${manifest.version}`);
  }
  if (!manifest.platforms || typeof manifest.platforms !== "object") {
    throw new Error("Updater manifest does not contain platforms");
  }
  for (const target of requiredTargets) {
    const platform = manifest.platforms[target];
    let trustedDownload = false;
    try {
      const url = new URL(platform?.url);
      trustedDownload =
        (url.origin === "https://github.com" &&
          url.pathname.startsWith("/ArcRelayProject/arcrelay/releases/download/")) ||
        (url.origin === "https://api.github.com" &&
          /^\/repos\/ArcRelayProject\/arcrelay\/releases\/assets\/\d+$/.test(url.pathname));
    } catch {
      trustedDownload = false;
    }
    if (!trustedDownload) {
      throw new Error(`Updater target ${target} has an invalid download URL`);
    }
    if (typeof platform.signature !== "string" || platform.signature.trim().length < 32) {
      throw new Error(`Updater target ${target} has no usable signature`);
    }
  }
  return manifest;
}

const [command, ...args] = process.argv.slice(2);
if (command === "test-version") {
  process.stdout.write(testVersion(args[0], args[1]));
} else if (command === "validate-stable-version") {
  process.stdout.write(validateStableVersion(args[0]));
} else if (command === "validate-manifest") {
  const [path, expectedVersion, targetList] = args;
  const manifest = JSON.parse(readFileSync(path, "utf8"));
  validateUpdaterManifest(manifest, expectedVersion, targetList.split(",").filter(Boolean));
  console.log(`Updater manifest ${path} is complete for ${expectedVersion}.`);
} else if (command) {
  throw new Error(`Unknown release-channel command: ${command}`);
}
