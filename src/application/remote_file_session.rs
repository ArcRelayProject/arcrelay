use arcrelay_protocol::remote_files::{
    RemoteFileEntry, RemoteFileRequest, RemoteFileResponse, RemoteFileResult,
};
use std::{path::PathBuf, sync::Arc};
#[cfg(target_os = "windows")]
use tokio::sync::mpsc;

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

/// File operations require a session capability, independent of clipboard replication.
#[async_trait::async_trait]
pub trait RemoteFileSession: Send + Sync {
    async fn remote_file_devices(&self) -> Vec<RemoteFileDeviceView>;
    async fn remote_file_request(
        &self,
        peer_id: &str,
        request: RemoteFileRequest,
    ) -> RemoteFileResult<RemoteFileResponse>;
    async fn list_remote_directory_all(
        &self,
        peer_id: &str,
        share_id: &str,
        relative_path: &str,
    ) -> Result<Vec<RemoteFileEntry>, String>;
    async fn remote_file_thumbnail(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        max_dimension: u32,
    ) -> Result<(Vec<u8>, String), String>;
    async fn download_remote_file(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        destination: PathBuf,
    ) -> Result<PathBuf, String>;
    async fn download_remote_file_with_progress(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        destination: PathBuf,
        progress: Option<RemoteFileProgressCallback>,
    ) -> Result<PathBuf, String>;
    async fn upload_remote_files(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        paths: Vec<PathBuf>,
    ) -> Result<usize, String>;
    async fn upload_remote_files_with_progress(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        paths: Vec<PathBuf>,
        progress: Option<RemoteFileProgressCallback>,
    ) -> Result<usize, String>;
    async fn upload_remote_edit(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        source: PathBuf,
        expected_modified_at_ms: i64,
    ) -> RemoteFileResult<()>;
    #[cfg(target_os = "windows")]
    async fn stream_remote_file(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
    ) -> Result<RemoteFileStreamReceiver, String>;
}
