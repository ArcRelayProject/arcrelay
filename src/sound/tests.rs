use super::*;
use arcrelay_automation::ActivityStatus;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

struct FakeAudio {
    plays: mpsc::Sender<SoundEvent>,
    probes: Arc<AtomicUsize>,
    fail: bool,
}
impl AudioBackend for FakeAudio {
    fn play(&mut self, event: SoundEvent, _: u8) -> Result<(), String> {
        self.plays.send(event).unwrap();
        if self.fail {
            Err("no audio device".into())
        } else {
            Ok(())
        }
    }
    fn busy(&self) -> bool {
        self.probes.fetch_add(1, Ordering::Relaxed);
        false
    }
    fn stop(&mut self) {}
    fn release(&mut self) {}
}
fn service() -> SoundService {
    SoundService::new(SoundPreferences::default(), Arc::new(|| false))
}

#[test]
fn construction_and_idle_worker_need_no_tokio_or_audio_device() {
    assert!(tokio::runtime::Handle::try_current().is_err());
    let service = service();
    assert!(service.worker.lock().unwrap().is_none());
    let (tx, rx) = mpsc::channel();
    let probes = Arc::new(AtomicUsize::new(0));
    let count = probes.clone();
    service
        .start_with(move || FakeAudio {
            plays: tx,
            probes: count,
            fail: false,
        })
        .unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(80)).is_err());
    let initial = probes.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(
        probes.load(Ordering::Relaxed),
        initial,
        "idle worker must not poll"
    );
    service.stop();
}

#[test]
fn preferences_are_independent_and_legacy_settings_remain_valid() {
    let settings: crate::settings::AppSettings =
        serde_json::from_str(r#"{"notifications":{"enabled":false}}"#).unwrap();
    assert!(settings.sounds.enabled);
    assert!(settings.sounds.clipboard_added);
    assert!(!settings.sounds.automation_succeeded);
    let patch: crate::settings::AppSettingsPatch =
        serde_json::from_str(r#"{"sounds":{"transferSent":false,"volume":15}}"#).unwrap();
    let next = patch.apply(settings);
    assert!(!next.sounds.transfer_sent);
    assert!(next.sounds.transfer_received);
    assert!(next.sounds.transfer_request);
    assert!(!next.notifications.enabled);
    assert_eq!(next.sounds.volume, 15);
    let invalid = SoundPreferences {
        volume: 101,
        ..Default::default()
    };
    assert!(invalid.validate().is_err());
    let encoded = serde_json::to_value(SoundPreferences::default()).unwrap();
    for event in SoundEvent::ALL {
        let key = serde_json::to_value(event)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        assert!(encoded[&key].is_boolean());
    }
}

#[test]
fn attention_sounds_require_visible_feedback() {
    for event in [
        SoundEvent::TransferRequest,
        SoundEvent::TransferFailed,
        SoundEvent::ActionFailed,
        SoundEvent::AutomationConfirmation,
        SoundEvent::AutomationFailed,
        SoundEvent::AutomationInterrupted,
    ] {
        assert!(event.requires_visible_feedback(), "{event:?}");
    }
    for event in [
        SoundEvent::ClipboardAdded,
        SoundEvent::ClipboardReceived,
        SoundEvent::ClipboardUsed,
        SoundEvent::TransferSent,
        SoundEvent::TransferReceived,
        SoundEvent::ActionSucceeded,
        SoundEvent::AutomationSucceeded,
    ] {
        assert!(!event.requires_visible_feedback(), "{event:?}");
    }
}

#[test]
fn master_mute_privacy_and_automation_override_are_applied_before_queueing() {
    let service = service();
    service.mute(Some(Duration::from_secs(1)));
    assert!(service
        .enqueue(SoundEvent::TransferRequest, None, None)
        .is_err());
    service.mute(None);
    service.configure(SoundPreferences {
        muted_automation_ids: vec!["quiet".into()],
        ..Default::default()
    });
    assert!(service
        .enqueue(SoundEvent::AutomationFailed, Some("quiet"), None)
        .is_err());
    assert!(service
        .enqueue(SoundEvent::AutomationFailed, Some("other"), None)
        .is_ok());
    service.configure(SoundPreferences {
        enabled: false,
        ..Default::default()
    });
    assert!(service
        .enqueue(
            SoundEvent::TransferRequest,
            None,
            Some(oneshot::channel().0)
        )
        .is_err());
    assert!(service
        .shared
        .state
        .lock()
        .unwrap()
        .pending
        .iter()
        .all(Option::is_none));
    let private = SoundService::new(SoundPreferences::default(), Arc::new(|| true));
    assert!(private
        .enqueue(SoundEvent::TransferFailed, None, None)
        .is_err());
}

#[test]
fn burst_is_bounded_and_request_wins_over_clipboard_and_success() {
    let service = service();
    for _ in 0..1000 {
        service
            .enqueue(SoundEvent::ClipboardAdded, None, None)
            .unwrap();
    }
    service
        .enqueue(SoundEvent::TransferSent, None, None)
        .unwrap();
    service
        .enqueue(SoundEvent::TransferRequest, None, None)
        .unwrap();
    assert_eq!(
        service
            .shared
            .state
            .lock()
            .unwrap()
            .pending
            .iter()
            .flatten()
            .count(),
        3
    );
    let (tx, rx) = mpsc::channel();
    service
        .start_with(move || FakeAudio {
            plays: tx,
            probes: Arc::new(AtomicUsize::new(0)),
            fail: false,
        })
        .unwrap();
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        SoundEvent::TransferRequest
    );
    assert!(rx.recv_timeout(Duration::from_millis(80)).is_err());
    service.stop();
}

#[test]
fn expired_cues_are_dropped_and_missing_device_uses_backoff() {
    let service = service();
    service
        .enqueue(SoundEvent::ClipboardAdded, None, None)
        .unwrap();
    service.shared.state.lock().unwrap().pending[0]
        .as_mut()
        .unwrap()
        .created -= Duration::from_secs(2);
    let (tx, rx) = mpsc::channel();
    service
        .start_with(move || FakeAudio {
            plays: tx,
            probes: Arc::new(AtomicUsize::new(0)),
            fail: true,
        })
        .unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(80)).is_err());
    let (reply, result) = oneshot::channel();
    service
        .enqueue(SoundEvent::TransferRequest, None, Some(reply))
        .unwrap();
    assert!(result.blocking_recv().unwrap().is_err());
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        SoundEvent::TransferRequest
    );
    service
        .enqueue(SoundEvent::TransferFailed, None, None)
        .unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(80)).is_err());
    service.stop();
}

#[test]
fn automation_steps_confirmation_and_terminal_events_do_not_repeat() {
    let mut tracker = AutomationSoundTracker::default();
    assert_eq!(
        tracker.transition("run", ActivityStatus::Queued, None),
        None
    );
    assert_eq!(
        tracker.transition("run", ActivityStatus::Running, None),
        Some(SoundEvent::AutomationStarted)
    );
    assert_eq!(
        tracker.transition("run", ActivityStatus::Running, None),
        None
    );
    assert_eq!(
        tracker.transition("run", ActivityStatus::AwaitingConfirmation, Some(1)),
        Some(SoundEvent::AutomationConfirmation)
    );
    assert_eq!(
        tracker.transition("run", ActivityStatus::AwaitingConfirmation, Some(1)),
        None
    );
    assert_eq!(
        tracker.transition("run", ActivityStatus::Running, None),
        None
    );
    assert_eq!(
        tracker.transition("run", ActivityStatus::AwaitingConfirmation, Some(3)),
        Some(SoundEvent::AutomationConfirmation)
    );
    assert_eq!(
        tracker.transition("run", ActivityStatus::Succeeded, None),
        Some(SoundEvent::AutomationSucceeded)
    );
    assert_eq!(
        tracker.transition("run", ActivityStatus::Succeeded, None),
        None
    );
    assert_eq!(
        tracker.transition("cancel", ActivityStatus::Canceled, None),
        None
    );
    assert_eq!(
        tracker.transition("skip", ActivityStatus::Skipped, None),
        None
    );
}

#[test]
fn cues_are_small_finite_and_have_smooth_edges() {
    let bank = tones::bank();
    assert!(bank.iter().map(|samples| samples.len() * 4).sum::<usize>() < 256_000);
    for samples in bank {
        assert!(samples.iter().all(|v| v.is_finite() && v.abs() < 0.6));
        assert!(samples.first().unwrap().abs() < 0.001);
        assert!(samples.last().unwrap().abs() < 0.001);
        assert!(samples.iter().any(|v| v.abs() > 0.05));
    }
}

#[test]
fn passive_arrivals_and_confirmation_requests_have_distinct_cues() {
    let arrival = SoundEvent::ClipboardReceived.tone();
    let request = SoundEvent::TransferRequest.tone();
    assert_ne!(arrival.index(), request.index());
    assert_eq!(
        request.index(),
        SoundEvent::AutomationConfirmation.tone().index()
    );
    // Passive arrivals are one brief strike; requests leave room for two.
    assert!(arrival.samples().len() < (tones::SAMPLE_RATE / 5) as usize);
    assert!(request.samples().len() > (tones::SAMPLE_RATE / 4) as usize);
}

#[test]
fn embedded_pcm_has_complete_samples_and_matches_bank_indices() {
    let bank = tones::bank();
    for tone in [
        tones::Tone::Tap,
        tones::Tone::Arrival,
        tones::Tone::Start,
        tones::Tone::Success,
        tones::Tone::Failure,
        tones::Tone::Request,
    ] {
        assert_eq!(tone.pcm().len() % 2, 0);
        assert_eq!(bank[tone.index()], tone.samples());
        assert!(tone.samples().len() <= (tones::SAMPLE_RATE / 2) as usize);
    }
}

#[test]
#[ignore = "requires a real audio output and plays one short cue"]
fn native_output_smoke() {
    let mut audio = NativeAudio::default();
    audio.play(SoundEvent::ClipboardAdded, 30).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while audio.busy() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!audio.busy());
    audio.release();
    assert!(audio.output.is_none());
}

#[test]
fn sound_preferences_survive_restart_without_resetting_other_settings() {
    let dir = tempfile::tempdir().unwrap();
    let manager = crate::settings::SettingsManager::new(dir.path()).unwrap();
    let mut settings = manager.snapshot();
    settings.notifications.enabled = false;
    settings.sounds.enabled = false;
    settings.sounds.clipboard_added = false;
    settings.sounds.automation_succeeded = true;
    settings.sounds.muted_automation_ids = vec!["frequent-run".into()];
    let saved = manager.save(settings).unwrap();
    let reopened = crate::settings::SettingsManager::new(dir.path()).unwrap();
    assert_eq!(reopened.snapshot().sounds, saved.sounds);
    assert!(!reopened.snapshot().notifications.enabled);
}

#[test]
fn stalled_device_is_released_then_worker_sleeps() {
    struct StalledAudio(mpsc::Sender<&'static str>);
    impl AudioBackend for StalledAudio {
        fn play(&mut self, _: SoundEvent, _: u8) -> Result<(), String> {
            self.0.send("play").unwrap();
            Ok(())
        }
        fn busy(&self) -> bool {
            true
        }
        fn stop(&mut self) {}
        fn release(&mut self) {
            self.0.send("release").unwrap();
        }
    }
    let service = service();
    let (tx, rx) = mpsc::channel();
    service.start_with(move || StalledAudio(tx)).unwrap();
    service
        .enqueue(SoundEvent::ClipboardAdded, None, None)
        .unwrap();
    assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), "play");
    assert_eq!(
        rx.recv_timeout(DEVICE_IDLE + Duration::from_secs(1))
            .unwrap(),
        "release"
    );
    assert!(rx.recv_timeout(Duration::from_millis(80)).is_err());
    service.stop();
}

#[test]
fn recovery_is_silent_but_resuming_an_old_confirmation_can_play() {
    use arcrelay_automation::{
        AutomationActivity, AutomationDefinition, AutomationEvent, AutomationTrigger, RunMode,
    };
    let old = chrono::Utc::now() - chrono::Duration::hours(1);
    let definition = AutomationDefinition {
        id: "automation".into(),
        name: "Example".into(),
        enabled: true,
        trigger: AutomationTrigger::Manual,
        conditions: vec![],
        steps: vec![],
        run_mode: RunMode::Automatic,
        revision: 1,
        created_at: old,
        updated_at: old,
        legacy_id: None,
    };
    let mut activity = AutomationActivity {
        id: "old".into(),
        automation_id: definition.id.clone(),
        definition,
        event: AutomationEvent::new("manual"),
        status: ActivityStatus::Interrupted,
        reason: None,
        created_at: old,
        finished_at: Some(chrono::Utc::now()),
        steps: vec![],
        confirmed_steps: vec![],
        run_confirmed: false,
    };
    let mut tracker = AutomationSoundTracker::default();
    assert_eq!(tracker.update(&activity), None);
    activity.id = "resume".into();
    activity.status = ActivityStatus::Queued;
    activity.finished_at = None;
    assert_eq!(tracker.update(&activity), None);
    activity.status = ActivityStatus::Running;
    assert_eq!(
        tracker.update(&activity),
        Some(SoundEvent::AutomationStarted)
    );
    activity.status = ActivityStatus::Succeeded;
    assert_eq!(
        tracker.update(&activity),
        Some(SoundEvent::AutomationSucceeded)
    );
}
