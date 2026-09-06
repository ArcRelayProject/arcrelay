use super::*;
use arcrelay_wire::common::{InputRelayOpen, InputRelayReady};
use prost::Message;
use std::collections::BTreeSet;
use std::time::Duration;
use tokio::sync::Semaphore;

type RelayAuthorizer = dyn Fn(&InputRelayOpen, &ServiceInstanceId) -> bool + Send + Sync;

struct RelaySessionLease(Arc<Session>);

impl Drop for RelaySessionLease {
    fn drop(&mut self) {
        self.0.close("input relay scope closed");
    }
}

pub(super) struct RelayState {
    authorizer: Mutex<Option<Arc<RelayAuthorizer>>>,
    capable: Mutex<BTreeSet<ServiceInstanceId>>,
    routes: Mutex<HashMap<ServiceInstanceId, BTreeSet<ServiceInstanceId>>>,
    capacity: Arc<Semaphore>,
    jobs: Mutex<Vec<tokio::task::AbortHandle>>,
}

impl Default for RelayState {
    fn default() -> Self {
        Self {
            authorizer: Mutex::new(None),
            capable: Mutex::new(BTreeSet::new()),
            routes: Mutex::new(HashMap::new()),
            capacity: Arc::new(Semaphore::new(8)),
            jobs: Mutex::new(Vec::new()),
        }
    }
}

impl PeerConnectionHub {
    pub fn set_relay_authorizer(
        &self,
        authorizer: impl Fn(&InputRelayOpen, &ServiceInstanceId) -> bool + Send + Sync + 'static,
    ) {
        *lock(&self.relay.authorizer) = Some(Arc::new(authorizer));
    }

    pub fn set_relay_capable(&self, peer: &ServiceInstanceId, capable: bool) -> bool {
        if capable {
            lock(&self.relay.capable).insert(peer.clone())
        } else {
            lock(&self.relay.capable).remove(peer)
        }
    }

    pub(super) fn clear_relay_peer(&self, peer: &ServiceInstanceId) {
        lock(&self.relay.capable).remove(peer);
        lock(&self.relay.routes).remove(peer);
    }

    pub fn set_relay_routes(&self, via: ServiceInstanceId, targets: BTreeSet<ServiceInstanceId>) {
        lock(&self.relay.routes).insert(via, targets);
    }

    pub fn abort_relays(&self) {
        for job in lock(&self.relay.jobs).drain(..) {
            job.abort();
        }
        let relayed = lock(&self.links)
            .iter()
            .filter(|(_, link)| link.session.is_relayed())
            .map(|(peer, _)| peer.clone())
            .collect::<Vec<_>>();
        for peer in relayed {
            self.disconnect(&peer, "input relay disabled");
        }
    }

    pub fn direct_relay_target(&self, peer: &ServiceInstanceId) -> Option<Arc<Session>> {
        if !lock(&self.relay.capable).contains(peer) {
            return None;
        }
        self.direct_session(peer)
    }

    fn direct_session(&self, peer: &ServiceInstanceId) -> Option<Arc<Session>> {
        lock(&self.links)
            .get(peer)
            .filter(|link| !link.session.is_relayed() && link.connection.close_reason().is_none())
            .map(|link| link.session.clone())
    }

    pub fn has_relay_route(&self, target: &ServiceInstanceId) -> bool {
        !self.relay_candidates(target).is_empty()
    }

    fn relay_candidates(&self, target: &ServiceInstanceId) -> Vec<ServiceInstanceId> {
        lock(&self.relay.routes)
            .iter()
            .filter(|(via, targets)| targets.contains(target) && self.direct_session(via).is_some())
            .map(|(via, _)| via.clone())
            .take(3)
            .collect()
    }

    pub async fn connect_via_relay(
        self: &Arc<Self>,
        target: ServiceInstanceId,
        workspace: String,
    ) -> Result<(), NetworkError> {
        let network = wait_for_network(&self.network).await;
        let target_id =
            DeviceId::parse(target.as_str()).map_err(|_| NetworkError::InvalidPeerIdentity)?;
        let mut failure = NetworkError::NotConnected;
        for via in self.relay_candidates(&target) {
            let Some(parent) = self.direct_session(&via) else {
                continue;
            };
            let request = InputRelayOpen {
                version: 1,
                workspace_id: workspace.clone(),
                source_device_id: network.device_id().to_string(),
                target_device_id: target.to_string(),
                forwarded: false,
            };
            if !self.relay_allowed(&request, &via) {
                continue;
            }
            let opened = tokio::time::timeout(Duration::from_secs(5), async {
                let (mut send, mut receive) = parent.transport_handle().open_bi().await?;
                send.write_u8(arcrelay_wire::STREAM_KIND_INPUT_RELAY)
                    .await?;
                write_header(&mut send, &request).await?;
                let ready: InputRelayReady = read_header(&mut receive).await?;
                if !ready.accepted {
                    return Err(NetworkError::Connection(ready.error));
                }
                Ok::<_, NetworkError>((send, receive))
            })
            .await;
            let result = match opened {
                Ok(Ok((send, receive))) => network
                    .connect_input_relay(&target_id, send, receive)
                    .await
                    .map_err(NetworkError::from),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(NetworkError::Connection(
                    "input relay negotiation timed out".into(),
                )),
            };
            match result {
                Ok(session) => {
                    if !self.relay_allowed(&request, &via) {
                        if session.is_relayed() {
                            session.close("input relay scope changed");
                        }
                        return Err(NetworkError::Closed);
                    }
                    tracing::info!(event = "input.connection.relay_connected",
                        peer_id = %crate::arc_input::log_peer_id(target.as_str()),
                        relay_peer_id = %crate::arc_input::log_peer_id(via.as_str()),
                        "Arc Input established an end-to-end session through a paired relay");
                    self.attach(session.clone()).await?;
                    self.watch_relay_scope(session, request, target.clone());
                    return Ok(());
                }
                Err(error) => failure = error,
            }
        }
        Err(failure)
    }

    fn relay_allowed(&self, request: &InputRelayOpen, via: &ServiceInstanceId) -> bool {
        lock(&self.relay.authorizer)
            .as_ref()
            .is_some_and(|check| check(request, via))
    }

    fn watch_relay_scope(
        self: &Arc<Self>,
        session: Arc<Session>,
        request: InputRelayOpen,
        peer: ServiceInstanceId,
    ) {
        if !session.is_relayed() {
            return;
        }
        let hub = self.clone();
        let lease = RelaySessionLease(session);
        let job = tokio::spawn(async move {
            let connection = lease.0.transport_handle();
            let _lease = lease;
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = connection.closed() => return,
                    _ = tick.tick() => if !hub.relay_allowed(&request, &peer) { return },
                }
            }
        });
        let mut jobs = lock(&self.relay.jobs);
        jobs.retain(|job| !job.is_finished());
        jobs.push(job.abort_handle());
    }

    pub(super) fn start_relay_request(
        self: &Arc<Self>,
        peer: ServiceInstanceId,
        session_id: u64,
        send: quinn::SendStream,
        receive: quinn::RecvStream,
    ) {
        let Ok(permit) = self.relay.capacity.clone().try_acquire_owned() else {
            return;
        };
        let hub = self.clone();
        let job = tokio::spawn(async move {
            let _permit = permit;
            if let Err(error) = hub
                .serve_relay_request(peer, session_id, send, receive)
                .await
            {
                tracing::debug!(event = "input.connection.relay_closed", %error,
                    "Arc Input relay request finished");
            }
        });
        let mut jobs = lock(&self.relay.jobs);
        jobs.retain(|job| !job.is_finished());
        jobs.push(job.abort_handle());
    }

    async fn serve_relay_request(
        self: &Arc<Self>,
        peer: ServiceInstanceId,
        session_id: u64,
        mut send: quinn::SendStream,
        mut receive: quinn::RecvStream,
    ) -> Result<(), NetworkError> {
        let request: InputRelayOpen =
            tokio::time::timeout(Duration::from_secs(5), read_header(&mut receive))
                .await
                .map_err(|_| NetworkError::InvalidStream)??;
        let network = wait_for_network(&self.network).await;
        let local = network.device_id().to_string();
        let source = ServiceInstanceId::parse(&request.source_device_id)
            .map_err(|_| NetworkError::InvalidPeerIdentity)?;
        let target = ServiceInstanceId::parse(&request.target_device_id)
            .map_err(|_| NetworkError::InvalidPeerIdentity)?;
        let parent = self
            .direct_session(&peer)
            .filter(|session| session.id() == session_id);
        let valid_role = if request.forwarded {
            request.target_device_id == local && source != peer
        } else {
            source == peer && request.target_device_id != local
        };
        if request.version != 1
            || source == target
            || !valid_role
            || parent.is_none()
            || !self.relay_allowed(&request, &peer)
        {
            write_header(
                &mut send,
                &InputRelayReady {
                    accepted: false,
                    error: "input relay scope is unavailable".into(),
                },
            )
            .await?;
            return Err(NetworkError::InvalidStream);
        }
        let parent = parent.expect("validated input relay parent");
        network
            .require(&parent, CapabilityId::CrossScreenInject)
            .await?;
        if request.forwarded {
            let source_id =
                DeviceId::parse(source.as_str()).map_err(|_| NetworkError::InvalidPeerIdentity)?;
            // Reject an unpaired source before reserving an inner endpoint.
            if !network.paired_peers().await?.iter().any(|record| {
                record.device_id == source_id
                    && record.trust_state == arcrelay_peer::TrustState::Paired
            }) {
                return Err(NetworkError::InvalidPeerIdentity);
            }
            write_header(
                &mut send,
                &InputRelayReady {
                    accepted: true,
                    error: String::new(),
                },
            )
            .await?;
            let session = network
                .accept_input_relay(&source_id, send, receive)
                .await?;
            if !self.relay_allowed(&request, &peer) {
                session.close("input relay scope changed");
                return Err(NetworkError::Closed);
            }
            self.attach(session.clone()).await?;
            self.watch_relay_scope(session, request, source);
            return Ok(());
        }
        let target_session = self
            .direct_relay_target(&target)
            .ok_or(NetworkError::NotConnected)?;
        network
            .require(&target_session, CapabilityId::CrossScreenInject)
            .await?;
        let (mut target_send, mut target_receive) =
            tokio::time::timeout(Duration::from_secs(5), async {
                let (mut target_send, mut target_receive) =
                    target_session.transport_handle().open_bi().await?;
                target_send
                    .write_u8(arcrelay_wire::STREAM_KIND_INPUT_RELAY)
                    .await?;
                let mut forwarded = request.clone();
                forwarded.forwarded = true;
                write_header(&mut target_send, &forwarded).await?;
                let ready: InputRelayReady = read_header(&mut target_receive).await?;
                if !ready.accepted {
                    return Err(NetworkError::Connection(ready.error));
                }
                Ok::<_, NetworkError>((target_send, target_receive))
            })
            .await
            .map_err(|_| NetworkError::InvalidStream)??;
        write_header(
            &mut send,
            &InputRelayReady {
                accepted: true,
                error: String::new(),
            },
        )
        .await?;
        let scope = async {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if !self.relay_allowed(&request, &peer) {
                    return;
                }
            }
        };
        let parent_connection = parent.transport_handle();
        let target_connection = target_session.transport_handle();
        tokio::select! {
            result = copy_packets(&mut receive, &mut target_send) => result,
            result = copy_packets(&mut target_receive, &mut send) => result,
            _ = parent_connection.closed() => Err(NetworkError::Closed),
            _ = target_connection.closed() => Err(NetworkError::Closed),
            _ = scope => Err(NetworkError::Closed),
        }
    }
}

async fn read_header<M: Message + Default>(
    receive: &mut quinn::RecvStream,
) -> Result<M, NetworkError> {
    let bytes = arcrelay_transport::read_frame(receive, arcrelay_wire::MAX_INPUT_RELAY_HEADER_SIZE)
        .await
        .map_err(|error| NetworkError::Connection(error.to_string()))?;
    M::decode(bytes.as_slice()).map_err(|_| NetworkError::InvalidStream)
}

async fn write_header<M: Message>(
    send: &mut quinn::SendStream,
    value: &M,
) -> Result<(), NetworkError> {
    arcrelay_transport::write_frame(
        send,
        &value.encode_to_vec(),
        arcrelay_wire::MAX_INPUT_RELAY_HEADER_SIZE,
    )
    .await
    .map_err(|error| NetworkError::Connection(error.to_string()))
}

async fn copy_packets(
    receive: &mut quinn::RecvStream,
    send: &mut quinn::SendStream,
) -> Result<(), NetworkError> {
    loop {
        let packet =
            arcrelay_transport::read_frame(receive, arcrelay_wire::MAX_INPUT_RELAY_PACKET_SIZE)
                .await
                .map_err(|error| NetworkError::Connection(error.to_string()))?;
        if packet.is_empty() {
            return Err(NetworkError::InvalidStream);
        }
        arcrelay_transport::write_frame(send, &packet, arcrelay_wire::MAX_INPUT_RELAY_PACKET_SIZE)
            .await
            .map_err(|error| NetworkError::Connection(error.to_string()))?;
    }
}
