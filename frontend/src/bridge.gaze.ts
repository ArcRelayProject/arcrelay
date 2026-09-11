import type { UnlistenFn } from "@tauri-apps/api/event";
import { invoke, listen } from "./ipc/client";
import type { GazeCalibrationOverlayEvent, GazeCalibrationScreenView, GazeCameraView, GazePreviewView, GazeStatusView } from "./ipc/generated";

export const gazeBridge = {
  listGazeCameras: (): Promise<GazeCameraView[]> => invoke("list_gaze_cameras"),
  getGazeStatus: (): Promise<GazeStatusView> => invoke("get_gaze_status"),
  setGazePreviewEnabled: (enabled: boolean): Promise<void> => invoke("set_gaze_preview_enabled", { enabled }),
  startGazeTracking: (cameraId: string): Promise<GazeStatusView> =>
    invoke("start_gaze_tracking", { cameraId }),
  stopGazeTracking: (): Promise<GazeStatusView> => invoke("stop_gaze_tracking"),
  beginGazeCalibration: (cameraId: string, displayId: string | null = null): Promise<GazeStatusView> =>
    invoke("begin_gaze_calibration", { cameraId, displayId }),
  captureGazeCalibrationSample: (deskXUm: number, deskYUm: number): Promise<number> =>
    invoke("capture_gaze_calibration_sample", { deskXUm, deskYUm }),
  finishGazeCalibration: (): Promise<GazeStatusView> => invoke("finish_gaze_calibration"),
  cancelGazeCalibration: (): Promise<GazeStatusView> => invoke("cancel_gaze_calibration"),
  clearGazeCalibration: (): Promise<GazeStatusView> => invoke("clear_gaze_calibration"),
  beginPresenceEnrollment: (displayName: string): Promise<GazeStatusView> =>
    invoke("begin_presence_enrollment", { displayName }),
  cancelPresenceEnrollment: (): Promise<GazeStatusView> => invoke("cancel_presence_enrollment"),
  clearPresenceProfile: (): Promise<GazeStatusView> => invoke("clear_presence_profile"),
  openGazeCalibrationWindows: (): Promise<GazeCalibrationScreenView[]> => invoke("open_gaze_calibration_windows"),
  focusGazeCalibrationScreen: (index: number): Promise<void> => invoke("focus_gaze_calibration_screen", { index }),
  closeGazeCalibrationWindows: (): Promise<void> => invoke("close_gaze_calibration_windows"),
  sendGazeCalibrationOverlay: (event: GazeCalibrationOverlayEvent): Promise<void> =>
    invoke("send_gaze_calibration_overlay", { event }),
  openCameraPermissionSettings: (): Promise<void> => invoke("open_camera_permission_settings"),
  onGazeState(listener: (status: GazeStatusView) => void): Promise<UnlistenFn> {
    return listen("gaze-state", (event) => listener(event.payload));
  },
  onGazePreview(listener: (preview: GazePreviewView) => void): Promise<UnlistenFn> {
    return listen("gaze-preview", (event) => listener(event.payload));
  },
};
