use std::path::Path;
use std::sync::Arc;

use arcrelay_files::{DirectorySortKey, FileEntry, FileError, FileKind, FileShareService};
use arcrelay_protocol::remote_files::{
    RemoteFileDirectoryPage, RemoteFileDownload, RemoteFileEntry, RemoteFileError,
    RemoteFileErrorCode, RemoteFileKind, RemoteFileProvider, RemoteFileResult, RemoteFileShare,
    RemoteFileSortDirection, RemoteFileSortKey, RemoteFileThumbnail, RemoteFileUpload,
};

pub use arcrelay_files::{LocalSharedDirectory, WebSharePolicyUpdate};

/// Protocol adapter for the shared file core. Paired-device write access remains
/// independent from every Web Gateway policy.
#[derive(Clone)]
pub struct RemoteFileManager {
    service: Arc<FileShareService>,
}

impl RemoteFileManager {
    pub fn load(config_directory: &Path) -> std::io::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            service: FileShareService::load(config_directory)?,
        }))
    }

    pub fn service(&self) -> Arc<FileShareService> {
        self.service.clone()
    }

    pub fn local_shares(&self) -> Vec<LocalSharedDirectory> {
        self.service.local_shares()
    }

    pub fn add_share(&self, path: &Path) -> Result<LocalSharedDirectory, FileError> {
        self.service.add_share(path)
    }

    pub fn remove_share(&self, id: &str) -> Result<(), FileError> {
        self.service.remove_share(id)
    }

    pub fn set_share_writable(
        &self,
        id: &str,
        writable: bool,
    ) -> Result<LocalSharedDirectory, FileError> {
        self.service.set_share_writable(id, writable)
    }

    pub fn set_web_policy(
        &self,
        id: &str,
        policy: WebSharePolicyUpdate,
        new_password: Option<&str>,
    ) -> Result<LocalSharedDirectory, FileError> {
        self.service.set_web_policy(id, policy, new_password)
    }
}

#[async_trait::async_trait]
impl RemoteFileProvider for RemoteFileManager {
    async fn list_shares(&self) -> RemoteFileResult<Vec<RemoteFileShare>> {
        Ok(self
            .service
            .local_shares()
            .into_iter()
            .map(|share| RemoteFileShare {
                id: share.id,
                name: share.name,
                writable: share.writable,
            })
            .collect())
    }

    async fn list_directory(
        &self,
        share_id: &str,
        relative_path: &str,
        cursor: Option<&str>,
        limit: u32,
        search: Option<&str>,
        sort_key: RemoteFileSortKey,
        sort_direction: RemoteFileSortDirection,
    ) -> RemoteFileResult<RemoteFileDirectoryPage> {
        if limit == 0 {
            return self
                .service
                .list_directory(share_id, relative_path)
                .await
                .map(|entries| RemoteFileDirectoryPage {
                    entries: entries.into_iter().map(protocol_entry).collect(),
                    next_cursor: None,
                })
                .map_err(remote_file_error);
        }
        self.service
            .list_directory_page(
                share_id,
                relative_path,
                cursor,
                limit as usize,
                search,
                match sort_key {
                    RemoteFileSortKey::Name => DirectorySortKey::Name,
                    RemoteFileSortKey::Modified => DirectorySortKey::Modified,
                    RemoteFileSortKey::Type => DirectorySortKey::Type,
                    RemoteFileSortKey::Size => DirectorySortKey::Size,
                },
                sort_direction == RemoteFileSortDirection::Descending,
            )
            .await
            .map(|page| RemoteFileDirectoryPage {
                entries: page.entries.into_iter().map(protocol_entry).collect(),
                next_cursor: page.next_cursor,
            })
            .map_err(remote_file_error)
    }

    async fn create_directory(
        &self,
        share_id: &str,
        relative_path: &str,
        name: &str,
    ) -> RemoteFileResult<RemoteFileEntry> {
        self.service
            .create_directory(share_id, relative_path, name)
            .await
            .map(protocol_entry)
            .map_err(remote_file_error)
    }

    async fn rename(
        &self,
        share_id: &str,
        relative_path: &str,
        new_name: &str,
    ) -> RemoteFileResult<RemoteFileEntry> {
        self.service
            .rename(share_id, relative_path, new_name)
            .await
            .map(protocol_entry)
            .map_err(remote_file_error)
    }

    async fn delete(&self, share_id: &str, relative_path: &str) -> RemoteFileResult<()> {
        self.service
            .delete(share_id, relative_path)
            .await
            .map_err(remote_file_error)
    }

    async fn prepare_download(
        &self,
        share_id: &str,
        relative_path: &str,
    ) -> RemoteFileResult<RemoteFileDownload> {
        let prepared = self
            .service
            .prepare_file(share_id, relative_path, false)
            .await
            .map_err(remote_file_error)?;
        Ok(RemoteFileDownload {
            path: prepared.path,
            entry: protocol_entry(prepared.entry),
        })
    }

    async fn prepare_thumbnail(
        &self,
        share_id: &str,
        relative_path: &str,
        max_dimension: u32,
    ) -> RemoteFileResult<Option<RemoteFileThumbnail>> {
        self.service
            .prepare_thumbnail(share_id, relative_path, max_dimension, false)
            .await
            .map(|thumbnail| {
                thumbnail.map(|thumbnail| RemoteFileThumbnail {
                    bytes: thumbnail.bytes,
                    media_type: thumbnail.media_type,
                })
            })
            .map_err(remote_file_error)
    }

    async fn prepare_upload(
        &self,
        share_id: &str,
        relative_path: &str,
        name: &str,
        size: u64,
        overwrite: bool,
        expected_modified_at_ms: Option<i64>,
    ) -> RemoteFileResult<RemoteFileUpload> {
        let upload = self
            .service
            .prepare_upload(
                share_id,
                relative_path,
                name,
                size,
                overwrite,
                expected_modified_at_ms,
            )
            .await
            .map_err(remote_file_error)?;
        Ok(RemoteFileUpload {
            destination: upload.destination,
            overwrite: upload.overwrite,
            expected_modified_at_ms: upload.expected_modified_at_ms,
            entry: protocol_entry(upload.entry),
        })
    }
}

fn remote_file_error(error: FileError) -> RemoteFileError {
    let code = match &error {
        FileError::Invalid(_) => RemoteFileErrorCode::InvalidArgument,
        FileError::NotFound(_) => RemoteFileErrorCode::NotFound,
        FileError::DirectorySnapshotExpired(_) | FileError::Conflict(_) => {
            RemoteFileErrorCode::Conflict
        }
        FileError::PermissionDenied(_) => RemoteFileErrorCode::PermissionDenied,
        FileError::DirectoryQueryTooBroad(_)
        | FileError::FileTooLarge(_)
        | FileError::InsufficientStorage(_) => RemoteFileErrorCode::ResourceExhausted,
        FileError::Unavailable(_) | FileError::Io { .. } => RemoteFileErrorCode::Unavailable,
        FileError::Serialization(_)
        | FileError::PasswordHash(_)
        | FileError::Task(_)
        | FileError::Image(_) => RemoteFileErrorCode::Internal,
    };
    RemoteFileError::new(code, error.to_string())
}

fn protocol_entry(entry: FileEntry) -> RemoteFileEntry {
    RemoteFileEntry {
        name: entry.name,
        relative_path: entry.relative_path,
        kind: match entry.kind {
            FileKind::File => RemoteFileKind::File,
            FileKind::Folder => RemoteFileKind::Folder,
        },
        size: entry.size,
        modified_at_ms: entry.modified_at_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn adapter_keeps_paired_device_writes_separate_from_web_policy() {
        let config = tempfile::tempdir().unwrap();
        let shared = tempfile::tempdir().unwrap();
        let manager = RemoteFileManager::load(config.path()).unwrap();
        let share = manager.add_share(shared.path()).unwrap();
        manager
            .set_web_policy(
                &share.id,
                WebSharePolicyUpdate {
                    mode: arcrelay_files::WebAccessMode::Public,
                    listed: true,
                    allow_preview: true,
                    allow_download: true,
                },
                None,
            )
            .unwrap();
        assert!(manager
            .create_directory(&share.id, "", "blocked")
            .await
            .is_err());
        manager.set_share_writable(&share.id, true).unwrap();
        assert!(manager
            .create_directory(&share.id, "", "allowed")
            .await
            .is_ok());
    }
}
