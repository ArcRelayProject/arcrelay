//! Process-local feedback. One bounded mailbox, one sleeping worker, no device
//! enumeration/polling at startup or while idle. Business operations never wait
//! for audio and failed playback never changes their result.
mod preferences;
#[cfg(test)]
mod tests;
mod tones;
mod tracking;

pub use preferences::{SoundEvent, SoundPreferences, SoundPreferencesPatch};
pub(crate) use tracking::AutomationSoundTracker;

use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

const CUE_TTL: Duration = Duration::from_millis(500);
const COOLDOWN: Duration = Duration::from_millis(300);
const DEVICE_IDLE: Duration = Duration::from_secs(1);
const DEVICE_RETRY: Duration = Duration::from_secs(30);
static SERVICE: OnceLock<SoundService> = OnceLock::new();

struct Cue {
    event: SoundEvent,
    created: Instant,
    preview: Option<oneshot::Sender<Result<(), String>>>,
}
impl Cue {
    fn finish(self, result: Result<(), String>) {
        if let Some(reply) = self.preview {
            let _ = reply.send(result);
        }
    }
}

struct State {
    preferences: SoundPreferences,
    // At most one pending cue per priority. No unbounded queue or history.
    pending: [Option<Cue>; 4],
    last: [Option<Instant>; SoundEvent::ALL.len()],
    muted_until: Option<(Instant, i64)>,
    generation: u64,
    stopped: bool,
}
impl State {
    fn allowed(&self, event: SoundEvent, preview: bool, privacy: bool, now: Instant) -> bool {
        !self.stopped
            && self.preferences.enabled
            && self.preferences.volume > 0
            && (preview || event.enabled(&self.preferences))
            && !(self.preferences.mute_during_privacy && privacy)
            && !self.muted_until.is_some_and(|(until, _)| now < until)
    }
    fn clear(&mut self) {
        for slot in &mut self.pending {
            if let Some(cue) = slot.take() {
                cue.finish(Err("sound was superseded or muted".into()));
            }
        }
    }
}
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    privacy_active: Arc<dyn Fn() -> bool + Send + Sync>,
}
pub struct SoundService {
    shared: Arc<Shared>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl SoundService {
    fn new(
        preferences: SoundPreferences,
        privacy_active: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    preferences,
                    pending: std::array::from_fn(|_| None),
                    last: [None; SoundEvent::ALL.len()],
                    muted_until: None,
                    generation: 0,
                    stopped: false,
                }),
                wake: Condvar::new(),
                privacy_active,
            }),
            worker: Mutex::new(None),
        }
    }
    fn start(&self) -> Result<(), String> {
        self.start_with(NativeAudio::default)
    }
    fn start_with<B: AudioBackend + 'static>(
        &self,
        create: impl FnOnce() -> B + Send + 'static,
    ) -> Result<(), String> {
        let mut handle = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        if handle.is_some() {
            return Ok(());
        }
        let shared = self.shared.clone();
        *handle = Some(
            std::thread::Builder::new()
                .name("arcrelay-sound".into())
                .stack_size(256 * 1024)
                .spawn(move || worker(shared, create()))
                .map_err(|e| e.to_string())?,
        );
        Ok(())
    }
    fn enqueue(
        &self,
        event: SoundEvent,
        automation_id: Option<&str>,
        preview: Option<oneshot::Sender<Result<(), String>>>,
    ) -> Result<(), String> {
        let now = Instant::now();
        let privacy = (self.shared.privacy_active)();
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.allowed(event, preview.is_some(), privacy, now)
            || automation_id.is_some_and(|id| {
                state
                    .preferences
                    .muted_automation_ids
                    .iter()
                    .any(|muted| muted == id)
            })
        {
            return Err(
                "sound is disabled, temporarily muted, or silenced by screen privacy".into(),
            );
        }
        let index = event as usize;
        if preview.is_none()
            && state.last[index].is_some_and(|last| now.duration_since(last) < COOLDOWN)
        {
            return Ok(());
        }
        state.last[index] = Some(now);
        let slot = &mut state.pending[event.priority() as usize];
        if let Some(old) = slot.replace(Cue {
            event,
            created: now,
            preview,
        }) {
            old.finish(Err("sound was superseded".into()));
        }
        self.shared.wake.notify_one();
        Ok(())
    }
    fn configure(&self, preferences: SoundPreferences) {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.preferences = preferences;
        state.generation = state.generation.wrapping_add(1);
        state.clear();
        self.shared.wake.notify_one();
    }
    fn mute(&self, duration: Option<Duration>) {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.muted_until = duration.map(|duration| {
            (
                Instant::now() + duration,
                chrono::Utc::now().timestamp_millis() + duration.as_millis() as i64,
            )
        });
        state.generation = state.generation.wrapping_add(1);
        state.clear();
        self.shared.wake.notify_one();
    }
    fn stop(&self) {
        {
            let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            state.stopped = true;
            state.clear();
            self.shared.wake.notify_one();
        }
        if let Some(handle) = self.worker.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = handle.join();
        }
    }
}
impl Drop for SoundService {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn initialize(preferences: SoundPreferences, privacy: Arc<crate::privacy::PrivacyManager>) {
    let service = SoundService::new(preferences, Arc::new(move || privacy.sound_is_private()));
    if let Err(error) = service.start() {
        tracing::warn!(%error, "sound worker unavailable");
        return;
    }
    let _ = SERVICE.set(service);
}
pub fn play(event: SoundEvent) {
    if let Some(service) = SERVICE.get() {
        let _ = service.enqueue(event, None, None);
    }
}
pub fn play_automation(event: SoundEvent, id: &str) {
    if let Some(service) = SERVICE.get() {
        let _ = service.enqueue(event, Some(id), None);
    }
}
pub fn configure(preferences: SoundPreferences) {
    if let Some(service) = SERVICE.get() {
        service.configure(preferences);
    }
}
pub fn shutdown() {
    if let Some(service) = SERVICE.get() {
        service.stop();
    }
}
pub fn set_temporary_mute(muted: bool) {
    if let Some(service) = SERVICE.get() {
        service.mute(muted.then_some(Duration::from_secs(3600)));
    }
}
pub fn temporary_mute_until() -> Option<i64> {
    SERVICE.get().and_then(|service| {
        service
            .shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .muted_until
            .filter(|(until, _)| Instant::now() < *until)
            .map(|(_, timestamp)| timestamp)
    })
}
pub async fn preview(event: SoundEvent) -> Result<(), String> {
    let service = SERVICE.get().ok_or("sound service is not ready")?;
    let (tx, rx) = oneshot::channel();
    service.enqueue(event, None, Some(tx))?;
    tokio::time::timeout(Duration::from_secs(3), rx)
        .await
        .map_err(|_| "audio device did not respond".to_string())?
        .map_err(|_| "sound worker stopped".to_string())?
}

trait AudioBackend {
    fn play(&mut self, event: SoundEvent, volume: u8) -> Result<(), String>;
    fn busy(&self) -> bool;
    fn failed(&self) -> bool {
        false
    }
    fn stop(&mut self);
    fn release(&mut self);
}

#[derive(Default)]
struct NativeAudio {
    output: Option<rodio::MixerDeviceSink>,
    player: Option<rodio::Player>,
    samples: Option<[Vec<f32>; tones::TONE_COUNT]>,
    failed: Arc<std::sync::atomic::AtomicBool>,
}
impl AudioBackend for NativeAudio {
    fn play(&mut self, event: SoundEvent, volume: u8) -> Result<(), String> {
        if self.output.is_none() {
            self.failed
                .store(false, std::sync::atomic::Ordering::Relaxed);
            let failed = self.failed.clone();
            let mut output = rodio::DeviceSinkBuilder::from_default_device()
                .map_err(|e| e.to_string())?
                .with_error_callback(move |_error| {
                    failed.store(true, std::sync::atomic::Ordering::Relaxed);
                })
                .open_sink_or_fallback()
                .map_err(|e| e.to_string())?;
            output.log_on_drop(false);
            self.output = Some(output);
        }
        let samples = self.samples.get_or_insert_with(tones::bank);
        let player = rodio::Player::connect_new(self.output.as_ref().unwrap().mixer());
        player.set_volume(f32::from(volume) / 100.0);
        player.append(rodio::buffer::SamplesBuffer::new(
            1.try_into().unwrap(),
            tones::SAMPLE_RATE.try_into().unwrap(),
            samples[event.tone().index()].clone(),
        ));
        self.player = Some(player);
        Ok(())
    }
    fn busy(&self) -> bool {
        self.player.as_ref().is_some_and(|player| !player.empty())
    }
    fn failed(&self) -> bool {
        self.failed.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn stop(&mut self) {
        self.player = None;
    }
    fn release(&mut self) {
        self.stop();
        self.output = None;
        self.failed
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

fn worker(shared: Arc<Shared>, mut audio: impl AudioBackend) {
    let mut generation = 0;
    let mut active_priority = 0;
    let mut device_deadline: Option<Instant> = None;
    let mut retry_after: Option<Instant> = None;
    loop {
        let privacy = (shared.privacy_active)();
        let (cue, allowed, volume, release) = {
            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.stopped {
                break;
            }
            let release = generation != state.generation
                || (state.preferences.mute_during_privacy && privacy);
            generation = state.generation;
            let cue = state.pending.iter_mut().rev().find_map(Option::take);
            let allowed = cue.as_ref().is_some_and(|cue| {
                state.allowed(cue.event, cue.preview.is_some(), privacy, Instant::now())
            });
            if cue.is_some() {
                state.clear();
            }
            (cue, allowed, state.preferences.volume, release)
        };
        // No audio API (including stream destruction) runs under the mailbox
        // mutex. Event producers cannot be blocked by a slow device driver.
        if audio.failed() {
            audio.release();
            device_deadline = None;
            retry_after = Some(Instant::now() + DEVICE_RETRY);
        }
        if release {
            audio.release();
            device_deadline = None;
        }
        if let Some(cue) = cue {
            let now = Instant::now();
            if !allowed || now.duration_since(cue.created) > CUE_TTL {
                cue.finish(Err("sound expired or was muted".into()));
                continue;
            }
            if audio.busy() && cue.preview.is_none() && cue.event.priority() < active_priority {
                cue.finish(Ok(()));
                continue;
            }
            if retry_after.is_some_and(|until| now < until) && cue.preview.is_none() {
                continue;
            }
            audio.stop();
            active_priority = cue.event.priority();
            match audio.play(cue.event, volume) {
                Ok(()) => {
                    retry_after = None;
                    device_deadline = Some(Instant::now() + DEVICE_IDLE);
                    cue.finish(Ok(()));
                }
                Err(error) => {
                    tracing::warn!(%error, "audio output unavailable; business operation unaffected");
                    audio.release();
                    device_deadline = None;
                    retry_after = Some(now + DEVICE_RETRY);
                    cue.finish(Err(format!("audio output unavailable: {error}")));
                }
            }
            continue;
        }
        let now = Instant::now();
        let timeout = if let Some(deadline) = device_deadline {
            // Also bounds streams whose device stalls without an error event.
            if now >= deadline {
                audio.release();
                device_deadline = None;
                None
            } else if audio.busy() {
                Some(Duration::from_millis(50).min(deadline.duration_since(now)))
            } else {
                Some(deadline.duration_since(now))
            }
        } else {
            None
        };
        let state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        // Recheck after device work so a concurrent producer/settings update
        // cannot be lost between releasing the mutex and entering the wait.
        if state.stopped
            || state.generation != generation
            || state.pending.iter().any(Option::is_some)
        {
            continue;
        }
        if let Some(timeout) = timeout {
            drop(
                shared
                    .wake
                    .wait_timeout(state, timeout)
                    .unwrap_or_else(|e| e.into_inner()),
            );
        } else {
            drop(shared.wake.wait(state).unwrap_or_else(|e| e.into_inner()));
        }
    }
    audio.release();
}
