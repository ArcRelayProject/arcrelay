use super::*;
use std::sync::Arc;
mod draft;
pub use draft::TransferDraftFile;

#[arcrelay_desktop_ipc::command]
pub fn take_tray_transfer_drop(app: AppHandle) -> crate::windowing::tray_transfer::DropSnapshot {
    crate::windowing::tray_transfer::take_drop(&app)
}

#[arcrelay_desktop_ipc::command]
pub fn update_tray_transfer_panel(
    app: AppHandle,
    height: f64,
    has_content: bool,
) -> Result<(), String> {
    crate::windowing::tray_transfer::update_panel(&app, height, has_content)
}

#[arcrelay_desktop_ipc::command]
pub fn hide_tray_transfer_panel(app: AppHandle) -> Result<(), String> {
    crate::windowing::tray_transfer::hide(&app).map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub fn open_tray_transfer_history(app: AppHandle) -> Result<(), String> {
    crate::windowing::tray_transfer::open_history(&app)
}

#[arcrelay_desktop_ipc::command]
pub async fn inspect_transfer_files(paths: Vec<String>) -> Result<Vec<TransferDraftFile>, String> {
    tokio::task::spawn_blocking(move || draft::inspect_paths(paths))
        .await
        .map_err(|error| format!("failed to read file metadata: {error}"))?
}

#[arcrelay_desktop_ipc::command]
pub async fn get_transfer_state(
    state: State<'_, DesktopState>,
) -> Result<arcrelay_transfer::TransferSnapshot, String> {
    Ok(state.transfer().await?.snapshot().await)
}

#[arcrelay_desktop_ipc::command]
pub fn take_pending_transfer_request() -> Option<String> {
    crate::desktop_notification::take_pending_transfer_request()
}

#[arcrelay_desktop_ipc::command]
pub async fn refresh_transfer_devices(
    state: State<'_, DesktopState>,
) -> Result<arcrelay_transfer::TransferSnapshot, String> {
    state
        .transfer()
        .await?
        .refresh_devices()
        .await
        .map_err(|error| error.to_string())
}
#[arcrelay_desktop_ipc::command]
pub async fn pick_transfer_files() -> Result<Vec<String>, String> {
    Ok(rfd::AsyncFileDialog::new()
        .set_title("选择要发送的文件")
        .pick_files()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|file| file.path().to_string_lossy().into_owned())
        .collect())
}

#[arcrelay_desktop_ipc::command]
pub async fn send_transfer(
    state: State<'_, DesktopState>,
    peer_id: String,
    paths: Vec<String>,
) -> Result<String, String> {
    state
        .transfer()
        .await?
        .clone()
        .send_files(
            peer_id,
            paths.into_iter().map(std::path::PathBuf::from).collect(),
        )
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn send_system_share_transfer(
    app: AppHandle,
    state: State<'_, DesktopState>,
    system_share: State<'_, Arc<crate::system_share::SystemShareService>>,
    peer_id: String,
    paths: Vec<String>,
    request_ids: Vec<String>,
) -> Result<String, String> {
    let paths: Vec<_> = paths.into_iter().map(std::path::PathBuf::from).collect();
    system_share
        .validate_pending_requests(&request_ids, &paths)
        .map_err(|error| error.to_string())?;
    let transfer = state.transfer().await?;
    let transfer_id = transfer
        .clone()
        .send_files(peer_id.clone(), paths)
        .await
        .map_err(|error| error.to_string())?;
    if let Err(error) = system_share.submit_many(&request_ids, &transfer_id, &peer_id) {
        let _ = transfer.cancel(&transfer_id).await;
        return Err(error.to_string());
    }
    let snapshot = transfer.snapshot().await;
    if system_share
        .observe_transfers(&snapshot)
        .map_err(|error| error.to_string())?
    {
        crate::system_share::announce_pending(&app, &system_share);
    }
    Ok(transfer_id)
}

#[arcrelay_desktop_ipc::command]
pub async fn respond_transfer(
    state: State<'_, DesktopState>,
    transfer_id: String,
    accepted: bool,
    automatic_receive: bool,
) -> Result<(), String> {
    state
        .transfer()
        .await?
        .respond_incoming(&transfer_id, accepted, automatic_receive)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn pause_transfer(
    state: State<'_, DesktopState>,
    transfer_id: String,
) -> Result<(), String> {
    state
        .transfer()
        .await?
        .pause(&transfer_id)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn resume_transfer(
    state: State<'_, DesktopState>,
    transfer_id: String,
) -> Result<(), String> {
    state
        .transfer()
        .await?
        .resume(&transfer_id)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn cancel_transfer(
    state: State<'_, DesktopState>,
    transfer_id: String,
) -> Result<(), String> {
    state
        .transfer()
        .await?
        .cancel(&transfer_id)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn choose_transfer_receive_directory(
    state: State<'_, DesktopState>,
) -> Result<arcrelay_transfer::TransferSnapshot, String> {
    let Some(directory) = rfd::AsyncFileDialog::new()
        .set_title("选择 ArcRelay 接收目录")
        .pick_folder()
        .await
    else {
        return Ok(state.transfer().await?.snapshot().await);
    };
    state
        .transfer()
        .await?
        .set_receive_directory(directory.path().to_path_buf())
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn open_transfer_receive_directory(state: State<'_, DesktopState>) -> Result<(), String> {
    let path = std::path::PathBuf::from(state.transfer().await?.snapshot().await.receive_directory);
    if !path.is_dir() {
        return Err(format!(
            "receive directory does not exist: {}",
            path.display()
        ));
    }

    #[cfg(target_os = "macos")]
    let mut command = tokio::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = tokio::process::Command::new("explorer.exe");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = tokio::process::Command::new("xdg-open");

    #[cfg(target_os = "windows")]
    {
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
        .arg(&path)
        .spawn()
        .map_err(|error| format!("failed to open receive directory: {error}"))?;
    Ok(())
}

#[arcrelay_desktop_ipc::command]
pub async fn set_transfer_receive_policy(
    state: State<'_, DesktopState>,
    peer_id: String,
    automatic: bool,
) -> Result<arcrelay_transfer::TransferSnapshot, String> {
    state
        .transfer()
        .await?
        .set_receive_policy(&peer_id, automatic)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/src_commands_transfer_ipc.rs"));
