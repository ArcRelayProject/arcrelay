use std::{sync::Arc, time::Duration};
use tokio::sync::Notify;

/// No timer exists until a native sample arrives. Keep this future across
/// select iterations so keyboard/other motion events cannot cancel its wakeup.
pub(super) async fn next(notify: Arc<Notify>, cadence: Duration) {
    notify.notified().await;
    tokio::time::sleep(cadence).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn idle_motion_has_no_periodic_wakeup_and_bursts_are_coalesced() {
        let notify = Arc::new(Notify::new());
        let waiting = tokio::spawn(next(notify.clone(), Duration::from_millis(4)));
        tokio::time::advance(Duration::from_secs(300)).await;
        assert!(!waiting.is_finished());
        for _ in 0..100 {
            notify.notify_one();
        }
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(3)).await;
        assert!(!waiting.is_finished());
        tokio::time::advance(Duration::from_millis(1)).await;
        waiting.await.unwrap();
        let idle = tokio::spawn(next(notify, Duration::from_millis(4)));
        tokio::time::advance(Duration::from_secs(300)).await;
        assert!(!idle.is_finished());
        idle.abort();
    }
}
