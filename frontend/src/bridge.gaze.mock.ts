import type { GazeCameraView, GazeStatusView } from "./ipc/generated";

const camera: GazeCameraView = {
  id: "preview-camera",
  name: "Windows Hello Camera",
  description: "camera-rs preview device",
};
let status: GazeStatusView = {
  revision: 0,
  state: "idle",
  cameraId: null,
  cameraName: null,
  calibrated: false,
  calibrationSamples: 0,
  capturedFrames: 0,
  inferredFrames: 0,
  droppedFrames: 0,
  inferenceMs: null,
  faceConfidence: null,
  target: null,
  error: null,
};
const clone = () => structuredClone({ ...status, revision: ++status.revision });

export const gazeMockBridge = {
  async listGazeCameras() { return [camera]; },
  async getGazeStatus() { return clone(); },
  async startGazeTracking(cameraId: string) {
    status = { ...status, state: "uncalibrated", cameraId, cameraName: camera.name, capturedFrames: 42, inferredFrames: 18, inferenceMs: 21.4, faceConfidence: 0.96 };
    return clone();
  },
  async stopGazeTracking() { status = { ...status, state: "stopped" }; return clone(); },
  async beginGazeCalibration(_cameraId: string) { status = { ...status, calibrationSamples: 0 }; return clone(); },
  async captureGazeCalibrationSample(_x: number, _y: number) { status = { ...status, calibrationSamples: status.calibrationSamples + 1 }; return status.calibrationSamples; },
  async finishGazeCalibration() { status = { ...status, calibrated: true, state: "tracking" }; return clone(); },
  async clearGazeCalibration() { status = { ...status, calibrated: false, calibrationSamples: 0, state: "uncalibrated" }; return clone(); },
  async openCameraPermissionSettings() {},
  async onGazeState(_listener: (value: GazeStatusView) => void) { return () => {}; },
};
