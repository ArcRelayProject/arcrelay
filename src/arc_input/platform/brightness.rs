//! Slow display control never runs on the keyboard/mouse input thread.
use arcrelay_input::{DisplayId, PlatformError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
#[path = "macos/brightness.rs"]
mod native;
#[cfg(target_os = "windows")]
#[path = "windows/brightness.rs"]
mod native;

#[derive(Default)]
struct State {
    pending: Option<(String, i32, u64)>,
    epoch: u64,
    refresh: bool,
    stop: bool,
    supported: Vec<String>,
    last_probe: Option<Instant>,
}
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}
#[derive(Default)]
pub(super) struct BrightnessWorker {
    shared: Arc<Shared>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}
fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|e| e.into_inner())
}
impl BrightnessWorker {
    pub fn supported(&self) -> Vec<String> {
        lock(&self.shared.state).supported.clone()
    }
    pub fn refresh(&self) {
        let mut thread = lock(&self.thread);
        if thread.is_none() {
            let shared = self.shared.clone();
            match std::thread::Builder::new()
                .name("arc-input-brightness".into())
                .spawn(move || run(shared))
            {
                Ok(handle) => *thread = Some(handle),
                Err(error) => {
                    tracing::warn!(%error, "Cannot start brightness worker");
                    return;
                }
            }
        }
        let mut state = lock(&self.shared.state);
        if state
            .last_probe
            .is_none_or(|last| last.elapsed() >= Duration::from_secs(30))
        {
            state.refresh = true;
            self.shared.wake.notify_one();
        }
    }
    pub fn adjust(&self, display: &DisplayId, up: bool) -> Result<(), PlatformError> {
        let mut state = lock(&self.shared.state);
        if !state.supported.iter().any(|id| id == display.as_str()) {
            return Err(PlatformError::Unsupported(
                "selected display has no brightness control".into(),
            ));
        }
        let step = if up { 1 } else { -1 };
        let epoch = state.epoch;
        match state.pending.as_mut() {
            Some((id, steps, generation)) if id == display.as_str() && *generation == epoch => {
                *steps = (*steps + step).clamp(-20, 20)
            }
            _ => state.pending = Some((display.to_string(), step, epoch)),
        }
        self.shared.wake.notify_one();
        Ok(())
    }
    pub fn cancel(&self) {
        let mut state = lock(&self.shared.state);
        state.epoch = state.epoch.wrapping_add(1);
        state.pending = None;
    }
}
impl Drop for BrightnessWorker {
    fn drop(&mut self) {
        let mut state = lock(&self.shared.state);
        state.stop = true;
        state.pending = None;
        self.shared.wake.notify_one();
        // A display driver may be slow. Do not block input/runtime shutdown on
        // an in-flight OS call; the detached worker exits immediately after it.
    }
}
fn run(shared: Arc<Shared>) {
    loop {
        let mut state = lock(&shared.state);
        while !state.stop && state.pending.is_none() && !state.refresh {
            state = shared.wake.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        if state.stop {
            return;
        }
        if let Some((target_display, steps, epoch)) = state.pending.take() {
            drop(state);
            if steps != 0 {
                // Check again after queueing/probing. One already-started OS
                // write cannot be undone, but no queued repeat survives release.
                let state = lock(&shared.state);
                if state.stop || state.epoch != epoch {
                    continue;
                }
                drop(state);
                if let Err(error) = native::adjust(&target_display, steps) {
                    tracing::warn!(event = "input.consumer.brightness_failed", display_id = target_display.as_str(), %error, "Brightness adjustment failed");
                    lock(&shared.state)
                        .supported
                        .retain(|id| id != &target_display);
                }
            }
        } else {
            state.refresh = false;
            state.last_probe = Some(Instant::now());
            drop(state);
            let supported = native::probe();
            lock(&shared.state).supported = supported;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constructor_is_lazy_and_release_discards_pending_work_without_tokio() {
        let worker = BrightnessWorker::default();
        assert!(lock(&worker.thread).is_none());
        lock(&worker.shared.state).supported.push("test".into());
        let display = DisplayId::parse("test").unwrap();
        for _ in 0..100 {
            worker.adjust(&display, true).unwrap();
        }
        assert_eq!(lock(&worker.shared.state).pending.as_ref().unwrap().1, 20);
        worker.cancel();
        assert!(lock(&worker.shared.state).pending.is_none());
        assert!(worker
            .adjust(&DisplayId::parse("other").unwrap(), true)
            .is_err());
    }
}
