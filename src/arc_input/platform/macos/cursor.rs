//! Background cursor ownership for the duration of an outgoing input route.
//! Public CGDisplayHideCursor normally only affects the foreground application.
//! Resolve the WindowServer connection property at runtime; never activate an app.

use super::*;
use core_foundation::{base::TCFType, boolean::CFBoolean, string::CFString};
use std::sync::OnceLock;

type Connection = unsafe extern "C" fn() -> i32;
type SetProperty = unsafe extern "C" fn(i32, i32, *const c_void, *const c_void) -> i32;
type IsVisible = unsafe extern "C" fn() -> u32;

struct BackgroundApi {
    connection: Connection,
    set_property: SetProperty,
    is_visible: IsVisible,
}

impl BackgroundApi {
    fn resolve() -> Option<Self> {
        unsafe {
            let connection = libc::dlsym(libc::RTLD_DEFAULT, c"CGSMainConnectionID".as_ptr());
            let set_property =
                libc::dlsym(libc::RTLD_DEFAULT, c"CGSSetConnectionProperty".as_ptr());
            let is_visible = libc::dlsym(libc::RTLD_DEFAULT, c"CGCursorIsVisible".as_ptr());
            if connection.is_null() || set_property.is_null() || is_visible.is_null() {
                return None;
            }
            Some(Self {
                connection: std::mem::transmute::<*mut c_void, Connection>(connection),
                set_property: std::mem::transmute::<*mut c_void, SetProperty>(set_property),
                is_visible: std::mem::transmute::<*mut c_void, IsVisible>(is_visible),
            })
        }
    }
}

fn api() -> Option<&'static BackgroundApi> {
    static API: OnceLock<Option<BackgroundApi>> = OnceLock::new();
    API.get_or_init(BackgroundApi::resolve).as_ref()
}

trait CursorBackend {
    fn prepare(&self) -> Result<(), PlatformError>;
    fn visible(&self) -> bool;
    fn hide(&self) -> Result<(), PlatformError>;
    fn show(&self) -> Result<(), PlatformError>;
}

struct Quartz;
impl CursorBackend for Quartz {
    fn prepare(&self) -> Result<(), PlatformError> {
        let api = api().ok_or_else(|| {
            PlatformError::Unsupported("macOS background cursor control is unavailable".into())
        })?;
        let key = CFString::new("SetsCursorInBackground");
        let enabled = CFBoolean::true_value();
        let result = unsafe {
            let connection = (api.connection)();
            (api.set_property)(
                connection,
                connection,
                key.as_CFTypeRef(),
                enabled.as_CFTypeRef(),
            )
        };
        quartz_result(result, "enable background cursor control")
    }

    fn visible(&self) -> bool {
        api().is_none_or(|api| unsafe { (api.is_visible)() != 0 })
    }
    fn hide(&self) -> Result<(), PlatformError> {
        CGDisplay::main()
            .hide_cursor()
            .map_err(|error| cursor_error(error, "hide cursor"))
    }
    fn show(&self) -> Result<(), PlatformError> {
        CGDisplay::main()
            .show_cursor()
            .map_err(|error| cursor_error(error, "show cursor"))
    }
}

fn cursor_error(error: i32, operation: &str) -> PlatformError {
    PlatformError::Operation(format!("cannot {operation}: CoreGraphics error {error}"))
}
fn quartz_result(error: i32, operation: &str) -> Result<(), PlatformError> {
    if error == 0 {
        Ok(())
    } else {
        Err(cursor_error(error, operation))
    }
}

/// Own at most one hide-count increment, even if the system reveals the cursor
/// after focus/Space changes. Release our old increment before reasserting it.
fn update(
    current: &mut bool,
    hidden: bool,
    backend: &impl CursorBackend,
) -> Result<bool, PlatformError> {
    if !hidden {
        if !*current {
            return Ok(false);
        }
        // Restoration must still run if background setup fails after a hide.
        let prepared = backend.prepare();
        backend.show()?;
        *current = false;
        prepared?;
        return Ok(true);
    }
    if *current && !backend.visible() {
        return Ok(false);
    }
    backend.prepare()?;
    if *current {
        backend.show()?;
        *current = false;
    }
    backend.hide()?;
    *current = true;
    Ok(true)
}

pub(super) fn set_hidden(state: &Mutex<bool>, hidden: bool) -> Result<(), PlatformError> {
    let mut current = lock(state);
    if update(&mut current, hidden, &Quartz)? {
        tracing::info!(
            event = "input.cursor.visibility",
            hidden,
            visible = Quartz.visible(),
            "Arc Input updated local cursor visibility"
        );
    }
    Ok(())
}

pub(super) fn maintain_hidden(state: &Mutex<bool>) -> Result<(), PlatformError> {
    let mut current = lock(state);
    if *current && update(&mut current, true, &Quartz)? {
        tracing::debug!(
            event = "input.cursor.rehidden",
            visible = Quartz.visible(),
            "Arc Input restored background cursor hiding"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct Fake {
        visible: Cell<bool>,
        fail_prepare: Cell<bool>,
        fail_hide: Cell<bool>,
        count: Cell<i32>,
        calls: RefCell<Vec<&'static str>>,
    }
    impl CursorBackend for Fake {
        fn prepare(&self) -> Result<(), PlatformError> {
            self.calls.borrow_mut().push("prepare");
            quartz_result(i32::from(self.fail_prepare.get()), "prepare")
        }
        fn visible(&self) -> bool {
            self.visible.get()
        }
        fn hide(&self) -> Result<(), PlatformError> {
            self.calls.borrow_mut().push("hide");
            quartz_result(i32::from(self.fail_hide.get()), "hide")?;
            self.count.set(self.count.get() + 1);
            self.visible.set(false);
            Ok(())
        }
        fn show(&self) -> Result<(), PlatformError> {
            self.calls.borrow_mut().push("show");
            self.count.set(self.count.get() - 1);
            self.visible.set(true);
            Ok(())
        }
    }

    #[test]
    fn background_hide_is_idempotent_and_focus_repairs_never_leak_hide_counts() {
        let backend = Fake::default();
        let mut hidden = false;
        assert!(update(&mut hidden, true, &backend).unwrap());
        assert_eq!(*backend.calls.borrow(), vec!["prepare", "hide"]);
        for _ in 0..10 {
            assert!(!update(&mut hidden, true, &backend).unwrap());
            backend.visible.set(true); // application/Space changed
            assert!(update(&mut hidden, true, &backend).unwrap());
            assert_eq!(backend.count.get(), 1);
        }
        assert!(update(&mut hidden, false, &backend).unwrap());
        assert!(!update(&mut hidden, false, &backend).unwrap());
        assert_eq!(backend.count.get(), 0);
        assert!(backend.visible.get());
    }

    #[test]
    fn errors_do_not_claim_a_hide_and_restoration_still_balances_our_count() {
        let backend = Fake::default();
        let mut hidden = false;
        backend.fail_prepare.set(true);
        assert!(update(&mut hidden, true, &backend).is_err());
        assert!(!hidden);
        backend.fail_prepare.set(false);
        update(&mut hidden, true, &backend).unwrap();
        backend.fail_prepare.set(true);
        assert!(update(&mut hidden, false, &backend).is_err());
        assert!(!hidden);
        assert_eq!(backend.count.get(), 0);
        backend.fail_prepare.set(false);
        update(&mut hidden, true, &backend).unwrap();
        backend.visible.set(true);
        backend.fail_hide.set(true);
        assert!(update(&mut hidden, true, &backend).is_err());
        assert!(!hidden);
        assert_eq!(backend.count.get(), 0);
    }

    #[test]
    fn resolve_cursor_api_without_changing_visibility_or_needing_tokio() {
        // Lookup only: no property changes, hide/show, or pointer movement.
        if gesture::supported() {
            assert!(BackgroundApi::resolve().is_some());
        }
    }
}
