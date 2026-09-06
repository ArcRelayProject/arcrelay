use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

const UPDATE_PROGRESS_EVENT: &str = "app-update-progress";
const UPDATE_NOTIFICATION_VERSION_FILE: &str = "last-update-notification-version";

#[derive(Default)]
pub struct AppUpdateState {
    checking: tokio::sync::Mutex<()>,
    pending: Mutex<Option<Update>>,
    last_notified_version: Mutex<Option<String>>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateMetadata {
    version: String,
    notes: Option<String>,
    published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateCheckResult {
    current_version: String,
    update: Option<AppUpdateMetadata>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateProgress {
    phase: AppUpdateProgressPhase,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum AppUpdateProgressPhase {
    Downloading,
    Downloaded,
    Installed,
    Failed,
}

#[arcrelay_desktop_ipc::command]
pub async fn check_for_app_update(
    app: AppHandle,
    update_state: State<'_, AppUpdateState>,
    desktop_state: State<'_, crate::backend::DesktopState>,
) -> Result<AppUpdateCheckResult, String> {
    let _checking = update_state.checking.lock().await;
    let current_version = app.package_info().version.to_string();
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(30))
        .header("Cache-Control", "no-cache")
        .map_err(|error| format!("failed to configure update request: {error}"))?
        .build()
        .map_err(|error| format!("failed to initialize updater: {error}"))?;
    let update = updater
        .check()
        .await
        .map_err(|error| format!("failed to check for updates: {error}"))?;
    let metadata = update.as_ref().map(|update| AppUpdateMetadata {
        version: update.version.clone(),
        notes: update.body.clone(),
        published_at: update.date.map(|date| date.to_string()),
    });
    *update_state
        .pending
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = update;
    if let Some(metadata) = metadata.as_ref() {
        let in_memory_version = update_state
            .last_notified_version
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let already_notified = in_memory_version
            .or_else(read_last_notified_version)
            .as_deref()
            == Some(metadata.version.as_str());
        if !already_notified {
            let language = desktop_state.settings.snapshot().language;
            let notification = crate::desktop_notification::DesktopNotification::new(
                crate::desktop_notification::DesktopNotificationCategory::UpdateAvailable,
                crate::desktop_notification::localized(
                    language,
                    &format!("ArcRelay {} 可以更新", metadata.version),
                    &format!("ArcRelay {} is available", metadata.version),
                ),
                crate::desktop_notification::localized(
                    language,
                    "打开设置查看更新说明并安装。",
                    "Open Settings to review and install the update.",
                ),
            );
            match crate::desktop_notification::show(&app, &desktop_state.settings, notification) {
                Ok(crate::desktop_notification::DesktopNotificationDelivery::Shown) => {
                    *update_state
                        .last_notified_version
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) =
                        Some(metadata.version.clone());
                    if let Err(error) = write_last_notified_version(&metadata.version) {
                        tracing::warn!(%error, "failed to persist update notification version");
                    }
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(%error, "failed to show update notification"),
            }
        }
    }
    let result = AppUpdateCheckResult {
        current_version,
        update: metadata,
    };
    let _ = app.emit("app-update-checked", &result);
    Ok(result)
}

fn update_notification_version_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("ArcRelay")
        .join(UPDATE_NOTIFICATION_VERSION_FILE)
}

fn read_last_notified_version() -> Option<String> {
    std::fs::read_to_string(update_notification_version_path())
        .ok()
        .map(|version| version.trim().to_string())
        .filter(|version| !version.is_empty() && version.len() <= 128)
}

fn write_last_notified_version(version: &str) -> Result<(), String> {
    crate::infrastructure::durable_file::replace(
        &update_notification_version_path(),
        version.as_bytes(),
    )
    .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn install_app_update(
    app: AppHandle,
    state: State<'_, AppUpdateState>,
) -> Result<(), String> {
    let update = state
        .pending
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
        .ok_or_else(|| "no update is pending installation; check for updates first".to_string())?;
    let retry_update = update.clone();
    let downloaded = Arc::new(AtomicU64::new(0));
    let progress_app = app.clone();
    let progress_downloaded = downloaded.clone();
    let finished_app = app.clone();
    let finished_downloaded = downloaded.clone();

    let result = update
        .download_and_install(
            move |chunk_length, total_bytes| {
                let downloaded_bytes = progress_downloaded
                    .fetch_add(chunk_length as u64, Ordering::Relaxed)
                    + chunk_length as u64;
                let _ = progress_app.emit(
                    UPDATE_PROGRESS_EVENT,
                    AppUpdateProgress {
                        phase: AppUpdateProgressPhase::Downloading,
                        downloaded_bytes,
                        total_bytes,
                    },
                );
            },
            move || {
                let _ = finished_app.emit(
                    UPDATE_PROGRESS_EVENT,
                    AppUpdateProgress {
                        phase: AppUpdateProgressPhase::Downloaded,
                        downloaded_bytes: finished_downloaded.load(Ordering::Relaxed),
                        total_bytes: None,
                    },
                );
            },
        )
        .await;

    if let Err(error) = result {
        *state
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(retry_update);
        let _ = app.emit(
            UPDATE_PROGRESS_EVENT,
            AppUpdateProgress {
                phase: AppUpdateProgressPhase::Failed,
                downloaded_bytes: downloaded.load(Ordering::Relaxed),
                total_bytes: None,
            },
        );
        return Err(format!("failed to install update: {error}"));
    }

    let _ = app.emit(
        UPDATE_PROGRESS_EVENT,
        AppUpdateProgress {
            phase: AppUpdateProgressPhase::Installed,
            downloaded_bytes: downloaded.load(Ordering::Relaxed),
            total_bytes: None,
        },
    );
    #[cfg(target_os = "windows")]
    return Ok(());
    #[cfg(not(target_os = "windows"))]
    app.restart()
}

/// One scheduler per Rust host, independent of WebView creation or visibility.
pub fn start_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        loop {
            let state = app.state::<crate::backend::DesktopState>();
            if state.settings.snapshot().auto_update_enabled {
                if let Err(error) =
                    check_for_app_update(app.clone(), app.state::<AppUpdateState>(), state).await
                {
                    tracing::debug!(%error, "automatic update check failed");
                }
            }
            tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
        }
    });
}

#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/src_app_update_ipc.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_has_no_pending_update() {
        let state = AppUpdateState::default();
        assert!(state
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_none());
    }
}
