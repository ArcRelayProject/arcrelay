//! Windows hides the system pointer when no mouse is attached. ShowCursor only
//! changes the calling thread's counter, so it cannot fix other applications.
//! Temporarily enabling MouseKeys supplies the missing system pointer instead.

use std::sync::Mutex;
use std::time::{Duration, Instant};

const MOUSE_KEYS_ON: u32 = 0x01;
const REPLACE_NUMBERS: u32 = 0x80;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MouseKeys {
    flags: u32,
    max_speed: u32,
    time_to_max_speed: u32,
    ctrl_speed: u32,
}

trait Backend {
    fn mouse_present(&self) -> bool;
    fn num_lock_on(&self) -> bool;
    fn read(&self) -> Result<MouseKeys, String>;
    fn write(&self, settings: MouseKeys) -> Result<(), String>;
}

#[derive(Default)]
struct State {
    restore: Option<(MouseKeys, MouseKeys)>,
    retry_after: Option<Instant>,
    num_lock: bool,
}

struct CursorVisibility<B> {
    backend: B,
    state: Mutex<State>,
}

impl<B: Backend> CursorVisibility<B> {
    fn new(backend: B) -> Self {
        Self {
            backend,
            state: Mutex::new(State::default()),
        }
    }

    fn ensure_visible(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.restore.is_some()
            || state.retry_after.is_some_and(|next| Instant::now() < next)
            || self.backend.mouse_present()
        {
            return;
        }
        // Avoid querying or changing accessibility settings on the pointer hot
        // path after a failed attempt, while allowing recovery from lock/UAC.
        state.retry_after = Some(Instant::now() + Duration::from_secs(1));
        let result = (|| {
            let original = self.backend.read()?;
            if original.flags & MOUSE_KEYS_ON != 0 {
                return Ok(());
            }
            let mut applied = original;
            applied.flags |= MOUSE_KEYS_ON;
            // Keep MouseKeys in the opposite NumLock mode so the remote
            // numeric keypad continues to type normally.
            state.num_lock = self.backend.num_lock_on();
            if state.num_lock {
                applied.flags |= REPLACE_NUMBERS;
            } else {
                applied.flags &= !REPLACE_NUMBERS;
            }
            self.backend.write(applied)?;
            state.restore = Some((original, applied));
            tracing::info!(
                event = "input.cursor.mouse_keys_enabled",
                "Arc Input enabled a Windows pointer without an attached mouse"
            );
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            // Cursor repair must not disable otherwise working input.
            tracing::warn!(%error, "Arc Input could not enable the Windows pointer");
        }
    }

    fn num_lock_toggled(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some((original, applied)) = state.restore else {
            return;
        };
        state.num_lock = !state.num_lock;
        let result = (|| {
            if self.backend.read()? != applied {
                // Respect changes made by the user or another application.
                state.restore = None;
                return Ok(());
            }
            let mut updated = applied;
            if state.num_lock {
                updated.flags |= REPLACE_NUMBERS;
            } else {
                updated.flags &= !REPLACE_NUMBERS;
            }
            self.backend.write(updated)?;
            state.restore = Some((original, updated));
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            tracing::warn!(%error, "Arc Input could not update Windows pointer NumLock mode");
        }
    }

    fn release(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.retry_after = None;
        let Some((original, applied)) = state.restore else {
            return;
        };
        let result = (|| {
            if self.backend.read()? == applied {
                self.backend.write(original)?;
            }
            state.restore = None;
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            // Retain the snapshot so stop/Drop can retry a failed restoration.
            tracing::warn!(%error, "Arc Input could not restore Windows MouseKeys settings");
        }
    }
}

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use windows::Win32::UI::Accessibility::MOUSEKEYS;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_NUMLOCK};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SystemParametersInfoW, MKF_LEFTBUTTONDOWN, MKF_LEFTBUTTONSEL,
        MKF_MOUSEMODE, MKF_RIGHTBUTTONDOWN, MKF_RIGHTBUTTONSEL, SM_MOUSEPRESENT, SPIF_SENDCHANGE,
        SPI_GETMOUSEKEYS, SPI_SETMOUSEKEYS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };

    struct Windows;

    impl Backend for Windows {
        fn mouse_present(&self) -> bool {
            unsafe { GetSystemMetrics(SM_MOUSEPRESENT) != 0 }
        }

        fn num_lock_on(&self) -> bool {
            unsafe { GetKeyState(i32::from(VK_NUMLOCK.0)) & 1 != 0 }
        }

        fn read(&self) -> Result<MouseKeys, String> {
            let mut settings = MOUSEKEYS {
                cbSize: std::mem::size_of::<MOUSEKEYS>() as u32,
                ..Default::default()
            };
            unsafe {
                SystemParametersInfoW(
                    SPI_GETMOUSEKEYS,
                    settings.cbSize,
                    Some((&mut settings as *mut MOUSEKEYS).cast()),
                    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                )
            }
            .map_err(|error| error.to_string())?;
            Ok(MouseKeys {
                // These transient button/mode flags are ignored by SET and
                // must not prevent restoring the saved configuration.
                flags: settings.dwFlags
                    & !(MKF_LEFTBUTTONDOWN
                        | MKF_LEFTBUTTONSEL
                        | MKF_MOUSEMODE
                        | MKF_RIGHTBUTTONDOWN
                        | MKF_RIGHTBUTTONSEL),
                max_speed: settings.iMaxSpeed,
                time_to_max_speed: settings.iTimeToMaxSpeed,
                ctrl_speed: settings.iCtrlSpeed,
            })
        }

        fn write(&self, settings: MouseKeys) -> Result<(), String> {
            let mut native = MOUSEKEYS {
                cbSize: std::mem::size_of::<MOUSEKEYS>() as u32,
                dwFlags: settings.flags,
                iMaxSpeed: settings.max_speed,
                iTimeToMaxSpeed: settings.time_to_max_speed,
                iCtrlSpeed: settings.ctrl_speed,
                ..Default::default()
            };
            unsafe {
                SystemParametersInfoW(
                    SPI_SETMOUSEKEYS,
                    native.cbSize,
                    Some((&mut native as *mut MOUSEKEYS).cast()),
                    // Session-only: do not persist this accessibility change.
                    SPIF_SENDCHANGE,
                )
            }
            .map_err(|error| error.to_string())
        }
    }

    pub struct RemoteCursor(CursorVisibility<Windows>);

    impl Default for RemoteCursor {
        fn default() -> Self {
            Self(CursorVisibility::new(Windows))
        }
    }

    impl RemoteCursor {
        pub fn ensure_visible(&self) {
            self.0.ensure_visible();
        }

        pub fn num_lock_toggled(&self) {
            self.0.num_lock_toggled();
        }

        pub fn release(&self) {
            self.0.release();
        }
    }

    impl Drop for RemoteCursor {
        fn drop(&mut self) {
            self.release();
        }
    }

    #[test]
    fn windows_cursor_native_constructs_and_releases_without_tokio() {
        let cursor = RemoteCursor::default();
        cursor.release();
        assert!(cursor.0.state.lock().unwrap().restore.is_none());
    }
}

#[cfg(target_os = "windows")]
pub(super) use native::RemoteCursor;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[derive(Default)]
    struct Fake {
        mouse: Cell<bool>,
        num_lock: Cell<bool>,
        settings: Cell<MouseKeys>,
        writes: Cell<usize>,
        fail_write: Cell<bool>,
        reads: Cell<usize>,
    }

    impl Backend for Fake {
        fn mouse_present(&self) -> bool {
            self.mouse.get()
        }
        fn num_lock_on(&self) -> bool {
            self.num_lock.get()
        }
        fn read(&self) -> Result<MouseKeys, String> {
            self.reads.set(self.reads.get() + 1);
            Ok(self.settings.get())
        }
        fn write(&self, settings: MouseKeys) -> Result<(), String> {
            if self.fail_write.get() {
                return Err("simulated settings failure".into());
            }
            self.writes.set(self.writes.get() + 1);
            self.settings.set(settings);
            Ok(())
        }
    }

    #[test]
    fn windows_cursor_constructs_and_releases_without_tokio_or_system_changes() {
        let cursor = CursorVisibility::new(Fake::default());
        cursor.release();
        assert_eq!(cursor.backend.writes.get(), 0);
        assert_eq!(cursor.backend.reads.get(), 0);
    }

    #[test]
    fn windows_cursor_without_mouse_is_enabled_once_and_restores_all_settings() {
        let cursor = CursorVisibility::new(Fake::default());
        let original = MouseKeys {
            flags: 0x06 | REPLACE_NUMBERS,
            max_speed: 80,
            time_to_max_speed: 2500,
            ctrl_speed: 12,
        };
        cursor.backend.settings.set(original);
        cursor.ensure_visible();
        cursor.ensure_visible();
        assert_eq!(cursor.backend.writes.get(), 1);
        assert_eq!(cursor.backend.settings.get().flags, 0x06 | MOUSE_KEYS_ON);
        cursor.release();
        cursor.release();
        assert_eq!(cursor.backend.settings.get(), original);
        assert_eq!(cursor.backend.writes.get(), 2);
        cursor.ensure_visible();
        cursor.release();
        assert_eq!(cursor.backend.settings.get(), original);
        assert_eq!(cursor.backend.writes.get(), 4);
    }

    #[test]
    fn windows_cursor_handles_mouse_unplugged_after_start_and_leaves_existing_mouse_keys() {
        let cursor = CursorVisibility::new(Fake::default());
        cursor.backend.mouse.set(true);
        cursor.ensure_visible();
        assert_eq!(cursor.backend.writes.get(), 0);
        cursor.backend.mouse.set(false);
        cursor.ensure_visible();
        assert_eq!(cursor.backend.writes.get(), 1);
        cursor.release();

        let enabled = MouseKeys {
            flags: MOUSE_KEYS_ON,
            ..Default::default()
        };
        cursor.backend.settings.set(enabled);
        cursor.ensure_visible();
        cursor.release();
        assert_eq!(cursor.backend.settings.get(), enabled);
        assert_eq!(cursor.backend.writes.get(), 2);
    }

    #[test]
    fn windows_cursor_tracks_num_lock_and_preserves_user_changes_on_release() {
        let cursor = CursorVisibility::new(Fake::default());
        cursor.backend.num_lock.set(true);
        cursor.ensure_visible();
        assert_ne!(cursor.backend.settings.get().flags & REPLACE_NUMBERS, 0);
        cursor.num_lock_toggled();
        assert_eq!(cursor.backend.settings.get().flags & REPLACE_NUMBERS, 0);
        let changed = MouseKeys {
            max_speed: 99,
            ..cursor.backend.settings.get()
        };
        cursor.backend.settings.set(changed);
        cursor.release();
        assert_eq!(cursor.backend.settings.get(), changed);
    }

    #[test]
    fn windows_cursor_failed_enable_has_no_snapshot_and_failed_restore_can_retry() {
        let cursor = CursorVisibility::new(Fake::default());
        cursor.backend.fail_write.set(true);
        cursor.ensure_visible();
        assert!(cursor.state.lock().unwrap().restore.is_none());
        cursor.backend.fail_write.set(false);
        cursor.release();
        cursor.ensure_visible();
        cursor.backend.fail_write.set(true);
        cursor.release();
        assert!(cursor.state.lock().unwrap().restore.is_some());
        cursor.backend.fail_write.set(false);
        cursor.release();
        assert_eq!(cursor.backend.settings.get(), MouseKeys::default());
    }

    #[test]
    fn windows_cursor_num_lock_recovers_after_a_failed_mode_change() {
        let cursor = CursorVisibility::new(Fake::default());
        cursor.ensure_visible();
        cursor.backend.fail_write.set(true);
        cursor.num_lock_toggled();
        cursor.backend.fail_write.set(false);
        cursor.num_lock_toggled();
        assert_eq!(cursor.backend.settings.get().flags & REPLACE_NUMBERS, 0);
        cursor.release();
        assert_eq!(cursor.backend.settings.get(), MouseKeys::default());
    }
}
