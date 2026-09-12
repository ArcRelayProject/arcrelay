use super::*;

use arcrelay_core::domain::input_control::{
    InputEvent as DomainInputEvent, MouseButton as DomainMouseButton,
    ScrollGesturePhase as DomainScrollGesturePhase,
};
use arcrelay_protocol::server::{WorkspaceInputFocusCause, WorkspaceInputRouteState};

pub(super) fn should_recenter_after_takeover(
    previous: &ControlSession,
    local: &ServiceInstanceId,
    next_controller: &ServiceInstanceId,
) -> bool {
    &previous.controller == local && &previous.current_target != local && next_controller != local
}

pub(super) fn local_pointer_center(
    inventory: &DisplayInventory,
    current: Option<LogicalPoint>,
) -> Option<(&arcrelay_input::DisplaySurface, LogicalPoint)> {
    let display = current
        .and_then(|point| {
            inventory
                .displays
                .iter()
                .find(|display| display.contains_logical_point(point))
        })
        .or_else(|| inventory.displays.first())?;
    Some((
        display,
        LogicalPoint {
            x: display.logical_bounds.x + display.logical_bounds.width / 2.0,
            y: display.logical_bounds.y + display.logical_bounds.height / 2.0,
        },
    ))
}

pub(super) fn local_pointer_location<'a>(
    layout: &'a WorkspaceLayout,
    local: &ServiceInstanceId,
    point: LogicalPoint,
) -> Option<(&'a arcrelay_input::DisplaySurface, DeskPointUm)> {
    let display = layout
        .displays
        .values()
        .find(|display| &display.device_id == local && display.contains_logical_point(point))?;
    Some((display, display.desk_point_from_logical(point)))
}

pub(super) fn gaze_target_is_active(
    active_route: Option<&(ServiceInstanceId, DisplayId)>,
    local: &ServiceInstanceId,
    selected_target: &ServiceInstanceId,
    selected_display: &DisplayId,
    local_pointer_on_selected_display: bool,
) -> bool {
    match active_route {
        Some((target, display)) => target == selected_target && display == selected_display,
        None => selected_target == local && local_pointer_on_selected_display,
    }
}

impl ArcInputRuntime {
    pub(super) fn disable_consumer_capture(&self) {
        *lock(&self.consumer_route) = None;
        self.consumer_generation.fetch_add(1, Ordering::AcqRel);
        self.platform.set_consumer_capture(0, 0);
    }
    pub(super) fn refresh_consumer_capture(&self) {
        let route = lock(&self.session)
            .as_ref()
            .filter(|s| {
                s.controller == self.identity.service_instance_id
                    && s.current_target != self.identity.service_instance_id
            })
            .map(|s| (s.current_target.clone(), s.current_display.clone()));
        let route = route.map(|(target, display)| {
            let mask = read(&self.remote_capabilities)
                .get(&target)
                .map_or(0, |caps| caps.consumer_mask_for_display(&display))
                & InputCapturePort::capabilities(self.platform.as_ref()).consumer_capture_mask;
            (target, display, mask)
        });
        let mut previous = lock(&self.consumer_route);
        if *previous != route {
            let generation = self.consumer_generation.fetch_add(1, Ordering::AcqRel) + 1;
            let mask = route.as_ref().map_or(0, |route| route.2);
            self.platform
                .set_consumer_capture(if mask != 0 { generation } else { 0 }, mask);
            *previous = route;
        }
    }
    pub fn input_sharing_enabled(&self) -> bool {
        self.store.snapshot().input_sharing_enabled
    }

    pub fn set_input_sharing_enabled(self: &Arc<Self>, enabled: bool) -> Result<(), RuntimeError> {
        let current = self.input_sharing_enabled();
        if current == enabled {
            if enabled {
                self.start_capture()?;
            }
            return Ok(());
        }

        if enabled {
            let capabilities =
                arcrelay_input::InputCapturePort::capabilities(self.platform.as_ref());
            if !capabilities.can_source() {
                return Err(RuntimeError::Platform(
                    arcrelay_input::PlatformError::Unsupported(
                        capabilities.limitation.unwrap_or_else(|| {
                            "this platform cannot capture and suppress input".into()
                        }),
                    ),
                ));
            }
        }

        let mut configuration = self.store.snapshot();
        configuration.input_sharing_enabled = enabled;
        self.store.save(configuration)?;

        if enabled {
            if let Err(error) = self.start_capture() {
                let mut rollback = self.store.snapshot();
                rollback.input_sharing_enabled = false;
                let _ = self.store.save(rollback);
                return Err(error);
            }
            self.reconnect_peers();
        } else {
            self.network.abort_relays();
            self.release_control()?;
            self.stop_capture()?;
        }
        self.update_configuration(self.store.snapshot())?;

        for peer in self.network.connected_peers() {
            let runtime = self.clone();
            self.runtime_handle.spawn(async move {
                if let Err(error) = runtime.send_capabilities(&peer).await {
                    tracing::debug!(%error, "Arc Input capability update failed");
                }
            });
        }
        let _ = self.events.send(RuntimeEvent::SnapshotChanged);
        tracing::info!(
            event = "input.sharing.state_changed",
            enabled,
            "Arc Input continuous sharing state changed"
        );
        Ok(())
    }

    pub async fn take_control(self: &Arc<Self>) -> Result<ControlEpoch, RuntimeError> {
        if !self.input_sharing_enabled() {
            return Err(RuntimeError::InputSharingDisabled);
        }
        if !self.workspace_mesh_connected() {
            return Err(RuntimeError::WorkspaceMeshIncomplete);
        }
        let capabilities = arcrelay_input::InputCapturePort::capabilities(self.platform.as_ref());
        tracing::info!(
            event = "input.control.take_requested",
            can_source = capabilities.can_source(),
            connected_peer_count = self.network.connected_peers().len(),
            "Arc Input control requested"
        );
        if !capabilities.can_source() {
            tracing::warn!(
                event = "input.control.take_rejected",
                reason = "platform_capability",
                "Arc Input control rejected"
            );
            return Err(RuntimeError::Platform(
                arcrelay_input::PlatformError::Unsupported(
                    capabilities.limitation.unwrap_or_else(|| {
                        "this platform cannot capture and suppress input".into()
                    }),
                ),
            ));
        }
        if let Some(session) = lock(&self.session)
            .as_ref()
            .filter(|session| session.controller == self.identity.service_instance_id)
        {
            return Ok(session.control_epoch);
        }
        let layout = self.store.snapshot().layout.ok_or(RuntimeError::NoLayout)?;
        let logical_pointer = self.platform.current_pointer_position()?;
        let initial_display = layout
            .displays
            .values()
            .find(|display| {
                display.device_id == self.identity.service_instance_id
                    && display.contains_logical_point(logical_pointer)
            })
            .or_else(|| {
                layout
                    .displays
                    .values()
                    .find(|display| display.device_id == self.identity.service_instance_id)
            })
            .cloned()
            .ok_or(RuntimeError::NoDisplay)?;
        let prior_session = lock(&self.session).clone();
        let current_grant = self.arbiter.current();
        let expected_epoch = current_grant
            .as_ref()
            .filter(|grant| grant.controller != self.identity.service_instance_id)
            .and_then(|_| prior_session.as_ref().map(|session| session.control_epoch));
        if expected_epoch.is_some() {
            self.platform.release_all()?;
        }
        let grant = match current_grant {
            Some(current) if current.controller != self.identity.service_instance_id => self
                .arbiter
                .transfer(current.epoch, self.identity.service_instance_id.clone())?,
            _ => self.arbiter.acquire(
                layout.workspace_id.clone(),
                self.identity.service_instance_id.clone(),
            )?,
        };
        let token_epoch = ControlEpoch(expected_epoch.map_or_else(
            || {
                self.control_epoch_clock
                    .fetch_add(1, Ordering::AcqRel)
                    .saturating_add(1)
                    .max(1)
            },
            |expected| expected.0.saturating_add(1).max(1),
        ));
        self.control_epoch_clock
            .fetch_max(token_epoch.0, Ordering::AcqRel);
        let participants = self.input_component();
        let control_targets = participants
            .iter()
            .filter(|peer| **peer != self.identity.service_instance_id)
            .cloned()
            .collect::<Vec<_>>();
        *lock(&self.control_participants) = participants;
        {
            let mut pending = lock(&self.takeover_pending_peers);
            pending.clear();
            if let Some(expected) = expected_epoch {
                for peer in &control_targets {
                    pending.insert(peer.clone(), expected);
                }
            }
        }
        *lock(&self.session) = Some(ControlSession {
            workspace_id: layout.workspace_id,
            controller: self.identity.service_instance_id.clone(),
            current_target: initial_display.device_id.clone(),
            current_display: initial_display.display_id.clone(),
            topology_revision: layout.revision,
            control_epoch: token_epoch,
            next_sequence: 1,
            state: SessionState::Local,
            held: lock(&self.standby_held).clone(),
        });
        *lock(&self.observed_control) = ObservedControlState {
            version: token_epoch.0,
            active: Some((self.identity.service_instance_id.clone(), token_epoch)),
        };
        *lock(&self.handoff_reentry_guard) = None;
        *lock(&self.pointer) = Some(initial_display.desk_point_from_logical(logical_pointer));
        if let Err(error) = self.start_capture() {
            lock(&self.session).take();
            self.arbiter
                .release(&self.identity.service_instance_id, grant.epoch);
            lock(&self.takeover_pending_peers).clear();
            return Err(error);
        }
        let mut acquisitions = tokio::task::JoinSet::new();
        for peer in control_targets {
            let runtime = self.clone();
            acquisitions.spawn(async move {
                let result = runtime.send_acquire_control(&peer).await;
                (peer, result)
            });
        }
        while let Some(completed) = acquisitions.join_next().await {
            match completed {
                Ok((_, Ok(()))) => {}
                Ok((peer, Err(error))) => {
                    acquisitions.abort_all();
                    self.record(
                        "control",
                        format!(
                            "{} rejected automatic input ownership: {error}",
                            crate::arc_input::log_peer_id(peer.as_str())
                        ),
                        None,
                    );
                    if lock(&self.session).as_ref().is_some_and(|session| {
                        session.controller == self.identity.service_instance_id
                    }) {
                        self.release_control()?;
                    }
                    return Err(error);
                }
                Err(error) => {
                    acquisitions.abort_all();
                    if lock(&self.session).as_ref().is_some_and(|session| {
                        session.controller == self.identity.service_instance_id
                    }) {
                        self.release_control()?;
                    }
                    return Err(RuntimeError::InvalidInput(format!(
                        "control acquisition task failed: {error}"
                    )));
                }
            }
        }
        if !lock(&self.session).as_ref().is_some_and(|session| {
            session.controller == self.identity.service_instance_id
                && session.control_epoch == token_epoch
        }) {
            return Err(RuntimeError::NoControlSession);
        }
        let _ = self.events.send(RuntimeEvent::SnapshotChanged);
        tracing::info!(
            event = "input.control.acquired",
            control_epoch = token_epoch.0,
            topology_revision = layout.revision.0,
            initial_target = %crate::arc_input::log_peer_id(initial_display.device_id.as_str()),
            "Arc Input control acquired"
        );
        if self.external_input_owner().is_none() {
            self.emit_workspace_focus(WorkspaceInputFocusCause::PhysicalActivity);
        }
        if let Err(error) = self
            .apply_active_gaze_target_as_controller(
                "physical input ownership retained the active gaze target",
            )
            .await
        {
            self.record(
                "gaze",
                format!("active target restore failed: {error}"),
                None,
            );
            return Err(error);
        }
        Ok(token_epoch)
    }

    async fn take_control_from_capture(self: &Arc<Self>) -> Result<(), RuntimeError> {
        match self.take_control().await {
            Ok(_) => {
                *lock(&self.automatic_control_retry_at) = None;
                Ok(())
            }
            Err(error) => {
                // One failed ownership round must not generate another round
                // for every captured mouse sample while the mesh repairs.
                *lock(&self.automatic_control_retry_at) =
                    Some(Instant::now() + Duration::from_secs(1));
                lock(&self.remote_takeover_intent).clear();
                self.reconnect_peers();
                Err(error)
            }
        }
    }

    pub fn release_control(&self) -> Result<(), RuntimeError> {
        let session = lock(&self.session).take();
        tracing::info!(
            event = "input.control.release_requested",
            control_epoch = session
                .as_ref()
                .map_or(0, |session| session.control_epoch.0),
            held_key_count = session
                .as_ref()
                .map_or(0, |session| session.held.held_physical_keys.len()),
            held_button_count = session
                .as_ref()
                .map_or(0, |session| session.held.held_mouse_buttons.len()),
            "Arc Input control release requested"
        );
        if let Some(session) = session.as_ref() {
            self.broadcast_release(session);
        }
        self.release_control_local(session)
    }

    /// Return input ownership to this device and place the native cursor at
    /// the center of the local display that currently contains it. This is a
    /// recovery action, so it deliberately releases all held remote input
    /// before moving the pointer.
    pub fn recenter_local_pointer(&self) -> Result<(), RuntimeError> {
        self.release_control()?;
        self.place_local_pointer_at_center()
    }

    /// Placement only: ownership transfer has already been authorized by the
    /// caller. Do not broadcast a release for the newly accepted controller.
    pub(super) fn place_local_pointer_at_center(&self) -> Result<(), RuntimeError> {
        let current = self.platform.current_pointer_position().ok();
        let inventory = read(&self.local_inventory)
            .clone()
            .map(Ok)
            .unwrap_or_else(|| self.platform.inventory())?;
        let (local_display, center) =
            local_pointer_center(&inventory, current).ok_or(RuntimeError::NoDisplay)?;
        self.platform
            .place_pointer(&local_display.display_id, center)?;
        *lock(&self.pointer) = Some(local_display.desk_point_from_logical(center));
        tracing::info!(
            event = "input.pointer.recentered",
            display_id = %local_display.display_id,
            "Arc Input returned the local pointer to the display center"
        );
        Ok(())
    }

    pub(super) fn release_control_local(
        &self,
        session: Option<ControlSession>,
    ) -> Result<(), RuntimeError> {
        let unexpected_external_release =
            lock(&self.external_input_owner).take().and_then(|owner| {
                session.as_ref().map(|session| WorkspaceInputSnapshot {
                    owner_device_id: Some(owner),
                    controller_device_id: session.controller.to_string(),
                    logical_target_device_id: session.current_target.to_string(),
                    target_display_id: session.current_display.to_string(),
                    control_epoch: session.control_epoch.0,
                    supports_system_gestures: self
                        .external_system_gesture_target_version(&session.current_target)
                        != 0,
                    state: WorkspaceInputRouteState::Suspended,
                    cause: WorkspaceInputFocusCause::GatewayUnavailable,
                    message: "the input gateway released workspace control".into(),
                })
            });
        if let Some(session) = session {
            self.retire_control_session(&session);
            if let Some(grant) = self.arbiter.current() {
                if !self.arbiter.release(&session.controller, grant.epoch) {
                    self.arbiter.revoke();
                }
            }
        }
        lock(&self.takeover_pending_peers).clear();
        lock(&self.control_participants).clear();
        lock(&self.standby_held).clear();
        for (_, waiter) in std::mem::take(&mut *lock(&self.control_waiters)) {
            let _ = waiter.send(Err("control session released".into()));
        }
        *lock(&self.handoff_reentry_guard) = None;
        lock(&self.remote_takeover_intent).clear();
        let observed_version = self.control_epoch_clock.load(Ordering::Acquire);
        *lock(&self.observed_control) = ObservedControlState {
            version: observed_version,
            active: None,
        };
        if let Some(snapshot) = unexpected_external_release {
            let _ = self.workspace_input_events.send(snapshot);
        }
        for peer in self.network.connected_peers() {
            let frame = self.peer_route_snapshot_frame(&peer);
            let network = self.network.clone();
            self.runtime_handle.spawn(async move {
                if let Err(error) = network.send_control(&peer, &frame).await {
                    tracing::debug!(%error, "Arc Input control-state route update failed");
                }
            });
        }
        let suppression_result = self.platform.set_suppress_local(false);
        self.disable_consumer_capture();
        self.system_gesture_generation
            .fetch_add(1, Ordering::AcqRel);
        let gesture_result = self.platform.set_system_gesture_capture_generation(0, 0);
        let release_result = self.platform.release_all();
        suppression_result?;
        gesture_result?;
        release_result?;
        if !self.input_sharing_enabled() {
            self.stop_capture()?;
        }
        let _ = self.events.send(RuntimeEvent::SnapshotChanged);
        tracing::info!(
            event = "input.control.released",
            "Arc Input control released"
        );
        Ok(())
    }

    pub fn shutdown(&self) -> Result<(), RuntimeError> {
        self.release_control()?;
        self.stop_capture()
    }

    pub(super) fn retire_control_session(&self, session: &ControlSession) {
        let mut retired = lock(&self.retired_control_epochs);
        let epoch = retired
            .entry(session.controller.clone())
            .or_insert(session.control_epoch);
        *epoch = (*epoch).max(session.control_epoch);
    }

    fn stop_capture(&self) -> Result<(), RuntimeError> {
        if self.capture_active.swap(false, Ordering::AcqRel) {
            self.platform.stop()?;
        }
        Ok(())
    }

    pub(super) fn broadcast_release(&self, session: &ControlSession) {
        let peers = if session.controller == self.identity.service_instance_id {
            lock(&self.control_participants)
                .iter()
                .filter(|peer| **peer != self.identity.service_instance_id)
                .cloned()
                .collect()
        } else {
            vec![session.controller.clone()]
        };
        self.broadcast_release_to(session, peers);
    }

    pub(super) fn broadcast_release_to(
        &self,
        session: &ControlSession,
        peers: Vec<ServiceInstanceId>,
    ) {
        for peer in peers {
            let network = self.network.clone();
            let header = proto::RuntimeHeader {
                workspace_id: session.workspace_id.to_string(),
                topology_revision: session.topology_revision.0,
                control_epoch: session.control_epoch.0,
                source_device_id: self.identity.service_instance_id.to_string(),
                target_device_id: peer.to_string(),
                sequence: session.next_sequence,
            };
            let held_state = proto_held_state(&session.held);
            let locally_controlled = session.controller == self.identity.service_instance_id;
            self.runtime_handle.spawn(async move {
                let body = if locally_controlled {
                    proto::control_frame::Body::ReleaseControl(proto::ReleaseControl {
                        header: Some(header),
                        held_state: Some(held_state),
                    })
                } else {
                    proto::control_frame::Body::EmergencyRelease(proto::EmergencyRelease {
                        header: Some(header),
                        reason: "target emergency release".into(),
                    })
                };
                if let Err(error) = network
                    .send_control(&peer, &proto::ControlFrame { body: Some(body) })
                    .await
                {
                    tracing::debug!(%error, "Arc Input could not deliver release frame");
                }
            });
        }
    }

    pub(super) fn start_capture(self: &Arc<Self>) -> Result<(), RuntimeError> {
        self.platform
            .set_consumer_shortcuts(self.store.snapshot().consumer_shortcuts);
        if self.capture_active.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let receiver = match self.platform.start(CaptureOptions {
            suppress_local: false,
            capture_pointer: true,
            capture_keyboard: true,
        }) {
            Ok(receiver) => receiver,
            Err(error) => {
                self.capture_active.store(false, Ordering::Release);
                tracing::error!(
                    event = "input.capture.start_failed",
                    %error,
                    "Arc Input capture failed to start"
                );
                return Err(error.into());
            }
        };
        tracing::info!(
            event = "input.capture.started",
            pointer = true,
            keyboard = true,
            suppress_local = false,
            "Arc Input capture started"
        );
        // Pointer samples are intentionally coalesced before they enter the
        // async input path. A mouse can report at 1000 Hz, while a handoff can
        // legitimately wait for a network round trip. Queueing every delta
        // would replay stale edge motion after a failed handoff and immediately
        // start another one. Lan Mouse follows the same principle on its native
        // capture backends: motion is lossy/coalesced, stateful input is not.
        let pending_pointer = Arc::new(Mutex::new(PendingPointerDelta::default()));
        let pointer_for_processing = pending_pointer.clone();
        let pending_scroll = Arc::new(Mutex::new(PendingScrollDelta::default()));
        let scroll_for_processing = pending_scroll.clone();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(64);
        let pointer_ready = Arc::new(tokio::sync::Notify::new());
        let scroll_ready = Arc::new(tokio::sync::Notify::new());
        let pointer_signal = pointer_ready.clone();
        let scroll_signal = scroll_ready.clone();
        let processing_weak = Arc::downgrade(self);
        self.runtime_handle.spawn(async move {
            let pointer_tick = coalesced_wakeup::next(pointer_ready.clone(), POINTER_DISPATCH_INTERVAL);
            let scroll_tick = coalesced_wakeup::next(scroll_ready.clone(), SCROLL_DISPATCH_INTERVAL);
            tokio::pin!(pointer_tick, scroll_tick);
            loop {
                let event = tokio::select! {
                    biased;
                    event = event_rx.recv() => {
                        let Some(event) = event else { break; };
                        event
                    }
                    _ = &mut pointer_tick => {
                        pointer_tick.set(coalesced_wakeup::next(pointer_ready.clone(), POINTER_DISPATCH_INTERVAL));
                        let Some((x, y)) = lock(&pointer_for_processing).take() else {
                            continue;
                        };
                        CapturedInputEvent::PointerDelta { x, y }
                    }
                    _ = &mut scroll_tick => {
                        scroll_tick.set(coalesced_wakeup::next(scroll_ready.clone(), SCROLL_DISPATCH_INTERVAL));
                        let Some(event) = lock(&scroll_for_processing).take() else {
                            continue;
                        };
                        CapturedInputEvent::Scroll {
                            event,
                            native_quartz_event: None,
                        }
                    }
                };
                let Some(runtime) = processing_weak.upgrade() else {
                    break;
                };
                let pointer_event = matches!(event, CapturedInputEvent::PointerDelta { .. });
                let location_before = pointer_event.then(|| {
                    lock(&runtime.session).as_ref().map(|session| {
                        (
                            session.current_target.clone(),
                            session.current_display.clone(),
                        )
                    })
                });
                if let Err(error) = runtime.handle_captured(event).await {
                    runtime.record("input", error.to_string(), None);
                    // Never replay motion collected while a handoff was
                    // stalled. Releasing the transient session keeps passive
                    // capture alive so another device can become the source.
                    lock(&pointer_for_processing).clear();
                    lock(&scroll_for_processing).clear();
                    let _ = runtime.release_control();
                } else if pointer_event {
                    let location_after = lock(&runtime.session).as_ref().map(|session| {
                        (
                            session.current_target.clone(),
                            session.current_display.clone(),
                        )
                    });
                    if location_before.flatten() != location_after {
                        // Motion reported while the network handoff was in
                        // flight belongs to the old surface. Replaying it on
                        // the new surface is what caused immediate bounce-back.
                        lock(&pointer_for_processing).clear();
                        lock(&scroll_for_processing).clear();
                    }
                }
            }
        });
        let emergency_weak = Arc::downgrade(self);
        let emergency_handle = self.runtime_handle.clone();
        let pointer_for_capture = pending_pointer;
        let scroll_for_capture = pending_scroll;
        if let Err(error) = std::thread::Builder::new()
            .name("arc-input-capture-router".into())
            .spawn(move || {
                while let Ok(event) = receiver.recv() {
                    match event {
                        CapturedInputEvent::PointerDelta { x, y } => {
                            lock(&pointer_for_capture).push(x, y);
                            pointer_signal.notify_one();
                        }
                        CapturedInputEvent::Scroll {
                            event,
                            native_quartz_event: None,
                        } if event.has_delta()
                            && matches!(
                                event.phase,
                                ScrollPhase::Changed | ScrollPhase::Unspecified
                            ) =>
                        {
                            let accepted = lock(&scroll_for_capture).push(event);
                            if !accepted {
                                if let Some(pending) = lock(&scroll_for_capture).take() {
                                    if event_tx
                                        .blocking_send(CapturedInputEvent::Scroll {
                                            event: pending,
                                            native_quartz_event: None,
                                        })
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                                let pushed = lock(&scroll_for_capture).push(event);
                                debug_assert!(pushed);
                            }
                            scroll_signal.notify_one();
                        }
                        CapturedInputEvent::EmergencyRelease => {
                            // Emergency release must not wait behind a network
                            // handoff in the normal input processor.
                            if let Some(runtime) = emergency_weak.upgrade() {
                                lock(&pointer_for_capture).clear();
                                lock(&scroll_for_capture).clear();
                                emergency_handle.spawn_blocking(move || {
                                    if let Err(error) = runtime.release_control() {
                                        tracing::error!(%error, "Arc Input capture-layer emergency release failed");
                                    }
                                });
                            }
                        }
                        event => {
                            if let Some((x, y)) = lock(&pointer_for_capture).take() {
                                if event_tx
                                    .blocking_send(CapturedInputEvent::PointerDelta { x, y })
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            if let Some(pending) = lock(&scroll_for_capture).take() {
                                if event_tx
                                    .blocking_send(CapturedInputEvent::Scroll {
                                        event: pending,
                                        native_quartz_event: None,
                                    })
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            if event_tx.blocking_send(event).is_err() {
                                break;
                            }
                        }
                    }
                }
                if let Some(runtime) = emergency_weak.upgrade() {
                    runtime.capture_active.store(false, Ordering::Release);
                    let _ = runtime.events.send(RuntimeEvent::SnapshotChanged);
                }
            })
        {
            self.capture_active.store(false, Ordering::Release);
            let _ = self.platform.stop();
            return Err(RuntimeError::CaptureThread(error.to_string()));
        }
        Ok(())
    }

    pub fn route_motion(
        &self,
        display: DisplayId,
        point: DeskPointUm,
        movement: DeskVectorUm,
    ) -> Result<arcrelay_input::RouteOutcome, RuntimeError> {
        let router = read(&self.router).clone().ok_or(RuntimeError::NoLayout)?;
        Ok(router.route(
            router.revision(),
            display,
            point,
            movement,
            arcrelay_input::RoutingContext::default(),
        )?)
    }

    /// Record a stable gaze candidate. This never changes input ownership on
    /// its own; a deliberate physical pointer movement must confirm it.
    pub fn preselect_gaze_target(&self, target: &arcrelay_gaze::GazeTarget) {
        let Ok(device) = ServiceInstanceId::parse(target.device_id.clone()) else {
            return;
        };
        let Ok(display) = DisplayId::parse(target.display_id.clone()) else {
            return;
        };
        let point = DeskPointUm {
            x: target.desk_x_um,
            y: target.desk_y_um,
        };
        let valid = self.store.snapshot().layout.is_some_and(|layout| {
            layout.displays.get(&display).is_some_and(|surface| {
                surface.device_id == device && surface.desk_rect_um.contains(point)
            })
        });
        if !valid {
            return;
        }
        let far_from_consumed = lock(&self.gaze_consumed).as_ref().is_none_or(|consumed| {
            consumed.target != device
                || consumed.display != display
                || (consumed.point.x.saturating_sub(point.x) as f64)
                    .hypot(consumed.point.y.saturating_sub(point.y) as f64)
                    >= 50_000.0
        });
        if !far_from_consumed {
            return;
        }
        lock(&self.gaze_consumed).take();
        *lock(&self.gaze_preselection) = Some(GazePreselection {
            target: device,
            display,
            point,
            expires_at: Instant::now() + Duration::from_millis(900),
        });
    }

    pub fn clear_gaze_preselection(&self) {
        lock(&self.gaze_preselection).take();
        lock(&self.gaze_consumed).take();
    }

    pub async fn clear_active_gaze_target(&self) {
        self.clear_gaze_preselection();
        let local = self.identity.service_instance_id.clone();
        let cleared = {
            let mut active = lock(&self.active_gaze_target);
            if active
                .as_ref()
                .is_some_and(|selection| selection.source == local)
            {
                active.take();
                true
            } else {
                false
            }
        };
        if cleared {
            self.broadcast_gaze_target(None).await;
        }
    }

    pub fn clear_gaze_candidate(&self) {
        lock(&self.gaze_preselection).take();
    }

    /// Move Arc Input ownership and the pointer to a stable head-selected
    /// display. Held keys or buttons always block the automatic transition.
    pub async fn activate_gaze_target(
        self: &Arc<Self>,
        target: &arcrelay_gaze::GazeTarget,
    ) -> Result<(), RuntimeError> {
        if !self.input_sharing_enabled() {
            return Ok(());
        }
        self.preselect_gaze_target(target);
        if lock(&self.session)
            .as_ref()
            .is_some_and(|session| !session.held.is_empty())
        {
            return Ok(());
        }
        self.activate_gaze_preselection("stable head direction selected gaze target")
            .await
    }

    async fn confirm_gaze_preselection(
        self: &Arc<Self>,
        event: &CapturedInputEvent,
    ) -> Result<(), RuntimeError> {
        let CapturedInputEvent::PointerDelta { x, y } = event else {
            return Ok(());
        };
        // A small amount of camera noise must never move input ownership. The
        // confirmation gesture is harmless pointer movement, not a click/key.
        if (*x).hypot(*y) < 3.0 {
            return Ok(());
        }
        // Never migrate a drag or keyboard chord between machines. Keep the
        // candidate armed briefly so a later pointer move can confirm it once
        // all physical keys and buttons have been released.
        if lock(&self.session)
            .as_ref()
            .is_some_and(|session| !session.held.is_empty())
        {
            return Ok(());
        }
        self.activate_gaze_preselection("physical pointer movement confirmed gaze target")
            .await
    }

    async fn activate_gaze_preselection(
        self: &Arc<Self>,
        reason: &'static str,
    ) -> Result<(), RuntimeError> {
        let Some(selection) = lock(&self.gaze_preselection).take() else {
            return Ok(());
        };
        if Instant::now() >= selection.expires_at {
            return Ok(());
        }
        let active = ActiveGazeTarget {
            source: self.identity.service_instance_id.clone(),
            target: selection.target.clone(),
            display: selection.display.clone(),
            point: selection.point,
        };
        self.publish_gaze_target(active.clone()).await;
        let controller = lock(&self.session)
            .as_ref()
            .map(|session| session.controller.clone());
        if controller.is_none() && active.target != self.identity.service_instance_id {
            if !self.workspace_mesh_connected() {
                return Ok(());
            }
            self.take_control_from_capture().await?;
        }
        let result = self.apply_gaze_target(active, reason).await;
        if result.is_ok() {
            *lock(&self.gaze_consumed) = Some(selection);
        }
        result
    }

    async fn apply_active_gaze_target_as_controller(
        self: &Arc<Self>,
        reason: &'static str,
    ) -> Result<(), RuntimeError> {
        let Some(selection) = lock(&self.active_gaze_target).clone() else {
            return Ok(());
        };
        if !lock(&self.session).as_ref().is_some_and(|session| {
            session.controller == self.identity.service_instance_id && session.held.is_empty()
        }) {
            return Ok(());
        }
        self.apply_gaze_target(selection, reason).await
    }

    async fn apply_gaze_target(
        self: &Arc<Self>,
        selection: ActiveGazeTarget,
        reason: &'static str,
    ) -> Result<(), RuntimeError> {
        let layout = self.store.snapshot().layout.ok_or(RuntimeError::NoLayout)?;
        let surface = layout
            .displays
            .get(&selection.display)
            .filter(|surface| {
                surface.device_id == selection.target
                    && surface.desk_rect_um.contains(selection.point)
            })
            .cloned()
            .ok_or(RuntimeError::NoDisplay)?;
        let local = self.identity.service_instance_id.clone();
        let active_route = lock(&self.session).as_ref().map(|session| {
            (
                session.current_target.clone(),
                session.current_display.clone(),
            )
        });
        let local_pointer_on_selected_display = active_route.is_none()
            && selection.target == local
            && self
                .platform
                .current_pointer_position()
                .ok()
                .is_some_and(|point| surface.contains_logical_point(point));
        let already_on_selected_display = gaze_target_is_active(
            active_route.as_ref(),
            &local,
            &selection.target,
            &selection.display,
            local_pointer_on_selected_display,
        );
        if already_on_selected_display {
            return Ok(());
        }

        let controller = lock(&self.session)
            .as_ref()
            .map(|session| session.controller.clone());
        if controller
            .as_ref()
            .is_some_and(|controller| controller != &local)
        {
            // The active controller receives the same workspace gaze target
            // and performs the handoff. Stealing ownership here would race a
            // physical mouse on that controller and reset its route to local.
            return Ok(());
        } else if selection.target != local && controller.is_none() {
            return Ok(());
        }

        let current_target = lock(&self.session)
            .as_ref()
            .map(|session| session.current_target.clone());
        if current_target
            .as_ref()
            .is_some_and(|target| target != &selection.target)
        {
            let moving_from_local = current_target.as_ref() == Some(&local);
            self.disable_consumer_capture();
            self.handoff_to(selection.target.clone(), selection.display.clone())
                .await?;
            self.platform.set_native_quartz_capture_enabled(
                selection.target != local && self.native_quartz_target_supported(&selection.target),
            )?;
            let generation = self
                .system_gesture_generation
                .fetch_add(1, Ordering::AcqRel)
                + 1;
            let gesture_version = self.system_gesture_target_version(&selection.target);
            self.platform.set_system_gesture_capture_generation(
                if selection.target != local && gesture_version != 0 {
                    generation
                } else {
                    0
                },
                gesture_version,
            )?;
            if moving_from_local && selection.target != local {
                self.platform.set_suppress_local(true)?;
            }
            self.refresh_consumer_capture();
        }

        *lock(&self.pointer) = Some(selection.point);
        if selection.target == local {
            self.platform.place_pointer(
                &selection.display,
                surface.logical_point_from_desk(selection.point),
            )?;
            self.platform.set_suppress_local(false)?;
        } else {
            self.send_input_event(
                selection.target.clone(),
                proto::InputEvent {
                    event: Some(proto::input_event::Event::PointerMotion(
                        proto::PointerMotion {
                            desk_x_um: selection.point.x,
                            desk_y_um: selection.point.y,
                        },
                    )),
                },
            )
            .await?;
        }
        self.record("gaze", format!("{reason} {}", selection.display), None);
        Ok(())
    }

    async fn publish_gaze_target(&self, selection: ActiveGazeTarget) {
        let changed = {
            let mut active = lock(&self.active_gaze_target);
            if active.as_ref() == Some(&selection) {
                false
            } else {
                *active = Some(selection.clone());
                true
            }
        };
        if changed {
            self.broadcast_gaze_target(Some(&selection)).await;
        }
    }

    async fn broadcast_gaze_target(&self, selection: Option<&ActiveGazeTarget>) {
        for peer in self.network.connected_peers() {
            let frame = self.gaze_target_frame(&peer, selection);
            if let Err(error) = self.network.send_control(&peer, &frame).await {
                self.record(
                    "gaze",
                    format!(
                        "could not publish active gaze target to {}: {error}",
                        crate::arc_input::log_peer_id(peer.as_str())
                    ),
                    None,
                );
            }
        }
    }

    pub(super) fn gaze_target_frame(
        &self,
        peer: &ServiceInstanceId,
        selection: Option<&ActiveGazeTarget>,
    ) -> proto::ControlFrame {
        let (active, target_device_id, target_display_id, target_x_um, target_y_um) = selection
            .map_or_else(
                || (false, String::new(), String::new(), 0, 0),
                |selection| {
                    (
                        true,
                        selection.target.to_string(),
                        selection.display.to_string(),
                        selection.point.x,
                        selection.point.y,
                    )
                },
            );
        proto::ControlFrame {
            body: Some(proto::control_frame::Body::GazeTargetSelection(
                proto::GazeTargetSelection {
                    header: Some(self.metadata_header(peer)),
                    active,
                    target_device_id,
                    target_display_id,
                    target_x_um,
                    target_y_um,
                },
            )),
        }
    }

    pub(super) async fn apply_shared_gaze_target_if_controller(
        self: Arc<Self>,
    ) -> Result<(), RuntimeError> {
        self.apply_active_gaze_target_as_controller(
            "workspace gaze selection moved the active controller",
        )
        .await
    }

    pub(super) async fn handle_captured(
        self: &Arc<Self>,
        event: CapturedInputEvent,
    ) -> Result<(), RuntimeError> {
        let is_scroll = matches!(&event, CapturedInputEvent::Scroll { .. });
        let started = Instant::now();
        let result = self.handle_captured_inner(event).await;
        let elapsed_micros = started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        self.input_capture_count.fetch_add(1, Ordering::Relaxed);
        if is_scroll {
            self.input_scroll_captured.fetch_add(1, Ordering::Relaxed);
        }
        self.input_capture_max_micros
            .fetch_max(elapsed_micros, Ordering::Relaxed);
        if result.is_err() {
            self.input_capture_errors.fetch_add(1, Ordering::Relaxed);
        }
        let sample = self
            .input_diagnostic_counter
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(128);
        if result.is_err() || sample || elapsed_micros >= 2_000 {
            self.record(
                "input-latency",
                "capture/router/enqueue sample".into(),
                Some(elapsed_micros),
            );
        }
        result
    }

    pub(super) async fn handle_captured_inner(
        self: &Arc<Self>,
        event: CapturedInputEvent,
    ) -> Result<(), RuntimeError> {
        if matches!(event, CapturedInputEvent::EmergencyRelease) {
            return self.release_control();
        }
        let _external_gate = self.external_input_gate.lock().await;
        if !self.input_sharing_enabled() {
            return Ok(());
        }
        if let CapturedInputEvent::ConsumerKey { generation, .. } = &event {
            if *generation == 0
                || *generation != self.consumer_generation.load(Ordering::Acquire)
                || !lock(&self.session).as_ref().is_some_and(|s| {
                    s.controller == self.identity.service_instance_id
                        && s.current_target != self.identity.service_instance_id
                })
            {
                return Ok(());
            }
        }
        if let CapturedInputEvent::SystemGesture { generation, .. } = &event {
            // Native gestures never acquire ownership, and old queued events
            // cannot migrate to the next target or a new control session.
            if !self.system_gesture_route_is_current(*generation) {
                return Ok(());
            }
        }
        if self.external_input_owner().is_some() {
            let (control_epoch, target_display) = {
                let session = lock(&self.session);
                let session = session.as_ref().ok_or(RuntimeError::NoControlSession)?;
                let display = self
                    .store
                    .snapshot()
                    .layout
                    .and_then(|layout| {
                        layout
                            .displays
                            .values()
                            .find(|display| display.device_id == self.identity.service_instance_id)
                            .map(|display| display.display_id.clone())
                    })
                    .unwrap_or_else(|| session.current_display.clone());
                (session.control_epoch.0.saturating_add(1), display)
            };
            self.suspend_external_input_for_physical(
                self.identity.service_instance_id.clone(),
                target_display,
                control_epoch,
                true,
            )?;
        }
        if lock(&self.session).is_none() {
            self.emit_idle_physical_focus()?;
        }
        self.confirm_gaze_preselection(&event).await?;

        // Offline islands are not participants. Only wait for ownership
        // synchronization within the current usable screen component.
        if !self.workspace_mesh_connected() {
            return Ok(());
        }

        let controller = lock(&self.session)
            .as_ref()
            .map(|session| session.controller.clone());
        let retry_pending =
            lock(&self.automatic_control_retry_at).is_some_and(|until| Instant::now() < until);
        if retry_pending && controller.as_ref() != Some(&self.identity.service_instance_id) {
            return Ok(());
        }
        let local_owns_input = match controller {
            Some(controller) if controller == self.identity.service_instance_id => {
                lock(&self.remote_takeover_intent).clear();
                true
            }
            Some(_) => match &event {
                CapturedInputEvent::PointerDelta { x, y } => {
                    if !lock(&self.remote_takeover_intent).observe(*x, *y, Instant::now()) {
                        false
                    } else {
                        tracing::info!(
                            event = "input.control.takeover_intent",
                            motion_threshold = REMOTE_TAKEOVER_MOTION_THRESHOLD,
                            "Arc Input detected deliberate local pointer movement"
                        );
                        self.take_control_from_capture().await?;
                        true
                    }
                }
                _ => {
                    lock(&self.remote_takeover_intent).clear();
                    self.take_control_from_capture().await?;
                    true
                }
            },
            None => match &event {
                CapturedInputEvent::PointerDelta { x, y } => {
                    let crosses_remote = self.standby_pointer_crosses_remote(*x, *y)?;
                    let intended = lock(&self.remote_takeover_intent).observe_portal_crossing(
                        crosses_remote,
                        *x,
                        *y,
                        Instant::now(),
                    );
                    if !intended {
                        false
                    } else if lock(&self.session).is_some() {
                        // A remote acquisition may have arrived after the
                        // controller snapshot above. Let the next capture use
                        // the remote-owner path instead of racing it here.
                        lock(&self.remote_takeover_intent).clear();
                        false
                    } else {
                        tracing::info!(
                            event = "input.control.portal_intent",
                            motion_threshold = REMOTE_TAKEOVER_MOTION_THRESHOLD,
                            motion_window_ms = REMOTE_TAKEOVER_WINDOW.as_millis() as u64,
                            "Arc Input detected deliberate local portal crossing"
                        );
                        self.take_control_from_capture().await?;
                        true
                    }
                }
                CapturedInputEvent::PointerButton {
                    hid_usage, down, ..
                } => {
                    let mut held = lock(&self.standby_held);
                    if *down {
                        held.held_mouse_buttons.insert(*hid_usage);
                    } else {
                        held.held_mouse_buttons.remove(hid_usage);
                    }
                    false
                }
                CapturedInputEvent::Keyboard(MappedKeyboardEvent::Physical { hid_usage, down }) => {
                    let mut held = lock(&self.standby_held);
                    if *down {
                        held.held_physical_keys.insert(*hid_usage);
                    } else {
                        held.held_physical_keys.remove(hid_usage);
                    }
                    false
                }
                _ => {
                    lock(&self.remote_takeover_intent).clear();
                    false
                }
            },
        };
        if !local_owns_input {
            return Ok(());
        }

        match event {
            CapturedInputEvent::Scroll {
                event: scroll,
                native_quartz_event,
            } => {
                if self.horizontal_navigation_enabled_for_current_target()
                    && scroll.unit == ScrollUnit::Pixel
                    && (scroll.phase != ScrollPhase::Unspecified
                        || scroll.momentum_phase != ScrollMomentumPhase::Unspecified)
                {
                    let output = lock(&self.horizontal_navigation).process(scroll);
                    for scroll in output.scroll_events {
                        self.route_owned_captured_event(
                            CapturedInputEvent::Scroll {
                                event: scroll,
                                native_quartz_event: None,
                            },
                            true,
                        )
                        .await?;
                    }
                    if let Some(action) = output.action {
                        let (hid_usage, action_name) = match action {
                            HorizontalNavigationAction::Back => (4, "back"),
                            HorizontalNavigationAction::Forward => (5, "forward"),
                        };
                        self.route_owned_captured_event(
                            CapturedInputEvent::PointerButton {
                                hid_usage,
                                down: true,
                                click_count: 1,
                            },
                            true,
                        )
                        .await?;
                        self.route_owned_captured_event(
                            CapturedInputEvent::PointerButton {
                                hid_usage,
                                down: false,
                                click_count: 1,
                            },
                            true,
                        )
                        .await?;
                        tracing::info!(
                            event = "input.scroll.history_navigation",
                            action = action_name,
                            "Arc Input translated a horizontal trackpad gesture"
                        );
                    }
                    return Ok(());
                }
                lock(&self.horizontal_navigation).reset();
                self.route_owned_captured_event(
                    CapturedInputEvent::Scroll {
                        event: scroll,
                        native_quartz_event,
                    },
                    true,
                )
                .await
            }
            event => {
                lock(&self.horizontal_navigation).reset();
                self.route_owned_captured_event(event, true).await
            }
        }
    }

    fn horizontal_navigation_enabled_for_current_target(&self) -> bool {
        if local_os_family() != OsFamily::MacOs
            || self.store.horizontal_scroll_behavior() != HorizontalScrollBehavior::NavigateHistory
        {
            return false;
        }
        let target = lock(&self.session)
            .as_ref()
            .map(|session| session.current_target.clone());
        target.is_some_and(|target| {
            target != self.identity.service_instance_id
                && read(&self.remote_operating_systems)
                    .get(&target)
                    .is_some_and(|family| *family == OsFamily::Windows)
        })
    }

    async fn route_owned_captured_event(
        self: &Arc<Self>,
        event: CapturedInputEvent,
        local_event_already_applied: bool,
    ) -> Result<(), RuntimeError> {
        if let CapturedInputEvent::PointerDelta { x, y } = event {
            return self
                .route_captured_pointer(x, y, local_event_already_applied)
                .await;
        }
        // A pointer-only application cannot consume a physical keyboard.
        // Restore local control and replay the captured event (the hook had
        // suppressed it while remote), without serializing it to that receiver.
        if let CapturedInputEvent::Keyboard(ref key) = event {
            let pointer_only = lock(&self.session).as_ref().is_some_and(|session| {
                session.current_target != self.identity.service_instance_id
                    && read(&self.remote_capabilities)
                        .get(&session.current_target)
                        .is_some_and(|caps| {
                            caps.can_inject_app_pointer && !caps.can_inject_keyboard
                        })
            });
            if pointer_only {
                self.recenter_local_pointer()?;
                if local_event_already_applied {
                    self.platform.apply_keyboard(key)?;
                }
                return Ok(());
            }
        }
        let (target, event) = {
            let mut sessions = lock(&self.session);
            let session = sessions.as_mut().ok_or(RuntimeError::NoControlSession)?;
            let event = match event {
                CapturedInputEvent::ConsumerKey { event, generation } => {
                    if generation != self.consumer_generation.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    let supported = read(&self.remote_capabilities)
                        .get(&session.current_target)
                        .is_some_and(|caps| {
                            caps.consumer_mask_for_display(&session.current_display)
                                & event.key.mask()
                                != 0
                        });
                    if !supported {
                        return Ok(());
                    }
                    proto::InputEvent {
                        event: Some(proto::input_event::Event::ConsumerKey(event.into())),
                    }
                }
                CapturedInputEvent::PointerButton {
                    hid_usage,
                    down,
                    click_count,
                } => {
                    if down {
                        session.held.held_mouse_buttons.insert(hid_usage);
                    } else {
                        session.held.held_mouse_buttons.remove(&hid_usage);
                    }
                    proto::InputEvent {
                        event: Some(proto::input_event::Event::PointerButton(
                            proto::PointerButton {
                                hid_usage: u32::from(hid_usage),
                                down,
                                click_count: u32::from(click_count.clamp(1, 3)),
                            },
                        )),
                    }
                }
                CapturedInputEvent::Scroll {
                    event: scroll,
                    native_quartz_event,
                } => {
                    let native_quartz_event =
                        if self.native_quartz_target_supported(&session.current_target) {
                            native_quartz_event.unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                    proto::InputEvent {
                        event: Some(proto::input_event::Event::Scroll(proto::Scroll {
                            x_milli: (scroll.delta_x * 1000.0).round() as i64,
                            y_milli: (scroll.delta_y * 1000.0).round() as i64,
                            precise: scroll.unit == ScrollUnit::Pixel,
                            phase: proto_scroll_phase(scroll.phase) as u32,
                            native_quartz_format_version: if native_quartz_event.is_empty() {
                                0
                            } else {
                                arcrelay_input::NATIVE_QUARTZ_FORMAT_VERSION
                            },
                            native_quartz_event,
                            unit: proto_scroll_unit(scroll.unit),
                            gesture_phase: proto_scroll_phase(scroll.phase),
                            momentum_phase: proto_momentum_phase(scroll.momentum_phase),
                        })),
                    }
                }
                CapturedInputEvent::Keyboard(mapped) => {
                    let mapped = if local_event_already_applied {
                        if let MappedKeyboardEvent::Physical { hid_usage, down } = mapped {
                            let chord_active = !session.held.held_physical_keys.is_empty();
                            if down {
                                session.held.held_physical_keys.insert(hid_usage);
                            } else {
                                session.held.held_physical_keys.remove(&hid_usage);
                            }
                            let right_option_raw_mode = self.store.snapshot().right_option_raw_mode
                                && session.held.held_physical_keys.contains(&0xe6);
                            self.map_physical_keyboard(
                                &session.current_target,
                                hid_usage,
                                down,
                                chord_active,
                                right_option_raw_mode,
                            )
                        } else {
                            mapped
                        }
                    } else {
                        if let MappedKeyboardEvent::Physical { hid_usage, down } = &mapped {
                            if *down {
                                session.held.held_physical_keys.insert(*hid_usage);
                            } else {
                                session.held.held_physical_keys.remove(hid_usage);
                            }
                        }
                        mapped
                    };
                    let proto_event = mapped_keyboard_to_proto(&mapped);
                    proto::InputEvent {
                        event: Some(proto_event),
                    }
                }
                CapturedInputEvent::SystemGesture { event, generation } => {
                    if local_event_already_applied {
                        if generation != self.system_gesture_generation.load(Ordering::Acquire)
                            || session.controller != self.identity.service_instance_id
                            || event.format_version()
                                > self.system_gesture_target_version(&session.current_target)
                        {
                            return Ok(());
                        }
                    } else if event.format_version()
                        > self.external_system_gesture_target_version(&session.current_target)
                    {
                        return Ok(());
                    }
                    event
                        .validate()
                        .map_err(|error| RuntimeError::InvalidInput(error.into()))?;
                    proto::InputEvent {
                        event: Some(proto::input_event::Event::SystemGesture(event.into())),
                    }
                }
                CapturedInputEvent::PointerDelta { .. } | CapturedInputEvent::EmergencyRelease => {
                    unreachable!()
                }
            };
            (session.current_target.clone(), event)
        };
        // Local input is observed but deliberately left to the OS. Reinjecting
        // it here duplicates clicks and makes a captured pointer chase itself.
        if target != self.identity.service_instance_id {
            self.send_input_event(target, event).await?;
        } else if !local_event_already_applied {
            self.apply_local_input_event(&event)?;
        }
        Ok(())
    }

    pub(super) async fn route_external_domain_event(
        self: &Arc<Self>,
        event: DomainInputEvent,
    ) -> Result<(), RuntimeError> {
        match event {
            DomainInputEvent::PointerMove { delta_x, delta_y } => {
                self.route_owned_captured_event(
                    CapturedInputEvent::PointerDelta {
                        x: f64::from(delta_x),
                        y: f64::from(delta_y),
                    },
                    false,
                )
                .await
            }
            DomainInputEvent::PointerButton {
                button,
                down,
                click_count,
            } => {
                let hid_usage = match button {
                    DomainMouseButton::Left => 1,
                    DomainMouseButton::Right => 2,
                    DomainMouseButton::Middle => 3,
                };
                self.route_owned_captured_event(
                    CapturedInputEvent::PointerButton {
                        hid_usage,
                        down,
                        click_count,
                    },
                    false,
                )
                .await
            }
            DomainInputEvent::Scroll {
                delta_x,
                delta_y,
                precise,
            } => {
                self.route_owned_captured_event(
                    CapturedInputEvent::Scroll {
                        event: ScrollEvent {
                            delta_x: f64::from(delta_x),
                            delta_y: f64::from(delta_y),
                            unit: if precise {
                                ScrollUnit::Pixel
                            } else {
                                ScrollUnit::WheelDetent
                            },
                            ..ScrollEvent::default()
                        },
                        native_quartz_event: None,
                    },
                    false,
                )
                .await
            }
            DomainInputEvent::ScrollGesture { phase } => {
                let (phase, momentum_phase) = match phase {
                    DomainScrollGesturePhase::Began => {
                        (ScrollPhase::Began, ScrollMomentumPhase::Unspecified)
                    }
                    DomainScrollGesturePhase::Ended => {
                        (ScrollPhase::Ended, ScrollMomentumPhase::Unspecified)
                    }
                    DomainScrollGesturePhase::Cancelled => {
                        (ScrollPhase::Cancelled, ScrollMomentumPhase::Unspecified)
                    }
                    DomainScrollGesturePhase::MomentumBegan => {
                        (ScrollPhase::Unspecified, ScrollMomentumPhase::Began)
                    }
                    DomainScrollGesturePhase::MomentumEnded => {
                        (ScrollPhase::Unspecified, ScrollMomentumPhase::Ended)
                    }
                };
                self.route_owned_captured_event(
                    CapturedInputEvent::Scroll {
                        event: ScrollEvent {
                            unit: ScrollUnit::Pixel,
                            phase,
                            momentum_phase,
                            ..ScrollEvent::default()
                        },
                        native_quartz_event: None,
                    },
                    false,
                )
                .await
            }
            DomainInputEvent::SystemGesture(event) => {
                let generation = self.system_gesture_generation.load(Ordering::Acquire);
                self.route_owned_captured_event(
                    CapturedInputEvent::SystemGesture { event, generation },
                    false,
                )
                .await
            }
            DomainInputEvent::Key {
                hid_usage,
                down,
                repeat: _,
            } => {
                self.route_owned_captured_event(
                    CapturedInputEvent::Keyboard(MappedKeyboardEvent::Physical { hid_usage, down }),
                    false,
                )
                .await
            }
            DomainInputEvent::TextCommit(text) => {
                self.route_owned_captured_event(
                    CapturedInputEvent::Keyboard(MappedKeyboardEvent::TextCommit(text)),
                    false,
                )
                .await
            }
            DomainInputEvent::ReleaseAll => self.release_external_held().await,
        }
    }

    pub(super) async fn release_external_held(&self) -> Result<(), RuntimeError> {
        let target = {
            let mut sessions = lock(&self.session);
            let session = sessions.as_mut().ok_or(RuntimeError::NoControlSession)?;
            session.held.clear();
            session.current_target.clone()
        };
        if target == self.identity.service_instance_id {
            self.platform.release_all()?;
        } else {
            self.send_release_all(target).await?;
        }
        Ok(())
    }

    fn apply_local_input_event(&self, event: &proto::InputEvent) -> Result<(), RuntimeError> {
        match event.event.as_ref() {
            Some(proto::input_event::Event::PointerButton(button)) => {
                let usage = u16::try_from(button.hid_usage)
                    .map_err(|_| RuntimeError::InvalidInput("button usage".into()))?;
                let click_count = u8::try_from(button.click_count)
                    .ok()
                    .filter(|count| (1..=3).contains(count))
                    .ok_or_else(|| RuntimeError::InvalidInput("button click count".into()))?;
                self.platform
                    .pointer_button(usage, button.down, click_count)?;
            }
            Some(proto::input_event::Event::Scroll(scroll)) => {
                self.platform.scroll(ScrollEvent {
                    delta_x: scroll.x_milli as f64 / 1000.0,
                    delta_y: scroll.y_milli as f64 / 1000.0,
                    unit: scroll_unit_from_proto(scroll.unit, scroll.precise),
                    phase: scroll_phase_from_proto(scroll.gesture_phase, scroll.phase),
                    momentum_phase: momentum_phase_from_proto(scroll.momentum_phase),
                })?;
            }
            Some(proto::input_event::Event::PhysicalKey(key)) => {
                let usage = u16::try_from(key.hid_usage)
                    .map_err(|_| RuntimeError::InvalidInput("key usage".into()))?;
                self.platform
                    .apply_keyboard(&MappedKeyboardEvent::Physical {
                        hid_usage: usage,
                        down: key.down,
                    })?;
            }
            Some(proto::input_event::Event::SemanticChord(chord)) => {
                self.platform
                    .apply_keyboard(&MappedKeyboardEvent::Semantic {
                        action: semantic_action(chord.action)?,
                        down: chord.down,
                    })?;
            }
            Some(proto::input_event::Event::TextCommit(text)) => self
                .platform
                .apply_keyboard(&MappedKeyboardEvent::TextCommit(text.text.clone()))?,
            Some(proto::input_event::Event::SystemGesture(value)) => {
                let gesture = arcrelay_input::SystemGestureEvent::try_from(*value)
                    .map_err(|error| RuntimeError::InvalidInput(error.into()))?;
                self.platform.system_gesture(gesture)?;
            }
            Some(proto::input_event::Event::ReleaseAll(_)) => self.platform.release_all()?,
            Some(proto::input_event::Event::PointerMotion(_))
            | Some(proto::input_event::Event::ConsumerKey(_))
            | None => {}
        }
        Ok(())
    }

    pub(super) fn native_quartz_target_supported(&self, target: &ServiceInstanceId) -> bool {
        local_os_family() == OsFamily::MacOs
            && arcrelay_input::InputCapturePort::capabilities(self.platform.as_ref())
                .can_capture_native_quartz_events
            && read(&self.remote_operating_systems)
                .get(target)
                .is_some_and(|family| *family == OsFamily::MacOs)
            && read(&self.remote_capabilities)
                .get(target)
                .is_some_and(|capabilities| capabilities.can_inject_native_quartz_events)
    }

    pub(super) fn system_gesture_route_is_current(&self, generation: u64) -> bool {
        generation != 0
            && generation == self.system_gesture_generation.load(Ordering::Acquire)
            && lock(&self.session).as_ref().is_some_and(|session| {
                session.controller == self.identity.service_instance_id
                    && session.current_target != self.identity.service_instance_id
                    && self.system_gesture_target_supported(&session.current_target)
            })
    }

    pub(super) fn system_gesture_target_supported(&self, target: &ServiceInstanceId) -> bool {
        self.system_gesture_target_version(target) != 0
    }

    pub(super) fn system_gesture_target_version(&self, target: &ServiceInstanceId) -> u32 {
        if target == &self.identity.service_instance_id {
            return 0;
        }
        let source = InputCapturePort::capabilities(self.platform.as_ref());
        read(&self.remote_capabilities)
            .get(target)
            .map_or(0, |target| source.negotiated_system_gesture_version(target))
    }

    /// Mobile input is already a structured gesture source, so only the
    /// logical target's injection capability matters. In particular, a
    /// gateway may target either a native macOS or Windows swipe adapter.
    pub(super) fn external_system_gesture_target_version(&self, target: &ServiceInstanceId) -> u32 {
        if target == &self.identity.service_instance_id {
            let target = InputInjectionPort::capabilities(self.platform.as_ref());
            return if target.can_inject_system_gestures {
                target.system_gesture_version()
            } else {
                0
            };
        }
        read(&self.remote_capabilities)
            .get(target)
            .map_or(0, |target| {
                if target.can_inject_system_gestures {
                    target.system_gesture_version()
                } else {
                    0
                }
            })
    }

    fn standby_pointer_crosses_remote(
        &self,
        delta_x: f64,
        delta_y: f64,
    ) -> Result<bool, RuntimeError> {
        if !delta_x.is_finite() || !delta_y.is_finite() {
            return Ok(false);
        }
        let logical_pointer = self.platform.current_pointer_position()?;
        let Some(router) = read(&self.router).clone() else {
            return Ok(false);
        };
        let routing = router.snapshot();
        let layout = &routing.layout;
        let Some(display) = layout.displays.values().find(|display| {
            display.device_id == self.identity.service_instance_id
                && display.contains_logical_point(logical_pointer)
        }) else {
            return Ok(false);
        };
        let point = display.desk_point_from_logical(logical_pointer);
        let movement = DeskVectorUm {
            x: (delta_x * display.desk_rect_um.width as f64 / display.logical_bounds.width).round()
                as i64,
            y: (delta_y * display.desk_rect_um.height as f64 / display.logical_bounds.height)
                .round() as i64,
        };
        let held = lock(&self.standby_held).clone();
        let held_modifier = held
            .held_physical_keys
            .iter()
            .copied()
            .find(|usage| (0xe0..=0xe7).contains(usage));
        let base_context = arcrelay_input::RoutingContext {
            dragging: !held.held_mouse_buttons.is_empty(),
            held_modifier,
            dwell_elapsed_ms: 0,
            cross_device_only: true,
        };
        let dwell_requirement = PortalResolver::dwell_requirement(
            layout,
            &display.display_id,
            point,
            movement,
            base_context,
        )?;
        let dwell_elapsed_ms = {
            let mut gate = lock(&self.dwell_gate);
            match dwell_requirement {
                Some((portal_id, _)) => {
                    if gate
                        .as_ref()
                        .is_none_or(|current| current.portal_id != portal_id)
                    {
                        *gate = Some(DwellGate {
                            portal_id,
                            since: Instant::now(),
                        });
                        0
                    } else {
                        gate.as_ref()
                            .map_or(0, |current| current.since.elapsed().as_millis() as u32)
                    }
                }
                None => {
                    *gate = None;
                    0
                }
            }
        };
        let outcome = router.route(
            routing.topology_revision,
            display.display_id.clone(),
            point,
            movement,
            arcrelay_input::RoutingContext {
                dwell_elapsed_ms,
                ..base_context
            },
        )?;
        let target = layout
            .displays
            .get(&outcome.display_id)
            .ok_or(RuntimeError::NoDisplay)?;
        Ok(!outcome.crossings.is_empty() && target.device_id != self.identity.service_instance_id)
    }

    pub(super) fn workspace_mesh_connected(&self) -> bool {
        let component = self.input_component();
        if component.len() < 2 {
            return false;
        }
        let connected = self.network.connected_peers();
        let capabilities = read(&self.remote_capabilities);
        if component.iter().any(|peer| {
            peer != &self.identity.service_instance_id
                && (!connected.contains(peer) || !capabilities.contains_key(peer))
        }) {
            return false;
        }
        drop(capabilities);
        let observed = lock(&self.observed_control);
        observed.active.as_ref().is_none_or(|(controller, epoch)| {
            !component.contains(controller)
                || lock(&self.session).as_ref().is_some_and(|session| {
                    &session.controller == controller && &session.control_epoch == epoch
                })
        })
    }

    pub(super) fn map_physical_keyboard(
        &self,
        target: &ServiceInstanceId,
        hid_usage: u16,
        down: bool,
        chord_active: bool,
        raw_override: bool,
    ) -> MappedKeyboardEvent {
        let mut mapper = lock(&self.keyboard_mapper);
        let should_replace = mapper.is_none()
            || (mapper
                .as_ref()
                .is_some_and(|(mapped_target, _)| mapped_target != target)
                && !chord_active);
        if should_replace {
            let configuration = self.store.snapshot();
            let profile = configuration
                .keyboard_profiles
                .iter()
                .find(|profile| profile.kind == configuration.active_keyboard_profile)
                .or_else(|| configuration.keyboard_profiles.first())
                .cloned()
                .unwrap_or_else(|| KeyboardProfile::built_in(KeyboardProfileKind::Productivity));
            let target_os = if target == &self.identity.service_instance_id {
                local_os_family()
            } else {
                read(&self.remote_operating_systems)
                    .get(target)
                    .copied()
                    .unwrap_or(OsFamily::Unknown)
            };
            *mapper = Some((
                target.clone(),
                KeyboardMappingEngine::new(local_os_family(), target_os, profile),
            ));
        }
        mapper
            .as_mut()
            .expect("keyboard mapper is initialized")
            .1
            .set_raw_override(raw_override);
        mapper
            .as_mut()
            .expect("keyboard mapper is initialized")
            .1
            .map_key(hid_usage, down)
    }

    pub(super) async fn route_captured_pointer(
        &self,
        delta_x: f64,
        delta_y: f64,
        local_event_already_applied: bool,
    ) -> Result<(), RuntimeError> {
        if !delta_x.is_finite() || !delta_y.is_finite() {
            return Err(RuntimeError::InvalidInput(
                "non-finite pointer delta".into(),
            ));
        }
        let (mut current_display, current_target, revision) = {
            let sessions = lock(&self.session);
            let session = sessions.as_ref().ok_or(RuntimeError::NoControlSession)?;
            (
                session.current_display.clone(),
                session.current_target.clone(),
                session.topology_revision,
            )
        };
        let router = read(&self.router).clone().ok_or(RuntimeError::NoLayout)?;
        let routing = router.snapshot();
        let layout = &routing.layout;
        let current_is_local = current_target == self.identity.service_instance_id;
        let observed_local_point = if current_is_local && local_event_already_applied {
            let logical = self.platform.current_pointer_position()?;
            let (native_display, point) =
                local_pointer_location(layout, &self.identity.service_instance_id, logical)
                    .ok_or(RuntimeError::NoDisplay)?;
            if native_display.display_id != current_display {
                current_display = native_display.display_id.clone();
                if let Some(session) = lock(&self.session).as_mut() {
                    session.current_display = current_display.clone();
                }
                *lock(&self.handoff_reentry_guard) = None;
                *lock(&self.dwell_gate) = None;
                tracing::debug!(
                    event = "input.pointer.native_display_changed",
                    display_id = %current_display,
                    "Arc Input followed the operating system's local display transition"
                );
            }
            Some(point)
        } else {
            None
        };
        let display = layout
            .displays
            .get(&current_display)
            .ok_or(RuntimeError::NoDisplay)?;
        let movement = DeskVectorUm {
            x: (delta_x * display.desk_rect_um.width as f64 / display.logical_bounds.width).round()
                as i64,
            y: (delta_y * display.desk_rect_um.height as f64 / display.logical_bounds.height)
                .round() as i64,
        };
        let point = observed_local_point.unwrap_or_else(|| {
            lock(&self.pointer).unwrap_or(DeskPointUm {
                x: display.desk_rect_um.x + display.desk_rect_um.width / 2,
                y: display.desk_rect_um.y + display.desk_rect_um.height / 2,
            })
        });
        let held_modifier = lock(&self.session).as_ref().and_then(|session| {
            session
                .held
                .held_physical_keys
                .iter()
                .copied()
                .find(|usage| (0xe0..=0xe7).contains(usage))
        });
        let dragging = !lock(&self.session)
            .as_ref()
            .is_none_or(|session| session.held.held_mouse_buttons.is_empty());
        let base_context = arcrelay_input::RoutingContext {
            dragging,
            held_modifier,
            dwell_elapsed_ms: 0,
            cross_device_only: current_is_local && local_event_already_applied,
        };
        let dwell_requirement = PortalResolver::dwell_requirement(
            layout,
            &current_display,
            point,
            movement,
            base_context,
        )?;
        let dwell_elapsed_ms = {
            let mut gate = lock(&self.dwell_gate);
            match dwell_requirement {
                Some((portal_id, _)) => {
                    if gate
                        .as_ref()
                        .is_none_or(|current| current.portal_id != portal_id)
                    {
                        *gate = Some(DwellGate {
                            portal_id,
                            since: Instant::now(),
                        });
                        0
                    } else {
                        gate.as_ref()
                            .map_or(0, |current| current.since.elapsed().as_millis() as u32)
                    }
                }
                None => {
                    *gate = None;
                    0
                }
            }
        };
        let mut outcome = router.route(
            revision,
            current_display.clone(),
            point,
            movement,
            arcrelay_input::RoutingContext {
                dwell_elapsed_ms,
                ..base_context
            },
        )?;
        if let Some(crossing) = outcome.crossings.first() {
            let now = Instant::now();
            let mut guard = lock(&self.handoff_reentry_guard);
            let blocked = guard
                .as_ref()
                .is_some_and(|guard| guard.blocks(crossing, now));
            if guard.as_ref().is_some_and(|guard| guard.expired(now)) {
                *guard = None;
            }
            if blocked {
                let rect = display.desk_rect_um;
                let inset_x = 1.min(rect.width / 2);
                let inset_y = 1.min(rect.height / 2);
                tracing::debug!(
                    event = "input.handoff.reentry_suppressed",
                    portal_id = %crossing.portal_id,
                    source_display = %crossing.source_display,
                    target_display = %crossing.target_display,
                    "Arc Input suppressed immediate reverse portal re-entry"
                );
                outcome = arcrelay_input::RouteOutcome {
                    display_id: current_display.clone(),
                    desk_point: DeskPointUm {
                        x: point.x.saturating_add(movement.x).clamp(
                            rect.x.saturating_add(inset_x),
                            rect.x.saturating_add(rect.width).saturating_sub(inset_x),
                        ),
                        y: point.y.saturating_add(movement.y).clamp(
                            rect.y.saturating_add(inset_y),
                            rect.y.saturating_add(rect.height).saturating_sub(inset_y),
                        ),
                    },
                    crossings: Vec::new(),
                };
            }
        }
        if !outcome.crossings.is_empty() {
            *lock(&self.dwell_gate) = None;
        }
        *lock(&self.pointer) = Some(
            if current_is_local && local_event_already_applied && outcome.crossings.is_empty() {
                point
            } else {
                outcome.desk_point
            },
        );
        let target_display = layout
            .displays
            .get(&outcome.display_id)
            .ok_or(RuntimeError::NoDisplay)?;
        let target_is_local = target_display.device_id == self.identity.service_instance_id;
        let changed_target = target_display.device_id != current_target;
        let changed_display = outcome.display_id != current_display;
        if changed_target || changed_display {
            self.disable_consumer_capture();
            let transferring_from_local = current_is_local && !target_is_local;
            let held = transferring_from_local.then(|| {
                lock(&self.session)
                    .as_ref()
                    .map(|session| session.held.clone())
                    .unwrap_or_default()
            });
            // Keep local input live until the target has acknowledged the
            // handoff. If the peer is stalled or disconnected, the source must
            // remain usable instead of suppressing input for the timeout.
            if let Err(error) = self
                .handoff_to(target_display.device_id.clone(), outcome.display_id.clone())
                .await
            {
                *lock(&self.pointer) = Some(point);
                return Err(error);
            }
            if let Some(crossing) = outcome.crossings.last() {
                *lock(&self.handoff_reentry_guard) =
                    Some(HandoffReentryGuard::from_crossing(crossing, Instant::now()));
            }
            self.platform.set_native_quartz_capture_enabled(
                !target_is_local && self.native_quartz_target_supported(&target_display.device_id),
            )?;
            if changed_target {
                let generation = self
                    .system_gesture_generation
                    .fetch_add(1, Ordering::AcqRel)
                    + 1;
                let version = self.system_gesture_target_version(&target_display.device_id);
                let enabled = !target_is_local && version != 0;
                self.platform.set_system_gesture_capture_generation(
                    if enabled { generation } else { 0 },
                    version,
                )?;
                self.record(
                    "system-gesture",
                    format!(
                        "Mac system gesture forwarding {} (format v{})",
                        if enabled { "enabled" } else { "disabled" },
                        version
                    ),
                    None,
                );
            }
            if transferring_from_local {
                self.platform.set_suppress_local(true)?;
                if let Some(held) = held.as_ref() {
                    self.release_held_state_locally(held)?;
                }
            }
            self.refresh_consumer_capture();
            self.emit_workspace_focus(if self.external_input_owner().is_some() {
                WorkspaceInputFocusCause::MobilePortal
            } else {
                WorkspaceInputFocusCause::DesktopPortal
            });
        }
        if target_is_local {
            if !current_is_local || !local_event_already_applied {
                // Warp before releasing suppression so the first native event
                // continues from the restored local position.
                let place_result = self.platform.place_pointer(
                    &outcome.display_id,
                    target_display.logical_point_from_desk(outcome.desk_point),
                );
                let suppression_result = self.platform.set_suppress_local(false);
                place_result?;
                suppression_result?;
            }
        } else {
            self.send_input_event(
                target_display.device_id.clone(),
                proto::InputEvent {
                    event: Some(proto::input_event::Event::PointerMotion(
                        proto::PointerMotion {
                            desk_x_um: outcome.desk_point.x,
                            desk_y_um: outcome.desk_point.y,
                        },
                    )),
                },
            )
            .await?;
        }
        Ok(())
    }

    pub(super) async fn handoff_to(
        &self,
        target: ServiceInstanceId,
        display: DisplayId,
    ) -> Result<(), RuntimeError> {
        let (previous, epoch, revision, sequence) = {
            let sessions = lock(&self.session);
            let session = sessions.as_ref().ok_or(RuntimeError::NoControlSession)?;
            (
                session.current_target.clone(),
                session.control_epoch.0,
                session.topology_revision.0,
                session.next_sequence,
            )
        };
        let handoff_id = format!("{epoch}-{sequence}");
        let started = Instant::now();
        tracing::info!(
            event = "input.handoff.started",
            handoff_id = %handoff_id,
            source_peer = %crate::arc_input::log_peer_id(previous.as_str()),
            target_peer = %crate::arc_input::log_peer_id(target.as_str()),
            topology_revision = revision,
            control_epoch = epoch,
            sequence,
            "Arc Input handoff started"
        );
        let result = self.handoff_to_inner(target.clone(), display).await;
        match &result {
            Ok(()) => tracing::info!(
                event = "input.handoff.completed",
                handoff_id = %handoff_id,
                target_peer = %crate::arc_input::log_peer_id(target.as_str()),
                topology_revision = revision,
                control_epoch = epoch,
                sequence,
                duration_micros = started.elapsed().as_micros() as u64,
                "Arc Input handoff completed"
            ),
            Err(error) => tracing::warn!(
                event = "input.handoff.failed",
                handoff_id = %handoff_id,
                target_peer = %crate::arc_input::log_peer_id(target.as_str()),
                topology_revision = revision,
                control_epoch = epoch,
                sequence,
                duration_micros = started.elapsed().as_micros() as u64,
                %error,
                "Arc Input handoff failed"
            ),
        }
        result
    }

    async fn handoff_to_inner(
        &self,
        target: ServiceInstanceId,
        display: DisplayId,
    ) -> Result<(), RuntimeError> {
        let previous = lock(&self.session)
            .as_ref()
            .ok_or(RuntimeError::NoControlSession)?
            .current_target
            .clone();
        let (workspace_id, revision, epoch, next_sequence, source_display, held) = {
            let mut sessions = lock(&self.session);
            let session = sessions.as_mut().ok_or(RuntimeError::NoControlSession)?;
            (
                session.workspace_id.clone(),
                session.topology_revision,
                session.control_epoch,
                session.next_sequence,
                session.current_display.clone(),
                session.held.clone(),
            )
        };
        if read(&self.remote_capabilities)
            .get(&target)
            .is_some_and(|caps| caps.can_inject_app_pointer && !caps.can_inject_keyboard)
            && !held.is_empty()
        {
            return Err(RuntimeError::HandoffRejected(
                "Release keys and buttons before entering an application pointer receiver".into(),
            ));
        }
        lock(&self.session)
            .as_mut()
            .ok_or(RuntimeError::NoControlSession)?
            .begin_handoff()?;
        if target != self.identity.service_instance_id {
            let point = lock(&self.pointer).unwrap_or_default();
            let header = proto::RuntimeHeader {
                workspace_id: workspace_id.to_string(),
                topology_revision: revision.0,
                control_epoch: epoch.0,
                source_device_id: self.identity.service_instance_id.to_string(),
                target_device_id: target.to_string(),
                sequence: next_sequence,
            };
            let (ready_tx, ready_rx) = oneshot::channel();
            lock(&self.handoff_waiters).insert((target.clone(), next_sequence), ready_tx);
            tracing::info!(
                event = "input.handoff.prepare_sent",
                target_peer = %crate::arc_input::log_peer_id(target.as_str()),
                control_epoch = epoch.0,
                topology_revision = revision.0,
                sequence = next_sequence,
                "Arc Input handoff prepare sent"
            );
            let prepare_send = tokio::time::timeout(
                HANDOFF_TIMEOUT,
                self.network.send_control(
                    &target,
                    &proto::ControlFrame {
                        body: Some(proto::control_frame::Body::PrepareHandoff(
                            proto::PrepareHandoff {
                                header: Some(header.clone()),
                                source_display_id: source_display.to_string(),
                                target_display_id: display.to_string(),
                                target_x_um: point.x,
                                target_y_um: point.y,
                                held_state: Some(proto_held_state(&held)),
                            },
                        )),
                    },
                ),
            )
            .await;
            if let Err(error) = match prepare_send {
                Ok(result) => result.map_err(RuntimeError::from),
                Err(_) => Err(RuntimeError::HandoffTimeout),
            } {
                lock(&self.handoff_waiters).remove(&(target.clone(), next_sequence));
                return Err(error);
            }
            let ready = tokio::time::timeout(HANDOFF_TIMEOUT, ready_rx).await;
            lock(&self.handoff_waiters).remove(&(target.clone(), next_sequence));
            let ready = match ready {
                Ok(Ok(result)) => result,
                Ok(Err(_)) | Err(_) => {
                    if let Some(session) = lock(&self.session).as_mut() {
                        session.state = if previous == self.identity.service_instance_id {
                            SessionState::Local
                        } else {
                            SessionState::Remote
                        };
                    }
                    return Err(RuntimeError::HandoffTimeout);
                }
            };
            if let Err(reason) = ready {
                lock(&self.handoff_waiters).remove(&(target.clone(), next_sequence));
                if let Some(session) = lock(&self.session).as_mut() {
                    session.state = if previous == self.identity.service_instance_id {
                        SessionState::Local
                    } else {
                        SessionState::Remote
                    };
                }
                return Err(RuntimeError::HandoffRejected(reason));
            }
            tracing::info!(
                event = "input.handoff.ready_received",
                target_peer = %crate::arc_input::log_peer_id(target.as_str()),
                control_epoch = epoch.0,
                topology_revision = revision.0,
                sequence = next_sequence,
                "Arc Input handoff target is ready"
            );
            if previous != self.identity.service_instance_id && previous != target {
                self.send_release_all(previous.clone()).await?;
            }
            let mut commit_header = header;
            commit_header.sequence = lock(&self.session)
                .as_ref()
                .ok_or(RuntimeError::NoControlSession)?
                .next_sequence;
            let commit_sequence = commit_header.sequence;
            let (commit_tx, commit_rx) = oneshot::channel();
            lock(&self.handoff_waiters).insert((target.clone(), commit_sequence), commit_tx);
            tracing::info!(
                event = "input.handoff.commit_sent",
                target_peer = %crate::arc_input::log_peer_id(target.as_str()),
                control_epoch = epoch.0,
                topology_revision = revision.0,
                sequence = commit_sequence,
                "Arc Input handoff commit sent"
            );
            let commit_send = tokio::time::timeout(
                HANDOFF_TIMEOUT,
                self.network.send_control(
                    &target,
                    &proto::ControlFrame {
                        body: Some(proto::control_frame::Body::CommitHandoff(
                            proto::CommitHandoff {
                                header: Some(commit_header),
                            },
                        )),
                    },
                ),
            )
            .await;
            if let Err(error) = match commit_send {
                Ok(result) => result.map_err(RuntimeError::from),
                Err(_) => Err(RuntimeError::HandoffTimeout),
            } {
                lock(&self.handoff_waiters).remove(&(target.clone(), commit_sequence));
                return Err(error);
            }
            let committed = tokio::time::timeout(HANDOFF_TIMEOUT, commit_rx).await;
            lock(&self.handoff_waiters).remove(&(target.clone(), commit_sequence));
            match committed {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(reason))) => return Err(RuntimeError::HandoffRejected(reason)),
                Ok(Err(_)) | Err(_) => return Err(RuntimeError::HandoffTimeout),
            }
        } else {
            if previous != self.identity.service_instance_id {
                self.send_release_all(previous).await?;
                self.restore_held_state(&held)?;
            }
        }
        let target_is_local = target == self.identity.service_instance_id;
        let mut sessions = lock(&self.session);
        let session = sessions.as_mut().ok_or(RuntimeError::NoControlSession)?;
        session.commit_handoff(target, display)?;
        if target_is_local {
            session.state = SessionState::Local;
        }
        Ok(())
    }

    pub(super) fn restore_held_state(&self, held: &HeldInputState) -> Result<(), RuntimeError> {
        for hid_usage in held.held_physical_keys.iter().copied() {
            self.platform
                .apply_keyboard(&MappedKeyboardEvent::Physical {
                    hid_usage,
                    down: true,
                })?;
        }
        for hid_usage in held.held_mouse_buttons.iter().copied() {
            self.platform.pointer_button(hid_usage, true, 1)?;
        }
        Ok(())
    }

    pub(super) fn release_held_state_locally(
        &self,
        held: &HeldInputState,
    ) -> Result<(), RuntimeError> {
        for hid_usage in held.held_mouse_buttons.iter().copied() {
            self.platform.pointer_button(hid_usage, false, 1)?;
        }
        for hid_usage in held.held_physical_keys.iter().rev().copied() {
            self.platform
                .apply_keyboard(&MappedKeyboardEvent::Physical {
                    hid_usage,
                    down: false,
                })?;
        }
        Ok(())
    }

    pub(super) async fn send_input_event(
        &self,
        target: ServiceInstanceId,
        event: proto::InputEvent,
    ) -> Result<(), RuntimeError> {
        let is_scroll = matches!(&event.event, Some(proto::input_event::Event::Scroll(_)));
        let replace_pending_motion = matches!(
            &event.event,
            Some(proto::input_event::Event::PointerMotion(_))
        );
        let mut sessions = lock(&self.session);
        let session = sessions.as_mut().ok_or(RuntimeError::NoControlSession)?;
        let sequence = session.next_sequence;
        let batch = proto::InputEventBatch {
            header: Some(proto::RuntimeHeader {
                workspace_id: session.workspace_id.to_string(),
                topology_revision: session.topology_revision.0,
                control_epoch: session.control_epoch.0,
                source_device_id: session.controller.to_string(),
                target_device_id: target.to_string(),
                sequence,
            }),
            events: vec![event],
            held_state_checksum: session.held.checksum(),
            sent_at_unix_micros: unix_micros(),
        };
        let outcome = self
            .network
            .send_input(&target, batch, replace_pending_motion)?;
        if outcome == InputEnqueueOutcome::Queued {
            session.next_sequence = session.next_sequence.saturating_add(1);
            if is_scroll {
                self.input_scroll_sent.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    pub(super) async fn send_release_all(
        &self,
        target: ServiceInstanceId,
    ) -> Result<(), RuntimeError> {
        let mut sessions = lock(&self.session);
        let session = sessions.as_mut().ok_or(RuntimeError::NoControlSession)?;
        let sequence = session.next_sequence;
        let batch = proto::InputEventBatch {
            header: Some(proto::RuntimeHeader {
                workspace_id: session.workspace_id.to_string(),
                topology_revision: session.topology_revision.0,
                control_epoch: session.control_epoch.0,
                source_device_id: session.controller.to_string(),
                target_device_id: target.to_string(),
                sequence,
            }),
            events: vec![proto::InputEvent {
                event: Some(proto::input_event::Event::ReleaseAll(proto::ReleaseAll {})),
            }],
            held_state_checksum: 0,
            sent_at_unix_micros: unix_micros(),
        };
        self.network.send_input(&target, batch, false)?;
        session.next_sequence = session.next_sequence.saturating_add(1);
        Ok(())
    }
}

#[cfg(test)]
mod pointer_accumulator_tests {
    #[tokio::test]
    async fn app_pointer_handoff_with_held_input_preserves_the_local_session() {
        use super::*;
        let directory = tempfile::tempdir().unwrap();
        let identity = Arc::new(ProductIdentity::from_device_id("desktop-app-test").unwrap());
        let runtime = ArcInputRuntime::load(
            ProductPaths::from_root(directory.path().join("input")),
            identity.clone(),
            Arc::new(tokio::sync::OnceCell::new()),
        )
        .await
        .unwrap();
        let target = ServiceInstanceId::parse("phone-app-test").unwrap();
        write(&runtime.remote_capabilities).insert(
            target.clone(),
            PlatformCapabilities {
                can_inject_app_pointer: true,
                ..Default::default()
            },
        );
        let mut held = HeldInputState::default();
        held.held_mouse_buttons.insert(1);
        *lock(&runtime.session) = Some(ControlSession {
            workspace_id: WorkspaceId::parse("workspace").unwrap(),
            controller: identity.service_instance_id.clone(),
            current_target: identity.service_instance_id.clone(),
            current_display: DisplayId::parse("local").unwrap(),
            topology_revision: TopologyRevision(1),
            control_epoch: ControlEpoch(1),
            next_sequence: 4,
            state: SessionState::Local,
            held,
        });
        assert!(matches!(
            runtime
                .handoff_to_inner(target, DisplayId::parse("app:phone").unwrap())
                .await,
            Err(RuntimeError::HandoffRejected(_))
        ));
        let session = lock(&runtime.session).clone().unwrap();
        assert_eq!(session.state, SessionState::Local);
        assert_eq!(session.current_target, identity.service_instance_id);
        assert_eq!(session.next_sequence, 4);
    }

    use super::{
        HandoffReentryGuard, HorizontalNavigationAction, HorizontalNavigationRecognizer,
        PendingPointerDelta, PendingScrollDelta, RemoteTakeoverIntent, HANDOFF_REENTRY_GUARD,
        REMOTE_TAKEOVER_ARMED_FOR, REMOTE_TAKEOVER_WINDOW,
    };
    use arcrelay_input::{
        DeskPointUm, DisplayId, PortalCrossing, PortalId, ScrollEvent, ScrollMomentumPhase,
        ScrollPhase, ScrollUnit,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn coalesces_motion_without_losing_total_delta() {
        let mut pending = PendingPointerDelta::default();
        pending.push(1.25, -2.0);
        pending.push(0.75, 3.5);

        assert_eq!(pending.take(), Some((2.0, 1.5)));
        assert_eq!(pending.take(), None);
    }

    #[test]
    fn clear_discards_motion_collected_during_failed_handoff() {
        let mut pending = PendingPointerDelta::default();
        pending.push(30.0, -10.0);
        pending.clear();

        assert_eq!(pending.take(), None);
    }

    #[test]
    fn remote_takeover_ignores_jitter_but_accepts_deliberate_motion() {
        let started = Instant::now();
        let mut intent = RemoteTakeoverIntent::default();
        assert!(!intent.observe(2.0, 1.0, started));
        assert!(!intent.observe(-2.0, -1.0, started + Duration::from_millis(4)));
        assert!(!intent.observe(7.0, 0.0, started + Duration::from_millis(8)));
        assert!(intent.observe(6.0, 0.0, started + Duration::from_millis(12)));
    }

    #[test]
    fn remote_takeover_motion_cannot_accumulate_past_the_total_window() {
        let started = Instant::now();
        let mut intent = RemoteTakeoverIntent::default();
        assert!(!intent.observe(8.0, 0.0, started));
        assert!(!intent.observe(3.0, 0.0, started + Duration::from_millis(40)));
        assert!(!intent.observe(8.0, 0.0, started + REMOTE_TAKEOVER_WINDOW));
    }

    #[test]
    fn remote_takeover_intent_stays_armed_briefly_for_portal_arrival() {
        let started = Instant::now();
        let mut intent = RemoteTakeoverIntent::default();
        assert!(!intent.observe(7.0, 0.0, started));
        assert!(intent.observe(6.0, 0.0, started + Duration::from_millis(10)));
        assert!(intent.observe(1.0, 0.0, started + Duration::from_millis(50)));
        assert!(!intent.observe(
            1.0,
            0.0,
            started + Duration::from_millis(10) + REMOTE_TAKEOVER_ARMED_FOR
        ));
    }

    #[test]
    fn standby_portal_crossing_requires_motion_intent_in_the_same_window() {
        let started = Instant::now();
        let mut intent = RemoteTakeoverIntent::default();
        for offset in [0, 30, 60, 90, 120] {
            assert!(!intent.observe_portal_crossing(
                true,
                1.0,
                0.0,
                started + Duration::from_millis(offset)
            ));
        }

        assert!(!intent.observe_portal_crossing(
            false,
            7.0,
            0.0,
            started + Duration::from_millis(200)
        ));
        assert!(intent.observe_portal_crossing(
            true,
            6.0,
            0.0,
            started + Duration::from_millis(210)
        ));
    }

    #[test]
    fn coalesces_only_compatible_scroll_motion() {
        let mut pending = PendingScrollDelta::default();
        let first = ScrollEvent {
            delta_x: 0.25,
            delta_y: 1.5,
            unit: ScrollUnit::Pixel,
            phase: ScrollPhase::Changed,
            momentum_phase: ScrollMomentumPhase::Unspecified,
        };
        assert!(pending.push(first));
        assert!(pending.push(ScrollEvent {
            delta_x: 0.75,
            delta_y: -0.5,
            ..first
        }));
        assert_eq!(
            pending.take(),
            Some(ScrollEvent {
                delta_x: 1.0,
                delta_y: 1.0,
                ..first
            })
        );

        assert!(!pending.push(ScrollEvent {
            phase: ScrollPhase::Ended,
            ..first
        }));
        assert!(pending.push(first));
        assert!(!pending.push(ScrollEvent {
            unit: ScrollUnit::WheelDetent,
            momentum_phase: ScrollMomentumPhase::Began,
            ..first
        }));
    }

    fn trackpad_scroll(delta_x: f64, delta_y: f64, phase: ScrollPhase) -> ScrollEvent {
        ScrollEvent {
            delta_x,
            delta_y,
            unit: ScrollUnit::Pixel,
            phase,
            momentum_phase: ScrollMomentumPhase::Unspecified,
        }
    }

    #[test]
    fn horizontal_trackpad_gesture_emits_one_history_action_and_swallows_momentum() {
        let mut recognizer = HorizontalNavigationRecognizer::default();
        assert!(recognizer
            .process(trackpad_scroll(0.0, 0.0, ScrollPhase::Began))
            .scroll_events
            .is_empty());
        assert!(recognizer
            .process(trackpad_scroll(-30.0, 2.0, ScrollPhase::Changed))
            .action
            .is_none());
        assert_eq!(
            recognizer
                .process(trackpad_scroll(-55.0, 1.0, ScrollPhase::Changed))
                .action,
            Some(HorizontalNavigationAction::Back)
        );
        assert!(recognizer
            .process(trackpad_scroll(0.0, 0.0, ScrollPhase::Ended))
            .scroll_events
            .is_empty());
        assert!(recognizer
            .process(ScrollEvent {
                delta_x: -20.0,
                momentum_phase: ScrollMomentumPhase::Changed,
                ..trackpad_scroll(0.0, 0.0, ScrollPhase::Unspecified)
            })
            .scroll_events
            .is_empty());
        assert!(recognizer
            .process(ScrollEvent {
                momentum_phase: ScrollMomentumPhase::Ended,
                ..trackpad_scroll(0.0, 0.0, ScrollPhase::Unspecified)
            })
            .scroll_events
            .is_empty());
        assert_eq!(
            recognizer
                .process(trackpad_scroll(1.0, 0.0, ScrollPhase::Unspecified))
                .scroll_events
                .len(),
            1
        );
    }

    #[test]
    fn vertical_and_short_horizontal_gestures_remain_native_scrolls() {
        let mut recognizer = HorizontalNavigationRecognizer::default();
        recognizer.process(trackpad_scroll(0.0, 0.0, ScrollPhase::Began));
        let vertical = recognizer.process(trackpad_scroll(2.0, 20.0, ScrollPhase::Changed));
        assert_eq!(vertical.scroll_events.len(), 2);
        assert!(vertical.action.is_none());
        assert_eq!(
            recognizer
                .process(trackpad_scroll(0.0, 0.0, ScrollPhase::Ended))
                .scroll_events
                .len(),
            1
        );

        recognizer.process(trackpad_scroll(0.0, 0.0, ScrollPhase::Began));
        recognizer.process(trackpad_scroll(35.0, 1.0, ScrollPhase::Changed));
        let short = recognizer.process(trackpad_scroll(0.0, 0.0, ScrollPhase::Ended));
        assert_eq!(short.scroll_events.len(), 3);
        assert!(short.action.is_none());
    }

    #[test]
    fn handoff_guard_blocks_only_the_immediate_reverse_crossing() {
        let started = Instant::now();
        let forward = PortalCrossing {
            portal_id: PortalId::parse("shared-edge").unwrap(),
            source_display: DisplayId::parse("left").unwrap(),
            target_display: DisplayId::parse("right").unwrap(),
            source_point: DeskPointUm::default(),
            target_point: DeskPointUm::default(),
            fraction: 1.0,
        };
        let reverse = PortalCrossing {
            portal_id: forward.portal_id.clone(),
            source_display: forward.target_display.clone(),
            target_display: forward.source_display.clone(),
            source_point: DeskPointUm::default(),
            target_point: DeskPointUm::default(),
            fraction: 1.0,
        };
        let guard = HandoffReentryGuard::from_crossing(&forward, started);

        assert!(guard.blocks(&reverse, started + Duration::from_millis(10)));
        assert!(!guard.blocks(&forward, started + Duration::from_millis(10)));
        assert!(!guard.blocks(&reverse, started + HANDOFF_REENTRY_GUARD));
    }
}
