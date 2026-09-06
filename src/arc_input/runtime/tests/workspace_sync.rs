use super::*;
use arcrelay_network::{DeviceMetadata, NetworkRuntimeConfig, SessionKind};

async fn node(root: &std::path::Path, name: &str) -> Arc<ArcInputRuntime> {
    let mut config = NetworkRuntimeConfig::new(
        root.join(name).join("identity"),
        DeviceMetadata {
            name: name.into(),
            platform: "test".into(),
            model: "test".into(),
        },
        Arc::new(arcrelay_peer::InMemoryPeerRepository::default()),
    );
    config.listen_address = "127.0.0.1".parse().unwrap();
    let network = NetworkRuntime::bind(config).await.unwrap();
    ArcInputRuntime::load(
        ProductPaths::from_root(root.join(name).join("input")),
        Arc::new(ProductIdentity::from_device_id(network.device_id().to_string()).unwrap()),
        Arc::new(tokio::sync::OnceCell::new_with(Some(network))),
    )
    .await
    .unwrap()
}

async fn connected_runtimes(
    root: &std::path::Path,
) -> (Arc<ArcInputRuntime>, Arc<ArcInputRuntime>) {
    let left = node(root, "left").await;
    let right = node(root, "right").await;
    let left_network = left.unified_network.get().unwrap();
    let right_network = right.unified_network.get().unwrap();
    let mut incoming = right_network.subscribe();
    let right_identity =
        arcrelay_network::DeviceIdentity::load_or_create(&root.join("right/identity")).unwrap();
    let advertisement = arcrelay_network::PeerAdvertisement {
        device_id: right_network.device_id(),
        public_key: right_identity.public_key_value(),
        metadata: right_network.metadata(),
        addresses: vec!["127.0.0.1".parse().unwrap()],
        connection_addresses: vec![SocketAddr::from((
            [127, 0, 0, 1],
            right_network.local_port().unwrap(),
        ))],
        port: right_network.local_port().unwrap(),
        certificate_sha256: right_identity.certificate_sha256(),
        last_seen_at_ms: 1,
    };
    let pairing = left_network
        .connect(&advertisement, SessionKind::Pairing)
        .await
        .unwrap();
    let remote_pairing = tokio::time::timeout(Duration::from_secs(5), incoming.recv())
        .await
        .unwrap()
        .unwrap();
    left_network.confirm_pairing(&pairing).await.unwrap();
    right_network
        .confirm_pairing(&remote_pairing)
        .await
        .unwrap();
    for (network, peer_id) in [
        (left_network, right_network.device_id()),
        (right_network, left_network.device_id()),
    ] {
        network
            .grant(arcrelay_peer::Grant {
                peer_id,
                capability: arcrelay_peer::CapabilityId::CrossScreenInject,
                direction: arcrelay_peer::GrantDirection::Inbound,
                constraints: arcrelay_peer::GrantConstraints::None,
                granted_at_ms: 1,
            })
            .await
            .unwrap();
    }
    left.network
        .connect(right.identity.service_instance_id.clone(), &[], 0)
        .await
        .unwrap();
    // Retain the incoming session while the second hub attaches to it.
    let _remote_input = tokio::time::timeout(Duration::from_secs(5), incoming.recv())
        .await
        .unwrap()
        .unwrap();
    right
        .network
        .connect(left.identity.service_instance_id.clone(), &[], 0)
        .await
        .unwrap();
    (left, right)
}

fn configure(
    runtime: &ArcInputRuntime,
    displays: Vec<arcrelay_input::DisplaySurface>,
    revision: u64,
) {
    *write(&runtime.local_inventory) = Some(DisplayInventory {
        device_id: runtime.identity.service_instance_id.clone(),
        revision: InventoryRevision(1),
        displays: displays
            .iter()
            .filter(|display| display.device_id == runtime.identity.service_instance_id)
            .cloned()
            .collect(),
    });
    let mut configuration = runtime.store.snapshot();
    configuration.input_sharing_enabled = true;
    configuration.topology_author = runtime.identity.service_instance_id.to_string();
    configuration.layout = Some(WorkspaceLayout {
        workspace_id: WorkspaceId::parse("desk").unwrap(),
        revision: TopologyRevision(revision),
        displays: displays
            .into_iter()
            .map(|display| (display.display_id.clone(), display))
            .collect(),
        portals: Vec::new(),
    });
    rebuild_auto_portals(&mut configuration).unwrap();
    runtime.store.save(configuration).unwrap();
}

async fn control_frame(receiver: &mut broadcast::Receiver<NetworkEvent>) -> proto::ControlFrame {
    let event = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
        .await
        .expect("control metadata must arrive without waiting for an ownership grant")
        .unwrap();
    let NetworkEvent::Control { frame, .. } = event else {
        panic!("expected control frame, got {event:?}")
    };
    frame
}

#[tokio::test]
async fn obsolete_transport_events_cannot_reset_reconnected_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let (source, target) = connected_runtimes(directory.path()).await;
    let peer = target.identity.service_instance_id.clone();
    write(&source.remote_sharing_enabled).insert(peer.clone(), true);
    let inventory = DisplayInventory {
        device_id: peer.clone(),
        revision: InventoryRevision(9),
        displays: vec![test_display("remote", &peer, 500_000)],
    };
    write(&source.remote_inventories).insert(peer.clone(), inventory);
    for event in [
        NetworkEvent::Disconnected {
            service_instance_id: peer.clone(),
            session_id: 0,
            reason: "old session closed".into(),
        },
        NetworkEvent::Connected {
            service_instance_id: peer.clone(),
            session_id: 0,
        },
        NetworkEvent::Control {
            service_instance_id: peer.clone(),
            session_id: 0,
            frame: proto::ControlFrame { body: None },
        },
        NetworkEvent::Input {
            service_instance_id: peer.clone(),
            session_id: 0,
            batch: proto::InputEventBatch::default(),
        },
    ] {
        source.handle_network_event(event).await.unwrap();
    }
    assert_eq!(read(&source.remote_sharing_enabled).get(&peer), Some(&true));
    assert_eq!(
        read(&source.remote_inventories)[&peer].revision,
        InventoryRevision(9)
    );
    source
        .unified_network
        .get()
        .unwrap()
        .shutdown("test complete");
    target
        .unified_network
        .get()
        .unwrap()
        .shutdown("test complete");
}

async fn pair_input_nodes(left: &ArcInputRuntime, right: &ArcInputRuntime) {
    use arcrelay_peer::{CapabilityId, Grant, GrantConstraints, GrantDirection};
    let left = left.unified_network.get().unwrap();
    let right = right.unified_network.get().unwrap();
    let mut incoming = right.subscribe();
    let peer = arcrelay_network::PeerAdvertisement {
        device_id: right.device_id(),
        public_key: right.public_key(),
        metadata: right.metadata(),
        addresses: vec!["127.0.0.1".parse().unwrap()],
        connection_addresses: vec![SocketAddr::from((
            [127, 0, 0, 1],
            right.local_port().unwrap(),
        ))],
        port: right.local_port().unwrap(),
        certificate_sha256: right.certificate_sha256(),
        last_seen_at_ms: 1,
    };
    let pair = left.connect(&peer, SessionKind::Pairing).await.unwrap();
    let remote_pair = incoming.recv().await.unwrap();
    for (network, session, peer) in [
        (left, &pair, right.device_id()),
        (right, &remote_pair, left.device_id()),
    ] {
        network
            .confirm_pairing_with_grants(
                session,
                vec![Grant {
                    peer_id: peer,
                    capability: CapabilityId::CrossScreenInject,
                    direction: GrantDirection::Inbound,
                    constraints: GrantConstraints::None,
                    granted_at_ms: 1,
                }],
            )
            .await
            .unwrap();
    }
    pair.close("pairing complete");
}

#[tokio::test]
async fn three_peer_partial_mesh_without_relay_support_recovers_on_timer() {
    check_three_peer_input_mesh(false, false).await;
}

#[tokio::test]
async fn three_peer_chain_relays_end_to_end_input_without_a_direct_link() {
    check_three_peer_input_mesh(true, false).await;
}

#[tokio::test]
async fn three_peer_chain_closes_relays_when_target_sharing_is_disabled() {
    check_three_peer_input_mesh(true, true).await;
}

async fn check_three_peer_input_mesh(use_relay: bool, disable_target: bool) {
    let directory = tempfile::tempdir().unwrap();
    let (left, middle) = connected_runtimes(directory.path()).await;
    let right = node(directory.path(), "third").await;
    pair_input_nodes(&left, &right).await;
    pair_input_nodes(&middle, &right).await;
    let nodes = [&left, &middle, &right];
    let displays = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            test_display(
                &format!("screen-{index}"),
                &node.identity.service_instance_id,
                index as i64 * 500_000,
            )
        })
        .collect::<Vec<_>>();
    let mut workers = Vec::new();
    for runtime in nodes {
        configure(runtime, displays.clone(), 80);
        let mut capabilities = InputCapturePort::capabilities(runtime.platform.as_ref());
        capabilities.can_inject_absolute_pointer = true;
        capabilities.can_inject_keyboard = true;
        *write(&runtime.availability_capabilities) = Some(capabilities);
        // Hold automatic dialing until the partial topology has converged.
        // This flag only controls new outgoing connections, not metadata.
        for mut record in runtime
            .unified_network
            .get()
            .unwrap()
            .paired_peers()
            .await
            .unwrap()
        {
            record.auto_connect = false;
            write(&runtime.paired_peers).insert(
                ServiceInstanceId::parse(record.device_id.to_string()).unwrap(),
                record,
            );
        }
        let mut events = runtime.network.subscribe();
        workers.push(tokio::spawn({
            let runtime = runtime.clone();
            async move {
                while let Ok(mut event) = events.recv().await {
                    if let NetworkEvent::Control { frame, .. } = &mut event {
                        if let Some(proto::control_frame::Body::CapabilitySnapshot(caps)) =
                            &mut frame.body
                        {
                            caps.can_inject_absolute_pointer = true;
                            caps.can_inject_keyboard = true;
                            // Keep the direct-recovery case equivalent to older
                            // peers. Otherwise the normal two-second fallback
                            // can legitimately recover through a relay first.
                            caps.supports_input_relay = use_relay;
                        }
                    }
                    runtime.handle_network_event(event).await.unwrap();
                }
            }
        }));
        let hub = runtime.network.clone();
        let incoming = runtime.unified_network.get().unwrap().subscribe();
        workers.push(tokio::spawn(async move {
            hub.run_with_incoming(incoming).await.unwrap();
        }));
    }
    middle
        .network
        .connect(right.identity.service_instance_id.clone(), &[], 0)
        .await
        .unwrap();
    left.send_workspace_sync(&middle.identity.service_instance_id)
        .await
        .unwrap();
    middle
        .send_workspace_sync(&left.identity.service_instance_id)
        .await
        .unwrap();
    let retained = left
        .unified_network
        .get()
        .unwrap()
        .connect_discovered(
            &middle.unified_network.get().unwrap().device_id(),
            SessionKind::RealtimeInput,
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while left.input_component().len() != 2
            || middle.input_component().len() != 3
            || right.input_component().len() != 2
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("partial mesh metadata must converge");
    assert_eq!(left.network.connected_peers().len(), 1);
    assert!(
        lock(&left.reconnect_attempts).is_empty(),
        "disabled automatic dialing must stay disabled"
    );

    if use_relay {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !left
                .network
                .has_relay_route(&right.identity.service_instance_id)
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the middle peer must advertise its direct input link");
        // An authenticated peer cannot ask B to forward across workspaces or
        // treat an already forwarded request as another forwarding hop.
        for (workspace, forwarded) in [("other-desk", false), ("desk", true)] {
            use prost::Message;
            use tokio::io::AsyncWriteExt;
            let (mut send, mut receive) = retained.transport_handle().open_bi().await.unwrap();
            send.write_u8(arcrelay_wire::STREAM_KIND_INPUT_RELAY)
                .await
                .unwrap();
            let request = arcrelay_wire::common::InputRelayOpen {
                version: 1,
                workspace_id: workspace.into(),
                source_device_id: left.identity.service_instance_id.to_string(),
                target_device_id: right.identity.service_instance_id.to_string(),
                forwarded,
            };
            arcrelay_transport::write_frame(
                &mut send,
                &request.encode_to_vec(),
                arcrelay_wire::MAX_INPUT_RELAY_HEADER_SIZE,
            )
            .await
            .unwrap();
            let reply = arcrelay_transport::read_frame(
                &mut receive,
                arcrelay_wire::MAX_INPUT_RELAY_HEADER_SIZE,
            )
            .await
            .unwrap();
            assert!(
                !arcrelay_wire::common::InputRelayReady::decode(reply.as_slice())
                    .unwrap()
                    .accepted
            );
            assert!(retained.transport_handle().close_reason().is_none());
        }
        // Keep every automatic A-C dial disabled. All A-C control and input
        // must pass through the existing A-B and B-C QUIC streams.
        left.connect_with_relay_fallback(&right.identity.service_instance_id, std::future::pending)
            .await
            .unwrap();
    } else {
        assert!(
            nodes.iter().all(|node| !node
                .network
                .has_relay_route(&right.identity.service_instance_id)),
            "peers without negotiated relay support must use direct recovery"
        );
        // The normal inventory timer reloads preferences and retries even
        // when no mDNS or route snapshot changes. The passive side also retries.
        left.refresh_paired_peers().await.unwrap();
    }
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            if !use_relay {
                left.reconnect_peers();
            }
            if nodes.iter().all(|node| node.input_component().len() == 3) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("timer reconciliation must repair the missing third link");
    assert!(retained.transport_handle().close_reason().is_none());
    let recovered = left
        .unified_network
        .get()
        .unwrap()
        .connect_remembered(
            &right.unified_network.get().unwrap().device_id(),
            SessionKind::RealtimeInput,
        )
        .await
        .unwrap();
    assert_eq!(recovered.is_relayed(), use_relay);
    assert_eq!(
        recovered.peer().public_key,
        right.unified_network.get().unwrap().public_key()
    );

    let left_id = left.identity.service_instance_id.clone();
    let layout = left.store.snapshot().layout.unwrap();
    left.arbiter
        .acquire(layout.workspace_id.clone(), left_id.clone())
        .unwrap();
    *lock(&left.control_participants) = left.input_component();
    *lock(&left.session) = Some(ControlSession {
        workspace_id: layout.workspace_id,
        controller: left_id.clone(),
        current_target: left_id.clone(),
        current_display: DisplayId::parse("screen-0").unwrap(),
        topology_revision: layout.revision,
        control_epoch: ControlEpoch(91),
        next_sequence: 1,
        state: SessionState::Local,
        held: HeldInputState::default(),
    });
    for target in [&middle, &right] {
        left.send_acquire_control(&target.identity.service_instance_id)
            .await
            .unwrap();
        let session = lock(&target.session).clone().unwrap();
        assert_eq!(session.controller, left_id);
        assert_eq!(session.control_epoch, ControlEpoch(91));
        assert_eq!(lock(&target.control_participants).len(), 3);
    }
    if use_relay {
        let header = proto::RuntimeHeader {
            workspace_id: "desk".into(),
            topology_revision: lock(&right.session).as_ref().unwrap().topology_revision.0,
            control_epoch: 91,
            source_device_id: left_id.to_string(),
            target_device_id: right.identity.service_instance_id.to_string(),
            sequence: 1,
        };
        // An empty valid input batch exercises the actual reliable stream and
        // source/epoch/sequence validation without injecting into the test OS.
        left.network
            .send_input(
                &right.identity.service_instance_id,
                proto::InputEventBatch {
                    header: Some(header),
                    ..Default::default()
                },
                false,
            )
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while lock(&right.session).as_ref().unwrap().next_sequence != 2 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the authenticated source's input must reach C through B");
        // C's physical takeover uses the same compare-and-swap epoch across
        // the relay; the reverse input direction must remain authenticated as C.
        let right_id = right.identity.service_instance_id.clone();
        let previous = lock(&right.session).clone().unwrap();
        right
            .arbiter
            .transfer(right.arbiter.current().unwrap().epoch, right_id.clone())
            .unwrap();
        *lock(&right.control_participants) = right.input_component();
        *lock(&right.session) = Some(ControlSession {
            controller: right_id.clone(),
            current_target: right_id.clone(),
            current_display: DisplayId::parse("screen-2").unwrap(),
            control_epoch: ControlEpoch(92),
            state: SessionState::Local,
            next_sequence: 1,
            held: HeldInputState::default(),
            ..previous
        });
        for target in [&left, &middle] {
            lock(&right.takeover_pending_peers).insert(
                target.identity.service_instance_id.clone(),
                ControlEpoch(91),
            );
            right
                .send_acquire_control(&target.identity.service_instance_id)
                .await
                .unwrap();
            assert_eq!(lock(&target.session).as_ref().unwrap().controller, right_id);
            assert_eq!(
                lock(&target.session).as_ref().unwrap().control_epoch,
                ControlEpoch(92)
            );
        }
        let reverse_header = proto::RuntimeHeader {
            workspace_id: "desk".into(),
            topology_revision: lock(&left.session).as_ref().unwrap().topology_revision.0,
            control_epoch: 92,
            source_device_id: right_id.to_string(),
            target_device_id: left_id.to_string(),
            sequence: 1,
        };
        right
            .network
            .send_input(
                &left_id,
                proto::InputEventBatch {
                    header: Some(reverse_header),
                    ..Default::default()
                },
                false,
            )
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while lock(&left.session).as_ref().unwrap().next_sequence != 2 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("C's input must return to A through B");
        let stale_revision = lock(&right.session).as_ref().unwrap().topology_revision.0;
        assert!(right
            .handle_input(
                left_id.clone(),
                proto::InputEventBatch {
                    header: Some(proto::RuntimeHeader {
                        workspace_id: "desk".into(),
                        topology_revision: stale_revision,
                        control_epoch: 91,
                        source_device_id: left_id.to_string(),
                        target_device_id: right_id.to_string(),
                        sequence: 2,
                    }),
                    ..Default::default()
                }
            )
            .is_err());
        // Losing one relay leg must fail closed at both endpoints immediately,
        // even while the other direct parent connection remains alive.
        if disable_target {
            right.set_input_sharing_enabled(false).unwrap();
        } else {
            middle.network.disconnect(
                &right.identity.service_instance_id,
                "simulate relay leg loss",
            );
        }
        tokio::time::timeout(Duration::from_secs(3), async {
            while recovered.transport_handle().close_reason().is_none()
                || right.network.connected_peers().contains(&left_id)
                || lock(&right.session).is_some()
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("relay failure must retire input ownership at both ends");
        assert!(retained.transport_handle().close_reason().is_none());
    }
    for worker in workers {
        worker.abort();
    }
    for node in nodes {
        node.unified_network
            .get()
            .unwrap()
            .shutdown("test complete");
    }
}

#[tokio::test]
async fn acquisition_repairs_stale_layout_before_requesting_ownership() {
    let directory = tempfile::tempdir().unwrap();
    let (source, target) = connected_runtimes(directory.path()).await;
    let source_id = source.identity.service_instance_id.clone();
    let target_id = target.identity.service_instance_id.clone();
    let source_screen = test_display("a", &source_id, 0);
    let target_screen = test_display("b", &target_id, 500_000);
    configure(&target, vec![target_screen.clone()], 1);
    configure(&source, vec![source_screen, target_screen], 80);
    *lock(&source.session) = Some(ControlSession {
        workspace_id: WorkspaceId::parse("desk").unwrap(),
        controller: source_id.clone(),
        current_target: source_id.clone(),
        current_display: DisplayId::parse("a").unwrap(),
        topology_revision: TopologyRevision(80),
        control_epoch: ControlEpoch(7),
        next_sequence: 1,
        state: SessionState::Local,
        held: HeldInputState::default(),
    });
    let mut received = target.network.subscribe();
    let acquisition = tokio::spawn({
        let source = source.clone();
        async move { source.send_acquire_control(&target_id).await }
    });
    for index in 0..3 {
        let frame = control_frame(&mut received).await;
        match (&frame.body, index) {
            (Some(proto::control_frame::Body::CapabilitySnapshot(_)), 0)
            | (Some(proto::control_frame::Body::DisplayInventory(_)), 1) => {}
            (Some(proto::control_frame::Body::WorkspaceTopology(topology)), 2) => {
                assert_eq!(topology.header.as_ref().unwrap().control_epoch, 0);
            }
            _ => panic!("metadata must precede acquisition, got {frame:?}"),
        }
        target
            .handle_control(source_id.clone(), frame)
            .await
            .unwrap();
    }
    assert!(same_shared_layout(
        &source.store.snapshot(),
        &target.store.snapshot()
    ));
    assert!(lock(&target.session).is_none());
    assert!(target.arbiter.current().is_none());
    assert!(matches!(
        control_frame(&mut received).await.body,
        Some(proto::control_frame::Body::AcquireControl(_))
    ));
    acquisition.abort();
    let _ = acquisition.await;
}

#[tokio::test]
async fn component_rejection_returns_newer_metadata_without_granting_ownership() {
    for target_can_inject in [false, true] {
        check_component_rejection_recovery(target_can_inject).await;
    }
}

async fn check_component_rejection_recovery(target_can_inject: bool) {
    let directory = tempfile::tempdir().unwrap();
    let (source, target) = connected_runtimes(directory.path()).await;
    let source_id = source.identity.service_instance_id.clone();
    let target_id = target.identity.service_instance_id.clone();
    let source_screen = test_display("a", &source_id, 0);
    configure(&source, vec![source_screen.clone()], 1);
    configure(
        &target,
        vec![
            source_screen,
            test_display("b", &target_id, 500_000),
            test_display(
                "c",
                &ServiceInstanceId::parse("offline").unwrap(),
                1_000_000,
            ),
        ],
        80,
    );
    let mut received = source.network.subscribe();
    target
        .handle_control(
            source_id.clone(),
            proto::ControlFrame {
                body: Some(proto::control_frame::Body::AcquireControl(
                    proto::AcquireControl {
                        header: Some(proto::RuntimeHeader {
                            workspace_id: "desk".into(),
                            topology_revision: 1,
                            control_epoch: 7,
                            source_device_id: source_id.to_string(),
                            target_device_id: target_id.to_string(),
                            sequence: 1,
                        }),
                        expected_control_epoch: 0,
                        participant_device_ids: vec![source_id.to_string(), target_id.to_string()],
                    },
                )),
            },
        )
        .await
        .unwrap();
    for _ in 0..3 {
        let mut frame = control_frame(&mut received).await;
        if let Some(proto::control_frame::Body::CapabilitySnapshot(capabilities)) = &mut frame.body
        {
            // Cover both permission states independently of the test runner's
            // native accessibility permission (not granted on headless CI).
            capabilities.can_inject_absolute_pointer = target_can_inject;
            capabilities.can_inject_keyboard = target_can_inject;
        }
        assert!(!matches!(
            frame.body,
            Some(proto::control_frame::Body::ControlGranted(_))
        ));
        source
            .handle_control(target_id.clone(), frame)
            .await
            .unwrap();
    }
    let Some(proto::control_frame::Body::ControlRevoked(revoked)) =
        control_frame(&mut received).await.body
    else {
        panic!("the mismatched claim must still be rejected")
    };
    assert!(revoked
        .reason
        .contains("online screen component has changed"));
    assert!(same_shared_layout(
        &source.store.snapshot(),
        &target.store.snapshot()
    ));
    assert!(lock(&target.session).is_none());
    assert!(target.arbiter.current().is_none());
    // A received layout cannot make an actually disconnected screen routable.
    let configuration = source.store.snapshot();
    assert_eq!(
        source.display_availability(&configuration)[&DisplayId::parse("c").unwrap()],
        DisplayAvailability::Offline
    );
    assert!(!source
        .input_component()
        .contains(&ServiceInstanceId::parse("offline").unwrap()));
    let offline_portals = configuration
        .layout
        .as_ref()
        .unwrap()
        .portals
        .iter()
        .filter(|portal| {
            portal.source_display.as_str() == "c" || portal.target_display.as_str() == "c"
        })
        .collect::<Vec<_>>();
    assert!(!offline_portals.is_empty());
    let expected_status = if target_can_inject {
        PortalStatus::SuspendedOffline
    } else {
        PortalStatus::UnsupportedCapability
    };
    assert!(offline_portals
        .iter()
        .all(|portal| portal.status == expected_status));
}

#[tokio::test]
async fn application_receiver_visibility_does_not_block_desktop_ownership() {
    for source_sees_app in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let (source, target) = connected_runtimes(directory.path()).await;
        let source_id = source.identity.service_instance_id.clone();
        let target_id = target.identity.service_instance_id.clone();
        let phone = ServiceInstanceId::parse("phone").unwrap();
        let displays = vec![
            test_display("a", &source_id, 0),
            test_display("b", &target_id, 500_000),
            test_display("c", &phone, 1_000_000),
        ];
        for (runtime, app_available) in [(&source, source_sees_app), (&target, !source_sees_app)] {
            configure(runtime, displays.clone(), 7);
            write(&runtime.remote_capabilities).insert(
                phone.clone(),
                PlatformCapabilities {
                    can_inject_app_pointer: true,
                    ..Default::default()
                },
            );
            if !app_available {
                let mut config = runtime.store.snapshot();
                for portal in &mut config.layout.as_mut().unwrap().portals {
                    if portal.source_display.as_str() == "c"
                        || portal.target_display.as_str() == "c"
                    {
                        portal.status = PortalStatus::SuspendedOffline;
                    }
                }
                runtime.store.save(config).unwrap();
            }
        }
        assert_ne!(source.input_component(), target.input_component());
        assert_eq!(
            source.input_control_component(),
            target.input_control_component()
        );
        let session = ControlSession {
            workspace_id: WorkspaceId::parse("desk").unwrap(),
            controller: source_id.clone(),
            current_target: source_id.clone(),
            current_display: DisplayId::parse("a").unwrap(),
            topology_revision: TopologyRevision(7),
            control_epoch: ControlEpoch(8),
            next_sequence: 1,
            state: SessionState::Local,
            held: HeldInputState::default(),
        };
        let layout = source.store.snapshot().layout.unwrap();
        if source_sees_app {
            let (app_claim, _) = source.control_acquisition_frames(&phone, &session, &layout, 0);
            let Some(proto::control_frame::Body::AcquireControl(claim)) = app_claim.body else {
                panic!("claim")
            };
            assert!(claim.participant_device_ids.contains(&phone.to_string()));
        }
        let (claim, _) = source.control_acquisition_frames(&target_id, &session, &layout, 0);
        let mut received = source.network.subscribe();
        target
            .handle_control(source_id.clone(), claim)
            .await
            .unwrap();
        assert!(matches!(
            control_frame(&mut received).await.body,
            Some(proto::control_frame::Body::ControlGranted(_))
        ));
        assert_eq!(
            lock(&target.session).as_ref().unwrap().controller,
            source_id
        );
        target.reconcile_control_component().unwrap();
        assert!(lock(&target.session).is_some());
        // Foreground/offline changes of a receiver paired to this target do
        // not invalidate ownership acquired by the other desktop.
        let mut config = target.store.snapshot();
        for portal in &mut config.layout.as_mut().unwrap().portals {
            if portal.source_display.as_str() == "c" || portal.target_display.as_str() == "c" {
                portal.status = if source_sees_app {
                    PortalStatus::Active
                } else {
                    PortalStatus::SuspendedOffline
                };
            }
        }
        target.store.save(config).unwrap();
        target.reconcile_control_component().unwrap();
        assert_eq!(
            lock(&target.session).as_ref().unwrap().control_epoch,
            ControlEpoch(8)
        );
    }
}
