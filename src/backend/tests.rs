use super::*;
use arcrelay_transfer::{FileKind, TransferFileView};

fn snapshot(status: TransferStatus) -> TransferSnapshot {
    TransferSnapshot {
        revision: 0,
        device_id: "local".into(),
        device_name: "ArcRelay".into(),
        receive_directory: "/Downloads".into(),
        discoverable: true,
        peers: Vec::new(),
        transfers: vec![TransferView {
            id: "transfer-1".into(),
            wire_id: 1,
            peer_id: "peer-1".into(),
            peer_name: "小明的 Mac".into(),
            direction: TransferDirection::Receiving,
            status,
            files: vec![TransferFileView {
                id: 1,
                name: "照片.png".into(),
                relative_path: "照片.png".into(),
                local_path: None,
                receive_directory: None,
                size: 1536,
                media_type: "image/png".into(),
                kind: FileKind::Image,
                thumbnail_data_url: None,
                completed_bytes: 0,
            }],
            total_bytes: 1536,
            completed_bytes: 0,
            speed_bytes_per_second: 0,
            remaining_seconds: None,
            error_message: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        }],
    }
}

#[test]
fn notifies_once_for_approval_and_completion_transitions() {
    let mut tracker =
        TransferNotificationTracker::from_snapshot(&snapshot(TransferStatus::Connecting));

    let approval = snapshot(TransferStatus::AwaitingApproval);
    let notifications = tracker.update(&approval, crate::settings::LanguagePreference::ZhCn);
    assert_eq!(notifications.len(), 1);
    assert!(notifications[0].title.contains("文件传输请求"));
    assert_eq!(
        notifications[0].transfer_request_id.as_deref(),
        Some("transfer-1")
    );
    assert!(tracker
        .update(&approval, crate::settings::LanguagePreference::ZhCn)
        .is_empty());

    assert!(tracker
        .update(
            &snapshot(TransferStatus::Transferring),
            crate::settings::LanguagePreference::ZhCn,
        )
        .is_empty());
    let completed = snapshot(TransferStatus::Completed);
    let notifications = tracker.update(&completed, crate::settings::LanguagePreference::ZhCn);
    assert_eq!(notifications.len(), 1);
    assert!(notifications[0].title.contains("已接收"));
    assert!(notifications[0].body.contains("/Downloads"));
    assert!(notifications[0].transfer_request_id.is_none());
    assert!(tracker
        .update(&completed, crate::settings::LanguagePreference::ZhCn)
        .is_empty());
}

#[test]
fn historical_completed_transfer_is_only_used_as_baseline() {
    let completed = snapshot(TransferStatus::Completed);
    let mut tracker = TransferNotificationTracker::from_snapshot(&completed);
    assert!(tracker
        .update(&completed, crate::settings::LanguagePreference::ZhCn)
        .is_empty());
}

#[test]
fn transfer_failures_and_outgoing_completion_are_notified_once() {
    let mut tracker =
        TransferNotificationTracker::from_snapshot(&snapshot(TransferStatus::Transferring));
    let mut failed = snapshot(TransferStatus::Failed);
    failed.transfers[0].error_message = Some("校验失败".into());
    let notifications = tracker.update(&failed, crate::settings::LanguagePreference::ZhCn);
    assert_eq!(notifications.len(), 1);
    assert_eq!(
        notifications[0].category,
        crate::desktop_notification::DesktopNotificationCategory::TransferFailed
    );
    assert!(notifications[0].body.contains("校验失败"));
    assert!(tracker
        .update(&failed, crate::settings::LanguagePreference::ZhCn)
        .is_empty());

    let mut sending = snapshot(TransferStatus::Transferring);
    sending.transfers[0].direction = TransferDirection::Sending;
    let mut tracker = TransferNotificationTracker::from_snapshot(&sending);
    sending.transfers[0].status = TransferStatus::Completed;
    let notifications = tracker.update(&sending, crate::settings::LanguagePreference::EnUs);
    assert_eq!(notifications.len(), 1);
    assert!(notifications[0].title.contains("sent"));
}

#[test]
fn pairing_uses_native_prompt_without_an_active_main_window() {
    assert!(should_use_native_pairing_prompt(false));
    assert!(!should_use_native_pairing_prompt(true));
}

#[test]
fn connected_device_clears_stale_outgoing_pairing() {
    let mut runtime = RuntimeViewState::new(9945);
    runtime.outgoing_pairings.push(OutgoingPairingView {
        device_name: "Peer Mac".into(),
        device_id: "peer-1".into(),
        pairing_code: "348410".into(),
    });

    runtime.register_connected_device("peer-1".into(), "Peer Mac".into());

    assert!(runtime.outgoing_pairings.is_empty());
    assert_eq!(runtime.connected_devices.len(), 1);
    assert_eq!(runtime.connected_devices[0].id, "peer-1");
    assert_eq!(runtime.connected_device_counts.get("peer-1"), Some(&1));
}

#[test]
fn realtime_input_connection_participates_in_the_product_connection_state() {
    let mut runtime = RuntimeViewState::new(9945);

    assert!(
        runtime.sync_input_connected_devices(vec![ConnectedDeviceView {
            id: "peer-1".into(),
            name: "Peer Mac".into(),
            auto_connect: true,
        }])
    );
    assert_eq!(runtime.connected_devices.len(), 1);
    assert!(runtime.input_connected_device_ids.contains("peer-1"));

    runtime.register_connected_device("peer-1".into(), "Peer Mac".into());
    assert!(runtime.sync_input_connected_devices(Vec::new()));
    assert_eq!(runtime.connected_devices.len(), 1);

    runtime.unregister_connected_device("peer-1");
    assert!(runtime.connected_devices.is_empty());
}

#[test]
fn async_service_snapshot_releases_read_lock_before_initialization_write() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");

    runtime.block_on(async {
        let slot = tokio::sync::RwLock::new(None::<u8>);
        assert_eq!(clone_initialized_service(&slot).await, None);

        let mut initialized =
            tokio::time::timeout(std::time::Duration::from_millis(100), slot.write())
                .await
                .expect("service initialization write must not retain the snapshot read lock");
        *initialized = Some(1);
    });
}
