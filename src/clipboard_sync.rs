use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use arcrelay_core::application::clipboard_service::ClipboardApplicationService;
use arcrelay_core::domain::clipboard::{
    ClipboardContentKind, ClipboardLabel, ClipboardLabelMembership, ClipboardSyncChangeKind,
    ClipboardSyncRecord, ClipboardTextSyntax,
};
use arcrelay_network::{NetworkRuntime, PairingGrantRequest, PeerAdvertisement, SessionKind};
use arcrelay_peer::{CapabilityId, GrantDirection};
use arcrelay_protocol::remote_files::{
    read_remote_message, write_remote_message, RemoteFileAccess, RemoteFileEntry, RemoteFileError,
    RemoteFileErrorCode, RemoteFileKind, RemoteFileProvider, RemoteFileRequest, RemoteFileResponse,
    RemoteFileResult, RemoteFileSortDirection, RemoteFileSortKey,
    DEFAULT_REMOTE_DIRECTORY_PAGE_SIZE,
};
use arcrelay_protocol::server::{serve_remote_file_stream, ConnectionRegistry, ServerEvent};
use arcrelay_wire::proto;
use arcrelay_wire::{
    MAX_BLOB_CHUNK_SIZE, MAX_CONTROL_FRAME_SIZE, MAX_PRINT_DOCUMENT_CHUNK_SIZE,
    MAX_PRINT_DOCUMENT_SIZE, MAX_RELIABLE_INPUT_FRAME_SIZE, STREAM_KIND_BLOB_DOWNLOAD,
    STREAM_KIND_CLIPBOARD_BLOB_UPLOAD, STREAM_KIND_REMOTE_FILES,
};
use prost::Message;
use rand::RngCore;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const CONTROL_STREAM_OPEN_TIMEOUT: Duration = Duration::from_secs(12);
const INITIAL_SYNC_TIMEOUT: Duration = Duration::from_secs(15);
const NETWORK_START_TIMEOUT: Duration = Duration::from_secs(10);
const NETWORK_START_POLL_INTERVAL: Duration = Duration::from_millis(50);
const AUTOMATIC_DIAL_FALLBACK_DELAY: Duration = Duration::from_secs(5);
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const MAX_RICH_TEXT_BLOB_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub struct ClipboardSyncManager {
    clipboard: Arc<ClipboardApplicationService>,
    network: Arc<tokio::sync::OnceCell<Arc<NetworkRuntime>>>,
    commands: Arc<tokio::sync::Mutex<HashMap<String, mpsc::Sender<ConnectionCommand>>>>,
    remote_file_peers: Arc<tokio::sync::Mutex<HashSet<String>>>,
    active: Arc<tokio::sync::Mutex<HashSet<String>>>,
    retries: Arc<tokio::sync::Mutex<HashMap<String, crate::retry::RetryBackoff>>>,
    connecting: Arc<tokio::sync::Mutex<HashSet<String>>>,
    discovered: Arc<tokio::sync::Mutex<HashMap<String, PeerAdvertisement>>>,
    incoming_connections: Arc<tokio::sync::OnceCell<ConnectionRegistry>>,
    remote_file_provider: Arc<dyn RemoteFileProvider>,
    event_tx: mpsc::Sender<ServerEvent>,
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct NearbyDesktopView {
    pub device_id: String,
    pub device_name: String,
    pub host: String,
    pub paired: bool,
    pub connecting: bool,
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileDeviceView {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct RemoteFileTransferProgress {
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub files_transferred: usize,
    pub total_files: usize,
    pub current_name: String,
}

pub type RemoteFileProgressCallback =
    Arc<dyn Fn(RemoteFileTransferProgress) + Send + Sync + 'static>;

#[cfg(target_os = "windows")]
pub type RemoteFileStreamReceiver = mpsc::Receiver<Result<Vec<u8>, String>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionIntent {
    Automatic,
    AutomaticFallback,
    UserInitiated,
}

enum ConnectionCommand {
    SystemUpload {
        request: RemoteFileRequest,
        source: PathBuf,
        response: oneshot::Sender<RemoteFileResult<RemoteFileResponse>>,
    },
    Merge(oneshot::Sender<Result<usize, String>>),
    RemoteRequest(
        RemoteFileRequest,
        oneshot::Sender<RemoteFileResult<RemoteFileResponse>>,
    ),
    Thumbnail {
        share_id: String,
        relative_path: String,
        max_dimension: u32,
        response: oneshot::Sender<Result<(Vec<u8>, String), String>>,
    },
    Download {
        share_id: String,
        relative_path: String,
        destination: PathBuf,
        progress: Option<RemoteFileProgressCallback>,
        response: oneshot::Sender<Result<PathBuf, String>>,
    },
    #[cfg(target_os = "windows")]
    DownloadStream {
        share_id: String,
        relative_path: String,
        chunks: mpsc::Sender<Result<Vec<u8>, String>>,
    },
    Upload {
        share_id: String,
        relative_path: String,
        paths: Vec<PathBuf>,
        progress: Option<RemoteFileProgressCallback>,
        response: oneshot::Sender<Result<usize, String>>,
    },
    UploadEdit {
        share_id: String,
        relative_path: String,
        source: PathBuf,
        expected_modified_at_ms: i64,
        response: oneshot::Sender<RemoteFileResult<()>>,
    },
}

enum RemoteFileRoute {
    Managed(mpsc::Sender<ConnectionCommand>),
    Incoming(quinn::Connection),
}

#[derive(Debug, Clone, Default)]
struct RemoteClipboardVersions {
    records: HashMap<String, RemoteClipboardVersion>,
}

#[derive(Debug, Clone)]
struct RemoteClipboardVersion {
    content: (u64, String),
    favorite: (u64, String),
    labels: HashMap<String, (u64, String)>,
    memberships: HashMap<String, (u64, String)>,
}

impl RemoteClipboardVersion {
    fn from_wire(record: &proto::ClipboardSyncRecord) -> Self {
        Self {
            content: (record.revision, record.updated_by_device_id.clone()),
            favorite: (
                record.favorite_revision,
                record.favorite_updated_by_device_id.clone(),
            ),
            labels: record
                .labels
                .iter()
                .map(|label| {
                    (
                        label.id.clone(),
                        (label.revision, label.updated_by_device_id.clone()),
                    )
                })
                .collect(),
            memberships: record
                .label_memberships
                .iter()
                .map(|membership| {
                    (
                        membership.label_id.clone(),
                        (membership.revision, membership.updated_by_device_id.clone()),
                    )
                })
                .collect(),
        }
    }

    fn has_current_content(&self, record: &ClipboardSyncRecord) -> bool {
        self.content >= (record.revision, record.updated_by_device_id.clone())
    }

    fn covers(&self, record: &ClipboardSyncRecord) -> bool {
        self.has_current_content(record)
            && self.favorite
                >= (
                    record.favorite_revision,
                    record.favorite_updated_by_device_id.clone(),
                )
            && record.labels.iter().all(|label| {
                self.labels.get(&label.id).is_some_and(|version| {
                    version >= &(label.revision, label.updated_by_device_id.clone())
                })
            })
            && record.label_memberships.iter().all(|membership| {
                self.memberships
                    .get(&membership.label_id)
                    .is_some_and(|version| {
                        version >= &(membership.revision, membership.updated_by_device_id.clone())
                    })
            })
    }
}

#[derive(Debug, Default)]
struct ClipboardMergeResult {
    merged: usize,
    remote_versions: RemoteClipboardVersions,
}

struct AbortTaskOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortTaskOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn wait_for_available<T>(
    timeout: Duration,
    poll_interval: Duration,
    mut value: impl FnMut() -> Option<T>,
) -> Option<T> {
    tokio::time::timeout(timeout, async {
        loop {
            if let Some(value) = value() {
                break value;
            }
            tokio::time::sleep(poll_interval).await;
        }
    })
    .await
    .ok()
}

mod manager;
mod remote_files;
mod transfer;
mod wire;

use remote_files::*;
use transfer::*;
use wire::*;

#[cfg(test)]
mod tests;
