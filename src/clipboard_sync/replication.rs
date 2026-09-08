use super::*;
use arcrelay_protocol::clipboard_replication::{self as replica, PeerClient, ReconciliationResult};

#[derive(Debug, Clone, Default, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardMergeSummary {
    pub devices: usize,
    pub received: usize,
    pub sent: usize,
    pub labels_received: usize,
    pub labels_sent: usize,
    pub failed: usize,
    pub total_records: usize,
    pub complete: bool,
    pub failures: Vec<String>,
}

impl ClipboardSyncManager {
    async fn replica_transports(&self) -> HashMap<String, quinn::Connection> {
        let mut outgoing = self.replica_connections.lock().await;
        outgoing.retain(|_, c| c.close_reason().is_none());
        let mut transports = outgoing.clone();
        drop(outgoing);
        if let Some(registry) = self.incoming_connections.get() {
            for id in registry
                .list_connected_with_feature(proto::Feature::ClipboardSync as i32, replica::VERSION)
                .await
            {
                if let Some(connection) = registry
                    .transport_for_feature(
                        &id,
                        proto::Feature::ClipboardSync as i32,
                        replica::VERSION,
                    )
                    .await
                {
                    if connection.close_reason().is_none() {
                        transports.insert(id, connection);
                    }
                }
            }
        }
        transports
    }

    async fn reconcile_peer(
        &self,
        id: &str,
        connection: quinn::Connection,
    ) -> Result<ReconciliationResult, String> {
        let gate = self
            .replica_gates
            .lock()
            .await
            .entry(id.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        let _guard = gate.lock().await;
        replica::reconcile(self.clipboard.clone(), PeerClient::new(connection)).await
    }

    pub async fn merge_all(&self) -> Result<ClipboardMergeSummary, String> {
        let _guard = self
            .manual_merge_gate
            .try_lock()
            .map_err(|_| "clipboard synchronization is already running".to_owned())?;
        if !self.clipboard.sync_preferences().enabled {
            return Err("clipboard synchronization is disabled".into());
        }
        let transports = self.replica_transports().await;
        if transports.is_empty() {
            return Err("no online desktop supports reliable clipboard synchronization; update both desktops and reconnect".into());
        }
        let mut result = ClipboardMergeSummary {
            devices: transports.len(),
            ..Default::default()
        };
        let mut connected = self.active.lock().await.clone();
        if let Some(registry) = self.incoming_connections.get() {
            connected.extend(registry.list_connected().await);
        }
        let discovered = self.discovered.lock().await;
        let unsupported = connected
            .into_iter()
            .filter(|id| !transports.contains_key(id))
            .filter_map(|id| discovered.get(&id))
            .filter(|peer| hosts_desktop_control_service(&peer.metadata.platform))
            .map(|peer| {
                format!(
                    "{}: update this desktop to use reliable clipboard synchronization",
                    peer.metadata.name
                )
            })
            .collect::<Vec<_>>();
        drop(discovered);
        // A second pass propagates records learned from one peer to another peer
        // visited earlier. Success requires a stable zero-change round for all.
        for _ in 0..3 {
            let mut round_changes = 0;
            let mut round_complete = true;
            let mut tasks = Vec::new();
            for (id, connection) in &transports {
                let manager = self.clone();
                let id = id.clone();
                let connection = connection.clone();
                tasks.push(tokio::spawn(async move {
                    let outcome = tokio::time::timeout(
                        Duration::from_secs(180),
                        manager.reconcile_peer(&id, connection),
                    )
                    .await
                    .unwrap_or_else(|_| {
                        Err(
                            "clipboard reconciliation timed out; background recovery will continue"
                                .into(),
                        )
                    });
                    (id, outcome)
                }));
            }
            for task in tasks {
                let (id, outcome) = task
                    .await
                    .map_err(|_| "clipboard reconciliation task stopped".to_owned())?;
                match outcome {
                    Ok(peer) => {
                        round_changes +=
                            peer.received + peer.sent + peer.labels_received + peer.labels_sent;
                        result.received += peer.received;
                        result.sent += peer.sent;
                        result.labels_received += peer.labels_received;
                        result.labels_sent += peer.labels_sent;
                        result.failed += peer.failed;
                        result.total_records = result.total_records.max(peer.total_records);
                        round_complete &= peer.converged;
                        if !peer.converged {
                            let name = self
                                .discovered
                                .lock()
                                .await
                                .get(&id)
                                .map(|p| p.metadata.name.clone())
                                .unwrap_or_else(|| crate::observability::pseudonym(&id));
                            result.failures.push(format!(
                                "{name}: {}",
                                peer.errors.first().cloned().unwrap_or_else(|| {
                                    "new clipboard changes are still being synchronized".into()
                                })
                            ));
                        }
                    }
                    Err(error) => {
                        round_complete = false;
                        result.failed += 1;
                        let name = self
                            .discovered
                            .lock()
                            .await
                            .get(&id)
                            .map(|p| p.metadata.name.clone())
                            .unwrap_or_else(|| crate::observability::pseudonym(&id));
                        result.failures.push(format!("{name}: {error}"));
                    }
                }
            }
            if result.failed > 0 {
                break;
            }
            if round_complete && round_changes == 0 {
                result.complete = true;
                result.failures.clear();
                break;
            }
            if round_complete {
                result.failures.clear();
            }
        }
        if !unsupported.is_empty() {
            result.devices += unsupported.len();
            result.failed += unsupported.len();
            result.complete = false;
            result.failures.extend(unsupported);
        }
        let page = self
            .clipboard
            .history(arcrelay_core::domain::clipboard::ClipboardQuery {
                kinds: vec![
                    ClipboardContentKind::Text,
                    ClipboardContentKind::Html,
                    ClipboardContentKind::Image,
                ],
                ..arcrelay_core::domain::clipboard::ClipboardQuery::recent(1)
            })
            .await
            .map_err(|e| e.to_string())?;
        result.total_records = page.total_count.unwrap_or_default() as usize;
        result.failures.sort();
        result.failures.dedup();
        tracing::info!(
            event = "clipboard.replica.manual_completed",
            devices = result.devices,
            received = result.received,
            sent = result.sent,
            failed = result.failed,
            complete = result.complete,
            "manual bidirectional clipboard reconciliation finished"
        );
        Ok(result)
    }

    pub(super) async fn replication_loop(self: Arc<Self>) {
        let mut workers: HashMap<String, (usize, AbortTaskOnDrop)> = HashMap::new();
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            let transports = self.replica_transports().await;
            workers.retain(|id, (identity, task)| {
                transports
                    .get(id)
                    .is_some_and(|c| c.stable_id() == *identity)
                    && !task.0.is_finished()
            });
            for (id, connection) in transports {
                if workers.contains_key(&id) {
                    continue;
                }
                let manager = self.clone();
                let worker_id = id.clone();
                let identity = connection.stable_id();
                let task =
                    tokio::spawn(
                        async move { manager.replica_worker(worker_id, connection).await },
                    );
                workers.insert(id, (identity, AbortTaskOnDrop(task)));
            }
        }
    }

    async fn replica_worker(self: Arc<Self>, id: String, connection: quinn::Connection) {
        let Some(mut changes) = self.clipboard.subscribe_sync_changes() else {
            return;
        };
        let client = PeerClient::new(connection.clone());
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        let mut known_revisions = None;
        let mut reconciliation: Option<AbortTaskOnDrop<Result<ReconciliationResult, String>>> =
            None;
        let mut live_task: Option<AbortTaskOnDrop<Result<(), String>>> = None;
        let mut pending_copy: Option<ClipboardSyncRecord> = None;
        let mut recover_selection = true;
        let mut retry_at = tokio::time::Instant::now();
        let mut retry_delay = 2;
        let mut last_error = String::new();
        let mut reconcile_at = tokio::time::Instant::now();
        let mut reconcile_delay = 2;
        loop {
            tokio::select! {
                _ = connection.closed() => break,
                change = changes.recv() => {
                    match change {
                        Ok(mut record) if self.clipboard.should_send_sync_record(&record) => {
                            if record.live && record.change_kind == ClipboardSyncChangeKind::Copy {
                                // Keep only the latest live selection queued. Earlier content
                                // remains durable in SQLite and is recovered by reconciliation.
                                if pending_copy.as_ref().is_none_or(|p| (record.captured_at_ms, &record.updated_by_device_id, &record.sync_id)
                                    >= (p.captured_at_ms, &p.updated_by_device_id, &p.sync_id)) {
                                    if let Some(task) = live_task.take() { task.0.abort(); }
                                    record.text = None; record.html = None; record.rtf = None; record.image_png = None;
                                    pending_copy = Some(record); retry_at = tokio::time::Instant::now(); retry_delay = 2;
                                }
                            }
                            known_revisions = None; reconcile_at = tokio::time::Instant::now();
                        }
                        Ok(_) => {},
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            known_revisions = None;
                            recover_selection = true;
                            pending_copy = None; live_task = None; retry_at = tokio::time::Instant::now();
                        },
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = tick.tick() => {
                    if recover_selection {
                        match self.clipboard.replica_selection().await {
                            Ok(selection) => { pending_copy = selection; recover_selection = false; }
                            Err(error) => tracing::warn!(event = "clipboard.replica.selection_pending", %error, "durable clipboard selection will be retried"),
                        }
                    }
                    if let Some(task) = reconciliation.as_ref() {
                        if task.0.is_finished() {
                            let outcome = (&mut reconciliation.take().unwrap().0).await;
                            match outcome {
                                Ok(Ok(result)) if result.converged => {
                                    known_revisions = Some((result.local_revision, result.remote_revision));
                                    last_error.clear(); reconcile_delay = 2;
                                }
                                Ok(Ok(result)) => {
                                    known_revisions = None;
                                    reconcile_at = tokio::time::Instant::now() + Duration::from_secs(reconcile_delay);
                                    reconcile_delay = (reconcile_delay * 2).min(60);
                                    let error = result.errors.first().cloned().unwrap_or_else(|| "clipboard changed during reconciliation".into());
                                    if last_error != error {
                                        tracing::warn!(event = "clipboard.replica.reconciliation_pending", peer_id = %crate::observability::pseudonym(&id), %error, "clipboard reconciliation will retry");
                                        last_error = error;
                                    }
                                }
                                other => {
                                    known_revisions = None;
                                    reconcile_at = tokio::time::Instant::now() + Duration::from_secs(reconcile_delay);
                                    reconcile_delay = (reconcile_delay * 2).min(60);
                                    let error = format!("{other:?}");
                                    if last_error != error {
                                        tracing::warn!(event = "clipboard.replica.reconciliation_failed", peer_id = %crate::observability::pseudonym(&id), %error, "clipboard reconciliation will retry");
                                        last_error = error;
                                    }
                                }
                            }
                        }
                    }
                    if reconciliation.is_none() && tokio::time::Instant::now() >= reconcile_at && self.clipboard.sync_preferences().enabled {
                        // Probe revisions in the background too: a stalled peer must
                        // never block subscription consumption or a new live copy.
                        let manager = self.clone(); let id = id.clone(); let connection = connection.clone();
                        let known = known_revisions;
                        reconciliation = Some(AbortTaskOnDrop(tokio::spawn(async move {
                            let local = manager.clipboard.revision().await.map_err(|e| e.to_string())?;
                            let remote = PeerClient::new(connection.clone()).revision().await?;
                            if known == Some((local, remote)) {
                                let mut result = ReconciliationResult::default();
                                result.converged = true; result.local_revision = local; result.remote_revision = remote;
                                Ok(result)
                            } else { manager.reconcile_peer(&id, connection).await }
                        })));
                    }
                }
            }
            if live_task.as_ref().is_some_and(|task| task.0.is_finished()) {
                match (&mut live_task.take().unwrap().0).await {
                    Ok(Ok(())) => {
                        pending_copy = None;
                        retry_delay = 2;
                    }
                    _ => {
                        retry_at = tokio::time::Instant::now() + Duration::from_secs(retry_delay);
                        retry_delay = (retry_delay * 2).min(30);
                        known_revisions = None;
                    }
                }
            }
            if live_task.is_none()
                && tokio::time::Instant::now() >= retry_at
                && self.clipboard.sync_preferences().enabled
            {
                if let Some(copy) = pending_copy.clone() {
                    let clipboard = self.clipboard.clone();
                    let client = client.clone();
                    live_task = Some(AbortTaskOnDrop(tokio::spawn(async move {
                        let Some(mut record) = clipboard
                            .replica_record(&copy.sync_id)
                            .await
                            .map_err(|e| e.to_string())?
                        else {
                            return Ok(());
                        };
                        if (record.record.revision, &record.record.updated_by_device_id)
                            != (copy.revision, &copy.updated_by_device_id)
                        {
                            return Ok(());
                        }
                        record.record.captured_at_ms = copy.captured_at_ms;
                        record.record.live = true;
                        record.record.change_kind = ClipboardSyncChangeKind::Copy;
                        client.send_record(record, true).await?;
                        Ok(())
                    })));
                }
            }
        }
        if let Some(task) = live_task {
            task.0.abort();
        }
        if let Some(task) = reconciliation {
            task.0.abort();
        }
    }
}
