use super::*;

impl ClipboardSyncManager {
    pub fn new(
        clipboard: Arc<ClipboardApplicationService>,
        network: Arc<tokio::sync::OnceCell<Arc<NetworkRuntime>>>,
        remote_file_provider: Arc<dyn RemoteFileProvider>,
        _config_directory: &Path,
        event_tx: mpsc::Sender<ServerEvent>,
    ) -> Arc<Self> {
        Arc::new(Self {
            clipboard,
            network,
            commands: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            remote_file_peers: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            active: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            connecting: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            discovered: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            incoming_connections: Arc::new(tokio::sync::OnceCell::new()),
            remote_file_provider,
            event_tx,
        })
    }

    pub async fn attach_connection_registry(&self, registry: ConnectionRegistry) {
        let _ = self.incoming_connections.set(registry);
    }

    pub fn start(self: &Arc<Self>) {
        let manager = self.clone();
        tokio::spawn(async move { manager.discovery_loop().await });
    }

    async fn network(&self) -> Result<Arc<arcrelay_network::NetworkRuntime>, String> {
        wait_for_available(NETWORK_START_TIMEOUT, NETWORK_START_POLL_INTERVAL, || {
            self.network.get().cloned()
        })
        .await
        .ok_or_else(|| "unified network runtime startup timed out; try again later".to_string())
    }

    pub async fn merge_all(&self) -> Result<usize, String> {
        if !self.clipboard.sync_preferences().enabled {
            return Err("multi-device clipboard synchronization is disabled".into());
        }
        let peers = self
            .commands
            .lock()
            .await
            .iter()
            .map(|(id, sender)| (id.clone(), sender.clone()))
            .collect::<Vec<_>>();
        if peers.is_empty() {
            return Err("no paired desktop device is currently online".into());
        }
        let mut merged = 0;
        let mut failures = Vec::new();
        for (peer_id, sender) in peers {
            let (result_tx, result_rx) = oneshot::channel();
            if sender
                .send(ConnectionCommand::Merge(result_tx))
                .await
                .is_err()
            {
                failures.push(format!("{peer_id}: connection closed"));
                continue;
            }
            match tokio::time::timeout(Duration::from_secs(120), result_rx).await {
                Ok(Ok(Ok(count))) => merged += count,
                Ok(Ok(Err(error))) => failures.push(format!("{peer_id}: {error}")),
                Ok(Err(_)) => failures.push(format!("{peer_id}: merge task was cancelled")),
                Err(_) => failures.push(format!("{peer_id}: merge timed out")),
            }
        }
        if merged == 0 && !failures.is_empty() {
            Err(failures.join("；"))
        } else {
            Ok(merged)
        }
    }

    /// Only established feature sessions, excluding discovery probes and pairing attempts.
    pub async fn connected_peer_ids(&self) -> Vec<String> {
        self.commands.lock().await.keys().cloned().collect()
    }

    pub async fn remote_file_devices(&self) -> Vec<RemoteFileDeviceView> {
        let mut connected = self.remote_file_peers.lock().await.clone();
        if let Some(registry) = self.incoming_connections.get() {
            connected.extend(
                registry
                    .list_connected_with_feature(
                        proto::Feature::RemoteFiles as i32,
                        arcrelay_protocol::remote_files::REMOTE_FILE_PROTOCOL_VERSION,
                    )
                    .await,
            );
        }
        let discovered = self.discovered.lock().await;
        let mut devices = connected
            .into_iter()
            .map(|id| RemoteFileDeviceView {
                name: discovered
                    .get(&id)
                    .map(|peer| peer.metadata.name.clone())
                    .unwrap_or_else(|| id.clone()),
                id,
            })
            .collect::<Vec<_>>();
        devices.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.id.cmp(&right.id))
        });
        devices
    }

    pub async fn upload_system_file(
        &self,
        peer_id: &str,
        request: RemoteFileRequest,
        source: PathBuf,
    ) -> RemoteFileResult<RemoteFileResponse> {
        match self
            .remote_file_route(peer_id)
            .await
            .map_err(|error| RemoteFileError::new(RemoteFileErrorCode::Unavailable, error))?
        {
            RemoteFileRoute::Managed(sender) => {
                let (response, receiver) = oneshot::channel();
                sender
                    .send(ConnectionCommand::SystemUpload {
                        request,
                        source,
                        response,
                    })
                    .await
                    .map_err(|_| {
                        RemoteFileError::new(
                            RemoteFileErrorCode::Unavailable,
                            "remote device disconnected",
                        )
                    })?;
                receiver.await.map_err(|_| {
                    RemoteFileError::new(
                        RemoteFileErrorCode::Unavailable,
                        "remote save interrupted",
                    )
                })?
            }
            RemoteFileRoute::Incoming(connection) => {
                remote_files::upload_system_file(&connection, request, &source).await
            }
        }
    }

    pub async fn remote_file_request(
        &self,
        peer_id: &str,
        request: RemoteFileRequest,
    ) -> RemoteFileResult<RemoteFileResponse> {
        match self
            .remote_file_route(peer_id)
            .await
            .map_err(|error| RemoteFileError::new(RemoteFileErrorCode::Unavailable, error))?
        {
            RemoteFileRoute::Managed(sender) => {
                let (response_tx, response_rx) = oneshot::channel();
                sender
                    .send(ConnectionCommand::RemoteRequest(request, response_tx))
                    .await
                    .map_err(|_| {
                        RemoteFileError::new(
                            RemoteFileErrorCode::Unavailable,
                            "remote device connection closed",
                        )
                    })?;
                response_rx.await.map_err(|_| {
                    RemoteFileError::new(
                        RemoteFileErrorCode::Cancelled,
                        "remote file request was cancelled",
                    )
                })?
            }
            RemoteFileRoute::Incoming(connection) => {
                remote_file_request_on_connection(&connection, request).await
            }
        }
    }

    pub async fn list_remote_directory_all(
        &self,
        peer_id: &str,
        share_id: &str,
        relative_path: &str,
    ) -> Result<Vec<RemoteFileEntry>, String> {
        let mut entries = Vec::new();
        let mut cursor = None;
        loop {
            let response = self
                .remote_file_request(
                    peer_id,
                    RemoteFileRequest::ListDirectory {
                        share_id: share_id.to_string(),
                        relative_path: relative_path.to_string(),
                        cursor,
                        limit: DEFAULT_REMOTE_DIRECTORY_PAGE_SIZE,
                        search: None,
                        sort_key: RemoteFileSortKey::Name,
                        sort_direction: RemoteFileSortDirection::Ascending,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            entries.extend(response.entries);
            let Some(next_cursor) = response.next_cursor else {
                return Ok(entries);
            };
            cursor = Some(next_cursor);
        }
    }

    pub async fn remote_file_thumbnail(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        max_dimension: u32,
    ) -> Result<(Vec<u8>, String), String> {
        match self.remote_file_route(peer_id).await? {
            RemoteFileRoute::Managed(sender) => {
                let (response_tx, response_rx) = oneshot::channel();
                sender
                    .send(ConnectionCommand::Thumbnail {
                        share_id,
                        relative_path,
                        max_dimension,
                        response: response_tx,
                    })
                    .await
                    .map_err(|_| "remote device connection closed".to_string())?;
                response_rx
                    .await
                    .map_err(|_| "remote thumbnail request was cancelled".to_string())?
            }
            RemoteFileRoute::Incoming(connection) => {
                download_remote_thumbnail(&connection, &share_id, &relative_path, max_dimension)
                    .await
            }
        }
    }

    pub async fn download_remote_file(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        destination: PathBuf,
    ) -> Result<PathBuf, String> {
        self.download_remote_file_with_progress(peer_id, share_id, relative_path, destination, None)
            .await
    }

    pub async fn download_remote_file_with_progress(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        destination: PathBuf,
        progress: Option<RemoteFileProgressCallback>,
    ) -> Result<PathBuf, String> {
        match self.remote_file_route(peer_id).await? {
            RemoteFileRoute::Managed(sender) => {
                let (response_tx, response_rx) = oneshot::channel();
                sender
                    .send(ConnectionCommand::Download {
                        share_id,
                        relative_path,
                        destination,
                        progress,
                        response: response_tx,
                    })
                    .await
                    .map_err(|_| "remote device connection closed".to_string())?;
                response_rx
                    .await
                    .map_err(|_| "remote download was cancelled".to_string())?
            }
            RemoteFileRoute::Incoming(connection) => {
                download_remote_entry(
                    &connection,
                    &share_id,
                    &relative_path,
                    &destination,
                    progress,
                )
                .await
            }
        }
    }

    #[cfg(target_os = "windows")]
    pub async fn stream_remote_file(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
    ) -> Result<RemoteFileStreamReceiver, String> {
        let (chunks, receiver) = mpsc::channel(8);
        match self.remote_file_route(peer_id).await? {
            RemoteFileRoute::Managed(sender) => sender
                .send(ConnectionCommand::DownloadStream {
                    share_id,
                    relative_path,
                    chunks,
                })
                .await
                .map_err(|_| "remote device connection closed".to_string())?,
            RemoteFileRoute::Incoming(connection) => {
                tokio::spawn(async move {
                    stream_remote_file(&connection, &share_id, &relative_path, chunks).await;
                });
            }
        }
        Ok(receiver)
    }

    pub async fn upload_remote_files(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        paths: Vec<PathBuf>,
    ) -> Result<usize, String> {
        self.upload_remote_files_with_progress(peer_id, share_id, relative_path, paths, None)
            .await
    }

    pub async fn upload_remote_files_with_progress(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        paths: Vec<PathBuf>,
        progress: Option<RemoteFileProgressCallback>,
    ) -> Result<usize, String> {
        match self.remote_file_route(peer_id).await? {
            RemoteFileRoute::Managed(sender) => {
                let (response_tx, response_rx) = oneshot::channel();
                sender
                    .send(ConnectionCommand::Upload {
                        share_id,
                        relative_path,
                        paths,
                        progress,
                        response: response_tx,
                    })
                    .await
                    .map_err(|_| "remote device connection closed".to_string())?;
                response_rx
                    .await
                    .map_err(|_| "remote upload was cancelled".to_string())?
            }
            RemoteFileRoute::Incoming(connection) => {
                upload_remote_paths(&connection, &share_id, &relative_path, paths, progress).await
            }
        }
    }

    pub async fn upload_remote_edit(
        &self,
        peer_id: &str,
        share_id: String,
        relative_path: String,
        source: PathBuf,
        expected_modified_at_ms: i64,
    ) -> RemoteFileResult<()> {
        match self
            .remote_file_route(peer_id)
            .await
            .map_err(|error| RemoteFileError::new(RemoteFileErrorCode::Unavailable, error))?
        {
            RemoteFileRoute::Managed(sender) => {
                let (response_tx, response_rx) = oneshot::channel();
                sender
                    .send(ConnectionCommand::UploadEdit {
                        share_id,
                        relative_path,
                        source,
                        expected_modified_at_ms,
                        response: response_tx,
                    })
                    .await
                    .map_err(|_| {
                        RemoteFileError::new(
                            RemoteFileErrorCode::Unavailable,
                            "remote device connection closed",
                        )
                    })?;
                response_rx.await.map_err(|_| {
                    RemoteFileError::new(
                        RemoteFileErrorCode::Cancelled,
                        "remote edit upload was cancelled",
                    )
                })?
            }
            RemoteFileRoute::Incoming(connection) => {
                upload_remote_file(
                    &connection,
                    &share_id,
                    &relative_path,
                    &source,
                    Some(expected_modified_at_ms),
                )
                .await
            }
        }
    }

    async fn remote_file_route(&self, peer_id: &str) -> Result<RemoteFileRoute, String> {
        if self.remote_file_peers.lock().await.contains(peer_id) {
            if let Some(sender) = self.commands.lock().await.get(peer_id).cloned() {
                return Ok(RemoteFileRoute::Managed(sender));
            }
        }
        if let Some(registry) = self.incoming_connections.get() {
            if let Some(connection) = registry
                .transport_for_feature(
                    peer_id,
                    proto::Feature::RemoteFiles as i32,
                    arcrelay_protocol::remote_files::REMOTE_FILE_PROTOCOL_VERSION,
                )
                .await
            {
                return Ok(RemoteFileRoute::Incoming(connection));
            }
        }
        Err("remote device has no usable connection".to_string())
    }

    pub async fn discover_desktops(&self) -> Result<Vec<NearbyDesktopView>, String> {
        let network = self.network().await?;
        network
            .scan_local_ipv4()
            .await
            .map_err(|error| error.to_string())?;
        let (peers, paired) = self.connection_candidates(&network).await?;
        self.remember_discovered(network.device_id().as_str(), &peers)
            .await;
        let connecting = self.connecting.lock().await.clone();
        let mut views = peers
            .into_iter()
            .map(|peer| NearbyDesktopView {
                paired: paired.contains(peer.device_id.as_str()),
                connecting: connecting.contains(peer.device_id.as_str()),
                device_id: peer.device_id.to_string(),
                device_name: peer.metadata.name,
                host: peer
                    .addresses
                    .first()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        views.sort_by(|left, right| {
            left.device_name
                .cmp(&right.device_name)
                .then_with(|| left.device_id.cmp(&right.device_id))
        });
        Ok(views)
    }

    pub async fn connect_desktop(self: &Arc<Self>, peer_id: &str) -> Result<String, String> {
        let network = self.network().await?;
        if peer_id == network.device_id().as_str() {
            return Err("cannot connect to the local device".into());
        }
        let peer = if let Some(peer) = self.discovered.lock().await.get(peer_id).cloned() {
            peer
        } else {
            self.discover_desktops().await?;
            self.discovered
                .lock()
                .await
                .get(peer_id)
                .cloned()
                .ok_or_else(|| {
                    "desktop device was not found on the LAN; refresh and try again".to_string()
                })?
        };
        let device_name = peer.metadata.name.clone();
        if !self
            .start_connection(peer, ConnectionIntent::UserInitiated)
            .await
        {
            return Err("desktop device is connecting or already connected".into());
        }
        Ok(device_name)
    }

    pub async fn connect_desktop_address(
        self: &Arc<Self>,
        host: &str,
        port: Option<u16>,
    ) -> Result<String, String> {
        let network = self.network().await?;
        let peer = network
            .discover_at(host, port)
            .await
            .map_err(|error| error.to_string())?;
        if peer.device_id == network.device_id() {
            return Err("cannot connect to the local device".into());
        }
        let device_name = peer.metadata.name.clone();
        self.discovered
            .lock()
            .await
            .insert(peer.device_id.to_string(), peer.clone());
        if !self
            .start_connection(peer, ConnectionIntent::UserInitiated)
            .await
        {
            return Err("desktop device is connecting or already connected".into());
        }
        Ok(device_name)
    }

    async fn discovery_loop(self: Arc<Self>) {
        let network = loop {
            if let Ok(network) = self.network().await {
                break network;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let mut receiver = network.discovery().subscribe();
        let fallback_at = tokio::time::Instant::now() + AUTOMATIC_DIAL_FALLBACK_DELAY;
        loop {
            let _ = receiver.borrow_and_update();
            match self.connection_candidates(&network).await {
                Ok((peers, _)) => {
                    self.remember_discovered(network.device_id().as_str(), &peers)
                        .await;
                    let intent = if tokio::time::Instant::now() >= fallback_at {
                        ConnectionIntent::AutomaticFallback
                    } else {
                        ConnectionIntent::Automatic
                    };
                    for peer in peers {
                        self.start_connection(peer, intent).await;
                    }
                }
                Err(error) => tracing::warn!(
                    event = "desktop.connection.candidate_refresh_failed",
                    %error,
                    "failed to refresh automatic desktop connection candidates"
                ),
            }
            tokio::select! {
                changed = receiver.changed() => {
                    if changed.is_err() { break; }
                }
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        }
    }

    async fn connection_candidates(
        &self,
        network: &NetworkRuntime,
    ) -> Result<(Vec<PeerAdvertisement>, HashSet<String>), String> {
        let mut peers = network
            .discovery()
            .snapshot()
            .iter()
            .cloned()
            .map(|peer| (peer.device_id.to_string(), peer))
            .collect::<HashMap<_, _>>();
        let paired_records = network
            .paired_peers()
            .await
            .map_err(|error| error.to_string())?;
        let paired = paired_records
            .iter()
            .map(|peer| peer.device_id.to_string())
            .collect::<HashSet<_>>();
        for paired_peer in paired_records {
            let peer_id = paired_peer.device_id.to_string();
            if peers.contains_key(&peer_id) {
                continue;
            }
            match network.remembered_peer(&paired_peer.device_id).await {
                Ok(Some(remembered)) => {
                    peers.insert(peer_id, remembered);
                }
                Ok(None) => {}
                Err(error) => tracing::warn!(
                    event = "desktop.connection.remembered_peer_load_failed",
                    peer_id = %crate::observability::pseudonym(paired_peer.device_id.as_str()),
                    %error,
                    "failed to load one remembered peer; continuing with other connection candidates"
                ),
            }
        }
        let mut peers = peers.into_values().collect::<Vec<_>>();
        peers.retain(|peer| hosts_desktop_control_service(&peer.metadata.platform));
        peers.sort_by(|left, right| left.device_id.cmp(&right.device_id));
        Ok((peers, paired))
    }

    async fn remember_discovered(&self, local_id: &str, peers: &[PeerAdvertisement]) {
        let mut discovered = self.discovered.lock().await;
        for peer in peers {
            if peer.device_id.as_str() != local_id {
                discovered.insert(peer.device_id.to_string(), peer.clone());
            }
        }
    }

    async fn start_connection(
        self: &Arc<Self>,
        peer: PeerAdvertisement,
        intent: ConnectionIntent,
    ) -> bool {
        let peer_id = peer.device_id.to_string();
        if !hosts_desktop_control_service(&peer.metadata.platform) {
            return false;
        }
        let network = match self.network().await {
            Ok(network) => network,
            Err(_) => return false,
        };
        let paired_record = network.paired_peers().await.ok().and_then(|peers| {
            peers
                .into_iter()
                .find(|value| value.device_id == peer.device_id)
        });
        let paired = paired_record.is_some();
        let auto_connect = paired_record.is_some_and(|record| record.auto_connect);
        if !should_start_connection(
            network.device_id().as_str(),
            &peer_id,
            paired,
            auto_connect,
            intent,
        ) {
            return false;
        }
        if let Some(registry) = self.incoming_connections.get() {
            if registry.transport(&peer_id).await.is_some() {
                return false;
            }
        }
        if !self.active.lock().await.insert(peer_id.clone()) {
            return false;
        }
        self.connecting.lock().await.insert(peer_id.clone());
        let manager = self.clone();
        tokio::spawn(async move {
            let peer_id = peer.device_id.to_string();
            if let Err(error) = manager.clone().run_connection(peer, !paired).await {
                tracing::warn!(%peer_id, %error, "desktop connection ended");
            }
            manager.connecting.lock().await.remove(&peer_id);
            let _ = manager
                .event_tx
                .send(ServerEvent::OutgoingPairingFinished {
                    device_id: peer_id.clone(),
                })
                .await;
            manager.commands.lock().await.remove(&peer_id);
            manager.remote_file_peers.lock().await.remove(&peer_id);
            manager.active.lock().await.remove(&peer_id);
            tokio::time::sleep(Duration::from_secs(2)).await;
        });
        true
    }

    async fn run_connection(
        self: Arc<Self>,
        peer: PeerAdvertisement,
        pair_first: bool,
    ) -> Result<(), String> {
        let network = self.network().await?;
        tracing::info!(
            event = "desktop.connection.connect_started",
            peer_id = %crate::observability::pseudonym(peer.device_id.as_str()),
            pair_first,
            "desktop control connection started"
        );
        if pair_first {
            let pending = network
                .begin_pairing(&peer, desktop_pairing_grants())
                .await
                .map_err(|error| error.to_string())?;
            let _ = self
                .event_tx
                .send(ServerEvent::OutgoingPairingCode {
                    device_id: peer.device_id.to_string(),
                    device_name: peer.metadata.name.clone(),
                    pairing_code: pending.verification_code().to_owned(),
                })
                .await;
            network
                .complete_pairing(pending, Duration::from_secs(180))
                .await
                .map_err(|error| error.to_string())?;
            // The pairing banner describes only the verification phase. The
            // control session below is long-lived, so waiting for
            // `run_connection` to return would leave the UI stuck on
            // "waiting for confirmation" for the entire connected session.
            let _ = self
                .event_tx
                .send(ServerEvent::OutgoingPairingFinished {
                    device_id: peer.device_id.to_string(),
                })
                .await;
        }
        let session = if pair_first {
            network.connect(&peer, SessionKind::Control).await
        } else {
            network
                .connect_discovered(&peer.device_id, SessionKind::Control)
                .await
        }
        .map_err(|error| error.to_string())?;
        tracing::info!(
            event = "desktop.connection.session_authenticated",
            peer_id = %crate::observability::pseudonym(peer.device_id.as_str()),
            session_id = session.id(),
            "desktop control session authenticated"
        );
        if session.initiator_id() != &network.device_id() {
            // The deterministic session registry may return a simultaneous
            // inbound winner. Its primary control stream is owned by
            // DesktopControlService; opening a second one would be parsed as
            // an auxiliary stream by the peer and tear down the connection.
            self.connecting.lock().await.remove(peer.device_id.as_str());
            return Ok(());
        }
        let stream = tokio::time::timeout(
            CONTROL_STREAM_OPEN_TIMEOUT,
            session.open_feature_stream("arcrelay.control", &[]),
        )
        .await
        .map_err(|_| "timed out opening desktop control stream".to_string())?
        .map_err(|error| error.to_string())?;
        tracing::info!(
            event = "desktop.connection.control_stream_opened",
            peer_id = %crate::observability::pseudonym(peer.device_id.as_str()),
            "desktop control stream opened"
        );
        let mut send = stream.send;
        let mut recv = stream.receive;
        let connection = session.transport_handle();
        send_client_frame(&mut send, control_hello()).await?;
        let welcome = recv_server_frame(&mut recv, CONTROL_STREAM_OPEN_TIMEOUT).await?;
        let welcome = match welcome.body {
            Some(proto::server_control_frame::Body::Welcome(welcome)) => welcome,
            _ => return Err("remote peer returned no control protocol negotiation result".into()),
        };
        if !welcome.features.iter().any(|feature| {
            feature.feature == proto::Feature::ClipboardSync as i32
                && feature.min_version == 1
                && feature.max_version == 1
        }) {
            return Err("remote peer does not support clipboard synchronization v1".into());
        }
        let remote_files_supported = welcome.features.iter().any(|feature| {
            feature.feature == proto::Feature::RemoteFiles as i32
                && feature.min_version
                    == arcrelay_protocol::remote_files::REMOTE_FILE_PROTOCOL_VERSION
                && feature.max_version
                    == arcrelay_protocol::remote_files::REMOTE_FILE_PROTOCOL_VERSION
        });
        let grants = network
            .grants(&peer.device_id)
            .await
            .map_err(|error| error.to_string())?;
        let _reverse_file_server = if remote_files_supported {
            let reverse_file_access =
                RemoteFileAccess::from_grants(&grants, GrantDirection::Inbound);
            Some(AbortTaskOnDrop(tokio::spawn(
                serve_reverse_remote_file_streams(
                    connection.clone(),
                    self.remote_file_provider.clone(),
                    reverse_file_access,
                ),
            )))
        } else {
            None
        };
        let sync_enabled = self.clipboard.sync_preferences().enabled;
        let mut next_request_id = 1_u64;
        if sync_enabled {
            let subscription_request_id = take_request_id(&mut next_request_id);
            send_client_frame(
                &mut send,
                proto::ClientControlFrame {
                    body: Some(proto::client_control_frame::Body::Subscribe(
                        proto::SubscribeRequest {
                            request_id: subscription_request_id,
                            topic: proto::SubscriptionTopic::ClipboardSync as i32,
                        },
                    )),
                },
            )
            .await?;
            await_subscription(&mut recv, subscription_request_id).await?;
            tracing::info!(
                event = "desktop.connection.subscription_ready",
                peer_id = %crate::observability::pseudonym(peer.device_id.as_str()),
                "desktop clipboard subscription is ready"
            );
        }

        let (incoming_tx, mut incoming_rx) = mpsc::channel(128);
        let reader_connection = connection.clone();
        let reader = tokio::spawn(async move {
            loop {
                match recv_server_frame(&mut recv, Duration::from_secs(60)).await {
                    Ok(frame) => {
                        if incoming_tx.send(frame).await.is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        reader_connection.close(4_u32.into(), error.as_bytes());
                        break;
                    }
                }
            }
        });

        let (command_tx, mut command_rx) = mpsc::channel(8);
        self.commands
            .lock()
            .await
            .insert(peer.device_id.to_string(), command_tx);
        if remote_files_supported {
            self.remote_file_peers
                .lock()
                .await
                .insert(peer.device_id.to_string());
        }
        self.connecting.lock().await.remove(peer.device_id.as_str());
        let mut local_changes = self
            .clipboard
            .subscribe_sync_changes()
            .ok_or_else(|| "local clipboard synchronization events are unavailable".to_string())?;
        let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
        let mut pending = VecDeque::new();
        let mut responses = HashMap::new();
        if sync_enabled {
            let initial_sync = tokio::time::timeout(INITIAL_SYNC_TIMEOUT, async {
                let merge = self
                    .merge_peer(
                        &connection,
                        &mut send,
                        &mut next_request_id,
                        &mut pending,
                        &mut responses,
                        &mut incoming_rx,
                    )
                    .await?;
                let mut sync = ClipboardDownloadSession {
                    connection: &connection,
                    send: &mut send,
                    next_request_id: &mut next_request_id,
                    pending: &mut pending,
                    responses: &mut responses,
                    incoming: &mut incoming_rx,
                };
                self.push_local_snapshot(&mut sync, &merge.remote_versions)
                    .await?;
                Ok::<usize, String>(merge.merged)
            })
            .await;
            match initial_sync {
                Ok(Ok(merged)) => tracing::info!(
                    event = "desktop.connection.initial_sync_ready",
                    peer_id = %crate::observability::pseudonym(peer.device_id.as_str()),
                    merged,
                    "initial clipboard reconciliation completed"
                ),
                Ok(Err(error)) => tracing::warn!(
                    %error,
                    "initial clipboard reconciliation failed; keeping desktop connection available"
                ),
                Err(_) => tracing::warn!(
                    event = "desktop.connection.initial_sync_timeout",
                    peer_id = %crate::observability::pseudonym(peer.device_id.as_str()),
                    timeout_ms = INITIAL_SYNC_TIMEOUT.as_millis() as u64,
                    "initial clipboard reconciliation timed out; continuing with live sync"
                ),
            }
        }
        let _ = self
            .event_tx
            .send(ServerEvent::DeviceConnected {
                device_id: peer.device_id.to_string(),
                device_name: peer.metadata.name.clone(),
            })
            .await;
        let result = loop {
            if let Some(record) = pending.pop_front() {
                let mut download = ClipboardDownloadSession {
                    connection: &connection,
                    send: &mut send,
                    next_request_id: &mut next_request_id,
                    pending: &mut pending,
                    responses: &mut responses,
                    incoming: &mut incoming_rx,
                };
                self.apply_remote_record_resilient(record, &mut download, "live")
                    .await;
                continue;
            }
            tokio::select! {
                _ = connection.closed() => break Err("remote connection closed".into()),
                _ = heartbeat.tick() => {
                    let request_id = take_request_id(&mut next_request_id);
                    send_client_frame(&mut send, proto::ClientControlFrame {
                        body: Some(proto::client_control_frame::Body::Ping(proto::Ping {
                            request_id,
                            monotonic_elapsed_us: 0,
                        })),
                    }).await?;
                }
                command = command_rx.recv() => {
                    let Some(command) = command else { break Ok(()) };
                    match command {
                        ConnectionCommand::SystemUpload { request, source, response } => {
                            let connection = connection.clone();
                            tokio::spawn(async move {
                                let result = remote_files::upload_system_file(&connection, request, &source).await;
                                let _ = response.send(result);
                            });
                        }
                        ConnectionCommand::Merge(response) => {
                            let result = if self.clipboard.sync_preferences().enabled {
                                self.merge_peer(
                                    &connection,
                                    &mut send,
                                    &mut next_request_id,
                                    &mut pending,
                                    &mut responses,
                                    &mut incoming_rx,
                                ).await.map(|result| result.merged)
                            } else {
                                Err("multi-device clipboard synchronization is disabled".into())
                            };
                            let _ = response.send(result);
                        }
                        ConnectionCommand::RemoteRequest(request, response) => {
                            let connection = connection.clone();
                            tokio::spawn(async move {
                                let result = remote_file_request_on_connection(&connection, request).await;
                                let _ = response.send(result);
                            });
                        }
                        ConnectionCommand::Thumbnail {
                            share_id,
                            relative_path,
                            max_dimension,
                            response,
                        } => {
                            let connection = connection.clone();
                            tokio::spawn(async move {
                                let result = download_remote_thumbnail(
                                    &connection,
                                    &share_id,
                                    &relative_path,
                                    max_dimension,
                                ).await;
                                let _ = response.send(result);
                            });
                        }
                        ConnectionCommand::Download {
                            share_id,
                            relative_path,
                            destination,
                            progress,
                            response,
                        } => {
                            let connection = connection.clone();
                            tokio::spawn(async move {
                                let result = download_remote_entry(
                                    &connection,
                                    &share_id,
                                    &relative_path,
                                    &destination,
                                    progress,
                                ).await;
                                let _ = response.send(result);
                            });
                        }
                        #[cfg(target_os = "windows")]
                        ConnectionCommand::DownloadStream {
                            share_id,
                            relative_path,
                            chunks,
                        } => {
                            let connection = connection.clone();
                            tokio::spawn(async move {
                                stream_remote_file(
                                    &connection,
                                    &share_id,
                                    &relative_path,
                                    chunks,
                                ).await;
                            });
                        }
                        ConnectionCommand::Upload {
                            share_id,
                            relative_path,
                            paths,
                            progress,
                            response,
                        } => {
                            let connection = connection.clone();
                            tokio::spawn(async move {
                                let result = upload_remote_paths(
                                    &connection,
                                    &share_id,
                                    &relative_path,
                                    paths,
                                    progress,
                                ).await;
                                let _ = response.send(result);
                            });
                        }
                        ConnectionCommand::UploadEdit {
                            share_id,
                            relative_path,
                            source,
                            expected_modified_at_ms,
                            response,
                        } => {
                            let connection = connection.clone();
                            tokio::spawn(async move {
                                let result = upload_remote_file(
                                    &connection,
                                    &share_id,
                                    &relative_path,
                                    &source,
                                    Some(expected_modified_at_ms),
                                ).await;
                                let _ = response.send(result);
                            });
                        }
                    }
                }
                change = local_changes.recv() => {
                    match change {
                        Ok(record) => {
                            if self.clipboard.should_send_sync_record(&record) {
                                let mut sync = ClipboardDownloadSession {
                                    connection: &connection,
                                    send: &mut send,
                                    next_request_id: &mut next_request_id,
                                    pending: &mut pending,
                                    responses: &mut responses,
                                    incoming: &mut incoming_rx,
                                };
                                send_sync_record(&mut sync, record, true).await?;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break Err("local clipboard synchronization event stream closed".into()),
                    }
                }
                frame = incoming_rx.recv() => {
                    let Some(frame) = frame else { break Err("remote control stream closed".into()) };
                    queue_server_frame(frame, &mut pending, &mut responses)?;
                }
            }
        };
        reader.abort();
        session.close("clipboard sync connection ended");
        let _ = self
            .event_tx
            .send(ServerEvent::DeviceDisconnected {
                device_id: peer.device_id.to_string(),
                device_name: peer.metadata.name,
            })
            .await;
        result
    }

    async fn apply_remote_record(
        &self,
        wire: proto::ClipboardSyncRecord,
        download: &mut ClipboardDownloadSession<'_>,
    ) -> Result<(), String> {
        use proto::clipboard_sync_record::Payload;
        let requires_payload = !wire.deleted
            && !matches!(
                proto::ClipboardContentKind::try_from(wire.kind),
                Ok(proto::ClipboardContentKind::Files)
            )
            && self
                .clipboard
                .sync_record_requires_payload(
                    &wire.sync_id,
                    wire.revision,
                    &wire.updated_by_device_id,
                )
                .await
                .map_err(|error| error.to_string())?;
        let (html, rtf, image_png) = match wire.payload.as_ref() {
            Some(Payload::RichText(rich_text)) if requires_payload => {
                let html = download_text_blob(
                    download,
                    rich_text.html.as_ref().ok_or_else(|| {
                        "synchronized rich text is missing an HTML blob reference".to_string()
                    })?,
                    "text/html; charset=utf-8",
                    MAX_RICH_TEXT_BLOB_BYTES,
                )
                .await?;
                let rtf = if let Some(reference) = rich_text.rtf.as_ref() {
                    Some(
                        download_text_blob(
                            download,
                            reference,
                            "text/rtf",
                            MAX_RICH_TEXT_BLOB_BYTES,
                        )
                        .await?,
                    )
                } else {
                    None
                };
                (Some(html), rtf, None)
            }
            Some(Payload::Image(reference)) if requires_payload => {
                let bytes =
                    download_blob(download, reference, "image/png", MAX_IMAGE_BYTES).await?;
                (None, None, Some(bytes))
            }
            _ => (None, None, None),
        };
        let record = domain_record(wire, html, rtf, image_png)?;
        self.clipboard
            .apply_sync_record(record, false)
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    async fn apply_remote_record_resilient(
        &self,
        wire: proto::ClipboardSyncRecord,
        download: &mut ClipboardDownloadSession<'_>,
        phase: &'static str,
    ) -> bool {
        let record_id = crate::observability::pseudonym(&wire.sync_id);
        let revision = wire.revision;
        match self.apply_remote_record(wire, download).await {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(
                    event = "desktop.connection.clipboard_record_rejected",
                    %record_id,
                    revision,
                    phase,
                    error = %crate::observability::sanitize_text(&error, 1_024),
                    "skipped an invalid clipboard sync record without closing the device connection"
                );
                false
            }
        }
    }

    async fn merge_peer(
        &self,
        connection: &quinn::Connection,
        send: &mut quinn::SendStream,
        next_request_id: &mut u64,
        pending: &mut VecDeque<proto::ClipboardSyncRecord>,
        responses: &mut HashMap<u64, proto::Response>,
        incoming: &mut mpsc::Receiver<proto::ServerControlFrame>,
    ) -> Result<ClipboardMergeResult, String> {
        let mut cursor = 0;
        let mut merged = 0;
        let mut remote_versions = RemoteClipboardVersions::default();
        loop {
            let request_id = take_request_id(next_request_id);
            send_client_frame(
                send,
                proto::ClientControlFrame {
                    body: Some(proto::client_control_frame::Body::Request(proto::Request {
                        request_id,
                        timeout_ms: 60_000,
                        idempotency_key: vec![],
                        body: Some(proto::request::Body::Query(proto::Query {
                            body: Some(proto::query::Body::GetClipboardSync(
                                proto::GetClipboardSyncRequest {
                                    after_local_id: cursor,
                                    limit: 100,
                                },
                            )),
                        })),
                    })),
                },
            )
            .await?;
            let response = await_response(request_id, pending, responses, incoming).await?;
            ensure_ok(response.status.as_ref(), "fetch clipboard history")?;
            let page = match response.body {
                Some(proto::response::Body::Snapshot(snapshot)) => match snapshot.data {
                    Some(proto::snapshot::Data::ClipboardSync(page)) => page,
                    _ => {
                        return Err("remote peer returned an invalid clipboard history kind".into())
                    }
                },
                _ => return Err("remote peer returned no clipboard synchronization history".into()),
            };
            for record in page.records {
                remote_versions.records.insert(
                    record.sync_id.clone(),
                    RemoteClipboardVersion::from_wire(&record),
                );
                let mut download = ClipboardDownloadSession {
                    connection,
                    send,
                    next_request_id: &mut *next_request_id,
                    pending,
                    responses,
                    incoming,
                };
                if self
                    .apply_remote_record_resilient(record, &mut download, "reconciliation")
                    .await
                {
                    merged += 1;
                }
            }
            let Some(next) = page.next_cursor else { break };
            if next <= cursor {
                return Err("invalid remote clipboard pagination cursor".into());
            }
            cursor = next;
        }
        Ok(ClipboardMergeResult {
            merged,
            remote_versions,
        })
    }

    async fn push_local_snapshot(
        &self,
        sync: &mut ClipboardDownloadSession<'_>,
        remote_versions: &RemoteClipboardVersions,
    ) -> Result<(), String> {
        let mut cursor = 0;
        loop {
            let page = self
                .clipboard
                .sync_records(cursor, 100)
                .await
                .map_err(|error| error.to_string())?;
            for (_, mut record) in page.records {
                if !self.clipboard.should_send_sync_record(&record) {
                    continue;
                }
                let remote_version = remote_versions.records.get(&record.sync_id);
                if remote_version.is_some_and(|version| version.covers(&record)) {
                    continue;
                }
                let include_payload =
                    remote_version.is_none_or(|version| !version.has_current_content(&record));
                record.live = false;
                record.change_kind = ClipboardSyncChangeKind::Snapshot;
                send_sync_record(sync, record, include_payload).await?;
            }
            let Some(next) = page.next_cursor else { break };
            if next <= cursor {
                return Err("invalid local clipboard pagination cursor".into());
            }
            cursor = next;
        }
        Ok(())
    }
}

pub(super) async fn serve_reverse_remote_file_streams(
    connection: quinn::Connection,
    provider: Arc<dyn RemoteFileProvider>,
    access: RemoteFileAccess,
) {
    loop {
        let (mut send, mut recv) = match connection.accept_bi().await {
            Ok(stream) => stream,
            Err(_) => break,
        };
        let provider = provider.clone();
        let access = access.clone();
        tokio::spawn(async move {
            let kind = tokio::time::timeout(Duration::from_secs(5), recv.read_u8()).await;
            match kind {
                Ok(Ok(STREAM_KIND_REMOTE_FILES)) => {
                    if let Err(error) =
                        serve_remote_file_stream(&mut send, &mut recv, Some(provider), access).await
                    {
                        tracing::warn!(%error, "reverse remote file stream ended");
                    }
                    let _ = send.finish();
                }
                Ok(Ok(kind)) => {
                    tracing::warn!(kind, "unsupported reverse bidirectional stream");
                }
                Ok(Err(error)) => {
                    tracing::warn!(%error, "reverse stream preface failed");
                }
                Err(_) => {
                    tracing::warn!("reverse stream preface timed out");
                }
            }
        });
    }
}
