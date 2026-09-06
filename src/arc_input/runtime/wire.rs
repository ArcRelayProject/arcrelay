use super::*;

pub(super) fn layout_from_inventory(
    inventory: DisplayInventory,
) -> Result<WorkspaceLayout, RuntimeError> {
    inventory.validate()?;
    Ok(WorkspaceLayout {
        workspace_id: WorkspaceId::parse("default-workspace")?,
        revision: TopologyRevision(1),
        displays: inventory
            .displays
            .into_iter()
            .map(|display| (display.display_id.clone(), display))
            .collect(),
        portals: Vec::new(),
    })
}

/// Keep one stable representative for outputs that occupy the same logical
/// desktop surface. Platform adapters already expose native mirror metadata
/// where available; this also normalizes remote and fallback inventories.
pub(super) fn collapse_mirrored_inventory(mut inventory: DisplayInventory) -> DisplayInventory {
    inventory
        .displays
        .sort_by(|left, right| left.display_id.cmp(&right.display_id));
    let mut occupied = BTreeSet::new();
    inventory.displays.retain(|display| {
        let bounds = display.logical_bounds;
        occupied.insert((
            bounds.x.to_bits(),
            bounds.y.to_bits(),
            bounds.width.to_bits(),
            bounds.height.to_bits(),
        ))
    });
    inventory
}

pub(super) fn reconcile_device_inventory(
    configuration: &mut WorkspaceConfiguration,
    inventory: &DisplayInventory,
) -> Result<(), RuntimeError> {
    inventory.validate()?;
    if configuration.layout.is_none() {
        configuration.layout = Some(layout_from_inventory(inventory.clone())?);
    }
    let layout = configuration
        .layout
        .as_mut()
        .ok_or(RuntimeError::NoLayout)?;
    // Only a live inventory can retire unplugged/mirrored outputs. Losing the
    // device connection does not remove any screen or change its geometry.
    let active_ids = inventory
        .displays
        .iter()
        .map(|display| display.display_id.clone())
        .collect::<BTreeSet<_>>();
    let missing_ids = layout
        .displays
        .iter()
        .filter(|(_, display)| {
            display.device_id == inventory.device_id && !active_ids.contains(&display.display_id)
        })
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    for id in missing_ids {
        if let Some(display) = layout.displays.remove(&id) {
            configuration.remembered_displays.insert(id, display);
        }
    }

    let mut next_x = layout
        .displays
        .values()
        .map(|display| {
            display
                .desk_rect_um
                .x
                .saturating_add(display.desk_rect_um.width)
        })
        .max()
        .unwrap_or(0);
    for mut reported in inventory.displays.iter().cloned() {
        let previous = layout
            .displays
            .get(&reported.display_id)
            .or_else(|| configuration.remembered_displays.get(&reported.display_id));
        if let Some(previous) = previous {
            reported.desk_rect_um = previous.desk_rect_um;
            if previous.geometry_confidence >= arcrelay_input::GeometryConfidence::UserProvided {
                reported.physical_size_um = previous.physical_size_um;
                reported.desk_rect_um.width = previous.desk_rect_um.width;
                reported.desk_rect_um.height = previous.desk_rect_um.height;
                reported.geometry_confidence = previous.geometry_confidence;
            }
        } else if !layout.displays.is_empty() {
            reported.desk_rect_um.x = next_x;
            reported.desk_rect_um.y = 0;
        }
        next_x = next_x.max(
            reported
                .desk_rect_um
                .x
                .saturating_add(reported.desk_rect_um.width),
        );
        configuration
            .remembered_displays
            .insert(reported.display_id.clone(), reported.clone());
        if !configuration
            .excluded_displays
            .contains(&reported.display_id)
        {
            layout
                .displays
                .insert(reported.display_id.clone(), reported);
        }
    }
    Ok(())
}

pub(super) fn rebuild_auto_portals(
    configuration: &mut WorkspaceConfiguration,
) -> Result<(), RuntimeError> {
    let Some(layout) = configuration.layout.as_mut() else {
        return Ok(());
    };
    align_system_display_groups(&mut layout.displays)?;
    for (id, display) in &layout.displays {
        configuration
            .remembered_displays
            .insert(id.clone(), display.clone());
    }
    let existing = std::mem::take(&mut layout.portals);
    layout.portals = derive_auto_portals(
        &layout.displays,
        &existing,
        DEFAULT_PORTAL_ADJACENCY_TOLERANCE_UM,
        DEFAULT_MINIMUM_PORTAL_SPAN_UM,
    )?;
    Ok(())
}

pub(super) fn same_shared_layout(
    left: &WorkspaceConfiguration,
    right: &WorkspaceConfiguration,
) -> bool {
    fn normalized(configuration: &WorkspaceConfiguration) -> Option<WorkspaceLayout> {
        let mut layout = configuration.layout.clone()?;
        layout.revision = TopologyRevision(1);
        for display in layout.displays.values_mut() {
            display.inventory_revision = InventoryRevision(1);
        }
        for portal in &mut layout.portals {
            portal.status = PortalStatus::Active;
        }
        Some(layout)
    }
    left.excluded_displays == right.excluded_displays && normalized(left) == normalized(right)
}

pub(super) fn suspend_initial_remote_portals(
    configuration: &mut WorkspaceConfiguration,
    local_device: &ServiceInstanceId,
) {
    let Some(layout) = configuration.layout.as_mut() else {
        return;
    };
    for portal in &mut layout.portals {
        let remote_endpoint = [&portal.source_display, &portal.target_display]
            .into_iter()
            .filter_map(|display_id| layout.displays.get(display_id))
            .any(|display| &display.device_id != local_device);
        if remote_endpoint {
            portal.status = PortalStatus::SuspendedOffline;
        }
    }
}

pub(super) fn unix_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .min(u128::from(u64::MAX)) as u64
}

pub(super) fn inventory_equivalent(left: &DisplayInventory, right: &DisplayInventory) -> bool {
    let normalize = |inventory: &DisplayInventory| {
        let mut inventory = inventory.clone();
        inventory.revision = InventoryRevision(0);
        for display in &mut inventory.displays {
            display.inventory_revision = InventoryRevision(0);
        }
        inventory
    };
    normalize(left) == normalize(right)
}

pub(super) fn display_to_proto(display: &arcrelay_input::DisplaySurface) -> proto::DisplaySurface {
    proto::DisplaySurface {
        display_id: display.display_id.to_string(),
        fingerprint: display.fingerprint.to_string(),
        pixel_width: display.pixel_size.width,
        pixel_height: display.pixel_size.height,
        logical_x_milli: (display.logical_bounds.x * 1000.0).round() as i64,
        logical_y_milli: (display.logical_bounds.y * 1000.0).round() as i64,
        logical_width_milli: (display.logical_bounds.width * 1000.0).round() as u64,
        logical_height_milli: (display.logical_bounds.height * 1000.0).round() as u64,
        scale_milli: (display.scale_factor.0 * 1000.0).round() as u32,
        physical_width_um: display.physical_size_um.width,
        physical_height_um: display.physical_size_um.height,
        rotation: match display.rotation {
            arcrelay_input::DisplayRotation::Degrees0 => 0,
            arcrelay_input::DisplayRotation::Degrees90 => 1,
            arcrelay_input::DisplayRotation::Degrees180 => 2,
            arcrelay_input::DisplayRotation::Degrees270 => 3,
        },
        geometry_confidence: display.geometry_confidence as u32,
        name: display.name.clone(),
        device_id: display.device_id.to_string(),
        desk_x_um: display.desk_rect_um.x,
        desk_y_um: display.desk_rect_um.y,
        desk_width_um: display.desk_rect_um.width,
        desk_height_um: display.desk_rect_um.height,
        inventory_revision: display.inventory_revision.0,
    }
}

pub(super) fn edge_proto(edge: arcrelay_input::Edge) -> u32 {
    match edge {
        arcrelay_input::Edge::Left => 0,
        arcrelay_input::Edge::Right => 1,
        arcrelay_input::Edge::Top => 2,
        arcrelay_input::Edge::Bottom => 3,
    }
}

pub(super) fn proto_edge(edge: u32) -> Result<arcrelay_input::Edge, RuntimeError> {
    match edge {
        0 => Ok(arcrelay_input::Edge::Left),
        1 => Ok(arcrelay_input::Edge::Right),
        2 => Ok(arcrelay_input::Edge::Top),
        3 => Ok(arcrelay_input::Edge::Bottom),
        _ => Err(RuntimeError::InvalidInput("portal edge".into())),
    }
}

pub(super) fn domain_workspace_topology(
    header: proto::RuntimeHeader,
    displays: Vec<proto::DisplaySurface>,
    portals: Vec<proto::Portal>,
) -> Result<WorkspaceLayout, RuntimeError> {
    let revision = TopologyRevision(header.topology_revision);
    let displays = displays
        .into_iter()
        .map(|display| {
            let display_id = DisplayId::parse(display.display_id)?;
            Ok((
                display_id.clone(),
                arcrelay_input::DisplaySurface {
                    display_id,
                    device_id: ServiceInstanceId::parse(display.device_id)?,
                    fingerprint: arcrelay_input::DisplayFingerprint::parse(display.fingerprint)?,
                    name: display.name,
                    pixel_size: arcrelay_input::SizeU32 {
                        width: display.pixel_width,
                        height: display.pixel_height,
                    },
                    logical_bounds: arcrelay_input::LogicalRect {
                        x: display.logical_x_milli as f64 / 1000.0,
                        y: display.logical_y_milli as f64 / 1000.0,
                        width: display.logical_width_milli as f64 / 1000.0,
                        height: display.logical_height_milli as f64 / 1000.0,
                    },
                    scale_factor: arcrelay_input::ScaleFactor(display.scale_milli as f64 / 1000.0),
                    physical_size_um: arcrelay_input::SizeI64 {
                        width: display.physical_width_um,
                        height: display.physical_height_um,
                    },
                    rotation: match display.rotation {
                        1 => arcrelay_input::DisplayRotation::Degrees90,
                        2 => arcrelay_input::DisplayRotation::Degrees180,
                        3 => arcrelay_input::DisplayRotation::Degrees270,
                        _ => arcrelay_input::DisplayRotation::Degrees0,
                    },
                    desk_rect_um: arcrelay_input::DeskRectUm {
                        x: display.desk_x_um,
                        y: display.desk_y_um,
                        width: display.desk_width_um,
                        height: display.desk_height_um,
                    },
                    geometry_confidence: match display.geometry_confidence {
                        4 => arcrelay_input::GeometryConfidence::UserCalibrated,
                        3 => arcrelay_input::GeometryConfidence::UserProvided,
                        2 => arcrelay_input::GeometryConfidence::HardwareReported,
                        1 => arcrelay_input::GeometryConfidence::Estimated,
                        _ => arcrelay_input::GeometryConfidence::Unknown,
                    },
                    inventory_revision: InventoryRevision(display.inventory_revision.max(1)),
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>, RuntimeError>>()?;
    let portals = portals
        .into_iter()
        .map(|portal| {
            let source_segment = portal
                .source_segment
                .ok_or_else(|| RuntimeError::InvalidInput("source edge segment".into()))?;
            let target_segment = portal
                .target_segment
                .ok_or_else(|| RuntimeError::InvalidInput("target edge segment".into()))?;
            Ok(arcrelay_input::Portal {
                portal_id: PortalId::parse(portal.portal_id)?,
                source_display: DisplayId::parse(portal.source_display_id)?,
                source_edge: proto_edge(portal.source_edge)?,
                source_segment: arcrelay_input::EdgeSegment {
                    start_um: source_segment.start_um,
                    end_um: source_segment.end_um,
                },
                target_display: DisplayId::parse(portal.target_display_id)?,
                target_edge: proto_edge(portal.target_edge)?,
                target_segment: arcrelay_input::EdgeSegment {
                    start_um: target_segment.start_um,
                    end_um: target_segment.end_um,
                },
                direction: if portal.direction == 1 {
                    PortalDirection::Bidirectional
                } else {
                    PortalDirection::OneWay
                },
                activation_policy: match portal.activation_policy {
                    1 => arcrelay_input::ActivationPolicy::RequireModifier {
                        hid_usage: u16::try_from(portal.activation_modifier_hid_usage).map_err(
                            |_| RuntimeError::InvalidInput("activation modifier".into()),
                        )?,
                    },
                    2 => arcrelay_input::ActivationPolicy::Dwell {
                        milliseconds: portal.activation_dwell_milliseconds,
                    },
                    _ => arcrelay_input::ActivationPolicy::Immediate,
                },
                allow_while_dragging: portal.allow_while_dragging,
                inset_um: portal.inset_um,
                hysteresis_um: portal.hysteresis_um,
                status: match portal.status {
                    1 => PortalStatus::SuspendedOffline,
                    2 => PortalStatus::NeedsReconciliation,
                    3 => PortalStatus::UnsupportedCapability,
                    _ => PortalStatus::Active,
                },
            })
        })
        .collect::<Result<Vec<_>, RuntimeError>>()?;
    let layout = WorkspaceLayout {
        workspace_id: WorkspaceId::parse(header.workspace_id)?,
        revision,
        displays,
        portals,
    };
    layout.validate()?;
    Ok(layout)
}

pub(super) fn domain_header(header: proto::RuntimeHeader) -> Result<RuntimeHeader, RuntimeError> {
    Ok(RuntimeHeader {
        workspace_id: WorkspaceId::parse(header.workspace_id)?,
        topology_revision: TopologyRevision(header.topology_revision),
        control_epoch: ControlEpoch(header.control_epoch),
        source_device_id: ServiceInstanceId::parse(header.source_device_id)?,
        target_device_id: ServiceInstanceId::parse(header.target_device_id)?,
        sequence: header.sequence,
    })
}

pub(super) fn validate_metadata_header(
    header: Option<&proto::RuntimeHeader>,
    source: &ServiceInstanceId,
    target: &ServiceInstanceId,
) -> Result<(), RuntimeError> {
    let header = header.ok_or(RuntimeError::MissingHeader)?;
    if header.source_device_id != source.as_str() || header.target_device_id != target.as_str() {
        return Err(RuntimeError::WrongPeer);
    }
    Ok(())
}

pub(super) fn semantic_action(value: u32) -> Result<SemanticAction, RuntimeError> {
    match value {
        0 => Ok(SemanticAction::SelectAll),
        1 => Ok(SemanticAction::Copy),
        2 => Ok(SemanticAction::Paste),
        3 => Ok(SemanticAction::Cut),
        4 => Ok(SemanticAction::Undo),
        5 => Ok(SemanticAction::Redo),
        6 => Ok(SemanticAction::ApplicationSwitch),
        _ => Err(RuntimeError::InvalidInput("semantic action".into())),
    }
}

pub(super) fn local_os_family() -> OsFamily {
    #[cfg(target_os = "windows")]
    return OsFamily::Windows;
    #[cfg(target_os = "macos")]
    return OsFamily::MacOs;
    #[cfg(target_os = "linux")]
    return if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        OsFamily::LinuxWayland
    } else {
        OsFamily::LinuxX11
    };
    #[allow(unreachable_code)]
    OsFamily::Unknown
}

pub(super) fn os_family_proto(value: OsFamily) -> i32 {
    match value {
        OsFamily::Android => proto::OsFamily::Android as i32,
        OsFamily::Ios => proto::OsFamily::Ios as i32,
        OsFamily::Unknown => proto::OsFamily::Unknown as i32,
        OsFamily::Windows => proto::OsFamily::Windows as i32,
        OsFamily::MacOs => proto::OsFamily::Macos as i32,
        OsFamily::LinuxX11 => proto::OsFamily::LinuxX11 as i32,
        OsFamily::LinuxWayland => proto::OsFamily::LinuxWayland as i32,
    }
}

pub(super) fn proto_os_family(value: i32) -> OsFamily {
    match proto::OsFamily::try_from(value).unwrap_or(proto::OsFamily::Unknown) {
        proto::OsFamily::Windows => OsFamily::Windows,
        proto::OsFamily::Macos => OsFamily::MacOs,
        proto::OsFamily::LinuxX11 => OsFamily::LinuxX11,
        proto::OsFamily::LinuxWayland => OsFamily::LinuxWayland,
        proto::OsFamily::Android => OsFamily::Android,
        proto::OsFamily::Ios => OsFamily::Ios,
        proto::OsFamily::Unknown => OsFamily::Unknown,
    }
}

pub(super) fn mapped_keyboard_to_proto(event: &MappedKeyboardEvent) -> proto::input_event::Event {
    match event {
        MappedKeyboardEvent::Physical { hid_usage, down } => {
            proto::input_event::Event::PhysicalKey(proto::PhysicalKey {
                hid_usage: u32::from(*hid_usage),
                down: *down,
                repeat: false,
            })
        }
        MappedKeyboardEvent::Semantic { action, down } => {
            proto::input_event::Event::SemanticChord(proto::SemanticChord {
                action: *action as u32,
                down: *down,
            })
        }
        MappedKeyboardEvent::TextCommit(text) => {
            proto::input_event::Event::TextCommit(proto::TextCommit { text: text.clone() })
        }
    }
}

pub(super) fn proto_held_state(state: &HeldInputState) -> proto::HeldState {
    proto::HeldState {
        held_physical_keys: state
            .held_physical_keys
            .iter()
            .map(|value| u32::from(*value))
            .collect(),
        held_mouse_buttons: state
            .held_mouse_buttons
            .iter()
            .map(|value| u32::from(*value))
            .collect(),
        modifier_state: state.modifier_state,
        caps_lock: state.caps_lock,
        num_lock: state.num_lock,
        scroll_lock: state.scroll_lock,
        keyboard_profile_revision: state.keyboard_profile_revision,
    }
}

pub(super) fn domain_held_state(state: proto::HeldState) -> Result<HeldInputState, RuntimeError> {
    Ok(HeldInputState {
        held_physical_keys: state
            .held_physical_keys
            .into_iter()
            .map(|value| {
                u16::try_from(value)
                    .map_err(|_| RuntimeError::InvalidInput("held physical key".into()))
            })
            .collect::<Result<_, _>>()?,
        held_mouse_buttons: state
            .held_mouse_buttons
            .into_iter()
            .map(|value| {
                u16::try_from(value)
                    .map_err(|_| RuntimeError::InvalidInput("held mouse button".into()))
            })
            .collect::<Result<_, _>>()?,
        modifier_state: state.modifier_state,
        caps_lock: state.caps_lock,
        num_lock: state.num_lock,
        scroll_lock: state.scroll_lock,
        keyboard_profile_revision: state.keyboard_profile_revision,
    })
}

pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(super) fn read<T>(lock_value: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock_value
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(super) fn write<T>(lock_value: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock_value
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
