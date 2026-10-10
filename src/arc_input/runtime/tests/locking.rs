use super::*;
use std::sync::{mpsc, Barrier};

async fn runtime(root: &std::path::Path) -> Arc<ArcInputRuntime> {
    ArcInputRuntime::load(
        ProductPaths::from_root(root.join("input")),
        Arc::new(ProductIdentity::from_device_id("locking-test").unwrap()),
        Arc::new(tokio::sync::OnceCell::new()),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn snapshots_finish_during_pairing_refresh_and_keep_peer_fields_consistent() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = runtime(directory.path()).await;
    let key = arcrelay_peer::DevicePublicKey::from_bytes(vec![7; 32]).unwrap();
    let device_id = DeviceId::from_public_key(&key);
    let peer = ServiceInstanceId::parse(device_id.to_string()).unwrap();
    let record = PeerRecord {
        device_id,
        public_key: key,
        display_name: "Paired".into(),
        platform: "test".into(),
        model: "test".into(),
        trust_state: arcrelay_peer::TrustState::Paired,
        auto_connect: false,
        paired_at_ms: 1,
        updated_at_ms: 1,
    };
    write(&runtime.discovered).insert(
        peer.clone(),
        DiscoveredPeer {
            service_instance_id: peer.clone(),
            display_name: "Discovered".into(),
            addresses: Vec::new(),
            connection_addresses: Vec::new(),
            port: 8765,
            certificate_sha256: "07".repeat(32),
            certificate_sha256_bytes: [7; 32],
            capability_digest: String::new(),
        },
    );
    let start = Arc::new(Barrier::new(4));
    let (done_tx, done_rx) = mpsc::channel();
    let mut workers = Vec::new();
    for _ in 0..3 {
        let runtime = runtime.clone();
        let start = start.clone();
        let done = done_tx.clone();
        workers.push(std::thread::spawn(move || {
            start.wait();
            for _ in 0..500 {
                let snapshot = runtime.snapshot();
                let peer = &snapshot.nearby_peers[0];
                assert_eq!(
                    peer.display_name.as_deref(),
                    Some(if peer.paired { "Paired" } else { "Discovered" })
                );
            }
            done.send(()).unwrap();
        }));
    }
    let writer_runtime = runtime.clone();
    workers.push(std::thread::spawn(move || {
        start.wait();
        for _ in 0..5_000 {
            write(&writer_runtime.paired_peers).insert(peer.clone(), record.clone());
            std::thread::yield_now();
            write(&writer_runtime.paired_peers).clear();
        }
        done_tx.send(()).unwrap();
    }));
    for _ in 0..4 {
        done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("pair refresh and all UI snapshots must finish without recursive read locks");
    }
    for worker in workers {
        worker.join().unwrap();
    }
}

#[tokio::test]
async fn configuration_update_releases_router_before_waiting_for_session() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = runtime(directory.path()).await;
    let local = runtime.identity.service_instance_id.clone();
    let display = test_display("local-display", &local, 0);
    let mut configuration = runtime.store.snapshot();
    configuration.layout = Some(WorkspaceLayout {
        workspace_id: WorkspaceId::parse("locking-desk").unwrap(),
        revision: TopologyRevision(1),
        displays: [(display.display_id.clone(), display)]
            .into_iter()
            .collect(),
        portals: Vec::new(),
    });
    runtime.update_configuration(configuration.clone()).unwrap();
    configuration.active_keyboard_profile = KeyboardProfileKind::Terminal;
    let router = read(&runtime.router);
    let worker_runtime = runtime.clone();
    let worker = std::thread::spawn(move || worker_runtime.update_configuration(configuration));
    // Hold a reader until the real configuration path queues its router writer.
    let deadline = Instant::now() + Duration::from_secs(5);
    while runtime.router.try_read().is_ok() && Instant::now() < deadline {
        std::thread::yield_now();
    }
    let writer_queued = runtime.router.try_read().is_err();
    let session = lock(&runtime.session);
    drop(router);
    let deadline = Instant::now() + Duration::from_secs(5);
    while runtime.router.try_read().is_err() && Instant::now() < deadline {
        std::thread::yield_now();
    }
    let router_available = runtime.router.try_read().is_ok();
    drop(session);
    worker.join().unwrap().unwrap();
    assert!(
        writer_queued,
        "configuration update must reach the router write"
    );
    assert!(
        router_available,
        "incoming input must be able to read the router while holding session"
    );
}

#[tokio::test]
async fn route_snapshot_releases_session_before_waiting_for_observed_control() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = runtime(directory.path()).await;
    let observed = lock(&runtime.observed_control);
    let worker_runtime = runtime.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        entered_tx.send(()).unwrap();
        let frame = worker_runtime
            .peer_route_snapshot_frame(&ServiceInstanceId::parse("remote-peer").unwrap());
        done_tx.send(frame).unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(
        done_rx.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    let session_available = runtime.session.try_lock().is_ok();
    drop(observed);
    done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    worker.join().unwrap();
    assert!(
        session_available,
        "mesh checks must acquire session while the route snapshot waits for observed control"
    );
}
