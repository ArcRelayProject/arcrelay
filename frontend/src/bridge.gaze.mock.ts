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
  observation: null,
  target: null,
  error: null,
};
const clone = () => structuredClone({ ...status, revision: ++status.revision });

export const gazeMockBridge = {
  async listGazeCameras() { return [camera]; },
  async getGazeStatus() { return clone(); },
  async startGazeTracking(cameraId: string) {
    status = { ...status, state: "uncalibrated", cameraId, cameraName: camera.name, capturedFrames: 42, inferredFrames: 18, inferenceMs: 21.4, faceConfidence: 0.96, observation: { leftEyeOpen: true, rightEyeOpen: true, headYaw: 1.4, headPitch: -0.8, headRoll: 0.2, gazeX: 0.02, gazeY: -0.06, gazeZ: 0.99 } };
    return clone();
  },
  async stopGazeTracking() { status = { ...status, state: "stopped" }; return clone(); },
  async beginGazeCalibration(_cameraId: string) { status = { ...status, calibrationSamples: 0 }; return clone(); },
  async captureGazeCalibrationSample(_x: number, _y: number) { status = { ...status, calibrationSamples: status.calibrationSamples + 1 }; return status.calibrationSamples; },
  async finishGazeCalibration() { status = { ...status, calibrated: true, state: "tracking" }; return clone(); },
  async cancelGazeCalibration() { status = { ...status, calibrationSamples: 0 }; return clone(); },
  async clearGazeCalibration() { status = { ...status, calibrated: false, calibrationSamples: 0, state: "uncalibrated" }; return clone(); },
  async openGazeCalibrationWindows() { return [{ index: 0, name: "内建视网膜显示器", x: 0, y: 0, width: 1728, height: 1117, scaleFactor: 2 }]; },
  async focusGazeCalibrationScreen(_index: number) {},
  async closeGazeCalibrationWindows() {},
  async sendGazeCalibrationOverlay(_event: import("./ipc/generated").GazeCalibrationOverlayEvent) {},
  async openCameraPermissionSettings() {},
  async onGazeState(listener: (value: GazeStatusView) => void) {
    const timer = window.setInterval(() => {
      if (["uncalibrated", "tracking"].includes(status.state)) {
        const frame = status.inferredFrames + 1;
        const wobble = Math.sin(frame / 4) * 0.002;
        status = {
          ...status,
          capturedFrames: status.capturedFrames + 3,
          inferredFrames: frame,
          faceConfidence: 0.96,
          observation: {
            leftEyeOpen: true,
            rightEyeOpen: true,
            headYaw: 1.4 + wobble,
            headPitch: -0.8 + wobble,
            headRoll: 0.2,
            gazeX: 0.02 + wobble,
            gazeY: -0.06 + wobble,
            gazeZ: 0.99,
          },
        };
        listener(clone());
      }
    }, 120);
    return () => window.clearInterval(timer);
  },
};
