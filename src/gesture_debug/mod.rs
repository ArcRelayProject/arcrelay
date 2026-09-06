//! Temporary, local-only gesture laboratory. No network or persisted settings.
//! Native APIs are available on macOS in every build profile and start on demand.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::State;

#[cfg(target_os = "macos")]
mod macos;

const MAX_BATCHES: usize = 32;
const MAX_SAMPLES_PER_BATCH: usize = 8_192;
const PREVIEW_SAMPLES: usize = 60;
const MAX_TRIGGERS_PER_BATCH: usize = 32;

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GestureDebugSample {
    pub id: u64,
    pub batch_id: u64,
    pub elapsed_ms: u64,
    /// "tap" may include synthetic events: source tags do not always survive posting.
    pub source: String,
    pub event_type: u32,
    pub subtype: i64,
    pub motion: i64,
    pub phase: i64,
    pub progress: f64,
    pub velocity_x: f64,
    pub velocity_y: f64,
    pub magnification: f64,
    pub rotation: f64,
    pub swipe_mask: i64,
    pub suppressed: bool,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GestureDebugTriggerRecord {
    pub elapsed_ms: u64,
    pub request: GestureDebugTrigger,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GestureDebugBatch {
    pub id: u64,
    /// User-supplied action label, never a detected finger count.
    pub label: String,
    pub mode: String,
    pub elapsed_ms: u64,
    pub finished: bool,
    pub stop_reason: String,
    pub total_samples: u64,
    pub marker_samples: u64,
    pub suppressed_markers: u64,
    pub dropped_samples: u64,
    pub capacity_dropped_samples: u64,
    pub retained_samples: usize,
    pub triggers: Vec<GestureDebugTriggerRecord>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GestureDebugBatchReport {
    pub batch: GestureDebugBatch,
    pub samples: Vec<GestureDebugSample>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GestureDebugReport {
    pub os_version: String,
    pub batches: Vec<GestureDebugBatchReport>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GestureDebugSnapshot {
    pub available: bool,
    pub os_version: String,
    pub accessibility_granted: bool,
    pub capturing: bool,
    pub suppressing: bool,
    pub injecting: bool,
    pub remaining_ms: u64,
    pub total_samples: u64,
    pub dropped_samples: u64,
    pub marker_samples: u64,
    pub capacity_dropped_samples: u64,
    pub sample_limit: usize,
    pub batch_limit: usize,
    pub batches: Vec<GestureDebugBatch>,
    /// Only the latest batch's tail for UI polling. Full export is a separate command.
    pub samples: Vec<GestureDebugSample>,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum GestureDebugAction {
    SwipeLeft,
    SwipeRight,
    SwipeUp,
    SwipeDown,
    PinchIn,
    PinchOut,
}

#[derive(Clone, Debug, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GestureDebugTrigger {
    pub action: GestureDebugAction,
    pub duration_ms: u32,
    pub delay_ms: u32,
    pub amount: f64,
    pub reverse: bool,
    pub cancel_at_end: bool,
}

impl GestureDebugTrigger {
    fn validate(&self) -> Result<(), String> {
        if !(150..=2_000).contains(&self.duration_ms)
            || self.delay_ms > 3_000
            || !self.amount.is_finite()
            || !(0.05..=1.0).contains(&self.amount)
        {
            return Err(
                "gesture parameters exceed experiment limits: duration 150–2000 ms, delay 0–3000 ms, magnitude 0.05–1.0".into(),
            );
        }
        Ok(())
    }

    fn motion_and_progress(&self) -> (i64, f64) {
        let (motion, sign) = match self.action {
            GestureDebugAction::SwipeLeft => (1, -1.0),
            GestureDebugAction::SwipeRight => (1, 1.0),
            GestureDebugAction::SwipeUp => (2, 1.0),
            GestureDebugAction::SwipeDown => (2, -1.0),
            GestureDebugAction::PinchIn => (3, -1.0),
            GestureDebugAction::PinchOut => (3, 1.0),
        };
        (
            motion,
            self.amount * sign * if self.reverse { -1.0 } else { 1.0 },
        )
    }
}

struct RecordedBatch {
    started: Instant,
    meta: GestureDebugBatch,
    samples: Vec<GestureDebugSample>,
}

struct DebugData {
    deadline: Option<Instant>,
    next_batch_id: u64,
    batches: Vec<RecordedBatch>,
    message: String,
}

impl Default for DebugData {
    fn default() -> Self {
        Self {
            deadline: None,
            next_batch_id: 1,
            batches: Vec::new(),
            message: "Not started. Only gesture metadata is captured; keyboard input, text, and raw touch points are never recorded.".into(),
        }
    }
}

#[derive(Default)]
struct Shared {
    data: Mutex<DebugData>,
    capturing: AtomicBool,
    suppressing: AtomicBool,
    injecting: AtomicBool,
    cancel_capture: AtomicBool,
    cancel_injection: AtomicBool,
    dropped: AtomicU64,
}

impl Shared {
    fn begin_batch(&self, label: &str, mode: &str) -> Result<(), String> {
        let label = label.trim();
        if label.is_empty() || label.chars().count() > 160 {
            return Err(
                "enter a 1–160 character action label that identifies finger count and direction"
                    .into(),
            );
        }
        let mut data = lock(&self.data);
        if data
            .batches
            .last()
            .is_some_and(|batch| !batch.meta.finished)
        {
            return Err("stop the current trial first".into());
        }
        if data.batches.len() >= MAX_BATCHES {
            return Err(
                "The 32-trial limit has been reached. Export the report, then clear the records. Existing trials will not be overwritten.".into(),
            );
        }
        let id = data.next_batch_id;
        data.next_batch_id += 1;
        self.dropped.store(0, Ordering::Relaxed);
        data.batches.push(RecordedBatch {
            started: Instant::now(),
            meta: GestureDebugBatch {
                id,
                label: label.into(),
                mode: mode.into(),
                elapsed_ms: 0,
                finished: false,
                stop_reason: String::new(),
                total_samples: 0,
                marker_samples: 0,
                suppressed_markers: 0,
                dropped_samples: 0,
                capacity_dropped_samples: 0,
                retained_samples: 0,
                triggers: Vec::new(),
            },
            // Allocate outside the native callback; never evict previous events.
            samples: Vec::with_capacity(MAX_SAMPLES_PER_BATCH),
        });
        Ok(())
    }

    fn batch_meta(&self, batch: &RecordedBatch) -> GestureDebugBatch {
        let mut meta = batch.meta.clone();
        if !meta.finished {
            meta.elapsed_ms = batch.started.elapsed().as_millis() as u64;
            meta.dropped_samples = self.dropped.load(Ordering::Relaxed);
        }
        meta.retained_samples = batch.samples.len();
        // Every attempted record is accounted for, including try_lock failures.
        meta.total_samples += meta.dropped_samples;
        meta
    }

    fn finish_if_idle(&self, reason: &str) {
        let mut data = lock(&self.data);
        self.finish_locked(&mut data, reason);
    }

    fn finish_locked(&self, data: &mut DebugData, reason: &str) {
        let Some(batch) = data.batches.last_mut().filter(|batch| !batch.meta.finished) else {
            return;
        };
        if batch.meta.stop_reason.is_empty() {
            batch.meta.stop_reason = reason.into();
        }
        if self.capturing.load(Ordering::Acquire) || self.injecting.load(Ordering::Acquire) {
            return;
        }
        batch.meta.finished = true;
        batch.meta.elapsed_ms = batch.started.elapsed().as_millis() as u64;
        batch.meta.dropped_samples = self.dropped.load(Ordering::Relaxed);
        data.deadline = None;
    }

    fn register_injection(&self, request: &GestureDebugTrigger) -> Result<bool, String> {
        let mut data = lock(&self.data);
        let Some(batch) = data.batches.last_mut().filter(|batch| !batch.meta.finished) else {
            return Ok(false);
        };
        if batch.meta.mode == "intercept" || batch.meta.capacity_dropped_samples > 0 {
            return Err("current trial is intercepting or stopped at capacity; stop it and create a new trial before synthesis".into());
        }
        if batch.meta.triggers.len() >= MAX_TRIGGERS_PER_BATCH {
            return Err(
                "a trial can record at most 32 syntheses; stop it and create a new trial".into(),
            );
        }
        // Claim while holding the same lock as finish_locked. A capture timeout
        // cannot close the batch between attaching this request and its events.
        self.cancel_injection.store(false, Ordering::Release);
        self.injecting.store(true, Ordering::Release);
        batch.meta.triggers.push(GestureDebugTriggerRecord {
            elapsed_ms: batch.started.elapsed().as_millis() as u64,
            request: request.clone(),
        });
        Ok(true)
    }

    fn finish_injection(&self) {
        let mut data = lock(&self.data);
        self.injecting.store(false, Ordering::Release);
        if !self.capturing.load(Ordering::Acquire) {
            self.finish_locked(
                &mut data,
                "synthesis worker ended; confirm the result manually",
            );
        }
    }

    fn finish_capture(&self, reason: &str) {
        let mut data = lock(&self.data);
        self.capturing.store(false, Ordering::Release);
        self.finish_locked(&mut data, reason);
        data.message = reason.into();
    }

    // Never block WindowServer's event callback on a UI snapshot or export.
    fn record(&self, mut sample: GestureDebugSample) {
        let Ok(mut data) = self.data.try_lock() else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let Some(batch) = data.batches.last_mut().filter(|batch| !batch.meta.finished) else {
            return;
        };
        batch.meta.total_samples += 1;
        if sample.event_type == 29 && sample.subtype == 0 {
            batch.meta.marker_samples += 1;
            batch.meta.suppressed_markers += u64::from(sample.suppressed);
            return;
        }
        if batch.samples.len() == MAX_SAMPLES_PER_BATCH {
            batch.meta.capacity_dropped_samples += 1;
            batch.meta.stop_reason = "recording reached capacity and stop was requested; the final sequence may be incomplete".into();
            data.message =
                "Recording is full. Stopping and releasing interception. Saved events will not be overwritten; export and then clear them.".into();
            self.suppressing.store(false, Ordering::Release);
            self.cancel_capture.store(true, Ordering::Release);
            self.cancel_injection.store(true, Ordering::Release);
            return;
        }
        sample.id = batch.meta.total_samples;
        sample.batch_id = batch.meta.id;
        sample.elapsed_ms = batch.started.elapsed().as_millis() as u64;
        batch.samples.push(sample);
    }

    fn message(&self, value: impl Into<String>) {
        lock(&self.data).message = value.into();
    }
}

#[derive(Default)]
pub struct GestureDebugState {
    shared: Arc<Shared>,
    // Serializes start/stop/claim-injection; never held by native callbacks.
    lifecycle: Mutex<()>,
    capture_thread: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|error| error.into_inner())
}

impl GestureDebugState {
    fn snapshot(&self) -> GestureDebugSnapshot {
        #[cfg(target_os = "macos")]
        let (available, os_version, accessibility_granted) =
            (true, macos::os_version().to_owned(), macos::trusted());
        #[cfg(not(target_os = "macos"))]
        let (available, os_version, accessibility_granted) = (false, String::new(), false);
        let data = lock(&self.shared.data);
        let capturing = self.shared.capturing.load(Ordering::Acquire);
        let batches: Vec<_> = data
            .batches
            .iter()
            .map(|batch| self.shared.batch_meta(batch))
            .collect();
        GestureDebugSnapshot {
            available,
            os_version,
            accessibility_granted,
            capturing,
            suppressing: capturing && self.shared.suppressing.load(Ordering::Acquire),
            injecting: self.shared.injecting.load(Ordering::Acquire),
            remaining_ms: if capturing {
                data.deadline.map_or(0, |deadline| {
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis() as u64
                })
            } else {
                0
            },
            total_samples: batches.iter().map(|batch| batch.total_samples).sum(),
            dropped_samples: batches.iter().map(|batch| batch.dropped_samples).sum(),
            marker_samples: batches.iter().map(|batch| batch.marker_samples).sum(),
            capacity_dropped_samples: batches
                .iter()
                .map(|batch| batch.capacity_dropped_samples)
                .sum(),
            sample_limit: MAX_SAMPLES_PER_BATCH,
            batch_limit: MAX_BATCHES,
            samples: data.batches.last().map_or_else(Vec::new, |batch| {
                batch.samples[batch.samples.len().saturating_sub(PREVIEW_SAMPLES)..].to_vec()
            }),
            batches,
            message: data.message.clone(),
        }
    }

    fn report(&self) -> Result<GestureDebugReport, String> {
        let _guard = lock(&self.lifecycle);
        self.ensure_idle()?;
        let data = lock(&self.shared.data);
        #[cfg(target_os = "macos")]
        let os_version = macos::os_version().to_owned();
        #[cfg(not(target_os = "macos"))]
        let os_version = String::new();
        Ok(GestureDebugReport {
            os_version,
            batches: data
                .batches
                .iter()
                .map(|batch| GestureDebugBatchReport {
                    batch: self.shared.batch_meta(batch),
                    samples: batch.samples.clone(),
                })
                .collect(),
        })
    }

    fn ensure_idle(&self) -> Result<(), String> {
        if self.shared.capturing.load(Ordering::Acquire)
            || self.shared.injecting.load(Ordering::Acquire)
            || lock(&self.shared.data)
                .batches
                .last()
                .is_some_and(|batch| !batch.meta.finished)
        {
            return Err("stop all experiments before exporting or clearing records to avoid truncating an active gesture".into());
        }
        Ok(())
    }

    fn clear(&self) -> Result<(), String> {
        let _guard = lock(&self.lifecycle);
        self.ensure_idle()?;
        let mut data = lock(&self.shared.data);
        data.batches.clear();
        data.deadline = None;
        data.message =
            "All trials were cleared. The next run will create a newly numbered trial.".into();
        self.shared.dropped.store(0, Ordering::Relaxed);
        Ok(())
    }

    fn ensure_available(&self) -> Result<(), String> {
        if !cfg!(target_os = "macos") {
            return Err("gesture experiments are available only in the macOS application; browser preview cannot control system input".into());
        }
        Ok(())
    }

    fn stop_capture(&self) {
        self.shared.cancel_capture.store(true, Ordering::Release);
        self.shared.suppressing.store(false, Ordering::Release);
        if let Some(thread) = lock(&self.capture_thread).take() {
            let _ = thread.join();
        }
        self.shared.capturing.store(false, Ordering::Release);
    }

    pub fn stop(&self) {
        let _guard = lock(&self.lifecycle);
        self.shared.cancel_injection.store(true, Ordering::Release);
        self.stop_capture();
        // Give the worker time to post its terminal cancellation, especially
        // when the main window/app is closing. Never wait indefinitely.
        let deadline = Instant::now() + Duration::from_millis(500);
        while self.shared.injecting.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.shared.finish_if_idle("stopped manually");
        self.shared
            .message("Capture stopped. If synthesis is active, a cancellation phase will be sent. Logs remain in memory.");
    }

    fn start_capture(&self, suppress: bool, label: &str) -> Result<(), String> {
        self.ensure_available()?;
        let _guard = lock(&self.lifecycle);
        if self.shared.injecting.load(Ordering::Acquire) {
            return Err("stop or wait for the current gesture synthesis to finish".into());
        }
        if self.shared.capturing.load(Ordering::Acquire) {
            return Err("stop the current capture before starting the next action".into());
        }
        self.stop_capture();
        self.shared.finish_if_idle("capture stopped");
        self.shared
            .begin_batch(label, if suppress { "intercept" } else { "observe" })?;
        #[cfg(target_os = "macos")]
        {
            let duration = Duration::from_secs(if suppress { 10 } else { 120 });
            self.shared.cancel_capture.store(false, Ordering::Release);
            self.shared.suppressing.store(suppress, Ordering::Release);
            lock(&self.shared.data).deadline = Some(Instant::now() + duration);
            let worker = match macos::start_capture(self.shared.clone(), suppress, duration) {
                Ok(worker) => worker,
                Err(error) => {
                    self.shared.suppressing.store(false, Ordering::Release);
                    self.shared.capturing.store(false, Ordering::Release);
                    self.shared.finish_if_idle(&error);
                    self.shared.message(&error);
                    return Err(error);
                }
            };
            *lock(&self.capture_thread) = Some(worker);
        }
        Ok(())
    }

    fn claim_injection(&self, request: &GestureDebugTrigger) -> Result<(), String> {
        self.ensure_available()?;
        request.validate()?;
        let _guard = lock(&self.lifecycle);
        if self.shared.injecting.load(Ordering::Acquire) {
            return Err(
                "a gesture is already being synthesized; stop it or wait for completion".into(),
            );
        }
        if self.shared.suppressing.load(Ordering::Acquire) {
            return Err("stop the interception test before manual synthesis to prevent the system from dropping synthesized events".into());
        }
        #[cfg(target_os = "macos")]
        macos::ensure_injection_supported()?;
        if !self.shared.register_injection(request)? {
            // Join the previous worker before opening a batch it must not close.
            self.stop_capture();
            self.shared.finish_if_idle("previous trial ended");
            self.shared
                .begin_batch(&format!("manual synthesis {:?}", request.action), "inject")?;
            self.shared.register_injection(request)?;
        }
        Ok(())
    }

    fn inject(&self, request: &GestureDebugTrigger) -> Result<(), String> {
        struct Reset<'a>(&'a Shared);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.finish_injection();
            }
        }
        let _reset = Reset(&self.shared);
        #[cfg(target_os = "macos")]
        {
            let result = macos::inject(&self.shared, request);
            if let Err(error) = &result {
                self.shared.finish_if_idle(error);
            }
            result
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = request;
            Err("gesture synthesis is unsupported on this platform".into())
        }
    }
}

impl Drop for GestureDebugState {
    fn drop(&mut self) {
        self.stop();
    }
}

#[arcrelay_desktop_ipc::command]
pub fn get_gesture_debug_snapshot(
    state: State<'_, Arc<GestureDebugState>>,
) -> GestureDebugSnapshot {
    state.snapshot()
}

#[arcrelay_desktop_ipc::command]
pub fn get_gesture_debug_report(
    state: State<'_, Arc<GestureDebugState>>,
) -> Result<GestureDebugReport, String> {
    state.report()
}

#[arcrelay_desktop_ipc::command]
pub async fn start_gesture_debug_capture(
    state: State<'_, Arc<GestureDebugState>>,
    runtime: State<'_, Arc<crate::arc_input::ArcInputRuntime>>,
    suppress: bool,
    label: String,
) -> Result<GestureDebugSnapshot, String> {
    if runtime.snapshot().configuration.input_sharing_enabled {
        return Err(
            "disable production cross-screen input before starting a local gesture experiment"
                .into(),
        );
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state.start_capture(suppress, &label)?;
        Ok(state.snapshot())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[arcrelay_desktop_ipc::command]
pub async fn stop_gesture_debug(
    state: State<'_, Arc<GestureDebugState>>,
) -> Result<GestureDebugSnapshot, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state.stop();
        state.snapshot()
    })
    .await
    .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub fn clear_gesture_debug(
    state: State<'_, Arc<GestureDebugState>>,
) -> Result<GestureDebugSnapshot, String> {
    state.clear()?;
    Ok(state.snapshot())
}

#[arcrelay_desktop_ipc::command]
pub async fn trigger_gesture_debug(
    state: State<'_, Arc<GestureDebugState>>,
    runtime: State<'_, Arc<crate::arc_input::ArcInputRuntime>>,
    request: GestureDebugTrigger,
) -> Result<GestureDebugSnapshot, String> {
    if runtime.snapshot().configuration.input_sharing_enabled {
        return Err(
            "disable production cross-screen input before starting a local gesture experiment"
                .into(),
        );
    }
    state.claim_injection(&request)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = state.inject(&request) {
            state.shared.message(&error);
            return Err(error);
        }
        Ok(state.snapshot())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/src_gesture_debug_mod_ipc.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> GestureDebugTrigger {
        GestureDebugTrigger {
            action: GestureDebugAction::SwipeRight,
            duration_ms: 500,
            delay_ms: 0,
            amount: 0.5,
            reverse: false,
            cancel_at_end: false,
        }
    }

    #[test]
    fn availability_depends_only_on_platform_not_build_profile() {
        let state = GestureDebugState::default();
        assert_eq!(state.ensure_available().is_ok(), cfg!(target_os = "macos"));
    }

    #[test]
    fn construction_and_stop_need_no_tokio_or_native_input() {
        let state = GestureDebugState::default();
        assert!(!state.shared.capturing.load(Ordering::Relaxed));
        assert!(!state.shared.injecting.load(Ordering::Relaxed));
        assert!(lock(&state.capture_thread).is_none());
        state.stop();
    }

    #[test]
    fn validates_all_untrusted_trigger_parameters() {
        assert!(request().validate().is_ok());
        for amount in [f64::NAN, f64::INFINITY, -0.1, 0.0, 1.01] {
            assert!(GestureDebugTrigger {
                amount,
                ..request()
            }
            .validate()
            .is_err());
        }
        assert!(GestureDebugTrigger {
            duration_ms: 0,
            ..request()
        }
        .validate()
        .is_err());
        assert!(GestureDebugTrigger {
            duration_ms: 2001,
            ..request()
        }
        .validate()
        .is_err());
        assert!(GestureDebugTrigger {
            delay_ms: 3001,
            ..request()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn actions_select_axes_and_direction_without_claiming_finger_count() {
        assert_eq!(request().motion_and_progress(), (1, 0.5));
        assert_eq!(
            GestureDebugTrigger {
                reverse: true,
                ..request()
            }
            .motion_and_progress(),
            (1, -0.5)
        );
        assert_eq!(
            GestureDebugTrigger {
                action: GestureDebugAction::SwipeDown,
                ..request()
            }
            .motion_and_progress(),
            (2, -0.5)
        );
        assert_eq!(
            GestureDebugTrigger {
                action: GestureDebugAction::PinchOut,
                ..request()
            }
            .motion_and_progress(),
            (3, 0.5)
        );
    }

    fn sample(phase: i64) -> GestureDebugSample {
        GestureDebugSample {
            id: 0,
            batch_id: 0,
            elapsed_ms: 0,
            source: "tap".into(),
            event_type: 30,
            subtype: 23,
            motion: 1,
            phase,
            progress: 0.5,
            velocity_x: 0.0,
            velocity_y: 0.0,
            magnification: 0.0,
            rotation: 0.0,
            swipe_mask: 0,
            suppressed: false,
        }
    }

    #[test]
    fn eight_actions_keep_all_gestures_despite_thousands_of_markers() {
        let state = GestureDebugState::default();
        for action in 1..=8 {
            state
                .shared
                .begin_batch(&format!("action {action}"), "intercept")
                .unwrap();
            state.shared.record(sample(1));
            for _ in 0..800 {
                state.shared.record(GestureDebugSample {
                    event_type: 29,
                    subtype: 0,
                    suppressed: true,
                    ..sample(0)
                });
            }
            for _ in 0..100 {
                state.shared.record(sample(2));
            }
            state.shared.record(sample(4));
            state.shared.finish_if_idle("test ended");
        }
        let report = state.report().unwrap();
        assert_eq!(report.batches.len(), 8);
        for (index, recorded) in report.batches.iter().enumerate() {
            assert_eq!(recorded.batch.id, index as u64 + 1);
            assert_eq!(recorded.batch.total_samples, 902);
            assert_eq!(recorded.batch.marker_samples, 800);
            assert_eq!(recorded.batch.suppressed_markers, 800);
            assert_eq!(recorded.batch.retained_samples, 102);
            assert_eq!(recorded.batch.capacity_dropped_samples, 0);
            assert_eq!(recorded.samples.first().unwrap().phase, 1);
            assert_eq!(recorded.samples.last().unwrap().phase, 4);
            assert!(recorded
                .samples
                .iter()
                .all(|sample| sample.batch_id == recorded.batch.id));
        }
        let preview = state.snapshot();
        assert_eq!(preview.samples.len(), PREVIEW_SAMPLES);
        assert_eq!(preview.total_samples, 902 * 8);
        assert_eq!(preview.marker_samples, 800 * 8);
        assert_eq!(preview.batches.len(), 8);
        assert!(preview.samples.iter().all(|sample| sample.batch_id == 8));
    }

    #[test]
    fn capacity_stops_capture_without_overwriting_begin_and_reports_loss() {
        let shared = Shared::default();
        shared.begin_batch("capacity test", "intercept").unwrap();
        shared.capturing.store(true, Ordering::Release);
        shared.suppressing.store(true, Ordering::Release);
        shared.record(sample(1));
        for _ in 1..MAX_SAMPLES_PER_BATCH {
            shared.record(sample(2));
        }
        shared.record(sample(4));
        assert!(shared.cancel_capture.load(Ordering::Acquire));
        assert!(shared.cancel_injection.load(Ordering::Acquire));
        assert!(!shared.suppressing.load(Ordering::Acquire));
        let data = lock(&shared.data);
        let batch = &data.batches[0];
        assert_eq!(batch.samples.len(), MAX_SAMPLES_PER_BATCH);
        assert_eq!(batch.samples[0].phase, 1);
        assert_eq!(batch.meta.capacity_dropped_samples, 1);
        assert!(batch.meta.stop_reason.contains("incomplete"));
        shared.record(sample(4)); // must never wait for a reader
        assert_eq!(shared.dropped.load(Ordering::Relaxed), 1);
        let meta = shared.batch_meta(batch);
        assert_eq!(
            meta.total_samples,
            meta.retained_samples as u64
                + meta.marker_samples
                + meta.dropped_samples
                + meta.capacity_dropped_samples
        );
    }

    #[test]
    fn export_and_clear_require_idle_and_reset_counts_without_reusing_batch_ids() {
        let state = GestureDebugState::default();
        state.shared.begin_batch("first action", "observe").unwrap();
        state.shared.record(sample(1));
        assert!(state.clear().is_err());
        assert!(state.report().is_err());
        {
            let _reader = lock(&state.shared.data);
            state.shared.record(sample(2));
        }
        state.shared.finish_if_idle("stopped");
        let report = state.report().unwrap();
        assert_eq!(report.batches[0].batch.dropped_samples, 1);
        assert_eq!(report.batches[0].batch.total_samples, 2);
        assert!(report.batches[0].batch.finished);
        state.clear().unwrap();
        assert!(state.report().unwrap().batches.is_empty());
        state
            .shared
            .begin_batch("second action", "observe")
            .unwrap();
        let snapshot = state.snapshot();
        assert_eq!(snapshot.total_samples, 0);
        assert_eq!(snapshot.dropped_samples, 0);
        assert_eq!(snapshot.batches[0].id, 2);
        assert!(snapshot.batches[0].elapsed_ms < 1000);
    }

    #[test]
    fn batch_limit_requires_explicit_clear_and_labels_are_validated() {
        let shared = Shared::default();
        assert!(shared.begin_batch(" ", "observe").is_err());
        assert!(shared.begin_batch(&"字".repeat(161), "observe").is_err());
        for _ in 0..MAX_BATCHES {
            shared.begin_batch("three-finger right", "observe").unwrap();
            shared.finish_if_idle("ended");
        }
        assert!(shared.begin_batch("four-finger down", "observe").is_err());
        assert_eq!(lock(&shared.data).batches.len(), MAX_BATCHES);
    }

    #[test]
    fn capture_stop_waits_for_injection_terminal_events_before_finishing_batch() {
        let shared = Shared::default();
        shared.begin_batch("observe synthesis", "observe").unwrap();
        shared.injecting.store(true, Ordering::Release);
        shared.finish_if_idle("capture timed out");
        assert!(!lock(&shared.data).batches[0].meta.finished);
        shared.record(GestureDebugSample {
            source: "posted".into(),
            ..sample(8)
        });
        shared.injecting.store(false, Ordering::Release);
        shared.finish_if_idle("synthesis stopped");
        let data = lock(&shared.data);
        assert!(data.batches[0].meta.finished);
        assert_eq!(data.batches[0].meta.stop_reason, "capture timed out");
        assert_eq!(data.batches[0].samples[0].phase, 8);
    }

    #[test]
    fn injection_request_and_completion_share_the_batch_lifecycle_lock() {
        let shared = Shared::default();
        assert!(!shared.register_injection(&request()).unwrap());
        shared
            .begin_batch("read-only synthesis observation", "observe")
            .unwrap();
        shared.capturing.store(true, Ordering::Release);
        assert!(shared.register_injection(&request()).unwrap());
        shared.finish_capture("capture timed out");
        assert!(!lock(&shared.data).batches[0].meta.finished);
        shared.record(GestureDebugSample {
            source: "posted".into(),
            ..sample(8)
        });
        shared.finish_injection();
        assert!(!shared.register_injection(&request()).unwrap());
        let data = lock(&shared.data);
        assert!(data.batches[0].meta.finished);
        assert_eq!(data.batches[0].meta.triggers.len(), 1);
        assert_eq!(data.batches[0].meta.stop_reason, "capture timed out");
        assert_eq!(data.batches[0].samples[0].phase, 8);
    }
}
