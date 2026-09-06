use serde::{Deserialize, Serialize};

// The event catalogue is also the persisted switch catalogue: adding a cue
// cannot accidentally share another event's preference.
macro_rules! catalogue {
    ($($event:ident => $field:ident, $default:expr, $tone:ident, $priority:expr;)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
        #[serde(rename_all = "camelCase")]
        pub enum SoundEvent { $($event,)* }

        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
        #[serde(default, rename_all = "camelCase")]
        pub struct SoundPreferences {
            pub enabled: bool,
            pub volume: u8,
            pub mute_during_privacy: bool,
            pub muted_automation_ids: Vec<String>,
            $(pub $field: bool,)*
        }
        impl Default for SoundPreferences {
            fn default() -> Self {
                Self { enabled: true, volume: 30, mute_during_privacy: true,
                    muted_automation_ids: Vec::new(), $($field: $default,)* }
            }
        }
        #[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
        #[serde(default, rename_all = "camelCase", deny_unknown_fields)]
        #[ts(optional_fields)]
        pub struct SoundPreferencesPatch {
            pub enabled: Option<bool>,
            pub volume: Option<u8>,
            pub mute_during_privacy: Option<bool>,
            pub muted_automation_ids: Option<Vec<String>>,
            $(pub $field: Option<bool>,)*
        }
        impl SoundPreferencesPatch {
            pub fn apply(self, mut current: SoundPreferences) -> SoundPreferences {
                if let Some(value) = self.enabled { current.enabled = value; }
                if let Some(value) = self.volume { current.volume = value; }
                if let Some(value) = self.mute_during_privacy { current.mute_during_privacy = value; }
                if let Some(value) = self.muted_automation_ids { current.muted_automation_ids = value; }
                $(if let Some(value) = self.$field { current.$field = value; })*
                current
            }
        }
        impl SoundEvent {
            pub const ALL: &'static [Self] = &[$(Self::$event,)*];
            pub fn enabled(self, prefs: &SoundPreferences) -> bool {
                match self { $(Self::$event => prefs.$field,)* }
            }
            pub(super) fn tone(self) -> super::tones::Tone {
                match self { $(Self::$event => super::tones::Tone::$tone,)* }
            }
            pub(super) fn priority(self) -> u8 {
                match self { $(Self::$event => $priority,)* }
            }
        }
    }
}

catalogue! {
    ClipboardAdded => clipboard_added, true, Tap, 0;
    ClipboardReceived => clipboard_received, false, Arrival, 0;
    ClipboardUsed => clipboard_used, false, Tap, 0;
    TransferRequest => transfer_request, true, Request, 3;
    TransferSent => transfer_sent, true, Success, 1;
    TransferReceived => transfer_received, true, Success, 1;
    TransferFailed => transfer_failed, true, Failure, 2;
    ActionStarted => action_started, false, Start, 0;
    ActionSucceeded => action_succeeded, true, Tap, 1;
    ActionFailed => action_failed, true, Failure, 2;
    AutomationStarted => automation_started, false, Start, 0;
    AutomationConfirmation => automation_confirmation, true, Request, 3;
    AutomationSucceeded => automation_succeeded, false, Success, 1;
    AutomationFailed => automation_failed, true, Failure, 2;
    AutomationInterrupted => automation_interrupted, true, Failure, 2;
}

impl SoundPreferences {
    pub fn validate(&self) -> Result<(), String> {
        if self.volume > 100 {
            return Err("sound volume must be between 0 and 100".into());
        }
        if self.muted_automation_ids.len() > 1000
            || self
                .muted_automation_ids
                .iter()
                .any(|id| id.is_empty() || id.len() > 128)
        {
            return Err("invalid muted automation IDs".into());
        }
        Ok(())
    }
}
