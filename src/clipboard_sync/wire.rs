use super::*;

#[allow(deprecated)]
pub(super) fn control_hello() -> proto::ClientControlFrame {
    let features = [
        proto::Feature::Subscriptions,
        proto::Feature::Clipboard,
        proto::Feature::ClipboardSync,
        proto::Feature::Blobs,
        proto::Feature::RemoteFiles,
    ]
    .into_iter()
    .map(|feature| proto::FeatureVersion {
        feature: feature as i32,
        min_version: if feature == proto::Feature::RemoteFiles {
            arcrelay_protocol::remote_files::REMOTE_FILE_PROTOCOL_VERSION
        } else {
            1
        },
        max_version: if feature == proto::Feature::RemoteFiles {
            arcrelay_protocol::remote_files::REMOTE_FILE_PROTOCOL_VERSION
        } else {
            1
        },
    })
    .collect();
    proto::ClientControlFrame {
        body: Some(proto::client_control_frame::Body::Hello(
            proto::ControlHello {
                features,
                receive_limits: Some(proto::ProtocolLimits {
                    max_control_frame_bytes: MAX_CONTROL_FRAME_SIZE as u32,
                    max_reliable_input_frame_bytes: MAX_RELIABLE_INPUT_FRAME_SIZE as u32,
                    max_datagram_bytes: 0,
                    max_concurrent_requests: 16,
                    heartbeat_interval_ms: 15_000,
                    heartbeat_timeout_ms: 45_000,
                    max_blob_chunk_bytes: MAX_BLOB_CHUNK_SIZE as u32,
                    max_remote_file_message_bytes:
                        arcrelay_protocol::remote_files::MAX_REMOTE_FILE_MESSAGE_SIZE as u32,
                    max_remote_file_content_bytes:
                        arcrelay_protocol::remote_files::MAX_REMOTE_FILE_CONTENT_SIZE,
                    max_print_chunk_bytes: MAX_PRINT_DOCUMENT_CHUNK_SIZE as u32,
                    max_print_document_bytes: MAX_PRINT_DOCUMENT_SIZE,
                }),
            },
        )),
    }
}

pub(super) async fn await_subscription(
    recv: &mut quinn::RecvStream,
    request_id: u64,
) -> Result<(), String> {
    loop {
        let frame = recv_server_frame(recv, REQUEST_TIMEOUT).await?;
        if let Some(proto::server_control_frame::Body::SubscriptionResult(result)) = frame.body {
            if result.request_id == request_id {
                return ensure_ok(
                    result.status.as_ref(),
                    "subscribe to clipboard synchronization events",
                );
            }
        }
    }
}

pub(super) async fn await_response(
    request_id: u64,
    pending: &mut VecDeque<proto::ClipboardSyncRecord>,
    responses: &mut HashMap<u64, proto::Response>,
    incoming: &mut mpsc::Receiver<proto::ServerControlFrame>,
) -> Result<proto::Response, String> {
    if let Some(response) = responses.remove(&request_id) {
        return Ok(response);
    }
    loop {
        let frame = tokio::time::timeout(REQUEST_TIMEOUT, incoming.recv())
            .await
            .map_err(|_| "timed out waiting for the remote response".to_string())?
            .ok_or_else(|| "remote control stream closed".to_string())?;
        queue_server_frame(frame, pending, responses)?;
        if let Some(response) = responses.remove(&request_id) {
            return Ok(response);
        }
    }
}

pub(super) fn queue_server_frame(
    frame: proto::ServerControlFrame,
    pending: &mut VecDeque<proto::ClipboardSyncRecord>,
    responses: &mut HashMap<u64, proto::Response>,
) -> Result<(), String> {
    match frame.body {
        Some(proto::server_control_frame::Body::Response(response)) => {
            responses.insert(response.request_id, response);
            Ok(())
        }
        Some(proto::server_control_frame::Body::Event(event)) => {
            if let Some(proto::event::Data::ClipboardSync(record)) = event.data {
                pending.push_back(record);
            }
            Ok(())
        }
        Some(proto::server_control_frame::Body::GoAway(go_away)) => {
            Err(go_away.status.map_or_else(
                || "remote peer closed the connection".into(),
                |status| status.message,
            ))
        }
        _ => Ok(()),
    }
}

pub(super) async fn send_client_frame(
    send: &mut quinn::SendStream,
    frame: proto::ClientControlFrame,
) -> Result<(), String> {
    write_message(send, &frame, MAX_CONTROL_FRAME_SIZE).await
}

pub(super) async fn recv_server_frame(
    recv: &mut quinn::RecvStream,
    timeout: Duration,
) -> Result<proto::ServerControlFrame, String> {
    tokio::time::timeout(timeout, read_message(recv, MAX_CONTROL_FRAME_SIZE))
        .await
        .map_err(|_| "timed out waiting for a remote control frame".to_string())?
}

pub(super) async fn write_message<W: tokio::io::AsyncWrite + Unpin, M: Message>(
    writer: &mut W,
    message: &M,
    maximum: usize,
) -> Result<(), String> {
    arcrelay_transport::write_frame(writer, &message.encode_to_vec(), maximum)
        .await
        .map_err(|error| error.to_string())
}

pub(super) async fn read_message<R: tokio::io::AsyncRead + Unpin, M: Message + Default>(
    reader: &mut R,
    maximum: usize,
) -> Result<M, String> {
    let bytes = arcrelay_transport::read_frame(reader, maximum)
        .await
        .map_err(|error| error.to_string())?;
    M::decode(bytes.as_slice()).map_err(|error| error.to_string())
}

pub(super) fn ensure_ok(status: Option<&proto::Status>, operation: &str) -> Result<(), String> {
    let status = status.ok_or_else(|| format!("{operation} returned no status"))?;
    if status.code == proto::ErrorCode::Ok as i32 {
        Ok(())
    } else {
        Err(if status.message.is_empty() {
            format!("{operation} failed (error code {})", status.code)
        } else {
            status.message.clone()
        })
    }
}

pub(super) fn random_bytes(length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes
}

pub(super) fn take_request_id(next: &mut u64) -> u64 {
    let value = (*next).max(1);
    *next = value.saturating_add(1).max(1);
    value
}

pub(super) fn should_start_connection(
    local_id: &str,
    peer_id: &str,
    paired: bool,
    auto_connect: bool,
    intent: ConnectionIntent,
) -> bool {
    local_id != peer_id
        && match intent {
            ConnectionIntent::UserInitiated => true,
            ConnectionIntent::Automatic => paired && auto_connect && local_id < peer_id,
            ConnectionIntent::AutomaticFallback => paired && auto_connect,
        }
}

pub(super) fn hosts_desktop_control_service(platform: &str) -> bool {
    matches!(
        platform.trim().to_ascii_lowercase().as_str(),
        "macos" | "mac" | "darwin" | "osx" | "windows" | "win32" | "linux"
    )
}

pub(super) fn desktop_pairing_grants() -> Vec<PairingGrantRequest> {
    [
        CapabilityId::ClipboardRead,
        CapabilityId::ClipboardWrite,
        CapabilityId::ClipboardSync,
        CapabilityId::RemoteFilesRead,
        CapabilityId::RemoteFilesWrite,
        CapabilityId::CrossScreenInject,
        CapabilityId::NearbyTransferSend,
        CapabilityId::PrintSubmit,
    ]
    .into_iter()
    .flat_map(|capability| {
        [
            PairingGrantRequest::outbound(capability),
            PairingGrantRequest::inbound(capability),
        ]
    })
    .collect()
}
