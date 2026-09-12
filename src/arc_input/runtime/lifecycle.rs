use super::*;

impl ArcInputRuntime {
    pub async fn load(
        paths: ProductPaths,
        identity: Arc<ProductIdentity>,
        unified_network: Arc<tokio::sync::OnceCell<Arc<NetworkRuntime>>>,
    ) -> Result<Arc<Self>, RuntimeError> {
        let runtime_handle =
            tokio::runtime::Handle::try_current().map_err(|_| NetworkError::RuntimeUnavailable)?;
        paths.create()?;
        let store = WorkspaceStore::load(paths.workspace_store.clone())?;
        let platform = Arc::new(NativePlatform::new(identity.service_instance_id.clone()));
        let initial_inventory = platform.inventory().ok().map(collapse_mirrored_inventory);
        let mut configuration = store.snapshot();
        if configuration.keyboard_profiles.is_empty() {
            configuration.keyboard_profiles = [
                KeyboardProfileKind::Productivity,
                KeyboardProfileKind::Terminal,
                KeyboardProfileKind::Ide,
                KeyboardProfileKind::RemoteDesktop,
                KeyboardProfileKind::GameRaw,
                KeyboardProfileKind::Presentation,
            ]
            .into_iter()
            .map(KeyboardProfile::built_in)
            .collect();
        }
        if configuration.layout.is_none() {
            if let Some(inventory) = initial_inventory.clone() {
                configuration.layout = Some(layout_from_inventory(inventory)?);
            }
        }
        if let Some(inventory) = initial_inventory.as_ref() {
            reconcile_device_inventory(&mut configuration, inventory)?;
        }
        if configuration.topology_author.is_empty() {
            configuration.topology_author = identity.service_instance_id.to_string();
        }
        rebuild_auto_portals(&mut configuration)?;
        suspend_initial_remote_portals(&mut configuration, &identity.service_instance_id);
        store.save(configuration.clone())?;
        let router = configuration
            .layout
            .clone()
            .map(RuntimeRoutingSnapshot::compile)
            .transpose()?
            .map(RuntimeRouter::new)
            .map(Arc::new);
        let network = PeerConnectionHub::bind(unified_network.clone())?;
        let (events, _) = broadcast::channel(128);
        let (workspace_input_events, _) = broadcast::channel(128);
        let runtime = Arc::new(Self {
            runtime_handle,
            paths,
            identity,
            store,
            unified_network,
            paired_peers: RwLock::new(BTreeMap::new()),
            network,
            platform,
            #[cfg(test)]
            availability_capabilities: RwLock::new(None),
            arbiter: ControlArbiter::default(),
            router: RwLock::new(router),
            session: Mutex::new(None),
            control_participants: Mutex::new(BTreeSet::new()),
            retired_control_epochs: Mutex::new(BTreeMap::new()),
            control_epoch_clock: AtomicU64::new(0),
            system_gesture_generation: AtomicU64::new(1),
            consumer_generation: AtomicU64::new(1),
            consumer_route: Mutex::new(None),
            observed_control: Mutex::new(ObservedControlState {
                version: 0,
                active: None,
            }),
            takeover_pending_peers: Mutex::new(BTreeMap::new()),
            control_waiters: Mutex::new(BTreeMap::new()),
            handoff_waiters: Mutex::new(BTreeMap::new()),
            handoff_reentry_guard: Mutex::new(None),
            remote_takeover_intent: Mutex::new(RemoteTakeoverIntent::default()),
            gaze_preselection: Mutex::new(None),
            gaze_consumed: Mutex::new(None),
            active_gaze_target: Mutex::new(None),
            pointer: Mutex::new(None),
            dwell_gate: Mutex::new(None),
            standby_held: Mutex::new(HeldInputState::default()),
            capture_active: AtomicBool::new(false),
            local_inventory: RwLock::new(initial_inventory),
            discovered: RwLock::new(BTreeMap::new()),
            relayed_routes: RwLock::new(BTreeMap::new()),
            reconnect_attempts: Mutex::new(BTreeMap::new()),
            automatic_control_retry_at: Mutex::new(None),
            remote_operating_systems: RwLock::new(BTreeMap::new()),
            remote_capabilities: RwLock::new(BTreeMap::new()),
            remote_sharing_enabled: RwLock::new(BTreeMap::new()),
            remote_inventories: RwLock::new(BTreeMap::new()),
            keyboard_mapper: Mutex::new(None),
            horizontal_navigation: Mutex::new(HorizontalNavigationRecognizer::default()),
            snapshot_revision: Mutex::new(0),
            diagnostics: Mutex::new(VecDeque::with_capacity(256)),
            input_diagnostic_counter: AtomicU64::new(0),
            input_capture_count: AtomicU64::new(0),
            input_capture_errors: AtomicU64::new(0),
            input_capture_max_micros: AtomicU64::new(0),
            input_scroll_captured: AtomicU64::new(0),
            input_scroll_sent: AtomicU64::new(0),
            input_scroll_received: AtomicU64::new(0),
            input_scroll_native_applied: AtomicU64::new(0),
            input_scroll_portable_applied: AtomicU64::new(0),
            input_scroll_native_failed: AtomicU64::new(0),
            input_scroll_inject_max_micros: AtomicU64::new(0),
            events,
            external_input_owner: Mutex::new(None),
            external_input_gate: AsyncMutex::new(()),
            workspace_input_events,
            last_physical_focus_emitted: Mutex::new(None),
        });
        let weak = Arc::downgrade(&runtime);
        runtime.network.set_relay_authorizer(move |request, via| {
            let Some(runtime) = weak.upgrade() else {
                return false;
            };
            let configuration = runtime.store.snapshot();
            configuration.input_sharing_enabled
                && configuration.layout.as_ref().is_some_and(|layout| {
                    layout.workspace_id.as_str() == request.workspace_id
                        && [
                            request.source_device_id.as_str(),
                            request.target_device_id.as_str(),
                            via.as_str(),
                            runtime.identity.service_instance_id.as_str(),
                        ]
                        .iter()
                        .all(|id| {
                            layout
                                .displays
                                .values()
                                .any(|display| display.device_id.as_str() == *id)
                        })
                })
        });
        Ok(runtime)
    }

    pub fn identity(&self) -> &ProductIdentity {
        &self.identity
    }

    pub fn paths(&self) -> &ProductPaths {
        &self.paths
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.events.subscribe()
    }

    pub(super) async fn refresh_paired_peers(self: &Arc<Self>) -> Result<(), RuntimeError> {
        let peers = wait_for_network(&self.unified_network)
            .await
            .paired_peers()
            .await
            .map_err(NetworkError::from)?;
        let paired = peers
            .into_iter()
            .filter_map(|peer| {
                ServiceInstanceId::parse(peer.device_id.to_string())
                    .ok()
                    .map(|id| (id, peer))
            })
            .collect::<BTreeMap<_, _>>();
        *write(&self.paired_peers) = paired;
        self.reconnect_peers();
        Ok(())
    }

    pub(super) fn is_paired(&self, peer: &ServiceInstanceId) -> bool {
        read(&self.paired_peers).contains_key(peer)
    }

    pub async fn connect_peer(
        &self,
        peer: ServiceInstanceId,
        addresses: Vec<std::net::IpAddr>,
        port: u16,
    ) -> Result<(), RuntimeError> {
        let peer_id = crate::arc_input::log_peer_id(peer.as_str());
        let started = Instant::now();
        if let Err(error) = self.network.connect(peer, &addresses, port).await {
            tracing::warn!(
                event = "input.connection.connect_failed",
                %peer_id,
                duration_ms = started.elapsed().as_millis() as u64,
                %error,
                "Arc Input connection failed"
            );
            return Err(error.into());
        }
        tracing::info!(
            event = "input.connection.connect_completed",
            %peer_id,
            duration_ms = started.elapsed().as_millis() as u64,
            "Arc Input connection completed"
        );
        Ok(())
    }

    pub async fn forget_peer(&self, peer: ServiceInstanceId) -> Result<(), RuntimeError> {
        let peer_owns_session = lock(&self.session)
            .as_ref()
            .is_some_and(|session| session.controller == peer || session.current_target == peer);
        if peer_owns_session {
            self.release_control()?;
        }

        self.network.disconnect(&peer, "device pairing removed");
        let device_id = DeviceId::parse(peer.as_str())?;
        wait_for_network(&self.unified_network)
            .await
            .forget(&device_id)
            .await
            .map_err(NetworkError::from)?;
        write(&self.paired_peers).remove(&peer);
        write(&self.remote_operating_systems).remove(&peer);
        write(&self.remote_capabilities).remove(&peer);
        write(&self.remote_sharing_enabled).remove(&peer);
        write(&self.remote_inventories).remove(&peer);
        if lock(&self.keyboard_mapper)
            .as_ref()
            .is_some_and(|(mapped_peer, _)| mapped_peer == &peer)
        {
            *lock(&self.keyboard_mapper) = None;
        }

        let mut configuration = self.store.snapshot();
        if let Some(layout) = configuration.layout.as_mut() {
            let removed_displays = layout
                .displays
                .values()
                .filter(|display| display.device_id == peer)
                .map(|display| display.display_id.clone())
                .collect::<Vec<_>>();
            layout
                .displays
                .retain(|_, display| display.device_id != peer);
            layout.portals.retain(|portal| {
                !removed_displays.contains(&portal.source_display)
                    && !removed_displays.contains(&portal.target_display)
            });
        }
        configuration.excluded_displays.retain(|id| {
            configuration
                .remembered_displays
                .get(id)
                .is_some_and(|display| display.device_id != peer)
        });
        configuration
            .remembered_displays
            .retain(|_, display| display.device_id != peer);
        self.update_configuration(configuration)?;
        self.record(
            "pairing",
            format!(
                "removed paired device {}",
                crate::arc_input::log_peer_id(peer.as_str())
            ),
            None,
        );
        let _ = self.events.send(RuntimeEvent::SnapshotChanged);
        Ok(())
    }

    pub fn snapshot(&self) -> RuntimeSnapshot {
        let mut revision = lock(&self.snapshot_revision);
        *revision += 1;
        let grant = self.arbiter.current();
        let connected = self.network.connected_peers();
        let discovered = read(&self.discovered);
        RuntimeSnapshot {
            revision: *revision,
            product_id: arcrelay_input::PRODUCT_ID,
            service_instance_id: self.identity.service_instance_id.to_string(),
            local_operating_system: local_os_family(),
            remote_operating_systems: read(&self.remote_operating_systems)
                .iter()
                .map(|(peer, family)| (peer.to_string(), *family))
                .collect(),
            capabilities: arcrelay_input::InputCapturePort::capabilities(self.platform.as_ref()),
            configuration: self.store.snapshot(),
            discovered_peers: discovered.keys().map(ToString::to_string).collect(),
            nearby_peers: discovered
                .values()
                .map(|peer| NearbyPeerView {
                    service_instance_id: peer.service_instance_id.to_string(),
                    display_name: read(&self.paired_peers)
                        .get(&peer.service_instance_id)
                        .map(|paired| paired.display_name.clone())
                        .or_else(|| Some(peer.display_name.clone())),
                    addresses: peer.addresses.iter().map(ToString::to_string).collect(),
                    port: peer.port,
                    certificate_sha256: peer.certificate_sha256.clone(),
                    capability_digest: peer.capability_digest.clone(),
                    paired: self.is_paired(&peer.service_instance_id),
                    connected: connected.contains(&peer.service_instance_id),
                    capabilities: read(&self.remote_capabilities)
                        .get(&peer.service_instance_id)
                        .cloned(),
                })
                .collect(),
            connected_peers: connected.into_iter().map(|id| id.to_string()).collect(),
            display_availability: self.display_availability(&self.store.snapshot()),
            controller: grant.as_ref().map(|grant| grant.controller.to_string()),
            control_epoch: lock(&self.session)
                .as_ref()
                .map(|session| session.control_epoch.0)
                .or_else(|| grant.map(|grant| grant.epoch.0)),
            capture_active: self.capture_active.load(Ordering::Acquire),
            diagnostics: lock(&self.diagnostics).iter().cloned().collect(),
        }
    }

    pub fn preview_configuration(
        &self,
        mut configuration: WorkspaceConfiguration,
    ) -> Result<WorkspaceConfiguration, RuntimeError> {
        configuration.topology_author = self.identity.service_instance_id.to_string();
        if let Some(layout) = configuration.layout.as_mut() {
            for (id, display) in &layout.displays {
                configuration
                    .remembered_displays
                    .insert(id.clone(), display.clone());
            }
            layout
                .displays
                .retain(|id, _| !configuration.excluded_displays.contains(id));
        }
        if let Some(layout) = configuration.layout.as_mut() {
            align_system_display_groups(&mut layout.displays)?;
            for (id, display) in &layout.displays {
                configuration
                    .remembered_displays
                    .insert(id.clone(), display.clone());
            }
            let existing_portals = std::mem::take(&mut layout.portals);
            layout.portals = derive_auto_portals(
                &layout.displays,
                &existing_portals,
                DEFAULT_PORTAL_ADJACENCY_TOLERANCE_UM,
                DEFAULT_MINIMUM_PORTAL_SPAN_UM,
            )?;
            let current_revision = self
                .store
                .snapshot()
                .layout
                .map_or(0, |layout| layout.revision.0);
            if layout.revision.0 <= current_revision {
                layout.revision = TopologyRevision(current_revision.saturating_add(1).max(1));
            }
            RuntimeRoutingSnapshot::compile(layout.clone())?;
        }
        self.refresh_portal_availability(&mut configuration);
        Ok(configuration)
    }

    pub fn update_configuration(
        &self,
        configuration: WorkspaceConfiguration,
    ) -> Result<(), RuntimeError> {
        let previous = self.store.snapshot();
        arcrelay_input::validate_consumer_shortcuts(&configuration.consumer_shortcuts)
            .map_err(|error| RuntimeError::InvalidInput(error.into()))?;
        let mut configuration = self.preview_configuration(configuration)?;
        let shared_layout_changed = !same_shared_layout(&previous, &configuration);
        if !shared_layout_changed {
            if let (Some(old), Some(next)) = (&previous.layout, &mut configuration.layout) {
                next.revision = old.revision;
            }
            configuration.topology_author = previous.topology_author;
        } else if lock(&self.session)
            .as_ref()
            .is_some_and(|session| session.controller != self.identity.service_instance_id)
        {
            self.release_control()?;
        }
        let topology_to_broadcast = configuration.layout.clone();
        self.store.save(configuration)?;
        self.platform
            .set_consumer_shortcuts(self.store.snapshot().consumer_shortcuts);
        self.reconcile_control_component()?;
        if let Some(layout) = topology_to_broadcast.as_ref() {
            let snapshot = RuntimeRoutingSnapshot::compile(layout.clone())?;
            let mut router = write(&self.router);
            match router.as_ref() {
                Some(router) => router.replace(snapshot),
                None => *router = Some(Arc::new(RuntimeRouter::new(snapshot))),
            }
            if let Some(session) = lock(&self.session).as_mut() {
                if session.controller == self.identity.service_instance_id {
                    session.topology_revision = layout.revision;
                }
            }
        }
        *lock(&self.keyboard_mapper) = None;
        if shared_layout_changed {
            if let Some(layout) = topology_to_broadcast.as_ref() {
                for peer in self.network.connected_peers() {
                    let network = self.network.clone();
                    let frame = self.workspace_topology_update_frame(&peer, layout);
                    self.runtime_handle.spawn(async move {
                        if let Err(error) = network.send_control(&peer, &frame).await {
                            tracing::debug!(%error, "Arc Input topology broadcast failed");
                        }
                    });
                }
            }
        }
        if let Some(layout) = topology_to_broadcast.as_ref() {
            let active_portals = layout
                .portals
                .iter()
                .filter(|portal| portal.status == PortalStatus::Active)
                .count();
            tracing::info!(
                event = "input.topology.compiled",
                topology_revision = layout.revision.0,
                display_count = layout.displays.len(),
                portal_count = layout.portals.len(),
                active_portal_count = active_portals,
                "Arc Input topology compiled"
            );
        }
        let _ = self.events.send(RuntimeEvent::SnapshotChanged);
        Ok(())
    }
}
