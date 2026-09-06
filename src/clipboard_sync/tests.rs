use super::*;

#[tokio::test]
async fn startup_wait_retries_until_runtime_is_available() {
    let mut attempts = 0;
    let runtime = wait_for_available(Duration::from_millis(50), Duration::ZERO, || {
        attempts += 1;
        (attempts == 3).then_some("ready")
    })
    .await;

    assert_eq!(runtime, Some("ready"));
    assert_eq!(attempts, 3);
}

#[test]
fn automatic_discovery_never_initiates_first_pairing() {
    assert!(!should_start_connection(
        "desktop-a",
        "desktop-b",
        false,
        false,
        ConnectionIntent::Automatic,
    ));
    assert!(should_start_connection(
        "desktop-a",
        "desktop-b",
        false,
        false,
        ConnectionIntent::UserInitiated,
    ));
}

#[test]
fn paired_desktops_elect_one_automatic_dialer() {
    assert!(!should_start_connection(
        "desktop-z",
        "desktop-a",
        true,
        true,
        ConnectionIntent::Automatic,
    ));
    assert!(should_start_connection(
        "desktop-a",
        "desktop-z",
        true,
        true,
        ConnectionIntent::Automatic,
    ));
    assert!(!should_start_connection(
        "desktop-a",
        "desktop-a",
        true,
        true,
        ConnectionIntent::Automatic,
    ));
}

#[test]
fn paired_desktops_allow_the_other_side_to_recover_after_the_grace_period() {
    assert!(should_start_connection(
        "desktop-z",
        "desktop-a",
        true,
        true,
        ConnectionIntent::AutomaticFallback,
    ));
    assert!(should_start_connection(
        "desktop-a",
        "desktop-z",
        true,
        true,
        ConnectionIntent::AutomaticFallback,
    ));
    assert!(!should_start_connection(
        "desktop-a",
        "desktop-a",
        true,
        true,
        ConnectionIntent::AutomaticFallback,
    ));
    assert!(!should_start_connection(
        "desktop-z",
        "desktop-a",
        false,
        false,
        ConnectionIntent::AutomaticFallback,
    ));
}

#[test]
fn disabled_auto_connect_blocks_background_dials_but_not_manual_connection() {
    assert!(!should_start_connection(
        "desktop-a",
        "desktop-b",
        true,
        false,
        ConnectionIntent::Automatic,
    ));
    assert!(!should_start_connection(
        "desktop-a",
        "desktop-b",
        true,
        false,
        ConnectionIntent::AutomaticFallback,
    ));
    assert!(should_start_connection(
        "desktop-a",
        "desktop-b",
        true,
        false,
        ConnectionIntent::UserInitiated,
    ));
}

#[test]
fn only_desktop_platforms_host_the_desktop_control_service() {
    for platform in ["macos", "MAC", "darwin", "osx", "windows", "Win32", "linux"] {
        assert!(
            hosts_desktop_control_service(platform),
            "expected {platform} to be a desktop control host"
        );
    }
    for platform in ["ios", "iPadOS", "android", "", "unknown"] {
        assert!(
            !hosts_desktop_control_service(platform),
            "expected {platform} to be rejected as a desktop control host"
        );
    }
}

fn clipboard_version_record() -> ClipboardSyncRecord {
    ClipboardSyncRecord {
        sync_id: "sync-version".into(),
        kind: ClipboardContentKind::Text,
        text: Some("payload".into()),
        html: None,
        rtf: None,
        image_png: None,
        width: None,
        height: None,
        preview: "payload".into(),
        source_app: Some("Tests".into()),
        source_device_id: "source".into(),
        source_device_name: "Source".into(),
        captured_at_ms: 1,
        revision: 4,
        updated_by_device_id: "device-b".into(),
        favorite: true,
        favorite_revision: 2,
        favorite_updated_by_device_id: "device-b".into(),
        labels: vec![ClipboardLabel {
            id: "label-a".into(),
            name: "Label".into(),
            color: "#fff".into(),
            revision: 3,
            updated_by_device_id: "device-b".into(),
            deleted: false,
        }],
        label_memberships: vec![ClipboardLabelMembership {
            label_id: "label-a".into(),
            attached: true,
            revision: 5,
            updated_by_device_id: "device-b".into(),
        }],
        deleted: false,
        change_kind: ClipboardSyncChangeKind::Snapshot,
        live: false,
        text_syntax: ClipboardTextSyntax::Plain,
    }
}

#[test]
fn remote_clipboard_manifest_skips_records_already_covered() {
    let record = clipboard_version_record();
    let wire = wire_record(
        record.clone(),
        Some(proto::clipboard_sync_record::Payload::Text(
            "payload".into(),
        )),
    )
    .unwrap();
    let remote = RemoteClipboardVersion::from_wire(&wire);

    assert!(remote.has_current_content(&record));
    assert!(remote.covers(&record));

    let mut newer_membership = record;
    newer_membership.label_memberships[0].revision += 1;
    assert!(remote.has_current_content(&newer_membership));
    assert!(!remote.covers(&newer_membership));
}

async fn test_network_runtime(
    directory: &Path,
    name: &str,
) -> Arc<arcrelay_network::NetworkRuntime> {
    let repository: Arc<dyn arcrelay_peer::PeerRepository> =
        Arc::new(arcrelay_peer::InMemoryPeerRepository::default());
    let mut config = arcrelay_network::NetworkRuntimeConfig::new(
        directory.to_path_buf(),
        arcrelay_network::DeviceMetadata {
            name: name.into(),
            platform: "test".into(),
            model: "test".into(),
        },
        repository,
    );
    config.listen_address = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
    arcrelay_network::NetworkRuntime::bind(config)
        .await
        .unwrap()
}

fn test_advertisement(
    runtime: &arcrelay_network::NetworkRuntime,
) -> arcrelay_network::PeerAdvertisement {
    let address = std::net::SocketAddr::new(
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        runtime.local_port().unwrap(),
    );
    arcrelay_network::PeerAdvertisement {
        device_id: runtime.device_id(),
        public_key: runtime.public_key(),
        metadata: runtime.metadata(),
        addresses: vec![address.ip()],
        connection_addresses: vec![address],
        port: address.port(),
        certificate_sha256: runtime.certificate_sha256(),
        last_seen_at_ms: 1,
    }
}

struct ReverseFileProvider;

#[async_trait::async_trait]
impl RemoteFileProvider for ReverseFileProvider {
    async fn list_shares(
        &self,
    ) -> arcrelay_protocol::remote_files::RemoteFileResult<
        Vec<arcrelay_protocol::remote_files::RemoteFileShare>,
    > {
        Ok(vec![arcrelay_protocol::remote_files::RemoteFileShare {
            id: "left-share".into(),
            name: "Left Share".into(),
            writable: false,
        }])
    }

    async fn list_directory(
        &self,
        _: &str,
        _: &str,
        _: Option<&str>,
        _: u32,
        _: Option<&str>,
        _: arcrelay_protocol::remote_files::RemoteFileSortKey,
        _: arcrelay_protocol::remote_files::RemoteFileSortDirection,
    ) -> arcrelay_protocol::remote_files::RemoteFileResult<
        arcrelay_protocol::remote_files::RemoteFileDirectoryPage,
    > {
        Err(test_remote_file_error())
    }

    async fn create_directory(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> arcrelay_protocol::remote_files::RemoteFileResult<
        arcrelay_protocol::remote_files::RemoteFileEntry,
    > {
        Err(test_remote_file_error())
    }

    async fn rename(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> arcrelay_protocol::remote_files::RemoteFileResult<
        arcrelay_protocol::remote_files::RemoteFileEntry,
    > {
        Err(test_remote_file_error())
    }

    async fn delete(
        &self,
        _: &str,
        _: &str,
    ) -> arcrelay_protocol::remote_files::RemoteFileResult<()> {
        Err(test_remote_file_error())
    }

    async fn prepare_download(
        &self,
        _: &str,
        _: &str,
    ) -> arcrelay_protocol::remote_files::RemoteFileResult<
        arcrelay_protocol::remote_files::RemoteFileDownload,
    > {
        Err(test_remote_file_error())
    }

    async fn prepare_upload(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: u64,
        _: bool,
        _: Option<i64>,
    ) -> arcrelay_protocol::remote_files::RemoteFileResult<
        arcrelay_protocol::remote_files::RemoteFileUpload,
    > {
        Err(test_remote_file_error())
    }
}

fn test_remote_file_error() -> arcrelay_protocol::remote_files::RemoteFileError {
    arcrelay_protocol::remote_files::RemoteFileError::new(
        arcrelay_protocol::remote_files::RemoteFileErrorCode::Internal,
        "not used",
    )
}

#[tokio::test]
async fn responder_can_browse_initiator_files_over_the_same_control_connection() {
    let root = tempfile::tempdir().unwrap();
    let left = test_network_runtime(&root.path().join("left"), "Left").await;
    let right = test_network_runtime(&root.path().join("right"), "Right").await;
    let mut right_incoming = right.subscribe();

    let left_pairing = left
        .connect(&test_advertisement(&right), SessionKind::Pairing)
        .await
        .unwrap();
    let right_pairing = tokio::time::timeout(Duration::from_secs(2), right_incoming.recv())
        .await
        .unwrap()
        .unwrap();
    left.confirm_pairing(&left_pairing).await.unwrap();
    right.confirm_pairing(&right_pairing).await.unwrap();
    left_pairing.close("pairing complete");

    let left_control = left
        .connect(&test_advertisement(&right), SessionKind::Control)
        .await
        .unwrap();
    let right_control = tokio::time::timeout(Duration::from_secs(2), right_incoming.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(left_control.initiator_id(), &left.device_id());
    assert_eq!(right_control.initiator_id(), &left.device_id());

    let reverse_server = tokio::spawn(manager::serve_reverse_remote_file_streams(
        left_control.transport_handle(),
        Arc::new(ReverseFileProvider),
        RemoteFileAccess::from_grants(
            &[arcrelay_peer::Grant {
                peer_id: right.device_id(),
                capability: CapabilityId::RemoteFilesRead,
                direction: arcrelay_peer::GrantDirection::Inbound,
                constraints: arcrelay_peer::GrantConstraints::None,
                granted_at_ms: 1,
            }],
            arcrelay_peer::GrantDirection::Inbound,
        ),
    ));
    let registry = ConnectionRegistry::new();
    let negotiated_features = HashMap::from([(
        proto::Feature::RemoteFiles as i32,
        arcrelay_protocol::remote_files::REMOTE_FILE_PROTOCOL_VERSION,
    )]);
    let disconnect = registry
        .register_transport_with_features(
            left.device_id().as_str(),
            right_control.transport_handle(),
            negotiated_features,
        )
        .await;
    let connection = registry
        .transport_for_feature(
            left.device_id().as_str(),
            proto::Feature::RemoteFiles as i32,
            arcrelay_protocol::remote_files::REMOTE_FILE_PROTOCOL_VERSION,
        )
        .await
        .expect("responder should expose the authenticated transport");

    let response = tokio::time::timeout(
        Duration::from_secs(2),
        remote_file_request_on_connection(&connection, RemoteFileRequest::ListShares),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.shares.len(), 1);
    assert_eq!(response.shares[0].id, "left-share");

    registry
        .unregister(left.device_id().as_str(), &disconnect)
        .await;
    left_control.close("test complete");
    reverse_server.abort();
}

#[test]
fn clipboard_record_rejection_requires_explicit_invalid_argument_status() {
    assert!(!transfer::clipboard_record_rejected(None));
    for code in [
        proto::ErrorCode::Ok as i32,
        proto::ErrorCode::Internal as i32,
        999,
    ] {
        assert!(!transfer::clipboard_record_rejected(Some(&proto::Status {
            code,
            message: "invalid clipboard sync metadata".into(),
            ..Default::default()
        })));
    }
    assert!(transfer::clipboard_record_rejected(Some(&proto::Status {
        code: proto::ErrorCode::InvalidArgument as i32,
        message: "a different validation message".into(),
        ..Default::default()
    })));
}

#[tokio::test]
async fn rejected_clipboard_record_preserves_stream_for_next_record_and_ping() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let root = tempfile::tempdir().unwrap();
        let left = test_network_runtime(&root.path().join("left"), "Left").await;
        let right = test_network_runtime(&root.path().join("right"), "Right").await;
        let mut inbound = right.subscribe();
        let outgoing = left
            .connect(&test_advertisement(&right), SessionKind::Pairing)
            .await
            .unwrap();
        let incoming = inbound.recv().await.unwrap();
        let connection = outgoing.transport_handle();
        let remote = incoming.transport_handle();
        let server = tokio::spawn(async move {
            let (mut send, mut recv) = remote.accept_bi().await.unwrap();
            for code in [proto::ErrorCode::InvalidArgument, proto::ErrorCode::Ok] {
                let frame: proto::ClientControlFrame =
                    read_message(&mut recv, MAX_CONTROL_FRAME_SIZE)
                        .await
                        .unwrap();
                let Some(proto::client_control_frame::Body::Request(request)) = frame.body else {
                    panic!("expected clipboard request")
                };
                let response = proto::ServerControlFrame {
                    body: Some(proto::server_control_frame::Body::Response(
                        proto::Response {
                            request_id: request.request_id,
                            status: Some(proto::Status {
                                code: code as i32,
                                message: "validation response".into(),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    )),
                };
                write_message(&mut send, &response, MAX_CONTROL_FRAME_SIZE)
                    .await
                    .unwrap();
            }
            let frame: proto::ClientControlFrame = read_message(&mut recv, MAX_CONTROL_FRAME_SIZE)
                .await
                .unwrap();
            assert!(matches!(
                frame.body,
                Some(proto::client_control_frame::Body::Ping(_))
            ));
            // Hold the stream open until the client confirms it remains usable.
            recv.read_u8().await.unwrap();
        });
        let (mut send, mut recv) = connection.open_bi().await.unwrap();
        let (tx, mut rx) = mpsc::channel(8);
        let reader = tokio::spawn(async move {
            while let Ok(frame) = recv_server_frame(&mut recv, Duration::from_secs(2)).await {
                if tx.send(frame).await.is_err() {
                    break;
                }
            }
        });
        let mut next_request_id = 1;
        let mut pending = VecDeque::new();
        let mut responses = HashMap::new();
        let mut sync = ClipboardDownloadSession {
            connection: &connection,
            send: &mut send,
            next_request_id: &mut next_request_id,
            pending: &mut pending,
            responses: &mut responses,
            incoming: &mut rx,
        };
        send_sync_record(&mut sync, clipboard_version_record(), true)
            .await
            .unwrap();
        send_sync_record(&mut sync, clipboard_version_record(), true)
            .await
            .unwrap();
        send_client_frame(
            &mut send,
            proto::ClientControlFrame {
                body: Some(proto::client_control_frame::Body::Ping(proto::Ping {
                    request_id: 3,
                    monotonic_elapsed_us: 0,
                })),
            },
        )
        .await
        .unwrap();
        assert!(connection.close_reason().is_none());
        send.write_u8(1).await.unwrap();
        server.await.unwrap();
        reader.abort();
        left.shutdown("test complete");
        right.shutdown("test complete");
    })
    .await
    .unwrap();
}
