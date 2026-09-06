use super::*;

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAppView {
    pub name: String,
    pub path: String,
    pub identifier: Option<String>,
    pub version: Option<String>,
    pub icon_data_url: Option<String>,
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ImportActionsResponse {
    pub result: ActionImportResult,
    pub state: BootstrapState,
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct McpConfigView {
    pub endpoint: String,
    pub token: String,
    pub config_text: String,
}

static INSTALLED_APPS: OnceLock<Mutex<Option<Vec<InstalledAppView>>>> = OnceLock::new();
static INSTALLED_APP_ICONS: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
#[arcrelay_desktop_ipc::command]
pub async fn get_bootstrap_state(state: State<'_, DesktopState>) -> Result<BootstrapState, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.snapshot())
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn get_action_presets(
    state: State<'_, DesktopState>,
) -> Result<Vec<ActionPresetView>, String> {
    let service = state.action_service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        service
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .preset_views()
    })
    .await
    .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn install_action_preset(
    app: AppHandle,
    state: State<'_, DesktopState>,
    preset_id: String,
) -> Result<BootstrapState, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state
            .action_service
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .install_preset(&preset_id)?;
        state.emit_snapshot(&app);
        Ok(state.snapshot())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[arcrelay_desktop_ipc::command]
pub async fn list_installed_apps(refresh: bool) -> Result<Vec<InstalledAppView>, String> {
    let cache = INSTALLED_APPS.get_or_init(|| Mutex::new(None));
    if !refresh {
        if let Some(apps) = cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
        {
            return Ok(apps);
        }
    }

    #[cfg(target_os = "linux")]
    let apps = tauri::async_runtime::spawn_blocking(
        crate::application::automations::platform::installed_linux_apps,
    )
    .await
    .map_err(|e| e.to_string())??;
    #[cfg(not(target_os = "linux"))]
    let apps = tauri::async_runtime::spawn_blocking(|| {
        let options = app_info::ListOptions::new();
        let report = app_info::get_installed_apps_with_options(options)
            .map_err(|error| format!("failed to scan installed applications: {error}"))?;
        for warning in report.warnings {
            tracing::debug!(path = ?warning.path, message = %warning.message, "app scan warning");
        }
        let mut seen = HashSet::new();
        let mut apps: Vec<InstalledAppView> = report
            .apps
            .into_iter()
            .filter_map(|app| {
                let name = app.name.trim().to_string();
                if name.is_empty() || app.path.as_os_str().is_empty() {
                    return None;
                }
                let path = app.path.to_string_lossy().into_owned();
                #[cfg(target_os = "windows")]
                if !path.to_ascii_lowercase().ends_with(".exe") {
                    return None;
                }
                #[cfg(target_os = "windows")]
                let identifier =
                    Some(crate::application::automations::platform::normalize_executable(&path));
                #[cfg(not(target_os = "windows"))]
                let identifier = app.identifier;
                let key = format!("{}\n{}", name.to_lowercase(), path.to_lowercase());
                if !seen.insert(key) {
                    return None;
                }
                Some(InstalledAppView {
                    name,
                    path,
                    identifier,
                    version: app.version,
                    icon_data_url: None,
                })
            })
            .collect();
        apps.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.path.cmp(&right.path))
        });
        Ok::<_, String>(apps)
    })
    .await
    .map_err(|error| error.to_string())??;

    *cache.lock().unwrap_or_else(|error| error.into_inner()) = Some(apps.clone());
    Ok(apps)
}

#[arcrelay_desktop_ipc::command]
pub async fn get_installed_app_icon(path: String) -> Result<Option<String>, String> {
    let known = INSTALLED_APPS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .is_some_and(|apps| apps.iter().any(|app| app.path == path));
    if !known {
        return Err("application is not in the installed application list".to_string());
    }
    let icons = INSTALLED_APP_ICONS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(icon) = icons
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&path)
        .cloned()
    {
        return Ok(icon);
    }
    let icon_path = path.clone();
    let icon = tauri::async_runtime::spawn_blocking(move || {
        app_info::get_file_icon(&icon_path, 48)
            .ok()
            .as_ref()
            .and_then(icon_data_url)
    })
    .await
    .map_err(|error| error.to_string())?;
    icons
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(path, icon.clone());
    Ok(icon)
}

fn icon_data_url(icon: &app_info::Icon) -> Option<String> {
    let image = RgbaImage::from_raw(icon.width, icon.height, icon.pixels.to_vec())?;
    let mut png = Cursor::new(Vec::new());
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        BASE64_STANDARD.encode(png.into_inner())
    ))
}

#[arcrelay_desktop_ipc::command]
pub async fn export_actions_text(state: State<'_, DesktopState>) -> Result<String, String> {
    let service = state.action_service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        service
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .export_text()
    })
    .await
    .map_err(|error| error.to_string())?
}

#[arcrelay_desktop_ipc::command]
pub async fn import_actions_text(
    state: State<'_, DesktopState>,
    text: String,
) -> Result<ImportActionsResponse, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = state
            .action_service
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .import_text(&text)?;
        Ok(ImportActionsResponse {
            result,
            state: state.snapshot(),
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[arcrelay_desktop_ipc::command]
pub async fn validate_action_shortcut(
    app: AppHandle,
    state: State<'_, DesktopState>,
    action_id: String,
    shortcut: String,
) -> Result<(), String> {
    let _mutation = crate::action_shortcuts::MUTATION.lock().await;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        {
            let service = state
                .action_service
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            crate::action_shortcuts::validate_shortcut(
                &action_id,
                Some(&shortcut),
                service.actions(),
                &state.settings.snapshot(),
            )?;
        }
        state
            .action_shortcuts
            .validate_available(&app, &action_id, Some(&shortcut))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[arcrelay_desktop_ipc::command]
pub async fn save_action(
    app: AppHandle,
    state: State<'_, DesktopState>,
    action: QuickAction,
) -> Result<BootstrapState, String> {
    let _mutation = crate::action_shortcuts::MUTATION.lock().await;
    crate::application::configuration::save_action_locked(&app, &state, action, "desktop").await?;
    Ok(state.snapshot())
}

#[arcrelay_desktop_ipc::command]
pub async fn delete_action(
    app: AppHandle,
    state: State<'_, DesktopState>,
    action_id: String,
) -> Result<BootstrapState, String> {
    let _mutation = crate::action_shortcuts::MUTATION.lock().await;
    crate::application::configuration::delete_action_locked(
        &app, &state, &action_id, None, "desktop",
    )
    .await?;
    Ok(state.snapshot())
}

#[arcrelay_desktop_ipc::command]
pub async fn execute_action(
    app: AppHandle,
    state: State<'_, DesktopState>,
    action_id: String,
) -> Result<String, String> {
    if action_id == PRIVACY_ACTION_ID {
        let enabled = state.privacy.toggle_from_remote()?;
        state.emit_snapshot(&app);
        return Ok(if enabled {
            "投屏隐私模式已开启".to_string()
        } else {
            "投屏隐私模式已关闭".to_string()
        });
    }
    let service = state.action_service.clone();
    let refresh_toggle_state = service
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .find(&action_id)
        .is_some_and(QuickAction::is_toggle);
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::application::action_service::execute_by_id(&service, &action_id)
    })
    .await
    .map_err(|error| error.to_string())?;
    if refresh_toggle_state {
        state.emit_snapshot(&app);
    }
    result
}

#[arcrelay_desktop_ipc::command]
pub async fn set_privacy_enabled(
    state: State<'_, DesktopState>,
    enabled: bool,
) -> Result<PrivacySnapshot, String> {
    let privacy = state.privacy.clone();
    tauri::async_runtime::spawn_blocking(move || {
        privacy.set_enabled(enabled);
        privacy.snapshot()
    })
    .await
    .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn update_privacy_settings(
    state: State<'_, DesktopState>,
    settings: PrivacySettings,
) -> Result<PrivacySnapshot, String> {
    let privacy = state.privacy.clone();
    tauri::async_runtime::spawn_blocking(move || privacy.update_settings(settings))
        .await
        .map_err(|error| error.to_string())?
}

#[arcrelay_desktop_ipc::command]
pub async fn get_action_output(
    state: State<'_, DesktopState>,
    action_id: String,
) -> Result<crate::application::output_manager::ActionOutputSnapshot, String> {
    let output = state.output_manager.clone();
    tauri::async_runtime::spawn_blocking(move || output.snapshot(&action_id))
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn list_notifications(
    state: State<'_, DesktopState>,
    include_read: bool,
    limit: usize,
) -> Result<Vec<NotificationView>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state.notification_views(include_read, limit.min(500))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[arcrelay_desktop_ipc::command]
pub async fn create_test_notification(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<Vec<NotificationView>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state.notification_store.create(NewNotification {
            title: "测试通知：Host 队列工作正常".to_string(),
            body: "这条通知由桌面端写入 Host 持久队列。手机离线时会保留，任一已配对设备阅读后会在 Host 标记为全局已读。".to_string(),
            source: "ArcRelay Desktop".to_string(),
            kind: NotificationKind::Info,
            reference: Some("desktop-test".to_string()),
        })?;
        state.emit_snapshot(&app);
        state.notification_views(true, 200)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[arcrelay_desktop_ipc::command]
pub async fn mark_notification_read(
    app: AppHandle,
    state: State<'_, DesktopState>,
    notification_id: String,
) -> Result<Vec<NotificationView>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state.notification_store.mark_read(
            &notification_id,
            Some("desktop-host"),
            Some("ArcRelay Desktop"),
        )?;
        state.emit_snapshot(&app);
        state.notification_views(true, 200)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[arcrelay_desktop_ipc::command]
pub async fn delete_notification(
    app: AppHandle,
    state: State<'_, DesktopState>,
    notification_id: String,
) -> Result<Vec<NotificationView>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if !state.notification_store.delete(&notification_id)? {
            return Err("notification does not exist".to_string());
        }
        state.emit_snapshot(&app);
        state.notification_views(true, 200)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[arcrelay_desktop_ipc::command]
pub async fn get_mcp_config(state: State<'_, DesktopState>) -> Result<McpConfigView, String> {
    let endpoint = format!("http://127.0.0.1:{}/mcp", state.mcp_port);
    let token = state.mcp_token.clone();
    let config_text = format!(
        "[mcp_servers.arcrelay]\nurl = \"{endpoint}\"\nhttp_headers = {{ Authorization = \"Bearer {token}\" }}"
    );
    Ok(McpConfigView {
        endpoint,
        token,
        config_text,
    })
}

#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/src_commands_actions_ipc.rs"));
