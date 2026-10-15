import assert from "node:assert/strict";
import { mkdtempSync, existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { retry, runTauri, runXcrun } from "./macos-release.mjs";

function stateFor(t) {
  const state = mkdtempSync(join(tmpdir(), "arcrelay-ticket-test-"));
  t.after(() => rmSync(state, { recursive: true, force: true }));
  return state;
}

test("ticket service connection failures recover within a bounded retry budget", () => {
  const responses = [68, 68, 0];
  const delays = [];
  assert.equal(
    retry("xcrun", [], {
      execute: () => responses.shift(),
      wait: (delay) => delays.push(delay),
      report: () => {},
    }),
    0,
  );
  assert.deepEqual(delays, [15, 30]);
});

test("persistent rejection never becomes a successful verification", () => {
  let attempts = 0;
  assert.equal(
    retry("spctl", [], {
      execute: () => {
        attempts += 1;
        return 3;
      },
      wait: () => {},
      report: () => {},
    }),
    3,
  );
  assert.equal(attempts, 5);
});

test("a missing ticket after successful stapling is attached again and validated", (t) => {
  const state = stateFor(t);
  const calls = [];
  const responses = [0, 65, 0, 0];
  const args = ["stapler", "staple", "-v", "App With Spaces.app"];
  assert.equal(
    runXcrun(args, {
      env: { ARCRELAY_NOTARIZATION_STATE: state },
      wait: () => {},
      execute(command, commandArgs) {
        assert.equal(command, "/usr/bin/xcrun");
        calls.push(commandArgs);
        return responses.shift();
      },
    }),
    0,
  );
  assert.deepEqual(calls, [
    args,
    ["stapler", "validate", args.at(-1)],
    args,
    ["stapler", "validate", args.at(-1)],
  ]);
  assert.equal(existsSync(join(state, "failed")), false);
});

test("permanent staple failure leaves a marker for the caller that ignores exit codes", (t) => {
  const state = stateFor(t);
  assert.equal(
    runXcrun(["stapler", "staple", "-v", "ArcRelay.app"], {
      env: { ARCRELAY_NOTARIZATION_STATE: state },
      wait: () => {},
      execute: () => 68,
    }),
    68,
  );
  assert.match(readFileSync(join(state, "failed"), "utf8"), /exit 68/);
});

test("failure to start the Apple tool also leaves a failure marker", (t) => {
  const state = stateFor(t);
  assert.throws(
    () =>
      runXcrun(["stapler", "staple", "ArcRelay.app"], {
        env: { ARCRELAY_NOTARIZATION_STATE: state },
        execute: () => {
          throw new Error("spawn failed");
        },
      }),
    /spawn failed/,
  );
  assert.match(readFileSync(join(state, "failed"), "utf8"), /spawn failed/);
});

test("notarytool and SDK lookups are forwarded without retries or argument changes", () => {
  for (const args of [
    ["--find", "clang"],
    ["notarytool", "submit", "app.zip", "--wait"],
  ]) {
    let calls = 0;
    assert.equal(
      runXcrun(args, {
        execute(command, actualArgs) {
          calls += 1;
          assert.equal(command, "/usr/bin/xcrun");
          assert.deepEqual(actualArgs, args);
          return 42;
        },
      }),
      42,
    );
    assert.equal(calls, 1);
  }
});

test("a swallowed stapler failure blocks Tauri success and cleans temporary state", () => {
  let state;
  assert.equal(
    runTauri(["build", "--target", "aarch64-apple-darwin"], {
      execute(command, args, { env }) {
        assert.equal(command, "npm");
        assert.deepEqual(args, ["run", "tauri", "--", "build", "--target", "aarch64-apple-darwin"]);
        state = env.ARCRELAY_NOTARIZATION_STATE;
        assert.equal(env.PATH.split(":")[0], state);
        assert.equal(existsSync(join(state, "xcrun")), true);
        writeFileSync(join(state, "failed"), "Simulated swallowed failure");
        return 0;
      },
    }),
    1,
  );
  assert.equal(existsSync(state), false);
});

test("Tauri build errors are preserved and never retried", () => {
  let calls = 0;
  assert.equal(
    runTauri(["build"], {
      execute: () => {
        calls += 1;
        return 42;
      },
    }),
    42,
  );
  assert.equal(calls, 1);
});
