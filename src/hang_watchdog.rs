#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(target_os = "macos")]
use tauri::AppHandle;

#[cfg(target_os = "macos")]
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
#[cfg(target_os = "macos")]
const HANG_THRESHOLD: Duration = Duration::from_secs(5);
#[cfg(target_os = "macos")]
const CAPTURE_COOLDOWN: Duration = Duration::from_secs(60);

#[cfg(target_os = "macos")]
pub fn start(app: AppHandle) {
    let heartbeat_ms = Arc::new(AtomicU64::new(now_ms()));
    let heartbeat_pending = Arc::new(AtomicBool::new(false));
    let _ = std::thread::Builder::new()
        .name("arcrelay-main-watchdog".into())
        .spawn(move || {
            let mut last_capture_ms = 0_u64;
            loop {
                if !heartbeat_pending.swap(true, Ordering::AcqRel) {
                    let heartbeat_ms = Arc::clone(&heartbeat_ms);
                    let callback_pending = Arc::clone(&heartbeat_pending);
                    if let Err(error) = app.run_on_main_thread(move || {
                        heartbeat_ms.store(now_ms(), Ordering::Release);
                        callback_pending.store(false, Ordering::Release);
                    }) {
                        heartbeat_pending.store(false, Ordering::Release);
                        tracing::warn!(%error, "failed to schedule main-thread watchdog heartbeat");
                    }
                }

                std::thread::sleep(HEARTBEAT_INTERVAL);
                let now = now_ms();
                let stalled_ms = now.saturating_sub(heartbeat_ms.load(Ordering::Acquire));
                if stalled_ms < HANG_THRESHOLD.as_millis() as u64
                    || now.saturating_sub(last_capture_ms) < CAPTURE_COOLDOWN.as_millis() as u64
                {
                    continue;
                }
                last_capture_ms = now;
                capture_sample(stalled_ms);
            }
        });
}

#[cfg(target_os = "macos")]
fn capture_sample(stalled_ms: u64) {
    let directory = match crate::observability::ensure_diagnostics_directory() {
        Ok(directory) => directory,
        Err(error) => {
            tracing::warn!(
                event = "app.hang_sample_directory_failed",
                %error,
                "Failed to create hang diagnostics directory"
            );
            return;
        }
    };
    let path = directory.join(format!(
        "main-thread-hang-{}-{}.sample.txt",
        std::process::id(),
        now_ms()
    ));
    tracing::error!(
        event = "app.main_thread_hang",
        stalled_ms,
        sample_file = %path.file_name().unwrap_or_default().to_string_lossy(),
        "AppKit main thread stopped responding; capturing sample"
    );
    let mut command = std::process::Command::new("/usr/bin/sample");
    command
        .arg(std::process::id().to_string())
        .arg("5")
        .arg("-file")
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    match command.spawn() {
        Ok(mut child) => {
            let deadline = std::time::Instant::now() + Duration::from_secs(12);
            loop {
                match child.try_wait() {
                    Ok(Some(status)) if status.success() => break,
                    Ok(Some(status)) => {
                        tracing::warn!(%status, "macOS sample command failed");
                        break;
                    }
                    Ok(None) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Ok(None) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        tracing::warn!("macOS sample command timed out");
                        break;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "Failed to wait for macOS sample command");
                        break;
                    }
                }
            }
        }
        Err(error) => tracing::warn!(%error, "failed to start macOS sample command"),
    }
}

#[cfg(target_os = "macos")]
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(not(target_os = "macos"))]
pub fn start(_app: tauri::AppHandle) {}
