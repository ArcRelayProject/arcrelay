use super::*;

impl ArcInputRuntime {
    pub async fn run_with_reserved_incoming(
        self: Arc<Self>,
        incoming: broadcast::Receiver<Arc<arcrelay_network::Session>>,
    ) -> Result<(), RuntimeError> {
        let unified_network = wait_for_network(&self.unified_network).await;
        self.run_with_incoming(unified_network, incoming).await
    }

    async fn run_with_incoming(
        self: Arc<Self>,
        unified_network: Arc<NetworkRuntime>,
        incoming: broadcast::Receiver<Arc<arcrelay_network::Session>>,
    ) -> Result<(), RuntimeError> {
        // Subscribe before any startup path can dial or attach a buffered
        // session, otherwise its first metadata exchange can be lost.
        let mut network_events = self.network.subscribe();
        self.refresh_paired_peers().await?;
        let network = self.network.clone();
        self.runtime_handle.spawn(async move {
            if let Err(error) = network.run_with_incoming(incoming).await {
                tracing::error!(%error, "Arc Input peer listener stopped");
            }
        });
        if self.input_sharing_enabled() {
            if let Err(error) = self.start_capture() {
                self.record("capture", error.to_string(), None);
                let mut configuration = self.store.snapshot();
                configuration.input_sharing_enabled = false;
                self.store.save(configuration)?;
                let _ = self.events.send(RuntimeEvent::SnapshotChanged);
            }
        }
        let mut discovered = unified_network.discovery().subscribe();
        let initial_peers = discovered
            .borrow_and_update()
            .iter()
            .filter_map(|peer| DiscoveredPeer::try_from(peer).ok())
            .collect();
        self.update_discovered(initial_peers).await;
        let mut inventory_tick = tokio::time::interval(std::time::Duration::from_secs(2));
        inventory_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut input_metrics_tick = tokio::time::interval(std::time::Duration::from_secs(5));
        input_metrics_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        input_metrics_tick.tick().await;
        let mut gesture_tick = tokio::time::interval(Duration::from_millis(500));
        let mut last_consumer_caps = InputCapturePort::capabilities(self.platform.as_ref());
        self.platform.refresh_consumer_capabilities();
        gesture_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = gesture_tick.tick() => {
                    if let Err(error) = self.platform.maintain_cursor_visibility() {
                        self.record("cursor", error.to_string(), None);
                        if let Err(error) = self.release_control() {
                            tracing::error!(%error, "Arc Input failed to release after cursor visibility error");
                        }
                    }
                    match self.platform.maintain_system_gesture() {
                        Ok(true) => self.record("system-gesture", "cancelled after 2s without a gesture update".into(), None),
                        Err(error) => self.record("system-gesture", error.to_string(), None),
                        Ok(false) => {}
                    }
                },
                _ = inventory_tick.tick() => {
                    self.platform.refresh_consumer_capabilities();
                    let caps = InputCapturePort::capabilities(self.platform.as_ref());
                    if caps != last_consumer_caps {
                        self.platform.release_consumer_keys();
                        last_consumer_caps = caps;
                        for peer in self.network.connected_peers() {
                            if let Err(error) = self.send_capabilities(&peer).await { self.record("consumer-capabilities", error.to_string(), None); }
                        }
                        self.refresh_consumer_capture();
                        let _ = self.events.send(RuntimeEvent::SnapshotChanged);
                    }
                    if let Err(error) = self.refresh_paired_peers().await {
                        self.record("pairing", error.to_string(), None);
                    }
                    if let Err(error) = self.refresh_local_inventory().await {
                        self.record("display", error.to_string(), None);
                    }
                },
                _ = input_metrics_tick.tick() => self.emit_input_metrics(),
                changed = discovered.changed() => {
                    if changed.is_err() {
                        return Err(RuntimeError::DiscoveryClosed);
                    }
                    let advertisements = {
                        let snapshot = discovered.borrow_and_update();
                        snapshot.as_ref().clone()
                    };
                    let peers = advertisements
                        .iter()
                        .filter_map(|peer| DiscoveredPeer::try_from(peer).ok())
                        .collect();
                    self.update_discovered(peers).await;
                },
                event = network_events.recv() => match event {
                    Ok(event) => {
                        if let Err(error) = self.handle_network_event(event).await {
                            self.record("protocol", error.to_string(), None);
                        }
                    },
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        self.record("network", format!("event receiver lagged by {count}"), None);
                    }
                    Err(broadcast::error::RecvError::Closed) => return Err(RuntimeError::NetworkClosed),
                },
            }
        }
    }

    pub(super) async fn update_discovered(self: &Arc<Self>, peers: Vec<DiscoveredPeer>) {
        let mut map = write(&self.discovered);
        *map = peers
            .iter()
            .cloned()
            .map(|peer| (peer.service_instance_id.clone(), peer))
            .collect();
        drop(map);
        if self.input_sharing_enabled() {
            self.reconnect_peers();
            for peer in self.network.connected_peers() {
                let frame = self.peer_route_snapshot_frame(&peer);
                let network = self.network.clone();
                self.runtime_handle.spawn(async move {
                    if let Err(error) = network.send_control(&peer, &frame).await {
                        tracing::debug!(%error, "Arc Input route snapshot update failed");
                    }
                });
            }
        }
        let _ = self.events.send(RuntimeEvent::SnapshotChanged);
    }

    pub(super) async fn handle_network_event(
        self: &Arc<Self>,
        event: NetworkEvent,
    ) -> Result<(), RuntimeError> {
        match event {
            NetworkEvent::Connected {
                service_instance_id,
                session_id,
            } => {
                if !self
                    .network
                    .is_current_session(&service_instance_id, session_id)
                {
                    return Ok(());
                }
                self.record(
                    "connection",
                    format!(
                        "connected {}",
                        crate::arc_input::log_peer_id(service_instance_id.as_str())
                    ),
                    None,
                );
                // Wait for fresh inventory/capabilities before this peer can
                // join a control component; never extend an old token here.
                write(&self.remote_capabilities).remove(&service_instance_id);
                write(&self.remote_inventories).remove(&service_instance_id);
                write(&self.remote_sharing_enabled).remove(&service_instance_id);
                self.suspend_offline_portals(&service_instance_id)?;
                *lock(&self.automatic_control_retry_at) = None;
                if self.input_sharing_enabled() {
                    let routes = self.peer_route_snapshot_frame(&service_instance_id);
                    self.network
                        .send_control(&service_instance_id, &routes)
                        .await?;
                }
                self.send_workspace_sync(&service_instance_id).await?;
                let _ = self.events.send(RuntimeEvent::SnapshotChanged);
            }
            NetworkEvent::Disconnected {
                service_instance_id,
                session_id,
                reason,
            } => {
                // An old receiver can report closure after its replacement
                // has already completed authentication and metadata exchange.
                if self
                    .network
                    .connected_peers()
                    .contains(&service_instance_id)
                {
                    tracing::debug!(
                        event = "input.connection.obsolete_close_ignored",
                        session_id,
                        "ignored closure from a replaced input session"
                    );
                    return Ok(());
                }
                self.record(
                    "connection",
                    format!(
                        "disconnected {}: {}",
                        crate::arc_input::log_peer_id(service_instance_id.as_str()),
                        crate::arc_input::sanitize_log_text(&reason, 512)
                    ),
                    None,
                );
                write(&self.remote_capabilities).remove(&service_instance_id);
                write(&self.remote_inventories).remove(&service_instance_id);
                write(&self.remote_sharing_enabled).remove(&service_instance_id);
                self.suspend_offline_portals(&service_instance_id)?;
                let should_release = lock(&self.session).as_ref().is_some_and(|session| {
                    session.controller == service_instance_id
                        || session.current_target == service_instance_id
                });
                if should_release {
                    self.release_control()?;
                }
                self.broadcast_peer_route_snapshots();
                let _ = self.events.send(RuntimeEvent::SnapshotChanged);
            }
            NetworkEvent::Control {
                service_instance_id,
                session_id,
                frame,
            } => {
                if self
                    .network
                    .is_current_session(&service_instance_id, session_id)
                {
                    self.handle_control(service_instance_id, frame).await?;
                }
            }
            NetworkEvent::Input {
                service_instance_id,
                session_id,
                batch,
            } => {
                if self
                    .network
                    .is_current_session(&service_instance_id, session_id)
                {
                    self.handle_input(service_instance_id, batch)?;
                }
            }
        }
        Ok(())
    }

    pub(super) async fn refresh_local_inventory(&self) -> Result<(), RuntimeError> {
        let mut reported = collapse_mirrored_inventory(self.platform.inventory()?);
        let changed = read(&self.local_inventory)
            .as_ref()
            .is_none_or(|current| !inventory_equivalent(current, &reported));
        if !changed {
            return Ok(());
        }
        let revision = read(&self.local_inventory)
            .as_ref()
            .map_or(1, |current| current.revision.0.saturating_add(1).max(1));
        reported.revision = InventoryRevision(revision);
        for display in &mut reported.displays {
            display.inventory_revision = reported.revision;
        }
        reported.validate()?;

        let mut configuration = self.store.snapshot();
        reconcile_device_inventory(&mut configuration, &reported)?;
        *write(&self.local_inventory) = Some(reported.clone());
        self.update_configuration(configuration)?;
        for peer in self.network.connected_peers() {
            self.send_inventory(&peer, reported.clone()).await?;
        }
        self.record(
            "display",
            format!("local inventory advanced to revision {revision}"),
            None,
        );
        Ok(())
    }

    pub(super) async fn handle_control(
        self: &Arc<Self>,
        peer: ServiceInstanceId,
        frame: proto::ControlFrame,
    ) -> Result<(), RuntimeError> {
        match frame.body {
            Some(proto::control_frame::Body::AcquireControl(acquire)) => {
                let _external_gate = self.external_input_gate.lock().await;
                let header = acquire.header.ok_or(RuntimeError::MissingHeader)?;
                tracing::info!(
                    event = "input.control.acquire_received",
                    peer_id = %crate::arc_input::log_peer_id(peer.as_str()),
                    control_epoch = header.control_epoch,
                    topology_revision = header.topology_revision,
                    expected_control_epoch = acquire.expected_control_epoch,
                    "Arc Input control acquisition received"
                );
                if header.source_device_id != peer.as_str()
                    || header.target_device_id != self.identity.service_instance_id.as_str()
                {
                    return Err(RuntimeError::WrongPeer);
                }
                if header.topology_revision == 0 {
                    return Err(RuntimeError::InvalidInput(
                        "control acquisition has a zero topology revision".into(),
                    ));
                }
                let workspace = WorkspaceId::parse(header.workspace_id.clone())?;
                // Topology revisions are controller-owned session tokens, not
                // node-local counters that can be compared across devices.
                // The authoritative topology itself follows this acquisition
                // frame. Adopting the controller revision here allows that
                // snapshot (and the first handoff) to replace a target's
                // independently persisted revision in either direction.
                let controller_topology_revision = TopologyRevision(header.topology_revision);
                let previous_session = lock(&self.session).clone();
                let expected = ControlEpoch(acquire.expected_control_epoch);
                let incoming = ControlEpoch(header.control_epoch.max(1));
                let active_token = lock(&self.session).as_ref().map(|session| {
                    (
                        session.control_epoch,
                        session.workspace_id.clone(),
                        session.controller.clone(),
                    )
                });
                let component = self.input_control_component();
                let participants = acquire
                    .participant_device_ids
                    .iter()
                    .map(|id| ServiceInstanceId::parse(id.clone()))
                    .collect::<Result<BTreeSet<_>, _>>()?;
                let scope_matches = if participants.is_empty() {
                    // Legacy senders can only safely claim a fully ready,
                    // unsplit workspace, since they use workspace-wide tokens.
                    self.store.snapshot().layout.as_ref().is_some_and(|layout| {
                        layout
                            .displays
                            .values()
                            .all(|display| component.contains(&display.device_id))
                    })
                } else {
                    participants == component
                };
                let retired_epoch = lock(&self.retired_control_epochs).get(&peer).copied();
                let arbitration = if !self.input_sharing_enabled() {
                    Err(arcrelay_input::ControlError::InputSharingDisabled)
                } else if !scope_matches || !component.contains(&peer) || component.len() < 2 {
                    Err(arcrelay_input::ControlError::ComponentChanged)
                } else if retired_epoch.is_some_and(|epoch| incoming <= epoch) {
                    Err(arcrelay_input::ControlError::StaleEpoch {
                        expected: ControlEpoch(
                            retired_epoch.map_or(0, |epoch| epoch.0).saturating_add(1),
                        ),
                        actual: incoming,
                    })
                } else {
                    match (active_token, self.arbiter.current()) {
                        (None, _) if acquire.expected_control_epoch == 0 => {
                            self.arbiter.acquire(workspace.clone(), peer.clone())
                        }
                        (Some((_, active_workspace, _)), Some(_))
                            if active_workspace != workspace =>
                        {
                            Err(arcrelay_input::ControlError::WrongWorkspace)
                        }
                        (Some((active, _, active_controller)), Some(grant))
                            if active == incoming && active_controller == peer =>
                        {
                            Ok(grant)
                        }
                        (Some((active, _, _)), Some(grant)) if active == expected => {
                            self.arbiter.transfer(grant.epoch, peer.clone())
                        }
                        (Some((active, _, active_controller)), Some(grant))
                            if simultaneous_claim_wins(
                                &peer,
                                &active_controller,
                                expected,
                                incoming,
                                active,
                            ) =>
                        {
                            // Simultaneous local activity can produce two
                            // claimants for the same next epoch. The stable
                            // service ID ordering makes every peer retain the
                            // same winner regardless of delivery order.
                            self.arbiter.transfer(grant.epoch, peer.clone())
                        }
                        (Some((active, _, _)), _) => {
                            Err(arcrelay_input::ControlError::StaleEpoch {
                                expected: active,
                                actual: expected,
                            })
                        }
                        (None, _) => Err(arcrelay_input::ControlError::NotControlled),
                    }
                };
                match arbitration {
                    Ok(grant) => {
                        if let Some(previous) = previous_session.as_ref().filter(|s| {
                            s.controller == self.identity.service_instance_id
                                && s.controller != peer
                        }) {
                            let application_targets = lock(&self.control_participants)
                                .iter()
                                .filter(|target| self.is_application_receiver(target))
                                .cloned()
                                .collect();
                            self.broadcast_release_to(previous, application_targets);
                        }
                        *lock(&self.control_participants) = component;
                        if previous_session
                            .as_ref()
                            .is_some_and(|session| session.controller != peer)
                        {
                            if let Some(previous) = &previous_session {
                                self.retire_control_session(previous);
                            }
                            self.system_gesture_generation
                                .fetch_add(1, Ordering::AcqRel);
                            let gesture_result =
                                self.platform.set_system_gesture_capture_generation(0, 0);
                            self.disable_consumer_capture();
                            let release_result = self.platform.release_all();
                            // A physical-input takeover restores the former
                            // controller's cursor on its own current display.
                            // Do this while still hidden, without broadcasting
                            // another release that could revoke the new owner.
                            if release_result.is_ok()
                                && previous_session.as_ref().is_some_and(|previous| {
                                    super::input::should_recenter_after_takeover(
                                        previous,
                                        &self.identity.service_instance_id,
                                        &peer,
                                    )
                                })
                            {
                                if let Err(error) = self.place_local_pointer_at_center() {
                                    self.record(
                                        "cursor",
                                        format!("takeover recenter failed: {error}"),
                                        None,
                                    );
                                }
                            }
                            // Always restore local input even if placement or
                            // releasing an injected key/gesture failed.
                            let visibility_result = self.platform.set_suppress_local(false);
                            gesture_result?;
                            release_result?;
                            visibility_result?;
                            for (_, waiter) in std::mem::take(&mut *lock(&self.control_waiters)) {
                                let _ = waiter
                                    .send(Err("input ownership moved to another device".into()));
                            }
                        }
                        let layout = self.store.snapshot().layout.ok_or(RuntimeError::NoLayout)?;
                        let display = layout
                            .displays
                            .values()
                            .find(|display| display.device_id == self.identity.service_instance_id)
                            .ok_or(RuntimeError::NoDisplay)?;
                        let token_epoch = ControlEpoch(header.control_epoch.max(1));
                        self.control_epoch_clock
                            .fetch_max(token_epoch.0, Ordering::AcqRel);
                        let physical_target_display = layout
                            .displays
                            .values()
                            .find(|display| display.device_id == peer)
                            .map(|display| display.display_id.clone())
                            .unwrap_or_else(|| display.display_id.clone());
                        let suspended_external = self.suspend_external_input_for_physical(
                            peer.clone(),
                            physical_target_display.clone(),
                            token_epoch.0,
                            false,
                        )?;
                        if !suspended_external {
                            self.emit_physical_focus_for_target(
                                &peer,
                                &physical_target_display,
                                token_epoch.0,
                            );
                        }
                        lock(&self.remote_takeover_intent).clear();
                        *lock(&self.session) = Some(ControlSession {
                            workspace_id: workspace,
                            controller: peer.clone(),
                            current_target: self.identity.service_instance_id.clone(),
                            current_display: display.display_id.clone(),
                            topology_revision: controller_topology_revision,
                            control_epoch: token_epoch,
                            next_sequence: header.sequence.max(1),
                            state: SessionState::Remote,
                            held: HeldInputState::default(),
                        });
                        *lock(&self.observed_control) = ObservedControlState {
                            version: token_epoch.0,
                            active: Some((peer.clone(), token_epoch)),
                        };
                        self.network
                            .send_control(
                                &peer,
                                &proto::ControlFrame {
                                    body: Some(proto::control_frame::Body::ControlGranted(
                                        proto::ControlGranted {
                                            header: Some(proto::RuntimeHeader {
                                                workspace_id: grant.workspace_id.to_string(),
                                                topology_revision: controller_topology_revision.0,
                                                control_epoch: token_epoch.0,
                                                source_device_id: self
                                                    .identity
                                                    .service_instance_id
                                                    .to_string(),
                                                target_device_id: peer.to_string(),
                                                sequence: 0,
                                            }),
                                        },
                                    )),
                                },
                            )
                            .await?;
                        tracing::info!(
                            event = "input.control.acquire_granted",
                            peer_id = %crate::arc_input::log_peer_id(peer.as_str()),
                            control_epoch = token_epoch.0,
                            topology_revision = controller_topology_revision.0,
                            replaced_local_topology_revision = layout.revision.0,
                            "Arc Input control acquisition granted"
                        );
                    }
                    Err(error) => {
                        tracing::warn!(
                            event = "input.control.acquire_rejected",
                            peer_id = %crate::arc_input::log_peer_id(peer.as_str()),
                            control_epoch = header.control_epoch,
                            topology_revision = header.topology_revision,
                            requested_participants = ?participants.iter().map(|id| crate::arc_input::log_peer_id(id.as_str())).collect::<Vec<_>>(),
                            available_participants = ?component.iter().map(|id| crate::arc_input::log_peer_id(id.as_str())).collect::<Vec<_>>(),
                            %error,
                            "Arc Input control acquisition rejected"
                        );
                        if error == arcrelay_input::ControlError::ComponentChanged {
                            // A rejected claim never reaches the session topology
                            // exchange. Return fresh metadata so the next claim
                            // can converge instead of repeating the same mismatch.
                            if let Err(sync_error) = self.send_workspace_sync(&peer).await {
                                tracing::debug!(%sync_error, "Arc Input component recovery sync failed");
                            }
                        }
                        self.network
                            .send_control(
                                &peer,
                                &proto::ControlFrame {
                                    body: Some(proto::control_frame::Body::ControlRevoked(
                                        proto::ControlRevoked {
                                            header: Some(proto::RuntimeHeader {
                                                workspace_id: header.workspace_id,
                                                topology_revision: header.topology_revision,
                                                control_epoch: header.control_epoch,
                                                source_device_id: self
                                                    .identity
                                                    .service_instance_id
                                                    .to_string(),
                                                target_device_id: peer.to_string(),
                                                sequence: header.sequence,
                                            }),
                                            reason: error.to_string(),
                                        },
                                    )),
                                },
                            )
                            .await?;
                        if error == arcrelay_input::ControlError::ComponentChanged {
                            self.network
                                .send_control(&peer, &self.peer_route_snapshot_frame(&peer))
                                .await?;
                            self.reconnect_peers();
                        }
                    }
                }
            }
            Some(proto::control_frame::Body::ControlGranted(granted)) => {
                let header = granted.header.ok_or(RuntimeError::MissingHeader)?;
                if header.source_device_id != peer.as_str()
                    || header.target_device_id != self.identity.service_instance_id.as_str()
                {
                    return Err(RuntimeError::WrongPeer);
                }
                {
                    let session = lock(&self.session);
                    let session = session.as_ref().ok_or(RuntimeError::NoControlSession)?;
                    if session.controller != self.identity.service_instance_id
                        || session.workspace_id.as_str() != header.workspace_id
                        || header.topology_revision > session.topology_revision.0
                        || session.control_epoch.0 != header.control_epoch
                    {
                        return Err(RuntimeError::WrongPeer);
                    }
                }
                if let Some(waiter) = lock(&self.control_waiters).remove(&peer) {
                    let _ = waiter.send(Ok(()));
                }
                tracing::info!(
                    event = "input.control.grant_received",
                    peer_id = %crate::arc_input::log_peer_id(peer.as_str()),
                    control_epoch = header.control_epoch,
                    topology_revision = header.topology_revision,
                    "Arc Input ownership grant received"
                );
            }
            Some(proto::control_frame::Body::CapabilitySnapshot(capabilities)) => {
                let supports_input_relay = capabilities.supports_input_relay;
                let remote_epoch = capabilities
                    .header
                    .as_ref()
                    .map_or(0, |header| header.control_epoch);
                validate_metadata_header(
                    capabilities.header.as_ref(),
                    &peer,
                    &self.identity.service_instance_id,
                )?;
                self.control_epoch_clock
                    .fetch_max(remote_epoch, Ordering::AcqRel);
                write(&self.remote_sharing_enabled).insert(
                    peer.clone(),
                    capabilities.input_sharing_enabled.unwrap_or(
                        capabilities.limitation != "cross-screen input is disabled on this device",
                    ),
                );
                write(&self.remote_operating_systems)
                    .insert(peer.clone(), proto_os_family(capabilities.os_family));
                let domain_capabilities = PlatformCapabilities {
                    can_capture_pointer: capabilities.can_capture_pointer,
                    can_capture_keyboard: capabilities.can_capture_keyboard,
                    can_suppress_local_input: capabilities.can_suppress_local_input,
                    can_place_internal_barrier: capabilities.can_place_internal_barrier,
                    can_inject_absolute_pointer: capabilities.can_inject_absolute_pointer,
                    can_inject_keyboard: capabilities.can_inject_keyboard,
                    can_inject_app_pointer: capabilities.can_inject_app_pointer,
                    can_control_elevated_apps: capabilities.can_control_elevated_apps,
                    can_persist_permission: capabilities.can_persist_permission,
                    can_capture_native_quartz_events: capabilities.can_capture_native_quartz_events,
                    can_inject_native_quartz_events: capabilities.can_inject_native_quartz_events,
                    can_capture_precision_touchpad_events: capabilities
                        .can_capture_precision_touchpad_events,
                    can_inject_precision_touchpad_events: capabilities
                        .can_inject_precision_touchpad_events,
                    can_capture_system_gestures: capabilities.can_capture_system_gestures,
                    can_inject_system_gestures: capabilities.can_inject_system_gestures,
                    system_gesture_format_version: capabilities.system_gesture_format_version,
                    consumer_capture_mask: capabilities.consumer_capture_mask
                        & arcrelay_input::ConsumerKey::ALL_MASK,
                    consumer_inject_mask: capabilities.consumer_inject_mask
                        & arcrelay_input::ConsumerKey::ALL_MASK,
                    brightness_display_ids: capabilities
                        .brightness_display_ids
                        .into_iter()
                        .take(64)
                        .collect(),
                    limitation: (!capabilities.limitation.is_empty())
                        .then_some(capabilities.limitation),
                };
                tracing::info!(
                    event = "input.capability.received",
                    peer_id = %crate::arc_input::log_peer_id(peer.as_str()),
                    can_capture_pointer = domain_capabilities.can_capture_pointer,
                    can_capture_keyboard = domain_capabilities.can_capture_keyboard,
                    can_suppress_local_input = domain_capabilities.can_suppress_local_input,
                    can_inject_pointer = domain_capabilities.can_inject_absolute_pointer,
                    can_inject_keyboard = domain_capabilities.can_inject_keyboard,
                    consumer_capture_mask = domain_capabilities.consumer_capture_mask,
                    consumer_inject_mask = domain_capabilities.consumer_inject_mask,
                    brightness_display_count = domain_capabilities.brightness_display_ids.len(),
                    can_inject_native_quartz_events = domain_capabilities
                        .can_inject_native_quartz_events,
                    can_inject_precision_touchpad_events = domain_capabilities
                        .can_inject_precision_touchpad_events,
                    target_ready = domain_capabilities.can_target(),
                    "Arc Input capability snapshot received"
                );
                if !domain_capabilities.can_target() {
                    self.record(
                        "capability",
                        format!(
                            "{} cannot act as a complete input target",
                            crate::arc_input::log_peer_id(peer.as_str())
                        ),
                        None,
                    );
                }
                let previous_capabilities = write(&self.remote_capabilities)
                    .insert(peer.clone(), domain_capabilities.clone());
                if previous_capabilities.as_ref() != Some(&domain_capabilities)
                    && lock(&self.session)
                        .as_ref()
                        .is_some_and(|session| session.controller == peer)
                {
                    self.platform.release_consumer_keys();
                }
                if previous_capabilities.is_some_and(|previous| {
                    (previous.can_capture_system_gestures
                        && !domain_capabilities.can_capture_system_gestures)
                        || (previous.can_inject_system_gestures
                            && !domain_capabilities.can_inject_system_gestures)
                        || previous.system_gesture_version()
                            > domain_capabilities.system_gesture_version()
                }) && lock(&self.session).as_ref().is_some_and(|session| {
                    session.controller == peer || session.current_target == peer
                }) {
                    // Capability loss must cancel an in-flight animation, not
                    // silently strand it while continuing the same session.
                    self.release_control()?;
                }
                if lock(&self.session)
                    .as_ref()
                    .is_some_and(|session| session.current_target == peer)
                {
                    let gesture_version = self.system_gesture_target_version(&peer);
                    self.platform.set_native_quartz_capture_enabled(
                        self.native_quartz_target_supported(&peer),
                    )?;
                    self.platform.set_system_gesture_capture_generation(
                        if gesture_version != 0 {
                            self.system_gesture_generation.load(Ordering::Acquire)
                        } else {
                            0
                        },
                        gesture_version,
                    )?;
                }
                self.update_configuration(self.store.snapshot())?;
                self.refresh_consumer_capture();
                if self.network.set_relay_capable(&peer, supports_input_relay) {
                    self.broadcast_peer_route_snapshots();
                }
            }
            Some(proto::control_frame::Body::DisplayInventory(inventory)) => {
                validate_metadata_header(
                    inventory.header.as_ref(),
                    &peer,
                    &self.identity.service_instance_id,
                )?;
                let display_count = inventory.displays.len();
                let revision = inventory.inventory_revision;
                let peer_id = crate::arc_input::log_peer_id(peer.as_str());
                self.merge_remote_inventory(peer, inventory)?;
                tracing::info!(
                    event = "input.display.inventory_received",
                    %peer_id,
                    revision,
                    display_count,
                    "Arc Input display inventory received"
                );
            }
            Some(proto::control_frame::Body::WorkspaceTopology(topology)) => {
                let header = topology.header.ok_or(RuntimeError::MissingHeader)?;
                if header.source_device_id != peer.as_str()
                    || header.target_device_id != self.identity.service_instance_id.as_str()
                {
                    return Err(RuntimeError::WrongPeer);
                }
                let excluded = domain_workspace_topology(
                    header.clone(),
                    topology.excluded_displays,
                    Vec::new(),
                )?
                .displays;
                if header.control_epoch == 0 {
                    // Disabling input never disables shared layout metadata.
                    let configuration = self.store.snapshot();
                    let current_revision = configuration
                        .layout
                        .as_ref()
                        .map_or(0, |layout| layout.revision.0);
                    let incoming_wins = (header.topology_revision, peer.as_str())
                        > (current_revision, configuration.topology_author.as_str());
                    if incoming_wins {
                        let layout =
                            domain_workspace_topology(header, topology.displays, topology.portals)?;
                        let mut incoming_configuration = configuration.clone();
                        incoming_configuration.layout = Some(layout.clone());
                        incoming_configuration.excluded_displays =
                            excluded.keys().cloned().collect();
                        if lock(&self.session).is_some() {
                            if same_shared_layout(&configuration, &incoming_configuration) {
                                return Ok(());
                            }
                            self.release_control()?;
                        }
                        self.apply_remote_topology_with_exclusions(
                            layout,
                            excluded,
                            peer.as_str(),
                        )?;
                    }
                    return Ok(());
                }
                {
                    let sessions = lock(&self.session);
                    let session = sessions.as_ref().ok_or(RuntimeError::NoControlSession)?;
                    if session.controller != peer
                        || session.control_epoch.0 != header.control_epoch
                        || session.workspace_id.as_str() != header.workspace_id
                        || header.topology_revision < session.topology_revision.0
                    {
                        return Err(RuntimeError::WrongPeer);
                    }
                }
                let acknowledgement = header.clone();
                let layout =
                    domain_workspace_topology(header, topology.displays, topology.portals)?;
                self.apply_remote_topology_with_exclusions(layout, excluded, peer.as_str())?;
                self.network
                    .send_control(
                        &peer,
                        &proto::ControlFrame {
                            body: Some(proto::control_frame::Body::ControlGranted(
                                proto::ControlGranted {
                                    header: Some(proto::RuntimeHeader {
                                        workspace_id: acknowledgement.workspace_id,
                                        topology_revision: acknowledgement.topology_revision,
                                        control_epoch: acknowledgement.control_epoch,
                                        source_device_id: self
                                            .identity
                                            .service_instance_id
                                            .to_string(),
                                        target_device_id: peer.to_string(),
                                        sequence: acknowledgement.sequence,
                                    }),
                                },
                            )),
                        },
                    )
                    .await?;
            }
            Some(proto::control_frame::Body::PeerRouteSnapshot(snapshot)) => {
                self.handle_peer_route_snapshot(peer, snapshot)?;
            }
            Some(proto::control_frame::Body::PrepareHandoff(prepare)) => {
                let header = prepare.header.ok_or(RuntimeError::MissingHeader)?;
                lock(&self.remote_takeover_intent).clear();
                tracing::info!(
                    event = "input.handoff.prepare_received",
                    source_peer = %crate::arc_input::log_peer_id(peer.as_str()),
                    control_epoch = header.control_epoch,
                    topology_revision = header.topology_revision,
                    sequence = header.sequence,
                    held_key_count = prepare.held_state.as_ref().map_or(0, |held| held.held_physical_keys.len()),
                    held_button_count = prepare.held_state.as_ref().map_or(0, |held| held.held_mouse_buttons.len()),
                    "Arc Input handoff prepare received"
                );
                if header.source_device_id != peer.as_str()
                    || header.target_device_id != self.identity.service_instance_id.as_str()
                {
                    return Err(RuntimeError::WrongPeer);
                }
                let display = DisplayId::parse(prepare.target_display_id)?;
                {
                    let mut sessions = lock(&self.session);
                    let session = sessions.as_mut().ok_or(RuntimeError::NoControlSession)?;
                    if session.controller != peer
                        || session.control_epoch.0 != header.control_epoch
                        || session.topology_revision.0 != header.topology_revision
                    {
                        return Err(RuntimeError::WrongPeer);
                    }
                    session.current_display = display;
                    session.next_sequence = header.sequence.max(1);
                    session.state = SessionState::Remote;
                    session.held = prepare
                        .held_state
                        .map(domain_held_state)
                        .transpose()?
                        .unwrap_or_default();
                }
                self.platform.release_consumer_keys();
                self.network
                    .send_control(
                        &peer,
                        &proto::ControlFrame {
                            body: Some(proto::control_frame::Body::HandoffReady(
                                proto::HandoffReady {
                                    header: Some(proto::RuntimeHeader {
                                        workspace_id: header.workspace_id,
                                        topology_revision: header.topology_revision,
                                        control_epoch: header.control_epoch,
                                        source_device_id: self
                                            .identity
                                            .service_instance_id
                                            .to_string(),
                                        target_device_id: peer.to_string(),
                                        sequence: header.sequence,
                                    }),
                                },
                            )),
                        },
                    )
                    .await?;
                tracing::info!(
                    event = "input.handoff.ready_sent",
                    target_peer = %crate::arc_input::log_peer_id(peer.as_str()),
                    control_epoch = header.control_epoch,
                    topology_revision = header.topology_revision,
                    sequence = header.sequence,
                    "Arc Input handoff ready sent"
                );
            }
            Some(proto::control_frame::Body::HandoffReady(ready)) => {
                let header = ready.header.ok_or(RuntimeError::MissingHeader)?;
                if header.source_device_id != peer.as_str()
                    || header.target_device_id != self.identity.service_instance_id.as_str()
                {
                    return Err(RuntimeError::WrongPeer);
                }
                if let Some(waiter) = lock(&self.handoff_waiters).remove(&(peer, header.sequence)) {
                    let _ = waiter.send(Ok(()));
                }
            }
            Some(proto::control_frame::Body::HandoffRejected(rejected)) => {
                let header = rejected.header.ok_or(RuntimeError::MissingHeader)?;
                if header.source_device_id != peer.as_str()
                    || header.target_device_id != self.identity.service_instance_id.as_str()
                {
                    return Err(RuntimeError::WrongPeer);
                }
                if let Some(waiter) = lock(&self.handoff_waiters).remove(&(peer, header.sequence)) {
                    tracing::warn!(
                        event = "input.handoff.rejected",
                        control_epoch = header.control_epoch,
                        topology_revision = header.topology_revision,
                        sequence = header.sequence,
                        reason = %crate::arc_input::sanitize_log_text(&rejected.reason, 512),
                        "Arc Input handoff rejected"
                    );
                    let _ = waiter.send(Err(rejected.reason));
                }
            }
            Some(proto::control_frame::Body::CommitHandoff(commit)) => {
                let header = domain_header(commit.header.ok_or(RuntimeError::MissingHeader)?)?;
                tracing::info!(
                    event = "input.handoff.commit_received",
                    source_peer = %crate::arc_input::log_peer_id(peer.as_str()),
                    control_epoch = header.control_epoch.0,
                    topology_revision = header.topology_revision.0,
                    sequence = header.sequence,
                    "Arc Input handoff commit received"
                );
                if header.source_device_id != peer
                    || header.target_device_id != self.identity.service_instance_id
                {
                    return Err(RuntimeError::WrongPeer);
                }
                let held = {
                    let mut sessions = lock(&self.session);
                    let session = sessions.as_mut().ok_or(RuntimeError::NoControlSession)?;
                    if session.controller != peer
                        || session.workspace_id != header.workspace_id
                        || session.control_epoch != header.control_epoch
                        || session.topology_revision != header.topology_revision
                        || session.next_sequence > header.sequence
                    {
                        return Err(RuntimeError::WrongPeer);
                    }
                    session.next_sequence = header.sequence;
                    session.held.clone()
                };
                self.restore_held_state(&held)?;
                self.network
                    .send_control(
                        &peer,
                        &proto::ControlFrame {
                            body: Some(proto::control_frame::Body::HandoffReady(
                                proto::HandoffReady {
                                    header: Some(proto::RuntimeHeader {
                                        workspace_id: header.workspace_id.to_string(),
                                        topology_revision: header.topology_revision.0,
                                        control_epoch: header.control_epoch.0,
                                        source_device_id: self
                                            .identity
                                            .service_instance_id
                                            .to_string(),
                                        target_device_id: peer.to_string(),
                                        sequence: header.sequence,
                                    }),
                                },
                            )),
                        },
                    )
                    .await?;
                tracing::info!(
                    event = "input.handoff.commit_acknowledged",
                    source_peer = %crate::arc_input::log_peer_id(peer.as_str()),
                    control_epoch = header.control_epoch.0,
                    topology_revision = header.topology_revision.0,
                    sequence = header.sequence,
                    "Arc Input handoff commit acknowledged"
                );
            }
            Some(proto::control_frame::Body::EmergencyRelease(release)) => {
                self.validate_control_exit(&peer, release.header.as_ref(), true)?;
                let session = lock(&self.session).take();
                if let Some(session) = session
                    .as_ref()
                    .filter(|session| session.controller == self.identity.service_instance_id)
                {
                    self.broadcast_release(session);
                }
                self.release_control_local(session)?;
            }
            Some(proto::control_frame::Body::ReleaseControl(release)) => {
                self.validate_control_exit(&peer, release.header.as_ref(), false)?;
                let session = lock(&self.session).take();
                self.release_control_local(session)?;
            }
            Some(proto::control_frame::Body::ControlRevoked(revoked)) => {
                if lock(&self.control_waiters).contains_key(&peer) {
                    let header = revoked.header.as_ref().ok_or(RuntimeError::MissingHeader)?;
                    validate_metadata_header(
                        Some(header),
                        &peer,
                        &self.identity.service_instance_id,
                    )?;
                    {
                        let session = lock(&self.session);
                        let session = session.as_ref().ok_or(RuntimeError::NoControlSession)?;
                        if session.controller != self.identity.service_instance_id
                            || session.workspace_id.as_str() != header.workspace_id
                            || session.control_epoch.0 != header.control_epoch
                            || header.topology_revision > session.topology_revision.0
                        {
                            return Err(RuntimeError::WrongPeer);
                        }
                    }
                    let waiter = lock(&self.control_waiters)
                        .remove(&peer)
                        .ok_or(RuntimeError::NoControlSession)?;
                    let _ = waiter.send(Err(revoked.reason));
                    let _ = self.events.send(RuntimeEvent::SnapshotChanged);
                    return Ok(());
                }
                self.validate_control_exit(&peer, revoked.header.as_ref(), false)?;
                let session = lock(&self.session).take();
                if let Some(session) = session
                    .as_ref()
                    .filter(|session| session.controller == self.identity.service_instance_id)
                {
                    self.broadcast_release(session);
                }
                self.release_control_local(session)?;
            }
            _ => {}
        }
        let _ = self.events.send(RuntimeEvent::SnapshotChanged);
        Ok(())
    }

    pub(super) fn validate_control_exit(
        &self,
        peer: &ServiceInstanceId,
        header: Option<&proto::RuntimeHeader>,
        allow_target_request: bool,
    ) -> Result<(), RuntimeError> {
        let header = header.ok_or(RuntimeError::MissingHeader)?;
        if header.source_device_id != peer.as_str()
            || header.target_device_id != self.identity.service_instance_id.as_str()
        {
            return Err(RuntimeError::WrongPeer);
        }
        let sessions = lock(&self.session);
        let session = sessions.as_ref().ok_or(RuntimeError::NoControlSession)?;
        if session.workspace_id.as_str() != header.workspace_id
            || session.control_epoch.0 != header.control_epoch
        {
            return Err(RuntimeError::WrongPeer);
        }
        if session.controller == self.identity.service_instance_id {
            if !allow_target_request || !lock(&self.control_participants).contains(peer) {
                return Err(RuntimeError::WrongPeer);
            }
        } else if &session.controller != peer {
            return Err(RuntimeError::WrongPeer);
        }
        Ok(())
    }

    pub(super) fn handle_input(
        &self,
        peer: ServiceInstanceId,
        batch: proto::InputEventBatch,
    ) -> Result<(), RuntimeError> {
        if !self.input_sharing_enabled() {
            return Err(RuntimeError::InputSharingDisabled);
        }
        let sent_at_unix_micros = batch.sent_at_unix_micros;
        let header = domain_header(batch.header.ok_or(RuntimeError::MissingHeader)?)?;
        if header.source_device_id != peer {
            return Err(RuntimeError::WrongPeer);
        }
        let mut session = lock(&self.session);
        let session = session.as_mut().ok_or(RuntimeError::NoControlSession)?;
        session.validate_header(&header)?;
        for event in batch.events {
            match event.event {
                Some(proto::input_event::Event::PointerMotion(pointer)) => {
                    let router = read(&self.router).clone().ok_or(RuntimeError::NoLayout)?;
                    let routing = router.snapshot();
                    let display = routing
                        .layout
                        .displays
                        .get(&session.current_display)
                        .ok_or(RuntimeError::NoDisplay)?;
                    let logical = display.logical_point_from_desk(DeskPointUm {
                        x: pointer.desk_x_um,
                        y: pointer.desk_y_um,
                    });
                    self.platform
                        .place_pointer(&session.current_display, logical)?;
                }
                Some(proto::input_event::Event::PointerButton(button)) => {
                    let usage = u16::try_from(button.hid_usage)
                        .map_err(|_| RuntimeError::InvalidInput("button usage".into()))?;
                    let click_count = u8::try_from(button.click_count)
                        .ok()
                        .filter(|count| (1..=3).contains(count))
                        .ok_or_else(|| RuntimeError::InvalidInput("button click count".into()))?;
                    self.platform
                        .pointer_button(usage, button.down, click_count)?;
                    if button.down {
                        session.held.held_mouse_buttons.insert(usage);
                    } else {
                        session.held.held_mouse_buttons.remove(&usage);
                    }
                }
                Some(proto::input_event::Event::Scroll(scroll)) => {
                    let started = Instant::now();
                    self.input_scroll_received.fetch_add(1, Ordering::Relaxed);
                    let source_supports_native =
                        local_os_family() == OsFamily::MacOs
                            && read(&self.remote_operating_systems)
                                .get(&peer)
                                .is_some_and(|family| *family == OsFamily::MacOs)
                            && read(&self.remote_capabilities).get(&peer).is_some_and(
                                |capabilities| capabilities.can_capture_native_quartz_events,
                            );
                    let local_supports_native =
                        arcrelay_input::InputInjectionPort::capabilities(self.platform.as_ref())
                            .can_inject_native_quartz_events;
                    let use_native = source_supports_native
                        && local_supports_native
                        && scroll.native_quartz_format_version
                            == arcrelay_input::NATIVE_QUARTZ_FORMAT_VERSION
                        && !scroll.native_quartz_event.is_empty()
                        && scroll.native_quartz_event.len()
                            <= arcrelay_input::MAX_NATIVE_QUARTZ_EVENT_SIZE;
                    let native_applied = if use_native {
                        match self
                            .platform
                            .native_quartz_scroll(&scroll.native_quartz_event)
                        {
                            Ok(()) => {
                                self.input_scroll_native_applied
                                    .fetch_add(1, Ordering::Relaxed);
                                true
                            }
                            Err(error) => {
                                // One warning per metrics interval, not one per
                                // high-frequency sample. Never log event bytes.
                                if self
                                    .input_scroll_native_failed
                                    .fetch_add(1, Ordering::Relaxed)
                                    == 0
                                {
                                    tracing::warn!(
                                        event = "input.scroll.native_fallback",
                                        %error,
                                        "Native Quartz scroll failed; using portable scrolling"
                                    );
                                }
                                false
                            }
                        }
                    } else {
                        false
                    };
                    if !native_applied {
                        self.platform.scroll(ScrollEvent {
                            delta_x: scroll.x_milli as f64 / 1000.0,
                            delta_y: scroll.y_milli as f64 / 1000.0,
                            unit: scroll_unit_from_proto(scroll.unit, scroll.precise),
                            phase: scroll_phase_from_proto(scroll.gesture_phase, scroll.phase),
                            momentum_phase: momentum_phase_from_proto(scroll.momentum_phase),
                        })?;
                        self.input_scroll_portable_applied
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    self.input_scroll_inject_max_micros.fetch_max(
                        started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
                        Ordering::Relaxed,
                    );
                }
                Some(proto::input_event::Event::PhysicalKey(key)) => {
                    let usage = u16::try_from(key.hid_usage)
                        .map_err(|_| RuntimeError::InvalidInput("key usage".into()))?;
                    self.platform
                        .apply_keyboard(&MappedKeyboardEvent::Physical {
                            hid_usage: usage,
                            down: key.down,
                        })?;
                    if key.down {
                        session.held.held_physical_keys.insert(usage);
                    } else {
                        session.held.held_physical_keys.remove(&usage);
                    }
                }
                Some(proto::input_event::Event::SystemGesture(value)) => {
                    if session.current_target != self.identity.service_instance_id {
                        return Err(RuntimeError::WrongPeer);
                    }
                    let format_version = value.format_version;
                    let gesture = arcrelay_input::SystemGestureEvent::try_from(value)
                        .map_err(|error| RuntimeError::InvalidInput(error.into()))?;
                    let supported = read(&self.remote_capabilities)
                        .get(&peer)
                        .is_some_and(|cap| {
                            format_version
                                <= cap.negotiated_system_gesture_version(
                                    &InputInjectionPort::capabilities(self.platform.as_ref()),
                                )
                        });
                    if !supported {
                        return Err(RuntimeError::InvalidInput(
                            "system gesture capability was not negotiated".into(),
                        ));
                    }
                    self.platform.system_gesture(gesture)?;
                    if matches!(gesture.phase, 1 | 4 | 8) {
                        self.record(
                            "system-gesture",
                            format!(
                                "received axis={} phase={} progress={:.4}",
                                gesture.axis, gesture.phase, gesture.progress
                            ),
                            None,
                        );
                    }
                }
                Some(proto::input_event::Event::SemanticChord(chord)) => {
                    let action = semantic_action(chord.action)?;
                    self.platform
                        .apply_keyboard(&MappedKeyboardEvent::Semantic {
                            action,
                            down: chord.down,
                        })?;
                }
                Some(proto::input_event::Event::TextCommit(text)) => self
                    .platform
                    .apply_keyboard(&MappedKeyboardEvent::TextCommit(text.text))?,
                Some(proto::input_event::Event::ReleaseAll(_)) => {
                    self.platform.release_all()?;
                    session.held.clear();
                }
                Some(proto::input_event::Event::ConsumerKey(value)) => {
                    let event = arcrelay_input::ConsumerKeyEvent::try_from(value)
                        .map_err(|error| RuntimeError::InvalidInput(error.into()))?;
                    let source_supports = read(&self.remote_capabilities)
                        .get(&peer)
                        .is_some_and(|caps| caps.consumer_capture_mask & event.key.mask() != 0);
                    let target_supports = InputInjectionPort::capabilities(self.platform.as_ref())
                        .consumer_mask_for_display(&session.current_display)
                        & event.key.mask()
                        != 0;
                    if session.current_target != self.identity.service_instance_id
                        || !source_supports
                        || !target_supports
                    {
                        return Err(RuntimeError::InvalidInput(
                            "consumer capability was not negotiated for this display".into(),
                        ));
                    }
                    self.platform
                        .consumer_key(event, &session.current_display)?;
                    if event.down && !event.repeat {
                        self.record("consumer-key", format!("received {:?}", event.key), None);
                    }
                }
                None => {}
            }
        }
        if batch.held_state_checksum != 0 && batch.held_state_checksum != session.held.checksum() {
            self.platform.release_all()?;
            session.held.clear();
            return Err(RuntimeError::HeldStateMismatch);
        }
        let now = unix_micros();
        if sent_at_unix_micros > 0 {
            if let Some(latency) = now.checked_sub(sent_at_unix_micros) {
                if latency <= 60_000_000 {
                    self.record(
                        "input-e2e",
                        "capture-to-inject sample".into(),
                        Some(latency),
                    );
                }
            }
        }
        Ok(())
    }

    pub(super) fn metadata_header(&self, peer: &ServiceInstanceId) -> proto::RuntimeHeader {
        let configuration = self.store.snapshot();
        let (workspace_id, topology_revision) = configuration.layout.map_or_else(
            || ("default-workspace".to_string(), 0),
            |layout| (layout.workspace_id.to_string(), layout.revision.0),
        );
        proto::RuntimeHeader {
            workspace_id,
            topology_revision,
            control_epoch: lock(&self.session).as_ref().map_or_else(
                || self.control_epoch_clock.load(Ordering::Acquire),
                |session| session.control_epoch.0,
            ),
            source_device_id: self.identity.service_instance_id.to_string(),
            target_device_id: peer.to_string(),
            sequence: 0,
        }
    }

    pub(super) fn peer_route_snapshot_frame(
        &self,
        peer: &ServiceInstanceId,
    ) -> proto::ControlFrame {
        let configuration = self.store.snapshot();
        let workspace_devices = configuration
            .layout
            .as_ref()
            .map(|layout| {
                layout
                    .displays
                    .values()
                    .map(|display| display.device_id.clone())
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let paired = read(&self.paired_peers)
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        let discovered = read(&self.discovered);
        let relayed = read(&self.relayed_routes);
        let mut routes = workspace_devices
            .into_iter()
            .filter(|device| {
                device != &self.identity.service_instance_id
                    && device != peer
                    && paired.contains(device)
            })
            .filter_map(|device| {
                let target_session = self.network.direct_relay_target(&device);
                let input_relay_available = configuration.input_sharing_enabled
                    && target_session.is_some()
                    && self.network.direct_relay_target(peer).is_some()
                    && read(&self.remote_sharing_enabled).get(&device) != Some(&false);
                if let Some(route) = discovered.get(&device) {
                    let endpoints = if route.connection_addresses.is_empty() {
                        route
                            .addresses
                            .iter()
                            .map(|address| SocketAddr::new(*address, route.port))
                            .collect::<Vec<_>>()
                    } else {
                        route.connection_addresses.clone()
                    };
                    return Some(proto::PeerRoute {
                        device_id: device.to_string(),
                        endpoints: endpoints
                            .into_iter()
                            .filter(portable_peer_route_endpoint)
                            .take(16)
                            .map(|endpoint| endpoint.to_string())
                            .collect(),
                        certificate_sha256: route.certificate_sha256_bytes.to_vec(),
                        input_relay_available,
                    });
                }
                relayed
                    .get(&device)
                    .map(|route| proto::PeerRoute {
                        device_id: device.to_string(),
                        endpoints: route
                            .endpoints
                            .iter()
                            .filter(|endpoint| portable_peer_route_endpoint(endpoint))
                            .take(16)
                            .map(ToString::to_string)
                            .collect(),
                        certificate_sha256: route.certificate_sha256.to_vec(),
                        input_relay_available,
                    })
                    .or_else(|| {
                        target_session.map(|session| proto::PeerRoute {
                            device_id: device.to_string(),
                            endpoints: Vec::new(),
                            certificate_sha256: session.peer().certificate_sha256.to_vec(),
                            input_relay_available,
                        })
                    })
            })
            .filter(|route| !route.endpoints.is_empty() || route.input_relay_available)
            .take(64)
            .collect::<Vec<_>>();
        routes.sort_by(|left, right| left.device_id.cmp(&right.device_id));
        let active_control = lock(&self.session)
            .as_ref()
            .map(|session| (session.controller.clone(), session.control_epoch))
            .or_else(|| lock(&self.observed_control).active.clone())
            .filter(|(controller, _)| {
                let component = self.input_component();
                component.contains(peer) && component.contains(controller)
            });
        let (active_controller_device_id, active_control_epoch) = active_control.map_or_else(
            || (String::new(), 0),
            |(controller, epoch)| (controller.to_string(), epoch.0),
        );
        proto::ControlFrame {
            body: Some(proto::control_frame::Body::PeerRouteSnapshot(
                proto::PeerRouteSnapshot {
                    header: Some(self.metadata_header(peer)),
                    routes,
                    active_controller_device_id,
                    active_control_epoch,
                },
            )),
        }
    }

    pub(super) fn handle_peer_route_snapshot(
        self: &Arc<Self>,
        source: ServiceInstanceId,
        snapshot: proto::PeerRouteSnapshot,
    ) -> Result<(), RuntimeError> {
        validate_metadata_header(
            snapshot.header.as_ref(),
            &source,
            &self.identity.service_instance_id,
        )?;
        if snapshot.routes.len() > 64 {
            return Err(RuntimeError::InvalidInput(
                "peer route snapshot exceeds route limit".into(),
            ));
        }
        let configuration = self.store.snapshot();
        let Some(layout) = configuration.layout else {
            return Ok(());
        };
        if snapshot
            .header
            .as_ref()
            .is_none_or(|header| header.workspace_id != layout.workspace_id.as_str())
        {
            return Err(RuntimeError::InvalidInput(
                "peer route snapshot belongs to another workspace".into(),
            ));
        }
        let workspace_devices = layout
            .displays
            .values()
            .map(|display| display.device_id.clone())
            .collect::<BTreeSet<_>>();
        let header_epoch = snapshot
            .header
            .as_ref()
            .map_or(0, |header| header.control_epoch);
        let advertised_active = match (
            snapshot.active_controller_device_id.is_empty(),
            snapshot.active_control_epoch,
        ) {
            (true, 0) => None,
            (false, epoch) if epoch > 0 => {
                let controller =
                    ServiceInstanceId::parse(snapshot.active_controller_device_id.clone())?;
                if !workspace_devices.contains(&controller) || epoch != header_epoch {
                    return Err(RuntimeError::InvalidInput(
                        "peer route snapshot has invalid active controller state".into(),
                    ));
                }
                Some((controller, ControlEpoch(epoch)))
            }
            _ => {
                return Err(RuntimeError::InvalidInput(
                    "peer route snapshot has incomplete active controller state".into(),
                ));
            }
        };
        self.control_epoch_clock
            .fetch_max(header_epoch, Ordering::AcqRel);
        if self.input_component().contains(&source) {
            let mut observed = lock(&self.observed_control);
            let clear_from_controller = advertised_active.is_none()
                && observed
                    .active
                    .as_ref()
                    .is_some_and(|(controller, _)| controller == &source);
            let relevant = advertised_active
                .as_ref()
                .is_none_or(|(controller, _)| self.input_component().contains(controller));
            if relevant
                && (advertised_active.is_some() && header_epoch >= observed.version
                    || clear_from_controller && header_epoch >= observed.version
                    || observed.active.is_none() && header_epoch > observed.version)
            {
                observed.version = header_epoch;
                observed.active = advertised_active;
            }
        }
        let paired = read(&self.paired_peers)
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut accepted = Vec::new();
        let mut relay_targets = BTreeSet::new();
        let mut changed_route_count = 0_usize;
        for route in snapshot.routes {
            let device = ServiceInstanceId::parse(route.device_id)?;
            if device == self.identity.service_instance_id
                || device == source
                || !workspace_devices.contains(&device)
                || !paired.contains(&device)
            {
                continue;
            }
            if route.endpoints.len() > 16 || route.certificate_sha256.len() != 32 {
                return Err(RuntimeError::InvalidInput(
                    "peer route has invalid endpoint or certificate data".into(),
                ));
            }
            let certificate_sha256: [u8; 32] = route
                .certificate_sha256
                .as_slice()
                .try_into()
                .map_err(|_| RuntimeError::InvalidInput("peer route certificate".into()))?;
            let mut endpoints = route
                .endpoints
                .into_iter()
                .map(|endpoint| {
                    endpoint
                        .parse::<SocketAddr>()
                        .map_err(|_| RuntimeError::InvalidInput("peer route endpoint".into()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            endpoints.retain(portable_peer_route_endpoint);
            endpoints.sort();
            endpoints.dedup();
            if endpoints.is_empty() && !route.input_relay_available {
                continue;
            }
            if route.input_relay_available {
                relay_targets.insert(device.clone());
            }
            let value = RelayedPeerRoute {
                endpoints,
                certificate_sha256,
            };
            let changed = write(&self.relayed_routes)
                .insert(device.clone(), value.clone())
                .as_ref()
                != Some(&value);
            if changed {
                changed_route_count += 1;
            }
            accepted.push((device, value));
        }
        self.network.set_relay_routes(source.clone(), relay_targets);
        if accepted.is_empty() {
            return Ok(());
        }
        tracing::info!(
            event = "input.connection.route_snapshot_received",
            peer_id = %crate::arc_input::log_peer_id(source.as_str()),
            accepted_route_count = accepted.len(),
            changed_route_count,
            "Arc Input peer routes received"
        );
        self.reconnect_peers();
        if changed_route_count > 0 {
            for peer in self
                .network
                .connected_peers()
                .into_iter()
                .filter(|peer| peer != &source)
            {
                let frame = self.peer_route_snapshot_frame(&peer);
                let network = self.network.clone();
                self.runtime_handle.spawn(async move {
                    if let Err(error) = network.send_control(&peer, &frame).await {
                        tracing::debug!(%error, "Arc Input relayed route propagation failed");
                    }
                });
            }
        }
        Ok(())
    }

    fn broadcast_peer_route_snapshots(self: &Arc<Self>) {
        for peer in self.network.connected_peers() {
            let frame = self.peer_route_snapshot_frame(&peer);
            let network = self.network.clone();
            self.runtime_handle.spawn(async move {
                if let Err(error) = network.send_control(&peer, &frame).await {
                    tracing::debug!(%error, "Arc Input relay availability update failed");
                }
            });
        }
    }

    pub(super) fn workspace_topology_sync_frame(
        &self,
        peer: &ServiceInstanceId,
        layout: &WorkspaceLayout,
    ) -> proto::ControlFrame {
        self.workspace_topology_frame_with_epoch(peer, layout, 0)
    }

    pub(super) fn workspace_topology_update_frame(
        &self,
        peer: &ServiceInstanceId,
        layout: &WorkspaceLayout,
    ) -> proto::ControlFrame {
        let epoch = lock(&self.session).as_ref().map_or(0, |session| {
            if session.controller == self.identity.service_instance_id
                && lock(&self.control_participants).contains(peer)
            {
                session.control_epoch.0
            } else {
                0
            }
        });
        self.workspace_topology_frame_with_epoch(peer, layout, epoch)
    }

    fn workspace_topology_frame_with_epoch(
        &self,
        peer: &ServiceInstanceId,
        layout: &WorkspaceLayout,
        session_epoch: u64,
    ) -> proto::ControlFrame {
        let displays = layout.displays.values().map(display_to_proto).collect();
        let portals = layout
            .portals
            .iter()
            .map(|portal| {
                let (activation_policy, modifier, dwell) = match portal.activation_policy {
                    arcrelay_input::ActivationPolicy::Immediate => (0, 0, 0),
                    arcrelay_input::ActivationPolicy::RequireModifier { hid_usage } => {
                        (1, u32::from(hid_usage), 0)
                    }
                    arcrelay_input::ActivationPolicy::Dwell { milliseconds } => {
                        (2, 0, milliseconds)
                    }
                };
                proto::Portal {
                    portal_id: portal.portal_id.to_string(),
                    source_display_id: portal.source_display.to_string(),
                    source_edge: edge_proto(portal.source_edge),
                    source_segment: Some(proto::EdgeSegment {
                        start_um: portal.source_segment.start_um,
                        end_um: portal.source_segment.end_um,
                    }),
                    target_display_id: portal.target_display.to_string(),
                    target_edge: edge_proto(portal.target_edge),
                    target_segment: Some(proto::EdgeSegment {
                        start_um: portal.target_segment.start_um,
                        end_um: portal.target_segment.end_um,
                    }),
                    direction: match portal.direction {
                        PortalDirection::OneWay => 0,
                        PortalDirection::Bidirectional => 1,
                    },
                    activation_policy,
                    inset_um: portal.inset_um,
                    hysteresis_um: portal.hysteresis_um,
                    allow_while_dragging: portal.allow_while_dragging,
                    status: portal.status as u32,
                    activation_modifier_hid_usage: modifier,
                    activation_dwell_milliseconds: dwell,
                }
            })
            .collect();
        proto::ControlFrame {
            body: Some(proto::control_frame::Body::WorkspaceTopology(
                proto::WorkspaceTopology {
                    header: Some(proto::RuntimeHeader {
                        workspace_id: layout.workspace_id.to_string(),
                        topology_revision: layout.revision.0,
                        control_epoch: session_epoch,
                        source_device_id: self.identity.service_instance_id.to_string(),
                        target_device_id: peer.to_string(),
                        sequence: 0,
                    }),
                    displays,
                    portals,
                    excluded_displays: {
                        let configuration = self.store.snapshot();
                        configuration
                            .excluded_displays
                            .iter()
                            .filter_map(|id| configuration.remembered_displays.get(id))
                            .map(display_to_proto)
                            .collect()
                    },
                },
            )),
        }
    }

    pub(super) fn control_acquisition_frames(
        &self,
        peer: &ServiceInstanceId,
        session: &ControlSession,
        layout: &WorkspaceLayout,
        expected_control_epoch: u64,
    ) -> (proto::ControlFrame, proto::ControlFrame) {
        let acquire = proto::ControlFrame {
            body: Some(proto::control_frame::Body::AcquireControl(
                proto::AcquireControl {
                    header: Some(proto::RuntimeHeader {
                        workspace_id: session.workspace_id.to_string(),
                        topology_revision: session.topology_revision.0,
                        control_epoch: session.control_epoch.0,
                        source_device_id: self.identity.service_instance_id.to_string(),
                        target_device_id: peer.to_string(),
                        sequence: session.next_sequence,
                    }),
                    expected_control_epoch,
                    participant_device_ids: (if self.is_application_receiver(peer) {
                        self.input_component()
                    } else {
                        self.input_control_component()
                    })
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                },
            )),
        };
        let topology =
            self.workspace_topology_frame_with_epoch(peer, layout, session.control_epoch.0);
        (acquire, topology)
    }

    pub(super) async fn send_acquire_control(
        &self,
        peer: &ServiceInstanceId,
    ) -> Result<(), RuntimeError> {
        // Component validation happens before ownership is granted. Refresh
        // the peer's view first, using unowned metadata; the acknowledged
        // session topology below is still required before sending any input.
        self.send_workspace_sync(peer).await?;
        let session = lock(&self.session)
            .as_ref()
            .cloned()
            .ok_or(RuntimeError::NoControlSession)?;
        let layout = self.store.snapshot().layout.ok_or(RuntimeError::NoLayout)?;
        if layout.workspace_id != session.workspace_id
            || layout.revision != session.topology_revision
        {
            return Err(RuntimeError::InvalidInput(
                "control session and workspace topology are out of sync".into(),
            ));
        }
        let expected_control_epoch = lock(&self.takeover_pending_peers)
            .remove(peer)
            .map_or(0, |epoch| epoch.0);
        let (acquire, _) =
            self.control_acquisition_frames(peer, &session, &layout, expected_control_epoch);
        let (granted_tx, granted_rx) = oneshot::channel();
        if let Some(previous) = lock(&self.control_waiters).insert(peer.clone(), granted_tx) {
            let _ = previous.send(Err("control acquisition was superseded".into()));
        }
        let sent = self.network.send_control(peer, &acquire).await;
        if let Err(error) = sent {
            lock(&self.control_waiters).remove(peer);
            if expected_control_epoch != 0 {
                lock(&self.takeover_pending_peers)
                    .insert(peer.clone(), ControlEpoch(expected_control_epoch));
            }
            return Err(error.into());
        }
        let granted = tokio::time::timeout(CONTROL_ACQUIRE_TIMEOUT, granted_rx).await;
        lock(&self.control_waiters).remove(peer);
        match granted {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(reason))) => {
                if expected_control_epoch != 0 {
                    lock(&self.takeover_pending_peers)
                        .insert(peer.clone(), ControlEpoch(expected_control_epoch));
                }
                return Err(RuntimeError::InvalidInput(reason));
            }
            Ok(Err(_)) | Err(_) => {
                if expected_control_epoch != 0 {
                    lock(&self.takeover_pending_peers)
                        .insert(peer.clone(), ControlEpoch(expected_control_epoch));
                }
                return Err(RuntimeError::ControlAcquireTimeout);
            }
        }
        let refreshed_session = lock(&self.session)
            .as_ref()
            .cloned()
            .ok_or(RuntimeError::NoControlSession)?;
        let refreshed_layout = self.store.snapshot().layout.ok_or(RuntimeError::NoLayout)?;
        if refreshed_session.controller != self.identity.service_instance_id
            || refreshed_session.control_epoch != session.control_epoch
            || refreshed_layout.workspace_id != refreshed_session.workspace_id
            || refreshed_layout.revision != refreshed_session.topology_revision
        {
            return Err(RuntimeError::InvalidInput(
                "control session changed during acquisition".into(),
            ));
        }
        let topology = self.workspace_topology_frame_with_epoch(
            peer,
            &refreshed_layout,
            refreshed_session.control_epoch.0,
        );
        // A target cannot validate PrepareHandoff or input frames against its
        // own persisted revision. Wait for ControlGranted, then send the
        // complete authoritative topology before any handoff can begin.
        let (topology_tx, topology_rx) = oneshot::channel();
        lock(&self.control_waiters).insert(peer.clone(), topology_tx);
        if let Err(error) = self.network.send_control(peer, &topology).await {
            lock(&self.control_waiters).remove(peer);
            return Err(error.into());
        }
        let topology_applied = tokio::time::timeout(CONTROL_ACQUIRE_TIMEOUT, topology_rx).await;
        lock(&self.control_waiters).remove(peer);
        match topology_applied {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(reason))) => return Err(RuntimeError::InvalidInput(reason)),
            Ok(Err(_)) | Err(_) => return Err(RuntimeError::ControlAcquireTimeout),
        }
        tracing::info!(
            event = "input.topology.sync_sent",
            peer_id = %crate::arc_input::log_peer_id(peer.as_str()),
            control_epoch = refreshed_session.control_epoch.0,
            topology_revision = refreshed_layout.revision.0,
            display_count = refreshed_layout.displays.len(),
            portal_count = refreshed_layout.portals.len(),
            "Arc Input controller topology synchronized"
        );
        Ok(())
    }

    pub(super) async fn send_workspace_sync(
        &self,
        peer: &ServiceInstanceId,
    ) -> Result<(), RuntimeError> {
        self.send_capabilities(peer).await?;
        let inventory = { read(&self.local_inventory).clone() };
        if let Some(inventory) = inventory {
            self.send_inventory(peer, inventory).await?;
        }
        if let Some(layout) = self.store.snapshot().layout {
            let topology = self.workspace_topology_sync_frame(peer, &layout);
            self.network.send_control(peer, &topology).await?;
        }
        Ok(())
    }

    pub(super) async fn send_capabilities(
        &self,
        peer: &ServiceInstanceId,
    ) -> Result<(), RuntimeError> {
        let mut value = arcrelay_input::InputCapturePort::capabilities(self.platform.as_ref());
        if !self.input_sharing_enabled() {
            value.can_capture_pointer = false;
            value.can_capture_keyboard = false;
            value.can_suppress_local_input = false;
            value.can_inject_absolute_pointer = false;
            value.can_inject_keyboard = false;
            value.can_capture_native_quartz_events = false;
            value.can_inject_native_quartz_events = false;
            value.can_capture_precision_touchpad_events = false;
            value.can_inject_precision_touchpad_events = false;
            value.can_capture_system_gestures = false;
            value.can_inject_system_gestures = false;
            value.system_gesture_format_version = 0;
            value.consumer_capture_mask = 0;
            value.consumer_inject_mask = 0;
            value.brightness_display_ids.clear();
            value.limitation = Some("cross-screen input is disabled on this device".into());
        }
        self.network
            .send_control(
                peer,
                &proto::ControlFrame {
                    body: Some(proto::control_frame::Body::CapabilitySnapshot(
                        proto::CapabilitySnapshot {
                            header: Some(self.metadata_header(peer)),
                            can_capture_pointer: value.can_capture_pointer,
                            can_capture_keyboard: value.can_capture_keyboard,
                            can_suppress_local_input: value.can_suppress_local_input,
                            can_place_internal_barrier: value.can_place_internal_barrier,
                            can_inject_absolute_pointer: value.can_inject_absolute_pointer,
                            can_inject_keyboard: value.can_inject_keyboard,
                            can_inject_app_pointer: value.can_inject_app_pointer,
                            can_control_elevated_apps: value.can_control_elevated_apps,
                            can_persist_permission: value.can_persist_permission,
                            limitation: value.limitation.unwrap_or_default(),
                            os_family: os_family_proto(local_os_family()),
                            can_capture_native_quartz_events: value
                                .can_capture_native_quartz_events,
                            can_inject_native_quartz_events: value.can_inject_native_quartz_events,
                            can_capture_precision_touchpad_events: value
                                .can_capture_precision_touchpad_events,
                            can_inject_precision_touchpad_events: value
                                .can_inject_precision_touchpad_events,
                            can_capture_system_gestures: value.can_capture_system_gestures,
                            can_inject_system_gestures: value.can_inject_system_gestures,
                            system_gesture_format_version: value.system_gesture_format_version,
                            consumer_capture_mask: value.consumer_capture_mask,
                            consumer_inject_mask: value.consumer_inject_mask,
                            brightness_display_ids: value.brightness_display_ids,
                            input_sharing_enabled: Some(self.input_sharing_enabled()),
                            supports_input_relay: self.input_sharing_enabled(),
                        },
                    )),
                },
            )
            .await?;
        Ok(())
    }

    pub(super) async fn send_inventory(
        &self,
        peer: &ServiceInstanceId,
        inventory: DisplayInventory,
    ) -> Result<(), RuntimeError> {
        let displays = inventory.displays.iter().map(display_to_proto).collect();
        self.network
            .send_control(
                peer,
                &proto::ControlFrame {
                    body: Some(proto::control_frame::Body::DisplayInventory(
                        proto::DisplayInventory {
                            header: Some(self.metadata_header(peer)),
                            inventory_revision: inventory.revision.0,
                            displays,
                        },
                    )),
                },
            )
            .await?;
        Ok(())
    }

    pub(super) fn merge_remote_inventory(
        &self,
        peer: ServiceInstanceId,
        inventory: proto::DisplayInventory,
    ) -> Result<(), RuntimeError> {
        let mut configuration = self.store.snapshot();
        let revision = InventoryRevision(inventory.inventory_revision.max(1));
        let mut displays = Vec::new();
        for display in inventory.displays {
            let width = display.physical_width_um.max(1);
            let height = display.physical_height_um.max(1);
            let display_id = DisplayId::parse(display.display_id)?;
            let confidence = match display.geometry_confidence {
                4 => arcrelay_input::GeometryConfidence::UserCalibrated,
                3 => arcrelay_input::GeometryConfidence::UserProvided,
                2 => arcrelay_input::GeometryConfidence::HardwareReported,
                1 => arcrelay_input::GeometryConfidence::Estimated,
                _ => arcrelay_input::GeometryConfidence::Unknown,
            };
            displays.push(arcrelay_input::DisplaySurface {
                display_id,
                device_id: peer.clone(),
                fingerprint: arcrelay_input::DisplayFingerprint::parse(display.fingerprint)?,
                name: display.name,
                pixel_size: arcrelay_input::SizeU32 {
                    width: display.pixel_width.max(1),
                    height: display.pixel_height.max(1),
                },
                logical_bounds: arcrelay_input::LogicalRect {
                    x: display.logical_x_milli as f64 / 1000.0,
                    y: display.logical_y_milli as f64 / 1000.0,
                    width: display.logical_width_milli as f64 / 1000.0,
                    height: display.logical_height_milli as f64 / 1000.0,
                },
                scale_factor: arcrelay_input::ScaleFactor(
                    display.scale_milli.max(1) as f64 / 1000.0,
                ),
                physical_size_um: arcrelay_input::SizeI64 { width, height },
                rotation: match display.rotation {
                    1 => arcrelay_input::DisplayRotation::Degrees90,
                    2 => arcrelay_input::DisplayRotation::Degrees180,
                    3 => arcrelay_input::DisplayRotation::Degrees270,
                    _ => arcrelay_input::DisplayRotation::Degrees0,
                },
                desk_rect_um: arcrelay_input::DeskRectUm {
                    x: display.desk_x_um,
                    y: display.desk_y_um,
                    width,
                    height,
                },
                geometry_confidence: confidence,
                inventory_revision: revision,
            });
        }
        let reported = collapse_mirrored_inventory(DisplayInventory {
            device_id: peer,
            revision,
            displays,
        });
        reported.validate()?;
        write(&self.remote_inventories).insert(reported.device_id.clone(), reported.clone());
        reconcile_device_inventory(&mut configuration, &reported)?;
        self.update_configuration(configuration)
    }

    #[cfg(test)]
    pub(super) fn apply_remote_topology(
        &self,
        layout: WorkspaceLayout,
        author: &str,
    ) -> Result<(), RuntimeError> {
        self.apply_remote_topology_with_exclusions(layout, BTreeMap::new(), author)
    }

    pub(super) fn apply_remote_topology_with_exclusions(
        &self,
        layout: WorkspaceLayout,
        excluded: BTreeMap<DisplayId, arcrelay_input::DisplaySurface>,
        author: &str,
    ) -> Result<(), RuntimeError> {
        if excluded.keys().any(|id| layout.displays.contains_key(id)) {
            return Err(RuntimeError::InvalidInput(
                "screen is both included and excluded".into(),
            ));
        }
        let mut configuration = self.store.snapshot();
        configuration.excluded_displays = excluded.keys().cloned().collect();
        configuration.remembered_displays.extend(excluded);
        configuration
            .remembered_displays
            .extend(layout.displays.clone());
        configuration.layout = Some(layout);
        configuration.topology_author = author.to_string();
        // A replicated workspace may have been authored before this client
        // learned the device's current native multi-display arrangement. Apply
        // the same deterministic system-layout normalization at the receiving
        // boundary so a stale or older peer cannot reintroduce an ambiguous
        // local/remote edge.
        rebuild_auto_portals(&mut configuration)?;
        // Availability is local observation, not an authoritative property of
        // a remote layout. In particular never activate a disconnected peer.
        self.refresh_portal_availability(&mut configuration);
        let layout = configuration
            .layout
            .as_ref()
            .ok_or(RuntimeError::NoLayout)?;
        let revision = layout.revision;
        let snapshot = RuntimeRoutingSnapshot::compile(layout.clone())?;
        self.store.save(configuration)?;
        self.reconcile_control_component()?;
        let mut router = write(&self.router);
        match router.as_ref() {
            Some(router) => router.replace(snapshot),
            None => *router = Some(Arc::new(RuntimeRouter::new(snapshot))),
        }
        drop(router);
        if let Some(session) = lock(&self.session).as_mut() {
            session.topology_revision = revision;
        }
        let _ = self.events.send(RuntimeEvent::SnapshotChanged);
        Ok(())
    }

    pub(super) fn suspend_offline_portals(
        &self,
        peer: &ServiceInstanceId,
    ) -> Result<(), RuntimeError> {
        let mut configuration = self.store.snapshot();
        let Some(layout) = configuration.layout.as_mut() else {
            return Ok(());
        };
        let display_ids = layout
            .displays
            .values()
            .filter(|display| &display.device_id == peer)
            .map(|display| display.display_id.clone())
            .collect::<Vec<_>>();
        for portal in &mut layout.portals {
            if display_ids.contains(&portal.source_display)
                || display_ids.contains(&portal.target_display)
            {
                portal.status = PortalStatus::SuspendedOffline;
            }
        }
        self.update_configuration(configuration)
    }

    pub(super) fn record(&self, category: &str, message: String, latency_micros: Option<u64>) {
        let event = diagnostic_event_name(category);
        let detail = crate::arc_input::sanitize_log_text(&message, 4_096);
        let is_warning = detail.contains("failed")
            || detail.contains("error")
            || detail.contains("rejected")
            || detail.contains("lagged")
            || detail.contains("disconnected")
            || detail.contains("mismatch")
            || detail.contains("timeout");
        // `input-e2e` is recorded in the bounded in-memory diagnostics ring for
        // UI inspection, but never forwarded to disk one event at a time. The
        // periodic pipeline summary below is the durable signal for this hot
        // path. `input-latency` is already sampled before it reaches here.
        match category {
            "input-e2e" => {}
            "input-latency" => tracing::debug!(
                event,
                category,
                detail = %detail,
                latency_micros = latency_micros.unwrap_or_default(),
                "Arc Input diagnostic sample"
            ),
            _ if is_warning => tracing::warn!(
                event,
                category,
                detail = %detail,
                latency_micros = latency_micros.unwrap_or_default(),
                "Arc Input diagnostic event"
            ),
            _ => tracing::info!(
                event,
                category,
                detail = %detail,
                latency_micros = latency_micros.unwrap_or_default(),
                "Arc Input diagnostic event"
            ),
        }
        let mut records = lock(&self.diagnostics);
        if records.len() == 256 {
            records.pop_front();
        }
        records.push_back(DiagnosticRecord {
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            category: category.to_string(),
            message,
            latency_micros,
        });
    }

    fn emit_input_metrics(&self) {
        let captured = self.input_capture_count.swap(0, Ordering::AcqRel);
        let capture_errors = self.input_capture_errors.swap(0, Ordering::AcqRel);
        let capture_max_micros = self.input_capture_max_micros.swap(0, Ordering::AcqRel);
        let scroll_captured = self.input_scroll_captured.swap(0, Ordering::AcqRel);
        let scroll_sent = self.input_scroll_sent.swap(0, Ordering::AcqRel);
        let scroll_received = self.input_scroll_received.swap(0, Ordering::AcqRel);
        let scroll_native_applied = self.input_scroll_native_applied.swap(0, Ordering::AcqRel);
        let scroll_portable_applied = self.input_scroll_portable_applied.swap(0, Ordering::AcqRel);
        let scroll_native_failed = self.input_scroll_native_failed.swap(0, Ordering::AcqRel);
        let scroll_inject_max_micros = self
            .input_scroll_inject_max_micros
            .swap(0, Ordering::AcqRel);
        let transport = self.network.take_input_metrics();
        let active = lock(&self.session).is_some();
        if !active
            && captured == 0
            && transport.enqueued == 0
            && transport.received == 0
            && transport.queue_full == 0
        {
            return;
        }
        let topology_revision = lock(&self.session)
            .as_ref()
            .map_or(0, |session| session.topology_revision.0);
        let control_epoch = lock(&self.session)
            .as_ref()
            .map_or(0, |session| session.control_epoch.0);
        if capture_errors > 0 || transport.queue_full > 0 || scroll_native_failed > 0 {
            tracing::warn!(
                event = "input.pipeline.summary",
                interval_ms = 5_000_u64,
                captured,
                capture_errors,
                capture_max_micros,
                scroll_captured,
                scroll_sent,
                scroll_received,
                scroll_native_applied,
                scroll_portable_applied,
                scroll_native_failed,
                scroll_inject_max_micros,
                enqueued = transport.enqueued,
                written = transport.written,
                received = transport.received,
                queue_full = transport.queue_full,
                queue_pending = transport.pending,
                queue_max_pending = transport.max_pending,
                topology_revision,
                control_epoch,
                "Arc Input pipeline summary contains anomalies"
            );
        } else {
            tracing::info!(
                event = "input.pipeline.summary",
                interval_ms = 5_000_u64,
                captured,
                capture_errors,
                capture_max_micros,
                scroll_captured,
                scroll_sent,
                scroll_received,
                scroll_native_applied,
                scroll_portable_applied,
                scroll_native_failed,
                scroll_inject_max_micros,
                enqueued = transport.enqueued,
                written = transport.written,
                received = transport.received,
                queue_full = transport.queue_full,
                queue_pending = transport.pending,
                queue_max_pending = transport.max_pending,
                topology_revision,
                control_epoch,
                "Arc Input pipeline summary"
            );
        }
    }
}

// An IPv6 zone index belongs to the observing host. Forwarding it as if it
// were the receiver's interface can silently dial the wrong network.
fn portable_peer_route_endpoint(endpoint: &SocketAddr) -> bool {
    let ip = endpoint.ip().to_canonical();
    endpoint.port() != 0
        && !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !matches!(ip, std::net::IpAddr::V6(ip) if ip.is_unicast_link_local())
}

fn diagnostic_event_name(category: &str) -> &'static str {
    match category {
        "connection" => "input.connection.state_changed",
        "pairing" => "input.pairing.state_changed",
        "display" => "input.display.state_changed",
        "capability" => "input.capability.state_changed",
        "control" => "input.control.state_changed",
        "protocol" => "input.protocol.error",
        "network" => "input.network.error",
        "input" => "input.pipeline.error",
        "input-latency" => "input.pipeline.latency_sample",
        "input-e2e" => "input.pipeline.e2e_sample",
        _ => "input.diagnostic",
    }
}

#[cfg(test)]
mod route_portability_tests {
    use super::*;

    #[test]
    fn relayed_routes_never_reuse_another_hosts_ipv6_zone() {
        for address in [
            "[fe80::1%4]:8765",
            "[::ffff:127.0.0.1]:8765",
            "[::]:8765",
            "224.0.0.1:8765",
        ] {
            assert!(!portable_peer_route_endpoint(&address.parse().unwrap()));
        }
        for address in [
            "192.0.2.1:8765",
            "[2001:db8::1]:8765",
            "[::ffff:192.0.2.1]:8765",
        ] {
            assert!(portable_peer_route_endpoint(&address.parse().unwrap()));
        }
    }
}
