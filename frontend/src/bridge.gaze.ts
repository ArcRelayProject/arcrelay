import type { UnlistenFn } from "@tauri-apps/api/event";
import { invoke, listen } from "./ipc/client";
import type { GazeCameraView, GazeStatusView } from "./ipc/generated";

export const gazeBridge = {
  listGazeCameras: (): Promise<GazeCameraView[]> => invoke("list_gaze_cameras"),
  getGazeStatus: (): Promise<GazeStatusView> => invoke("get_gaze_status"),
  startGazeTracking: (cameraId: string): Promise<GazeStatusView> =>
    invoke("start_gaze_tracking", { cameraId }),
  stopGazeTracking: (): Promise<GazeStatusView> => invoke("stop_gaze_tracking"),
  beginGazeCalibration: (cameraId: string): Promise<GazeStatusView> =>
    invoke("begin_gaze_calibration", { cameraId }),
  captureGazeCalibrationSample: (deskXUm: number, deskYUm: number): Promise<number> =>
    invoke("capture_gaze_calibration_sample", { deskXUm, deskYUm }),
  finishGazeCalibration: (): Promise<GazeStatusView> => invoke("finish_gaze_calibration"),
  clearGazeCalibration: (): Promise<GazeStatusView> => invoke("clear_gaze_calibration"),
  openCameraPermissionSettings: (): Promise<void> => invoke("open_camera_permission_settings"),
  onGazeState(listener: (status: GazeStatusView) => void): Promise<UnlistenFn> {
    return listen("gaze-state", (event) => listener(event.payload));
  },
};
