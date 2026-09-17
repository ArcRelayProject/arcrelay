use super::*;
use crate::application::remote_file_session::RemoteFileSession;

#[async_trait::async_trait]
impl RemoteFileSession for ClipboardSyncManager {
    async fn remote_file_devices(&self) -> Vec<RemoteFileDeviceView> {
        ClipboardSyncManager::remote_file_devices(self).await
    }
    async fn remote_file_request(
        &self,
        peer_id: &str,
        request: RemoteFileRequest,
    ) -> RemoteFileResult<RemoteFileResponse> {
        ClipboardSyncManager::remote_file_request(self, peer_id, request).await
    }
    async fn list_remote_directory_all(
        &self,
        peer_id: &str,
        share_id: &str,
        relative_path: &str,
    ) -> Result<Vec<RemoteFileEntry>, String> {
        ClipboardSyncManager::list_remote_directory_all(self, peer_id, share_id, relative_path)
            .await
    }
    async fn remote_file_thumbnail(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        max_dimension: u32,
    ) -> Result<(Vec<u8>, String), String> {
        ClipboardSyncManager::remote_file_thumbnail(
            self,
            peer_id,
            share_id,
            relative_path,
            max_dimension,
        )
        .await
    }
    async fn download_remote_file(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        destination: PathBuf,
    ) -> Result<PathBuf, String> {
        ClipboardSyncManager::download_remote_file(
            self,
            peer_id,
            share_id,
            relative_path,
            destination,
        )
        .await
    }
    async fn download_remote_file_with_progress(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        destination: PathBuf,
        progress: Option<RemoteFileProgressCallback>,
    ) -> Result<PathBuf, String> {
        ClipboardSyncManager::download_remote_file_with_progress(
            self,
            peer_id,
            share_id,
            relative_path,
            destination,
            progress,
        )
        .await
    }
    async fn upload_remote_files(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        paths: Vec<PathBuf>,
    ) -> Result<usize, String> {
        ClipboardSyncManager::upload_remote_files(self, peer_id, share_id, relative_path, paths)
            .await
    }
    async fn upload_remote_files_with_progress(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        paths: Vec<PathBuf>,
        progress: Option<RemoteFileProgressCallback>,
    ) -> Result<usize, String> {
        ClipboardSyncManager::upload_remote_files_with_progress(
            self,
            peer_id,
            share_id,
            relative_path,
            paths,
            progress,
        )
        .await
    }
    async fn upload_remote_edit(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        source: PathBuf,
        expected_modified_at_ms: i64,
    ) -> RemoteFileResult<()> {
        ClipboardSyncManager::upload_remote_edit(
            self,
            peer_id,
            share_id,
            relative_path,
            source,
            expected_modified_at_ms,
        )
        .await
    }
    #[cfg(target_os = "windows")]
    async fn stream_remote_file(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
    ) -> Result<RemoteFileStreamReceiver, String> {
        ClipboardSyncManager::stream_remote_file(self, peer_id, share_id, relative_path).await
    }
}
