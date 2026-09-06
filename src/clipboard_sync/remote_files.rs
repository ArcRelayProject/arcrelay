use super::*;

#[derive(Debug)]
enum LocalUploadItem {
    Directory {
        remote_parent: String,
        name: String,
    },
    File {
        source: PathBuf,
        remote_parent: String,
        size: u64,
    },
}

#[derive(Debug)]
struct RemoteDownloadItem {
    remote_path: String,
    local_path: PathBuf,
    name: String,
    size: u64,
}

type ByteProgressCallback = Arc<dyn Fn(u64, u64) + Send + Sync + 'static>;

const MAX_REMOTE_THUMBNAIL_BYTES: u64 = 2 * 1024 * 1024;

async fn list_remote_directory_entries_on_connection(
    connection: &quinn::Connection,
    share_id: &str,
    relative_path: &str,
) -> Result<Vec<RemoteFileEntry>, String> {
    let mut entries = Vec::new();
    let mut cursor = None;
    loop {
        let response = remote_file_request_on_connection(
            connection,
            RemoteFileRequest::ListDirectory {
                share_id: share_id.to_string(),
                relative_path: relative_path.to_string(),
                cursor,
                limit: DEFAULT_REMOTE_DIRECTORY_PAGE_SIZE,
                search: None,
                sort_key: RemoteFileSortKey::Name,
                sort_direction: RemoteFileSortDirection::Ascending,
            },
        )
        .await
        .map_err(|error| error.to_string())?;
        entries.extend(response.entries);
        let Some(next_cursor) = response.next_cursor else {
            return Ok(entries);
        };
        cursor = Some(next_cursor);
    }
}

pub(super) async fn remote_file_request_on_connection(
    connection: &quinn::Connection,
    request: RemoteFileRequest,
) -> RemoteFileResult<RemoteFileResponse> {
    tracing::debug!(
        event = "remote_files.request.started",
        operation = request.operation_name(),
        "sending remote file request"
    );
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|error| remote_file_unavailable("failed to open remote file stream", error))?;
    send.write_u8(STREAM_KIND_REMOTE_FILES)
        .await
        .map_err(|error| {
            remote_file_unavailable("failed to write remote file stream kind", error)
        })?;
    write_remote_message(&mut send, &request)
        .await
        .map_err(|error| remote_file_unavailable("failed to write remote file request", error))?;
    send.finish()
        .map_err(|error| remote_file_unavailable("failed to finish remote file request", error))?;
    let response: RemoteFileResponse = read_remote_message(&mut recv).await.map_err(|error| {
        tracing::warn!(%error, "remote file response failed");
        remote_file_unavailable("failed to read remote file response", error)
    })?;
    ensure_remote_file_response(response).map_err(|error| {
        tracing::warn!(%error, "remote file request rejected");
        error
    })
}

pub(super) fn ensure_remote_file_response(
    response: RemoteFileResponse,
) -> RemoteFileResult<RemoteFileResponse> {
    if response.ok {
        Ok(response)
    } else {
        Err(response.error.unwrap_or_else(|| {
            RemoteFileError::new(
                RemoteFileErrorCode::Internal,
                "remote file operation failed without an error payload",
            )
        }))
    }
}

fn remote_file_unavailable(context: &str, error: impl std::fmt::Display) -> RemoteFileError {
    RemoteFileError::new(
        RemoteFileErrorCode::Unavailable,
        format!("{context}: {error}"),
    )
}

pub(super) async fn download_remote_thumbnail(
    connection: &quinn::Connection,
    share_id: &str,
    relative_path: &str,
    max_dimension: u32,
) -> Result<(Vec<u8>, String), String> {
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|error| format!("failed to open thumbnail stream: {error}"))?;
    send.write_u8(STREAM_KIND_REMOTE_FILES)
        .await
        .map_err(|error| error.to_string())?;
    write_remote_message(
        &mut send,
        &RemoteFileRequest::Thumbnail {
            share_id: share_id.to_string(),
            relative_path: relative_path.to_string(),
            max_dimension,
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    send.finish().map_err(|error| error.to_string())?;
    let response: RemoteFileResponse = read_remote_message(&mut recv)
        .await
        .map_err(|error| error.to_string())?;
    let response = ensure_remote_file_response(response).map_err(|error| error.to_string())?;
    if response.thumbnail_size == 0 || response.thumbnail_size > MAX_REMOTE_THUMBNAIL_BYTES {
        return Err("remote peer returned an invalid thumbnail size".into());
    }
    let mut bytes = Vec::with_capacity(response.thumbnail_size as usize);
    let mut limited = recv.take(response.thumbnail_size);
    limited
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 != response.thumbnail_size {
        return Err("thumbnail transfer ended early".into());
    }
    Ok((
        bytes,
        response
            .thumbnail_media_type
            .unwrap_or_else(|| "image/png".into()),
    ))
}

pub(super) async fn download_remote_entry(
    connection: &quinn::Connection,
    share_id: &str,
    relative_path: &str,
    destination: &Path,
    progress: Option<RemoteFileProgressCallback>,
) -> Result<PathBuf, String> {
    tokio::fs::create_dir_all(destination)
        .await
        .map_err(|error| error.to_string())?;
    let directory_probe =
        list_remote_directory_entries_on_connection(connection, share_id, relative_path).await;
    if let Ok(root_entries) = directory_probe {
        let root_name = remote_name(relative_path)?;
        let root_destination = destination.join(root_name);
        tokio::fs::create_dir_all(&root_destination)
            .await
            .map_err(|error| error.to_string())?;
        let mut directories = VecDeque::from([(root_destination.clone(), root_entries)]);
        let mut files = Vec::new();
        while let Some((local_directory, entries)) = directories.pop_front() {
            for entry in entries {
                let local_path = local_directory.join(&entry.name);
                match entry.kind {
                    RemoteFileKind::File => {
                        files.push(RemoteDownloadItem {
                            remote_path: entry.relative_path,
                            local_path,
                            name: entry.name,
                            size: entry.size,
                        });
                    }
                    RemoteFileKind::Folder => {
                        tokio::fs::create_dir_all(&local_path)
                            .await
                            .map_err(|error| error.to_string())?;
                        let entries = list_remote_directory_entries_on_connection(
                            connection,
                            share_id,
                            &entry.relative_path,
                        )
                        .await?;
                        directories.push_back((local_path, entries));
                    }
                }
            }
        }
        let total_bytes: u64 = files.iter().map(|file| file.size).sum();
        let total_files = files.len();
        emit_transfer_progress(&progress, 0, total_bytes, 0, total_files, root_name);
        let mut completed_bytes = 0_u64;
        for (index, file) in files.into_iter().enumerate() {
            let base = completed_bytes;
            let aggregate = progress.as_ref().map(|progress| {
                let progress = progress.clone();
                let name = file.name.clone();
                Arc::new(move |current: u64, _file_total: u64| {
                    progress(RemoteFileTransferProgress {
                        bytes_transferred: base.saturating_add(current),
                        total_bytes,
                        files_transferred: index,
                        total_files,
                        current_name: name.clone(),
                    });
                }) as ByteProgressCallback
            });
            download_remote_file_with_progress(
                connection,
                share_id,
                &file.remote_path,
                &file.local_path,
                aggregate,
            )
            .await?;
            completed_bytes = completed_bytes.saturating_add(file.size);
            emit_transfer_progress(
                &progress,
                completed_bytes,
                total_bytes,
                index + 1,
                total_files,
                &file.name,
            );
        }
        Ok(root_destination)
    } else {
        let destination = destination.join(remote_name(relative_path)?);
        let name = remote_name(relative_path)?.to_string();
        let aggregate = progress.as_ref().map(|progress| {
            let progress = progress.clone();
            let name = name.clone();
            Arc::new(move |current: u64, total: u64| {
                progress(RemoteFileTransferProgress {
                    bytes_transferred: current,
                    total_bytes: total,
                    files_transferred: usize::from(current == total),
                    total_files: 1,
                    current_name: name.clone(),
                });
            }) as ByteProgressCallback
        });
        download_remote_file_with_progress(
            connection,
            share_id,
            relative_path,
            &destination,
            aggregate,
        )
        .await?;
        Ok(destination)
    }
}

async fn download_remote_file_with_progress(
    connection: &quinn::Connection,
    share_id: &str,
    relative_path: &str,
    destination: &Path,
    progress: Option<ByteProgressCallback>,
) -> Result<(), String> {
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|error| format!("failed to open download stream: {error}"))?;
    send.write_u8(STREAM_KIND_REMOTE_FILES)
        .await
        .map_err(|error| error.to_string())?;
    write_remote_message(
        &mut send,
        &RemoteFileRequest::Download {
            share_id: share_id.to_string(),
            relative_path: relative_path.to_string(),
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    send.finish().map_err(|error| error.to_string())?;
    let response: RemoteFileResponse = read_remote_message(&mut recv)
        .await
        .map_err(|error| error.to_string())?;
    let response = ensure_remote_file_response(response).map_err(|error| error.to_string())?;
    let entry = response
        .entry
        .ok_or_else(|| "remote peer returned no file metadata".to_string())?;
    let parent = destination
        .parent()
        .ok_or_else(|| "invalid download destination".to_string())?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(".arcrelay-download-{}", uuid::Uuid::new_v4()));
    let result: Result<(), String> = async {
        let mut output = tokio::fs::File::create(&temporary)
            .await
            .map_err(|error| error.to_string())?;
        let mut limited = recv.take(entry.size);
        let mut received = 0_u64;
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let count = limited
                .read(&mut buffer)
                .await
                .map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            output
                .write_all(&buffer[..count])
                .await
                .map_err(|error| error.to_string())?;
            received = received.saturating_add(count as u64);
            if let Some(progress) = &progress {
                progress(received, entry.size);
            }
        }
        output.flush().await.map_err(|error| error.to_string())?;
        output.sync_all().await.map_err(|error| error.to_string())?;
        drop(output);
        if received != entry.size {
            return Err(format!(
                "download ended early: expected {} bytes, received {received} bytes",
                entry.size
            ));
        }
        if destination.exists() {
            if destination.is_dir() {
                return Err(
                    "download destination already contains a folder with the same name".into(),
                );
            }
            tokio::fs::remove_file(destination)
                .await
                .map_err(|error| error.to_string())?;
        }
        tokio::fs::rename(&temporary, destination)
            .await
            .map_err(|error| error.to_string())
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    result
}

#[cfg(target_os = "windows")]
pub(super) async fn stream_remote_file(
    connection: &quinn::Connection,
    share_id: &str,
    relative_path: &str,
    chunks: mpsc::Sender<Result<Vec<u8>, String>>,
) {
    let result: Result<(), String> = async {
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|error| format!("failed to open download stream: {error}"))?;
        send.write_u8(STREAM_KIND_REMOTE_FILES)
            .await
            .map_err(|error| error.to_string())?;
        write_remote_message(
            &mut send,
            &RemoteFileRequest::Download {
                share_id: share_id.to_string(),
                relative_path: relative_path.to_string(),
            },
        )
        .await
        .map_err(|error| error.to_string())?;
        send.finish().map_err(|error| error.to_string())?;
        let response: RemoteFileResponse = read_remote_message(&mut recv)
            .await
            .map_err(|error| error.to_string())?;
        let response = ensure_remote_file_response(response).map_err(|error| error.to_string())?;
        let entry = response
            .entry
            .ok_or_else(|| "remote peer returned no file metadata".to_string())?;
        if entry.kind != RemoteFileKind::File {
            return Err("remote entry is not a regular file".into());
        }

        let mut limited = recv.take(entry.size);
        let mut received = 0_u64;
        loop {
            let mut buffer = vec![0_u8; 64 * 1024];
            let count = limited
                .read(&mut buffer)
                .await
                .map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            buffer.truncate(count);
            received = received.saturating_add(count as u64);
            if chunks.send(Ok(buffer)).await.is_err() {
                return Ok(());
            }
        }
        if received != entry.size {
            return Err(format!(
                "download ended early: expected {} bytes, received {received} bytes",
                entry.size
            ));
        }
        Ok(())
    }
    .await;

    if let Err(error) = result {
        let _ = chunks.send(Err(error)).await;
    }
}

pub(super) async fn upload_remote_paths(
    connection: &quinn::Connection,
    share_id: &str,
    relative_path: &str,
    paths: Vec<PathBuf>,
    progress: Option<RemoteFileProgressCallback>,
) -> Result<usize, String> {
    if paths.is_empty() {
        return Err("no files selected for upload".into());
    }
    let base = relative_path.to_string();
    let items = tokio::task::spawn_blocking(move || collect_upload_items(&base, &paths))
        .await
        .map_err(|error| error.to_string())??;
    let total_bytes: u64 = items
        .iter()
        .map(|item| match item {
            LocalUploadItem::File { size, .. } => *size,
            LocalUploadItem::Directory { .. } => 0,
        })
        .sum();
    let total_files = items
        .iter()
        .filter(|item| matches!(item, LocalUploadItem::File { .. }))
        .count();
    emit_transfer_progress(&progress, 0, total_bytes, 0, total_files, "");
    let mut uploaded = 0;
    let mut completed_bytes = 0_u64;
    for item in items {
        match item {
            LocalUploadItem::Directory {
                remote_parent,
                name,
            } => {
                remote_file_request_on_connection(
                    connection,
                    RemoteFileRequest::CreateDirectory {
                        share_id: share_id.to_string(),
                        relative_path: remote_parent,
                        name,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            }
            LocalUploadItem::File {
                source,
                remote_parent,
                size,
            } => {
                let base = completed_bytes;
                let name = source
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("file")
                    .to_string();
                let aggregate = progress.as_ref().map(|progress| {
                    let progress = progress.clone();
                    let name = name.clone();
                    Arc::new(move |current: u64, _file_total: u64| {
                        progress(RemoteFileTransferProgress {
                            bytes_transferred: base.saturating_add(current),
                            total_bytes,
                            files_transferred: uploaded,
                            total_files,
                            current_name: name.clone(),
                        });
                    }) as ByteProgressCallback
                });
                upload_remote_file_with_progress(
                    connection,
                    share_id,
                    &remote_parent,
                    &source,
                    None,
                    aggregate,
                )
                .await
                .map_err(|error| error.to_string())?;
                uploaded += 1;
                completed_bytes = completed_bytes.saturating_add(size);
                emit_transfer_progress(
                    &progress,
                    completed_bytes,
                    total_bytes,
                    uploaded,
                    total_files,
                    &name,
                );
            }
        }
    }
    Ok(uploaded)
}

pub(super) async fn upload_remote_file(
    connection: &quinn::Connection,
    share_id: &str,
    relative_path: &str,
    source: &Path,
    expected_modified_at_ms: Option<i64>,
) -> RemoteFileResult<()> {
    upload_remote_file_with_progress(
        connection,
        share_id,
        relative_path,
        source,
        expected_modified_at_ms,
        None,
    )
    .await
}

async fn upload_remote_file_with_progress(
    connection: &quinn::Connection,
    share_id: &str,
    relative_path: &str,
    source: &Path,
    expected_modified_at_ms: Option<i64>,
    progress: Option<ByteProgressCallback>,
) -> RemoteFileResult<()> {
    let metadata = tokio::fs::metadata(source)
        .await
        .map_err(|error| remote_file_unavailable("failed to read upload source metadata", error))?;
    if !metadata.is_file() {
        return Err(RemoteFileError::new(
            RemoteFileErrorCode::InvalidArgument,
            format!("not a regular file: {}", source.display()),
        ));
    }
    let name = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            RemoteFileError::new(
                RemoteFileErrorCode::InvalidArgument,
                "filename is not valid text",
            )
        })?;
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|error| remote_file_unavailable("failed to open upload stream", error))?;
    send.write_u8(STREAM_KIND_REMOTE_FILES)
        .await
        .map_err(|error| remote_file_unavailable("failed to write upload stream kind", error))?;
    write_remote_message(
        &mut send,
        &RemoteFileRequest::Upload {
            share_id: share_id.to_string(),
            relative_path: relative_path.to_string(),
            name: name.to_string(),
            size: metadata.len(),
            overwrite: true,
            expected_modified_at_ms,
        },
    )
    .await
    .map_err(|error| remote_file_unavailable("failed to write upload request", error))?;
    let ready: RemoteFileResponse = read_remote_message(&mut recv)
        .await
        .map_err(|error| remote_file_unavailable("failed to read upload readiness", error))?;
    ensure_remote_file_response(ready)?;
    let mut input = tokio::fs::File::open(source)
        .await
        .map_err(|error| remote_file_unavailable("failed to open upload source", error))?;
    let mut sent = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = input
            .read(&mut buffer)
            .await
            .map_err(|error| remote_file_unavailable("failed to read upload source", error))?;
        if count == 0 {
            break;
        }
        send.write_all(&buffer[..count])
            .await
            .map_err(|error| remote_file_unavailable("failed to send upload data", error))?;
        sent = sent.saturating_add(count as u64);
        if let Some(progress) = &progress {
            progress(sent, metadata.len());
        }
    }
    if sent != metadata.len() {
        return Err(RemoteFileError::new(
            RemoteFileErrorCode::Conflict,
            "local file changed during upload",
        ));
    }
    send.finish()
        .map_err(|error| remote_file_unavailable("failed to finish upload stream", error))?;
    let completed: RemoteFileResponse = read_remote_message(&mut recv)
        .await
        .map_err(|error| remote_file_unavailable("failed to read upload result", error))?;
    ensure_remote_file_response(completed)?;
    Ok(())
}

fn collect_upload_items(base: &str, paths: &[PathBuf]) -> Result<Vec<LocalUploadItem>, String> {
    let mut items = Vec::new();
    for path in paths {
        let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
        if metadata.is_file() {
            items.push(LocalUploadItem::File {
                source: path.clone(),
                remote_parent: base.to_string(),
                size: metadata.len(),
            });
        } else if metadata.is_dir() {
            collect_upload_directory(base, path, &mut items)?;
        }
    }
    Ok(items)
}

fn collect_upload_directory(
    remote_parent: &str,
    directory: &Path,
    items: &mut Vec<LocalUploadItem>,
) -> Result<(), String> {
    let name = directory
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "folder name is not valid text".to_string())?
        .to_string();
    items.push(LocalUploadItem::Directory {
        remote_parent: remote_parent.to_string(),
        name: name.clone(),
    });
    let child_parent = join_remote_path(remote_parent, &name);
    let mut children = std::fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    children.sort_by_key(std::fs::DirEntry::file_name);
    for child in children {
        let file_type = child.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_upload_directory(&child_parent, &child.path(), items)?;
        } else if file_type.is_file() {
            let metadata = child.metadata().map_err(|error| error.to_string())?;
            items.push(LocalUploadItem::File {
                source: child.path(),
                remote_parent: child_parent.clone(),
                size: metadata.len(),
            });
        }
    }
    Ok(())
}

fn emit_transfer_progress(
    progress: &Option<RemoteFileProgressCallback>,
    bytes_transferred: u64,
    total_bytes: u64,
    files_transferred: usize,
    total_files: usize,
    current_name: &str,
) {
    if let Some(progress) = progress {
        progress(RemoteFileTransferProgress {
            bytes_transferred,
            total_bytes,
            files_transferred,
            total_files,
            current_name: current_name.to_string(),
        });
    }
}

pub(super) fn join_remote_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), name)
    }
}

pub(super) fn remote_name(path: &str) -> Result<&str, String> {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "remote path has no usable name".to_string())
}
