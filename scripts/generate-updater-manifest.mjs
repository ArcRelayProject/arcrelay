import { readFileSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const TARGETS = [
  {
    keys: ["darwin-aarch64", "darwin-aarch64-app"],
    payload: (version) => `ArcRelay_${version}_aarch64.app.tar.gz`,
  },
  {
    keys: ["darwin-x86_64", "darwin-x86_64-app"],
    payload: (version) => `ArcRelay_${version}_x64.app.tar.gz`,
  },
  {
    keys: ["windows-x86_64", "windows-x86_64-nsis"],
    payload: (version) => `ArcRelay_${version}_x64-setup.exe`,
  },
  {
    keys: ["windows-aarch64", "windows-aarch64-nsis"],
    payload: (version) => `ArcRelay_${version}_arm64-setup.exe`,
  },
];

export const REQUIRED_UPDATER_TARGETS = TARGETS.flatMap(({ keys }) => keys);

function requiredAsset(assets, name) {
  const matches = assets.filter((asset) => asset.name === name || asset.label === name);
  if (matches.length !== 1) {
    throw new Error(`Expected exactly one release asset named ${name}, found ${matches.length}`);
  }
  const asset = matches[0];
  if (!Number.isSafeInteger(asset.id) || asset.id <= 0) {
    throw new Error(`Release asset ${name} has an invalid id`);
  }
  return asset;
}

function signaturePath(directory, name) {
  if (basename(name) !== name || name === "." || name === "..") {
    throw new Error(`Invalid signature asset name: ${name}`);
  }
  return join(directory, name);
}

export function buildUpdaterManifest({
  release,
  assets,
  version,
  signatureDirectory,
}) {
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`Invalid updater version: ${version}`);
  }
  if (!release || typeof release !== "object" || !Array.isArray(assets)) {
    throw new Error("Release metadata and assets are required");
  }

  const platforms = {};
  for (const target of TARGETS) {
    const payloadName = target.payload(version);
    const payload = requiredAsset(assets, payloadName);
    const signatureName = `${payloadName}.sig`;
    requiredAsset(assets, signatureName);
    const signature = readFileSync(
      signaturePath(signatureDirectory, signatureName),
      "utf8",
    ).trim();
    if (!signature) {
      throw new Error(`Updater signature ${signatureName} is empty`);
    }
    const entry = {
      signature,
      url: `https://api.github.com/repos/ArcRelayProject/arcrelay/releases/assets/${payload.id}`,
    };
    for (const key of target.keys) platforms[key] = entry;
  }

  const publicationDate = release.published_at ?? release.created_at;
  if (typeof publicationDate !== "string" || Number.isNaN(Date.parse(publicationDate))) {
    throw new Error("Release metadata has no valid publication date");
  }

  return {
    version,
    notes: typeof release.body === "string" ? release.body : "",
    pub_date: new Date(publicationDate).toISOString(),
    platforms,
  };
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url)) {
  const [releasePath, assetsPath, signatureDirectory, outputPath, version] = process.argv.slice(2);
  if (!releasePath || !assetsPath || !signatureDirectory || !outputPath || !version) {
    throw new Error(
      "Usage: generate-updater-manifest.mjs RELEASE_JSON ASSETS_JSON SIGNATURE_DIR OUTPUT VERSION",
    );
  }
  const manifest = buildUpdaterManifest({
    release: JSON.parse(readFileSync(releasePath, "utf8")),
    assets: JSON.parse(readFileSync(assetsPath, "utf8")),
    version,
    signatureDirectory,
  });
  writeFileSync(outputPath, `${JSON.stringify(manifest, null, 2)}\n`);
}
