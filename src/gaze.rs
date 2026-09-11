use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arcrelay_gaze::{
    layout_signature, CalibrationProfile, Calibrator, GazeTracker, TargetingSource, TrackerConfig,
    TrackerSession, TrackerSnapshot, WorkspaceMapper,
};
use arcrelay_input::DeskPointUm;
use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Position, Size, State, WebviewUrl,
    WebviewWindowBuilder,
};
use tokio::sync::{watch, Mutex, OnceCell};

use crate::arc_input::runtime::GazeCalibrationOverlayEvent;
use crate::arc_input::ArcInputRuntime;

const GAZE_EVENT: &str = "gaze-state";
const GAZE_CALIBRATION_WINDOW_PREFIX: &str = "gaze-calibration-";
const GAZE_INDICATOR_WINDOW_PREFIX: &str = "gaze-indicator-";
static GAZE_INDICATOR_WINDOW_ID: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/src_gaze_ipc.rs"));

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GazeCameraView {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GazeTargetView {
    pub device_id: String,
    pub display_id: String,
    pub logical_x: f64,
    pub logical_y: f64,
    pub confidence: f32,
    pub stable_for_ms: u64,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GazeObservationView {
    pub left_eye_open: bool,
    pub right_eye_open: bool,
    pub head_yaw: f32,
    pub head_pitch: f32,
    pub head_roll: f32,
    pub gaze_x: f32,
    pub gaze_y: f32,
    pub gaze_z: f32,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GazeCalibrationScreenView {
    pub index: usize,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GazeCalibrationFlowPayload {
    session_id: String,
    source_device_id: String,
    target_device_id: String,
    stage: String,
    screen_index: u32,
    next_screen_index: Option<u32>,
    screen_name: String,
    next_screen_name: Option<String>,
    u: f64,
    v: f64,
    dwell_progress: f64,
    current: u32,
    total: u32,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GazeStatusView {
    pub revision: u64,
    pub state: String,
    pub camera_id: Option<String>,
    pub camera_name: Option<String>,
    pub calibrated: bool,
    pub calibrated_display_ids: Vec<String>,
    pub calibration_samples: usize,
    pub captured_frames: u64,
    pub inferred_frames: u64,
    pub dropped_frames: u64,
    pub inference_ms: Option<f32>,
    pub face_confidence: Option<f32>,
    pub observation: Option<GazeObservationView>,
    pub target: Option<GazeTargetView>,
    pub error: Option<String>,
}

#[derive(Clone)]
struct CalibrationRun {
    calibrator: Calibrator,
    refine_display_id: Option<String>,
    base_profile: Option<CalibrationProfile>,
}

pub struct GazeService {
    input: Arc<ArcInputRuntime>,
    profile_path: PathBuf,
    tracker: OnceCell<Arc<GazeTracker>>,
    session: Mutex<Option<TrackerSession>>,
    forwarding: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    calibration: Mutex<Option<CalibrationRun>>,
    profile: Mutex<Option<CalibrationProfile>>,
    indicator_display: Mutex<Option<String>>,
    snapshots: watch::Sender<TrackerSnapshot>,
    revision: AtomicU64,
    indicator_revision: AtomicU64,
}

impl GazeService {
    pub fn new(input: Arc<ArcInputRuntime>) -> Arc<Self> {
        let profile_path = input.paths().root.join("gaze-calibration.json");
        let profile = load_profile_from_path(&profile_path)
            .filter(|profile| profile.version >= 3 && !profile.head_regions.is_empty());
        Arc::new(Self {
            input,
            profile_path,
            tracker: OnceCell::new(),
            session: Mutex::new(None),
            forwarding: Mutex::new(None),
            calibration: Mutex::new(None),
            profile: Mutex::new(profile),
            indicator_display: Mutex::new(None),
            snapshots: watch::channel(TrackerSnapshot::default()).0,
            revision: AtomicU64::new(0),
            indicator_revision: AtomicU64::new(0),
        })
    }

    async fn tracker(&self) -> Result<Arc<GazeTracker>, String> {
        self.tracker
            .get_or_try_init(|| async {
                tokio::task::spawn_blocking(|| GazeTracker::with_bundled_models(4))
                    .await
                    .map_err(|error| format!("gaze model worker failed: {error}"))?
                    .map(Arc::new)
                    .map_err(|error| error.to_string())
            })
            .await
            .cloned()
    }

    pub async fn cameras(&self) -> Result<Vec<GazeCameraView>, String> {
        GazeTracker::available_cameras()
            .await
            .map_err(|error| error.to_string())
            .map(|cameras| {
                cameras
                    .into_iter()
                    .map(|camera| GazeCameraView {
                        id: camera.id,
                        name: camera.name,
                        description: camera.description,
                    })
                    .collect()
            })
    }

    pub async fn start(self: &Arc<Self>, app: AppHandle, camera_id: String) -> Result<(), String> {
        self.stop().await?;
        let tracker = self.tracker().await?;
        let layout = self
            .input
            .snapshot()
            .configuration
            .layout
            .ok_or_else(|| "请先完成跨屏输入的屏幕布局".to_string())?;
        let profile = self
            .load_profile()
            .and_then(|profile| compatible_profile(profile, &camera_id, &layout));
        if let Some(profile) = profile.as_ref() {
            self.save_profile(profile)?;
        }
        let mapper = profile
            .clone()
            .map(|profile| WorkspaceMapper::new(layout, profile))
            .transpose()
            .map_err(|error| error.to_string())?;
        tracker.set_workspace_mapper(mapper);
        *self.profile.lock().await = profile;
        let session = tracker
            .start(&camera_id, TrackerConfig::default())
            .await
            .map_err(|error| error.to_string())?;
        self.snapshots.send_replace(session.snapshot());
        let mut snapshots = session.subscribe_snapshots();
        *self.session.lock().await = Some(session);
        let service = self.clone();
        let forwarding = tauri::async_runtime::spawn(async move {
            loop {
                if snapshots.changed().await.is_err() {
                    break;
                }
                let snapshot = snapshots.borrow().clone();
                service.snapshots.send_replace(snapshot.clone());
                let calibrating = service.calibration.lock().await.is_some();
                let automatic_head_regions = service
                    .profile
                    .lock()
                    .await
                    .as_ref()
                    .is_some_and(|profile| !profile.head_regions.is_empty());
                if calibrating {
                    service.input.clear_gaze_candidate();
                } else if let Some(stable) = snapshot.target.as_ref() {
                    if automatic_head_regions {
                        if let Err(error) = service.input.activate_gaze_target(&stable.target).await
                        {
                            tracing::warn!(%error, "failed to activate head-selected display");
                        }
                        if let Err(error) = service.publish_target_indicator(&stable.target).await {
                            tracing::warn!(%error, "failed to show gaze target indicator");
                        }
                    } else {
                        service.input.preselect_gaze_target(&stable.target);
                    }
                } else if automatic_head_regions {
                    service.input.clear_gaze_candidate();
                } else {
                    service.input.clear_gaze_preselection();
                }
                let _ = app.emit(GAZE_EVENT, service.status().await);
            }
        });
        *self.forwarding.lock().await = Some(forwarding);
        Ok(())
    }

    pub async fn stop(&self) -> Result<(), String> {
        self.input.clear_active_gaze_target().await;
        self.indicator_display.lock().await.take();
        if let Some(mut session) = self.session.lock().await.take() {
            session.stop().await.map_err(|error| error.to_string())?;
            self.snapshots.send_replace(session.snapshot());
        }
        if let Some(forwarding) = self.forwarding.lock().await.take() {
            forwarding.abort();
        }
        Ok(())
    }

    pub async fn status(&self) -> GazeStatusView {
        let snapshot = self.snapshots.borrow().clone();
        let profile = self.profile.lock().await.clone();
        let observation = snapshot
            .observation
            .as_ref()
            .map(|observation| GazeObservationView {
                left_eye_open: observation.left_eye_open,
                right_eye_open: observation.right_eye_open,
                head_yaw: observation.head_pose.yaw,
                head_pitch: observation.head_pose.pitch,
                head_roll: observation.head_pose.roll,
                gaze_x: observation.gaze.x,
                gaze_y: observation.gaze.y,
                gaze_z: observation.gaze.z,
            });
        let target = snapshot.target.map(|stable| GazeTargetView {
            device_id: stable.target.device_id,
            display_id: stable.target.display_id,
            logical_x: stable.target.logical_x,
            logical_y: stable.target.logical_y,
            confidence: stable.target.confidence,
            stable_for_ms: stable.stable_for_ms,
            source: match stable.target.source {
                TargetingSource::Eye => "eye".into(),
                TargetingSource::HeadFallback => "headFallback".into(),
            },
        });
        GazeStatusView {
            revision: self.revision.fetch_add(1, Ordering::AcqRel) + 1,
            state: format!("{:?}", snapshot.state).to_lowercase(),
            camera_id: snapshot
                .camera_id
                .or_else(|| profile.as_ref().map(|profile| profile.camera_id.clone())),
            camera_name: snapshot.camera_name,
            calibrated: profile.is_some(),
            calibrated_display_ids: profile
                .as_ref()
                .map(|profile| {
                    profile
                        .head_regions
                        .iter()
                        .map(|region| region.display_id.clone())
                        .collect()
                })
                .unwrap_or_default(),
            calibration_samples: self
                .calibration
                .lock()
                .await
                .as_ref()
                .map_or(0, |run| run.calibrator.sample_count()),
            captured_frames: snapshot.captured_frames,
            inferred_frames: snapshot.inferred_frames,
            dropped_frames: snapshot.dropped_frames,
            inference_ms: snapshot
                .observation
                .as_ref()
                .map(|observation| observation.inference_ms),
            face_confidence: snapshot
                .observation
                .as_ref()
                .map(|observation| observation.face_confidence),
            observation,
            target,
            error: snapshot.error,
        }
    }

    pub async fn begin_calibration(
        &self,
        camera_id: String,
        refine_display_id: Option<String>,
    ) -> Result<(), String> {
        self.indicator_display.lock().await.take();
        let layout = self
            .input
            .snapshot()
            .configuration
            .layout
            .ok_or_else(|| "请先完成跨屏输入的屏幕布局".to_string())?;
        if let Some(display_id) = refine_display_id.as_ref() {
            if !layout
                .displays
                .values()
                .any(|display| display.display_id.as_str() == display_id)
            {
                return Err("要优化的屏幕不在当前布局中".into());
            }
        }
        let base_profile = if refine_display_id.is_some() {
            let profile = self
                .profile
                .lock()
                .await
                .clone()
                .and_then(|profile| compatible_profile(profile, &camera_id, &layout))
                .ok_or_else(|| "请先完成一次全屏标定，再单独优化屏幕".to_string())?;
            Some(profile)
        } else {
            None
        };
        *self.calibration.lock().await = Some(CalibrationRun {
            calibrator: Calibrator::new(camera_id, layout_signature(&layout)),
            refine_display_id,
            base_profile,
        });
        Ok(())
    }

    pub async fn capture_calibration_sample(
        &self,
        desk_x_um: i64,
        desk_y_um: i64,
    ) -> Result<usize, String> {
        let observation = self
            .snapshots
            .borrow()
            .observation
            .clone()
            .ok_or_else(|| "当前没有可用的头部姿态观测".to_string())?;
        let point = DeskPointUm {
            x: desk_x_um,
            y: desk_y_um,
        };
        let layout = self
            .input
            .snapshot()
            .configuration
            .layout
            .ok_or_else(|| "屏幕布局在标定期间被移除".to_string())?;
        let display_id = layout
            .displays
            .values()
            .find(|display| display.desk_rect_um.contains(point))
            .map(|display| display.display_id.to_string())
            .ok_or_else(|| "标定点不属于当前屏幕布局".to_string())?;
        let mut calibration = self.calibration.lock().await;
        let calibration = calibration
            .as_mut()
            .ok_or_else(|| "尚未开始标定".to_string())?;
        if calibration
            .refine_display_id
            .as_ref()
            .is_some_and(|expected| expected != &display_id)
        {
            return Err("当前优化样本不属于所选屏幕".into());
        }
        calibration
            .calibrator
            .push_for_display(&observation, point, display_id)
            .map_err(|error| error.to_string())?;
        Ok(calibration.calibrator.sample_count())
    }

    pub async fn finish_calibration(&self) -> Result<GazeStatusView, String> {
        let calibration = self
            .calibration
            .lock()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| "尚未开始标定".to_string())?;
        let refined_profile = calibration
            .calibrator
            .finish()
            .map_err(|error| error.to_string())?;
        let layout = self
            .input
            .snapshot()
            .configuration
            .layout
            .ok_or_else(|| "屏幕布局在标定期间被移除".to_string())?;
        let profile = if let (Some(display_id), Some(mut base)) =
            (calibration.refine_display_id, calibration.base_profile)
        {
            merge_refined_profile(
                &mut base,
                refined_profile,
                &display_id,
                layout_signature(&layout),
            )?;
            base
        } else {
            refined_profile
        };
        let mapper =
            WorkspaceMapper::new(layout, profile.clone()).map_err(|error| error.to_string())?;
        self.save_profile(&profile)?;
        self.tracker().await?.set_workspace_mapper(Some(mapper));
        self.calibration.lock().await.take();
        *self.profile.lock().await = Some(profile);
        Ok(self.status().await)
    }

    pub async fn clear_calibration(&self) -> Result<GazeStatusView, String> {
        self.calibration.lock().await.take();
        self.profile.lock().await.take();
        if let Some(tracker) = self.tracker.get() {
            tracker.set_workspace_mapper(None);
        }
        match std::fs::remove_file(&self.profile_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("删除眼动标定失败: {error}")),
        }
        Ok(self.status().await)
    }

    pub async fn cancel_calibration(&self) -> GazeStatusView {
        self.calibration.lock().await.take();
        self.status().await
    }

    fn load_profile(&self) -> Option<CalibrationProfile> {
        load_profile_from_path(&self.profile_path)
    }

    fn save_profile(&self, profile: &CalibrationProfile) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(profile).map_err(|error| error.to_string())?;
        crate::infrastructure::durable_file::replace_private(&self.profile_path, &bytes)
            .map_err(|error| format!("保存眼动标定失败: {error}"))
    }

    async fn publish_target_indicator(
        &self,
        target: &arcrelay_gaze::GazeTarget,
    ) -> Result<(), String> {
        let mut shown_display = self.indicator_display.lock().await;
        if shown_display.as_deref() == Some(target.display_id.as_str()) {
            return Ok(());
        }
        *shown_display = Some(target.display_id.clone());
        drop(shown_display);

        let layout = self
            .input
            .snapshot()
            .configuration
            .layout
            .ok_or_else(|| "屏幕布局不可用".to_string())?;
        let display = layout
            .displays
            .values()
            .find(|display| display.display_id.as_str() == target.display_id)
            .ok_or_else(|| "识别到的屏幕不在当前布局中".to_string())?;
        let mut device_displays = layout
            .displays
            .values()
            .filter(|candidate| candidate.device_id == display.device_id)
            .collect::<Vec<_>>();
        device_displays.sort_by(|left, right| {
            left.logical_bounds
                .y
                .total_cmp(&right.logical_bounds.y)
                .then_with(|| left.logical_bounds.x.total_cmp(&right.logical_bounds.x))
        });
        let screen_index = device_displays
            .iter()
            .position(|candidate| candidate.display_id == display.display_id)
            .ok_or_else(|| "无法定位识别到的屏幕".to_string())? as u32;
        let sequence = self.indicator_revision.fetch_add(1, Ordering::AcqRel) + 1;

        let result = self
            .input
            .send_gaze_calibration_overlay(GazeCalibrationOverlayEvent {
                session_id: format!("indicator:{sequence}"),
                stage: "indicator".into(),
                source_device_id: self.input.identity().service_instance_id.to_string(),
                target_device_id: display.device_id.to_string(),
                display_id: display.display_id.to_string(),
                screen_index,
                next_screen_index: None,
                screen_name: display.name.clone(),
                next_screen_name: None,
                target_u: 0.5,
                target_v: 0.5,
                dwell_progress: 0.0,
                current: 0,
                total: 0,
            })
            .await
            .map_err(|error| error.to_string());
        if result.is_err() {
            let mut shown_display = self.indicator_display.lock().await;
            if shown_display.as_deref() == Some(target.display_id.as_str()) {
                shown_display.take();
            }
        }
        result
    }
}

fn load_profile_from_path(path: &std::path::Path) -> Option<CalibrationProfile> {
    let bytes = std::fs::read(path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(profile) => Some(profile),
        Err(error) => {
            tracing::warn!(%error, "ignored invalid gaze calibration profile");
            None
        }
    }
}

fn compatible_profile(
    mut profile: CalibrationProfile,
    camera_id: &str,
    layout: &arcrelay_input::WorkspaceLayout,
) -> Option<CalibrationProfile> {
    if profile.version < 3
        || profile.head_regions.is_empty()
        || profile.camera_id != camera_id
        || profile.head_regions.iter().any(|region| {
            !layout
                .displays
                .values()
                .any(|display| display.display_id.as_str() == region.display_id)
        })
    {
        return None;
    }
    profile.layout_signature = layout_signature(layout);
    Some(profile)
}

fn merge_refined_profile(
    base: &mut CalibrationProfile,
    refined: CalibrationProfile,
    display_id: &str,
    layout_signature: String,
) -> Result<(), String> {
    let replacement = refined
        .head_regions
        .into_iter()
        .find(|region| region.display_id == display_id)
        .ok_or_else(|| "没有生成所选屏幕的头部方向样本".to_string())?;
    base.head_regions
        .retain(|region| region.display_id != display_id);
    base.head_regions.push(replacement);
    base.head_regions
        .sort_by(|left, right| left.display_id.cmp(&right.display_id));
    base.layout_signature = layout_signature;
    base.sample_count = base
        .head_regions
        .iter()
        .map(|region| region.sample_count)
        .sum();
    base.eye_sample_count = Some(0);
    Ok(())
}

#[cfg(test)]
mod profile_tests {
    use std::collections::BTreeMap;

    use arcrelay_gaze::HeadRegionProfile;
    use arcrelay_input::{
        DeskRectUm, DisplayFingerprint, DisplayId, DisplayRotation, DisplaySurface,
        GeometryConfidence, InventoryRevision, LogicalRect, ScaleFactor, ServiceInstanceId,
        SizeI64, SizeU32, TopologyRevision, WorkspaceId, WorkspaceLayout,
    };

    use super::*;

    fn region(display_id: &str, yaw: f64, sample_count: usize) -> HeadRegionProfile {
        HeadRegionProfile {
            display_id: display_id.into(),
            centroid: [yaw, 0.0, 0.5, 0.5, 0.1],
            scale: [0.05; 5],
            sample_count,
        }
    }

    fn profile(regions: Vec<HeadRegionProfile>, signature: &str) -> CalibrationProfile {
        CalibrationProfile {
            version: 3,
            camera_id: "camera".into(),
            layout_signature: signature.into(),
            coefficients_x: [0.0; 8],
            coefficients_y: [0.0; 8],
            rms_error_um: 0.0,
            sample_count: regions.iter().map(|region| region.sample_count).sum(),
            eye_sample_count: Some(0),
            head_coefficients_x: None,
            head_coefficients_y: None,
            head_rms_error_um: None,
            head_regions: regions,
        }
    }

    fn layout() -> WorkspaceLayout {
        let display_id = DisplayId::parse("display-a").unwrap();
        let display = DisplaySurface {
            display_id: display_id.clone(),
            device_id: ServiceInstanceId::parse("device-a").unwrap(),
            fingerprint: DisplayFingerprint::parse("panel-a").unwrap(),
            name: "Main".into(),
            pixel_size: SizeU32 {
                width: 1920,
                height: 1080,
            },
            logical_bounds: LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            scale_factor: ScaleFactor(1.0),
            physical_size_um: SizeI64 {
                width: 600_000,
                height: 340_000,
            },
            rotation: DisplayRotation::Degrees0,
            desk_rect_um: DeskRectUm {
                x: 0,
                y: 0,
                width: 600_000,
                height: 340_000,
            },
            geometry_confidence: GeometryConfidence::HardwareReported,
            inventory_revision: InventoryRevision(1),
        };
        WorkspaceLayout {
            workspace_id: WorkspaceId::parse("desk-after-restart").unwrap(),
            revision: TopologyRevision(9),
            displays: BTreeMap::from([(display_id, display)]),
            portals: Vec::new(),
        }
    }

    #[test]
    fn compatible_saved_profile_survives_a_changed_layout_signature() {
        let layout = layout();
        let restored = compatible_profile(
            profile(
                vec![region("display-a", 0.0, 9)],
                "signature-before-restart",
            ),
            "camera",
            &layout,
        )
        .expect("restore compatible profile");
        assert_eq!(restored.layout_signature, layout_signature(&layout));
        assert!(compatible_profile(restored.clone(), "different-camera", &layout).is_none());
        let mut missing = restored;
        missing.head_regions[0].display_id = "missing-display".into();
        assert!(compatible_profile(missing, "camera", &layout).is_none());
    }

    #[test]
    fn refining_one_display_preserves_every_other_region() {
        let mut base = profile(
            vec![region("display-a", -0.4, 9), region("display-b", 0.4, 9)],
            "old",
        );
        let refined = profile(vec![region("display-b", 0.65, 12)], "temporary");
        merge_refined_profile(&mut base, refined, "display-b", "new".into()).unwrap();
        assert_eq!(base.layout_signature, "new");
        assert_eq!(base.sample_count, 21);
        assert_eq!(base.head_regions.len(), 2);
        assert_eq!(base.head_regions[0].display_id, "display-a");
        assert_eq!(base.head_regions[0].centroid[0], -0.4);
        assert_eq!(base.head_regions[1].display_id, "display-b");
        assert_eq!(base.head_regions[1].centroid[0], 0.65);
    }
}

fn close_gaze_calibration_windows_inner(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(GAZE_CALIBRATION_WINDOW_PREFIX) {
            let _ = window.close();
        }
    }
}

fn close_gaze_indicator_windows_inner(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(GAZE_INDICATOR_WINDOW_PREFIX) {
            let _ = window.close();
        }
    }
}

fn show_gaze_target_indicator(
    app: &AppHandle,
    event: &GazeCalibrationOverlayEvent,
) -> Result<(), String> {
    close_gaze_indicator_windows_inner(app);
    let mut monitors = app
        .available_monitors()
        .map_err(|error| error.to_string())?;
    monitors.sort_by_key(|monitor| (monitor.position().y, monitor.position().x));
    let monitor = monitors
        .get(event.screen_index as usize)
        .ok_or_else(|| format!("识别提示的屏幕 {} 不可用", event.screen_index + 1))?;
    let monitor_position = *monitor.position();
    let monitor_size = *monitor.size();
    let scale = monitor.scale_factor();
    let width = (360.0 * scale).round() as u32;
    let height = (82.0 * scale).round() as u32;
    let x = monitor_position.x + (monitor_size.width.saturating_sub(width) / 2) as i32;
    let y = monitor_position.y + (28.0 * scale).round() as i32;
    let id = GAZE_INDICATOR_WINDOW_ID.fetch_add(1, Ordering::AcqRel) + 1;
    let label = format!("{GAZE_INDICATOR_WINDOW_PREFIX}{id}");
    let url = WebviewUrl::App(
        format!(
            "gaze-calibration.html?mode=indicator&screen={}",
            event.screen_index
        )
        .into(),
    );
    let window = WebviewWindowBuilder::new(app, &label, url)
        .title("ArcRelay 屏幕识别")
        .inner_size(360.0, 82.0)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .transparent(true)
        .visible(false)
        .focused(false)
        .build()
        .map_err(|error| format!("创建屏幕识别提示失败: {error}"))?;
    window
        .set_position(Position::Physical(PhysicalPosition::new(x, y)))
        .map_err(|error| format!("定位屏幕识别提示失败: {error}"))?;
    window
        .set_size(Size::Physical(PhysicalSize::new(width, height)))
        .map_err(|error| format!("调整屏幕识别提示失败: {error}"))?;
    let _ = window.set_ignore_cursor_events(true);
    window
        .show()
        .map_err(|error| format!("显示屏幕识别提示失败: {error}"))?;

    let payload = GazeCalibrationFlowPayload {
        session_id: event.session_id.clone(),
        source_device_id: event.source_device_id.clone(),
        target_device_id: event.target_device_id.clone(),
        stage: event.stage.clone(),
        screen_index: event.screen_index,
        next_screen_index: None,
        screen_name: event.screen_name.clone(),
        next_screen_name: None,
        u: 0.5,
        v: 0.5,
        dwell_progress: 0.0,
        current: 0,
        total: 0,
    };
    window
        .emit("gaze-calibration-flow", payload.clone())
        .map_err(|error| error.to_string())?;
    let app_for_replay = app.clone();
    let replay_payload = payload.clone();
    let close_label = label.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(140)).await;
        if let Some(window) = app_for_replay.get_webview_window(&close_label) {
            let _ = window.emit("gaze-calibration-flow", replay_payload);
        }
        tokio::time::sleep(std::time::Duration::from_millis(1_660)).await;
        if let Some(window) = app_for_replay.get_webview_window(&close_label) {
            let _ = window.close();
        }
    });
    Ok(())
}

fn gaze_calibration_windows_exist(app: &AppHandle) -> bool {
    app.webview_windows()
        .keys()
        .any(|label| label.starts_with(GAZE_CALIBRATION_WINDOW_PREFIX))
}

#[tauri::command]
pub fn open_gaze_calibration_windows(
    app: AppHandle,
) -> Result<Vec<GazeCalibrationScreenView>, String> {
    close_gaze_calibration_windows_inner(&app);
    let mut monitors = app
        .available_monitors()
        .map_err(|error| error.to_string())?;
    monitors.sort_by_key(|monitor| (monitor.position().y, monitor.position().x));

    let mut screens = Vec::with_capacity(monitors.len());
    for (index, monitor) in monitors.iter().enumerate() {
        let position = *monitor.position();
        let size = *monitor.size();
        let name = monitor
            .name()
            .cloned()
            .unwrap_or_else(|| format!("屏幕 {}", index + 1));
        let label = format!("{GAZE_CALIBRATION_WINDOW_PREFIX}{index}");
        let url = WebviewUrl::App(format!("gaze-calibration.html?screen={index}").into());
        let window = WebviewWindowBuilder::new(&app, &label, url)
            .title("ArcRelay 眼动标定")
            .inner_size(800.0, 600.0)
            .decorations(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .build()
            .map_err(|error| format!("创建{name}标定覆盖层失败: {error}"))?;
        window
            .set_position(Position::Physical(PhysicalPosition::new(
                position.x, position.y,
            )))
            .map_err(|error| format!("定位{name}标定覆盖层失败: {error}"))?;
        window
            .set_size(Size::Physical(PhysicalSize::new(size.width, size.height)))
            .map_err(|error| format!("调整{name}标定覆盖层失败: {error}"))?;
        window
            .show()
            .map_err(|error| format!("显示{name}标定覆盖层失败: {error}"))?;
        screens.push(GazeCalibrationScreenView {
            index,
            name,
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
            scale_factor: monitor.scale_factor(),
        });
    }

    if let Some(window) = app.get_webview_window(&format!("{GAZE_CALIBRATION_WINDOW_PREFIX}0")) {
        let _ = window.set_focus();
    }
    Ok(screens)
}

#[tauri::command]
pub fn focus_gaze_calibration_screen(app: AppHandle, index: usize) -> Result<(), String> {
    app.get_webview_window(&format!("{GAZE_CALIBRATION_WINDOW_PREFIX}{index}"))
        .ok_or_else(|| format!("标定屏幕 {} 不可用", index + 1))?
        .set_focus()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn close_gaze_calibration_windows(app: AppHandle) -> Result<(), String> {
    close_gaze_calibration_windows_inner(&app);
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
    Ok(())
}

pub fn apply_gaze_calibration_overlay_event(
    app: &AppHandle,
    event: &GazeCalibrationOverlayEvent,
) -> Result<(), String> {
    if event.stage == "indicator" {
        return show_gaze_target_indicator(app, event);
    }
    if event.stage == "close" {
        close_gaze_calibration_windows_inner(app);
        return Ok(());
    }
    if event.stage == "cancel" {
        close_gaze_calibration_windows_inner(app);
        app.emit("gaze-calibration-cancel", &event.session_id)
            .map_err(|error| error.to_string())?;
        return Ok(());
    }
    let opened_windows = !gaze_calibration_windows_exist(app);
    if opened_windows {
        open_gaze_calibration_windows(app.clone())?;
    }
    let focus_index = if event.stage == "transition" {
        event.next_screen_index.unwrap_or(event.screen_index)
    } else {
        event.screen_index
    };
    let _ = focus_gaze_calibration_screen(app.clone(), focus_index as usize);
    let payload = GazeCalibrationFlowPayload {
        session_id: event.session_id.clone(),
        source_device_id: event.source_device_id.clone(),
        target_device_id: event.target_device_id.clone(),
        stage: event.stage.clone(),
        screen_index: event.screen_index,
        next_screen_index: event.next_screen_index,
        screen_name: event.screen_name.clone(),
        next_screen_name: event.next_screen_name.clone(),
        u: event.target_u,
        v: event.target_v,
        dwell_progress: event.dwell_progress,
        current: event.current,
        total: event.total,
    };
    app.emit("gaze-calibration-flow", &payload)
        .map_err(|error| error.to_string())?;

    // A freshly-created webview can miss the event emitted in the same run-loop
    // turn. Replay the first frame once after its frontend listener is mounted.
    if opened_windows {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(180)).await;
            let _ = app.emit("gaze-calibration-flow", payload);
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn send_gaze_calibration_overlay(
    service: State<'_, Arc<GazeService>>,
    event: GazeCalibrationOverlayEvent,
) -> Result<(), String> {
    service
        .input
        .send_gaze_calibration_overlay(event)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn request_gaze_calibration_cancel(
    app: AppHandle,
    service: State<'_, Arc<GazeService>>,
    session_id: String,
    source_device_id: String,
) -> Result<(), String> {
    close_gaze_calibration_windows_inner(&app);
    service
        .input
        .send_gaze_calibration_overlay(GazeCalibrationOverlayEvent {
            session_id,
            stage: "cancel".into(),
            source_device_id: service.input.identity().service_instance_id.to_string(),
            target_device_id: source_device_id,
            display_id: String::new(),
            screen_index: 0,
            next_screen_index: None,
            screen_name: String::new(),
            next_screen_name: None,
            target_u: 0.5,
            target_v: 0.5,
            dwell_progress: 0.0,
            current: 0,
            total: 0,
        })
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_gaze_cameras(
    service: State<'_, Arc<GazeService>>,
) -> Result<Vec<GazeCameraView>, String> {
    service.cameras().await
}

#[tauri::command]
pub async fn get_gaze_status(
    service: State<'_, Arc<GazeService>>,
) -> Result<GazeStatusView, String> {
    Ok(service.status().await)
}

#[tauri::command]
pub async fn start_gaze_tracking(
    app: AppHandle,
    service: State<'_, Arc<GazeService>>,
    camera_id: String,
) -> Result<GazeStatusView, String> {
    service.inner().start(app, camera_id).await?;
    Ok(service.status().await)
}

#[tauri::command]
pub async fn stop_gaze_tracking(
    service: State<'_, Arc<GazeService>>,
) -> Result<GazeStatusView, String> {
    service.stop().await?;
    Ok(service.status().await)
}

#[tauri::command]
pub async fn begin_gaze_calibration(
    service: State<'_, Arc<GazeService>>,
    camera_id: String,
    display_id: Option<String>,
) -> Result<GazeStatusView, String> {
    service.begin_calibration(camera_id, display_id).await?;
    Ok(service.status().await)
}

#[tauri::command]
pub async fn capture_gaze_calibration_sample(
    service: State<'_, Arc<GazeService>>,
    desk_x_um: i64,
    desk_y_um: i64,
) -> Result<usize, String> {
    service
        .capture_calibration_sample(desk_x_um, desk_y_um)
        .await
}

#[tauri::command]
pub async fn finish_gaze_calibration(
    service: State<'_, Arc<GazeService>>,
) -> Result<GazeStatusView, String> {
    service.finish_calibration().await
}

#[tauri::command]
pub async fn clear_gaze_calibration(
    service: State<'_, Arc<GazeService>>,
) -> Result<GazeStatusView, String> {
    service.clear_calibration().await
}

#[tauri::command]
pub async fn cancel_gaze_calibration(
    service: State<'_, Arc<GazeService>>,
) -> Result<GazeStatusView, String> {
    Ok(service.cancel_calibration().await)
}

#[tauri::command]
pub fn open_camera_permission_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let (program, argument) = (
        "open",
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Camera",
    );
    #[cfg(target_os = "windows")]
    let (program, argument) = ("cmd", "/C start ms-settings:privacy-webcam");
    #[cfg(target_os = "linux")]
    return Err("请在系统设置中允许 ArcRelay 使用摄像头".into());

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    std::process::Command::new(program)
        .args(argument.split_whitespace())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("打开摄像头权限设置失败: {error}"))
}
