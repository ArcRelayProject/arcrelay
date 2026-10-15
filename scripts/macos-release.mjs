#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import {
  appendFileSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(import.meta.url);
const pause = (seconds) =>
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, seconds * 1000);

function run(command, args, options = {}) {
  const { logFile, ...spawnOptions } = options;
  const result = spawnSync(command, args, {
    stdio: logFile ? ["ignore", "pipe", "pipe"] : "inherit",
    encoding: "utf8",
    ...spawnOptions,
  });
  if (logFile) appendFileSync(logFile, `${result.stdout ?? ""}${result.stderr ?? ""}`);
  if (result.error) throw result.error;
  return result.status ?? 1;
}

export function retry(command, args, { execute = run, wait = pause, report = console.error } = {}) {
  const delays = [15, 30, 60, 120];
  for (let attempt = 0; ; attempt += 1) {
    const status = execute(command, args);
    if (status === 0 || attempt === delays.length) return status;
    report(`Apple ticket check failed (${status}); retrying in ${delays[attempt]}s.`);
    wait(delays[attempt]);
  }
}

// Tauri CLI 2.11.4 discards the exit status of `xcrun stapler staple`.
// Intercept only this operation, before Tauri creates the DMG and updater archive.
// Keep a separate failure marker so a swallowed child failure still blocks upload.
export function runXcrun(args, { execute = run, wait = pause, env = process.env } = {}) {
  if (args[0] !== "stapler" || args[1] !== "staple") {
    return execute("/usr/bin/xcrun", args);
  }
  if (!env.ARCRELAY_NOTARIZATION_STATE) throw new Error("Missing release notarization state");
  const state = env.ARCRELAY_NOTARIZATION_STATE;
  const report = (message) => appendFileSync(join(state, "log"), `${message}\n`);
  let status;
  try {
    status = retry("/usr/bin/xcrun", args, {
      wait,
      report,
      execute(command, stapleArgs) {
        const stapled = execute(command, stapleArgs, { logFile: join(state, "log") });
        if (stapled !== 0) return stapled;
        return execute(command, ["stapler", "validate", args.at(-1)], {
          logFile: join(state, "log"),
        });
      },
    });
  } catch (error) {
    writeFileSync(join(state, "failed"), `Ticket attachment failed: ${error.message}\n`);
    throw error;
  }
  if (status !== 0) {
    writeFileSync(
      join(state, "failed"),
      `Cannot staple and validate ${args.at(-1)} (exit ${status}).\n`,
    );
  }
  report(`Staple and validate ${args.at(-1)}: exit ${status}.`);
  return status;
}

export function runTauri(args, { execute = run } = {}) {
  const state = mkdtempSync(join(tmpdir(), "arcrelay-notarization-"));
  try {
    symlinkSync(script, join(state, "xcrun"));
    const status = execute("npm", ["run", "tauri", "--", ...args], {
      env: {
        ...process.env,
        PATH: `${state}:${process.env.PATH}`,
        ARCRELAY_NOTARIZATION_STATE: state,
      },
    });
    if (existsSync(join(state, "log"))) console.error(readFileSync(join(state, "log"), "utf8"));
    if (existsSync(join(state, "failed"))) {
      console.error(readFileSync(join(state, "failed"), "utf8"));
      return 1;
    }
    return status;
  } finally {
    rmSync(state, { recursive: true, force: true });
  }
}

function requireSuccess(command, args) {
  if (run(command, args) !== 0) throw new Error(`${command} failed`);
}

function verifyPackage(target) {
  const arch = { "aarch64-apple-darwin": "aarch64", "x86_64-apple-darwin": "x64" }[target];
  if (!arch) throw new Error(`Unsupported macOS target: ${target}`);
  const identity = process.env.APPLE_SIGNING_IDENTITY;
  if (!identity) throw new Error("Missing Apple signing identity");
  const { version } = JSON.parse(readFileSync("package.json", "utf8"));
  const dmg = resolve(`target/${target}/release/bundle/dmg/ArcRelay_${version}_${arch}.dmg`);
  if (!existsSync(dmg)) throw new Error(`Missing release package: ${dmg}`);
  requireSuccess("codesign", ["--verify", "--strict", "--verbose=4", dmg]);
  const mount = mkdtempSync(join(tmpdir(), "arcrelay-verify-"));
  let mounted = false;
  try {
    requireSuccess("hdiutil", ["attach", dmg, "-nobrowse", "-readonly", "-mountpoint", mount]);
    mounted = true;
    const app = join(mount, "ArcRelay.app");
    requireSuccess("codesign", ["--verify", "--deep", "--strict", "--verbose=4", app]);
    const signature = spawnSync("codesign", ["-d", "--verbose=4", app], { encoding: "utf8" });
    if (signature.status !== 0 || !signature.stderr.split("\n").includes(`Authority=${identity}`)) {
      throw new Error("The packaged app does not have the expected Developer ID signature");
    }
    if (retry("/usr/bin/xcrun", ["stapler", "validate", app]) !== 0) {
      throw new Error("The packaged app failed notarization ticket validation");
    }
    if (retry("spctl", ["--assess", "--type", "execute", "--verbose=4", app]) !== 0) {
      throw new Error("The packaged app failed Gatekeeper assessment");
    }
  } finally {
    // Never recursively remove a directory while an installer is still mounted.
    if (mounted) requireSuccess("hdiutil", ["detach", mount]);
    rmdirSync(mount);
  }
}

if (
  process.argv[1] &&
  (basename(process.argv[1]) === "xcrun" || resolve(process.argv[1]) === script)
) {
  try {
    if (basename(process.argv[1]) === "xcrun") {
      process.exitCode = runXcrun(process.argv.slice(2));
    } else if (process.argv[2] === "tauri") {
      process.exitCode = runTauri(process.argv.slice(3));
    } else if (process.argv[2] === "verify") {
      verifyPackage(process.argv[3]);
    } else {
      throw new Error("Usage: macos-release.mjs tauri <args> | verify <target>");
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
