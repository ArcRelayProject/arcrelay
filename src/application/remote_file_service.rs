use crate::backend::DesktopState;
use crate::remote_files::LocalSharedDirectory;
use arcrelay_files::FileError;
use arcrelay_protocol::remote_files::{
    RemoteFileDirectoryPage, RemoteFileEntry, RemoteFileKind, RemoteFileRequest, RemoteFileShare,
    RemoteFileSortDirection, RemoteFileSortKey, DEFAULT_REMOTE_DIRECTORY_PAGE_SIZE,
};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use image::ImageEncoder;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

use sha2::{Digest, Sha256};
use std::sync::Arc;

#[cfg(target_os = "macos")]
#[path = "../commands/remote_file_drag_macos.rs"]
mod remote_file_drag_macos;

#[cfg(target_os = "windows")]
#[path = "../commands/remote_file_drag_windows.rs"]
mod remote_file_drag_windows;

#[cfg(target_os = "linux")]
#[path = "../commands/remote_file_drag_linux.rs"]
mod remote_file_drag_linux;

const MAX_EDITABLE_REMOTE_FILE_BYTES: u64 = 8 * 1024 * 1024;
const REMOTE_EDIT_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);
const REMOTE_EDIT_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(1_000);
const REMOTE_FILE_TRANSFER_EVENT: &str = "remote-file-transfer";
const MAX_RETAINED_REMOTE_FILE_TRANSFERS: usize = 48;

#[derive(Debug, Clone)]
struct RemoteOpenSession {
    local_path: PathBuf,
    editable: bool,
}

static REMOTE_OPEN_TEMP_CLEANED: OnceLock<()> = OnceLock::new();
static REMOTE_DRAG_TEMP_CLEANED: OnceLock<()> = OnceLock::new();
pub struct RemoteFileService {
    opening: tokio::sync::Mutex<()>,
    open_sessions: Mutex<HashMap<String, RemoteOpenSession>>,
    transfers: Mutex<HashMap<String, RemoteFileTransferSession>>,
    edit_tasks: Mutex<HashMap<String, tokio::task::AbortHandle>>,
    thumbnail_slots: tokio::sync::Semaphore,
    thumbnail_keys: Mutex<HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>,
}
impl Default for RemoteFileService {
    fn default() -> Self {
        Self {
            opening: tokio::sync::Mutex::new(()),
            open_sessions: Mutex::default(),
            transfers: Mutex::default(),
            edit_tasks: Mutex::default(),
            thumbnail_slots: tokio::sync::Semaphore::new(4),
            thumbnail_keys: Mutex::default(),
        }
    }
}
impl RemoteFileService {
    /// Active file operations prevent tray disconnect from interrupting an upload/download.
    pub fn active_transfer_peer_ids(&self) -> Vec<String> {
        self.transfers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .filter(|session| session.status == "transferring")
            .map(|session| session.peer_id.clone())
            .collect()
    }

    fn thumbnail_key(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut keys = self
            .thumbnail_keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        keys.retain(|_, key| key.strong_count() > 0);
        if let Some(lock) = keys.get(key).and_then(std::sync::Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        keys.insert(key.into(), Arc::downgrade(&lock));
        lock
    }
    pub fn shutdown(&self) {
        for (_, task) in self
            .edit_tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain()
        {
            task.abort();
        }
        self.open_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileTransferSession {
    pub id: String,
    pub direction: String,
    pub status: String,
    pub name: String,
    pub peer_id: String,
    pub share_id: String,
    pub directory_path: String,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub files_transferred: usize,
    pub total_files: usize,
    pub current_name: String,
    pub error: Option<String>,
    pub started_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileDragPreparation {
    pub local_path: String,
    pub icon_path: String,
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileOpenResult {
    pub local_path: String,
    pub editable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RemoteFileEditEvent {
    status: &'static str,
    message: String,
    relative_path: String,
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileState {
    pub devices: Vec<crate::clipboard_sync::RemoteFileDeviceView>,
    pub local_shares: Vec<LocalSharedDirectory>,
}

fn remote_file_transfers(app: &AppHandle) -> &Mutex<HashMap<String, RemoteFileTransferSession>> {
    &app.state::<DesktopState>()
        .inner()
        .remote_file_service
        .transfers
}

fn now_timestamp_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn register_remote_file_transfer(
    app: &AppHandle,
    direction: &str,
    name: String,
    peer_id: String,
    share_id: String,
    directory_path: String,
) -> RemoteFileTransferSession {
    let now = now_timestamp_ms();
    let session = RemoteFileTransferSession {
        id: uuid::Uuid::new_v4().to_string(),
        direction: direction.to_string(),
        status: "transferring".into(),
        name,
        peer_id,
        share_id,
        directory_path,
        bytes_transferred: 0,
        total_bytes: 0,
        files_transferred: 0,
        total_files: 0,
        current_name: String::new(),
        error: None,
        started_at_ms: now,
        updated_at_ms: now,
    };
    let mut sessions = remote_file_transfers(app)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if sessions.len() >= MAX_RETAINED_REMOTE_FILE_TRANSFERS {
        let mut completed = sessions
            .values()
            .filter(|session| session.status != "transferring")
            .map(|session| (session.updated_at_ms, session.id.clone()))
            .collect::<Vec<_>>();
        completed.sort_by_key(|item| item.0);
        if let Some((_, id)) = completed.first() {
            sessions.remove(id);
        }
    }
    sessions.insert(session.id.clone(), session.clone());
    drop(sessions);
    let _ = app.emit(REMOTE_FILE_TRANSFER_EVENT, &session);
    session
}

fn remote_file_progress_callback(
    app: AppHandle,
    session_id: String,
) -> crate::clipboard_sync::RemoteFileProgressCallback {
    Arc::new(move |progress| {
        let now = now_timestamp_ms();
        let mut sessions = remote_file_transfers(&app)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(session) = sessions.get_mut(&session_id) else {
            return;
        };
        let should_emit = now.saturating_sub(session.updated_at_ms) >= 100
            || progress.bytes_transferred == progress.total_bytes;
        session.bytes_transferred = progress.bytes_transferred;
        session.total_bytes = progress.total_bytes;
        session.files_transferred = progress.files_transferred;
        session.total_files = progress.total_files;
        session.current_name = progress.current_name;
        if should_emit {
            session.updated_at_ms = now;
            let view = session.clone();
            drop(sessions);
            let _ = app.emit(REMOTE_FILE_TRANSFER_EVENT, view);
        }
    })
}

fn finish_remote_file_transfer(app: &AppHandle, session_id: &str, result: Result<(), String>) {
    let mut sessions = remote_file_transfers(app)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(session) = sessions.get_mut(session_id) else {
        return;
    };
    match result {
        Ok(()) => {
            session.status = "completed".into();
            session.bytes_transferred = session.total_bytes;
            session.files_transferred = session.total_files;
            session.error = None;
        }
        Err(error) => {
            session.status = "failed".into();
            session.error = Some(error);
        }
    }
    session.updated_at_ms = now_timestamp_ms();
    let view = session.clone();
    drop(sessions);
    let _ = app.emit(REMOTE_FILE_TRANSFER_EVENT, &view);
    show_remote_file_system_notification(app, &view);
}

fn show_remote_file_system_notification(app: &AppHandle, session: &RemoteFileTransferSession) {
    let state = app.state::<DesktopState>();
    let language = state.settings.snapshot().language;
    let completed = session.status == "completed";
    let uploading = session.direction == "upload";
    let category = if completed {
        crate::desktop_notification::DesktopNotificationCategory::RemoteFileCompleted
    } else {
        crate::desktop_notification::DesktopNotificationCategory::RemoteFileFailed
    };
    let title = crate::desktop_notification::localized(
        language,
        &format!(
            "remote file {} {}",
            if uploading { "upload" } else { "download" },
            if completed { "completed" } else { "failed" }
        ),
        &format!(
            "Remote file {} {}",
            if uploading { "upload" } else { "download" },
            if completed { "completed" } else { "failed" }
        ),
    );
    let body = if completed {
        session.name.clone()
    } else {
        format!(
            "{}: {}",
            session.name,
            session.error.as_deref().unwrap_or_else(|| {
                if language.uses_english_fallback() {
                    "Unknown error"
                } else {
                    "unknown error"
                }
            })
        )
    };
    let notification = crate::desktop_notification::DesktopNotification::new(category, title, body);
    if let Err(error) = crate::desktop_notification::show(app, &state.settings, notification) {
        tracing::warn!(%error, "failed to show remote file notification");
    }
}

pub async fn list_remote_file_transfers(
    app: AppHandle,
) -> Result<Vec<RemoteFileTransferSession>, String> {
    let mut sessions = remote_file_transfers(&app)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .values()
        .cloned()
        .collect::<Vec<_>>();
    sessions.sort_by_key(|session| std::cmp::Reverse(session.started_at_ms));
    Ok(sessions)
}

pub async fn get_remote_file_state(state: &DesktopState) -> Result<RemoteFileState, String> {
    Ok(RemoteFileState {
        devices: state.clipboard_sync.remote_file_devices().await,
        local_shares: state.remote_files.local_shares(),
    })
}

pub async fn get_local_file_shares(
    state: &DesktopState,
) -> Result<Vec<LocalSharedDirectory>, String> {
    Ok(state.remote_files.local_shares())
}

pub async fn list_remote_file_shares(
    state: &DesktopState,
    peer_id: String,
) -> arcrelay_protocol::remote_files::RemoteFileResult<Vec<RemoteFileShare>> {
    Ok(state
        .clipboard_sync
        .remote_file_request(&peer_id, RemoteFileRequest::ListShares)
        .await?
        .shares)
}

#[allow(clippy::too_many_arguments)]
pub async fn list_remote_directory(
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_path: String,
    cursor: Option<String>,
    search: Option<String>,
    sort_key: RemoteFileSortKey,
    sort_direction: RemoteFileSortDirection,
) -> arcrelay_protocol::remote_files::RemoteFileResult<RemoteFileDirectoryPage> {
    let response = state
        .clipboard_sync
        .remote_file_request(
            &peer_id,
            RemoteFileRequest::ListDirectory {
                share_id,
                relative_path,
                cursor,
                limit: DEFAULT_REMOTE_DIRECTORY_PAGE_SIZE,
                search,
                sort_key,
                sort_direction,
            },
        )
        .await?;
    Ok(RemoteFileDirectoryPage {
        entries: response.entries,
        next_cursor: response.next_cursor,
    })
}

pub async fn create_remote_directory(
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_path: String,
    name: String,
) -> arcrelay_protocol::remote_files::RemoteFileResult<RemoteFileEntry> {
    state
        .clipboard_sync
        .remote_file_request(
            &peer_id,
            RemoteFileRequest::CreateDirectory {
                share_id,
                relative_path,
                name,
            },
        )
        .await?
        .entry
        .ok_or_else(|| {
            arcrelay_protocol::remote_files::RemoteFileError::new(
                arcrelay_protocol::remote_files::RemoteFileErrorCode::Internal,
                "remote peer returned no new folder metadata",
            )
        })
}

pub async fn rename_remote_entry(
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_path: String,
    new_name: String,
) -> arcrelay_protocol::remote_files::RemoteFileResult<RemoteFileEntry> {
    state
        .clipboard_sync
        .remote_file_request(
            &peer_id,
            RemoteFileRequest::Rename {
                share_id,
                relative_path,
                new_name,
            },
        )
        .await?
        .entry
        .ok_or_else(|| {
            arcrelay_protocol::remote_files::RemoteFileError::new(
                arcrelay_protocol::remote_files::RemoteFileErrorCode::Internal,
                "remote peer returned no rename result",
            )
        })
}

pub async fn delete_remote_entry(
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_path: String,
) -> arcrelay_protocol::remote_files::RemoteFileResult<()> {
    state
        .clipboard_sync
        .remote_file_request(
            &peer_id,
            RemoteFileRequest::Delete {
                share_id,
                relative_path,
            },
        )
        .await?;
    Ok(())
}

pub async fn pick_remote_upload(
    app: AppHandle,
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_path: String,
    folder: bool,
) -> Result<usize, String> {
    let paths: Vec<std::path::PathBuf> = if folder {
        app.dialog()
            .file()
            .set_title("选择要上传的文件夹")
            .blocking_pick_folder()
            .into_iter()
            .map(|folder| folder.into_path().map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        app.dialog()
            .file()
            .set_title("选择要上传的文件")
            .blocking_pick_files()
            .unwrap_or_default()
            .into_iter()
            .map(|file| file.into_path().map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()?
    };
    if paths.is_empty() {
        return Ok(0);
    }
    state
        .clipboard_sync
        .upload_remote_files(&peer_id, share_id, relative_path, paths)
        .await
}

pub async fn upload_remote_paths(
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_path: String,
    paths: Vec<String>,
) -> Result<usize, String> {
    state
        .clipboard_sync
        .upload_remote_files(
            &peer_id,
            share_id,
            relative_path,
            paths.into_iter().map(std::path::PathBuf::from).collect(),
        )
        .await
}

fn launch_remote_upload(
    app: AppHandle,
    manager: Arc<crate::clipboard_sync::ClipboardSyncManager>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    paths: Vec<PathBuf>,
) -> RemoteFileTransferSession {
    let name = if paths.len() == 1 {
        paths[0]
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("upload item")
            .to_string()
    } else {
        format!("upload {} items", paths.len())
    };
    let session = register_remote_file_transfer(
        &app,
        "upload",
        name,
        peer_id.clone(),
        share_id.clone(),
        relative_path.clone(),
    );
    let session_id = session.id.clone();
    let progress = remote_file_progress_callback(app.clone(), session_id.clone());
    tauri::async_runtime::spawn(async move {
        let result = manager
            .upload_remote_files_with_progress(
                &peer_id,
                share_id,
                relative_path,
                paths,
                Some(progress),
            )
            .await
            .map(|_| ());
        finish_remote_file_transfer(&app, &session_id, result);
    });
    session
}

pub async fn start_remote_upload(
    app: AppHandle,
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_path: String,
    folder: bool,
) -> Result<Option<RemoteFileTransferSession>, String> {
    let paths = if folder {
        rfd::AsyncFileDialog::new()
            .set_title("选择要上传的文件夹")
            .pick_folder()
            .await
            .into_iter()
            .map(|handle| handle.path().to_path_buf())
            .collect::<Vec<_>>()
    } else {
        rfd::AsyncFileDialog::new()
            .set_title("选择要上传的文件")
            .pick_files()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|handle| handle.path().to_path_buf())
            .collect::<Vec<_>>()
    };
    if paths.is_empty() {
        return Ok(None);
    }
    Ok(Some(launch_remote_upload(
        app,
        state.clipboard_sync.clone(),
        peer_id,
        share_id,
        relative_path,
        paths,
    )))
}

pub async fn start_remote_upload_paths(
    app: AppHandle,
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_path: String,
    paths: Vec<String>,
) -> Result<RemoteFileTransferSession, String> {
    let paths = paths.into_iter().map(PathBuf::from).collect::<Vec<_>>();
    if paths.is_empty() {
        return Err("no files selected for upload".into());
    }
    Ok(launch_remote_upload(
        app,
        state.clipboard_sync.clone(),
        peer_id,
        share_id,
        relative_path,
        paths,
    ))
}

pub async fn start_remote_download_entries(
    app: AppHandle,
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_paths: Vec<String>,
) -> Result<Option<Vec<RemoteFileTransferSession>>, String> {
    if relative_paths.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let Some(destination) = rfd::AsyncFileDialog::new()
        .set_title("选择下载位置")
        .pick_folder()
        .await
    else {
        return Ok(None);
    };
    let destination = destination.path().to_path_buf();
    let manager = state.clipboard_sync.clone();
    let mut sessions = Vec::with_capacity(relative_paths.len());
    for relative_path in relative_paths {
        let name = relative_path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("download item")
            .to_string();
        let directory_path = relative_path
            .split('/')
            .collect::<Vec<_>>()
            .split_last()
            .map(|(_, parent)| parent.join("/"))
            .unwrap_or_default();
        let session = register_remote_file_transfer(
            &app,
            "download",
            name,
            peer_id.clone(),
            share_id.clone(),
            directory_path,
        );
        let session_id = session.id.clone();
        let app_task = app.clone();
        let manager_task = manager.clone();
        let peer_task = peer_id.clone();
        let share_task = share_id.clone();
        let destination_task = destination.clone();
        let progress = remote_file_progress_callback(app.clone(), session_id.clone());
        tauri::async_runtime::spawn(async move {
            let result = manager_task
                .download_remote_file_with_progress(
                    &peer_task,
                    share_task,
                    relative_path,
                    destination_task,
                    Some(progress),
                )
                .await
                .map(|_| ());
            finish_remote_file_transfer(&app_task, &session_id, result);
        });
        sessions.push(session);
    }
    Ok(Some(sessions))
}

pub async fn download_remote_entry(
    app: AppHandle,
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_path: String,
) -> Result<Option<String>, String> {
    let Some(destination) = app
        .dialog()
        .file()
        .set_title("选择下载位置")
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    let destination = destination.into_path().map_err(|error| error.to_string())?;
    let path = state
        .clipboard_sync
        .download_remote_file(&peer_id, share_id, relative_path, destination)
        .await?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

pub async fn download_remote_entries(
    app: AppHandle,
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_paths: Vec<String>,
) -> Result<Option<Vec<String>>, String> {
    if relative_paths.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let Some(destination) = app
        .dialog()
        .file()
        .set_title("选择下载位置")
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    let destination = destination.into_path().map_err(|error| error.to_string())?;
    let mut paths = Vec::with_capacity(relative_paths.len());
    for relative_path in relative_paths {
        let path = state
            .clipboard_sync
            .download_remote_file(
                &peer_id,
                share_id.clone(),
                relative_path,
                destination.clone(),
            )
            .await?;
        paths.push(path.to_string_lossy().into_owned());
    }
    Ok(Some(paths))
}

pub async fn prepare_remote_drag(
    app: AppHandle,
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_path: String,
) -> Result<RemoteFileDragPreparation, String> {
    clean_stale_remote_drag_files_once();
    let destination = remote_drag_base_directory().join(uuid::Uuid::new_v4().to_string());
    let name = relative_path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("remote file")
        .to_string();
    let directory_path = relative_path
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default();
    let session = register_remote_file_transfer(
        &app,
        "download",
        name,
        peer_id.clone(),
        share_id.clone(),
        directory_path,
    );
    let progress = remote_file_progress_callback(app.clone(), session.id.clone());
    let result = state
        .clipboard_sync
        .download_remote_file_with_progress(
            &peer_id,
            share_id,
            relative_path,
            destination,
            Some(progress),
        )
        .await;
    let path = match result {
        Ok(path) => {
            finish_remote_file_transfer(&app, &session.id, Ok(()));
            path
        }
        Err(error) => {
            finish_remote_file_transfer(&app, &session.id, Err(error.clone()));
            return Err(error);
        }
    };
    let icon_path = create_remote_drag_preview(path.clone()).await?;
    Ok(RemoteFileDragPreparation {
        local_path: path.to_string_lossy().into_owned(),
        icon_path: icon_path.to_string_lossy().into_owned(),
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn start_remote_file_promise_drag(
    app: AppHandle,
    window: tauri::WebviewWindow,
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_path: String,
    kind: RemoteFileKind,
    size: u64,
) -> Result<(), String> {
    #[cfg(not(target_os = "windows"))]
    let _ = size;

    #[cfg(target_os = "macos")]
    {
        clean_stale_remote_drag_files_once();
        let icon_path = create_remote_drag_promise_preview(kind).await?;
        remote_file_drag_macos::start_remote_file_promise_drag(
            app,
            window,
            state.clipboard_sync.clone(),
            peer_id,
            share_id,
            relative_path,
            kind,
            icon_path,
        )
        .await
    }

    #[cfg(target_os = "windows")]
    {
        remote_file_drag_windows::start_remote_file_promise_drag(
            app,
            window,
            state.clipboard_sync.clone(),
            peer_id,
            share_id,
            relative_path,
            kind,
            size,
        )
        .await
    }

    #[cfg(target_os = "linux")]
    {
        clean_stale_remote_drag_files_once();
        let icon_path = create_remote_drag_promise_preview(kind).await?;
        remote_file_drag_linux::start_remote_file_promise_drag(
            app,
            window,
            state.clipboard_sync.clone(),
            peer_id,
            share_id,
            relative_path,
            icon_path,
        )
        .await
    }
}

pub async fn get_remote_file_thumbnail(
    app: AppHandle,
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_path: String,
    modified_at_ms: i64,
) -> Result<Option<String>, String> {
    let key = format!("{peer_id}\0{share_id}\0{relative_path}\0{modified_at_ms}");
    let key_lock = state.remote_file_service.thumbnail_key(&key);
    let _key_guard = key_lock.lock().await;
    let _slot = state
        .remote_file_service
        .thumbnail_slots
        .acquire()
        .await
        .map_err(|e| e.to_string())?;
    let digest = Sha256::digest(key.as_bytes());
    let file_name = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let directory = app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?
        .join("remote-file-thumbnails");
    let destination = directory.join(format!("{file_name}.png"));
    if destination.is_file() {
        return Ok(Some(destination.to_string_lossy().into_owned()));
    }
    let mut attempt = 0;
    let (bytes, media_type) = loop {
        match state
            .clipboard_sync
            .remote_file_thumbnail(&peer_id, share_id.clone(), relative_path.clone(), 320)
            .await
        {
            Ok(result) => break result,
            Err(error) if attempt < 2 => {
                attempt += 1;
                tracing::debug!(%error, attempt, "retry remote thumbnail");
                tokio::time::sleep(std::time::Duration::from_millis(300 * attempt)).await;
            }
            Err(error) => return Err(error),
        }
    };
    if media_type != "image/png" {
        return Ok(None);
    }
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| error.to_string())?;
    let cache_path = destination.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::infrastructure::durable_file::replace(&cache_path, &bytes)
            .map_err(|e| e.to_string())?;
        prune_thumbnail_cache(&cache_path);
        Ok::<_, String>(())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(Some(destination.to_string_lossy().into_owned()))
}

async fn create_remote_drag_preview(source: PathBuf) -> Result<PathBuf, String> {
    let directory = remote_drag_preview_directory();
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| error.to_string())?;
    let destination = directory.join(format!("{}.png", uuid::Uuid::new_v4()));
    let output = destination.clone();
    tokio::task::spawn_blocking(move || build_remote_drag_preview(&source, &output))
        .await
        .map_err(|error| error.to_string())??;
    Ok(destination)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn create_remote_drag_promise_preview(kind: RemoteFileKind) -> Result<PathBuf, String> {
    let directory = remote_drag_preview_directory();
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| error.to_string())?;
    let destination = directory.join(format!("promise-{}.png", uuid::Uuid::new_v4()));
    let output = destination.clone();
    tokio::task::spawn_blocking(move || build_remote_drag_promise_preview(kind, &output))
        .await
        .map_err(|error| error.to_string())??;
    Ok(destination)
}

fn remote_drag_base_directory() -> PathBuf {
    std::env::temp_dir().join("ArcRelay").join("remote-drag")
}

fn remote_drag_preview_directory() -> PathBuf {
    std::env::temp_dir()
        .join("ArcRelay")
        .join("remote-drag-previews")
}

fn clean_stale_remote_drag_files_once() {
    REMOTE_DRAG_TEMP_CLEANED.get_or_init(|| {
        let _ = std::fs::remove_dir_all(remote_drag_base_directory());
        let _ = std::fs::remove_dir_all(remote_drag_preview_directory());
    });
}

fn build_remote_drag_preview(source: &Path, destination: &Path) -> Result<(), String> {
    let mut canvas = image::RgbaImage::from_pixel(160, 160, image::Rgba([0, 0, 0, 0]));
    if source.is_file() {
        if let Ok(image) = image::open(source) {
            let thumbnail = image.thumbnail(144, 144).to_rgba8();
            let x = (160_u32.saturating_sub(thumbnail.width()) / 2) as i64;
            let y = (160_u32.saturating_sub(thumbnail.height()) / 2) as i64;
            image::imageops::overlay(&mut canvas, &thumbnail, x, y);
        } else {
            draw_file_preview(&mut canvas);
        }
    } else {
        draw_folder_preview(&mut canvas);
    }
    let file = std::fs::File::create(destination).map_err(|error| error.to_string())?;
    image::codecs::png::PngEncoder::new(file)
        .write_image(
            canvas.as_raw(),
            canvas.width(),
            canvas.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| error.to_string())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn build_remote_drag_promise_preview(
    kind: RemoteFileKind,
    destination: &Path,
) -> Result<(), String> {
    let mut canvas = image::RgbaImage::from_pixel(160, 160, image::Rgba([0, 0, 0, 0]));
    match kind {
        RemoteFileKind::File => draw_file_preview(&mut canvas),
        RemoteFileKind::Folder => draw_folder_preview(&mut canvas),
    }
    let file = std::fs::File::create(destination).map_err(|error| error.to_string())?;
    image::codecs::png::PngEncoder::new(file)
        .write_image(
            canvas.as_raw(),
            canvas.width(),
            canvas.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| error.to_string())
}

fn fill_preview_rect(
    image: &mut image::RgbaImage,
    x: std::ops::Range<u32>,
    y: std::ops::Range<u32>,
    color: image::Rgba<u8>,
) {
    for row in y {
        for column in x.clone() {
            image.put_pixel(column, row, color);
        }
    }
}

fn draw_folder_preview(image: &mut image::RgbaImage) {
    fill_preview_rect(image, 25..84, 38..61, image::Rgba([246, 184, 40, 255]));
    fill_preview_rect(image, 18..142, 53..130, image::Rgba([255, 199, 55, 255]));
    fill_preview_rect(image, 18..142, 61..70, image::Rgba([255, 215, 91, 255]));
}

fn draw_file_preview(image: &mut image::RgbaImage) {
    fill_preview_rect(image, 36..124, 18..142, image::Rgba([247, 249, 253, 255]));
    fill_preview_rect(image, 112..124, 18..42, image::Rgba([218, 224, 235, 255]));
    fill_preview_rect(image, 51..109, 73..80, image::Rgba([103, 119, 151, 210]));
    fill_preview_rect(image, 51..109, 91..98, image::Rgba([103, 119, 151, 170]));
    fill_preview_rect(image, 51..92, 109..116, image::Rgba([103, 119, 151, 130]));
}

pub async fn open_remote_entry(
    app: AppHandle,
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_path: String,
) -> Result<RemoteFileOpenResult, String> {
    let _opening = state.remote_file_service.opening.lock().await;
    let session_key = remote_open_session_key(&peer_id, &share_id, &relative_path);
    let existing_session = {
        let sessions = remote_open_sessions(&app)
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sessions
            .get(&session_key)
            .cloned()
            .filter(|session| session.local_path.is_file())
    };
    if let Some(session) = existing_session {
        open_with_default_application(&session.local_path).await?;
        return Ok(RemoteFileOpenResult {
            local_path: session.local_path.to_string_lossy().into_owned(),
            editable: session.editable,
        });
    }

    if state
        .remote_file_service
        .open_sessions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .len()
        >= 32
    {
        return Err(
            "at most 32 remotely edited files can be monitored; stop some synchronizations first"
                .into(),
        );
    }
    clean_stale_remote_open_files_once();
    let share = state
        .clipboard_sync
        .remote_file_request(&peer_id, RemoteFileRequest::ListShares)
        .await
        .map_err(|error| error.to_string())?
        .shares
        .into_iter()
        .find(|share| share.id == share_id)
        .ok_or_else(|| "remote shared directory not found".to_string())?;
    let entry =
        fetch_remote_entry(&state.clipboard_sync, &peer_id, &share_id, &relative_path).await?;
    if entry.kind != arcrelay_protocol::remote_files::RemoteFileKind::File {
        return Err("only files can be opened with a system application".into());
    }

    let destination = remote_open_base_directory().join(uuid::Uuid::new_v4().to_string());
    let transfer = register_remote_file_transfer(
        &app,
        "download",
        entry.name.clone(),
        peer_id.clone(),
        share_id.clone(),
        remote_parent_path(&relative_path)?,
    );
    let progress = remote_file_progress_callback(app.clone(), transfer.id.clone());
    let download = state
        .clipboard_sync
        .download_remote_file_with_progress(
            &peer_id,
            share_id.clone(),
            relative_path.clone(),
            destination,
            Some(progress),
        )
        .await;
    let local_path = match download {
        Ok(path) => {
            finish_remote_file_transfer(&app, &transfer.id, Ok(()));
            path
        }
        Err(error) => {
            finish_remote_file_transfer(&app, &transfer.id, Err(error.clone()));
            return Err(error);
        }
    };
    let editable = share.writable && is_editable_text_file(&local_path, entry.size).await;
    let initial_hash = if editable {
        Some(hash_file(&local_path).await?)
    } else {
        None
    };
    open_with_default_application(&local_path).await?;

    if let Some(initial_hash) = initial_hash {
        remote_open_sessions(&app)
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                session_key.clone(),
                RemoteOpenSession {
                    local_path: local_path.clone(),
                    editable,
                },
            );
        let manager = state.clipboard_sync.clone();
        let app = app.clone();
        let local_path_for_task = local_path.clone();
        let remote_name = entry.name.clone();
        let service = state.remote_file_service.clone();
        let task_key = session_key.clone();
        let (watcher_ready, watcher_start) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            if watcher_start.await.is_err() {
                return;
            }
            monitor_remote_text_edit(
                app,
                manager,
                peer_id,
                share_id,
                relative_path,
                remote_name,
                local_path_for_task,
                entry.modified_at_ms,
                initial_hash,
            )
            .await;
            service
                .open_sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&task_key);
            service
                .edit_tasks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&task_key);
        });
        state
            .remote_file_service
            .edit_tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session_key, task.abort_handle());
        let _ = watcher_ready.send(());
    }

    Ok(RemoteFileOpenResult {
        local_path: local_path.to_string_lossy().into_owned(),
        editable,
    })
}

fn remote_open_sessions(app: &AppHandle) -> &Mutex<HashMap<String, RemoteOpenSession>> {
    &app.state::<DesktopState>()
        .inner()
        .remote_file_service
        .open_sessions
}

fn remote_open_session_key(peer_id: &str, share_id: &str, relative_path: &str) -> String {
    format!("{peer_id}\u{0}{share_id}\u{0}{relative_path}")
}

fn remote_open_base_directory() -> PathBuf {
    std::env::temp_dir().join("ArcRelay").join("remote-open")
}

fn clean_stale_remote_open_files_once() {
    REMOTE_OPEN_TEMP_CLEANED.get_or_init(|| {
        let _ = std::fs::create_dir_all(remote_open_base_directory());
    });
}

fn remote_parent_path(relative_path: &str) -> Result<String, String> {
    let path = relative_path.trim_matches('/');
    if path.is_empty() {
        return Err("invalid remote file path".into());
    }
    Ok(path
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or_default()
        .to_string())
}

async fn fetch_remote_entry(
    manager: &crate::clipboard_sync::ClipboardSyncManager,
    peer_id: &str,
    share_id: &str,
    relative_path: &str,
) -> Result<RemoteFileEntry, String> {
    let parent = remote_parent_path(relative_path)?;
    manager
        .list_remote_directory_all(peer_id, share_id, &parent)
        .await?
        .into_iter()
        .find(|entry| entry.relative_path == relative_path)
        .ok_or_else(|| "remote file does not exist or was moved".to_string())
}

async fn is_editable_text_file(path: &Path, size: u64) -> bool {
    if size > MAX_EDITABLE_REMOTE_FILE_BYTES {
        return false;
    }
    const TEXT_EXTENSIONS: &[&str] = &[
        "txt", "md", "markdown", "csv", "tsv", "json", "jsonc", "xml", "yaml", "yml", "toml",
        "ini", "conf", "config", "log", "html", "htm", "css", "scss", "less", "js", "jsx", "mjs",
        "cjs", "ts", "tsx", "rs", "py", "rb", "php", "java", "kt", "swift", "go", "dart", "c", "h",
        "cc", "cpp", "hpp", "sh", "bash", "zsh", "fish", "sql", "graphql", "proto",
    ];
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };
    if !TEXT_EXTENSIONS
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
    {
        return false;
    }
    tokio::fs::read(path).await.ok().is_some_and(|bytes| {
        std::str::from_utf8(&bytes).is_ok()
            || bytes.starts_with(&[0xff, 0xfe])
            || bytes.starts_with(&[0xfe, 0xff])
    })
}

async fn hash_file(path: &Path) -> Result<[u8; 32], String> {
    let metadata = tokio::fs::metadata(path).await.map_err(|e| e.to_string())?;
    if metadata.len() > MAX_EDITABLE_REMOTE_FILE_BYTES || !metadata.is_file() {
        return Err("edited copy is outside the synchronization scope".into());
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| format!("failed to read local edited copy: {error}"))?;
    Ok(Sha256::digest(bytes).into())
}

#[allow(clippy::too_many_arguments)]
async fn monitor_remote_text_edit(
    app: AppHandle,
    manager: Arc<crate::clipboard_sync::ClipboardSyncManager>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    remote_name: String,
    local_path: PathBuf,
    mut expected_modified_at_ms: i64,
    initial_hash: [u8; 32],
) {
    let mut observed_hash = initial_hash;
    let mut uploaded_hash = initial_hash;
    let mut changed_at: Option<tokio::time::Instant> = None;
    let mut reported_error = String::new();

    use notify::Watcher;
    let (changes, mut changed) = tokio::sync::mpsc::channel(1);
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event
            .as_ref()
            .is_ok_and(|e| !matches!(e.kind, notify::EventKind::Access(_)))
            || event.is_err()
        {
            let _ = changes.try_send(());
        }
    })
    .ok();
    if let Some(watcher) = watcher.as_mut() {
        if let Err(error) = watcher.watch(
            local_path.parent().unwrap_or(&local_path),
            notify::RecursiveMode::NonRecursive,
        ) {
            tracing::warn!(%error, "edit watcher unavailable; reconciling metadata");
        }
    }
    let mut metadata = file_signature(&local_path).await;
    let mut missing_since: Option<tokio::time::Instant> = None;
    loop {
        let delay = if changed_at.is_some() {
            REMOTE_EDIT_DEBOUNCE
        } else {
            REMOTE_EDIT_POLL_INTERVAL
        };
        let event = tokio::select! {
            Some(()) = changed.recv() => true,
            _ = tokio::time::sleep(delay) => false,
        };
        let next_metadata = file_signature(&local_path).await;
        if next_metadata.is_none() {
            if missing_since
                .get_or_insert_with(tokio::time::Instant::now)
                .elapsed()
                >= std::time::Duration::from_secs(300)
            {
                emit_remote_edit_event(
                    &app,
                    "stopped",
                    "local edited copy was removed; monitoring stopped".into(),
                    &relative_path,
                );
                return;
            }
            continue;
        } // Editors often replace files by rename.
        missing_since = None;
        if !event && changed_at.is_none() && metadata == next_metadata {
            continue;
        }
        metadata = next_metadata;
        let current_hash = match hash_file(&local_path).await {
            Ok(hash) => hash,
            Err(error) => {
                emit_remote_edit_event(&app, "error", error, &relative_path);
                return;
            }
        };
        if current_hash != observed_hash {
            observed_hash = current_hash;
            changed_at = Some(tokio::time::Instant::now());
            continue;
        }
        if observed_hash == uploaded_hash
            || changed_at.is_none_or(|changed| changed.elapsed() < REMOTE_EDIT_DEBOUNCE)
        {
            continue;
        }

        let current_remote =
            match fetch_remote_entry(&manager, &peer_id, &share_id, &relative_path).await {
                Ok(entry) => entry,
                Err(error) => {
                    emit_remote_edit_event(
                        &app,
                        "conflict",
                        format!(
                            "{error}; local edited copy was retained at {}",
                            local_path.display()
                        ),
                        &relative_path,
                    );
                    return;
                }
            };
        if current_remote.modified_at_ms != expected_modified_at_ms {
            emit_remote_edit_event(
                &app,
                "conflict",
                format!(
                    "remote file was modified elsewhere; local edited copy was retained at {}",
                    local_path.display()
                ),
                &relative_path,
            );
            return;
        }

        let parent = match remote_parent_path(&relative_path) {
            Ok(parent) => parent,
            Err(error) => {
                emit_remote_edit_event(&app, "error", error, &relative_path);
                return;
            }
        };
        let result = manager
            .upload_remote_edit(
                &peer_id,
                share_id.clone(),
                parent,
                local_path.clone(),
                expected_modified_at_ms,
            )
            .await;
        match result {
            Ok(()) => {
                let refreshed =
                    match fetch_remote_entry(&manager, &peer_id, &share_id, &relative_path).await {
                        Ok(entry) => entry,
                        Err(error) => {
                            emit_remote_edit_event(
                                &app,
                                "error",
                                format!(
                                "changes were uploaded, but monitoring cannot continue: {error}"
                            ),
                                &relative_path,
                            );
                            return;
                        }
                    };
                expected_modified_at_ms = refreshed.modified_at_ms;
                uploaded_hash = observed_hash;
                changed_at = None;
                reported_error.clear();
                emit_remote_edit_event(
                    &app,
                    "synced",
                    format!("synchronized changes to `{remote_name}` with the remote device"),
                    &relative_path,
                );
            }
            Err(error)
                if matches!(
                    error.code,
                    arcrelay_protocol::remote_files::RemoteFileErrorCode::Conflict
                        | arcrelay_protocol::remote_files::RemoteFileErrorCode::NotFound
                ) =>
            {
                emit_remote_edit_event(
                    &app,
                    "conflict",
                    format!(
                        "{error}; local edited copy was retained at {}",
                        local_path.display()
                    ),
                    &relative_path,
                );
                return;
            }
            Err(error) => {
                let error_message = error.to_string();
                if error_message != reported_error {
                    emit_remote_edit_event(
                        &app,
                        "error",
                        format!("temporarily unable to upload `{remote_name}`: {error}; retrying automatically"),
                        &relative_path,
                    );
                    reported_error = error_message;
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

fn emit_remote_edit_event(
    app: &AppHandle,
    status: &'static str,
    message: String,
    relative_path: &str,
) {
    let _ = app.emit(
        "remote-file-edit-status",
        RemoteFileEditEvent {
            status,
            message,
            relative_path: relative_path.to_string(),
        },
    );
}

async fn open_with_default_application(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!(
            "local temporary file does not exist: {}",
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
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
        .arg(path)
        .spawn()
        .map_err(|error| format!("failed to open file with the default application: {error}"))?;
    Ok(())
}

pub async fn add_remote_file_share(
    app: AppHandle,
    state: &DesktopState,
) -> Result<Option<LocalSharedDirectory>, FileError> {
    let Some(directory) = app
        .dialog()
        .file()
        .set_title("选择要共享给已配对设备的文件夹")
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    let directory = directory
        .into_path()
        .map_err(|error| FileError::Invalid(error.to_string()))?;
    state.remote_files.add_share(&directory).map(Some)
}

pub async fn remove_remote_file_share(
    state: &DesktopState,
    share_id: String,
) -> Result<Vec<LocalSharedDirectory>, FileError> {
    state.remote_files.remove_share(&share_id)?;
    state.web_gateway.revoke_sessions(Some(&share_id));
    Ok(state.remote_files.local_shares())
}

pub async fn set_remote_file_share_writable(
    state: &DesktopState,
    share_id: String,
    writable: bool,
) -> Result<LocalSharedDirectory, FileError> {
    state.remote_files.set_share_writable(&share_id, writable)
}

pub async fn set_remote_file_share_web_policy(
    state: &DesktopState,
    share_id: String,
    policy: crate::remote_files::WebSharePolicyUpdate,
    new_password: Option<String>,
) -> Result<LocalSharedDirectory, FileError> {
    let manager = state.remote_files.clone();
    let policy_share_id = share_id.clone();
    let updated = tokio::task::spawn_blocking(move || {
        manager.set_web_policy(&policy_share_id, policy, new_password.as_deref())
    })
    .await??;
    state.web_gateway.revoke_sessions(Some(&share_id));
    Ok(updated)
}

pub async fn get_web_gateway_status(
    state: &DesktopState,
) -> Result<arcrelay_web_gateway::WebGatewayStatus, String> {
    Ok(state.web_gateway.status().await)
}

pub async fn revoke_web_sessions(
    state: &DesktopState,
    share_id: Option<String>,
) -> Result<usize, String> {
    Ok(state.web_gateway.revoke_sessions(share_id.as_deref()))
}

pub async fn web_gateway_qr_code(state: &DesktopState, url: String) -> Result<String, String> {
    validate_web_gateway_url(state, &url).await?;
    let code = qrcode::QrCode::new(url.as_bytes()).map_err(|error| error.to_string())?;
    let image = code
        .render::<image::Luma<u8>>()
        .min_dimensions(320, 320)
        .quiet_zone(true)
        .build();
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::L8,
        )
        .map_err(|error| error.to_string())?;
    Ok(format!(
        "data:image/png;base64,{}",
        BASE64_STANDARD.encode(bytes)
    ))
}

pub async fn open_web_gateway_url(state: &DesktopState, url: String) -> Result<(), String> {
    validate_web_gateway_url(state, &url).await?;
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "linux")]
    let mut command = std::process::Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut value = std::process::Command::new("rundll32");
        value.arg("url.dll,FileProtocolHandler");
        value
    };
    command
        .arg(&url)
        .spawn()
        .map_err(|error| format!("failed to open browser: {error}"))?;
    Ok(())
}

async fn validate_web_gateway_url(state: &DesktopState, url: &str) -> Result<(), String> {
    let status = state.web_gateway.status().await;
    if !status.running
        || !status
            .addresses
            .iter()
            .any(|base| url == base || url.starts_with(&format!("{base}/")))
    {
        return Err("web access URL is invalid or the service is not running".into());
    }
    Ok(())
}

#[cfg(test)]
mod remote_open_tests {
    use super::*;

    #[test]
    fn active_transfer_peers_exclude_terminal_history_without_runtime() {
        let service = RemoteFileService::default();
        for status in ["transferring", "completed", "failed"] {
            service.transfers.lock().unwrap().insert(
                status.into(),
                RemoteFileTransferSession {
                    id: status.into(),
                    direction: "upload".into(),
                    status: status.into(),
                    name: String::new(),
                    peer_id: status.into(),
                    share_id: String::new(),
                    directory_path: String::new(),
                    bytes_transferred: 0,
                    total_bytes: 0,
                    files_transferred: 0,
                    total_files: 0,
                    current_name: String::new(),
                    error: None,
                    started_at_ms: 0,
                    updated_at_ms: 0,
                },
            );
        }
        assert_eq!(service.active_transfer_peer_ids(), vec!["transferring"]);
    }

    #[test]
    fn derives_remote_parent_paths() {
        assert_eq!(remote_parent_path("notes.txt").unwrap(), "");
        assert_eq!(remote_parent_path("docs/notes.txt").unwrap(), "docs");
        assert_eq!(
            remote_parent_path("docs/archive/notes.txt").unwrap(),
            "docs/archive"
        );
        assert!(remote_parent_path("").is_err());
    }

    #[tokio::test]
    async fn editable_text_detection_rejects_binary_and_large_files() {
        let directory = tempfile::tempdir().unwrap();
        let text = directory.path().join("notes.md");
        let binary = directory.path().join("image.txt");
        let unknown = directory.path().join("notes.bin");
        let utf16 = directory.path().join("windows.txt");
        tokio::fs::write(&text, "hello 世界").await.unwrap();
        tokio::fs::write(&binary, [0xff, 0xfd, 0xfc]).await.unwrap();
        tokio::fs::write(&unknown, "hello").await.unwrap();
        tokio::fs::write(&utf16, [0xff, 0xfe, b'h', 0, b'i', 0])
            .await
            .unwrap();

        assert!(is_editable_text_file(&text, 12).await);
        assert!(is_editable_text_file(&utf16, 6).await);
        assert!(!is_editable_text_file(&binary, 3).await);
        assert!(!is_editable_text_file(&unknown, 5).await);
        assert!(!is_editable_text_file(&text, MAX_EDITABLE_REMOTE_FILE_BYTES + 1).await);
    }

    #[test]
    fn drag_preview_uses_file_content_or_a_type_icon() {
        let directory = tempfile::tempdir().unwrap();
        let source_image = directory.path().join("photo.png");
        let source_file = directory.path().join("notes.bin");
        let image_preview = directory.path().join("image-preview.png");
        let file_preview = directory.path().join("file-preview.png");
        let folder_preview = directory.path().join("folder-preview.png");
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let promised_file_preview = directory.path().join("promised-file-preview.png");
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let promised_folder_preview = directory.path().join("promised-folder-preview.png");
        image::RgbaImage::from_pixel(40, 20, image::Rgba([20, 100, 220, 255]))
            .save(&source_image)
            .unwrap();
        std::fs::write(&source_file, b"not an image").unwrap();

        build_remote_drag_preview(&source_image, &image_preview).unwrap();
        build_remote_drag_preview(&source_file, &file_preview).unwrap();
        build_remote_drag_preview(directory.path(), &folder_preview).unwrap();
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        build_remote_drag_promise_preview(RemoteFileKind::File, &promised_file_preview).unwrap();
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        build_remote_drag_promise_preview(RemoteFileKind::Folder, &promised_folder_preview)
            .unwrap();

        let mut previews = vec![image_preview, file_preview, folder_preview];
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        previews.extend([promised_file_preview, promised_folder_preview]);
        for preview in previews {
            assert_eq!(image::image_dimensions(preview).unwrap(), (160, 160));
        }
    }
}

async fn file_signature(path: &Path) -> Option<(u64, Option<std::time::SystemTime>)> {
    let metadata = tokio::fs::metadata(path).await.ok()?;
    Some((metadata.len(), metadata.modified().ok()))
}
fn prune_thumbnail_cache(current: &Path) {
    let Some(directory) = current.parent() else {
        return;
    };
    let Ok(reader) = std::fs::read_dir(directory) else {
        return;
    };
    let mut entries = reader
        .flatten()
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            Some((entry.path(), metadata.len(), metadata.modified().ok()))
        })
        .collect::<Vec<_>>();
    let mut bytes = entries.iter().map(|entry| entry.1).sum::<u64>();
    let mut count = entries.len();
    entries.sort_by_key(|entry| entry.2);
    for (path, size, _) in entries {
        if count <= 256 && bytes <= 64 * 1024 * 1024 {
            break;
        }
        if path != current && std::fs::remove_file(path).is_ok() {
            count -= 1;
            bytes = bytes.saturating_sub(size);
        }
    }
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDeleteResult {
    pub path: String,
    pub error: Option<String>,
}
pub async fn delete_remote_entries(
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    paths: Vec<String>,
) -> Result<Vec<RemoteDeleteResult>, String> {
    if paths.len() > 500 {
        return Err("at most 500 items can be deleted at once".into());
    }
    let mut results = Vec::with_capacity(paths.len());
    for path in paths {
        let result = state
            .clipboard_sync
            .remote_file_request(
                &peer_id,
                RemoteFileRequest::Delete {
                    share_id: share_id.clone(),
                    relative_path: path.clone(),
                },
            )
            .await;
        results.push(RemoteDeleteResult {
            path,
            error: result.err().map(|error| error.to_string()),
        });
    }
    Ok(results)
}
pub async fn stop_remote_edit(
    state: &DesktopState,
    peer_id: String,
    share_id: String,
    relative_path: String,
) -> Result<(), String> {
    let _opening = state.remote_file_service.opening.lock().await;
    let key = remote_open_session_key(&peer_id, &share_id, &relative_path);
    if let Some(task) = state
        .remote_file_service
        .edit_tasks
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&key)
    {
        task.abort();
    }
    state
        .remote_file_service
        .open_sessions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&key);
    Ok(())
}
