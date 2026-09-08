//! One application-owned runtime, retained until the desktop event loop exits.

pub fn build() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(worker_count())
        // File and platform APIs also need blocking workers. Apply tighter
        // admission to CPU-heavy content work, not to all blocking operations.
        .max_blocking_threads(32)
        .thread_name("arcrelay-async")
        .build()
}

fn worker_count() -> usize {
    std::thread::available_parallelism().map_or(2, |count| count.get().min(4))
}

pub fn configure_image_workers() -> Result<(), rayon::ThreadPoolBuildError> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .thread_name(|index| format!("arcrelay-image-{index}"))
        .build_global()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_runtime_survives_preparation_and_drives_background_work() {
        assert!(tokio::runtime::Handle::try_current().is_err());
        let runtime = build().unwrap();
        assert!(runtime.metrics().num_workers() <= 4);
        let (sender, receiver) = std::sync::mpsc::channel();
        runtime.block_on(async {
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                sender
                    .send(tokio::task::spawn_blocking(|| 42).await.unwrap())
                    .unwrap();
            });
        });
        // No temporary block_on drives the task while the caller is outside Tokio.
        assert_eq!(
            receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            42
        );
    }
}
