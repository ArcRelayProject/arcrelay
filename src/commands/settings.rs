use super::*;

#[arcrelay_desktop_ipc::command]
pub fn take_pending_tray_navigation() -> Option<String> {
    crate::windowing::take_pending_tray_navigation()
}

#[arcrelay_desktop_ipc::command]
pub async fn preview_sound(event: crate::sound::SoundEvent) -> Result<(), String> {
    crate::sound::preview(event).await
}

#[arcrelay_desktop_ipc::command]
pub fn get_sound_mute_until() -> Option<i64> {
    crate::sound::temporary_mute_until()
}

#[arcrelay_desktop_ipc::command]
pub fn set_sound_temporary_mute(app: AppHandle, muted: bool) -> Result<Option<i64>, String> {
    crate::sound::set_temporary_mute(muted);
    let until = crate::sound::temporary_mute_until();
    let _ = app.emit("sound-mute-changed", until);
    Ok(until)
}

#[arcrelay_desktop_ipc::command]
pub async fn reset_sound_preferences(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<AppSettings, String> {
    let defaults = crate::sound::SoundPreferences::default();
    let patch = serde_json::from_value(serde_json::to_value(defaults).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    crate::application::settings_service::update(
        &app,
        &state,
        crate::settings::AppSettingsPatch {
            sounds: Some(patch),
            ..Default::default()
        },
    )
    .await
}

#[arcrelay_desktop_ipc::command]
pub async fn get_app_settings(state: State<'_, DesktopState>) -> Result<AppSettings, String> {
    Ok(state.settings.snapshot())
}

#[arcrelay_desktop_ipc::command]
pub async fn show_test_system_notification(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<String, String> {
    crate::desktop_notification::show_test(&app, &state.settings)?;
    Ok(crate::desktop_notification::localized(
        state.settings.snapshot().language,
        "测试通知已发送",
        "Test notification sent",
    ))
}

#[arcrelay_desktop_ipc::command]
pub async fn update_app_settings(
    app: AppHandle,
    state: State<'_, DesktopState>,
    patch: crate::settings::AppSettingsPatch,
) -> Result<AppSettings, String> {
    crate::application::settings_service::update(&app, &state, patch).await
}

#[arcrelay_desktop_ipc::command]
pub async fn open_sniptra_settings(state: State<'_, DesktopState>) -> Result<String, String> {
    if !state.settings.snapshot().enhanced_screenshot_enabled {
        return Err("enable Sniptra enhanced screenshots first".to_string());
    }
    state.screenshot.ensure_available()?;
    match state.screenshot.open_settings()? {
        crate::screenshot::StartOutcome::Started(pid) => {
            Ok(format!("Sniptra 截图设置已打开（pid {pid}）"))
        }
        crate::screenshot::StartOutcome::AlreadyRunning => {
            Ok("Sniptra 截图或设置界面已经打开".to_string())
        }
    }
}

#[arcrelay_desktop_ipc::command]
pub fn open_full_disk_access_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("failed to open Full Disk Access settings: {error}"))
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("Full Disk Access settings are only available on macOS".to_string())
    }
}

#[arcrelay_desktop_ipc::command]
pub async fn start_screenshot_capture(state: State<'_, DesktopState>) -> Result<String, String> {
    if !state.settings.snapshot().enhanced_screenshot_enabled {
        return Err("enable enhanced screenshots first".to_string());
    }
    state.screenshot.ensure_available()?;
    match state.screenshot.capture_region()? {
        crate::screenshot::StartOutcome::Started(_) => Ok("区域截图已打开".to_string()),
        crate::screenshot::StartOutcome::AlreadyRunning => Ok("截图界面已经打开".to_string()),
    }
}

pub(crate) fn apply_launch_at_startup(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        crate::autostart::enable(app)
    } else {
        crate::autostart::disable(app)
    }
    .map_err(|error| format!("failed to update launch-at-login setting: {error}"))
}

pub fn apply_configured_shortcuts(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<DesktopState>();
    let settings = state.settings.snapshot();
    register_clipboard_shortcut(app, &settings)?;
    register_screenshot_shortcut(app, &settings)
}

pub(crate) fn replace_global_shortcuts(
    app: &AppHandle,
    previous: &AppSettings,
    next: &AppSettings,
) -> Result<(), String> {
    let shortcuts = app.global_shortcut();
    let previous_shortcuts = [
        (previous.clipboard_enabled, &previous.clipboard_shortcut),
        (
            previous.enhanced_screenshot_enabled,
            &previous.screenshot_shortcut,
        ),
    ];
    // Check ownership before changing anything: a failed registration must
    // never cause rollback to unregister another feature's shortcut.
    let previous_ids = previous_shortcuts
        .iter()
        .filter(|(enabled, _)| *enabled)
        .filter_map(|(_, key)| key.parse::<tauri_plugin_global_shortcut::Shortcut>().ok())
        .collect::<HashSet<_>>();
    for (enabled, key) in [
        (next.clipboard_enabled, &next.clipboard_shortcut),
        (next.enhanced_screenshot_enabled, &next.screenshot_shortcut),
    ] {
        if enabled {
            let shortcut = key
                .parse::<tauri_plugin_global_shortcut::Shortcut>()
                .map_err(|e| e.to_string())?;
            if shortcuts.is_registered(shortcut) && !previous_ids.contains(&shortcut) {
                return Err(
                    "global shortcut is already used by another ArcRelay feature".to_string(),
                );
            }
        }
    }
    for (enabled, shortcut) in previous_shortcuts {
        if !enabled {
            continue;
        }
        if shortcuts.is_registered(shortcut.as_str()) {
            shortcuts
                .unregister(shortcut.as_str())
                .map_err(|error| format!("failed to update global shortcut: {error}"))?;
        }
    }
    if let Err(error) =
        register_clipboard_shortcut(app, next).and_then(|_| register_screenshot_shortcut(app, next))
    {
        if next.clipboard_enabled {
            let _ = shortcuts.unregister(next.clipboard_shortcut.as_str());
        }
        if next.enhanced_screenshot_enabled {
            let _ = shortcuts.unregister(next.screenshot_shortcut.as_str());
        }
        let _ = register_clipboard_shortcut(app, previous);
        let _ = register_screenshot_shortcut(app, previous);
        return Err(error);
    }
    Ok(())
}

fn register_clipboard_shortcut(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    if !settings.clipboard_enabled {
        return Ok(());
    }
    let shortcut = settings.clipboard_shortcut.clone();
    app.global_shortcut()
        .on_shortcut(shortcut.as_str(), |app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            if !app
                .state::<DesktopState>()
                .settings
                .snapshot()
                .clipboard_enabled
            {
                return;
            }
            if crate::commands::clipboard_continuous_paste_active() {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) =
                        crate::commands::paste_next_continuous_record(app.clone()).await
                    {
                        tracing::warn!(%error, "Failed to execute continuous clipboard paste");
                        let _ = crate::commands::clipboard_stop_continuous_paste().await;
                        let _ = app.emit("clipboard-continuous-paste-error", error);
                    }
                });
                return;
            }
            if let Err(error) = crate::windowing::toggle_clipboard_window(app) {
                tracing::warn!(%error, "failed to toggle clipboard window");
            }
        })
        .map_err(|error| format!("failed to register clipboard shortcut; it may conflict with another application: {error}"))
}

fn register_screenshot_shortcut(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    if !settings.enhanced_screenshot_enabled {
        return Ok(());
    }
    let shortcut = settings.screenshot_shortcut.clone();
    app.global_shortcut()
        .on_shortcut(shortcut.as_str(), |app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let state = app.state::<DesktopState>();
            if !state.settings.snapshot().enhanced_screenshot_enabled {
                return;
            }
            let screenshot = state.screenshot.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(error) = screenshot.capture_region() {
                    tracing::warn!(%error, "failed to start screenshot from global shortcut");
                }
            });
        })
        .map_err(|error| format!("failed to register screenshot shortcut; it may conflict with another application: {error}"))
}

#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/src_commands_settings_ipc.rs"));
