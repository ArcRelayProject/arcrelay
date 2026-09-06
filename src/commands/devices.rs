use super::*;

#[arcrelay_desktop_ipc::command]
pub async fn respond_pairing(
    app: AppHandle,
    state: State<'_, DesktopState>,
    accepted: bool,
    approved_grant_ids: Option<Vec<String>>,
) -> Result<(), String> {
    let (sender, decision) = state
        .take_pairing_response(None, accepted, approved_grant_ids.as_deref())
        .ok_or_else(|| "no pairing request is pending".to_string())?;
    sender
        .send(decision)
        .await
        .map_err(|_| "pairing request expired".to_string())?;
    state.finish_pairing(accepted, &app);
    Ok(())
}

#[arcrelay_desktop_ipc::command]
pub async fn discover_desktop_devices(
    state: State<'_, DesktopState>,
) -> Result<Vec<NearbyDesktopView>, String> {
    state.clipboard_sync.discover_desktops().await
}

#[arcrelay_desktop_ipc::command]
pub async fn connect_desktop_device(
    state: State<'_, DesktopState>,
    device_id: String,
) -> Result<String, String> {
    state.clipboard_sync.connect_desktop(&device_id).await
}

#[arcrelay_desktop_ipc::command]
pub async fn connect_desktop_address(
    state: State<'_, DesktopState>,
    host: String,
    port: Option<u16>,
) -> Result<String, String> {
    state
        .clipboard_sync
        .connect_desktop_address(&host, port)
        .await
}

#[arcrelay_desktop_ipc::command]
pub async fn forget_paired_device(
    state: State<'_, DesktopState>,
    device_id: String,
) -> Result<(), String> {
    state
        .command_tx
        .send(BackendCommand::ForgetPairedDevice(device_id))
        .await
        .map_err(|_| "background service is unavailable".to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn disconnect_device(
    state: State<'_, DesktopState>,
    device_id: String,
) -> Result<(), String> {
    state
        .command_tx
        .send(BackendCommand::DisconnectDevice(device_id))
        .await
        .map_err(|_| "background service is unavailable".to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn set_device_auto_connect(
    state: State<'_, DesktopState>,
    device_id: String,
    enabled: bool,
) -> Result<(), String> {
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    state
        .command_tx
        .send(BackendCommand::SetDeviceAutoConnect {
            device_id,
            enabled,
            response: response_tx,
        })
        .await
        .map_err(|_| "background service is unavailable".to_string())?;
    response_rx
        .await
        .map_err(|_| "automatic connection settings were not applied".to_string())?
}

#[arcrelay_desktop_ipc::command]
pub async fn open_input_permission_settings(app: AppHandle) -> Result<(), String> {
    crate::windowing::ensure_permission_guide_window(&app).map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn close_input_permission_guide(app: AppHandle) -> Result<(), String> {
    crate::windowing::close_permission_guide_window(&app).map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn permission_guide_ready(app: AppHandle) -> Result<(), String> {
    crate::windowing::permission_guide_window_ready(&app).map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn start_permission_guide_window_drag(app: AppHandle) -> Result<(), String> {
    crate::windowing::start_permission_guide_window_drag(&app).map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn open_accessibility_system_settings(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    state
        .command_tx
        .send(BackendCommand::OpenInputPermissionSettings)
        .await
        .map_err(|_| "background service is unavailable".to_string())?;
    crate::windowing::schedule_permission_guide_next_to_system_settings(app);
    Ok(())
}

#[arcrelay_desktop_ipc::command]
pub async fn refresh_input_permission(
    state: State<'_, DesktopState>,
) -> Result<InputPermissionState, String> {
    refresh_input_permission_state(&state).await
}

pub(super) async fn refresh_input_permission_state(
    state: &DesktopState,
) -> Result<InputPermissionState, String> {
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    state
        .command_tx
        .send(BackendCommand::RefreshInputPermission(Some(response_tx)))
        .await
        .map_err(|_| "background service is unavailable".to_string())?;
    response_rx
        .await
        .map_err(|_| "failed to refresh accessibility permission".to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn get_application_bundle_path() -> Result<String, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .map(|path| path.to_string_lossy().into_owned())
        .ok_or_else(|| "current process is not inside a macOS application bundle".to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn get_application_drag_icon_path() -> Result<String, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    if let Some(app_bundle) = executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
    {
        let bundled_icon = app_bundle
            .join("Contents")
            .join("Resources")
            .join("icon-drag-preview.png");
        if bundled_icon.is_file() {
            return Ok(bundled_icon.to_string_lossy().into_owned());
        }
    }

    let source_icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("icons")
        .join("icon-drag-preview.png");
    source_icon
        .is_file()
        .then(|| source_icon.to_string_lossy().into_owned())
        .ok_or_else(|| "ArcRelay drag icon not found".to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn quit_app(app: AppHandle) {
    let state = app.state::<DesktopState>().inner().clone();
    state.shutdown_and_exit(app).await;
}

#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/src_commands_devices_ipc.rs"));
