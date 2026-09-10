import assert from "node:assert/strict";
import test from "node:test";

import { clampProgress, observationCanCalibrate, observationCanHeadCalibrate, samplesAreStable } from "./gazeCalibration.ts";

test("rejects closed eyes and excessive head rotation", () => {
  const base = {
    leftEyeOpen: true,
    rightEyeOpen: true,
    headYaw: 2,
    headPitch: -1,
    headRoll: 0,
    gazeX: 0.1,
    gazeY: -0.2,
    gazeZ: 0.9,
  };
  assert.equal(observationCanCalibrate(base, 0.9), true);
  assert.equal(observationCanCalibrate({ ...base, leftEyeOpen: false }, 0.9), false);
  assert.equal(observationCanCalibrate({ ...base, headYaw: 31 }, 0.9), false);
  assert.equal(observationCanCalibrate(base, 0.6), false);
});

test("keeps head calibration available when the eyes are occluded", () => {
  const observation = {
    leftEyeOpen: false,
    rightEyeOpen: false,
    headYaw: 42,
    headPitch: 3,
    headRoll: 1,
    gazeX: 0,
    gazeY: 0,
    gazeZ: 0,
  };
  assert.equal(observationCanCalibrate(observation, 0.9), false);
  assert.equal(observationCanHeadCalibrate(observation, 0.9), true);
  assert.equal(observationCanHeadCalibrate({ ...observation, headYaw: 60 }, 0.9), false);
});

test("requires a compact four-frame gaze and pose window", () => {
  const stable = [0, 1, 2, 3].map((index) => ({
    gazeX: 0.1 + index * 0.004,
    gazeY: -0.2 + index * 0.003,
    headYaw: 1 + index * 0.3,
    headPitch: -2 + index * 0.2,
  }));
  assert.equal(samplesAreStable(stable.slice(0, 3)), false);
  assert.equal(samplesAreStable(stable), true);
  assert.equal(samplesAreStable([...stable.slice(0, 3), { ...stable[3], gazeX: 0.3 }]), false);
});

test("clamps dwell progress", () => {
  assert.equal(clampProgress(-1), 0);
  assert.equal(clampProgress(0.45), 0.45);
  assert.equal(clampProgress(2), 1);
});
