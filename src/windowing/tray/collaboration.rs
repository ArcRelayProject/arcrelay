use std::collections::{BTreeMap, HashSet};

use arcrelay_transfer::{TransferSnapshot, TransferStatus};

use crate::backend::{ConnectedDeviceView, DesktopState};
use crate::clipboard_sync::RemoteFileDeviceView;

pub const MAX_DEVICES: usize = 5;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollaborationState {
    pub connected_count: usize,
    pub devices: Vec<Device>,
    pub transfers: TransferSummary,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransferSummary {
    pub ready: bool,
    pub active: usize,
    pub waiting: usize,
    pub paused: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub connected: bool,
    pub paired: bool,
    pub nearby: bool,
    pub can_send: bool,
    pub can_browse: bool,
    pub transfer_pending: bool,
}

impl Device {
    pub fn can_disconnect(&self) -> bool {
        self.connected && !self.transfer_pending
    }
}

impl CollaborationState {
    pub async fn read(state: &DesktopState) -> Self {
        // These are cached views only: opening the tray must not initialize a
        // module, scan the LAN, establish a connection, or request OS permission.
        let (mut connected, paired) = state.device_views();
        for id in state.clipboard_sync.connected_peer_ids().await {
            if !connected.iter().any(|device| device.id == id) {
                let name = state
                    .network
                    .get()
                    .and_then(|network| {
                        arcrelay_peer::DeviceId::parse(&id)
                            .ok()
                            .and_then(|peer_id| network.discovery().peer(&peer_id))
                    })
                    .map(|peer| peer.metadata.name)
                    .or_else(|| {
                        paired
                            .iter()
                            .find(|device| device.id == id)
                            .map(|device| device.name.clone())
                    })
                    .unwrap_or_else(|| id.clone());
                connected.push(ConnectedDeviceView {
                    id,
                    name,
                    auto_connect: false,
                });
            }
        }
        let files = state.clipboard_sync.remote_file_devices().await;
        let transfer = state.modules.initialized_transfer().map(|manager| {
            // The watch value is an Arc: do not clone file lists/thumbnails or
            // full transfer history once per second just to draw a tray summary.
            let snapshots = manager.subscribe();
            let snapshot = snapshots.borrow().clone();
            snapshot
        });
        let mut snapshot = Self::project(&connected, &paired, &files, transfer.as_deref());
        snapshot.transfers.ready &= state.modules.statuses().iter().any(|status| {
            status.name == "transfer"
                && status.state == crate::application::runtime_modules::ModuleState::Ready
        });
        let remote_busy = state.remote_file_service.active_transfer_peer_ids();
        for device in &mut snapshot.devices {
            device.can_send &= snapshot.transfers.ready;
            device.transfer_pending |= remote_busy.contains(&device.id);
        }
        snapshot
    }

    fn project(
        connected: &[ConnectedDeviceView],
        paired: &[ConnectedDeviceView],
        files: &[RemoteFileDeviceView],
        transfer: Option<&TransferSnapshot>,
    ) -> Self {
        let mut devices = BTreeMap::<String, Device>::new();
        for peer in paired {
            devices.insert(
                peer.id.clone(),
                Device {
                    id: peer.id.clone(),
                    name: peer.name.clone(),
                    paired: true,
                    ..Default::default()
                },
            );
        }
        for peer in connected {
            let device = devices.entry(peer.id.clone()).or_default();
            device.id = peer.id.clone();
            device.name = peer.name.clone();
            device.connected = true;
        }
        for peer in files {
            let device = devices.entry(peer.id.clone()).or_default();
            device.id = peer.id.clone();
            if device.name.is_empty() {
                device.name = peer.name.clone();
            }
            // This registry already checks the negotiated RemoteFiles feature
            // and protocol version; OS names alone are not a capability check.
            device.can_browse = true;
            device.connected = true;
        }
        let mut transfers = TransferSummary {
            ready: transfer.is_some(),
            ..Default::default()
        };
        let mut busy = HashSet::new();
        if let Some(snapshot) = transfer {
            for peer in &snapshot.peers {
                let device = devices.entry(peer.id.clone()).or_default();
                device.id = peer.id.clone();
                device.name = peer.name.clone();
                device.paired |= peer.paired;
                device.nearby = true;
                // File sending uses the same available endpoints as the main
                // transfer UI. Remote acceptance is still negotiated on send.
                device.can_send = peer.endpoints().is_ok();
            }
            for task in &snapshot.transfers {
                if task.status.terminal() {
                    continue;
                }
                busy.insert(task.peer_id.as_str());
                match task.status {
                    TransferStatus::AwaitingApproval => transfers.waiting += 1,
                    TransferStatus::Paused => transfers.paused += 1,
                    _ => transfers.active += 1,
                }
            }
        }
        let mut devices: Vec<_> = devices.into_values().collect();
        for device in &mut devices {
            device.transfer_pending = busy.contains(device.id.as_str());
        }
        devices.sort_by(|left, right| {
            (
                !left.connected,
                !left.nearby,
                !left.paired,
                left.name.to_lowercase(),
                &left.id,
            )
                .cmp(&(
                    !right.connected,
                    !right.nearby,
                    !right.paired,
                    right.name.to_lowercase(),
                    &right.id,
                ))
        });
        let connected_count = devices.iter().filter(|device| device.connected).count();
        // Counts include all devices; the native menu itself remains bounded.
        // Keep the full view for live action validation; only rendering is capped.
        Self {
            connected_count,
            devices,
            transfers,
        }
    }
}

// Native menu labels interpret '&' as a mnemonic on Windows/GTK. Remote names
// are data, not menu markup, and must never add accelerators or extra lines.
pub fn device_name(value: &str) -> String {
    let mut chars = value.chars().filter(|c| !c.is_control());
    let mut name: String = chars.by_ref().take(40).collect();
    if chars.next().is_some() {
        name.push('…');
    }
    name.replace('&', "&&")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn peer(id: &str) -> ConnectedDeviceView {
        ConnectedDeviceView {
            id: id.into(),
            name: id.into(),
            auto_connect: false,
        }
    }

    #[test]
    fn device_capabilities_are_not_inferred_from_connection_or_platform() {
        let state = CollaborationState::project(
            &[peer("phone")],
            &[peer("offline")],
            &[RemoteFileDeviceView {
                id: "desktop".into(),
                name: "Desktop".into(),
            }],
            None,
        );
        assert_eq!(state.connected_count, 2);
        let phone = state.devices.iter().find(|d| d.id == "phone").unwrap();
        assert!(!phone.can_send && !phone.can_browse && phone.can_disconnect());
        let desktop = state.devices.iter().find(|d| d.id == "desktop").unwrap();
        assert!(desktop.can_browse && !desktop.can_send);
        let offline = state.devices.iter().find(|d| d.id == "offline").unwrap();
        assert!(!offline.can_disconnect() && !offline.can_browse && !offline.can_send);
        assert!(!state.transfers.ready);
    }

    #[test]
    fn menu_order_is_stable_and_total_count_is_not_truncated() {
        let peers: Vec<_> = (0..10).map(|id| peer(&id.to_string())).collect();
        let mut reverse = peers.clone();
        reverse.reverse();
        let a = CollaborationState::project(&peers, &[], &[], None);
        let b = CollaborationState::project(&reverse, &[], &[], None);
        assert_eq!(a, b);
        assert_eq!(a.connected_count, 10);
        assert_eq!(a.devices.iter().take(MAX_DEVICES).count(), MAX_DEVICES);
    }

    fn transfer_snapshot(statuses: &[TransferStatus]) -> TransferSnapshot {
        TransferSnapshot {
            revision: 1,
            device_id: "local".into(),
            device_name: "Local".into(),
            receive_directory: String::new(),
            discoverable: true,
            peers: Vec::new(),
            transfers: statuses
                .iter()
                .enumerate()
                .map(|(index, status)| arcrelay_transfer::TransferView {
                    id: index.to_string(),
                    wire_id: index as u64,
                    peer_id: "peer".into(),
                    peer_name: "Peer".into(),
                    direction: arcrelay_transfer::TransferDirection::Sending,
                    status: *status,
                    files: Vec::new(),
                    total_bytes: 1000,
                    completed_bytes: 0,
                    speed_bytes_per_second: 0,
                    remaining_seconds: None,
                    error_message: None,
                    created_at_ms: 0,
                    updated_at_ms: 0,
                })
                .collect(),
        }
    }

    #[test]
    fn summary_excludes_history_and_progress_does_not_rebuild_menus() {
        use TransferStatus::*;
        let mut transfer = transfer_snapshot(&[
            Preparing,
            Connecting,
            Transferring,
            AwaitingApproval,
            Paused,
            Completed,
            Rejected,
            Cancelled,
            Failed,
        ]);
        let before = CollaborationState::project(&[peer("peer")], &[], &[], Some(&transfer));
        assert_eq!(
            before.transfers,
            TransferSummary {
                ready: true,
                active: 3,
                waiting: 1,
                paused: 1
            }
        );
        assert!(!before.devices[0].can_disconnect());
        transfer.revision += 1;
        transfer.transfers[2].completed_bytes = 900;
        transfer.transfers[2].speed_bytes_per_second = 500;
        transfer.transfers[2].updated_at_ms = 500;
        assert_eq!(
            before,
            CollaborationState::project(&[peer("peer")], &[], &[], Some(&transfer))
        );
        for task in &mut transfer.transfers {
            task.status = Completed;
        }
        let after = CollaborationState::project(&[peer("peer")], &[], &[], Some(&transfer));
        assert!(after.devices[0].can_disconnect());
        assert_eq!(
            after.transfers.active + after.transfers.waiting + after.transfers.paused,
            0
        );
    }

    #[test]
    fn sending_uses_endpoints_on_every_remote_platform_and_browsing_requires_negotiation() {
        let mut snapshot = transfer_snapshot(&[]);
        for platform in ["macos", "windows", "linux", "ios", "android", "unknown"] {
            snapshot.peers = vec![arcrelay_transfer::NearbyPeer {
                id: "peer".into(),
                name: "Peer".into(),
                platform: platform.into(),
                model: String::new(),
                address: "127.0.0.1".into(),
                addresses: Vec::new(),
                port: 18765,
                public_key: String::new(),
                certificate_sha256: String::new(),
                paired: false,
                automatic_receive: false,
                last_seen_at_ms: 0,
            }];
            let state = CollaborationState::project(&[], &[], &[], Some(&snapshot));
            assert!(state.devices[0].can_send, "{platform}");
            assert!(!state.devices[0].can_browse, "{platform}");
        }
        snapshot.peers[0].address = "invalid".into();
        assert!(!CollaborationState::project(&[], &[], &[], Some(&snapshot)).devices[0].can_send);
    }

    #[test]
    fn native_labels_cannot_inject_mnemonics_or_new_lines() {
        assert_eq!(device_name("A&B\tQ\nX"), "A&&BQX");
        assert_eq!(
            device_name(&"界".repeat(41)),
            format!("{}…", "界".repeat(40))
        );
    }
}
