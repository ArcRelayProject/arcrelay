use std::sync::atomic::{AtomicU8, Ordering};

use arcrelay_core::domain::clipboard::ClipboardSyncPreferences;
use arcrelay_gaze::PresenceState;
use tauri::AppHandle;

use crate::backend::DesktopState;

const NOT_ENROLLED: u8 = 0;
const OWNER_CONFIRMED: u8 = 1;
const OWNER_UNCONFIRMED: u8 = 2;

/// Process-local authorization view derived from the biometric tracker.
///
/// It deliberately stores no embedding. Consumers only learn whether a fresh,
/// stabilized observation confirms the enrolled owner.
pub struct PresenceAccessController {
    state: AtomicU8,
}

impl Default for PresenceAccessController {
    fn default() -> Self {
        Self {
            state: AtomicU8::new(NOT_ENROLLED),
        }
    }
}

impl PresenceAccessController {
    pub fn update(&self, enrolled: bool, state: PresenceState) -> bool {
        let next = if !enrolled {
            NOT_ENROLLED
        } else if state == PresenceState::OwnerPresent {
            OWNER_CONFIRMED
        } else {
            OWNER_UNCONFIRMED
        };
        self.state.swap(next, Ordering::AcqRel) != next
    }

    pub fn clipboard_locked(&self, enabled: bool) -> bool {
        enabled && self.state.load(Ordering::Acquire) == OWNER_UNCONFIRMED
    }
}

pub fn clipboard_locked(state: &DesktopState) -> bool {
    let settings = state.settings.snapshot();
    state
        .presence_access
        .clipboard_locked(settings.clipboard_lock_when_owner_unconfirmed)
}

pub fn require_clipboard_access(state: &DesktopState) -> Result<(), String> {
    if clipboard_locked(state) {
        Err("clipboard is locked; confirm the local user and retry".to_string())
    } else {
        Ok(())
    }
}

/// Reconcile passive capture and network synchronization with the committed
/// settings and current presence authorization. The operating-system clipboard
/// remains available to other applications; this protects ArcRelay history and
/// replication.
pub async fn reconcile_clipboard_runtime(
    app: &AppHandle,
    state: &DesktopState,
) -> Result<(), String> {
    let settings = state.settings.snapshot();
    let locked = state
        .presence_access
        .clipboard_locked(settings.clipboard_lock_when_owner_unconfirmed);
    let mut policy = state.clipboard.policy().await.map_err(|e| e.to_string())?;
    policy.history_enabled = settings.clipboard_enabled && !locked;
    state
        .clipboard
        .update_policy(policy)
        .await
        .map_err(|e| e.to_string())?;
    state
        .clipboard
        .update_sync_preferences(ClipboardSyncPreferences {
            enabled: settings.clipboard_sync_enabled && !locked,
            update_system_clipboard: settings.clipboard_sync_update_system_clipboard && !locked,
            sync_edits_and_deletes: settings.clipboard_sync_edits_and_deletes,
            sync_favorites: settings.clipboard_sync_favorites,
        });

    if locked {
        crate::commands::clipboard_stop_continuous_paste(app.clone()).await;
        crate::commands::clear_clipboard_thumbnail_cache();
        crate::windowing::hide_clipboard_window(app).map_err(|error| error.to_string())?;
    } else if !settings.clipboard_enabled {
        crate::windowing::sync_clipboard_window(app, false).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_is_fail_closed_only_after_enrollment() {
        let controller = PresenceAccessController::default();
        assert!(!controller.clipboard_locked(true));
        controller.update(true, PresenceState::Uncertain);
        assert!(controller.clipboard_locked(true));
        controller.update(true, PresenceState::Absent);
        assert!(controller.clipboard_locked(true));
        controller.update(true, PresenceState::OwnerPresent);
        assert!(!controller.clipboard_locked(true));
        assert!(!controller.clipboard_locked(false));
    }
}
