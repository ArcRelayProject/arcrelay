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
  calibratedDisplayIds: [],
  calibrationSamples: 0,
  capturedFrames: 0,
  inferredFrames: 0,
  droppedFrames: 0,
  inferenceMs: null,
  faceConfidence: null,
  presenceState: "uncertain",
  presenceFaceCount: 0,
  presenceOwnerSimilarity: null,
  presenceStableForMs: 0,
  presenceProfileName: null,
  presenceProfileEnrolled: false,
  presenceEnrollmentActive: false,
  presenceEnrollmentSamples: 0,
  presenceEnrollmentRequiredSamples: 12,
  presenceEnrollmentRejectedFrames: 0,
  observation: null,
  target: null,
  error: null,
};
const clone = () => structuredClone({ ...status, revision: ++status.revision });
let refiningDisplayId: string | null = null;

export const gazeMockBridge = {
  async listGazeCameras() { return [camera]; },
  async getGazeStatus() { return clone(); },
  async startGazeTracking(cameraId: string) {
    status = { ...status, state: "uncalibrated", cameraId, cameraName: camera.name, capturedFrames: 42, inferredFrames: 18, inferenceMs: 21.4, faceConfidence: 0.96, observation: { leftEyeOpen: true, rightEyeOpen: true, headYaw: 1.4, headPitch: -0.8, headRoll: 0.2, gazeX: 0.02, gazeY: -0.06, gazeZ: 0.99 } };
    return clone();
  },
  async stopGazeTracking() { status = { ...status, state: "stopped" }; return clone(); },
  async beginGazeCalibration(_cameraId: string, displayId: string | null = null) { refiningDisplayId = displayId; status = { ...status, calibrationSamples: 0 }; return clone(); },
  async captureGazeCalibrationSample(_x: number, _y: number) { status = { ...status, calibrationSamples: status.calibrationSamples + 1 }; return status.calibrationSamples; },
  async finishGazeCalibration() {
    const allDisplays = ["browser-display", "browser-laptop", "remote-display"];
    const calibratedDisplayIds = refiningDisplayId
      ? [...new Set([...status.calibratedDisplayIds, refiningDisplayId])]
      : allDisplays;
    refiningDisplayId = null;
    status = { ...status, calibrated: true, calibratedDisplayIds, state: "tracking" };
    return clone();
  },
  async cancelGazeCalibration() { status = { ...status, calibrationSamples: 0 }; return clone(); },
  async clearGazeCalibration() { status = { ...status, calibrated: false, calibratedDisplayIds: [], calibrationSamples: 0, state: "uncalibrated" }; return clone(); },
  async beginPresenceEnrollment(displayName: string) {
    status = { ...status, presenceProfileName: displayName, presenceEnrollmentActive: true, presenceEnrollmentSamples: 0 };
    return clone();
  },
  async cancelPresenceEnrollment() { status = { ...status, presenceEnrollmentActive: false }; return clone(); },
  async clearPresenceProfile() {
    status = { ...status, presenceProfileName: null, presenceProfileEnrolled: false, presenceEnrollmentActive: false, presenceEnrollmentSamples: 0, presenceState: "uncertain", presenceOwnerSimilarity: null };
    return clone();
  },
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
        const displayIds = status.calibratedDisplayIds;
        const displayId = displayIds.length ? displayIds[Math.floor(frame / 24) % displayIds.length] : null;
        status = {
          ...status,
          capturedFrames: status.capturedFrames + 3,
          inferredFrames: frame,
          faceConfidence: 0.96,
          presenceState: status.presenceProfileEnrolled ? "ownerPresent" : "uncertain",
          presenceFaceCount: 1,
          presenceOwnerSimilarity: status.presenceProfileEnrolled ? 0.82 : null,
          presenceStableForMs: status.presenceStableForMs + 120,
          ...(status.presenceEnrollmentActive ? {
            presenceEnrollmentSamples: Math.min(status.presenceEnrollmentRequiredSamples, status.presenceEnrollmentSamples + 1),
            ...(status.presenceEnrollmentSamples + 1 >= status.presenceEnrollmentRequiredSamples ? {
              presenceEnrollmentActive: false,
              presenceProfileEnrolled: true,
              presenceState: "ownerPresent",
            } : {}),
          } : {}),
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
          target: displayId ? {
            deviceId: displayId === "remote-display" ? "remote-office-mac" : "browser-preview",
            displayId,
            logicalX: 640,
            logicalY: 360,
            confidence: 0.84,
            stableForMs: 680,
            source: "headFallback",
          } : null,
        };
        listener(clone());
      }
    }, 120);
    return () => window.clearInterval(timer);
  },
};
