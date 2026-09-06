use super::*;
use crate::application::remote_file_service as service;
pub use service::{
    RemoteDeleteResult, RemoteFileDragPreparation, RemoteFileOpenResult, RemoteFileState,
    RemoteFileTransferSession,
};

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn list_remote_file_transfers(
    app: AppHandle,
) -> Result<Vec<RemoteFileTransferSession>, String> {
    service::list_remote_file_transfers(app).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn get_remote_file_state(
    state: State<'_, DesktopState>,
) -> Result<RemoteFileState, String> {
    service::get_remote_file_state(state.inner()).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn get_local_file_shares(
    state: State<'_, DesktopState>,
) -> Result<Vec<LocalSharedDirectory>, String> {
    service::get_local_file_shares(state.inner()).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn list_remote_file_shares(
    state: State<'_, DesktopState>,
    peer_id: String,
) -> Result<Vec<RemoteFileShare>, arcrelay_protocol::remote_files::RemoteFileError> {
    service::list_remote_file_shares(state.inner(), peer_id).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn list_remote_directory(
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    cursor: Option<String>,
    search: Option<String>,
    sort_key: RemoteFileSortKey,
    sort_direction: RemoteFileSortDirection,
) -> Result<RemoteFileDirectoryPage, arcrelay_protocol::remote_files::RemoteFileError> {
    service::list_remote_directory(
        state.inner(),
        peer_id,
        share_id,
        relative_path,
        cursor,
        search,
        sort_key,
        sort_direction,
    )
    .await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn create_remote_directory(
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    name: String,
) -> Result<RemoteFileEntry, arcrelay_protocol::remote_files::RemoteFileError> {
    service::create_remote_directory(state.inner(), peer_id, share_id, relative_path, name).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn rename_remote_entry(
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    new_name: String,
) -> Result<RemoteFileEntry, arcrelay_protocol::remote_files::RemoteFileError> {
    service::rename_remote_entry(state.inner(), peer_id, share_id, relative_path, new_name).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn delete_remote_entry(
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_path: String,
) -> Result<(), arcrelay_protocol::remote_files::RemoteFileError> {
    service::delete_remote_entry(state.inner(), peer_id, share_id, relative_path).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn pick_remote_upload(
    app: AppHandle,
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    folder: bool,
) -> Result<usize, String> {
    service::pick_remote_upload(app, state.inner(), peer_id, share_id, relative_path, folder).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn upload_remote_paths(
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    paths: Vec<String>,
) -> Result<usize, String> {
    service::upload_remote_paths(state.inner(), peer_id, share_id, relative_path, paths).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_remote_upload(
    app: AppHandle,
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    folder: bool,
) -> Result<Option<RemoteFileTransferSession>, String> {
    service::start_remote_upload(app, state.inner(), peer_id, share_id, relative_path, folder).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_remote_upload_paths(
    app: AppHandle,
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    paths: Vec<String>,
) -> Result<RemoteFileTransferSession, String> {
    service::start_remote_upload_paths(app, state.inner(), peer_id, share_id, relative_path, paths)
        .await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_remote_download_entries(
    app: AppHandle,
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_paths: Vec<String>,
) -> Result<Option<Vec<RemoteFileTransferSession>>, String> {
    service::start_remote_download_entries(app, state.inner(), peer_id, share_id, relative_paths)
        .await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn download_remote_entry(
    app: AppHandle,
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_path: String,
) -> Result<Option<String>, String> {
    service::download_remote_entry(app, state.inner(), peer_id, share_id, relative_path).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn download_remote_entries(
    app: AppHandle,
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_paths: Vec<String>,
) -> Result<Option<Vec<String>>, String> {
    service::download_remote_entries(app, state.inner(), peer_id, share_id, relative_paths).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn prepare_remote_drag(
    app: AppHandle,
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_path: String,
) -> Result<RemoteFileDragPreparation, String> {
    service::prepare_remote_drag(app, state.inner(), peer_id, share_id, relative_path).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_remote_file_promise_drag(
    app: AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    kind: RemoteFileKind,
    size: u64,
) -> Result<(), String> {
    service::start_remote_file_promise_drag(
        app,
        window,
        state.inner(),
        peer_id,
        share_id,
        relative_path,
        kind,
        size,
    )
    .await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn get_remote_file_thumbnail(
    app: AppHandle,
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    modified_at_ms: i64,
) -> Result<Option<String>, String> {
    service::get_remote_file_thumbnail(
        app,
        state.inner(),
        peer_id,
        share_id,
        relative_path,
        modified_at_ms,
    )
    .await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn open_remote_entry(
    app: AppHandle,
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_path: String,
) -> Result<RemoteFileOpenResult, String> {
    service::open_remote_entry(app, state.inner(), peer_id, share_id, relative_path).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn add_remote_file_share(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<Option<LocalSharedDirectory>, arcrelay_files::FileError> {
    service::add_remote_file_share(app, state.inner()).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn remove_remote_file_share(
    state: State<'_, DesktopState>,
    share_id: String,
) -> Result<Vec<LocalSharedDirectory>, arcrelay_files::FileError> {
    service::remove_remote_file_share(state.inner(), share_id).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn set_remote_file_share_writable(
    state: State<'_, DesktopState>,
    share_id: String,
    writable: bool,
) -> Result<LocalSharedDirectory, arcrelay_files::FileError> {
    service::set_remote_file_share_writable(state.inner(), share_id, writable).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn set_remote_file_share_web_policy(
    state: State<'_, DesktopState>,
    share_id: String,
    policy: crate::remote_files::WebSharePolicyUpdate,
    new_password: Option<String>,
) -> Result<LocalSharedDirectory, arcrelay_files::FileError> {
    service::set_remote_file_share_web_policy(state.inner(), share_id, policy, new_password).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn get_web_gateway_status(
    state: State<'_, DesktopState>,
) -> Result<arcrelay_web_gateway::WebGatewayStatus, String> {
    service::get_web_gateway_status(state.inner()).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn revoke_web_sessions(
    state: State<'_, DesktopState>,
    share_id: Option<String>,
) -> Result<usize, String> {
    service::revoke_web_sessions(state.inner(), share_id).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn web_gateway_qr_code(
    state: State<'_, DesktopState>,
    url: String,
) -> Result<String, String> {
    service::web_gateway_qr_code(state.inner(), url).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn open_web_gateway_url(
    state: State<'_, DesktopState>,
    url: String,
) -> Result<(), String> {
    service::open_web_gateway_url(state.inner(), url).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn delete_remote_entries(
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    paths: Vec<String>,
) -> Result<Vec<RemoteDeleteResult>, String> {
    service::delete_remote_entries(state.inner(), peer_id, share_id, paths).await
}

#[arcrelay_desktop_ipc::command]
#[allow(clippy::too_many_arguments)]
pub async fn stop_remote_edit(
    state: State<'_, DesktopState>,
    peer_id: String,
    share_id: String,
    relative_path: String,
) -> Result<(), String> {
    service::stop_remote_edit(state.inner(), peer_id, share_id, relative_path).await
}

#[cfg(test)]
include!(concat!(
    env!("OUT_DIR"),
    "/src_commands_remote_files_ipc.rs"
));
