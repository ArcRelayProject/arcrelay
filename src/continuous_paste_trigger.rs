use std::sync::atomic::{AtomicBool, Ordering};

use tauri::AppHandle;

static DISPATCHING: AtomicBool = AtomicBool::new(false);

pub fn start(app: AppHandle, fallback_shortcut: &str) -> String {
    stop();
    match platform::start(app) {
        Ok(()) => native_shortcut_label().to_string(),
        Err(error) => {
            tracing::warn!(%error, "Native paste trigger unavailable; using clipboard shortcut");
            display_shortcut(fallback_shortcut)
        }
    }
}

pub fn stop() {
    platform::stop();
    DISPATCHING.store(false, Ordering::Release);
}

fn dispatch_after_modifier_release(app: AppHandle) -> bool {
    if !crate::commands::clipboard_continuous_paste_active()
        || DISPATCHING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return false;
    }

    tauri::async_runtime::spawn(async move {
        // Event hooks run before the target application observes modifier-up.
        // Let the operating system settle its physical modifier state before
        // injecting ArcRelay's tagged paste event.
        tokio::time::sleep(std::time::Duration::from_millis(24)).await;
        let result = crate::commands::paste_next_continuous_record(app.clone()).await;
        DISPATCHING.store(false, Ordering::Release);
        if let Err(error) = result {
            tracing::warn!(%error, "Failed to execute continuous clipboard paste");
            let _ = crate::commands::clipboard_stop_continuous_paste(app.clone()).await;
            let _ = tauri::Emitter::emit(&app, "clipboard-continuous-paste-error", error);
        }
    });
    true
}

fn display_shortcut(value: &str) -> String {
    #[cfg(target_os = "macos")]
    return value
        .replace("CmdOrCtrl", "⌘")
        .replace("Shift", "⇧")
        .replace("Alt", "⌥")
        .replace("Control", "⌃")
        .replace("Ctrl", "⌃")
        .replace('+', "");

    #[cfg(not(target_os = "macos"))]
    value.replace("CmdOrCtrl", "Ctrl")
}

#[cfg(target_os = "macos")]
fn native_shortcut_label() -> &'static str {
    "⌘V"
}

#[cfg(not(target_os = "macos"))]
fn native_shortcut_label() -> &'static str {
    "Ctrl+V"
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Mutex, OnceLock};
    use std::thread::JoinHandle;

    use tauri::AppHandle;

    const KEY_DOWN: u32 = 10;
    const KEY_UP: u32 = 11;
    const FLAGS_CHANGED: u32 = 12;
    const KEYCODE_V: i64 = 0x09;
    const FIELD_KEYCODE: u32 = 9;
    const FIELD_AUTOREPEAT: u32 = 8;
    const FIELD_EVENT_SOURCE_USER_DATA: u32 = 42;
    const COMMAND_FLAG: u64 = 1 << 20;
    const PHYSICAL_MODIFIERS: u64 = (1 << 17) | (1 << 18) | (1 << 19) | (1 << 20);
    const EVENT_MASK: u64 = (1 << KEY_DOWN) | (1 << KEY_UP) | (1 << FLAGS_CHANGED);

    type CGEventRef = *mut c_void;
    type CFMachPortRef = *mut c_void;
    type CFRunLoopRef = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFStringRef = *const c_void;
    type EventTapCallback =
        unsafe extern "C" fn(*const c_void, u32, CGEventRef, *mut c_void) -> CGEventRef;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: EventTapCallback,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
        fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
        fn CGEventGetFlags(event: CGEventRef) -> u64;
        fn CFMachPortCreateRunLoopSource(
            allocator: *const c_void,
            port: CFMachPortRef,
            order: isize,
        ) -> CFRunLoopSourceRef;
        fn CFMachPortInvalidate(port: CFMachPortRef);
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(
            loop_ref: CFRunLoopRef,
            source: CFRunLoopSourceRef,
            mode: CFStringRef,
        );
        fn CFRunLoopRemoveSource(
            loop_ref: CFRunLoopRef,
            source: CFRunLoopSourceRef,
            mode: CFStringRef,
        );
        fn CFRunLoopRun();
        fn CFRunLoopStop(loop_ref: CFRunLoopRef);
        fn CFRelease(value: *const c_void);
        static kCFRunLoopCommonModes: CFStringRef;
    }

    struct Context {
        app: AppHandle,
        pending: AtomicBool,
        suppress_v_up: AtomicBool,
        tap: AtomicUsize,
    }

    struct Handle {
        run_loop: Arc<AtomicUsize>,
        thread: Option<JoinHandle<()>>,
    }

    static HANDLE: OnceLock<Mutex<Option<Handle>>> = OnceLock::new();

    fn handle() -> &'static Mutex<Option<Handle>> {
        HANDLE.get_or_init(|| Mutex::new(None))
    }

    unsafe extern "C" fn callback(
        _proxy: *const c_void,
        event_type: u32,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef {
        if event.is_null() || user_info.is_null() {
            return event;
        }
        if matches!(event_type, 0xffff_fffe | 0xffff_ffff) {
            let context = &*(user_info as *const Context);
            let tap = context.tap.load(Ordering::Acquire) as CFMachPortRef;
            if !tap.is_null() {
                CGEventTapEnable(tap, true);
            }
            return event;
        }
        if CGEventGetIntegerValueField(event, FIELD_EVENT_SOURCE_USER_DATA)
            == arcrelay_core::infrastructure::input_control::macos_system_gesture::EVENT_TAG
        {
            return event;
        }

        let context = &*(user_info as *const Context);
        let keycode = CGEventGetIntegerValueField(event, FIELD_KEYCODE);
        let modifiers = CGEventGetFlags(event) & PHYSICAL_MODIFIERS;

        if event_type == KEY_DOWN && keycode == KEYCODE_V && modifiers == COMMAND_FLAG {
            if !crate::commands::clipboard_continuous_paste_active() {
                return event;
            }
            if CGEventGetIntegerValueField(event, FIELD_AUTOREPEAT) == 0 {
                context.pending.store(true, Ordering::Release);
            }
            context.suppress_v_up.store(true, Ordering::Release);
            return std::ptr::null_mut();
        }

        if event_type == KEY_UP
            && keycode == KEYCODE_V
            && context.suppress_v_up.swap(false, Ordering::AcqRel)
        {
            return std::ptr::null_mut();
        }

        if event_type == FLAGS_CHANGED
            && modifiers & COMMAND_FLAG == 0
            && context.pending.swap(false, Ordering::AcqRel)
        {
            super::dispatch_after_modifier_release(context.app.clone());
        }
        event
    }

    pub fn start(app: AppHandle) -> Result<(), String> {
        let mut active = handle().lock().unwrap_or_else(|error| error.into_inner());
        if active.is_some() {
            return Ok(());
        }
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let run_loop = Arc::new(AtomicUsize::new(0));
        let run_loop_thread = run_loop.clone();
        let thread = std::thread::Builder::new()
            .name("continuous-paste-trigger".into())
            .spawn(move || unsafe {
                let context = Box::into_raw(Box::new(Context {
                    app,
                    pending: AtomicBool::new(false),
                    suppress_v_up: AtomicBool::new(false),
                    tap: AtomicUsize::new(0),
                }));
                let tap = CGEventTapCreate(0, 0, 0, EVENT_MASK, callback, context.cast());
                if tap.is_null() {
                    drop(Box::from_raw(context));
                    let _ = started_tx.send(Err(
                        "macOS paste-key event tap could not be created".to_string()
                    ));
                    return;
                }
                (*context).tap.store(tap as usize, Ordering::Release);
                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
                if source.is_null() {
                    CFMachPortInvalidate(tap);
                    CFRelease(tap);
                    drop(Box::from_raw(context));
                    let _ = started_tx.send(Err(
                        "macOS paste-key run-loop source could not be created".to_string(),
                    ));
                    return;
                }
                let loop_ref = CFRunLoopGetCurrent();
                run_loop_thread.store(loop_ref as usize, Ordering::Release);
                CFRunLoopAddSource(loop_ref, source, kCFRunLoopCommonModes);
                CGEventTapEnable(tap, true);
                let _ = started_tx.send(Ok(()));
                CFRunLoopRun();
                run_loop_thread.store(0, Ordering::Release);
                CGEventTapEnable(tap, false);
                CFRunLoopRemoveSource(loop_ref, source, kCFRunLoopCommonModes);
                CFMachPortInvalidate(tap);
                CFRelease(source);
                CFRelease(tap);
                drop(Box::from_raw(context));
            })
            .map_err(|error| error.to_string())?;

        match started_rx.recv() {
            Ok(Ok(())) => {
                *active = Some(Handle {
                    run_loop,
                    thread: Some(thread),
                });
                Ok(())
            }
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(error) => {
                let _ = thread.join();
                Err(error.to_string())
            }
        }
    }

    pub fn stop() {
        let current = handle()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let Some(mut current) = current else {
            return;
        };
        let loop_ref = current.run_loop.load(Ordering::Acquire) as CFRunLoopRef;
        if !loop_ref.is_null() {
            unsafe { CFRunLoopStop(loop_ref) };
        }
        if let Some(thread) = current.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fallback_shortcut_uses_platform_notation() {
        #[cfg(target_os = "macos")]
        assert_eq!(super::display_shortcut("CmdOrCtrl+Shift+V"), "⌘⇧V");

        #[cfg(not(target_os = "macos"))]
        assert_eq!(super::display_shortcut("CmdOrCtrl+Shift+V"), "Ctrl+Shift+V");
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Mutex, OnceLock};
    use std::thread::JoinHandle;

    use tauri::AppHandle;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU,
        VK_RSHIFT, VK_RWIN, VK_SHIFT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
        WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    const VK_V: u32 = 0x56;
    const MOD_CONTROL: usize = 1 << 0;
    const MOD_SHIFT: usize = 1 << 1;
    const MOD_ALT: usize = 1 << 2;
    const MOD_WIN: usize = 1 << 3;

    struct Context {
        app: AppHandle,
        modifiers: AtomicUsize,
        pending: AtomicBool,
        suppress_v_up: AtomicBool,
    }

    struct Handle {
        thread_id: u32,
        thread: Option<JoinHandle<()>>,
    }

    static CONTEXT: OnceLock<Mutex<Option<Arc<Context>>>> = OnceLock::new();
    static HANDLE: OnceLock<Mutex<Option<Handle>>> = OnceLock::new();

    fn context() -> &'static Mutex<Option<Arc<Context>>> {
        CONTEXT.get_or_init(|| Mutex::new(None))
    }

    fn handle() -> &'static Mutex<Option<Handle>> {
        HANDLE.get_or_init(|| Mutex::new(None))
    }

    fn modifier_bit(vk: u32) -> usize {
        match vk {
            value
                if value == VK_CONTROL.0 as u32
                    || value == VK_LCONTROL.0 as u32
                    || value == VK_RCONTROL.0 as u32 =>
            {
                MOD_CONTROL
            }
            value
                if value == VK_SHIFT.0 as u32
                    || value == VK_LSHIFT.0 as u32
                    || value == VK_RSHIFT.0 as u32 =>
            {
                MOD_SHIFT
            }
            value
                if value == VK_MENU.0 as u32
                    || value == VK_LMENU.0 as u32
                    || value == VK_RMENU.0 as u32 =>
            {
                MOD_ALT
            }
            value if value == VK_LWIN.0 as u32 || value == VK_RWIN.0 as u32 => MOD_WIN,
            _ => 0,
        }
    }

    unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code != HC_ACTION as i32 {
            return CallNextHookEx(None, code, wparam, lparam);
        }
        let event = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if event.flags.contains(LLKHF_INJECTED) {
            return CallNextHookEx(None, code, wparam, lparam);
        }
        let active = context()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let Some(active) = active else {
            return CallNextHookEx(None, code, wparam, lparam);
        };
        let message = wparam.0 as u32;
        let down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
        let up = matches!(message, WM_KEYUP | WM_SYSKEYUP);
        if !down && !up {
            return CallNextHookEx(None, code, wparam, lparam);
        }

        let bit = modifier_bit(event.vkCode);
        if bit != 0 {
            if down {
                active.modifiers.fetch_or(bit, Ordering::AcqRel);
            } else {
                active.modifiers.fetch_and(!bit, Ordering::AcqRel);
                if bit == MOD_CONTROL && active.pending.swap(false, Ordering::AcqRel) {
                    super::dispatch_after_modifier_release(active.app.clone());
                }
            }
            return CallNextHookEx(None, code, wparam, lparam);
        }

        if event.vkCode == VK_V
            && down
            && active.modifiers.load(Ordering::Acquire) == MOD_CONTROL
            && crate::commands::clipboard_continuous_paste_active()
        {
            active.pending.store(true, Ordering::Release);
            active.suppress_v_up.store(true, Ordering::Release);
            return LRESULT(1);
        }
        if event.vkCode == VK_V && up && active.suppress_v_up.swap(false, Ordering::AcqRel) {
            return LRESULT(1);
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    pub fn start(app: AppHandle) -> Result<(), String> {
        let mut active_handle = handle().lock().unwrap_or_else(|error| error.into_inner());
        if active_handle.is_some() {
            return Ok(());
        }
        let active_context = Arc::new(Context {
            app,
            modifiers: AtomicUsize::new(0),
            pending: AtomicBool::new(false),
            suppress_v_up: AtomicBool::new(false),
        });
        *context().lock().unwrap_or_else(|error| error.into_inner()) = Some(active_context);
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("continuous-paste-trigger".into())
            .spawn(move || unsafe {
                let thread_id = GetCurrentThreadId();
                let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), None, 0) {
                    Ok(hook) => hook,
                    Err(error) => {
                        let _ = started_tx.send(Err(error.to_string()));
                        return;
                    }
                };
                let _ = started_tx.send(Ok(thread_id));
                let mut message = MSG::default();
                while GetMessageW(&mut message, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                let _ = UnhookWindowsHookEx(hook);
            })
            .map_err(|error| error.to_string())?;
        match started_rx.recv() {
            Ok(Ok(thread_id)) => {
                *active_handle = Some(Handle {
                    thread_id,
                    thread: Some(thread),
                });
                Ok(())
            }
            Ok(Err(error)) => {
                *context().lock().unwrap_or_else(|value| value.into_inner()) = None;
                let _ = thread.join();
                Err(error)
            }
            Err(error) => {
                *context().lock().unwrap_or_else(|value| value.into_inner()) = None;
                let _ = thread.join();
                Err(error.to_string())
            }
        }
    }

    pub fn stop() {
        *context().lock().unwrap_or_else(|error| error.into_inner()) = None;
        let current = handle()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let Some(mut current) = current else {
            return;
        };
        unsafe {
            let _ = PostThreadMessageW(current.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(thread) = current.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use tauri::AppHandle;

    pub fn start(_app: AppHandle) -> Result<(), String> {
        Err("native paste-key interception is unavailable on this Linux session".to_string())
    }

    pub fn stop() {}
}
