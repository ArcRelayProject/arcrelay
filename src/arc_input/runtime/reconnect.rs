use super::*;

const REVERSE_DIAL_DELAY: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub(super) struct ReconnectAttempt {
    next_attempt: Instant,
    failures: u32,
    in_flight: bool,
}

impl ReconnectAttempt {
    fn new(now: Instant, preferred_dialer: bool) -> Self {
        Self {
            next_attempt: now
                + if preferred_dialer {
                    Duration::ZERO
                } else {
                    REVERSE_DIAL_DELAY
                },
            failures: 0,
            in_flight: false,
        }
    }

    fn start(&mut self, now: Instant) -> bool {
        if self.in_flight || now < self.next_attempt {
            return false;
        }
        self.in_flight = true;
        true
    }

    fn finish(&mut self, now: Instant, succeeded: bool) {
        self.in_flight = false;
        self.failures = if succeeded {
            0
        } else {
            self.failures.saturating_add(1)
        };
        let delay = if succeeded {
            Duration::ZERO
        } else {
            crate::retry::delay(self.failures)
        };
        self.next_attempt = now + delay;
    }
}

impl ArcInputRuntime {
    pub(super) async fn connect_with_relay_fallback<F, Attempt>(
        self: &Arc<Self>,
        peer: &ServiceInstanceId,
        direct: F,
    ) -> Result<(), NetworkError>
    where
        F: Fn() -> Attempt,
        Attempt: std::future::Future<Output = Result<(), NetworkError>>,
    {
        let relay_available = async {
            tokio::time::sleep(Duration::from_secs(2)).await;
            while !self.network.has_relay_route(peer) {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        };
        let direct_result = {
            let attempt = direct();
            tokio::pin!(attempt);
            tokio::select! {
                biased;
                result = &mut attempt => Some(result),
                _ = relay_available => None,
            }
        };
        // Drop the pending direct dial before negotiating a tunnel. Otherwise
        // two same-initiator handshakes can replace each other in opposite order.
        match direct_result {
            Some(Ok(())) => return Ok(()),
            Some(Err(error)) if !self.network.has_relay_route(peer) => return Err(error),
            _ => {}
        }
        let workspace = self
            .store
            .snapshot()
            .layout
            .map(|layout| layout.workspace_id.to_string())
            .unwrap_or_default();
        match self
            .network
            .connect_via_relay(peer.clone(), workspace)
            .await
        {
            Ok(()) => Ok(()),
            // A stale/unavailable relay must not permanently shorten the direct
            // path's source-port recovery budget. Give it a complete new attempt.
            Err(_) => direct().await,
        }
    }

    /// Reconcile desired links on the inventory timer as well as discovery.
    /// One task per peer owns both direct and relayed attempts. A passive peer
    /// becomes a fallback dialer when the preferred side cannot reconnect.
    pub(super) fn reconnect_peers(self: &Arc<Self>) {
        if !self.input_sharing_enabled() {
            return;
        }
        let paired = read(&self.paired_peers).clone();
        let connected = self.network.connected_peers();
        let now = Instant::now();
        let mut attempts = lock(&self.reconnect_attempts);
        attempts.retain(|peer, attempt| paired.contains_key(peer) || attempt.in_flight);
        for (peer, record) in paired {
            if peer == self.identity.service_instance_id || !record.auto_connect {
                continue;
            }
            if connected.contains(&peer) {
                if attempts
                    .get(&peer)
                    .is_some_and(|attempt| !attempt.in_flight)
                {
                    attempts.remove(&peer);
                }
                continue;
            }
            let preferred = matches!(record.platform.as_str(), "ios" | "android")
                || should_dial(
                    &self.identity.service_instance_id,
                    &peer,
                    true,
                    DialIntent::Automatic,
                    false,
                );
            let attempt = attempts
                .entry(peer.clone())
                .or_insert_with(|| ReconnectAttempt::new(now, preferred));
            if !attempt.start(now) {
                continue;
            }
            let route = read(&self.relayed_routes).get(&peer).cloned();
            let runtime = self.clone();
            self.runtime_handle.spawn(async move {
                let direct = || async {
                    if let Some(route) = &route {
                        runtime
                            .network
                            .connect_relayed(peer.clone(), route.endpoints.clone())
                            .await
                    } else {
                        runtime.network.connect(peer.clone(), &[], 0).await
                    }
                };
                let result = runtime.connect_with_relay_fallback(&peer, direct).await;
                let succeeded = result.is_ok() && runtime.network.connected_peers().contains(&peer);
                if let Some(attempt) = lock(&runtime.reconnect_attempts).get_mut(&peer) {
                    attempt.finish(Instant::now(), succeeded);
                }
                if let Err(error) = result {
                    tracing::debug!(event = "input.connection.reconnect_failed",
                        peer_id = %crate::arc_input::log_peer_id(peer.as_str()), %error,
                        "Arc Input will retry the unavailable peer after backoff");
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_has_one_inflight_attempt_and_bounded_backoff() {
        let now = Instant::now();
        let mut attempt = ReconnectAttempt::new(now, true);
        assert!(attempt.start(now));
        assert!(!attempt.start(now + Duration::from_secs(120)));
        attempt.finish(now, false);
        assert!(!attempt.start(now + Duration::from_secs(1)));
        assert!(attempt.start(now + Duration::from_secs(2)));
        for _ in 0..50 {
            attempt.finish(now, false);
        }
        assert_eq!(attempt.next_attempt, now + Duration::from_secs(120));
        attempt.finish(now, true);
        assert!(attempt.start(now));
    }

    #[test]
    fn reverse_dialer_recovers_without_another_discovery_event() {
        let now = Instant::now();
        let mut attempt = ReconnectAttempt::new(now, false);
        assert!(!attempt.start(now));
        assert!(attempt.start(now + REVERSE_DIAL_DELAY));
        attempt.finish(now + REVERSE_DIAL_DELAY, false);
        assert!(attempt.start(now + REVERSE_DIAL_DELAY + Duration::from_secs(2)));
    }
}
