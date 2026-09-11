#[cfg(target_os = "macos")]
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use tauri::{WebviewUrl, WebviewWindowBuilder};
use tokio::sync::Notify;

pub const PRIVACY_ACTION_ID: &str = "builtin-privacy-mode";

#[cfg(target_os = "macos")]
const ACTIVE_FALLBACK_INTERVAL: Duration = Duration::from_millis(100);
#[cfg(not(target_os = "macos"))]
const ACTIVE_FALLBACK_INTERVAL: Duration = Duration::from_millis(400);
const IDLE_FALLBACK_INTERVAL: Duration = Duration::from_secs(2);
const OVERLAY_REASSERT_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum MaskStyle {
    #[default]
    Frosted,
    Solid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedApp {
    pub name: String,
    pub identifier: Option<String>,
    pub path: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PrivacySettings {
    #[serde(default = "default_true")]
    pub auto_enable_on_mirror: bool,
    #[serde(default = "default_true")]
    pub allow_remote_actions: bool,
    #[serde(default)]
    pub mask_style: MaskStyle,
    #[serde(default = "default_protected_apps")]
    pub protected_apps: Vec<ProtectedApp>,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            auto_enable_on_mirror: true,
            allow_remote_actions: true,
            mask_style: MaskStyle::Frosted,
            protected_apps: default_protected_apps(),
        }
    }
}

fn default_protected_apps() -> Vec<ProtectedApp> {
    vec![
        ProtectedApp {
            name: "微信".to_string(),
            identifier: Some("com.tencent.xinWeChat".to_string()),
            path: None,
            enabled: true,
        },
        ProtectedApp {
            name: "钉钉".to_string(),
            identifier: Some("com.alibaba.DingTalkMac".to_string()),
            path: None,
            enabled: true,
        },
        ProtectedApp {
            name: "飞书".to_string(),
            identifier: Some("com.bytedance.Feishu".to_string()),
            path: None,
            enabled: true,
        },
    ]
}

#[derive(Debug, Clone, PartialEq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedWindowView {
    pub window_id: u64,
    pub app_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PrivacySnapshot {
    pub active: bool,
    pub manual_enabled: bool,
    pub mirror_detected: bool,
    pub presence_guard: bool,
    pub activation_source: String,
    pub visible_protected_windows: Vec<ProtectedWindowView>,
    pub settings: PrivacySettings,
}

#[derive(Debug, Clone, PartialEq)]
struct NativeWindow {
    id: u64,
    app_name: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    // CGWindowList is ordered front-to-back. Tracking that position makes a
    // target window being raised observable even when its geometry is unchanged.
    #[cfg(target_os = "macos")]
    z_order: usize,
}

#[derive(Debug, Clone)]
struct ForegroundApp {
    name: String,
    identifier: Option<String>,
}

#[derive(Debug, Default)]
struct RuntimeState {
    manual_enabled: bool,
    mirror_detected: bool,
    presence_guard: bool,
    suppress_auto_until_mirror_ends: bool,
    visible_windows: Vec<NativeWindow>,
    overlay_style: Option<MaskStyle>,
    last_overlay_reassert: Option<Instant>,
}

#[derive(Default)]
struct WindowScanCache {
    #[cfg(target_os = "macos")]
    identifiers: HashMap<i64, Option<String>>,
}

#[derive(Clone)]
struct OverlayRequest {
    windows: Vec<NativeWindow>,
    style: MaskStyle,
    force_order_front: bool,
}

#[derive(Default)]
struct OverlayScheduleState {
    pending: bool,
    latest: Option<OverlayRequest>,
}

#[derive(Default)]
struct OverlayScheduler {
    state: Mutex<OverlayScheduleState>,
}

pub struct PrivacyManager {
    settings_path: PathBuf,
    settings: RwLock<PrivacySettings>,
    runtime: RwLock<RuntimeState>,
    app: Mutex<Option<AppHandle>>,
    wake: Arc<Notify>,
    scan_cache: Mutex<WindowScanCache>,
    overlay_scheduler: Arc<OverlayScheduler>,
    force_overlay_refresh: Arc<AtomicBool>,
    started: AtomicBool,
}

impl PrivacyManager {
    pub fn new(config_dir: &Path) -> Arc<Self> {
        let settings_path = config_dir.join("privacy.json");
        let settings = std::fs::read(&settings_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PrivacySettings>(&bytes).ok())
            .unwrap_or_default();
        Arc::new(Self {
            settings_path,
            settings: RwLock::new(normalize_settings(settings)),
            runtime: RwLock::new(RuntimeState::default()),
            app: Mutex::new(None),
            wake: Arc::new(Notify::new()),
            scan_cache: Mutex::new(WindowScanCache::default()),
            overlay_scheduler: Arc::new(OverlayScheduler::default()),
            force_overlay_refresh: Arc::new(AtomicBool::new(false)),
            started: AtomicBool::new(false),
        })
    }

    pub fn start(self: &Arc<Self>, app: AppHandle) {
        *self.app.lock().unwrap_or_else(|error| error.into_inner()) = Some(app.clone());
        install_platform_event_wake(&app, self.wake.clone(), self.force_overlay_refresh.clone());
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let poll_manager = manager.clone();
                let poll_app = app.clone();
                if let Err(error) = tauri::async_runtime::spawn_blocking(move || {
                    poll_manager.poll_once(&poll_app);
                })
                .await
                {
                    tracing::warn!(%error, "Privacy refresh worker failed");
                }

                let snapshot = manager.snapshot();
                let fallback = privacy_fallback_interval(
                    snapshot.active,
                    snapshot.visible_protected_windows.len(),
                );
                tokio::select! {
                    _ = tokio::time::sleep(fallback) => {}
                    _ = manager.wake.notified() => {}
                }
            }
        });
    }

    pub fn request_refresh(&self) {
        self.wake.notify_one();
    }

    pub fn sound_is_private(&self) -> bool {
        let settings = self.settings.read().unwrap_or_else(|e| e.into_inner());
        let runtime = self.runtime.read().unwrap_or_else(|e| e.into_inner());
        is_active(&settings, &runtime)
    }

    pub fn snapshot(&self) -> PrivacySnapshot {
        let settings = self
            .settings
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let runtime = self
            .runtime
            .read()
            .unwrap_or_else(|error| error.into_inner());
        snapshot_from(&settings, &runtime)
    }

    pub fn set_enabled(&self, enabled: bool) {
        {
            let mut runtime = self
                .runtime
                .write()
                .unwrap_or_else(|error| error.into_inner());
            runtime.manual_enabled = enabled;
            if enabled {
                runtime.suppress_auto_until_mirror_ends = false;
            } else if runtime.mirror_detected {
                runtime.suppress_auto_until_mirror_ends = true;
            }
        }
        self.emit_snapshot();
        self.request_refresh();
    }

    /// Engage privacy overlays because local presence recognition cannot confirm
    /// that only the enrolled owner is viewing the screen. This state is kept
    /// separate from the user's manual preference so recognition never disables
    /// privacy that the user explicitly enabled.
    pub fn set_presence_guard(&self, enabled: bool) {
        let changed = {
            let mut runtime = self
                .runtime
                .write()
                .unwrap_or_else(|error| error.into_inner());
            let changed = runtime.presence_guard != enabled;
            runtime.presence_guard = enabled;
            changed
        };
        if changed {
            self.emit_snapshot();
            self.request_refresh();
        }
    }

    pub fn toggle_from_remote(&self) -> Result<bool, String> {
        if !self
            .settings
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .allow_remote_actions
        {
            return Err("mobile privacy-mode control is disabled on the desktop".to_string());
        }
        let next = !self.snapshot().active;
        self.set_enabled(next);
        Ok(next)
    }

    pub fn toggle_local(&self) -> bool {
        let next = !self.snapshot().active;
        self.set_enabled(next);
        next
    }

    pub fn update_settings(&self, settings: PrivacySettings) -> Result<PrivacySnapshot, String> {
        let settings = normalize_settings(settings);
        persist_settings(&self.settings_path, &settings)?;
        *self
            .settings
            .write()
            .unwrap_or_else(|error| error.into_inner()) = settings;
        self.emit_snapshot();
        self.request_refresh();
        Ok(self.snapshot())
    }

    pub fn disable_frontmost_app(&self) -> Result<Option<String>, String> {
        let Some(frontmost) = platform::frontmost_application() else {
            return Ok(None);
        };
        let disabled_name = {
            let mut settings = self
                .settings
                .write()
                .unwrap_or_else(|error| error.into_inner());
            let Some(disabled_name) = disable_matching_app(
                &mut settings,
                &frontmost.name,
                frontmost.identifier.as_deref(),
            ) else {
                return Ok(None);
            };
            persist_settings(&self.settings_path, &settings)?;
            disabled_name
        };
        self.emit_snapshot();
        self.request_refresh();
        Ok(Some(disabled_name))
    }

    fn poll_once(&self, app: &AppHandle) {
        let event_force_order_front = self.force_overlay_refresh.swap(false, Ordering::SeqCst);
        let settings = self
            .settings
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let mirror_detected = platform::screen_mirrored();

        let (active, previous_active, mirror_changed) = {
            let mut runtime = self
                .runtime
                .write()
                .unwrap_or_else(|error| error.into_inner());
            let previous_active = is_active(&settings, &runtime);
            let previous_mirror = runtime.mirror_detected;
            runtime.mirror_detected = mirror_detected;
            if !mirror_detected {
                runtime.suppress_auto_until_mirror_ends = false;
            }
            (
                is_active(&settings, &runtime),
                previous_active,
                previous_mirror != mirror_detected,
            )
        };

        let windows = if active {
            let mut cache = self
                .scan_cache
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            platform::visible_protected_windows(&settings.protected_apps, &mut cache)
        } else {
            Vec::new()
        };

        let (overlay_update_needed, force_order_front, snapshot_changed) = {
            let mut runtime = self
                .runtime
                .write()
                .unwrap_or_else(|error| error.into_inner());
            let layout_changed = runtime.visible_windows != windows || active != previous_active;
            let snapshot_changed = layout_changed || mirror_changed;
            let style_changed = runtime.overlay_style != active.then_some(settings.mask_style);
            let now = Instant::now();
            let force_order_front = active
                && (event_force_order_front
                    || runtime
                        .last_overlay_reassert
                        .is_none_or(|last| now.duration_since(last) >= OVERLAY_REASSERT_INTERVAL));
            let overlay_update_needed = layout_changed || style_changed || force_order_front;
            runtime.visible_windows = windows.clone();
            runtime.overlay_style = active.then_some(settings.mask_style);
            if overlay_update_needed {
                runtime.last_overlay_reassert = active.then_some(now);
            }
            (
                overlay_update_needed,
                force_order_front || layout_changed,
                snapshot_changed,
            )
        };

        if overlay_update_needed {
            schedule_overlay_reconcile(
                self.overlay_scheduler.clone(),
                app.clone(),
                windows,
                settings.mask_style,
                force_order_front,
            );
        }
        if snapshot_changed {
            self.emit_snapshot_to(app);
        }
    }

    fn emit_snapshot(&self) {
        let app = self
            .app
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if let Some(app) = app {
            self.emit_snapshot_to(&app);
        }
    }

    fn emit_snapshot_to(&self, app: &AppHandle) {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.emit("privacy-state", self.snapshot());
        }
    }
}

fn privacy_fallback_interval(active: bool, visible_protected_windows: usize) -> Duration {
    if !active {
        IDLE_FALLBACK_INTERVAL
    } else if visible_protected_windows == 0 {
        Duration::from_secs(1)
    } else {
        ACTIVE_FALLBACK_INTERVAL
    }
}

fn persist_settings(path: &Path, settings: &PrivacySettings) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?;
    crate::infrastructure::durable_file::replace(path, &bytes).map_err(|error| error.to_string())
}

fn disable_matching_app(
    settings: &mut PrivacySettings,
    owner_name: &str,
    owner_identifier: Option<&str>,
) -> Option<String> {
    for protected_app in &mut settings.protected_apps {
        if protected_app.enabled
            && matches_protected_app(
                owner_name,
                owner_identifier,
                std::slice::from_ref(protected_app),
            )
        {
            protected_app.enabled = false;
            return Some(protected_app.name.clone());
        }
    }
    None
}

fn is_active(settings: &PrivacySettings, runtime: &RuntimeState) -> bool {
    runtime.manual_enabled
        || runtime.presence_guard
        || (settings.auto_enable_on_mirror
            && runtime.mirror_detected
            && !runtime.suppress_auto_until_mirror_ends)
}

fn snapshot_from(settings: &PrivacySettings, runtime: &RuntimeState) -> PrivacySnapshot {
    let active = is_active(settings, runtime);
    let activation_source = if runtime.manual_enabled {
        "manual"
    } else if runtime.presence_guard {
        "presence"
    } else if active && runtime.mirror_detected {
        "screenMirror"
    } else {
        "off"
    };
    PrivacySnapshot {
        active,
        manual_enabled: runtime.manual_enabled,
        mirror_detected: runtime.mirror_detected,
        presence_guard: runtime.presence_guard,
        activation_source: activation_source.to_string(),
        visible_protected_windows: runtime
            .visible_windows
            .iter()
            .map(|window| ProtectedWindowView {
                window_id: window.id,
                app_name: window.app_name.clone(),
            })
            .collect(),
        settings: settings.clone(),
    }
}

fn normalize_settings(mut settings: PrivacySettings) -> PrivacySettings {
    let mut seen = HashSet::new();
    settings.protected_apps.retain_mut(|app| {
        app.name = app.name.trim().chars().take(128).collect();
        app.identifier = app
            .identifier
            .take()
            .map(|value| value.trim().chars().take(256).collect::<String>())
            .filter(|value| !value.is_empty());
        app.path = app
            .path
            .take()
            .map(|value| value.trim().chars().take(2048).collect::<String>())
            .filter(|value| !value.is_empty());
        if app.name.is_empty() {
            return false;
        }
        let key = app
            .identifier
            .as_ref()
            .or(app.path.as_ref())
            .cloned()
            .unwrap_or_else(|| app.name.clone())
            .to_lowercase();
        seen.insert(key)
    });
    settings
}

fn schedule_overlay_reconcile(
    scheduler: Arc<OverlayScheduler>,
    app: AppHandle,
    windows: Vec<NativeWindow>,
    style: MaskStyle,
    force_order_front: bool,
) {
    let should_dispatch = {
        let mut state = scheduler
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.latest = Some(OverlayRequest {
            windows,
            style,
            force_order_front,
        });
        if state.pending {
            false
        } else {
            state.pending = true;
            true
        }
    };
    if should_dispatch {
        dispatch_overlay_reconcile(scheduler, app);
    }
}

fn dispatch_overlay_reconcile(scheduler: Arc<OverlayScheduler>, app: AppHandle) {
    let callback_scheduler = scheduler.clone();
    let callback_app = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        let request = {
            callback_scheduler
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .latest
                .take()
        };
        if let Some(request) = request {
            if let Err(error) =
                reconcile_overlays(&request.windows, request.style, request.force_order_front)
            {
                tracing::warn!(%error, "Failed to update privacy overlays");
            }
        }

        let should_dispatch_again = {
            let mut state = callback_scheduler
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state.latest.is_some() {
                true
            } else {
                state.pending = false;
                false
            }
        };
        if should_dispatch_again {
            dispatch_overlay_reconcile(callback_scheduler, callback_app);
        }
    }) {
        scheduler
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pending = false;
        tracing::warn!(%error, "Failed to schedule privacy overlays");
    }
}

#[cfg(target_os = "macos")]
fn reconcile_overlays(
    windows: &[NativeWindow],
    style: MaskStyle,
    force_order_front: bool,
) -> Result<(), String> {
    native_overlay::reconcile(windows, style, force_order_front)
}

#[cfg(target_os = "windows")]
fn reconcile_overlays(
    windows: &[NativeWindow],
    style: MaskStyle,
    force_order_front: bool,
) -> Result<(), String> {
    native_overlay::reconcile(windows, style, force_order_front)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn reconcile_overlays(
    _windows: &[NativeWindow],
    _style: MaskStyle,
    _force_order_front: bool,
) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_platform_event_wake(
    app: &AppHandle,
    wake: Arc<Notify>,
    force_overlay_refresh: Arc<AtomicBool>,
) {
    native_overlay::install_event_wake(app, wake, force_overlay_refresh);
}

#[cfg(target_os = "windows")]
fn install_platform_event_wake(
    _app: &AppHandle,
    wake: Arc<Notify>,
    force_overlay_refresh: Arc<AtomicBool>,
) {
    native_overlay::install_event_wake(wake, force_overlay_refresh);
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn install_platform_event_wake(
    _app: &AppHandle,
    _wake: Arc<Notify>,
    _force_overlay_refresh: Arc<AtomicBool>,
) {
}

#[cfg(target_os = "macos")]
#[path = "privacy/macos_overlay.rs"]
mod native_overlay;
#[cfg(target_os = "windows")]
#[path = "privacy/windows_overlay.rs"]
mod native_overlay;
fn matches_protected_app(
    owner_name: &str,
    owner_identifier: Option<&str>,
    apps: &[ProtectedApp],
) -> bool {
    let owner_name = owner_name.trim().to_lowercase();
    let owner_identifier = owner_identifier.map(str::to_lowercase);
    apps.iter().filter(|app| app.enabled).any(|app| {
        let configured_name = app.name.to_lowercase();
        let name_matches = owner_name == configured_name
            || owner_name.contains(&configured_name)
            || known_aliases(&configured_name)
                .iter()
                .any(|alias| owner_name.contains(alias));
        let identifier_matches = app.identifier.as_ref().is_some_and(|identifier| {
            owner_identifier
                .as_ref()
                .is_some_and(|owner| owner.eq_ignore_ascii_case(identifier))
        });
        let path_matches = app.path.as_ref().is_some_and(|path| {
            Path::new(path)
                .file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|stem| owner_name.contains(&stem.to_lowercase()))
        });
        name_matches || identifier_matches || path_matches
    })
}

fn known_aliases(name: &str) -> &'static [&'static str] {
    if name.contains("微信") || name.contains("wechat") {
        &["微信", "wechat"]
    } else if name.contains("钉钉") || name.contains("dingtalk") {
        &["钉钉", "dingtalk"]
    } else if name.contains("飞书") || name.contains("feishu") || name.contains("lark") {
        &["飞书", "feishu", "lark"]
    } else {
        &[]
    }
}

#[cfg(target_os = "macos")]
#[path = "privacy/macos.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "privacy/windows.rs"]
mod platform;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[path = "privacy/fallback.rs"]
mod platform;
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_include_common_messaging_apps() {
        let settings = PrivacySettings::default();
        assert_eq!(settings.protected_apps.len(), 3);
        assert!(settings.auto_enable_on_mirror);
        assert!(settings.allow_remote_actions);
    }

    #[test]
    fn aliases_match_localized_and_english_names() {
        let settings = PrivacySettings::default();
        assert!(matches_protected_app(
            "WeChat",
            None,
            &settings.protected_apps
        ));
        assert!(matches_protected_app(
            "钉钉",
            None,
            &settings.protected_apps
        ));
        assert!(matches_protected_app(
            "Lark",
            None,
            &settings.protected_apps
        ));
    }

    #[test]
    fn shortcut_disable_only_turns_off_the_frontmost_app() {
        let mut settings = PrivacySettings {
            protected_apps: vec![
                ProtectedApp {
                    name: "App Store".to_string(),
                    identifier: Some("com.apple.AppStore".to_string()),
                    path: None,
                    enabled: true,
                },
                ProtectedApp {
                    name: "Mail".to_string(),
                    identifier: Some("com.apple.mail".to_string()),
                    path: None,
                    enabled: true,
                },
            ],
            ..PrivacySettings::default()
        };
        assert_eq!(
            disable_matching_app(&mut settings, "App Store", Some("com.apple.AppStore")),
            Some("App Store".to_string())
        );
        assert!(!settings.protected_apps[0].enabled);
        assert!(settings.protected_apps[1].enabled);
    }

    #[test]
    fn automatic_privacy_tracks_mirror_state() {
        let settings = PrivacySettings::default();
        let mut runtime = RuntimeState::default();

        assert!(!is_active(&settings, &runtime));
        runtime.mirror_detected = true;
        assert!(is_active(&settings, &runtime));
        runtime.mirror_detected = false;
        assert!(!is_active(&settings, &runtime));
    }

    #[test]
    fn manual_privacy_remains_enabled_after_mirroring_ends() {
        let settings = PrivacySettings::default();
        let runtime = RuntimeState {
            manual_enabled: true,
            mirror_detected: false,
            ..RuntimeState::default()
        };

        assert!(is_active(&settings, &runtime));
        assert_eq!(
            snapshot_from(&settings, &runtime).activation_source,
            "manual"
        );
    }

    #[test]
    fn presence_guard_is_independent_from_manual_privacy() {
        let settings = PrivacySettings::default();
        let mut runtime = RuntimeState {
            presence_guard: true,
            ..RuntimeState::default()
        };
        assert!(is_active(&settings, &runtime));
        assert_eq!(
            snapshot_from(&settings, &runtime).activation_source,
            "presence"
        );

        runtime.manual_enabled = true;
        runtime.presence_guard = false;
        assert!(is_active(&settings, &runtime));
        assert_eq!(
            snapshot_from(&settings, &runtime).activation_source,
            "manual"
        );
    }

    #[test]
    fn privacy_polling_is_fast_only_while_protected_windows_are_visible() {
        assert_eq!(privacy_fallback_interval(false, 0), IDLE_FALLBACK_INTERVAL);
        assert_eq!(privacy_fallback_interval(true, 0), Duration::from_secs(1));
        assert_eq!(privacy_fallback_interval(true, 1), ACTIVE_FALLBACK_INTERVAL);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn disabled_catalog_skips_window_scan_and_clears_identifier_cache() {
        let mut apps = PrivacySettings::default().protected_apps;
        for app in &mut apps {
            app.enabled = false;
        }
        let mut cache = WindowScanCache::default();
        cache
            .identifiers
            .insert(1234, Some("com.example.cached".to_string()));

        assert!(platform::visible_protected_windows(&apps, &mut cache).is_empty());
        assert!(cache.identifiers.is_empty());
    }
}
