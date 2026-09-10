import type { GazeObservationView } from "../../ipc/generated";

export type CalibrationStage = "idle" | "calibrating" | "paused" | "transition" | "indicator";

export type CalibrationFlowPayload = {
  sessionId: string;
  sourceDeviceId: string;
  targetDeviceId: string;
  stage: CalibrationStage;
  screenIndex: number;
  nextScreenIndex: number | null;
  screenName: string;
  nextScreenName: string | null;
  u: number;
  v: number;
  dwellProgress: number;
  current: number;
  total: number;
};

export type StabilitySample = {
  headYaw: number;
  headPitch: number;
  headRoll: number;
};

export function observationCanCalibrate(
  observation: GazeObservationView | null | undefined,
  faceConfidence: number | null | undefined,
): observation is GazeObservationView {
  return Boolean(
    observation
      && observation.leftEyeOpen
      && observation.rightEyeOpen
      && (faceConfidence ?? 0) >= 0.72
      && Math.abs(observation.headYaw) <= 30
      && Math.abs(observation.headPitch) <= 25
      && Number.isFinite(observation.gazeX)
      && Number.isFinite(observation.gazeY),
  );
}

export function observationCanHeadCalibrate(
  observation: GazeObservationView | null | undefined,
  faceConfidence: number | null | undefined,
): observation is GazeObservationView {
  return Boolean(
    observation
      && (faceConfidence ?? 0) >= 0.6
      && Number.isFinite(observation.headYaw)
      && Number.isFinite(observation.headPitch)
      && Number.isFinite(observation.headRoll)
      && Math.abs(observation.headYaw) <= 80
      && Math.abs(observation.headPitch) <= 60
      && Math.abs(observation.headRoll) <= 50,
  );
}

export function toStabilitySample(observation: GazeObservationView): StabilitySample {
  return {
    headYaw: observation.headYaw,
    headPitch: observation.headPitch,
    headRoll: observation.headRoll,
  };
}

function range(values: number[]): number {
  return Math.max(...values) - Math.min(...values);
}

export function samplesAreStable(samples: StabilitySample[]): boolean {
  if (samples.length < 4) return false;
  return range(samples.map((sample) => sample.headYaw)) <= 7
    && range(samples.map((sample) => sample.headPitch)) <= 7
    && range(samples.map((sample) => sample.headRoll)) <= 8;
}

export function clampProgress(value: number): number {
  return Math.max(0, Math.min(1, value));
}
