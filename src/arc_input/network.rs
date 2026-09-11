use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use arcrelay_input::{proto, wire};
use arcrelay_network::{NetworkRuntime, Session, SessionKind};
use arcrelay_peer::{CapabilityId, DeviceId, ServiceInstanceId};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{broadcast, Notify};

const INPUT_SEND_QUEUE_CAPACITY: usize = 128;
mod relay;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEnqueueOutcome {
    Queued,
    ReplacedPendingMotion,
}

#[derive(Debug)]
struct InputSendQueueState {
    frames: VecDeque<proto::InputEventBatch>,
    closed: bool,
}

#[derive(Debug)]
struct InputSendQueueInner {
    state: Mutex<InputSendQueueState>,
    notify: Notify,
    sender_count: AtomicUsize,
}

#[derive(Debug)]
struct InputSendQueueSender {
    inner: Arc<InputSendQueueInner>,
}

#[derive(Debug)]
struct InputSendQueueReceiver {
    inner: Arc<InputSendQueueInner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputSendQueueError {
    Full,
    Closed,
}

impl Clone for InputSendQueueSender {
    fn clone(&self) -> Self {
        self.inner.sender_count.fetch_add(1, Ordering::Relaxed);
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Drop for InputSendQueueSender {
    fn drop(&mut self) {
        if self.inner.sender_count.fetch_sub(1, Ordering::AcqRel) == 1 {
            lock(&self.inner.state).closed = true;
            self.inner.notify.notify_waiters();
        }
    }
}

impl InputSendQueueSender {
    fn channel() -> (Self, InputSendQueueReceiver) {
        let inner = Arc::new(InputSendQueueInner {
            state: Mutex::new(InputSendQueueState {
                frames: VecDeque::with_capacity(INPUT_SEND_QUEUE_CAPACITY),
                closed: false,
            }),
            notify: Notify::new(),
            sender_count: AtomicUsize::new(1),
        });
        (
            Self {
                inner: inner.clone(),
            },
            InputSendQueueReceiver { inner },
        )
    }

    fn try_send(
        &self,
        mut batch: proto::InputEventBatch,
        replace_pending_motion: bool,
    ) -> Result<InputEnqueueOutcome, InputSendQueueError> {
        let mut state = lock(&self.inner.state);
        if state.closed {
            return Err(InputSendQueueError::Closed);
        }
        if replace_pending_motion && is_pointer_motion_batch(&batch) {
            if let Some(pending) = state.frames.back_mut() {
                if replaceable_pointer_motion(pending, &batch) {
                    if let (Some(pending_header), Some(next_header)) =
                        (pending.header.as_ref(), batch.header.as_mut())
                    {
                        // The replaced frame has not left the application yet.
                        // Reuse its reliable sequence so the receiver still
                        // observes a contiguous stream across ordering
                        // boundaries such as buttons and keys.
                        next_header.sequence = pending_header.sequence;
                    }
                    *pending = batch;
                    return Ok(InputEnqueueOutcome::ReplacedPendingMotion);
                }
            }
        }
        if state.frames.len() >= INPUT_SEND_QUEUE_CAPACITY {
            return Err(InputSendQueueError::Full);
        }
        state.frames.push_back(batch);
        drop(state);
        self.inner.notify.notify_one();
        Ok(InputEnqueueOutcome::Queued)
    }

    fn pending(&self) -> u64 {
        lock(&self.inner.state).frames.len() as u64
    }
}

impl InputSendQueueReceiver {
    async fn recv(&mut self) -> Option<proto::InputEventBatch> {
        loop {
            let notified = self.inner.notify.notified();
            {
                let mut state = lock(&self.inner.state);
                if let Some(frame) = state.frames.pop_front() {
                    return Some(frame);
                }
                if state.closed {
                    return None;
                }
            }
            notified.await;
        }
    }
}

impl Drop for InputSendQueueReceiver {
    fn drop(&mut self) {
        let mut state = lock(&self.inner.state);
        state.closed = true;
        state.frames.clear();
        drop(state);
        self.inner.notify.notify_waiters();
    }
}

fn is_pointer_motion_batch(batch: &proto::InputEventBatch) -> bool {
    matches!(
        batch.events.as_slice(),
        [proto::InputEvent {
            event: Some(proto::input_event::Event::PointerMotion(_)),
        }]
    )
}

fn replaceable_pointer_motion(
    pending: &proto::InputEventBatch,
    next: &proto::InputEventBatch,
) -> bool {
    if !is_pointer_motion_batch(pending) || pending.held_state_checksum != next.held_state_checksum
    {
        return false;
    }
    match (pending.header.as_ref(), next.header.as_ref()) {
        (Some(pending), Some(next)) => {
            pending.workspace_id == next.workspace_id
                && pending.topology_revision == next.topology_revision
                && pending.control_epoch == next.control_epoch
                && pending.source_device_id == next.source_device_id
                && pending.target_device_id == next.target_device_id
        }
        _ => false,
    }
}

#[derive(Debug, Default)]
struct TransportCounters {
    enqueued: AtomicU64,
    written: AtomicU64,
    received: AtomicU64,
    queue_full: AtomicU64,
    max_pending: AtomicU64,
}

#[derive(Debug, Default)]
pub struct InputTransportMetrics {
    pub enqueued: u64,
    pub written: u64,
    pub received: u64,
    pub queue_full: u64,
    pub max_pending: u64,
    pub pending: u64,
}

#[derive(Debug, Clone)]
pub enum NetworkEvent {
    Connected {
        service_instance_id: ServiceInstanceId,
        session_id: u64,
    },
    Disconnected {
        service_instance_id: ServiceInstanceId,
        session_id: u64,
        reason: String,
    },
    Control {
        service_instance_id: ServiceInstanceId,
        session_id: u64,
        frame: proto::ControlFrame,
    },
    Input {
        service_instance_id: ServiceInstanceId,
        session_id: u64,
        batch: proto::InputEventBatch,
    },
}

#[derive(Clone)]
struct ActiveLink {
    session_id: u64,
    session: Arc<Session>,
    connection: quinn::Connection,
    input_sender: InputSendQueueSender,
}

/// Arc Input adapter over the process-wide authenticated network runtime.
/// It owns no endpoint, identity, discovery browser, or trust database.
pub struct PeerConnectionHub {
    network: Arc<tokio::sync::OnceCell<Arc<NetworkRuntime>>>,
    links: Mutex<HashMap<ServiceInstanceId, ActiveLink>>,
    events: broadcast::Sender<NetworkEvent>,
    counters: Arc<TransportCounters>,
    relay: relay::RelayState,
}

impl PeerConnectionHub {
    pub fn bind(
        network: Arc<tokio::sync::OnceCell<Arc<NetworkRuntime>>>,
    ) -> Result<Arc<Self>, NetworkError> {
        tokio::runtime::Handle::try_current().map_err(|_| NetworkError::RuntimeUnavailable)?;
        let (events, _) = broadcast::channel(128);
        Ok(Arc::new(Self {
            network,
            links: Mutex::new(HashMap::new()),
            events,
            counters: Arc::new(TransportCounters::default()),
            relay: relay::RelayState::default(),
        }))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<NetworkEvent> {
        self.events.subscribe()
    }

    pub fn connected_peers(&self) -> Vec<ServiceInstanceId> {
        let mut peers = lock(&self.links)
            .iter()
            .filter(|(_, link)| link.connection.close_reason().is_none())
            .map(|(peer, _)| peer.clone())
            .collect::<Vec<_>>();
        peers.sort();
        peers
    }

    pub fn is_current_session(&self, peer: &ServiceInstanceId, session_id: u64) -> bool {
        lock(&self.links).get(peer).is_some_and(|link| {
            link.session_id == session_id && link.connection.close_reason().is_none()
        })
    }

    pub fn disconnect(&self, peer: &ServiceInstanceId, reason: &str) -> bool {
        let Some(link) = lock(&self.links).remove(peer) else {
            return false;
        };
        link.session.close(reason);
        tracing::info!(
            event = "input.connection.disconnect_requested",
            peer_id = %crate::arc_input::log_peer_id(peer.as_str()),
            session_id = link.session_id,
            reason = %crate::arc_input::sanitize_log_text(reason, 512),
            "Arc Input connection disconnect requested"
        );
        let _ = self.events.send(NetworkEvent::Disconnected {
            service_instance_id: peer.clone(),
            session_id: link.session_id,
            reason: reason.to_string(),
        });
        true
    }

    /// Runs the Arc Input session service with a receiver reserved by the
    /// unified network bootstrap path.
    ///
    /// Reserving the receiver before the shared runtime is published prevents
    /// a fast peer from authenticating a realtime-input session during app
    /// startup and having that session consumed only by another feature
    /// service before Arc Input subscribes.
    pub async fn run_with_incoming(
        self: Arc<Self>,
        mut incoming: broadcast::Receiver<Arc<Session>>,
    ) -> Result<(), NetworkError> {
        let network = wait_for_network(&self.network).await;
        loop {
            let session = match incoming.recv().await {
                Ok(session) => session,
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    tracing::warn!(
                        event = "input.connection.listener_lagged",
                        count,
                        "Arc Input session listener missed incoming sessions"
                    );
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return Err(NetworkError::Closed),
            };
            if session.kind() != SessionKind::RealtimeInput {
                continue;
            }
            if let Err(error) = network
                .require(&session, CapabilityId::CrossScreenInject)
                .await
            {
                session.close("cross-screen input capability denied");
                tracing::warn!(
                    event = "input.connection.authorization_rejected",
                    peer_id = %crate::arc_input::log_peer_id(session.peer().device_id.as_str()),
                    %error,
                    "Arc Input authorization rejected"
                );
                continue;
            }
            if let Err(error) = self.attach(session).await {
                tracing::warn!(
                    event = "input.connection.session_rejected",
                    %error,
                    "Arc Input session rejected"
                );
            }
        }
    }

    pub async fn connect(
        self: &Arc<Self>,
        peer_id: ServiceInstanceId,
        _addresses: &[IpAddr],
        _port: u16,
    ) -> Result<(), NetworkError> {
        if self.connected_peers().contains(&peer_id) {
            tracing::debug!(
                event = "input.connection.connect_skipped",
                peer_id = %crate::arc_input::log_peer_id(peer_id.as_str()),
                reason = "already_connected",
                "Arc Input connection already exists"
            );
            return Ok(());
        }
        tracing::info!(
            event = "input.connection.connect_started",
            peer_id = %crate::arc_input::log_peer_id(peer_id.as_str()),
            address_count = _addresses.len(),
            advertised_port = _port,
            "Arc Input connection started"
        );
        let device_id =
            DeviceId::parse(peer_id.as_str()).map_err(|_| NetworkError::InvalidPeerIdentity)?;
        let network = wait_for_network(&self.network).await;
        let session = network
            .connect_discovered(&device_id, SessionKind::RealtimeInput)
            .await?;
        self.attach(session).await
    }

    pub async fn connect_relayed(
        self: &Arc<Self>,
        peer_id: ServiceInstanceId,
        endpoints: Vec<SocketAddr>,
    ) -> Result<(), NetworkError> {
        if self.connected_peers().contains(&peer_id) {
            return Ok(());
        }
        tracing::info!(
            event = "input.connection.relayed_connect_started",
            peer_id = %crate::arc_input::log_peer_id(peer_id.as_str()),
            address_count = endpoints.len(),
            "Arc Input relayed-route connection started"
        );
        let device_id =
            DeviceId::parse(peer_id.as_str()).map_err(|_| NetworkError::InvalidPeerIdentity)?;
        let network = wait_for_network(&self.network).await;
        // Keep local discovery ahead of relayed routes, but retain the latter
        // on every retry when multicast is missing or its route is stale.
        let mut candidates = network
            .discovery()
            .peer(&device_id)
            .map(|peer| {
                if peer.connection_addresses.is_empty() {
                    peer.addresses
                        .into_iter()
                        .map(|ip| SocketAddr::new(ip, peer.port))
                        .collect()
                } else {
                    peer.connection_addresses
                }
            })
            .unwrap_or_default();
        candidates.extend(endpoints);
        match network.remembered_peer(&device_id).await {
            Ok(Some(remembered)) => candidates.extend(remembered.connection_addresses),
            Ok(None) => {}
            Err(error) => {
                tracing::debug!(%error, "Arc Input remembered routes unavailable; trying known routes")
            }
        }
        let mut endpoints = Vec::new();
        for endpoint in candidates {
            let endpoint = match endpoint.ip().to_canonical() {
                IpAddr::V4(ip) => SocketAddr::new(IpAddr::V4(ip), endpoint.port()),
                IpAddr::V6(_) => endpoint,
            };
            if !endpoints.contains(&endpoint) {
                endpoints.push(endpoint);
            }
            if endpoints.len() == 16 {
                break;
            }
        }
        let session = network
            .connect_paired_at(&device_id, endpoints, SessionKind::RealtimeInput)
            .await?;
        self.attach(session).await
    }

    async fn attach(self: &Arc<Self>, session: Arc<Session>) -> Result<(), NetworkError> {
        let peer_id = ServiceInstanceId::parse(session.peer().device_id.to_string())
            .map_err(|_| NetworkError::InvalidPeerIdentity)?;
        let session_id = session.id();
        let connection = session.transport_handle();
        let (input_sender, input_receiver) = InputSendQueueSender::channel();
        let existing = {
            let mut links = lock(&self.links);
            // Authentication may finish after a newer connection was installed.
            // Check and insert under one lock so simultaneous callers cannot
            // replace a link with itself and close the shared transport.
            if connection.close_reason().is_some() {
                return Err(NetworkError::Closed);
            }
            if links
                .get(&peer_id)
                .is_some_and(|existing| existing.session_id == session_id)
            {
                return Ok(());
            }
            links.insert(
                peer_id.clone(),
                ActiveLink {
                    session_id,
                    session: session.clone(),
                    connection: connection.clone(),
                    input_sender,
                },
            )
        };
        if let Some(existing) = existing {
            tracing::warn!(
                event = "input.connection.replaced",
                peer_id = %crate::arc_input::log_peer_id(peer_id.as_str()),
                old_session_id = existing.session_id,
                new_session_id = session_id,
                "Arc Input connection was replaced"
            );
            existing.session.close("input session replaced");
        }
        self.clear_relay_peer(&peer_id);
        tracing::info!(
            event = "input.connection.authenticated",
            peer_id = %crate::arc_input::log_peer_id(peer_id.as_str()),
            session_id,
            "Arc Input connection authenticated"
        );
        let _ = self.events.send(NetworkEvent::Connected {
            service_instance_id: peer_id.clone(),
            session_id,
        });
        let hub = self.clone();
        let input_session = session.clone();
        let input_connection = connection.clone();
        let input_counters = self.counters.clone();
        tokio::spawn(async move {
            if let Err(error) =
                send_input_loop(input_connection, input_receiver, input_counters).await
            {
                tracing::warn!(
                    event = "input.transport.sender_stopped",
                    session_id,
                    %error,
                    "Arc Input reliable sender stopped"
                );
                input_session.close("input send failed");
            }
        });
        tokio::spawn(async move {
            let reason = hub
                .receive_loop(peer_id.clone(), session_id, connection)
                .await
                .err()
                .map_or_else(
                    || "connection closed".to_string(),
                    |error| error.to_string(),
                );
            let removed = {
                let mut links = lock(&hub.links);
                links
                    .get(&peer_id)
                    .is_some_and(|link| link.session_id == session_id)
                    .then(|| links.remove(&peer_id))
                    .flatten()
                    .is_some()
            };
            if removed {
                tracing::warn!(
                    event = "input.connection.closed",
                    peer_id = %crate::arc_input::log_peer_id(peer_id.as_str()),
                    session_id,
                    reason = %crate::arc_input::sanitize_log_text(&reason, 1_024),
                    "Arc Input connection closed"
                );
                let _ = hub.events.send(NetworkEvent::Disconnected {
                    service_instance_id: peer_id,
                    session_id,
                    reason,
                });
            }
        });
        Ok(())
    }

    async fn receive_loop(
        self: &Arc<Self>,
        peer_id: ServiceInstanceId,
        session_id: u64,
        connection: quinn::Connection,
    ) -> Result<(), NetworkError> {
        loop {
            tokio::select! {
                closed = connection.closed() => return Err(NetworkError::Connection(closed.to_string())),
                stream = connection.accept_bi() => {
                    let (send, mut receive) = stream?;
                    let kind = receive.read_u8().await?;
                    if kind == arcrelay_wire::STREAM_KIND_INPUT_RELAY {
                        self.start_relay_request(peer_id.clone(), session_id, send, receive);
                        continue;
                    }
                    if kind != arcrelay_input::STREAM_KIND_INPUT_CONTROL {
                        return Err(NetworkError::InvalidStream);
                    }
                    let frame = wire::read_control(&mut receive).await?;
                    let _ = self.events.send(NetworkEvent::Control {
                        service_instance_id: peer_id.clone(),
                        session_id,
                        frame,
                    });
                }
                stream = connection.accept_uni() => {
                    let mut receive = stream?;
                    if receive.read_u8().await? != arcrelay_input::STREAM_KIND_RELIABLE_INPUT {
                        return Err(NetworkError::InvalidStream);
                    }
                    let events = self.events.clone();
                    let input_peer = peer_id.clone();
                    let counters = self.counters.clone();
                    tokio::spawn(async move {
                        loop {
                            match wire::read_input(&mut receive).await {
                                Ok(batch) => {
                                    counters.received.fetch_add(1, Ordering::Relaxed);
                                    let _ = events.send(NetworkEvent::Input {
                                        service_instance_id: input_peer.clone(),
                                        session_id,
                                        batch,
                                    });
                                }
                                Err(error) => {
                                    tracing::debug!(
                                        event = "input.transport.receive_stream_closed",
                                        %error,
                                        peer_id = %crate::arc_input::log_peer_id(input_peer.as_str()),
                                        "Arc Input reliable input stream closed"
                                    );
                                    break;
                                }
                            }
                        }
                    });
                }
            }
        }
    }

    pub async fn send_control(
        &self,
        peer: &ServiceInstanceId,
        frame: &proto::ControlFrame,
    ) -> Result<(), NetworkError> {
        let connection = lock(&self.links)
            .get(peer)
            .map(|link| link.connection.clone())
            .ok_or(NetworkError::NotConnected)?;
        let (mut send, _receive) = connection.open_bi().await?;
        send.write_u8(arcrelay_input::STREAM_KIND_INPUT_CONTROL)
            .await?;
        wire::write_control(&mut send, frame).await?;
        send.finish()?;
        Ok(())
    }

    pub fn send_input(
        &self,
        peer: &ServiceInstanceId,
        batch: proto::InputEventBatch,
        replace_pending_motion: bool,
    ) -> Result<InputEnqueueOutcome, NetworkError> {
        let sender = lock(&self.links)
            .get(peer)
            .map(|link| link.input_sender.clone())
            .ok_or(NetworkError::NotConnected)?;
        let sequence = batch.header.as_ref().map_or(0, |header| header.sequence);
        match sender.try_send(batch, replace_pending_motion) {
            Ok(outcome) => {
                if outcome == InputEnqueueOutcome::Queued {
                    self.counters.enqueued.fetch_add(1, Ordering::Relaxed);
                }
                let pending = sender.pending();
                self.counters
                    .max_pending
                    .fetch_max(pending, Ordering::Relaxed);
                Ok(outcome)
            }
            Err(InputSendQueueError::Full) => {
                self.counters.queue_full.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(
                    event = "input.transport.queue_full",
                    peer_id = %crate::arc_input::log_peer_id(peer.as_str()),
                    sequence,
                    queue_capacity = INPUT_SEND_QUEUE_CAPACITY,
                    "Arc Input send queue is full"
                );
                Err(NetworkError::InputQueueFull)
            }
            Err(InputSendQueueError::Closed) => Err(NetworkError::Closed),
        }
    }

    pub fn take_input_metrics(&self) -> InputTransportMetrics {
        let pending = lock(&self.links)
            .values()
            .map(|link| link.input_sender.pending())
            .sum();
        InputTransportMetrics {
            enqueued: self.counters.enqueued.swap(0, Ordering::AcqRel),
            written: self.counters.written.swap(0, Ordering::AcqRel),
            received: self.counters.received.swap(0, Ordering::AcqRel),
            queue_full: self.counters.queue_full.swap(0, Ordering::AcqRel),
            max_pending: self.counters.max_pending.swap(0, Ordering::AcqRel),
            pending,
        }
    }
}

async fn send_input_loop(
    connection: quinn::Connection,
    mut receiver: InputSendQueueReceiver,
    counters: Arc<TransportCounters>,
) -> Result<(), NetworkError> {
    let Some(first) = receiver.recv().await else {
        return Ok(());
    };
    let mut stream = connection.open_uni().await?;
    stream
        .write_u8(arcrelay_input::STREAM_KIND_RELIABLE_INPUT)
        .await?;
    wire::write_input(&mut stream, &first).await?;
    counters.written.fetch_add(1, Ordering::Relaxed);
    while let Some(batch) = receiver.recv().await {
        wire::write_input(&mut stream, &batch).await?;
        counters.written.fetch_add(1, Ordering::Relaxed);
    }
    stream.finish()?;
    Ok(())
}

#[cfg(test)]
mod reserved_session_tests {
    use std::path::Path;
    use std::time::Duration;

    use arcrelay_network::{DeviceMetadata, NetworkRuntimeConfig, PeerAdvertisement};
    use arcrelay_peer::{
        Grant, GrantConstraints, GrantDirection, InMemoryPeerRepository, PeerRepository,
    };

    use super::*;

    async fn runtime(directory: &Path, name: &str) -> Arc<NetworkRuntime> {
        let repository: Arc<dyn PeerRepository> = Arc::new(InMemoryPeerRepository::default());
        let mut config = NetworkRuntimeConfig::new(
            directory.to_path_buf(),
            DeviceMetadata {
                name: name.into(),
                platform: "test".into(),
                model: "test".into(),
            },
            repository,
        );
        config.listen_address = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        NetworkRuntime::bind(config).await.unwrap()
    }

    fn advertisement(runtime: &NetworkRuntime) -> PeerAdvertisement {
        let address = SocketAddr::new(
            IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            runtime.local_port().unwrap(),
        );
        PeerAdvertisement {
            device_id: runtime.device_id(),
            public_key: runtime.public_key(),
            metadata: runtime.metadata(),
            addresses: vec![address.ip()],
            connection_addresses: vec![address],
            port: address.port(),
            certificate_sha256: runtime.certificate_sha256(),
            last_seen_at_ms: 1,
        }
    }

    #[tokio::test]
    async fn reserved_receiver_buffers_reconnect_until_input_listener_starts() {
        let root = tempfile::tempdir().unwrap();
        let left = runtime(&root.path().join("left"), "Left").await;
        let right = runtime(&root.path().join("right"), "Right").await;
        let mut right_pairing = right.subscribe();

        let left_pairing = left
            .connect(&advertisement(&right), SessionKind::Pairing)
            .await
            .unwrap();
        let right_pairing_session =
            tokio::time::timeout(Duration::from_secs(2), right_pairing.recv())
                .await
                .unwrap()
                .unwrap();
        left.confirm_pairing(&left_pairing).await.unwrap();
        right
            .confirm_pairing_with_grants(
                &right_pairing_session,
                vec![Grant {
                    peer_id: left.device_id(),
                    capability: CapabilityId::CrossScreenInject,
                    direction: GrantDirection::Inbound,
                    constraints: GrantConstraints::None,
                    granted_at_ms: 1,
                }],
            )
            .await
            .unwrap();
        left_pairing.close("pairing complete");

        let network_cell = Arc::new(tokio::sync::OnceCell::new());
        assert!(network_cell.set(right.clone()).is_ok());
        let hub = PeerConnectionHub::bind(network_cell).unwrap();
        let mut events = hub.subscribe();

        // Production reserves this receiver before publishing the shared
        // network runtime. Let the peer complete its reconnect before the Arc
        // Input listener starts to reproduce the startup ordering window.
        let reserved = right.subscribe();
        let outbound = left
            .connect(&advertisement(&right), SessionKind::RealtimeInput)
            .await
            .unwrap();
        let listener = tokio::spawn(hub.clone().run_with_incoming(reserved));

        let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            event,
            NetworkEvent::Connected {
                service_instance_id, ..
            } if service_instance_id.as_str() == left.device_id().as_str()
        ));
        assert_eq!(hub.connected_peers().len(), 1);

        outbound.close("test complete");
        listener.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn repeated_and_obsolete_attaches_cannot_close_the_current_link() {
        let root = tempfile::tempdir().unwrap();
        let left = runtime(&root.path().join("left"), "Left").await;
        let right = runtime(&root.path().join("right"), "Right").await;
        let mut incoming = right.subscribe();
        let pair = left
            .connect(&advertisement(&right), SessionKind::Pairing)
            .await
            .unwrap();
        let remote_pair = incoming.recv().await.unwrap();
        left.confirm_pairing(&pair).await.unwrap();
        right.confirm_pairing(&remote_pair).await.unwrap();
        let cell = Arc::new(tokio::sync::OnceCell::new_with(Some(right.clone())));
        let hub = PeerConnectionHub::bind(cell).unwrap();
        let mut events = hub.subscribe();
        let old_outbound = left
            .connect(&advertisement(&right), SessionKind::RealtimeInput)
            .await
            .unwrap();
        let old = incoming.recv().await.unwrap();
        let barrier = Arc::new(tokio::sync::Barrier::new(32));
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..32 {
            let (hub, session, barrier) = (hub.clone(), old.clone(), barrier.clone());
            tasks.spawn(async move {
                barrier.wait().await;
                hub.attach(session).await
            });
        }
        while let Some(result) = tasks.join_next().await {
            result.unwrap().unwrap();
        }
        assert!(old.transport_handle().close_reason().is_none());
        assert!(matches!(
            events.try_recv().unwrap(),
            NetworkEvent::Connected { .. }
        ));
        assert!(
            events.try_recv().is_err(),
            "duplicate attaches must not reset metadata"
        );

        old.close("simulate old transport loss");
        old_outbound.close("simulate old transport loss");
        let _new_outbound = left
            .connect(&advertisement(&right), SessionKind::RealtimeInput)
            .await
            .unwrap();
        let new = incoming.recv().await.unwrap();
        hub.attach(new.clone()).await.unwrap();
        assert!(hub.attach(old).await.is_err());
        let peer = ServiceInstanceId::parse(left.device_id().to_string()).unwrap();
        assert!(hub.is_current_session(&peer, new.id()));
        assert!(new.transport_handle().close_reason().is_none());
        left.shutdown("test complete");
        right.shutdown("test complete");
    }
}

#[cfg(test)]
mod input_send_queue_tests {
    use super::*;

    fn batch(sequence: u64, event: proto::input_event::Event) -> proto::InputEventBatch {
        proto::InputEventBatch {
            header: Some(proto::RuntimeHeader {
                workspace_id: "workspace".into(),
                topology_revision: 7,
                control_epoch: 3,
                source_device_id: "source".into(),
                target_device_id: "target".into(),
                sequence,
            }),
            events: vec![proto::InputEvent { event: Some(event) }],
            held_state_checksum: 11,
            sent_at_unix_micros: sequence,
        }
    }

    fn motion(sequence: u64, x: i64) -> proto::InputEventBatch {
        batch(
            sequence,
            proto::input_event::Event::PointerMotion(proto::PointerMotion {
                desk_x_um: x,
                desk_y_um: -x,
            }),
        )
    }

    #[tokio::test]
    async fn pending_motion_is_replaced_without_consuming_a_sequence() {
        let (sender, mut receiver) = InputSendQueueSender::channel();
        assert_eq!(
            sender.try_send(motion(1, 10), true),
            Ok(InputEnqueueOutcome::Queued)
        );
        assert_eq!(
            sender.try_send(motion(2, 20), true),
            Ok(InputEnqueueOutcome::ReplacedPendingMotion)
        );
        assert_eq!(sender.pending(), 1);

        let delivered = receiver.recv().await.unwrap();
        assert_eq!(delivered.header.unwrap().sequence, 1);
        let proto::input_event::Event::PointerMotion(pointer) =
            delivered.events.into_iter().next().unwrap().event.unwrap()
        else {
            panic!("expected pointer motion");
        };
        assert_eq!((pointer.desk_x_um, pointer.desk_y_um), (20, -20));
    }

    #[tokio::test]
    async fn discrete_input_seals_the_motion_ordering_boundary() {
        let (sender, mut receiver) = InputSendQueueSender::channel();
        sender.try_send(motion(1, 10), true).unwrap();
        sender
            .try_send(
                batch(
                    2,
                    proto::input_event::Event::PointerButton(proto::PointerButton {
                        hid_usage: 1,
                        down: true,
                        click_count: 1,
                    }),
                ),
                false,
            )
            .unwrap();
        sender.try_send(motion(3, 30), true).unwrap();
        assert_eq!(
            sender.try_send(motion(4, 40), true),
            Ok(InputEnqueueOutcome::ReplacedPendingMotion)
        );

        let first = receiver.recv().await.unwrap();
        let second = receiver.recv().await.unwrap();
        let third = receiver.recv().await.unwrap();
        assert_eq!(first.header.unwrap().sequence, 1);
        assert!(matches!(
            second.events[0].event,
            Some(proto::input_event::Event::PointerButton(_))
        ));
        assert_eq!(second.header.unwrap().sequence, 2);
        assert_eq!(third.header.as_ref().unwrap().sequence, 3);
        let Some(proto::input_event::Event::PointerMotion(pointer)) = &third.events[0].event else {
            panic!("expected pointer motion");
        };
        assert_eq!(pointer.desk_x_um, 40);
    }

    #[test]
    fn dropping_receiver_closes_the_queue() {
        let (sender, receiver) = InputSendQueueSender::channel();
        drop(receiver);
        assert_eq!(
            sender.try_send(motion(1, 10), true),
            Err(InputSendQueueError::Closed)
        );
    }

    #[tokio::test]
    async fn system_gesture_phases_are_ordered_and_never_coalesced() {
        let (sender, mut receiver) = InputSendQueueSender::channel();
        sender.try_send(motion(1, 10), true).unwrap();
        for (index, phase) in [1, 2, 2, 4].into_iter().enumerate() {
            let event = proto::input_event::Event::SystemGesture(
                arcrelay_input::SystemGestureEvent {
                    axis: 1,
                    phase,
                    progress: -0.1 * index as f64,
                    velocity_x: 0.0,
                    velocity_y: 0.0,
                    inverted_from_device: false,
                    finger_count: 0,
                }
                .into(),
            );
            // Even a mistaken coalescing flag cannot replace a gesture frame.
            assert_eq!(
                sender.try_send(batch(index as u64 + 2, event), true),
                Ok(InputEnqueueOutcome::Queued)
            );
        }
        sender.try_send(motion(6, 60), true).unwrap();
        sender.try_send(motion(7, 70), true).unwrap();
        assert_eq!(sender.pending(), 6);
        for sequence in 1..=6 {
            let delivered = receiver.recv().await.unwrap();
            assert_eq!(delivered.header.unwrap().sequence, sequence);
            if (2..=5).contains(&sequence) {
                assert!(matches!(
                    delivered.events[0].event,
                    Some(proto::input_event::Event::SystemGesture(_))
                ));
            }
        }
    }

    #[tokio::test]
    async fn consumer_down_repeat_up_seal_motion_and_remain_ordered() {
        let (sender, mut receiver) = InputSendQueueSender::channel();
        sender.try_send(motion(1, 10), true).unwrap();
        for (index, (down, repeat)) in [(true, false), (true, true), (false, false)]
            .into_iter()
            .enumerate()
        {
            let value = batch(
                index as u64 + 2,
                proto::input_event::Event::ConsumerKey(proto::ConsumerKey {
                    hid_usage: 0xe9,
                    down,
                    repeat,
                }),
            );
            assert_eq!(
                sender.try_send(value, true),
                Ok(InputEnqueueOutcome::Queued)
            );
        }
        sender.try_send(motion(5, 50), true).unwrap();
        sender.try_send(motion(6, 60), true).unwrap();
        assert_eq!(sender.pending(), 5);
        for sequence in 1..=5 {
            let delivered = receiver.recv().await.unwrap();
            assert_eq!(delivered.header.unwrap().sequence, sequence);
            if (2..=4).contains(&sequence) {
                let Some(proto::input_event::Event::ConsumerKey(key)) = &delivered.events[0].event
                else {
                    panic!("consumer event expected");
                };
                assert_eq!(
                    (key.down, key.repeat),
                    match sequence {
                        2 => (true, false),
                        3 => (true, true),
                        _ => (false, false),
                    }
                );
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) async fn wait_for_network(
    cell: &tokio::sync::OnceCell<Arc<NetworkRuntime>>,
) -> Arc<NetworkRuntime> {
    loop {
        if let Some(network) = cell.get() {
            return network.clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("Arc Input networking requires an active Tokio runtime")]
    RuntimeUnavailable,
    #[error(transparent)]
    Unified(#[from] arcrelay_network::NetworkError),
    #[error("QUIC connection failed: {0}")]
    QuinnConnection(#[from] quinn::ConnectionError),
    #[error("QUIC stream failed: {0}")]
    QuinnWrite(#[from] quinn::WriteError),
    #[error("QUIC stream closed: {0}")]
    QuinnClosed(#[from] quinn::ClosedStream),
    #[error("Arc Input stream I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Arc Input wire failed: {0}")]
    Wire(#[from] wire::WireError),
    #[error("peer identity is not a unified device id")]
    InvalidPeerIdentity,
    #[error("peer is not connected")]
    NotConnected,
    #[error("invalid Arc Input stream kind")]
    InvalidStream,
    #[error("network runtime stopped")]
    Closed,
    #[error("Arc Input send queue is full")]
    InputQueueFull,
    #[error("connection failed: {0}")]
    Connection(String),
}
