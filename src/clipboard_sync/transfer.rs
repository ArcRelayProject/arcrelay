use super::*;

pub(super) async fn send_sync_record(
    session: &mut ClipboardDownloadSession<'_>,
    record: ClipboardSyncRecord,
    include_payload: bool,
) -> Result<(), String> {
    use proto::clipboard_sync_record::Payload;
    let payload =
        if !include_payload || record.deleted || record.kind == ClipboardContentKind::Files {
            None
        } else {
            match record.kind {
                ClipboardContentKind::Text => {
                    Some(Payload::Text(record.text.clone().ok_or_else(|| {
                        "local synchronized text content is missing".to_string()
                    })?))
                }
                ClipboardContentKind::Html => {
                    let html = record
                        .html
                        .as_ref()
                        .ok_or_else(|| "local synchronized HTML content is missing".to_string())?;
                    let html_ref = upload_blob(
                        session.connection,
                        "text/html; charset=utf-8",
                        html.as_bytes(),
                        MAX_RICH_TEXT_BLOB_BYTES,
                    )
                    .await?;
                    let rtf = if let Some(rtf) = record.rtf.as_ref() {
                        Some(
                            upload_blob(
                                session.connection,
                                "text/rtf",
                                rtf.as_bytes(),
                                MAX_RICH_TEXT_BLOB_BYTES,
                            )
                            .await?,
                        )
                    } else {
                        None
                    };
                    Some(Payload::RichText(proto::ClipboardRichTextPayload {
                        plain_text: record.text.clone().unwrap_or_default(),
                        html: Some(html_ref),
                        rtf,
                    }))
                }
                ClipboardContentKind::Image => {
                    let png = record
                        .image_png
                        .as_ref()
                        .ok_or_else(|| "local synchronized image content is missing".to_string())?;
                    Some(Payload::Image(
                        upload_blob(session.connection, "image/png", png, MAX_IMAGE_BYTES).await?,
                    ))
                }
                ClipboardContentKind::Files => None,
            }
        };
    let record_id = crate::observability::pseudonym(&record.sync_id);
    let revision = record.revision;
    let wire = wire_record(record, payload)?;
    let request_id = take_request_id(session.next_request_id);
    send_client_frame(
        session.send,
        proto::ClientControlFrame {
            body: Some(proto::client_control_frame::Body::Request(proto::Request {
                request_id,
                timeout_ms: 60_000,
                idempotency_key: random_bytes(16),
                body: Some(proto::request::Body::Command(proto::Command {
                    action: Some(proto::command::Action::ApplyClipboardSyncRecord(
                        proto::ApplyClipboardSyncRecordCmd { record: Some(wire) },
                    )),
                })),
            })),
        },
    )
    .await?;
    let response = await_response(
        request_id,
        session.pending,
        session.responses,
        session.incoming,
    )
    .await?;
    if clipboard_record_rejected(response.status.as_ref()) {
        // A complete InvalidArgument response rejects this record, not the
        // transport. Continue live sync and snapshot pagination past it.
        tracing::warn!(
            event = "clipboard.sync.record_rejected",
            record_id,
            revision,
            request_id,
            error_code = proto::ErrorCode::InvalidArgument as i32,
            "remote peer rejected clipboard record; keeping control connection available"
        );
        return Ok(());
    }
    ensure_ok(response.status.as_ref(), "synchronize clipboard change")
}

pub(super) fn clipboard_record_rejected(status: Option<&proto::Status>) -> bool {
    status.is_some_and(|status| status.code == proto::ErrorCode::InvalidArgument as i32)
}

pub(super) async fn upload_blob(
    connection: &quinn::Connection,
    media_type: &str,
    bytes: &[u8],
    maximum: usize,
) -> Result<proto::BlobRef, String> {
    if bytes.is_empty() || bytes.len() > maximum {
        return Err(format!(
            "synchronized blob is empty or exceeds the limit: {media_type}"
        ));
    }
    let digest = Sha256::digest(bytes).to_vec();
    let upload_id = digest.clone();
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|error| error.to_string())?;
    send.write_u8(STREAM_KIND_CLIPBOARD_BLOB_UPLOAD)
        .await
        .map_err(|error| error.to_string())?;
    write_message(
        &mut send,
        &proto::ClipboardBlobUploadOpen {
            upload_id: upload_id.clone(),
            size: bytes.len() as u64,
            sha256: digest.clone(),
            media_type: media_type.to_string(),
        },
        MAX_CONTROL_FRAME_SIZE,
    )
    .await?;
    let header: proto::ClipboardBlobUploadHeader =
        read_message(&mut recv, MAX_CONTROL_FRAME_SIZE).await?;
    ensure_ok(header.status.as_ref(), "upload synchronized blob")?;
    for (index, chunk) in bytes.chunks(MAX_BLOB_CHUNK_SIZE).enumerate() {
        let offset = index * MAX_BLOB_CHUNK_SIZE;
        write_message(
            &mut send,
            &proto::ClipboardBlobUploadChunk {
                upload_id: upload_id.clone(),
                offset: offset as u64,
                data: chunk.to_vec(),
                end_of_upload: offset + chunk.len() == bytes.len(),
            },
            MAX_BLOB_CHUNK_SIZE + 1024,
        )
        .await?;
    }
    let result: proto::ClipboardBlobUploadResult =
        read_message(&mut recv, MAX_CONTROL_FRAME_SIZE).await?;
    ensure_ok(result.status.as_ref(), "upload synchronized blob")?;
    Ok(proto::BlobRef {
        blob_id: digest.clone(),
        size: bytes.len() as u64,
        media_type: media_type.to_string(),
        sha256: digest,
    })
}

pub(super) struct ClipboardDownloadSession<'a> {
    pub(super) connection: &'a quinn::Connection,
    pub(super) send: &'a mut quinn::SendStream,
    pub(super) next_request_id: &'a mut u64,
    pub(super) pending: &'a mut VecDeque<proto::ClipboardSyncRecord>,
    pub(super) responses: &'a mut HashMap<u64, proto::Response>,
    pub(super) incoming: &'a mut mpsc::Receiver<proto::ServerControlFrame>,
}

pub(super) async fn download_blob(
    download: &mut ClipboardDownloadSession<'_>,
    reference: &proto::BlobRef,
    media_type: &str,
    maximum: usize,
) -> Result<Vec<u8>, String> {
    if reference.size == 0
        || reference.size > maximum as u64
        || reference.blob_id.len() != 32
        || reference.sha256 != reference.blob_id
        || reference.media_type != media_type
    {
        return Err("invalid synchronized clipboard blob reference".into());
    }
    let request_id = take_request_id(download.next_request_id);
    send_client_frame(
        download.send,
        proto::ClientControlFrame {
            body: Some(proto::client_control_frame::Body::Request(proto::Request {
                request_id,
                timeout_ms: 60_000,
                idempotency_key: vec![],
                body: Some(proto::request::Body::GetBlob(proto::GetBlobRequest {
                    blob_id: reference.blob_id.clone(),
                })),
            })),
        },
    )
    .await?;
    let response = await_response(
        request_id,
        download.pending,
        download.responses,
        download.incoming,
    )
    .await?;
    ensure_ok(response.status.as_ref(), "fetch synchronized blob")?;
    let ticket = match response.body {
        Some(proto::response::Body::BlobTicket(ticket)) => ticket,
        _ => return Err("remote peer returned no blob download ticket".into()),
    };
    let (mut blob_send, mut blob_recv) = download
        .connection
        .open_bi()
        .await
        .map_err(|error| error.to_string())?;
    blob_send
        .write_u8(STREAM_KIND_BLOB_DOWNLOAD)
        .await
        .map_err(|error| error.to_string())?;
    write_message(
        &mut blob_send,
        &proto::BlobStreamOpen {
            transfer_id: ticket.transfer_id,
            ticket: ticket.ticket,
            offset: 0,
        },
        MAX_CONTROL_FRAME_SIZE,
    )
    .await?;
    let header: proto::BlobStreamHeader =
        read_message(&mut blob_recv, MAX_CONTROL_FRAME_SIZE).await?;
    ensure_ok(header.status.as_ref(), "download synchronized blob")?;
    let mut bytes = Vec::with_capacity(reference.size as usize);
    loop {
        let chunk: proto::BlobStreamChunk =
            read_message(&mut blob_recv, MAX_BLOB_CHUNK_SIZE + 1024).await?;
        if chunk.offset as usize != bytes.len() {
            return Err("invalid synchronized blob chunk offset".into());
        }
        bytes.extend_from_slice(&chunk.data);
        if bytes.len() > reference.size as usize {
            return Err("synchronized blob exceeds its declared size".into());
        }
        if chunk.end_of_blob {
            break;
        }
    }
    if bytes.len() != reference.size as usize
        || Sha256::digest(&bytes).as_slice() != reference.sha256
    {
        return Err("synchronized blob integrity check failed".into());
    }
    Ok(bytes)
}

pub(super) async fn download_text_blob(
    download: &mut ClipboardDownloadSession<'_>,
    reference: &proto::BlobRef,
    media_type: &str,
    maximum: usize,
) -> Result<String, String> {
    String::from_utf8(download_blob(download, reference, media_type, maximum).await?)
        .map_err(|_| format!("synchronized {media_type} is not UTF-8"))
}

pub(super) fn wire_record(
    record: ClipboardSyncRecord,
    payload: Option<proto::clipboard_sync_record::Payload>,
) -> Result<proto::ClipboardSyncRecord, String> {
    Ok(proto::ClipboardSyncRecord {
        sync_id: record.sync_id,
        kind: match record.kind {
            ClipboardContentKind::Text => proto::ClipboardContentKind::Text,
            ClipboardContentKind::Html => proto::ClipboardContentKind::Html,
            ClipboardContentKind::Image => proto::ClipboardContentKind::Image,
            ClipboardContentKind::Files => proto::ClipboardContentKind::Files,
        } as i32,
        width: record.width,
        height: record.height,
        preview: record.preview,
        source_app: record.source_app,
        source_device_id: record.source_device_id,
        source_device_name: record.source_device_name,
        captured_at_ms: record.captured_at_ms,
        revision: record.revision,
        updated_by_device_id: record.updated_by_device_id,
        favorite: record.favorite,
        favorite_revision: record.favorite_revision,
        favorite_updated_by_device_id: record.favorite_updated_by_device_id,
        labels: record
            .labels
            .into_iter()
            .map(|label| proto::ClipboardLabel {
                id: label.id,
                name: label.name,
                color: label.color,
                revision: label.revision,
                updated_by_device_id: label.updated_by_device_id,
                deleted: label.deleted,
            })
            .collect(),
        label_memberships: record
            .label_memberships
            .into_iter()
            .map(|membership| proto::ClipboardLabelMembership {
                label_id: membership.label_id,
                attached: membership.attached,
                revision: membership.revision,
                updated_by_device_id: membership.updated_by_device_id,
            })
            .collect(),
        deleted: record.deleted,
        change_kind: match record.change_kind {
            ClipboardSyncChangeKind::Copy => proto::ClipboardSyncChangeKind::Copy,
            ClipboardSyncChangeKind::Edit => proto::ClipboardSyncChangeKind::Edit,
            ClipboardSyncChangeKind::Favorite => proto::ClipboardSyncChangeKind::Favorite,
            ClipboardSyncChangeKind::Label => proto::ClipboardSyncChangeKind::Label,
            ClipboardSyncChangeKind::Delete => proto::ClipboardSyncChangeKind::Delete,
            ClipboardSyncChangeKind::Snapshot => proto::ClipboardSyncChangeKind::Snapshot,
        } as i32,
        live: record.live,
        text_syntax_json: serde_json::to_string(&record.text_syntax)
            .map_err(|error| error.to_string())?,
        payload,
    })
}

pub(super) fn domain_record(
    record: proto::ClipboardSyncRecord,
    html: Option<String>,
    rtf: Option<String>,
    image_png: Option<Vec<u8>>,
) -> Result<ClipboardSyncRecord, String> {
    use proto::clipboard_sync_record::Payload;
    let text = match record.payload.as_ref() {
        Some(Payload::Text(text)) => Some(text.clone()),
        Some(Payload::RichText(rich_text)) => Some(rich_text.plain_text.clone()),
        _ => None,
    };
    let text_syntax = serde_json::from_str::<ClipboardTextSyntax>(&record.text_syntax_json)
        .map_err(|error| format!("invalid remote clipboard text kind: {error}"))?;
    Ok(ClipboardSyncRecord {
        sync_id: record.sync_id,
        kind: match proto::ClipboardContentKind::try_from(record.kind)
            .unwrap_or(proto::ClipboardContentKind::Unspecified)
        {
            proto::ClipboardContentKind::Text => ClipboardContentKind::Text,
            proto::ClipboardContentKind::Html => ClipboardContentKind::Html,
            proto::ClipboardContentKind::Image => ClipboardContentKind::Image,
            proto::ClipboardContentKind::Files => ClipboardContentKind::Files,
            _ => return Err("remote clipboard kind cannot be synchronized".into()),
        },
        text,
        html,
        rtf,
        image_png,
        width: record.width,
        height: record.height,
        preview: record.preview,
        source_app: record.source_app,
        source_device_id: record.source_device_id,
        source_device_name: record.source_device_name,
        captured_at_ms: record.captured_at_ms,
        revision: record.revision,
        updated_by_device_id: record.updated_by_device_id,
        favorite: record.favorite,
        favorite_revision: record.favorite_revision,
        favorite_updated_by_device_id: record.favorite_updated_by_device_id,
        labels: record
            .labels
            .into_iter()
            .map(|label| ClipboardLabel {
                id: label.id,
                name: label.name,
                color: label.color,
                revision: label.revision,
                updated_by_device_id: label.updated_by_device_id,
                deleted: label.deleted,
            })
            .collect(),
        label_memberships: record
            .label_memberships
            .into_iter()
            .map(|membership| ClipboardLabelMembership {
                label_id: membership.label_id,
                attached: membership.attached,
                revision: membership.revision,
                updated_by_device_id: membership.updated_by_device_id,
            })
            .collect(),
        deleted: record.deleted,
        change_kind: match proto::ClipboardSyncChangeKind::try_from(record.change_kind)
            .unwrap_or(proto::ClipboardSyncChangeKind::Unspecified)
        {
            proto::ClipboardSyncChangeKind::Copy => ClipboardSyncChangeKind::Copy,
            proto::ClipboardSyncChangeKind::Edit => ClipboardSyncChangeKind::Edit,
            proto::ClipboardSyncChangeKind::Favorite => ClipboardSyncChangeKind::Favorite,
            proto::ClipboardSyncChangeKind::Label => ClipboardSyncChangeKind::Label,
            proto::ClipboardSyncChangeKind::Delete => ClipboardSyncChangeKind::Delete,
            proto::ClipboardSyncChangeKind::Snapshot => ClipboardSyncChangeKind::Snapshot,
            proto::ClipboardSyncChangeKind::Unspecified => {
                return Err("invalid remote synchronization change kind".into())
            }
        },
        live: record.live,
        text_syntax,
    })
}
