import { mount } from "svelte";

import GazeCalibrationOverlay from "./GazeCalibrationOverlay.svelte";
import "./overlay.css";

mount(GazeCalibrationOverlay, {
  target: document.getElementById("gaze-calibration")!,
});
