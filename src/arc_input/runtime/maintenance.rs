use super::*;
use std::future::Future;

/// One awaited operation per worker. A slow probe cannot accumulate detached
/// jobs or cause missed ticks to be replayed. Timers are created only at runtime.
async fn periodic<F, Fut>(period: Duration, mut operation: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let mut tick = tokio::time::interval(period);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        if !operation().await {
            return;
        }
    }
}

impl ArcInputRuntime {
    /// The caller owns the JoinSet so leaving the receiver loop cancels all
    /// workers. Native probes run in the blocking pool, with at most one probe
    /// outstanding per worker, and do not own runtime state while blocked.
    pub(super) fn start_maintenance(
        self: &Arc<Self>,
    ) -> (
        tokio::task::JoinSet<()>,
        tokio::sync::mpsc::Receiver<DisplayInventory>,
    ) {
        let mut workers = tokio::task::JoinSet::new();
        let (inventory_tx, inventory_rx) = tokio::sync::mpsc::channel(1);
        let weak = Arc::downgrade(self);
        workers.spawn(async move {
            let last_caps = Arc::new(Mutex::new(None));
            periodic(Duration::from_secs(2), move || {
                let weak = weak.clone();
                let last_caps = last_caps.clone();
                let inventory_tx = inventory_tx.clone();
                async move {
                    let Some(runtime) = weak.upgrade() else {
                        return false;
                    };
                    let platform = runtime.platform.clone();
                    let initial = lock(&last_caps).is_none();
                    let probe = tokio::task::spawn_blocking(move || {
                        let before =
                            initial.then(|| InputCapturePort::capabilities(platform.as_ref()));
                        platform.refresh_consumer_capabilities();
                        (before, InputCapturePort::capabilities(platform.as_ref()))
                    })
                    .await;
                    let (before, caps) = match probe {
                        Ok(caps) => caps,
                        Err(error) => {
                            runtime.record("consumer-capabilities", error.to_string(), None);
                            return true;
                        }
                    };
                    let changed = {
                        let mut previous = lock(&last_caps);
                        let changed = previous
                            .as_ref()
                            .or(before.as_ref())
                            .is_some_and(|old| old != &caps);
                        *previous = Some(caps);
                        changed
                    };
                    if changed {
                        runtime.platform.release_consumer_keys();
                        for peer in runtime.network.connected_peers() {
                            if let Err(error) = runtime.send_capabilities(&peer).await {
                                runtime.record("consumer-capabilities", error.to_string(), None);
                            }
                        }
                        runtime.refresh_consumer_capture();
                        let _ = runtime.events.send(RuntimeEvent::SnapshotChanged);
                    }
                    if let Err(error) = runtime.refresh_paired_peers().await {
                        runtime.record("pairing", error.to_string(), None);
                    }
                    let platform = runtime.platform.clone();
                    let probe = tokio::task::spawn_blocking(move || platform.inventory()).await;
                    match probe {
                        Ok(Ok(inventory)) => {
                            let inventory = collapse_mirrored_inventory(inventory);
                            let changed = read(&runtime.local_inventory)
                                .as_ref()
                                .is_none_or(|current| !inventory_equivalent(current, &inventory));
                            // Only state changes return to the ordered receiver. Native
                            // probes and pairing reads never execute on that task.
                            if changed && inventory_tx.send(inventory).await.is_err() {
                                return false;
                            }
                        }
                        Ok(Err(error)) => runtime.record("display", error.to_string(), None),
                        Err(error) => runtime.record("display", error.to_string(), None),
                    }
                    true
                }
            })
            .await;
        });
        let weak = Arc::downgrade(self);
        workers.spawn(async move {
            periodic(Duration::from_millis(500), move || {
                let weak = weak.clone();
                async move {
                    let Some(runtime) = weak.upgrade() else { return false; };
                    let platform = runtime.platform.clone();
                    let probe = tokio::task::spawn_blocking(move || {
                        (platform.maintain_cursor_visibility(), platform.maintain_system_gesture())
                    }).await;
                    match probe {
                        Ok((cursor, gesture)) => {
                            if let Err(error) = cursor {
                                runtime.record("cursor", error.to_string(), None);
                                if let Err(error) = runtime.release_control() {
                                    tracing::error!(%error, "Arc Input failed to release after cursor visibility error");
                                }
                            }
                            match gesture {
                                Ok(true) => runtime.record("system-gesture", "cancelled after 2s without a gesture update".into(), None),
                                Err(error) => runtime.record("system-gesture", error.to_string(), None),
                                Ok(false) => {}
                            }
                        }
                        Err(error) => runtime.record("maintenance", error.to_string(), None),
                    }
                    true
                }
            }).await;
        });
        (workers, inventory_rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn stalled_maintenance_does_not_block_input_or_accumulate_jobs() {
        let calls = Arc::new(AtomicU64::new(0));
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let mut workers = tokio::task::JoinSet::new();
        let seen = calls.clone();
        let wait = gate.clone();
        workers.spawn(periodic(Duration::from_millis(500), move || {
            seen.fetch_add(1, Ordering::Relaxed);
            let wait = wait.clone();
            async move {
                wait.acquire().await.unwrap().forget();
                true
            }
        }));
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        tokio::time::advance(Duration::from_secs(60)).await;
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(1);
        input_tx.send(42).await.unwrap();
        assert_eq!(input_rx.recv().await, Some(42));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        gate.add_permits(1);
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        drop(workers);
        tokio::task::yield_now().await;
        gate.add_permits(10);
        tokio::time::advance(Duration::from_secs(60)).await;
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }
}
