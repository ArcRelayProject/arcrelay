use super::super::{ProductIdentity, ProductPaths};
use super::*;

mod workspace_sync;

#[tokio::test]
async fn local_gaze_calibration_overlay_uses_the_runtime_event_path() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = ArcInputRuntime::load(
        ProductPaths::from_root(directory.path().join("input")),
        Arc::new(ProductIdentity::from_device_id("local-gaze-overlay").unwrap()),
        Arc::new(tokio::sync::OnceCell::new()),
    )
    .await
    .unwrap();
    let local = runtime.identity.service_instance_id.to_string();
    let local_id = runtime.identity.service_instance_id.clone();
    let display = test_display("screen-1", &local_id, 0);
    let mut configuration = runtime.store.snapshot();
    configuration.layout = Some(WorkspaceLayout {
        workspace_id: WorkspaceId::parse("gaze-overlay-desk").unwrap(),
        revision: TopologyRevision(1),
        displays: [(display.display_id.clone(), display)]
            .into_iter()
            .collect(),
        portals: Vec::new(),
    });
    runtime.store.save(configuration).unwrap();
    let mut events = runtime.subscribe();
    runtime
        .send_gaze_calibration_overlay(GazeCalibrationOverlayEvent {
            session_id: "session-1".into(),
            stage: "calibrating".into(),
            source_device_id: String::new(),
            target_device_id: local.clone(),
            display_id: "screen-1".into(),
            screen_index: 0,
            next_screen_index: None,
            screen_name: "Main".into(),
            next_screen_name: None,
            target_u: 0.5,
            target_v: 0.5,
            dwell_progress: 0.25,
            current: 1,
            total: 9,
        })
        .await
        .unwrap();
    let RuntimeEvent::GazeCalibrationOverlay(event) = events.recv().await.unwrap() else {
        panic!("expected gaze calibration overlay event");
    };
    assert_eq!(event.source_device_id, local);
    assert_eq!(event.display_id, "screen-1");
    assert_eq!(event.dwell_progress, 0.25);
}

#[tokio::test]
async fn gaze_preselection_never_changes_control_without_physical_confirmation() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = ArcInputRuntime::load(
        ProductPaths::from_root(directory.path().join("input")),
        Arc::new(ProductIdentity::from_device_id("local").unwrap()),
        Arc::new(tokio::sync::OnceCell::new()),
    )
    .await
    .unwrap();
    let local = runtime.identity.service_instance_id.clone();
    let display = test_display("local-screen", &local, 0);
    let mut configuration = runtime.store.snapshot();
    configuration.layout = Some(WorkspaceLayout {
        workspace_id: WorkspaceId::parse("gaze-desk").unwrap(),
        revision: TopologyRevision(1),
        displays: [(display.display_id.clone(), display)]
            .into_iter()
            .collect(),
        portals: Vec::new(),
    });
    runtime.store.save(configuration).unwrap();

    runtime.preselect_gaze_target(&arcrelay_gaze::GazeTarget {
        device_id: local.to_string(),
        display_id: "local-screen".into(),
        desk_x_um: 250_000,
        desk_y_um: 150_000,
        logical_x: 960.0,
        logical_y: 540.0,
        confidence: 0.9,
        source: arcrelay_gaze::TargetingSource::Eye,
    });

    assert!(lock(&runtime.gaze_preselection).is_some());
    assert!(lock(&runtime.session).is_none());

    let consumed = lock(&runtime.gaze_preselection).take().unwrap();
    *lock(&runtime.gaze_consumed) = Some(consumed);
    runtime.preselect_gaze_target(&arcrelay_gaze::GazeTarget {
        device_id: local.to_string(),
        display_id: "local-screen".into(),
        desk_x_um: 260_000,
        desk_y_um: 150_000,
        logical_x: 998.0,
        logical_y: 540.0,
        confidence: 0.9,
        source: arcrelay_gaze::TargetingSource::Eye,
    });
    assert!(lock(&runtime.gaze_preselection).is_none());
    assert!(lock(&runtime.gaze_consumed).is_some());

    runtime.preselect_gaze_target(&arcrelay_gaze::GazeTarget {
        device_id: local.to_string(),
        display_id: "local-screen".into(),
        desk_x_um: 320_000,
        desk_y_um: 150_000,
        logical_x: 1229.0,
        logical_y: 540.0,
        confidence: 0.9,
        source: arcrelay_gaze::TargetingSource::Eye,
    });
    assert!(lock(&runtime.gaze_preselection).is_some());
    assert!(lock(&runtime.gaze_consumed).is_none());

    runtime.clear_gaze_preselection();
    assert!(lock(&runtime.gaze_preselection).is_none());
}

#[tokio::test]
async fn shared_gaze_target_is_received_and_cleared_by_its_source() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = ArcInputRuntime::load(
        ProductPaths::from_root(directory.path().join("input")),
        Arc::new(ProductIdentity::from_device_id("local-gaze-target").unwrap()),
        Arc::new(tokio::sync::OnceCell::new()),
    )
    .await
    .unwrap();
    let local = runtime.identity.service_instance_id.clone();
    let remote = ServiceInstanceId::parse("remote-gaze-source").unwrap();
    let local_display = test_display("local-screen", &local, 0);
    let remote_display = test_display("remote-screen", &remote, 500_000);
    let workspace = WorkspaceId::parse("shared-gaze-desk").unwrap();
    let mut configuration = runtime.store.snapshot();
    configuration.layout = Some(WorkspaceLayout {
        workspace_id: workspace.clone(),
        revision: TopologyRevision(4),
        displays: [
            (local_display.display_id.clone(), local_display),
            (remote_display.display_id.clone(), remote_display),
        ]
        .into_iter()
        .collect(),
        portals: Vec::new(),
    });
    runtime.store.save(configuration).unwrap();

    let header = proto::RuntimeHeader {
        workspace_id: workspace.to_string(),
        topology_revision: 4,
        control_epoch: 7,
        source_device_id: remote.to_string(),
        target_device_id: local.to_string(),
        sequence: 0,
    };
    runtime
        .handle_control(
            remote.clone(),
            proto::ControlFrame {
                body: Some(proto::control_frame::Body::GazeTargetSelection(
                    proto::GazeTargetSelection {
                        header: Some(header.clone()),
                        active: true,
                        target_device_id: local.to_string(),
                        target_display_id: "local-screen".into(),
                        target_x_um: 250_000,
                        target_y_um: 150_000,
                    },
                )),
            },
        )
        .await
        .unwrap();

    assert_eq!(
        lock(&runtime.active_gaze_target).clone(),
        Some(ActiveGazeTarget {
            source: remote.clone(),
            target: local,
            display: DisplayId::parse("local-screen").unwrap(),
            point: DeskPointUm {
                x: 250_000,
                y: 150_000,
            },
        })
    );

    runtime
        .handle_control(
            remote,
            proto::ControlFrame {
                body: Some(proto::control_frame::Body::GazeTargetSelection(
                    proto::GazeTargetSelection {
                        header: Some(header),
                        active: false,
                        target_device_id: String::new(),
                        target_display_id: String::new(),
                        target_x_um: 0,
                        target_y_um: 0,
                    },
                )),
            },
        )
        .await
        .unwrap();
    assert!(lock(&runtime.active_gaze_target).is_none());
}

#[test]
fn gaze_return_to_local_uses_the_active_route_not_the_hidden_native_pointer() {
    let local = ServiceInstanceId::parse("local-device").unwrap();
    let remote = ServiceInstanceId::parse("remote-device").unwrap();
    let local_display = DisplayId::parse("local-display").unwrap();
    let remote_display = DisplayId::parse("remote-display").unwrap();
    let remote_route = (remote, remote_display);

    assert!(!super::input::gaze_target_is_active(
        Some(&remote_route),
        &local,
        &local,
        &local_display,
        true,
    ));
    assert!(super::input::gaze_target_is_active(
        None,
        &local,
        &local,
        &local_display,
        true,
    ));

    let local_route = (local.clone(), local_display.clone());
    assert!(super::input::gaze_target_is_active(
        Some(&local_route),
        &local,
        &local,
        &local_display,
        false,
    ));
}

#[tokio::test]
async fn windows_gesture_target_uses_negotiated_capabilities_and_falls_back_when_unavailable() {
    let directory = tempfile::tempdir().unwrap();
    let identity = Arc::new(ProductIdentity::from_device_id("windows-gesture-gateway").unwrap());
    let runtime = ArcInputRuntime::load(
        ProductPaths::from_root(directory.path().join("input")),
        identity,
        Arc::new(tokio::sync::OnceCell::new()),
    )
    .await
    .unwrap();
    let target = ServiceInstanceId::parse("windows-target").unwrap();
    write(&runtime.remote_operating_systems).insert(target.clone(), OsFamily::Windows);
    assert_eq!(runtime.external_system_gesture_target_version(&target), 0);
    write(&runtime.remote_capabilities).insert(
        target.clone(),
        PlatformCapabilities {
            can_inject_system_gestures: true,
            system_gesture_format_version: 1,
            ..PlatformCapabilities::default()
        },
    );
    assert_eq!(runtime.external_system_gesture_target_version(&target), 1);
    write(&runtime.remote_capabilities)
        .get_mut(&target)
        .unwrap()
        .can_inject_system_gestures = false;
    assert_eq!(runtime.external_system_gesture_target_version(&target), 0);
}

#[tokio::test]
async fn external_workspace_focus_reports_gateway_owner_and_logical_target() {
    let directory = tempfile::tempdir().unwrap();
    let paths = ProductPaths::from_root(directory.path().join("arc-input"));
    let identity = Arc::new(ProductIdentity::from_device_id("gateway-device").unwrap());
    let runtime = ArcInputRuntime::load(paths, identity, Arc::new(tokio::sync::OnceCell::new()))
        .await
        .unwrap();
    let gateway = runtime.identity.service_instance_id.clone();
    let target = ServiceInstanceId::parse("target-device").unwrap();
    write(&runtime.remote_operating_systems).insert(target.clone(), OsFamily::MacOs);
    write(&runtime.remote_capabilities).insert(
        target.clone(),
        PlatformCapabilities {
            can_inject_system_gestures: true,
            system_gesture_format_version: 2,
            ..PlatformCapabilities::default()
        },
    );
    *lock(&runtime.external_input_owner) = Some("mobile-device".into());
    *lock(&runtime.session) = Some(ControlSession {
        workspace_id: WorkspaceId::parse("workspace").unwrap(),
        controller: gateway.clone(),
        current_target: target.clone(),
        current_display: DisplayId::parse("target-display").unwrap(),
        topology_revision: TopologyRevision(3),
        control_epoch: ControlEpoch(9),
        next_sequence: 1,
        state: SessionState::Remote,
        held: HeldInputState::default(),
    });
    let mut events = runtime.subscribe_workspace_input();

    runtime.emit_workspace_focus(arcrelay_protocol::server::WorkspaceInputFocusCause::MobilePortal);
    let event = events.recv().await.unwrap();

    assert_eq!(event.owner_device_id.as_deref(), Some("mobile-device"));
    assert_eq!(event.controller_device_id, gateway.to_string());
    assert_eq!(event.logical_target_device_id, target.to_string());
    assert_eq!(event.target_display_id, "target-display");
    assert_eq!(event.control_epoch, 9);
    assert!(event.supports_system_gestures);
    assert_eq!(
        event.cause,
        arcrelay_protocol::server::WorkspaceInputFocusCause::MobilePortal
    );
}

#[tokio::test]
async fn physical_input_suspends_external_workspace_control_without_reacquiring_it() {
    let directory = tempfile::tempdir().unwrap();
    let paths = ProductPaths::from_root(directory.path().join("arc-input"));
    let identity = Arc::new(ProductIdentity::from_device_id("gateway-device").unwrap());
    let runtime = ArcInputRuntime::load(paths, identity, Arc::new(tokio::sync::OnceCell::new()))
        .await
        .unwrap();
    *lock(&runtime.external_input_owner) = Some("mobile-device".into());
    let mut events = runtime.subscribe_workspace_input();
    let physical_device = ServiceInstanceId::parse("physical-device").unwrap();

    assert!(runtime
        .suspend_external_input_for_physical(
            physical_device.clone(),
            DisplayId::parse("physical-display").unwrap(),
            11,
            false,
        )
        .unwrap());
    let event = events.recv().await.unwrap();

    assert!(runtime.external_input_owner().is_none());
    assert_eq!(event.owner_device_id.as_deref(), Some("mobile-device"));
    assert_eq!(event.logical_target_device_id, physical_device.to_string());
    assert_eq!(event.target_display_id, "physical-display");
    assert_eq!(event.control_epoch, 11);
    assert_eq!(
        event.state,
        arcrelay_protocol::server::WorkspaceInputRouteState::Suspended
    );
    assert_eq!(
        event.cause,
        arcrelay_protocol::server::WorkspaceInputFocusCause::PhysicalActivity
    );
}

#[test]
fn takeover_recenters_only_the_former_outgoing_controller() {
    let local = ServiceInstanceId::parse("mac-54").unwrap();
    let peer = ServiceInstanceId::parse("mac-target").unwrap();
    let mut session = ControlSession {
        workspace_id: WorkspaceId::parse("workspace").unwrap(),
        controller: local.clone(),
        current_target: peer.clone(),
        current_display: DisplayId::parse("target-display").unwrap(),
        topology_revision: TopologyRevision(1),
        control_epoch: ControlEpoch(1),
        next_sequence: 1,
        state: SessionState::Remote,
        held: HeldInputState::default(),
    };
    assert!(super::input::should_recenter_after_takeover(
        &session, &local, &peer
    ));
    assert!(!super::input::should_recenter_after_takeover(
        &session, &local, &local
    ));
    session.current_target = local.clone();
    assert!(!super::input::should_recenter_after_takeover(
        &session, &local, &peer
    ));
    session.controller = peer.clone();
    assert!(!super::input::should_recenter_after_takeover(
        &session, &local, &peer
    ));
}

#[test]
fn cursor_recovery_uses_current_local_monitor_not_remote_layout_or_main_screen() {
    let local = ServiceInstanceId::parse("mac-54").unwrap();
    let mut primary = test_display("primary", &local, 0);
    primary.logical_bounds = arcrelay_input::LogicalRect {
        x: 0.0,
        y: 0.0,
        width: 1728.0,
        height: 1117.0,
    };
    let mut secondary = test_display("secondary", &local, -500_000);
    secondary.logical_bounds = arcrelay_input::LogicalRect {
        x: -2560.0,
        y: -200.0,
        width: 2560.0,
        height: 1440.0,
    };
    let mut inventory = DisplayInventory {
        device_id: local,
        revision: InventoryRevision(1),
        displays: vec![primary, secondary],
    };
    let (display, center) =
        super::input::local_pointer_center(&inventory, Some(LogicalPoint { x: -1.0, y: 100.0 }))
            .unwrap();
    assert_eq!(display.display_id.as_str(), "secondary");
    assert_eq!(
        center,
        LogicalPoint {
            x: -1280.0,
            y: 520.0
        }
    );
    for position in [
        None,
        Some(LogicalPoint {
            x: 99_999.0,
            y: 0.0,
        }),
    ] {
        let (display, center) = super::input::local_pointer_center(&inventory, position).unwrap();
        assert_eq!(display.display_id.as_str(), "primary");
        assert_eq!(center, LogicalPoint { x: 864.0, y: 558.5 });
    }
    inventory.displays.clear();
    assert!(super::input::local_pointer_center(&inventory, None).is_none());
}

#[test]
fn local_pointer_location_uses_the_system_display_before_workspace_coordinates() {
    let local = ServiceInstanceId::parse("local-device").unwrap();
    let remote = ServiceInstanceId::parse("remote-device").unwrap();
    let mut primary = test_display("primary", &local, 0);
    primary.logical_bounds = arcrelay_input::LogicalRect {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
    };
    let mut secondary = test_display("secondary", &local, 500_000);
    secondary.logical_bounds = arcrelay_input::LogicalRect {
        x: 1920.0,
        y: 0.0,
        width: 1280.0,
        height: 720.0,
    };
    let remote_display = test_display("remote", &remote, 500_000);
    let layout = WorkspaceLayout {
        workspace_id: WorkspaceId::parse("desk").unwrap(),
        revision: TopologyRevision(1),
        displays: [primary, secondary, remote_display]
            .into_iter()
            .map(|display| (display.display_id.clone(), display))
            .collect(),
        portals: Vec::new(),
    };

    let (display, point) = super::input::local_pointer_location(
        &layout,
        &local,
        LogicalPoint {
            x: 2240.0,
            y: 360.0,
        },
    )
    .unwrap();

    assert_eq!(display.display_id.as_str(), "secondary");
    assert_eq!(point.x, 625_000);
    assert_eq!(point.y, 150_000);
}

fn test_display(id: &str, device: &ServiceInstanceId, x: i64) -> arcrelay_input::DisplaySurface {
    arcrelay_input::DisplaySurface {
        display_id: DisplayId::parse(id).unwrap(),
        device_id: device.clone(),
        fingerprint: arcrelay_input::DisplayFingerprint::parse(format!("fingerprint-{id}"))
            .unwrap(),
        name: id.into(),
        pixel_size: arcrelay_input::SizeU32 {
            width: 1920,
            height: 1080,
        },
        logical_bounds: arcrelay_input::LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        },
        scale_factor: arcrelay_input::ScaleFactor(1.0),
        physical_size_um: arcrelay_input::SizeI64 {
            width: 500_000,
            height: 300_000,
        },
        rotation: arcrelay_input::DisplayRotation::Degrees0,
        desk_rect_um: arcrelay_input::DeskRectUm {
            x,
            y: 0,
            width: 500_000,
            height: 300_000,
        },
        geometry_confidence: arcrelay_input::GeometryConfidence::UserCalibrated,
        inventory_revision: InventoryRevision(1),
    }
}

#[test]
fn explicit_screen_removal_survives_inventory_refresh_and_store_reload() {
    let device = ServiceInstanceId::parse("device-a").unwrap();
    let screen = test_display("screen-a", &device, 750_000);
    let inventory = DisplayInventory {
        device_id: device,
        revision: InventoryRevision(1),
        displays: vec![screen.clone()],
    };
    let mut configuration = WorkspaceConfiguration::default();
    reconcile_device_inventory(&mut configuration, &inventory).unwrap();
    assert!(configuration
        .layout
        .as_ref()
        .unwrap()
        .displays
        .contains_key(&screen.display_id));
    configuration
        .layout
        .as_mut()
        .unwrap()
        .displays
        .remove(&screen.display_id);
    configuration
        .excluded_displays
        .insert(screen.display_id.clone());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("workspace.json");
    let store = WorkspaceStore::load(&path).unwrap();
    store.save(configuration).unwrap();
    let mut restored = WorkspaceStore::load(path).unwrap().snapshot();
    reconcile_device_inventory(&mut restored, &inventory).unwrap();
    assert!(restored.layout.as_ref().unwrap().displays.is_empty());
    assert_eq!(
        restored.remembered_displays[&screen.display_id].desk_rect_um,
        screen.desk_rect_um
    );
    restored.excluded_displays.remove(&screen.display_id);
    reconcile_device_inventory(&mut restored, &inventory).unwrap();
    assert_eq!(
        restored.layout.unwrap().displays[&screen.display_id].desk_rect_um,
        screen.desk_rect_um
    );
}

#[test]
fn availability_and_inventory_counters_are_not_shared_layout_edits() {
    let device = ServiceInstanceId::parse("device-a").unwrap();
    let mut original = WorkspaceConfiguration::default();
    reconcile_device_inventory(
        &mut original,
        &DisplayInventory {
            device_id: device,
            revision: InventoryRevision(1),
            displays: vec![test_display(
                "a",
                &ServiceInstanceId::parse("device-a").unwrap(),
                0,
            )],
        },
    )
    .unwrap();
    let mut next = original.clone();
    next.input_sharing_enabled = true;
    next.layout.as_mut().unwrap().revision = TopologyRevision(999);
    next.layout
        .as_mut()
        .unwrap()
        .displays
        .values_mut()
        .next()
        .unwrap()
        .inventory_revision = InventoryRevision(999);
    assert!(same_shared_layout(&original, &next));
    next.excluded_displays
        .insert(DisplayId::parse("a").unwrap());
    assert!(!same_shared_layout(&original, &next));
}

#[tokio::test]
async fn removed_screen_catalog_is_synced_and_cannot_be_resurrected_by_inventory() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = ArcInputRuntime::load(
        ProductPaths::from_root(directory.path().join("input")),
        Arc::new(ProductIdentity::from_device_id("local").unwrap()),
        Arc::new(tokio::sync::OnceCell::new()),
    )
    .await
    .unwrap();
    let mut configuration = runtime.store.snapshot();
    let peer = ServiceInstanceId::parse("removed-peer").unwrap();
    let display = test_display("removed-screen", &peer, 950_000);
    configuration
        .remembered_displays
        .insert(display.display_id.clone(), display.clone());
    configuration
        .excluded_displays
        .insert(display.display_id.clone());
    runtime.store.save(configuration.clone()).unwrap();
    let frame =
        runtime.workspace_topology_sync_frame(&peer, configuration.layout.as_ref().unwrap());
    let Some(proto::control_frame::Body::WorkspaceTopology(topology)) = frame.body else {
        panic!("topology expected")
    };
    assert_eq!(topology.excluded_displays.len(), 1);
    let header = topology.header.unwrap();
    let excluded =
        domain_workspace_topology(header.clone(), topology.excluded_displays, Vec::new()).unwrap();
    assert_eq!(excluded.displays[&display.display_id], display);
    // Simulate the receiving peer, including persisting the catalog before it
    // receives a fresh inventory for the explicitly removed screen.
    configuration.excluded_displays.clear();
    configuration
        .remembered_displays
        .remove(&display.display_id);
    runtime.store.save(configuration).unwrap();
    runtime
        .apply_remote_topology_with_exclusions(
            domain_workspace_topology(header, topology.displays, topology.portals).unwrap(),
            excluded.displays,
            peer.as_str(),
        )
        .unwrap();
    runtime
        .merge_remote_inventory(
            peer,
            proto::DisplayInventory {
                header: None,
                inventory_revision: 3,
                displays: vec![display_to_proto(&display)],
            },
        )
        .unwrap();
    assert!(runtime
        .store
        .snapshot()
        .excluded_displays
        .contains(&display.display_id));
    assert!(!runtime
        .store
        .snapshot()
        .layout
        .unwrap()
        .displays
        .contains_key(&display.display_id));
}

#[tokio::test]
async fn disabled_input_is_unavailable_without_changing_layout_revision() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = ArcInputRuntime::load(
        ProductPaths::from_root(directory.path().join("input")),
        Arc::new(ProductIdentity::from_device_id("local").unwrap()),
        Arc::new(tokio::sync::OnceCell::new()),
    )
    .await
    .unwrap();
    let mut configuration = runtime.store.snapshot();
    let local = runtime.identity.service_instance_id.clone();
    let display = test_display("local-screen", &local, 0);
    configuration.layout = Some(
        layout_from_inventory(DisplayInventory {
            device_id: local,
            revision: InventoryRevision(1),
            displays: vec![display.clone()],
        })
        .unwrap(),
    );
    configuration.input_sharing_enabled = false;
    runtime.store.save(configuration.clone()).unwrap();
    let revision = configuration.layout.as_ref().unwrap().revision;
    runtime.update_configuration(configuration).unwrap();
    assert_eq!(
        runtime.snapshot().display_availability[&display.display_id],
        DisplayAvailability::SharingDisabled
    );
    assert_eq!(runtime.store.snapshot().layout.unwrap().revision, revision);
}

#[test]
fn portable_scroll_wire_mapping_preserves_units_and_phases() {
    for unit in [ScrollUnit::Pixel, ScrollUnit::WheelDetent] {
        assert_eq!(scroll_unit_from_proto(proto_scroll_unit(unit), false), unit);
    }
    for phase in [
        ScrollPhase::MayBegin,
        ScrollPhase::Began,
        ScrollPhase::Changed,
        ScrollPhase::Ended,
        ScrollPhase::Cancelled,
    ] {
        assert_eq!(scroll_phase_from_proto(proto_scroll_phase(phase), 0), phase);
    }
    for phase in [
        ScrollMomentumPhase::Began,
        ScrollMomentumPhase::Changed,
        ScrollMomentumPhase::Ended,
    ] {
        assert_eq!(
            momentum_phase_from_proto(proto_momentum_phase(phase)),
            phase
        );
    }
    assert_eq!(scroll_unit_from_proto(0, true), ScrollUnit::Pixel);
    assert_eq!(scroll_unit_from_proto(0, false), ScrollUnit::WheelDetent);
}

#[tokio::test]
async fn unrelated_offline_island_keeps_ownership_but_component_merge_releases_it() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = ArcInputRuntime::load(
        ProductPaths::from_root(directory.path().join("input")),
        Arc::new(ProductIdentity::from_device_id("local").unwrap()),
        Arc::new(tokio::sync::OnceCell::new()),
    )
    .await
    .unwrap();
    let local = runtime.identity.service_instance_id.clone();
    let online = ServiceInstanceId::parse("online-peer").unwrap();
    let offline = ServiceInstanceId::parse("offline-peer").unwrap();
    let mut configuration = runtime.store.snapshot();
    configuration.layout = Some(WorkspaceLayout {
        workspace_id: WorkspaceId::parse("desk").unwrap(),
        revision: TopologyRevision(1),
        portals: Vec::new(),
        displays: [
            test_display("a", &local, 0),
            test_display("b", &online, 500_000),
            test_display("c", &offline, 1_000_000),
        ]
        .into_iter()
        .map(|display| (display.display_id.clone(), display))
        .collect(),
    });
    rebuild_auto_portals(&mut configuration).unwrap();
    for portal in &mut configuration.layout.as_mut().unwrap().portals {
        if portal.target_display.as_str() == "c" || portal.source_display.as_str() == "c" {
            portal.status = PortalStatus::SuspendedOffline;
        }
    }
    runtime.store.save(configuration.clone()).unwrap();
    *lock(&runtime.control_participants) = [local.clone(), online].into_iter().collect();
    *lock(&runtime.session) = Some(ControlSession {
        workspace_id: WorkspaceId::parse("desk").unwrap(),
        controller: local.clone(),
        current_target: local,
        current_display: DisplayId::parse("a").unwrap(),
        topology_revision: TopologyRevision(1),
        control_epoch: ControlEpoch(7),
        next_sequence: 1,
        state: SessionState::Local,
        held: HeldInputState::default(),
    });
    runtime.reconcile_control_component().unwrap();
    assert_eq!(
        lock(&runtime.session).as_ref().unwrap().control_epoch,
        ControlEpoch(7)
    );
    for portal in &mut configuration.layout.as_mut().unwrap().portals {
        portal.status = PortalStatus::Active;
    }
    runtime.store.save(configuration).unwrap();
    let generation = runtime.system_gesture_generation.load(Ordering::Acquire);
    runtime.reconcile_control_component().unwrap();
    assert!(lock(&runtime.session).is_none());
    assert!(lock(&runtime.control_participants).is_empty());
    assert!(runtime.system_gesture_generation.load(Ordering::Acquire) > generation);
}

#[tokio::test]
async fn claim_with_a_different_component_cannot_establish_ownership() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = ArcInputRuntime::load(
        ProductPaths::from_root(directory.path().join("input")),
        Arc::new(ProductIdentity::from_device_id("local").unwrap()),
        Arc::new(tokio::sync::OnceCell::new()),
    )
    .await
    .unwrap();
    let local = runtime.identity.service_instance_id.clone();
    let peer = ServiceInstanceId::parse("controller").unwrap();
    let third = ServiceInstanceId::parse("third").unwrap();
    let mut configuration = runtime.store.snapshot();
    configuration.input_sharing_enabled = true;
    configuration.layout = Some(WorkspaceLayout {
        workspace_id: WorkspaceId::parse("desk").unwrap(),
        revision: TopologyRevision(1),
        portals: Vec::new(),
        displays: [
            test_display("a", &local, 0),
            test_display("b", &peer, 500_000),
            test_display("c", &third, 1_000_000),
        ]
        .into_iter()
        .map(|display| (display.display_id.clone(), display))
        .collect(),
    });
    rebuild_auto_portals(&mut configuration).unwrap();
    runtime.store.save(configuration).unwrap();
    let result = runtime
        .handle_control(
            peer.clone(),
            proto::ControlFrame {
                body: Some(proto::control_frame::Body::AcquireControl(
                    proto::AcquireControl {
                        header: Some(proto::RuntimeHeader {
                            workspace_id: "desk".into(),
                            topology_revision: 1,
                            control_epoch: 1,
                            source_device_id: peer.to_string(),
                            target_device_id: local.to_string(),
                            sequence: 1,
                        }),
                        expected_control_epoch: 0,
                        participant_device_ids: vec![local.to_string(), peer.to_string()],
                    },
                )),
            },
        )
        .await;
    // The synthetic hub cannot deliver the rejection, but no arbiter/session
    // state may be installed for the incomplete participant set.
    assert!(matches!(
        result,
        Err(RuntimeError::Network(NetworkError::NotConnected))
    ));
    assert!(lock(&runtime.session).is_none());
    assert!(runtime.arbiter.current().is_none());

    // A delayed acquire sent before a split/release cannot resurrect its old
    // token even after this receiver has become idle again.
    lock(&runtime.retired_control_epochs).insert(peer.clone(), ControlEpoch(4));
    let claim = |epoch| proto::ControlFrame {
        body: Some(proto::control_frame::Body::AcquireControl(
            proto::AcquireControl {
                header: Some(proto::RuntimeHeader {
                    workspace_id: "desk".into(),
                    topology_revision: 1,
                    control_epoch: epoch,
                    source_device_id: peer.to_string(),
                    target_device_id: local.to_string(),
                    sequence: 1,
                }),
                expected_control_epoch: 0,
                participant_device_ids: vec![
                    local.to_string(),
                    peer.to_string(),
                    third.to_string(),
                ],
            },
        )),
    };
    let _ = runtime.handle_control(peer.clone(), claim(4)).await;
    assert!(lock(&runtime.session).is_none());
    let _ = runtime.handle_control(peer.clone(), claim(5)).await;
    assert_eq!(
        lock(&runtime.session).as_ref().unwrap().control_epoch,
        ControlEpoch(5)
    );
}

#[tokio::test]
async fn disabled_input_still_accepts_newer_layout_membership() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = ArcInputRuntime::load(
        ProductPaths::from_root(directory.path().join("input")),
        Arc::new(ProductIdentity::from_device_id("local").unwrap()),
        Arc::new(tokio::sync::OnceCell::new()),
    )
    .await
    .unwrap();
    assert!(!runtime.input_sharing_enabled());
    // Also exercise creating the first router from a remote layout, without
    // trying to lock the same router write lock twice.
    *write(&runtime.router) = None;
    let local = runtime.identity.service_instance_id.clone();
    let peer = ServiceInstanceId::parse("layout-author").unwrap();
    let excluded = test_display("removed", &peer, 700_000);
    runtime
        .handle_control(
            peer.clone(),
            proto::ControlFrame {
                body: Some(proto::control_frame::Body::WorkspaceTopology(
                    proto::WorkspaceTopology {
                        header: Some(proto::RuntimeHeader {
                            workspace_id: "desk".into(),
                            topology_revision: 100,
                            control_epoch: 0,
                            source_device_id: peer.to_string(),
                            target_device_id: local.to_string(),
                            sequence: 0,
                        }),
                        displays: vec![display_to_proto(&test_display("a", &local, 0))],
                        portals: Vec::new(),
                        excluded_displays: vec![display_to_proto(&excluded)],
                    },
                )),
            },
        )
        .await
        .unwrap();
    assert!(runtime
        .store
        .snapshot()
        .excluded_displays
        .contains(&excluded.display_id));
    assert_eq!(
        runtime.store.snapshot().layout.unwrap().revision,
        TopologyRevision(100)
    );
    assert!(read(&runtime.router).is_some());
    assert!(!runtime.input_sharing_enabled());
    assert!(lock(&runtime.session).is_none());
}

#[tokio::test]
async fn system_gesture_requires_current_owner_epoch_and_negotiated_capabilities() {
    let directory = tempfile::tempdir().unwrap();
    let identity = Arc::new(ProductIdentity::from_device_id("gesture-target").unwrap());
    let runtime = ArcInputRuntime::load(
        ProductPaths::from_root(directory.path().join("input")),
        identity,
        Arc::new(tokio::sync::OnceCell::new()),
    )
    .await
    .unwrap();
    let mut configuration = runtime.store.snapshot();
    configuration.input_sharing_enabled = true;
    runtime.store.save(configuration).unwrap();
    let peer = ServiceInstanceId::parse("gesture-controller").unwrap();
    *lock(&runtime.session) = Some(ControlSession {
        workspace_id: WorkspaceId::parse("gesture-workspace").unwrap(),
        controller: peer.clone(),
        current_target: runtime.identity.service_instance_id.clone(),
        current_display: DisplayId::parse("gesture-display").unwrap(),
        topology_revision: TopologyRevision(1),
        control_epoch: ControlEpoch(7),
        next_sequence: 1,
        state: SessionState::Remote,
        held: HeldInputState::default(),
    });
    let batch = proto::InputEventBatch {
        header: Some(proto::RuntimeHeader {
            workspace_id: "gesture-workspace".into(),
            topology_revision: 1,
            control_epoch: 7,
            source_device_id: peer.to_string(),
            target_device_id: runtime.identity.service_instance_id.to_string(),
            sequence: 1,
        }),
        events: vec![proto::InputEvent {
            event: Some(proto::input_event::Event::SystemGesture(
                arcrelay_input::SystemGestureEvent {
                    axis: 1,
                    phase: 1,
                    progress: -0.02,
                    velocity_x: 0.0,
                    velocity_y: 0.0,
                    inverted_from_device: false,
                    finger_count: 0,
                }
                .into(),
            )),
        }],
        ..Default::default()
    };
    let other = ServiceInstanceId::parse("another-controller").unwrap();
    assert!(matches!(
        runtime.handle_input(other, batch.clone()),
        Err(RuntimeError::WrongPeer)
    ));
    let mut stale = batch.clone();
    stale.header.as_mut().unwrap().control_epoch = 6;
    assert!(matches!(
        runtime.handle_input(peer.clone(), stale),
        Err(RuntimeError::Control(_))
    ));
    // Even a valid session may not inject without a new peer's explicit opt-in.
    assert!(matches!(
        runtime.handle_input(peer, batch),
        Err(RuntimeError::InvalidInput(_))
    ));
    assert!(!runtime.system_gesture_route_is_current(0));
    assert!(!runtime.system_gesture_route_is_current(
        runtime.system_gesture_generation.load(Ordering::Acquire)
    ));
    let previous_generation = runtime
        .system_gesture_generation
        .fetch_add(1, Ordering::AcqRel);
    assert!(!runtime.system_gesture_route_is_current(previous_generation));
    lock(&runtime.session).take();
}

#[tokio::test]
async fn runtime_uses_product_local_identity_and_store() {
    let directory = tempfile::tempdir().unwrap();
    let paths = ProductPaths::from_root(directory.path().join("arc-input"));
    let identity = Arc::new(ProductIdentity::from_device_id("local-device").unwrap());
    let runtime = ArcInputRuntime::load(
        paths.clone(),
        identity,
        Arc::new(tokio::sync::OnceCell::new()),
    )
    .await
    .unwrap();
    let snapshot = runtime.snapshot();
    assert_eq!(snapshot.product_id, "arc.input");
    assert_eq!(snapshot.configuration.version, 2);
    assert!(paths.workspace_store.exists());
    assert!(!snapshot.configuration.keyboard_profiles.is_empty());
}

#[tokio::test]
async fn consumer_keys_require_owner_epoch_whitelist_and_display_capabilities() {
    use arcrelay_input::{ConsumerKey, ConsumerKeyEvent, PlatformCapabilities};
    let directory = tempfile::tempdir().unwrap();
    let runtime = ArcInputRuntime::load(
        ProductPaths::from_root(directory.path().join("input")),
        Arc::new(ProductIdentity::from_device_id("consumer-target").unwrap()),
        Arc::new(tokio::sync::OnceCell::new()),
    )
    .await
    .unwrap();
    let mut configuration = runtime.store.snapshot();
    configuration.input_sharing_enabled = true;
    runtime.store.save(configuration).unwrap();
    let peer = ServiceInstanceId::parse("consumer-controller").unwrap();
    *lock(&runtime.session) = Some(ControlSession {
        workspace_id: WorkspaceId::parse("consumer-workspace").unwrap(),
        controller: peer.clone(),
        current_target: runtime.identity.service_instance_id.clone(),
        current_display: DisplayId::parse("uncontrollable-display").unwrap(),
        topology_revision: TopologyRevision(1),
        control_epoch: ControlEpoch(7),
        next_sequence: 1,
        state: SessionState::Remote,
        held: HeldInputState::default(),
    });
    let batch = |usage| proto::InputEventBatch {
        header: Some(proto::RuntimeHeader {
            workspace_id: "consumer-workspace".into(),
            topology_revision: 1,
            control_epoch: 7,
            source_device_id: peer.to_string(),
            target_device_id: runtime.identity.service_instance_id.to_string(),
            sequence: lock(&runtime.session).as_ref().unwrap().next_sequence,
        }),
        events: vec![proto::InputEvent {
            event: Some(proto::input_event::Event::ConsumerKey(proto::ConsumerKey {
                hid_usage: usage,
                down: true,
                repeat: false,
            })),
        }],
        ..Default::default()
    };
    assert!(matches!(
        runtime.handle_input(ServiceInstanceId::parse("other").unwrap(), batch(0xe9)),
        Err(RuntimeError::WrongPeer)
    ));
    let mut stale = batch(0xe9);
    stale.header.as_mut().unwrap().control_epoch = 6;
    assert!(matches!(
        runtime.handle_input(peer.clone(), stale),
        Err(RuntimeError::Control(_))
    ));
    // An old peer does not opt in, even if the local OS supports the key.
    assert!(matches!(
        runtime.handle_input(peer.clone(), batch(0xe9)),
        Err(RuntimeError::InvalidInput(_))
    ));
    write(&runtime.remote_capabilities).insert(
        peer.clone(),
        PlatformCapabilities {
            consumer_capture_mask: ConsumerKey::ALL_MASK,
            ..Default::default()
        },
    );
    // Neither an unknown consumer key nor brightness for an unsupported display
    // may reach native injection. These tests never change system settings.
    assert!(matches!(
        runtime.handle_input(peer.clone(), batch(0x30)),
        Err(RuntimeError::InvalidInput(_))
    ));
    assert!(matches!(
        runtime.handle_input(peer.clone(), batch(0x6f)),
        Err(RuntimeError::InvalidInput(_))
    ));
    let old_generation = runtime.consumer_generation.load(Ordering::Acquire);
    runtime.disable_consumer_capture();
    assert!(runtime.consumer_generation.load(Ordering::Acquire) > old_generation);
    runtime
        .handle_captured_inner(CapturedInputEvent::ConsumerKey {
            event: ConsumerKeyEvent {
                key: ConsumerKey::VolumeUp,
                down: true,
                repeat: false,
            },
            generation: old_generation,
        })
        .await
        .unwrap();
    assert_eq!(lock(&runtime.session).as_ref().unwrap().controller, peer);
    lock(&runtime.session).take();
    let mut headless_configuration = runtime.store.snapshot();
    headless_configuration
        .layout
        .as_mut()
        .unwrap()
        .displays
        .clear();
    runtime.store.save(headless_configuration).unwrap();
    runtime
        .handle_captured_inner(CapturedInputEvent::ConsumerKey {
            event: ConsumerKeyEvent {
                key: ConsumerKey::VolumeUp,
                down: true,
                repeat: false,
            },
            generation: runtime.consumer_generation.load(Ordering::Acquire),
        })
        .await
        .unwrap();
    assert!(lock(&runtime.session).is_none());
}

#[tokio::test]
async fn workspace_topology_wire_round_trips_the_physical_layout() {
    let directory = tempfile::tempdir().unwrap();
    let paths = ProductPaths::from_root(directory.path().join("arc-input"));
    let identity = Arc::new(ProductIdentity::from_device_id("local-device").unwrap());
    let runtime = ArcInputRuntime::load(paths, identity, Arc::new(tokio::sync::OnceCell::new()))
        .await
        .unwrap();
    let layout = runtime.store.snapshot().layout.unwrap();
    let peer = ServiceInstanceId::parse("topology-peer").unwrap();
    let frame = runtime.workspace_topology_sync_frame(&peer, &layout);
    let proto::control_frame::Body::WorkspaceTopology(topology) = frame.body.unwrap() else {
        panic!("workspace topology frame expected");
    };
    let decoded = domain_workspace_topology(
        topology.header.unwrap(),
        topology.displays,
        topology.portals,
    )
    .unwrap();
    assert_eq!(decoded, layout);
}

#[tokio::test]
async fn control_acquisition_carries_the_authoritative_topology_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let paths = ProductPaths::from_root(directory.path().join("arc-input"));
    let identity = Arc::new(ProductIdentity::from_device_id("local-device").unwrap());
    let runtime = ArcInputRuntime::load(paths, identity, Arc::new(tokio::sync::OnceCell::new()))
        .await
        .unwrap();
    let peer = ServiceInstanceId::parse("target-device").unwrap();
    let local = runtime.identity.service_instance_id.clone();
    let layout = WorkspaceLayout {
        workspace_id: WorkspaceId::parse("shared-desk").unwrap(),
        revision: TopologyRevision(62),
        displays: [test_display("local-display", &local, 0)]
            .into_iter()
            .map(|display| (display.display_id.clone(), display))
            .collect(),
        portals: Vec::new(),
    };
    let session = ControlSession {
        workspace_id: layout.workspace_id.clone(),
        controller: local,
        current_target: runtime.identity.service_instance_id.clone(),
        current_display: DisplayId::parse("local-display").unwrap(),
        topology_revision: layout.revision,
        control_epoch: ControlEpoch(7),
        next_sequence: 1,
        state: SessionState::Local,
        held: HeldInputState::default(),
    };

    let (acquire, topology) = runtime.control_acquisition_frames(&peer, &session, &layout, 0);
    let proto::control_frame::Body::AcquireControl(acquire) = acquire.body.unwrap() else {
        panic!("control acquisition frame expected");
    };
    let proto::control_frame::Body::WorkspaceTopology(topology) = topology.body.unwrap() else {
        panic!("authoritative topology frame expected");
    };
    let acquire_header = acquire.header.unwrap();
    let topology_header = topology.header.unwrap();
    assert_eq!(acquire_header.topology_revision, 62);
    assert_eq!(topology_header.topology_revision, 62);
    assert_eq!(acquire_header.control_epoch, 7);
    assert_eq!(topology_header.control_epoch, 7);
    assert_eq!(topology.displays.len(), 1);
}

#[tokio::test]
async fn route_snapshot_relays_a_paired_workspace_peer_without_local_mdns() {
    let directory = tempfile::tempdir().unwrap();
    let paths = ProductPaths::from_root(directory.path().join("arc-input"));
    let identity = Arc::new(ProductIdentity::from_device_id("local-device").unwrap());
    let runtime = ArcInputRuntime::load(paths, identity, Arc::new(tokio::sync::OnceCell::new()))
        .await
        .unwrap();
    let local = runtime.identity.service_instance_id.clone();
    let route_key = arcrelay_peer::DevicePublicKey::from_bytes(vec![7; 32]).unwrap();
    let route_device_id = arcrelay_peer::DeviceId::from_public_key(&route_key);
    let route_device = ServiceInstanceId::parse(route_device_id.to_string()).unwrap();
    let receiver_key = arcrelay_peer::DevicePublicKey::from_bytes(vec![8; 32]).unwrap();
    let receiver_device_id = arcrelay_peer::DeviceId::from_public_key(&receiver_key);
    let receiver = ServiceInstanceId::parse(receiver_device_id.to_string()).unwrap();
    let layout = WorkspaceLayout {
        workspace_id: WorkspaceId::parse("shared-desk").unwrap(),
        revision: TopologyRevision(63),
        displays: [
            test_display("local-display", &local, 0),
            test_display("route-display", &route_device, 500_000),
            test_display("receiver-display", &receiver, 1_000_000),
        ]
        .into_iter()
        .map(|display| (display.display_id.clone(), display))
        .collect(),
        portals: Vec::new(),
    };
    let mut configuration = runtime.store.snapshot();
    configuration.layout = Some(layout);
    rebuild_auto_portals(&mut configuration).unwrap();
    runtime.store.save(configuration).unwrap();
    write(&runtime.paired_peers).insert(
        route_device.clone(),
        PeerRecord {
            device_id: route_device_id,
            public_key: route_key,
            display_name: "Route peer".into(),
            platform: "test".into(),
            model: "test".into(),
            trust_state: arcrelay_peer::TrustState::Paired,
            auto_connect: true,
            paired_at_ms: 1,
            updated_at_ms: 1,
        },
    );
    write(&runtime.discovered).insert(
        route_device.clone(),
        DiscoveredPeer {
            service_instance_id: route_device.clone(),
            display_name: "Route peer".into(),
            addresses: vec!["10.1.1.122".parse().unwrap()],
            connection_addresses: vec!["10.1.1.122:8765".parse().unwrap()],
            port: 8765,
            certificate_sha256: "09".repeat(32),
            certificate_sha256_bytes: [9; 32],
            capability_digest: String::new(),
        },
    );
    *lock(&runtime.session) = Some(ControlSession {
        workspace_id: WorkspaceId::parse("shared-desk").unwrap(),
        controller: receiver.clone(),
        current_target: local,
        current_display: DisplayId::parse("local-display").unwrap(),
        topology_revision: TopologyRevision(63),
        control_epoch: ControlEpoch(77),
        next_sequence: 1,
        state: SessionState::Remote,
        held: HeldInputState::default(),
    });

    let frame = runtime.peer_route_snapshot_frame(&receiver);
    let proto::control_frame::Body::PeerRouteSnapshot(mut snapshot) = frame.body.unwrap() else {
        panic!("peer route snapshot expected");
    };
    assert_eq!(snapshot.routes.len(), 1);
    assert_eq!(snapshot.routes[0].device_id, route_device.to_string());
    assert_eq!(snapshot.routes[0].endpoints, ["10.1.1.122:8765"]);
    assert_eq!(snapshot.routes[0].certificate_sha256, vec![9; 32]);
    assert_eq!(snapshot.active_controller_device_id, receiver.to_string());
    assert_eq!(snapshot.active_control_epoch, 77);
    lock(&runtime.session).take();
    snapshot.routes.clear();
    snapshot.header = Some(proto::RuntimeHeader {
        workspace_id: "shared-desk".into(),
        topology_revision: 63,
        control_epoch: 77,
        source_device_id: receiver.to_string(),
        target_device_id: runtime.identity.service_instance_id.to_string(),
        sequence: 0,
    });
    runtime
        .handle_peer_route_snapshot(receiver.clone(), snapshot)
        .unwrap();
    assert!(lock(&runtime.observed_control)
        .active
        .as_ref()
        .is_some_and(|(controller, epoch)| controller == &receiver && *epoch == ControlEpoch(77)));
    assert!(!runtime.workspace_mesh_connected());
}

#[tokio::test]
async fn control_target_adopts_controller_revision_before_topology_arrives() {
    let directory = tempfile::tempdir().unwrap();
    let paths = ProductPaths::from_root(directory.path().join("arc-input"));
    let identity = Arc::new(ProductIdentity::from_device_id("target-device").unwrap());
    let runtime = ArcInputRuntime::load(paths, identity, Arc::new(tokio::sync::OnceCell::new()))
        .await
        .unwrap();
    let target = runtime.identity.service_instance_id.clone();
    let controller = ServiceInstanceId::parse("controller-device").unwrap();
    let workspace_id = WorkspaceId::parse("shared-desk").unwrap();
    let mut configuration = runtime.store.snapshot();
    configuration.input_sharing_enabled = true;
    configuration.layout = Some(WorkspaceLayout {
        workspace_id: workspace_id.clone(),
        revision: TopologyRevision(62),
        displays: [
            test_display("target-display", &target, 0),
            test_display("controller-display", &controller, 500_000),
        ]
        .into_iter()
        .map(|display| (display.display_id.clone(), display))
        .collect(),
        portals: Vec::new(),
    });
    rebuild_auto_portals(&mut configuration).unwrap();
    runtime.store.save(configuration).unwrap();

    let result = runtime
        .handle_control(
            controller.clone(),
            proto::ControlFrame {
                body: Some(proto::control_frame::Body::AcquireControl(
                    proto::AcquireControl {
                        header: Some(proto::RuntimeHeader {
                            workspace_id: workspace_id.to_string(),
                            topology_revision: 51,
                            control_epoch: 9,
                            source_device_id: controller.to_string(),
                            target_device_id: target.to_string(),
                            sequence: 1,
                        }),
                        expected_control_epoch: 0,
                        participant_device_ids: Vec::new(),
                    },
                )),
            },
        )
        .await;
    assert!(matches!(
        result,
        Err(RuntimeError::Network(NetworkError::NotConnected))
    ));
    let session = lock(&runtime.session).clone().unwrap();
    assert_eq!(session.controller, controller);
    assert_eq!(session.topology_revision, TopologyRevision(51));
    assert_eq!(session.control_epoch, ControlEpoch(9));
}

#[test]
fn inventory_reconciliation_hides_and_restores_a_mirrored_member() {
    let device = ServiceInstanceId::parse("local-device").unwrap();
    let first = test_display("first", &device, 0);
    let second = test_display("second", &device, 500_000);
    let mut configuration = WorkspaceConfiguration {
        layout: Some(WorkspaceLayout {
            workspace_id: WorkspaceId::parse("desk").unwrap(),
            revision: TopologyRevision(1),
            displays: [first.clone(), second.clone()]
                .into_iter()
                .map(|display| (display.display_id.clone(), display))
                .collect(),
            portals: Vec::new(),
        }),
        ..WorkspaceConfiguration::default()
    };

    reconcile_device_inventory(
        &mut configuration,
        &DisplayInventory {
            device_id: device.clone(),
            revision: InventoryRevision(1),
            displays: vec![first.clone()],
        },
    )
    .unwrap();
    assert_eq!(configuration.layout.as_ref().unwrap().displays.len(), 1);
    assert!(configuration
        .remembered_displays
        .contains_key(&second.display_id));

    reconcile_device_inventory(
        &mut configuration,
        &DisplayInventory {
            device_id: device,
            revision: InventoryRevision(1),
            displays: vec![first, second.clone()],
        },
    )
    .unwrap();
    let restored = &configuration.layout.as_ref().unwrap().displays[&second.display_id];
    assert_eq!(restored.desk_rect_um.x, 500_000);
    assert_eq!(
        restored.geometry_confidence,
        arcrelay_input::GeometryConfidence::UserCalibrated
    );
}

#[test]
fn simultaneous_input_claims_converge_on_stable_device_order() {
    let lower = ServiceInstanceId::parse("device-a").unwrap();
    let higher = ServiceInstanceId::parse("device-b").unwrap();
    assert!(simultaneous_claim_wins(
        &higher,
        &lower,
        ControlEpoch(0),
        ControlEpoch(78),
        ControlEpoch(78)
    ));
    assert!(!simultaneous_claim_wins(
        &lower,
        &higher,
        ControlEpoch(0),
        ControlEpoch(78),
        ControlEpoch(78)
    ));

    assert!(simultaneous_claim_wins(
        &higher,
        &lower,
        ControlEpoch(8),
        ControlEpoch(9),
        ControlEpoch(9),
    ));
    assert!(!simultaneous_claim_wins(
        &lower,
        &higher,
        ControlEpoch(8),
        ControlEpoch(9),
        ControlEpoch(9),
    ));
    assert!(!simultaneous_claim_wins(
        &higher,
        &lower,
        ControlEpoch(7),
        ControlEpoch(9),
        ControlEpoch(9),
    ));
}

#[tokio::test]
async fn standby_topology_sync_is_session_independent_and_persists_author() {
    let directory = tempfile::tempdir().unwrap();
    let paths = ProductPaths::from_root(directory.path().join("arc-input"));
    let identity = Arc::new(ProductIdentity::from_device_id("local-device").unwrap());
    let runtime = ArcInputRuntime::load(paths, identity, Arc::new(tokio::sync::OnceCell::new()))
        .await
        .unwrap();
    let peer = ServiceInstanceId::parse("remote-device").unwrap();
    let mut layout = runtime.store.snapshot().layout.unwrap();
    layout.revision = TopologyRevision(7);

    let frame = runtime.workspace_topology_sync_frame(&peer, &layout);
    let proto::control_frame::Body::WorkspaceTopology(topology) = frame.body.unwrap() else {
        panic!("standby topology frame expected");
    };
    assert_eq!(topology.header.as_ref().unwrap().control_epoch, 0);

    runtime
        .apply_remote_topology(layout.clone(), peer.as_str())
        .unwrap();
    let stored = runtime.store.snapshot();
    assert_eq!(stored.topology_author, peer.as_str());
    assert_eq!(stored.layout.unwrap().revision, layout.revision);
}

#[test]
fn mirrored_inventory_keeps_one_stable_logical_surface() {
    let device = ServiceInstanceId::parse("local-device").unwrap();
    let primary = test_display("a-primary", &device, 0);
    let mut mirror = test_display("b-mirror", &device, 0);
    mirror.logical_bounds = primary.logical_bounds;
    let inventory = collapse_mirrored_inventory(DisplayInventory {
        device_id: device,
        revision: InventoryRevision(4),
        displays: vec![mirror, primary.clone()],
    });

    assert_eq!(inventory.displays.len(), 1);
    assert_eq!(inventory.displays[0].display_id, primary.display_id);
}
